use super::fill_fact::{derive_refs, FillFact, FillFactView};
use super::retail_projection::{
    canonical_unseen_receipts, project_retail_receipts, RetailProjectionError,
    RetailProjectionInput, RetailProjectionSeen, RetailReceiptEvent,
};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec, StepFatal,
};
use crate::{AccountId, Money, OrderId, Position, RetailExperienceState, Side, StockId};
use std::collections::{BTreeMap, BTreeSet};

fn code() -> StockId {
    StockId(0)
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
        // A buyer's ReceiptAggregation receipt may have a nominal fee greater than the actual
        // charged delta. Settlement must preserve that distinction.
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
        [Envelope::created_at_validation(
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
        ReceiptKind::PriceResolved { .. } => {
            panic!("non_fill does not construct a price resolution")
        }
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
        [Envelope::created_at_validation(
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
        [Envelope::created_at_validation(
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

fn positions(qty: u32) -> BTreeMap<AccountId, BTreeMap<StockId, Position>> {
    BTreeMap::from([(
        AccountId(1),
        BTreeMap::from([(
            code(),
            Position::from_restored_parts(qty, 0, i64::from(qty) * 1_000, 0),
        )]),
    )])
}

fn retail() -> crate::session::account_paged_map::AccountPagedMap<crate::RetailExperienceState> {
    BTreeMap::from([(
        AccountId(1),
        RetailExperienceState::without_equity_reference(),
    )])
    .into()
}

fn retail_accounts() -> BTreeSet<AccountId> {
    BTreeSet::from([AccountId(1)])
}

/// 任务65 #11 后投影是 facts 的纯函数：测试内用同一唯一派生入口构造 facts
/// （canonical-unseen 过滤仍由 canonical_unseen_receipts 承担，见重放门测试）。
fn facts(receipts: &[EnvelopeReceipt]) -> Vec<FillFact> {
    let canonical = canonical_unseen_receipts(receipts, &RetailProjectionSeen::default()).unwrap();
    derive_refs(&canonical).unwrap()
}

#[test]
fn duplicate_receipt_identity_is_seen_once_and_never_repeats_experience() {
    let receipt = fill(7, Side::Buy, 10, 100, 100_000);
    // canonical 单遍去重：同一身份只保留一条（任务65 #11 起 seen 门在 settlement 边界）。
    let batch = [receipt.clone(), receipt.clone()];
    let canonical = canonical_unseen_receipts(&batch, &RetailProjectionSeen::default()).unwrap();
    assert_eq!(canonical.len(), 1);

    let before = BTreeMap::new();
    let after = positions(100);
    let first = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        market_minute: 10,
        fill_facts: FillFactView::all(&derive_refs(&canonical).unwrap()),
    })
    .unwrap();

    assert_eq!(first.events.len(), 1);
    assert!(matches!(
        first.events.as_slice(),
        [RetailReceiptEvent::Filled { charged, .. }]
            if charged.commission == Money::from_cents(100)
    ));

    // 已消费身份再次出现时被 canonical 过滤为空批次：投影不重复经验。
    let replay_batch = [receipt];
    let replay = canonical_unseen_receipts(&replay_batch, &insert_seen_once(&canonical)).unwrap();
    assert!(replay.is_empty());
    let second = project_retail_receipts(RetailProjectionInput {
        retail_experience: &first.retail_experience.clone().into(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        market_minute: 10,
        fill_facts: FillFactView::all(&[]),
    })
    .unwrap();
    assert!(second.events.is_empty());
    assert_eq!(second.retail_experience, first.retail_experience);
}

/// 测试辅助：把 canonical 批按序插入空 seen，模拟 settlement 单遍后的重放保护。
fn insert_seen_once(canonical: &[&EnvelopeReceipt]) -> RetailProjectionSeen {
    let mut seen = RetailProjectionSeen::default();
    for receipt in canonical {
        seen.insert_test_identity(receipt.index, receipt.local_key.clone());
    }
    seen
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
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[first_receipt])),
    })
    .unwrap();

    let after_second = positions(100);
    let second = project_retail_receipts(RetailProjectionInput {
        retail_experience: &first.retail_experience.clone().into(),
        retail_accounts: &retail_accounts(),
        positions_before: &after_first,
        positions_after: &after_second,
        market_minute: 11,
        fill_facts: FillFactView::all(&facts(&[second_receipt])),
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
    let (chain_first, chain_second) = buy_fill_chain(10);
    assert_eq!(facts(&[chain_first, chain_second]).len(), 2);
}

#[test]
fn multiple_receipts_for_one_order_in_one_batch_produce_one_aggregated_experience_event() {
    let (first_leg, second_leg) = buy_fill_chain(10);
    let before = BTreeMap::new();
    let after = positions(100);
    let fill_facts = facts(&[second_leg, first_leg]);

    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &before,
        positions_after: &after,
        market_minute: 10,
        fill_facts: FillFactView::all(&fill_facts),
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
    assert_eq!(fill_facts.len(), 2);
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
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[sell, buy])),
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
fn non_fill_kinds_produce_no_fill_facts_and_are_explicitly_ignored() {
    let release = non_fill(1, ReceiptKind::Release);
    let reject = non_fill(2, ReceiptKind::Reject);
    let rollover = non_fill(3, ReceiptKind::Rollover);
    let empty = BTreeMap::new();

    // 非 Fill 收据进入 seen（重放门）但不产生 fill 事实；投影保持无事件。
    let batch = [rollover, reject, release];
    let canonical = canonical_unseen_receipts(&batch, &RetailProjectionSeen::default()).unwrap();
    assert_eq!(canonical.len(), 3);
    let fill_facts = derive_refs(&canonical).unwrap();
    assert!(fill_facts.is_empty());

    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &empty,
        positions_after: &empty,
        market_minute: 10,
        fill_facts: FillFactView::all(&fill_facts),
    })
    .unwrap();

    assert!(result.events.is_empty());
    assert_eq!(result.retail_experience, retail().to_map());
}

#[test]
fn malformed_fill_returns_a_typed_error_without_a_partial_result() {
    let good = fill(1, Side::Buy, 1, 100, 100_000);
    let mut bad = fill(2, Side::Buy, 2, 100, 100_000);
    bad.value_after = Money::ZERO;

    // 边界前移（任务65 #11）：非法腿在 fill_fact 唯一派生入口被拒，无部分结果。
    let error = derive_refs(&[&good, &bad]).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { ref description, .. }
            if description.contains("成交额非正")
    ));
}

#[test]
fn conflicting_payload_for_one_receipt_identity_is_rejected() {
    let first = fill(7, Side::Buy, 10, 100, 100_000);
    let mut conflicting = first.clone();
    conflicting.charged.commission = Money::from_cents(101);
    conflicting.charged_after.commission = Money::from_cents(101);

    // canonical 重放门仍在 settlement 边界拒绝同 index 冲突 payload。
    let batch = [first, conflicting];
    let error = canonical_unseen_receipts(&batch, &RetailProjectionSeen::default()).unwrap_err();

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
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[receipt])),
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
    let fill_facts = facts(&receipts);
    let accounts = BTreeSet::from([AccountId(1), AccountId(2)]);
    let experience = crate::session::account_paged_map::AccountPagedMap::<
        crate::RetailExperienceState,
    >::from(BTreeMap::from([
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
                    market_minute: 10,
                    fill_facts: FillFactView::all(&fill_facts),
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
    assert_eq!(fill_facts.len(), 2);
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
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[first, second])),
    })
    .unwrap_err();
    assert_eq!(
        error,
        RetailProjectionError::MissingRetailExperience {
            account: AccountId(2)
        }
    );
}

#[test]
fn retail_order_average_truncates_fractional_cents_without_changing_gross() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_099);
    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &retail(),
        retail_accounts: &retail_accounts(),
        positions_before: &BTreeMap::new(),
        positions_after: &positions(100),
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[receipt])),
    })
    .unwrap();

    assert_eq!(
        result.retail_experience[&AccountId(1)].stocks[&code()].last_buy_price,
        Some(Money::from_cents(1_000))
    );
    assert!(
        matches!(result.events.as_slice(), [RetailReceiptEvent::Filled { gross, .. }] if *gross == Money::from_cents(100_099))
    );
}

#[test]
fn retail_multiple_stock_fills_prune_only_unheld_watchlist_entries() {
    let account = AccountId(1);
    let held = StockId(1);
    let mut second = fill(2, Side::Buy, 11, 100, 100_000);
    second.envelope.stock = held.clone();
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(2),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let mut experience = RetailExperienceState::without_equity_reference();
    let unheld = (0..9)
        .map(|index| StockId(u32::MAX - 10 - index as u32))
        .collect::<Vec<_>>();
    for (index, stock) in unheld.iter().enumerate() {
        experience.observe_stock(stock, index as u64);
    }
    let untouched = StockId(2);
    experience
        .initialize_holding(
            &untouched,
            Some(Money::from_cents(900)),
            Money::from_cents(1_000),
            0,
        )
        .unwrap();
    let untouched_before = experience.stocks[&untouched].clone();
    let mut after = positions(100);
    let position = after[&account][&code()].clone();
    after
        .get_mut(&account)
        .unwrap()
        .insert(held.clone(), position.clone());
    after
        .get_mut(&account)
        .unwrap()
        .insert(untouched.clone(), position);
    let input_states = BTreeMap::from([(account, experience)]).into();
    let result = project_retail_receipts(RetailProjectionInput {
        retail_experience: &input_states,
        retail_accounts: &retail_accounts(),
        positions_before: &BTreeMap::new(),
        positions_after: &after,
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[second, fill(1, Side::Buy, 10, 100, 100_000)])),
    })
    .unwrap();

    let stocks = &result.retail_experience[&account].stocks;
    assert_eq!(stocks[&untouched], untouched_before);
    assert!(stocks.contains_key(&code()) && stocks.contains_key(&held));
    assert!(!stocks.contains_key(&unheld[0]));
    assert!(unheld[1..].iter().all(|stock| stocks.contains_key(stock)));
    assert_eq!(result.events.len(), 2);
}

#[test]
fn institutional_exit_missing_actual_fee_history_returns_no_patch() {
    let account = AccountId(1);
    let stock = code();
    let mut experience = RetailExperienceState::without_equity_reference();
    experience
        .initialize_holding(
            &stock,
            Some(Money::from_cents(1_000)),
            Money::from_cents(1_000),
            0,
        )
        .unwrap();
    let original = experience.clone();
    let experience = BTreeMap::from([(account, experience)]).into();
    let error = super::retail_projection::project_institutional_receipts(
        &crate::ComputeContext::shared_default().unwrap(),
        RetailProjectionInput {
            retail_experience: &experience,
            retail_accounts: &retail_accounts(),
            positions_before: &positions(100),
            positions_after: &BTreeMap::new(),
            market_minute: 10,
            fill_facts: FillFactView::all(&facts(&[fill(1, Side::Sell, 10, 100, 100_000)])),
        },
        crate::experience::ExperienceMoment {
            civil_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            market_minute: 10,
            trading_day: 0,
        },
    )
    .unwrap_err();

    assert!(
        matches!(error, RetailProjectionError::Experience(crate::ExperienceError::InconsistentFeedback { detail })
        if detail == "institutional exit is missing actual fee history")
    );
    assert_eq!(experience[&account], original);
}

#[test]
fn retail_sell_beyond_the_running_position_returns_no_patch() {
    let experience = retail();
    let original = experience.clone();
    let error = project_retail_receipts(RetailProjectionInput {
        retail_experience: &experience,
        retail_accounts: &retail_accounts(),
        positions_before: &positions(100),
        positions_after: &BTreeMap::new(),
        market_minute: 10,
        fill_facts: FillFactView::all(&facts(&[fill(1, Side::Sell, 10, 200, 200_000)])),
    })
    .unwrap_err();

    assert_eq!(
        error,
        RetailProjectionError::PositionTransition {
            account: AccountId(1)
        }
    );
    assert_eq!(experience, original);
}

#[test]
fn retail_zero_quantity_fill_fails_closed_at_the_derivation_boundary() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.qty_after = receipt.qty_before;
    // 边界前移（任务65 #11）：零数量 Fill 腿在唯一派生入口 fail closed，
    // 与生产路径 fill_receipt 拒绝 fill_qty == 0 的既有防御一致。
    let error = derive_refs(&[&receipt]).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { ref description, .. }
            if description.contains("数量回退或为零")
    ));
}
