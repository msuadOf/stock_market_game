use super::p6_transaction::{
    apply_p6_transaction, apply_session_p6_transaction, prepare_p6_transaction, P6TransactionError,
};
use super::retail_projection::{RetailProjectionError, RetailProjectionSeen};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::{
    Account, AccountId, AccountKind, Money, OrderId, RetailExperienceState, Side, StockCode,
};
use std::collections::BTreeMap;

fn retail_session(cash: i64) -> crate::GameSession {
    let mut game = crate::GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    game.accounts = accounts(cash);
    game.retail_experience = retail();
    game.retail_projection_seen = RetailProjectionSeen::default();
    game
}

fn stock() -> StockCode {
    StockCode("600001".to_owned())
}

fn fill(index: u64, side: Side, order: u64, qty: u32, gross: i64) -> EnvelopeReceipt {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: stock(),
        order: OrderId(order),
        side,
    };
    let charged = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let gross = Money::from_cents(gross);
    let (delta, deliver_qty, deliver_cash) = match side {
        Side::Buy => (
            ReceiptDelta::sealed(
                ResVec::new(gross.add(charged.total().unwrap()).unwrap(), 0),
                ResVec::ZERO,
                ResVec::ZERO,
            ),
            qty,
            Money::ZERO,
        ),
        Side::Sell => (
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, qty), ResVec::ZERO, ResVec::ZERO),
            0,
            gross.sub(charged.total().unwrap()).unwrap(),
        ),
    };
    EnvelopeReceipt {
        index,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: qty,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: gross,
        delta,
        nominal: charged,
        charged,
        charged_before: FeeComponents::ZERO,
        charged_after: charged,
        deliver_qty,
        deliver_cash,
    }
}

fn normalized_buy_fill_chain(order: u64) -> (EnvelopeReceipt, EnvelopeReceipt) {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: stock(),
        order: OrderId(order),
        side: Side::Buy,
    };
    let charged = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let nominal = FeeComponents {
        commission: Money::from_cents(9_999),
        ..FeeComponents::ZERO
    };
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::from_cents(100_200),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let mut first = chained_buy_fill(
        &key,
        0,
        100,
        50,
        Money::ZERO,
        Money::from_cents(50_000),
        FeeComponents::ZERO,
        charged,
        nominal,
    );
    ledger.apply(&mut first).unwrap();
    let mut second = chained_buy_fill(
        &key,
        1,
        50,
        0,
        Money::from_cents(50_000),
        Money::from_cents(100_000),
        charged,
        charged,
        nominal,
    );
    ledger.apply(&mut second).unwrap();
    (first[0].clone(), second[0].clone())
}

#[allow(clippy::too_many_arguments)]
fn chained_buy_fill(
    key: &EnvelopeKey,
    source_index: u32,
    qty_before: u32,
    qty_after: u32,
    value_before: Money,
    value_after: Money,
    charged_before: FeeComponents,
    charged: FeeComponents,
    nominal: FeeComponents,
) -> [EnvelopeReceipt; 1] {
    let filled_qty = qty_before.checked_sub(qty_after).unwrap();
    let gross = value_after.sub(value_before).unwrap();
    let charged_after = FeeComponents {
        commission: charged_before.commission.add(charged.commission).unwrap(),
        stamp_tax: charged_before.stamp_tax.add(charged.stamp_tax).unwrap(),
        transfer_fee: charged_before
            .transfer_fee
            .add(charged.transfer_fee)
            .unwrap(),
    };
    [EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::Auction(source_index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key.clone(),
        kind: ReceiptKind::Fill,
        qty_before,
        qty_after,
        value_before,
        value_after,
        delta: ReceiptDelta::sealed(
            ResVec::new(gross.add(charged.total().unwrap()).unwrap(), 0),
            ResVec::ZERO,
            ResVec::new(Money::from_cents(i64::from(qty_after) * 1_002), 0),
        ),
        nominal,
        charged,
        charged_before,
        charged_after,
        deliver_qty: filled_qty,
        deliver_cash: Money::ZERO,
    }]
}

fn retail() -> AccountPagedMap<crate::RetailExperienceState> {
    BTreeMap::from([(
        AccountId(1),
        RetailExperienceState::without_equity_reference(),
    )])
    .into()
}

fn accounts(cash: i64) -> AccountBook {
    BTreeMap::from([(
        AccountId(1),
        Account::new(AccountId(1), AccountKind::Retail, Money::from_cents(cash)),
    )])
    .into()
}

#[test]
fn p6_without_new_fills_prepares_no_account_or_retail_state_patch() {
    let accounts = accounts(200_000);
    let experience = retail();
    let seen = RetailProjectionSeen::default();
    let prepared = prepare_p6_transaction(&accounts, &experience, &seen, 10, &[], true).unwrap();

    assert!(prepared.account_patch.is_empty());
    assert!(prepared.retail_patch.is_empty());
    assert!(prepared.output.events.is_empty());
    assert_eq!(prepared.seen, seen);
    assert_eq!(accounts[&AccountId(1)].cash, Money::from_cents(200_000));
    assert_eq!(experience, retail());
}

#[test]
fn p6_empty_batch_does_not_repair_an_invalid_watchlist_during_settlement() {
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    for index in 0..9 {
        experience
            .get_mut(&AccountId(1))
            .unwrap()
            .observe_stock(&StockCode(format!("6000{index:02}")), index);
    }
    let mut seen = RetailProjectionSeen::default();
    let prepared = prepare_p6_transaction(&accounts, &experience, &seen, 10, &[], true).unwrap();
    assert!(prepared.account_patch.is_empty());
    assert!(prepared.retail_patch.is_empty());

    apply_p6_transaction(&mut accounts, &mut experience, &mut seen, 10, &[], true).unwrap();
    assert_eq!(experience[&AccountId(1)].stocks.len(), 9);
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    let template = setup.stocks[0].clone();
    setup.stocks = (0..9)
        .map(|index| {
            let mut stock = template.clone();
            stock.code = StockCode(format!("6000{index:02}"));
            stock
        })
        .collect();
    let game = crate::GameSession::new(setup, 42).unwrap();
    let mut save = game.save().unwrap();
    save.retail_experience = experience.to_map();
    assert!(matches!(
        crate::GameSession::restore(&save),
        Err(crate::SessionError::InvalidSave(reason)) if reason.contains("unheld watchlist limit")
    ));
}

#[test]
fn p6_final_sell_prunes_only_the_affected_retail_account() {
    let mut accounts = accounts(200_000);
    accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(stock(), 100, Money::from_cents(1_000))
        .unwrap();
    let mut experience = retail();
    let state = experience.get_mut(&AccountId(1)).unwrap();
    for index in 0..8 {
        state.observe_stock(&StockCode(format!("6010{index:02}")), index);
    }
    state
        .initialize_holding(
            &stock(),
            Some(Money::from_cents(1_000)),
            Money::from_cents(1_000),
            0,
        )
        .unwrap();
    let mut seen = RetailProjectionSeen::default();

    let result = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[fill(1, Side::Sell, 10, 100, 100_000)],
        true,
    )
    .unwrap();

    assert_eq!(result.settlement.applied_receipts, 1);
    assert!(accounts[&AccountId(1)].positions.get(&stock()).is_none());
    assert_eq!(experience[&AccountId(1)].stocks.len(), 8);
    assert!(experience[&AccountId(1)].stocks.contains_key(&stock()));
    assert!(!experience[&AccountId(1)]
        .stocks
        .contains_key(&StockCode("601000".to_owned())));
}

#[test]
fn non_fill_receipt_advances_seen_without_changing_accounts_or_experience() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.kind = ReceiptKind::Release;
    receipt.qty_after = receipt.qty_before;
    receipt.value_after = receipt.value_before;
    receipt.delta = ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::ZERO);
    receipt.nominal = FeeComponents::ZERO;
    receipt.charged = FeeComponents::ZERO;
    receipt.charged_after = FeeComponents::ZERO;
    receipt.deliver_qty = 0;
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash;
    let before_experience = experience.clone();

    for _ in 0..2 {
        let result = apply_p6_transaction(
            &mut accounts,
            &mut experience,
            &mut seen,
            10,
            &[receipt.clone()],
            true,
        )
        .unwrap();
        assert_eq!(result.settlement.applied_receipts, 0);
        assert!(result.events.is_empty());
        assert_eq!(seen.len(), 1);
        assert_eq!(accounts[&AccountId(1)].cash, before_cash);
        assert!(accounts[&AccountId(1)].positions.is_empty());
        assert_eq!(experience, before_experience);
    }
}

#[test]
fn session_p6_consumes_shadow_owned_seen_and_is_idempotent() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut game = retail_session(200_000);

    let first = apply_session_p6_transaction(&mut game, &[receipt.clone()]).unwrap();
    let after_first = game.business_state_hash().unwrap();
    let second = apply_session_p6_transaction(&mut game, &[receipt]).unwrap();

    assert_eq!(first.settlement.applied_receipts, 1);
    assert_eq!(second.settlement.applied_receipts, 0);
    assert!(second.events.is_empty());
    assert_eq!(game.business_state_hash().unwrap(), after_first);
}

#[test]
fn session_p6_failure_keeps_all_authoritative_containers_unchanged() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.value_after = Money::ZERO;
    let mut game = retail_session(200_000);
    let before = game.business_state_hash().unwrap();

    assert!(apply_session_p6_transaction(&mut game, &[receipt]).is_err());

    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn session_p6_uses_the_sessions_t1_policy() {
    for (t1_enabled, expected_locked) in [(false, 0), (true, 100)] {
        let mut game = retail_session(200_000);
        game.setup.t1_enabled = t1_enabled;

        apply_session_p6_transaction(&mut game, &[fill(1, Side::Buy, 10, 100, 100_000)]).unwrap();

        let position = &game.accounts[&AccountId(1)].positions[&stock()];
        assert_eq!(position.qty, 100);
        assert_eq!(position.t1_locked, expected_locked);
    }
}

#[test]
fn session_p6_records_the_sessions_nonzero_market_minute() {
    let mut game = retail_session(200_000);
    game.tick = 1;
    let market_minute = game.current_market_minute();
    assert!(market_minute > 0);

    apply_session_p6_transaction(&mut game, &[fill(1, Side::Buy, 10, 100, 100_000)]).unwrap();

    let experience = &game.retail_experience[&AccountId(1)].stocks[&stock()];
    assert_eq!(experience.last_trade_market_minute, market_minute);
    assert_eq!(experience.last_observed_market_minute, market_minute);
}

#[test]
fn duplicate_receipt_is_idempotent_for_accounts_experience_and_seen() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let first = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt.clone()],
        true,
    )
    .unwrap();
    let cash_after_first = accounts[&AccountId(1)].cash;
    let experience_after_first = experience.clone();
    let second = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        11,
        &[receipt],
        true,
    )
    .unwrap();
    assert_eq!(first.settlement.applied_receipts, 1);
    assert_eq!(second.settlement.applied_receipts, 0);
    assert!(second.events.is_empty());
    assert_eq!(accounts[&AccountId(1)].cash, cash_after_first);
    assert_eq!(experience, experience_after_first);
}

#[test]
fn projection_failure_rolls_back_successful_settlement() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.value_after = Money::ZERO;
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash;
    let before_experience = experience.clone();
    assert!(apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true
    )
    .is_err());
    assert_eq!(accounts[&AccountId(1)].cash, before_cash);
    assert_eq!(experience, before_experience);
    assert!(seen.is_empty());
}

#[test]
fn same_order_multi_leg_receipts_settle_once_and_project_one_event() {
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let result = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &{
            let (first, second) = normalized_buy_fill_chain(10);
            [second, first]
        },
        true,
    )
    .unwrap();
    assert_eq!(result.settlement.applied_receipts, 2);
    assert_eq!(result.events.len(), 1);
    assert_eq!(accounts[&AccountId(1)].positions[&stock()].qty, 100);
}

#[test]
fn same_account_buy_then_sell_preserves_the_buy_before_sell_lifecycle() {
    let mut accounts = accounts(300_000);
    accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(stock(), 100, Money::from_cents(1_000))
        .unwrap();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[
            fill(2, Side::Sell, 21, 100, 100_000),
            fill(1, Side::Buy, 20, 100, 100_000),
        ],
        true,
    )
    .unwrap();
    let position = &accounts[&AccountId(1)].positions[&stock()];
    assert_eq!(position.qty, 100);
    assert_eq!(position.t1_locked, 100);
}

#[test]
fn missing_settlement_account_is_typed_and_leaves_every_container_unchanged() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts = AccountBook::default();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_experience = experience.clone();
    let error = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("settlement account 1 is missing"));
    assert!(accounts.is_empty());
    assert_eq!(experience, before_experience);
    assert!(seen.is_empty());
}

#[test]
fn retail_fill_without_experience_is_typed_and_rolls_back_every_container() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts: AccountBook = BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Retail,
            Money::from_cents(200_000),
        ),
    )])
    .into();
    let mut experience = AccountPagedMap::<crate::RetailExperienceState>::default();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash;

    let error = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        P6TransactionError::Projection(RetailProjectionError::MissingRetailExperience {
            account: AccountId(1),
        })
    ));
    assert_eq!(accounts[&AccountId(1)].cash, before_cash);
    assert!(accounts[&AccountId(1)].positions.is_empty());
    assert!(experience.is_empty());
    assert!(seen.is_empty());
}

#[test]
fn later_retail_account_failure_does_not_commit_an_earlier_accounts_settlement() {
    let first = fill(1, Side::Buy, 10, 100, 100_000);
    let mut second = fill(2, Side::Buy, 11, 100, 100_000);
    second.envelope.account = AccountId(2);
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(2),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let mut accounts: AccountBook = BTreeMap::from([
        (
            AccountId(1),
            Account::new(
                AccountId(1),
                AccountKind::Retail,
                Money::from_cents(200_000),
            ),
        ),
        (
            AccountId(2),
            Account::new(
                AccountId(2),
                AccountKind::Retail,
                Money::from_cents(200_000),
            ),
        ),
    ])
    .into();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let experience_before = experience.clone();
    let seen_before = seen.clone();

    let error = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[first, second],
        true,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        P6TransactionError::Projection(RetailProjectionError::MissingRetailExperience {
            account: AccountId(2)
        })
    ));
    for account in accounts.values() {
        assert_eq!(account.cash, Money::from_cents(200_000));
        assert!(account.positions.is_empty());
    }
    assert_eq!(experience, experience_before);
    assert_eq!(seen, seen_before);
}

#[test]
fn non_retail_fill_settles_without_projecting_even_if_an_experience_entry_exists() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts: AccountBook = BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Player,
            Money::from_cents(200_000),
        ),
    )])
    .into();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_experience = experience.clone();

    let result = apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true,
    )
    .unwrap();

    assert_eq!(result.settlement.applied_receipts, 1);
    assert!(result.events.is_empty());
    assert_eq!(accounts[&AccountId(1)].positions[&stock()].qty, 100);
    assert_eq!(experience, before_experience);
    assert_eq!(seen.len(), 1);
}

#[test]
fn settlement_overflow_rolls_back_every_container() {
    let mut first = fill(1, Side::Buy, 10, 1, i64::MAX - 100);
    first.charged = FeeComponents::ZERO;
    first.charged_after = FeeComponents::ZERO;
    first.delta = ReceiptDelta::sealed(
        ResVec::new(first.value_after, 0),
        ResVec::ZERO,
        ResVec::ZERO,
    );
    let mut second = first.clone();
    second.index = 2;
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(2),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let mut accounts = accounts(i64::MAX);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash;
    let before_experience = experience.clone();
    assert!(apply_p6_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[first, second],
        true
    )
    .is_err());
    assert_eq!(accounts[&AccountId(1)].cash, before_cash);
    assert_eq!(experience, before_experience);
    assert!(seen.is_empty());
}
