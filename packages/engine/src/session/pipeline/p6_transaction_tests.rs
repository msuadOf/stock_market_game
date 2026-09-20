use super::p6_transaction::{
    apply_p6_transaction, apply_session_p6_transaction, P6TransactionError,
};
use super::retail_projection::{RetailProjectionError, RetailProjectionSeen};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
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

fn retail() -> BTreeMap<AccountId, RetailExperienceState> {
    BTreeMap::from([(
        AccountId(1),
        RetailExperienceState::without_equity_reference(),
    )])
}

fn accounts(cash: i64) -> BTreeMap<AccountId, Account> {
    BTreeMap::from([(
        AccountId(1),
        Account::new(AccountId(1), AccountKind::Retail, Money::from_cents(cash)),
    )])
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
    assert!(seen.receipts.is_empty());
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
    let mut accounts = BTreeMap::new();
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
    assert!(seen.receipts.is_empty());
}

#[test]
fn retail_fill_without_experience_is_typed_and_rolls_back_every_container() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts = BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Retail,
            Money::from_cents(200_000),
        ),
    )]);
    let mut experience = BTreeMap::new();
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
    assert!(seen.receipts.is_empty());
}

#[test]
fn non_retail_fill_settles_without_projecting_even_if_an_experience_entry_exists() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts = BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Player,
            Money::from_cents(200_000),
        ),
    )]);
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
    assert_eq!(seen.receipts.len(), 1);
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
    assert!(seen.receipts.is_empty());
}
