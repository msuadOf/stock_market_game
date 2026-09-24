use super::b1_continuous_transaction::{
    apply_tick_shadow_b1_continuous_transaction, prepare_b1_continuous_tick,
};
use super::*;
use crate::session::RetailOrderDiagnosticEvent;
use crate::{
    Account, AccountId, AccountKind, Event, Intent, Money, Order, OrderId, RetailExperienceState,
    Side, StockCode,
};

const PLAYER: AccountId = AccountId(0);
const SELLER: AccountId = AccountId(1);
const SELL_ORDER: OrderId = OrderId(1);
const FIRST_BUY_ORDER: OrderId = OrderId(2);
const SECOND_BUY_ORDER: OrderId = OrderId(3);
const PRICE_CENTS: i64 = 1_000;
const LOT: u32 = 100;

#[test]
fn crossing_buy_commits_exact_trade_when_t1_locks_the_new_position() {
    run_single_trade_acceptance(true, LOT, 0);
}

#[test]
fn crossing_buy_commits_exact_trade_when_the_disabled_t1_test_seam_keeps_it_sellable() {
    run_single_trade_acceptance(false, 0, LOT);
}

#[test]
fn two_player_inputs_keep_fifo_identity_through_fills_events_and_p9_rebase() {
    let (mut authority, code) = session_with_resting_sell(true, LOT * 2);
    for _ in 0..2 {
        enqueue_crossing_buy(&mut authority, &code);
    }
    let authority_before = authority.business_state_hash().unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output = apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();

    assert_eq!(authority.business_state_hash().unwrap(), authority_before);
    assert_eq!(authority.pending_player.len(), 2);
    assert_eq!(
        output
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0), P2CandidateKey::player(1)]
    );
    assert_eq!(
        output.validation.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0), P2CandidateKey::player(1)]
    );
    assert_eq!(output.receipts.len(), 4);
    assert_eq!(
        output
            .receipts
            .iter()
            .map(|receipt| receipt.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    let buyer_receipts = output
        .receipts
        .iter()
        .filter(|receipt| receipt.envelope.account == PLAYER)
        .map(|receipt| (receipt.envelope.order, receipt.local_key.source()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        buyer_receipts,
        std::collections::BTreeSet::from([
            (FIRST_BUY_ORDER, ReceiptSource::SealedIntent(0)),
            (SECOND_BUY_ORDER, ReceiptSource::SealedIntent(1)),
        ])
    );
    let seller_receipts = output
        .receipts
        .iter()
        .filter(|receipt| receipt.envelope.account == SELLER)
        .map(|receipt| {
            (
                receipt.envelope.order,
                receipt.local_key.source(),
                receipt.qty_before,
                receipt.qty_after,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        seller_receipts,
        vec![
            (SELL_ORDER, ReceiptSource::SealedIntent(0), LOT * 2, LOT),
            (SELL_ORDER, ReceiptSource::SealedIntent(1), LOT, 0),
        ]
    );
    assert_eq!(output.p6.settlement.applied_receipts, 4);
    assert_eq!(output.p6.settlement.applied_groups, 2);
    assert!(matches!(
        output.events.as_slice(),
        [
            Event::Trade {
                seq: 1,
                code: first_code,
                price: first_price,
                qty: LOT,
                maker: SELLER,
                taker: PLAYER,
            },
            Event::Trade {
                seq: 2,
                code: second_code,
                price: second_price,
                qty: LOT,
                maker: SELLER,
                taker: PLAYER,
            },
            Event::PriceTick { seq: 3, tick: 1, code: price_code, daily_candle, .. },
        ] if first_code == &code
            && second_code == &code
            && *first_price == Money::from_cents(PRICE_CENTS)
            && *second_price == Money::from_cents(PRICE_CENTS)
            && price_code == &code && daily_candle.volume == u64::from(LOT * 2)
    ));

    let committed =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
            .unwrap()
            .commit();

    assert_eq!(committed.tick.events, output.events);
    assert!(authority.pending_player.is_empty());
    assert_eq!(authority.next_order_id, 4);
    assert_eq!(authority.seq(), 3);
    assert_eq!(authority.next_receipt_base, 4);
    assert_eq!(authority.envelope_ledger.next_receipt_index(), 4);
    assert_eq!(authority.envelope_ledger.iter().count(), 0);
    assert_eq!(authority.envelope_ledger.terminal_count(), 0);
    assert!(authority.markets[&code].resting_orders().is_empty());
    assert_eq!(authority.accounts[&PLAYER].positions[&code].qty, LOT * 2);
    assert_eq!(authority.accounts[&PLAYER].sellable_qty(&code), 0);
    assert!(!authority.accounts[&SELLER].positions.contains_key(&code));
    assert_eq!(authority.accounts[&SELLER].cash, Money::from_cents(199_398));
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.receipt.business_hash()
    );
    assert_eq!(
        authority.session_state_hash().unwrap(),
        committed.receipt.session_hash()
    );
    assert_eq!(committed.receipt.next_receipt_base(), 4);
}

#[test]
fn b1_projects_retail_submission_and_bilateral_fills_from_typed_facts() {
    let (mut authority, code) = session_with_resting_sell(true, LOT);
    authority
        .retail_experience
        .insert(PLAYER, RetailExperienceState::without_equity_reference());
    authority
        .retail_experience
        .insert(SELLER, RetailExperienceState::without_equity_reference());
    enqueue_crossing_buy(&mut authority, &code);

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert!(matches!(
        authority.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Submitted {
                account: PLAYER,
                code: submitted_code,
                side: Side::Buy,
                order_id: FIRST_BUY_ORDER,
                qty: LOT,
            },
            RetailOrderDiagnosticEvent::Filled {
                account: PLAYER,
                code: buyer_code,
                side: Side::Buy,
                order_id: FIRST_BUY_ORDER,
                qty: LOT,
            },
            RetailOrderDiagnosticEvent::Filled {
                account: SELLER,
                code: seller_code,
                side: Side::Sell,
                order_id: SELL_ORDER,
                qty: LOT,
            },
        ] if submitted_code == &code && buyer_code == &code && seller_code == &code
    ));
}

#[test]
fn b1_projects_each_retail_fill_with_its_own_request() {
    let (mut authority, code) = session_with_resting_sell(true, LOT * 2);
    for account in [PLAYER, SELLER] {
        authority
            .retail_experience
            .insert(account, RetailExperienceState::without_equity_reference());
    }
    enqueue_crossing_buy(&mut authority, &code);
    enqueue_crossing_buy(&mut authority, &code);

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert!(matches!(
        authority.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Submitted {
                order_id: FIRST_BUY_ORDER,
                ..
            },
            RetailOrderDiagnosticEvent::Filled {
                account: PLAYER,
                order_id: FIRST_BUY_ORDER,
                qty: LOT,
                ..
            },
            RetailOrderDiagnosticEvent::Filled {
                account: SELLER,
                order_id: SELL_ORDER,
                qty: LOT,
                ..
            },
            RetailOrderDiagnosticEvent::Submitted {
                order_id: SECOND_BUY_ORDER,
                ..
            },
            RetailOrderDiagnosticEvent::Filled {
                account: PLAYER,
                order_id: SECOND_BUY_ORDER,
                qty: LOT,
                ..
            },
            RetailOrderDiagnosticEvent::Filled {
                account: SELLER,
                order_id: SELL_ORDER,
                qty: LOT,
                ..
            },
        ]
    ));
}

#[test]
fn b1_market_remainder_is_not_reported_as_an_auction_abort() {
    let (mut authority, code) = session_with_resting_sell(true, LOT);
    authority
        .retail_experience
        .insert(PLAYER, RetailExperienceState::without_equity_reference());
    authority
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: LOT * 2,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert!(authority
        .last_retail_order_events()
        .iter()
        .any(|event| matches!(
            event,
            RetailOrderDiagnosticEvent::Submitted {
                account: PLAYER,
                order_id: FIRST_BUY_ORDER,
                qty,
                ..
            } if *qty == LOT * 2
        )));
    assert!(!authority
        .last_retail_order_events()
        .iter()
        .any(|event| matches!(event, RetailOrderDiagnosticEvent::Aborted { .. })));
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn b1_crossing_trade_has_a_complete_causal_chain() {
    use crate::diagnostics::causal::{CausalFactKind, Termination};

    let (mut authority, code) = session_with_resting_sell(true, LOT);
    // The fixture installs the maker directly. Seed its genuine pre-existing origin so the
    // diagnostic report can reconcile both sides of the B1 execution.
    authority.causal_submitted(&authority.markets[&code].resting_orders()[0], &code);
    authority
        .retail_experience
        .insert(PLAYER, RetailExperienceState::without_equity_reference());
    authority
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: LOT * 2,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    let report = authority.causal_diagnostics().unwrap();
    assert_eq!(report.submitted_qty, u64::from(LOT * 3));
    assert_eq!(report.filled_qty, u64::from(LOT * 2));
    assert_eq!(report.canceled_qty, u64::from(LOT));
    assert_eq!(report.aborted_qty, 0);
    assert!(authority.causal_facts().iter().any(|fact| matches!(
        fact.kind,
        CausalFactKind::Terminated {
            order: FIRST_BUY_ORDER,
            qty: LOT,
            reason: Termination::MarketRemainder,
            ..
        }
    )));
}

fn run_single_trade_acceptance(t1_enabled: bool, expected_locked: u32, expected_sellable: u32) {
    let (mut authority, code) = session_with_resting_sell(t1_enabled, LOT);
    enqueue_crossing_buy(&mut authority, &code);
    let authority_before = authority.business_state_hash().unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output = apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();

    assert_eq!(authority.business_state_hash().unwrap(), authority_before);
    assert_eq!(authority.pending_player.len(), 1);
    assert_eq!(
        output
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert_eq!(
        output.validation.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert!(output.plan_reports.is_empty());
    assert_eq!(output.receipts.len(), 2);
    assert_buy_receipt(&output.receipts[0], &code);
    assert_sell_receipt(&output.receipts[1], &code);
    assert_eq!(output.p6.settlement.applied_receipts, 2);
    assert_eq!(output.p6.settlement.applied_groups, 2);
    assert!(output.p6.events.is_empty());
    assert!(matches!(
        output.events.as_slice(),
        [Event::Trade {
            seq: 1,
            code: traded_code,
            price,
            qty: LOT,
            maker: SELLER,
            taker: PLAYER,
        }, Event::PriceTick { seq: 2, tick: 1, code: price_code, daily_candle, .. }]
        if traded_code == &code && *price == Money::from_cents(PRICE_CENTS)
            && price_code == &code && daily_candle.volume == u64::from(LOT)
    ));

    let committed =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
            .unwrap()
            .commit();

    assert_eq!(committed.tick.events, output.events);
    assert!(authority.pending_player.is_empty());
    assert_eq!(authority.next_order_id, 3);
    assert_eq!(authority.seq(), 2);
    assert_eq!(authority.next_receipt_base, 2);
    assert_eq!(authority.envelope_ledger.next_receipt_index(), 2);
    assert_eq!(authority.envelope_ledger.iter().count(), 0);
    assert_eq!(authority.envelope_ledger.terminal_count(), 0);
    assert!(authority.markets[&code].resting_orders().is_empty());

    let buyer = &authority.accounts[&PLAYER];
    let buyer_position = &buyer.positions[&code];
    assert_eq!(buyer.cash, Money::from_cents(9_899_499));
    assert_eq!(buyer_position.qty, LOT);
    assert_eq!(buyer_position.t1_locked, expected_locked);
    assert_eq!(buyer_position.invested_cents, 100_000);
    assert_eq!(buyer_position.recovered_cents, 0);
    assert_eq!(buyer.sellable_qty(&code), expected_sellable);

    let seller = &authority.accounts[&SELLER];
    assert_eq!(seller.cash, Money::from_cents(99_449));
    assert!(!seller.positions.contains_key(&code));
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.receipt.business_hash()
    );
    assert_eq!(
        authority.session_state_hash().unwrap(),
        committed.receipt.session_hash()
    );
    assert_eq!(committed.receipt.next_receipt_base(), 2);
}

fn session_with_resting_sell(t1_enabled: bool, sell_qty: u32) -> (GameSession, StockCode) {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    // Product sessions are correctly fixed to A-share T+1. The false branch is an
    // in-crate test seam for the lower settlement transaction only.
    game.setup.t1_enabled = t1_enabled;
    let code = game.markets.keys().next().unwrap().clone();
    let mut seller = Account::new(SELLER, AccountKind::Player, Money::ZERO);
    seller
        .grant_position(code.clone(), sell_qty, Money::from_cents(PRICE_CENTS))
        .unwrap();
    assert!(game.accounts.insert(SELLER, seller).is_none());
    let placed = game
        .markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: SELL_ORDER,
            side: Side::Sell,
            price: Money::from_cents(PRICE_CENTS),
            qty: sell_qty,
            original_qty: sell_qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: SELLER,
            seq: 0,
        })
        .unwrap();
    assert!(placed.trades.is_empty());
    assert_eq!(placed.resting.unwrap().qty, sell_qty);
    game.next_order_id = FIRST_BUY_ORDER.0;
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let (key, envelope) = game.envelope_ledger.iter().next().unwrap();
    assert_eq!(game.envelope_ledger.iter().count(), 1);
    assert_eq!(key.account, SELLER);
    assert_eq!(key.stock, code);
    assert_eq!(key.order, SELL_ORDER);
    assert_eq!(key.side, Side::Sell);
    assert_eq!(envelope.origin(), EnvelopeOrigin::TickStart);
    assert_eq!(envelope.live(), ResVec::new(Money::ZERO, sell_qty));
    (game, code)
}

fn enqueue_crossing_buy(game: &mut GameSession, code: &StockCode) {
    game.enqueue_player_intent(
        PLAYER,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(PRICE_CENTS),
            qty: LOT,
        },
    )
    .unwrap();
}

fn assert_buy_receipt(receipt: &EnvelopeReceipt, code: &StockCode) {
    let charged = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::ZERO,
        transfer_fee: Money::from_cents(1),
    };
    assert_eq!(receipt.index, 0);
    assert_eq!(receipt.local_key.source(), ReceiptSource::SealedIntent(0));
    assert_eq!(
        receipt.envelope,
        EnvelopeKey {
            account: PLAYER,
            stock: code.clone(),
            order: FIRST_BUY_ORDER,
            side: Side::Buy,
        }
    );
    assert_eq!(receipt.kind, ReceiptKind::Fill);
    assert_eq!((receipt.qty_before, receipt.qty_after), (LOT, 0));
    assert_eq!(
        (receipt.value_before, receipt.value_after),
        (Money::ZERO, Money::from_cents(100_000))
    );
    assert_eq!(
        receipt.delta,
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100_501), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        )
    );
    assert_eq!(receipt.nominal, charged);
    assert_eq!(receipt.charged, charged);
    assert_eq!(receipt.charged_before, FeeComponents::ZERO);
    assert_eq!(receipt.charged_after, charged);
    assert_eq!(receipt.deliver_qty, LOT);
    assert_eq!(receipt.deliver_cash, Money::ZERO);
}

fn assert_sell_receipt(receipt: &EnvelopeReceipt, code: &StockCode) {
    let charged = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::from_cents(50),
        transfer_fee: Money::from_cents(1),
    };
    assert_eq!(receipt.index, 1);
    assert_eq!(receipt.local_key.source(), ReceiptSource::SealedIntent(0));
    assert_eq!(
        receipt.envelope,
        EnvelopeKey {
            account: SELLER,
            stock: code.clone(),
            order: SELL_ORDER,
            side: Side::Sell,
        }
    );
    assert_eq!(receipt.kind, ReceiptKind::Fill);
    assert_eq!((receipt.qty_before, receipt.qty_after), (LOT, 0));
    assert_eq!(
        (receipt.value_before, receipt.value_after),
        (Money::ZERO, Money::from_cents(100_000))
    );
    assert_eq!(
        receipt.delta,
        ReceiptDelta::sealed(ResVec::new(Money::ZERO, LOT), ResVec::ZERO, ResVec::ZERO,)
    );
    assert_eq!(receipt.nominal, charged);
    assert_eq!(receipt.charged, charged);
    assert_eq!(receipt.charged_before, FeeComponents::ZERO);
    assert_eq!(receipt.charged_after, charged);
    assert_eq!(receipt.deliver_qty, 0);
    assert_eq!(receipt.deliver_cash, Money::from_cents(99_449));
}
