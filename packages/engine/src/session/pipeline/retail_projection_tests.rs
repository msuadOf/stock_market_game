use super::retail_projection::{
    project_retail_receipts, RetailProjectionError, RetailProjectionInput, RetailProjectionSeen,
    RetailReceiptEvent,
};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::{AccountId, Money, OrderId, Position, RetailExperienceState, Side, StockCode};
use std::collections::{BTreeMap, BTreeSet};

fn code() -> StockCode {
    StockCode("600001".to_owned())
}

fn fill(index: u64, side: Side, order: u64, qty: u32, gross: i64) -> EnvelopeReceipt {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code(),
        order: OrderId(order),
        side,
    };
    let charged = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let nominal = match side {
        // A buyer's P5 receipt may have a nominal fee greater than the actual
        // charged delta. P6 must preserve that distinction.
        Side::Buy => FeeComponents {
            commission: Money::from_cents(9_999),
            ..FeeComponents::ZERO
        },
        // Seller charge allocation is validated against its nominal history.
        Side::Sell => charged,
    };
    let charged_total = charged.total().unwrap();
    let gross_money = Money::from_cents(gross);
    let (delta, deliver_qty, deliver_cash) = match side {
        Side::Buy => (
            ReceiptDelta::sealed(
                ResVec::new(gross_money.add(charged_total).unwrap(), 0),
                ResVec::ZERO,
                ResVec::ZERO,
            ),
            qty,
            Money::ZERO,
        ),
        Side::Sell => (
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, qty), ResVec::ZERO, ResVec::ZERO),
            0,
            gross_money.sub(charged_total).unwrap(),
        ),
    };
    let receipt = EnvelopeReceipt {
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
        envelope: key.clone(),
        kind: ReceiptKind::Fill,
        qty_before: qty,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: Money::from_cents(gross),
        delta,
        nominal,
        charged,
        charged_before: FeeComponents::ZERO,
        charged_after: charged,
        deliver_qty,
        deliver_cash,
    };
    let (cash, shares) = match side {
        Side::Buy => (receipt.delta.spent.cash, 0),
        Side::Sell => (Money::ZERO, receipt.delta.spent.shares),
    };
    let mut ledger = EnvelopeLedger::new(
        index,
        [Envelope::p3_created(
            key,
            cash,
            shares,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: qty,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let mut normalized = [receipt];
    ledger.apply(&mut normalized).unwrap();
    normalized.into_iter().next().unwrap()
}

fn non_fill(index: u64, kind: ReceiptKind) -> EnvelopeReceipt {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code(),
        order: OrderId(index),
        side: Side::Buy,
    };
    let live_cash = Money::from_cents(100_000);
    let (source, delta) = match kind {
        ReceiptKind::Release | ReceiptKind::Reject => (
            ReceiptSource::SealedIntent(index),
            ReceiptDelta::sealed(ResVec::ZERO, ResVec::new(live_cash, 0), ResVec::ZERO),
        ),
        ReceiptKind::Rollover => (
            ReceiptSource::Auction(index.try_into().unwrap()),
            ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::new(live_cash, 0)),
        ),
        ReceiptKind::Fill => panic!("non_fill requires a non-fill receipt kind"),
    };
    let receipt = EnvelopeReceipt {
        index,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            source,
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key.clone(),
        kind,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    };
    let mut ledger = EnvelopeLedger::new(
        index,
        [Envelope::p3_created(
            key,
            live_cash,
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
    let mut normalized = [receipt];
    ledger.apply(&mut normalized).unwrap();
    normalized.into_iter().next().unwrap()
}

fn buy_fill_chain(order: u64) -> (EnvelopeReceipt, EnvelopeReceipt) {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code(),
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
    let first = chained_buy_fill(
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
    let charged_after_first = charged;
    let second = chained_buy_fill(
        &key,
        1,
        50,
        0,
        Money::from_cents(50_000),
        Money::from_cents(100_000),
        charged_after_first,
        charged,
        nominal,
    );
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key,
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
    let mut first_normalized = [first];
    ledger.apply(&mut first_normalized).unwrap();
    let mut second_normalized = [second];
    ledger.apply(&mut second_normalized).unwrap();
    (
        first_normalized.into_iter().next().unwrap(),
        second_normalized.into_iter().next().unwrap(),
    )
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
) -> EnvelopeReceipt {
    let qty = qty_before.checked_sub(qty_after).unwrap();
    let gross = value_after.sub(value_before).unwrap();
    let charged_after = FeeComponents {
        commission: charged_before.commission.add(charged.commission).unwrap(),
        stamp_tax: charged_before.stamp_tax.add(charged.stamp_tax).unwrap(),
        transfer_fee: charged_before
            .transfer_fee
            .add(charged.transfer_fee)
            .unwrap(),
    };
    EnvelopeReceipt {
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
        deliver_qty: qty,
        deliver_cash: Money::ZERO,
    }
}

fn positions(qty: u32) -> BTreeMap<AccountId, BTreeMap<StockCode, Position>> {
    BTreeMap::from([(
        AccountId(1),
        BTreeMap::from([(
            code(),
            Position {
                qty,
                t1_locked: 0,
                invested_cents: i64::from(qty) * 1_000,
                recovered_cents: 0,
            },
        )]),
    )])
}

fn retail() -> crate::session::retail_experience_book::RetailExperienceBook {
    BTreeMap::from([(
        AccountId(1),
        RetailExperienceState::without_equity_reference(),
    )])
    .into()
}

fn retail_accounts() -> BTreeSet<AccountId> {
    BTreeSet::from([AccountId(1)])
}

#[test]
fn duplicate_receipt_identity_is_seen_once_and_never_repeats_experience() {
    let receipt = fill(7, Side::Buy, 10, 100, 100_000);
    let before = BTreeMap::new();
    let after = positions(100);
    let first = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[receipt.clone(), receipt.clone()],
    })
    .unwrap();

    assert_eq!(first.events.len(), 1);
    assert!(matches!(
        first.events.as_slice(),
        [RetailReceiptEvent::Filled { charged, .. }]
            if charged.commission == Money::from_cents(100)
    ));
    assert_eq!(first.seen.len(), 1);

    let second = project_retail_receipts(RetailProjectionInput {
        retail_experience: &first.retail_experience.clone().into(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &first.seen,
        market_minute: 10,
        receipts: &[receipt],
    })
    .unwrap();
    assert!(second.events.is_empty());
    assert_eq!(second.retail_experience, first.retail_experience);
}

#[test]
fn later_receipt_for_the_same_order_projects_the_later_partial_fill() {
    let (first_receipt, second_receipt) = buy_fill_chain(10);
    let empty = BTreeMap::new();
    let after_first = positions(50);
    let first = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &empty,
        positions_after: &after_first,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[first_receipt],
    })
    .unwrap();

    let after_second = positions(100);
    let second = project_retail_receipts(RetailProjectionInput {
        retail_experience: &first.retail_experience.clone().into(),
        retail_accounts: &retail_accounts(),
        positions_before: &after_first,
        positions_after: &after_second,
        seen: &first.seen,
        market_minute: 11,
        receipts: &[second_receipt],
    })
    .unwrap();

    assert!(matches!(
        second.events.as_slice(),
        [RetailReceiptEvent::Filled {
            order: OrderId(10),
            qty: 50,
            ..
        }]
    ));
    assert_eq!(second.seen.len(), 2);
}

#[test]
fn multiple_receipts_for_one_order_in_one_batch_produce_one_aggregated_experience_event() {
    let (first_leg, second_leg) = buy_fill_chain(10);
    let before = BTreeMap::new();
    let after = positions(100);

    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[second_leg, first_leg],
    })
    .unwrap();

    assert!(matches!(
        result.events.as_slice(),
        [RetailReceiptEvent::Filled {
            order: OrderId(10),
            qty: 100,
            gross,
            charged,
            ..
        }] if *gross == Money::from_cents(100_000)
            && charged.commission == Money::from_cents(200)
    ));
    assert_eq!(result.seen.len(), 2);
}

#[test]
fn unordered_receipts_project_in_buy_before_sell_order() {
    let buy = fill(2, Side::Buy, 20, 100, 100_000);
    let sell = fill(1, Side::Sell, 21, 100, 100_000);
    let before = positions(100);
    let after = positions(100);
    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[sell, buy],
    })
    .unwrap();

    assert!(matches!(
        result.events.as_slice(),
        [
            RetailReceiptEvent::Filled {
                side: Side::Buy,
                ..
            },
            RetailReceiptEvent::Filled {
                side: Side::Sell,
                ..
            }
        ]
    ));
}

#[test]
fn non_fill_kinds_are_seen_and_explicitly_ignored() {
    let release = non_fill(1, ReceiptKind::Release);
    let reject = non_fill(2, ReceiptKind::Reject);
    let rollover = non_fill(3, ReceiptKind::Rollover);
    let empty = BTreeMap::new();

    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &empty,
        positions_after: &empty,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[rollover, reject, release],
    })
    .unwrap();

    assert!(result.events.is_empty());
    assert_eq!(result.seen.len(), 3);
    assert_eq!(result.retail_experience, retail().to_map());
}

#[test]
fn malformed_fill_returns_a_typed_error_without_a_partial_result() {
    let good = fill(1, Side::Buy, 1, 100, 100_000);
    let mut bad = fill(2, Side::Buy, 2, 100, 100_000);
    bad.value_after = Money::ZERO;
    let before = BTreeMap::new();
    let after = positions(200);

    let error = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[good, bad],
    })
    .unwrap_err();

    assert!(error.to_string().contains("non-positive gross"));
}

#[test]
fn conflicting_payload_for_one_receipt_identity_is_rejected() {
    let first = fill(7, Side::Buy, 10, 100, 100_000);
    let mut conflicting = first.clone();
    conflicting.charged.commission = Money::from_cents(101);
    conflicting.charged_after.commission = Money::from_cents(101);
    let before = BTreeMap::new();
    let after = positions(100);

    let error = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[first, conflicting],
    })
    .unwrap_err();

    assert_eq!(
        error,
        RetailProjectionError::ConflictingIdentity { index: 7 }
    );
}

#[test]
fn final_position_disagreement_is_a_typed_error() {
    let receipt = fill(7, Side::Buy, 10, 100, 100_000);
    let before = BTreeMap::new();
    let after = positions(99);

    let error = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[receipt],
    })
    .unwrap_err();

    assert_eq!(
        error,
        RetailProjectionError::FinalPosition {
            account: AccountId(1)
        }
    );
}

#[test]
fn independent_retail_accounts_project_identically_with_one_or_four_workers() {
    let first = fill(7, Side::Buy, 10, 100, 100_000);
    let mut second = fill(8, Side::Buy, 11, 200, 220_000);
    second.envelope.account = AccountId(2);
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(8),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let receipts = [second, first];
    let accounts = BTreeSet::from([AccountId(1), AccountId(2)]);
    let experience =
        crate::session::retail_experience_book::RetailExperienceBook::from(BTreeMap::from([
            (
                AccountId(1),
                RetailExperienceState::without_equity_reference(),
            ),
            (
                AccountId(2),
                RetailExperienceState::without_equity_reference(),
            ),
        ]));
    let before = BTreeMap::new();
    let after = BTreeMap::from([
        (AccountId(1), positions(100).remove(&AccountId(1)).unwrap()),
        (AccountId(2), positions(200).remove(&AccountId(1)).unwrap()),
    ]);
    let seen = RetailProjectionSeen::default();
    let run = |workers| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| {
                project_retail_receipts(RetailProjectionInput {
                    retail_experience: &experience,
                    retail_accounts: &accounts,
                    positions_before: &before,
                    positions_after: &after,
                    seen: &seen,
                    market_minute: 10,
                    receipts: &receipts,
                })
            })
    };
    let single = run(1).unwrap();
    assert_eq!(single, run(4).unwrap());
    assert_eq!(single.events.len(), 2);
    assert!(matches!(
        single.events[0],
        RetailReceiptEvent::Filled {
            account: AccountId(1),
            ..
        }
    ));
    assert!(matches!(
        single.events[1],
        RetailReceiptEvent::Filled {
            account: AccountId(2),
            ..
        }
    ));
    assert_eq!(single.seen.len(), 2);
}

#[test]
fn retail_processing_error_precedes_another_accounts_final_position_error() {
    let first = fill(7, Side::Buy, 10, 100, 100_000);
    let mut second = fill(8, Side::Buy, 11, 100, 100_000);
    second.envelope.account = AccountId(2);
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(8),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let accounts = BTreeSet::from([AccountId(1), AccountId(2)]);
    let before = BTreeMap::new();
    let after = BTreeMap::from([(AccountId(1), positions(99).remove(&AccountId(1)).unwrap())]);
    let error = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &accounts,
        positions_before: &before,
        positions_after: &after,
        seen: &RetailProjectionSeen::default(),
        market_minute: 10,
        receipts: &[first, second],
    })
    .unwrap_err();
    assert_eq!(
        error,
        RetailProjectionError::MissingRetailExperience {
            account: AccountId(2)
        }
    );
}
