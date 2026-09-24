use super::settlement::{apply_receipt_settlements, prepare_receipt_settlements};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::session::account_book::AccountBook;
use crate::{Account, AccountId, AccountKind, Money, OrderId, Side, StockCode};
use std::collections::BTreeMap;

#[test]
fn settlement_prepares_only_affected_accounts_with_pool_independent_results() {
    let accounts: AccountBook = (1..=8)
        .map(|id| {
            let account_id = AccountId(id);
            (
                account_id,
                Account::new(account_id, AccountKind::Player, Money::from_cents(200_000)),
            )
        })
        .collect();
    let receipts = [
        fill(
            AccountId(2),
            10,
            Side::Buy,
            100,
            100_000,
            FeeComponents::ZERO,
        ),
        fill(
            AccountId(7),
            11,
            Side::Buy,
            100,
            100_000,
            FeeComponents::ZERO,
        ),
    ];
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| prepare_receipt_settlements(&accounts, &receipts, true).unwrap())
    };
    let (one, one_counts) = run(1);
    let (four, four_counts) = run(4);
    assert_eq!(one_counts, four_counts);
    assert_eq!(
        one.keys().copied().collect::<Vec<_>>(),
        [AccountId(2), AccountId(7)]
    );
    assert_eq!(account_state(&one.into()), account_state(&four.into()));
    assert_eq!(one_counts.applied_groups, 2);
    assert_eq!(one_counts.applied_receipts, 2);
    assert_eq!(accounts[&AccountId(2)].cash, Money::from_cents(200_000));
}

fn stock() -> StockCode {
    StockCode("600001".to_owned())
}

fn fill(
    account: AccountId,
    order: u64,
    side: Side,
    qty: u32,
    gross: i64,
    charged: FeeComponents,
) -> EnvelopeReceipt {
    fill_leg(
        account,
        order,
        side,
        0,
        qty,
        0,
        Money::ZERO,
        Money::from_cents(gross),
        charged,
        charged,
    )
}

#[allow(clippy::too_many_arguments)]
fn fill_leg(
    account: AccountId,
    order: u64,
    side: Side,
    ordinal: u64,
    qty_before: u32,
    qty_after: u32,
    value_before: Money,
    value_after: Money,
    nominal: FeeComponents,
    charged: FeeComponents,
) -> EnvelopeReceipt {
    let key = EnvelopeKey {
        account,
        stock: stock(),
        order: OrderId(order),
        side,
    };
    let qty = qty_before.checked_sub(qty_after).unwrap();
    let gross = value_after.sub(value_before).unwrap();
    let delta = match side {
        Side::Buy => ReceiptDelta::sealed(
            ResVec::new(
                gross
                    .add(charged.commission)
                    .unwrap()
                    .add(charged.transfer_fee)
                    .unwrap(),
                0,
            ),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        Side::Sell => {
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, qty), ResVec::ZERO, ResVec::ZERO)
        }
    };
    let deliver_cash = match side {
        Side::Buy => Money::ZERO,
        Side::Sell => gross.sub(charged.total().unwrap()).unwrap(),
    };
    EnvelopeReceipt {
        index: order,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(order),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal,
            },
        )
        .unwrap(),
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before,
        qty_after,
        value_before,
        value_after,
        delta,
        nominal,
        charged,
        charged_before: FeeComponents::ZERO,
        charged_after: charged,
        deliver_qty: if side == Side::Buy { qty } else { 0 },
        deliver_cash,
    }
}

fn assert_validated_terminal_receipt(receipt: EnvelopeReceipt) {
    let remaining_qty = receipt.qty_before;
    assert_validated_terminal_receipt_with_audit(
        receipt,
        EnvelopeAudit {
            limit: Money::from_cents(100),
            remaining_qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            nominal: FeeComponents::ZERO,
            charged: FeeComponents::ZERO,
        },
    );
}

fn assert_validated_terminal_receipt_with_audit(receipt: EnvelopeReceipt, audit: EnvelopeAudit) {
    let (cash, shares) = match receipt.envelope.side {
        Side::Buy => (receipt.delta.spent.cash, 0),
        Side::Sell => (Money::ZERO, receipt.qty_before),
    };
    let mut ledger = EnvelopeLedger::new(
        1,
        [Envelope::p3_created(
            receipt.envelope.clone(),
            cash,
            shares,
            audit,
        )],
    )
    .unwrap();
    let key = receipt.envelope.clone();
    let mut receipts = [receipt];
    ledger.apply(&mut receipts).unwrap();
    ledger.remove_terminal(&[key]).unwrap();
}

fn account_state(
    accounts: &AccountBook,
) -> BTreeMap<AccountId, (Money, Vec<(StockCode, u32, u32, i64, i64)>)> {
    accounts
        .iter()
        .map(|(id, account)| {
            let positions = account
                .positions
                .iter()
                .map(|(code, position)| {
                    (
                        code.clone(),
                        position.qty,
                        position.t1_locked,
                        position.invested_cents,
                        position.recovered_cents,
                    )
                })
                .collect();
            (*id, (account.cash, positions))
        })
        .collect()
}

fn non_fill(kind: ReceiptKind) -> EnvelopeReceipt {
    let mut receipt = fill(
        AccountId(1),
        10,
        Side::Buy,
        100,
        100_000,
        FeeComponents::ZERO,
    );
    receipt.kind = kind;
    receipt.qty_after = receipt.qty_before;
    receipt.value_after = receipt.value_before;
    receipt.deliver_qty = 0;
    receipt.deliver_cash = Money::ZERO;
    receipt.delta = ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::ZERO);
    receipt
}

#[test]
fn settles_receipt_actual_fee_components_without_recomputing_them() {
    let account = AccountId(1);
    let mut accounts: AccountBook = BTreeMap::from([(
        account,
        Account::new(account, AccountKind::Player, Money::from_cents(200_000)),
    )])
    .into();
    let charged = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::ZERO,
        transfer_fee: Money::from_cents(1),
    };

    let applied = apply_receipt_settlements(
        &mut accounts,
        &[fill(account, 1, Side::Buy, 100, 100_000, charged)],
        true,
    )
    .unwrap();

    assert_eq!(applied.applied_receipts, 1);
    assert_eq!(applied.applied_groups, 1);
    assert_eq!(accounts[&account].cash, Money::from_cents(99_499));
    let position = &accounts[&account].positions[&stock()];
    assert_eq!(position.qty, 100);
    assert_eq!(position.t1_locked, 100);
    assert_eq!(position.invested_cents, 100_000);
}

#[test]
fn applies_same_account_stock_buy_before_sell_to_preserve_t1_lifecycle() {
    let account = AccountId(1);
    let mut holder = Account::new(account, AccountKind::Player, Money::from_cents(200_000));
    holder
        .grant_position(stock(), 100, Money::from_cents(1_000))
        .unwrap();
    let mut accounts: AccountBook = BTreeMap::from([(account, holder)]).into();
    let buy_charged = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::ZERO,
        transfer_fee: Money::from_cents(1),
    };
    let sell_charged = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::from_cents(50),
        transfer_fee: Money::from_cents(1),
    };

    let sell = fill(account, 2, Side::Sell, 100, 100_000, sell_charged);
    let buy = fill(account, 1, Side::Buy, 100, 100_000, buy_charged);
    assert_validated_terminal_receipt(sell.clone());
    assert_validated_terminal_receipt(buy.clone());
    let applied = apply_receipt_settlements(&mut accounts, &[sell, buy], true).unwrap();

    assert_eq!(applied.applied_receipts, 2);
    assert_eq!(applied.applied_groups, 2);
    assert_eq!(accounts[&account].cash, Money::from_cents(198_948));
    let position = &accounts[&account].positions[&stock()];
    assert_eq!(position.qty, 100);
    assert_eq!(position.t1_locked, 100);
    assert_eq!(position.invested_cents, 200_000);
    assert_eq!(position.recovered_cents, 100_000);
}

#[test]
fn non_fill_receipts_do_not_settle_accounts() {
    let account = AccountId(1);
    let mut accounts: AccountBook = BTreeMap::from([(
        account,
        Account::new(account, AccountKind::Player, Money::from_cents(200_000)),
    )])
    .into();
    let before_cash = accounts[&account].cash;

    let applied = apply_receipt_settlements(
        &mut accounts,
        &[
            non_fill(ReceiptKind::Release),
            non_fill(ReceiptKind::Rollover),
            non_fill(ReceiptKind::Reject),
        ],
        true,
    )
    .unwrap();

    assert_eq!(applied.applied_receipts, 0);
    assert_eq!(applied.applied_groups, 0);
    assert_eq!(accounts[&account].cash, before_cash);
    assert!(accounts[&account].positions.is_empty());
}

#[test]
fn buyer_stamp_tax_is_a_typed_fatal_instead_of_a_silent_drop() {
    let account = AccountId(1);
    let mut accounts: AccountBook = BTreeMap::from([(
        account,
        Account::new(account, AccountKind::Player, Money::from_cents(200_000)),
    )])
    .into();
    let receipt = fill(
        account,
        1,
        Side::Buy,
        100,
        100_000,
        FeeComponents {
            stamp_tax: Money::from_cents(1),
            ..FeeComponents::ZERO
        },
    );

    let error = apply_receipt_settlements(&mut accounts, &[receipt], true).unwrap_err();

    assert!(error
        .to_string()
        .contains("buyer receipt has non-zero stamp tax"));
    assert_eq!(accounts[&account].cash, Money::from_cents(200_000));
}

#[test]
fn same_group_buy_success_then_sell_failure_leaves_the_entire_input_unchanged() {
    let account = AccountId(1);
    let mut holder = Account::new(account, AccountKind::Player, Money::from_cents(300_000));
    holder
        .grant_position(stock(), 100, Money::from_cents(1_000))
        .unwrap();
    let mut accounts: AccountBook = BTreeMap::from([(account, holder)]).into();
    let before = account_state(&accounts);

    let error = apply_receipt_settlements(
        &mut accounts,
        &[
            fill(account, 1, Side::Buy, 100, 100_000, FeeComponents::ZERO),
            fill(account, 2, Side::Sell, 200, 200_000, FeeComponents::ZERO),
        ],
        true,
    )
    .unwrap_err();

    assert!(error.to_string().contains("insufficient shares"));
    assert_eq!(account_state(&accounts), before);
}

#[test]
fn a_later_account_failure_rolls_back_an_earlier_account_success() {
    let first = AccountId(1);
    let second = AccountId(2);
    let mut accounts: AccountBook = BTreeMap::from([
        (
            first,
            Account::new(first, AccountKind::Player, Money::from_cents(300_000)),
        ),
        (
            second,
            Account::new(second, AccountKind::Player, Money::from_cents(300_000)),
        ),
    ])
    .into();
    let before = account_state(&accounts);

    let error = apply_receipt_settlements(
        &mut accounts,
        &[
            fill(first, 1, Side::Buy, 100, 100_000, FeeComponents::ZERO),
            fill(second, 2, Side::Sell, 100, 100_000, FeeComponents::ZERO),
        ],
        true,
    )
    .unwrap_err();

    assert!(error.to_string().contains("insufficient shares"));
    assert_eq!(account_state(&accounts), before);
}

#[test]
fn seller_settlement_charges_only_the_capped_receipt_amount_when_nominal_fee_exceeds_gross() {
    let account = AccountId(1);
    let mut holder = Account::new(account, AccountKind::Player, Money::ZERO);
    holder
        .grant_position(stock(), 1, Money::from_cents(100))
        .unwrap();
    let mut accounts: AccountBook = BTreeMap::from([(account, holder)]).into();
    let nominal = FeeComponents {
        commission: Money::from_cents(500),
        ..FeeComponents::ZERO
    };
    let charged = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let receipt = fill_leg(
        account,
        1,
        Side::Sell,
        0,
        1,
        0,
        Money::ZERO,
        Money::from_cents(100),
        nominal,
        charged,
    );
    assert_validated_terminal_receipt(receipt.clone());

    apply_receipt_settlements(&mut accounts, &[receipt], true).unwrap();

    assert_eq!(accounts[&account].cash, Money::ZERO);
    assert!(!accounts[&account].positions.contains_key(&stock()));
}

#[test]
fn seller_settlement_deducts_charged_not_nominal_and_recovers_full_gross() {
    let account = AccountId(1);
    let mut holder = Account::new(account, AccountKind::Player, Money::from_cents(1_000));
    holder
        .grant_position(stock(), 2, Money::from_cents(100))
        .unwrap();
    let mut accounts: AccountBook = BTreeMap::from([(account, holder)]).into();
    let charged_before = FeeComponents {
        commission: Money::from_cents(1_400),
        ..FeeComponents::ZERO
    };
    let nominal = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let charged = nominal;
    let mut receipt = fill_leg(
        account,
        1,
        Side::Sell,
        0,
        1,
        0,
        Money::from_cents(1_400),
        Money::from_cents(2_400),
        nominal,
        charged,
    );
    receipt.charged_before = charged_before;
    receipt.charged_after = FeeComponents {
        commission: Money::from_cents(1_500),
        ..FeeComponents::ZERO
    };
    assert_validated_terminal_receipt_with_audit(
        receipt.clone(),
        EnvelopeAudit {
            limit: Money::from_cents(100),
            remaining_qty: 1,
            filled_qty: 1,
            filled_value: Money::from_cents(1_400),
            nominal: charged_before,
            charged: charged_before,
        },
    );

    apply_receipt_settlements(&mut accounts, &[receipt], true).unwrap();

    assert_eq!(accounts[&account].cash, Money::from_cents(1_900));
    let position = &accounts[&account].positions[&stock()];
    assert_eq!(position.qty, 1);
    assert_eq!(position.recovered_cents, 1_000);
}

#[test]
fn multiple_orders_and_legs_share_one_account_stock_side_settlement_group() {
    let account = AccountId(1);
    let mut accounts: AccountBook = BTreeMap::from([(
        account,
        Account::new(account, AccountKind::Player, Money::from_cents(400_000)),
    )])
    .into();
    let first_fee = FeeComponents {
        commission: Money::from_cents(250),
        ..FeeComponents::ZERO
    };
    let second_fee = FeeComponents {
        commission: Money::from_cents(250),
        ..FeeComponents::ZERO
    };
    let third_fee = FeeComponents {
        commission: Money::from_cents(500),
        ..FeeComponents::ZERO
    };
    let first_leg = fill_leg(
        account,
        1,
        Side::Buy,
        0,
        200,
        100,
        Money::ZERO,
        Money::from_cents(100_000),
        first_fee,
        first_fee,
    );
    let mut first_leg = first_leg;
    first_leg.delta.live_after = ResVec::new(Money::from_cents(100_250), 0);
    let mut second_leg = fill_leg(
        account,
        1,
        Side::Buy,
        1,
        100,
        0,
        Money::from_cents(100_000),
        Money::from_cents(200_000),
        second_fee,
        second_fee,
    );
    second_leg.charged_before = first_fee;
    second_leg.charged_after = FeeComponents {
        commission: Money::from_cents(500),
        ..FeeComponents::ZERO
    };
    let mut ledger = EnvelopeLedger::new(
        1,
        [Envelope::p3_created(
            first_leg.envelope.clone(),
            Money::from_cents(200_500),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 200,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let terminal_key = first_leg.envelope.clone();
    let mut receipt_chain = [first_leg.clone(), second_leg.clone()];
    ledger.apply(&mut receipt_chain).unwrap();
    ledger.remove_terminal(&[terminal_key]).unwrap();
    assert_validated_terminal_receipt(fill(account, 2, Side::Buy, 100, 100_000, third_fee));

    let applied = apply_receipt_settlements(
        &mut accounts,
        &[
            first_leg,
            second_leg,
            fill(account, 2, Side::Buy, 100, 100_000, third_fee),
        ],
        true,
    )
    .unwrap();

    assert_eq!(applied.applied_receipts, 3);
    assert_eq!(applied.applied_groups, 1);
    assert_eq!(accounts[&account].cash, Money::from_cents(99_000));
    let position = &accounts[&account].positions[&stock()];
    assert_eq!(position.qty, 300);
    assert_eq!(position.t1_locked, 300);
    assert_eq!(position.invested_cents, 300_000);
}
