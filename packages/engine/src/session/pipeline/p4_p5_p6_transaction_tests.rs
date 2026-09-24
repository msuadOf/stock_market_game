use super::p4_continuous::{
    process_continuous_stock, ContinuousEnvelopeSnapshot, ContinuousPlaceFact, ContinuousStockInput,
};
use super::p4_p5_p6_transaction::{apply_p4_p5_p6_transaction, P4P5P6TransactionError};
use super::p6_transaction::P6TransactionError;
use super::retail_projection::{RetailProjectionError, RetailProjectionSeen};
use super::*;
use crate::session::account_book::AccountBook;
use crate::{
    Account, AccountId, AccountKind, GameConfig, Intent, Market, Money, Order, OrderId,
    SecurityCategory, Side, StockCode, TradingPhase,
};
use std::collections::BTreeMap;

fn code() -> StockCode {
    StockCode("600888".to_owned())
}

fn empty_market(code: &StockCode) -> Market {
    Market::new(
        code.clone(),
        Money::from_cents(1_000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap()
}

fn p3_buy_operations(code: &StockCode) -> (Vec<P3ValidatedOperation>, GameConfig) {
    p3_buy_operations_for_quantities(code, &[100])
}

fn p3_buy_operations_for_quantities(
    code: &StockCode,
    quantities: &[u32],
) -> (Vec<P3ValidatedOperation>, GameConfig) {
    let account = AccountId(0);
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(
        quantities
            .iter()
            .enumerate()
            .map(|(index, qty)| {
                P2Candidate::new(
                    P2CandidateKey::player(u64::try_from(index).unwrap()),
                    account,
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: Money::from_cents(1_000),
                        qty: *qty,
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                SecurityCategory::MainBoard,
                Money::from_cents(1_100),
                Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let config = game.setup.config.clone();
    let validation = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();
    assert_eq!(validation.rejected().count(), 0);
    (validation.operations().to_vec(), config)
}

fn resting_sell_snapshot(market: &mut Market, code: &StockCode) -> ContinuousEnvelopeSnapshot {
    resting_sell_snapshot_with_qty(market, code, 100)
}

fn resting_sell_snapshot_with_qty(
    market: &mut Market,
    code: &StockCode,
    qty: u32,
) -> ContinuousEnvelopeSnapshot {
    let order = Order {
        id: OrderId(100),
        side: Side::Sell,
        price: Money::from_cents(1_000),
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: AccountId(0),
        seq: 0,
    };
    let placed = market.place(order).unwrap();
    assert!(placed.trades.is_empty());
    assert!(placed.resting.is_some());
    let audit = EnvelopeAudit {
        limit: Money::from_cents(1_000),
        remaining_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    };
    ContinuousEnvelopeSnapshot {
        envelope: Envelope::tick_start_existing(
            EnvelopeKey {
                account: AccountId(0),
                stock: code.clone(),
                order: OrderId(100),
                side: Side::Sell,
            },
            Money::ZERO,
            qty,
            audit,
        ),
        audit,
    }
}

fn accounts_with_position(code: &StockCode) -> AccountBook {
    let mut account = Account::new(
        AccountId(0),
        AccountKind::Player,
        Money::from_cents(300_000),
    );
    account
        .grant_position(code.clone(), 100, Money::from_cents(1_000))
        .unwrap();
    BTreeMap::from([(AccountId(0), account)]).into()
}

#[test]
fn inverse_identity_fills_keep_maker_receipt_chain_and_settle_both_trades() {
    let code = code();
    let (mut operations, config) = p3_buy_operations_for_quantities(&code, &[100, 100]);
    operations.swap(0, 1);
    let first_identity = operations[0].sealed_index();
    let second_identity = operations[1].sealed_index();
    assert!(first_identity > second_identity);
    let mut market = empty_market(&code);
    let maker = resting_sell_snapshot_with_qty(&mut market, &code, 200);
    let worker = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations,
        config,
    })
    .unwrap();
    let maker_key = maker.envelope.key().clone();
    let accounts = {
        let mut account = Account::new(
            AccountId(0),
            AccountKind::Player,
            Money::from_cents(300_000),
        );
        account
            .grant_position(code.clone(), 200, Money::from_cents(1_000))
            .unwrap();
        BTreeMap::from([(AccountId(0), account)]).into()
    };
    let committed = apply_p4_p5_p6_transaction(
        &EnvelopeLedger::new(0, [maker.envelope]).unwrap(),
        &accounts,
        &crate::session::account_paged_map::AccountPagedMap::default(),
        &RetailProjectionSeen::default(),
        10,
        vec![worker],
        true,
    )
    .unwrap();
    let maker_receipts = committed
        .receipts
        .iter()
        .filter(|receipt| receipt.envelope == maker_key)
        .collect::<Vec<_>>();
    assert_eq!(maker_receipts.len(), 2);
    assert_eq!(
        maker_receipts[0].local_key.source(),
        ReceiptSource::SealedIntent(first_identity)
    );
    assert_eq!(
        maker_receipts[1].local_key.source(),
        ReceiptSource::SealedIntent(second_identity)
    );
    assert_eq!(maker_receipts[0].qty_before, 200);
    assert_eq!(maker_receipts[0].qty_after, 100);
    assert_eq!(maker_receipts[1].qty_before, 100);
    assert_eq!(maker_receipts[1].qty_after, 0);
    assert_eq!(committed.p6.settlement.applied_receipts, 4);
    assert_eq!(committed.ledger.terminal_count(), 3);
    assert_eq!(
        committed.account_patch[&AccountId(0)].positions[&code].t1_locked,
        200
    );
}

#[test]
fn inverse_identity_partial_fill_then_cancel_keeps_maker_audit_and_t1() {
    let code = code();
    let (operations, config) = p3_buy_operations_for_quantities(&code, &[100, 100]);
    let mut market = empty_market(&code);
    let maker = resting_sell_snapshot_with_qty(&mut market, &code, 200);
    let worker = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations: vec![
            operations[1].clone(),
            P3ValidatedOperation::Cancel {
                candidate_key: P2CandidateKey::player(0),
                sealed_index: 0,
                account: AccountId(0),
                code: code.clone(),
                order_id: OrderId(100),
            },
        ],
        config,
    })
    .unwrap();
    let maker_key = maker.envelope.key().clone();
    let mut account = Account::new(
        AccountId(0),
        AccountKind::Player,
        Money::from_cents(300_000),
    );
    account
        .grant_position(code.clone(), 200, Money::from_cents(1_000))
        .unwrap();
    let accounts: AccountBook = BTreeMap::from([(AccountId(0), account)]).into();
    let committed = apply_p4_p5_p6_transaction(
        &EnvelopeLedger::new(0, [maker.envelope]).unwrap(),
        &accounts,
        &crate::session::account_paged_map::AccountPagedMap::default(),
        &RetailProjectionSeen::default(),
        10,
        vec![worker],
        true,
    )
    .unwrap();
    let maker_receipts = committed
        .receipts
        .iter()
        .filter(|receipt| receipt.envelope == maker_key)
        .collect::<Vec<_>>();
    assert_eq!(maker_receipts.len(), 2);
    assert_eq!(maker_receipts[0].kind, ReceiptKind::Fill);
    assert_eq!(maker_receipts[0].qty_before, 200);
    assert_eq!(maker_receipts[0].qty_after, 100);
    assert_eq!(maker_receipts[1].kind, ReceiptKind::Release);
    assert_eq!(maker_receipts[1].qty_before, 100);
    assert_eq!(maker_receipts[1].qty_after, 100);
    assert_eq!(committed.receipts.len(), 3);
    assert_eq!(committed.p6.settlement.applied_receipts, 2);
    assert_eq!(
        committed.account_patch[&AccountId(0)].positions[&code].t1_locked,
        100
    );
    assert_eq!(committed.ledger.terminal_count(), 2);
}

#[test]
fn non_crossing_place_keeps_a_live_order_and_escrow_without_settlement() {
    let code = code();
    let (operations, config) = p3_buy_operations(&code);
    let worker = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations,
        config,
    })
    .unwrap();
    let accounts = accounts_with_position(&code);

    let committed = apply_p4_p5_p6_transaction(
        &EnvelopeLedger::new(0, []).unwrap(),
        &accounts,
        &crate::session::account_paged_map::AccountPagedMap::<crate::RetailExperienceState>::default(),
        &RetailProjectionSeen::default(),
        10,
        vec![worker],
        true,
    )
    .unwrap();

    let stock = committed.stocks.get(&code).unwrap();
    assert_eq!(stock.market.resting_orders().len(), 1);
    assert!(matches!(
        stock.place_facts.as_slice(),
        [ContinuousPlaceFact::Resting {
            remaining_qty: 100,
            ..
        }]
    ));
    assert_eq!(committed.ledger.iter().count(), 1);
    assert!(committed.ledger.iter().next().unwrap().1.live().cash > Money::ZERO);
    assert!(committed.receipts.is_empty());
    assert_eq!(committed.p6.settlement.applied_receipts, 0);
    assert!(committed.account_patch.is_empty());
    assert_eq!(accounts[&AccountId(0)].cash, Money::from_cents(300_000));
}

#[test]
fn crossing_buy_chains_receipts_and_settles_self_cross_once_buy_before_sell() {
    let code = code();
    let (operations, config) = p3_buy_operations(&code);
    let mut market = empty_market(&code);
    let maker = resting_sell_snapshot(&mut market, &code);
    let worker = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations,
        config,
    })
    .unwrap();
    let initial_ledger = EnvelopeLedger::new(0, [maker.envelope.clone()]).unwrap();
    let accounts = accounts_with_position(&code);

    let committed = apply_p4_p5_p6_transaction(
        &initial_ledger,
        &accounts,
        &crate::session::account_paged_map::AccountPagedMap::<crate::RetailExperienceState>::default(),
        &RetailProjectionSeen::default(),
        10,
        vec![worker],
        true,
    )
    .unwrap();

    assert_eq!(committed.receipts.len(), 2);
    assert_eq!(committed.receipts[0].index, 0);
    assert_eq!(committed.receipts[1].index, 1);
    assert!(committed
        .receipts
        .iter()
        .all(|receipt| receipt.kind == ReceiptKind::Fill));
    assert_eq!(committed.ledger.iter().count(), 0);
    assert_eq!(committed.ledger.terminal_count(), 2);
    assert!(committed.stocks[&code].market.resting_orders().is_empty());
    assert_eq!(committed.p6.settlement.applied_receipts, 2);
    assert_eq!(committed.p6.settlement.applied_groups, 2);
    let position = &committed.account_patch[&AccountId(0)].positions[&code];
    assert_eq!(position.qty, 100, "P6 applies the buy before the sell");
    assert_eq!(position.t1_locked, 100);
    assert_ne!(
        committed.account_patch[&AccountId(0)].cash,
        accounts[&AccountId(0)].cash
    );
}

#[test]
fn p6_projection_failure_after_p5_receipts_keeps_every_input_authority_unchanged() {
    let code = code();
    let (operations, config) = p3_buy_operations(&code);
    let mut market = empty_market(&code);
    let maker = resting_sell_snapshot(&mut market, &code);
    let worker = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations,
        config,
    })
    .unwrap();
    assert_eq!(
        worker.receipts.len(),
        2,
        "the crossing worker supplies P5 fills"
    );
    let initial_ledger = EnvelopeLedger::new(41, [maker.envelope.clone()]).unwrap();
    let ledger_before = initial_ledger.clone();
    let live_before = initial_ledger.get(maker.envelope.key()).unwrap().live();
    let mut accounts = accounts_with_position(&code);
    accounts.get_mut(&AccountId(0)).unwrap().kind = AccountKind::Retail;
    let cash_before = accounts[&AccountId(0)].cash;
    let position_before = &accounts[&AccountId(0)].positions[&code];
    let position_before = (
        position_before.qty,
        position_before.t1_locked,
        position_before.invested_cents,
        position_before.recovered_cents,
    );
    let retail =
        crate::session::account_paged_map::AccountPagedMap::<crate::RetailExperienceState>::default(
        );
    let retail_before = retail.clone();
    let seen = RetailProjectionSeen::default();
    let seen_before = seen.clone();

    let result = apply_p4_p5_p6_transaction(
        &initial_ledger,
        &accounts,
        &retail,
        &seen,
        10,
        vec![worker],
        true,
    );

    assert!(matches!(
        result,
        Err(P4P5P6TransactionError::P6(P6TransactionError::Projection(
            RetailProjectionError::MissingRetailExperience {
                account: AccountId(0)
            }
        )))
    ));
    assert_eq!(initial_ledger, ledger_before);
    assert_eq!(initial_ledger.next_receipt_index(), 41);
    assert_eq!(
        initial_ledger.get(maker.envelope.key()).unwrap().live(),
        live_before
    );
    assert_eq!(initial_ledger.terminal_count(), 0);
    assert_eq!(accounts[&AccountId(0)].cash, cash_before);
    assert_eq!(accounts[&AccountId(0)].positions.len(), 1);
    let position_after = &accounts[&AccountId(0)].positions[&code];
    assert_eq!(
        (
            position_after.qty,
            position_after.t1_locked,
            position_after.invested_cents,
            position_after.recovered_cents,
        ),
        position_before
    );
    assert_eq!(retail, retail_before);
    assert_eq!(seen, seen_before);
}

#[test]
fn duplicate_stock_worker_outputs_are_rejected_before_p5_or_p6() {
    let code = code();
    let (first_operations, config) = p3_buy_operations(&code);
    let (second_operations, _) = p3_buy_operations(&code);
    let first = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations: first_operations,
        config: config.clone(),
    })
    .unwrap();
    let second = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations: second_operations,
        config,
    })
    .unwrap();
    let accounts = accounts_with_position(&code);

    let result = apply_p4_p5_p6_transaction(
        &EnvelopeLedger::new(0, []).unwrap(),
        &accounts,
        &crate::session::account_paged_map::AccountPagedMap::<crate::RetailExperienceState>::default(),
        &RetailProjectionSeen::default(),
        10,
        vec![first, second],
        true,
    );

    assert!(matches!(
        result,
        Err(P4P5P6TransactionError::DuplicateStockWorker { code: duplicate }) if duplicate == code
    ));
}
