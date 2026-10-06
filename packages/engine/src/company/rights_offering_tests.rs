//! 配股／增发基础测试：权证分配（碎权证降序＋同额 seed 洗牌）、方案日程校验、
//! 除权公式配股价分量与认购策略枚举的未实现边界（2026-10-07 M 批）。

use super::{
    allocate_rights_entitlements, DirectedPlacementTarget, HolderRightsSettlement,
    RightsEntitlement, RightsOfferingBook, RightsOfferingError, RightsOfferingEventPlan,
    RightsOfferingMode, RightsOfferingStatus, RightsSettlementReceipt, RightsSubscriptionRecord,
    RightsSubscriptionStrategy, SubscriptionOrigin,
};
use crate::company::{
    share_registry::{
        AcquisitionSource, HolderId, IssuerRepurchaseAccountFacts, RegistrationSnapshot,
        ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    },
    CompanyId,
};
use crate::{account::StockCode, calendar::CivilDate, money::Money, orderbook::AccountId};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.to_owned(),
        qty,
        acquired_on: date("2020-01-02"),
        source: AcquisitionSource::InitialAllocation {
            evidence: format!("opening-{id}"),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

fn snapshot(holdings: Vec<ShareHolding>, issued_shares: u64) -> crate::company::share_registry::RegistrationSnapshot {
    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId("issuer-1".to_owned()),
        issued_shares,
        date("2030-06-03"),
        holdings,
    )
    .unwrap();
    registry
        .register("rights-1".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone()
}

fn rights_plan(mode: RightsOfferingMode) -> RightsOfferingEventPlan {
    RightsOfferingEventPlan {
        event_id: "rights-1".to_owned(),
        approval_reference: "board-resolution-1".to_owned(),
        issuer: CompanyId("issuer-1".to_owned()),
        stock: StockCode("600001".to_owned()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date("2030-06-01"),
        announced_on: date("2030-06-02"),
        registered_on: date("2030-06-03"),
        payment_start_on: date("2030-06-04"),
        payment_deadline_on: date("2030-06-11"),
        ex_rights_on: date("2030-06-12"),
        settlement_on: date("2030-06-13"),
        listing_on: date("2030-06-13"),
        price_per_share: Money::from_cents(800),
        mode,
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    }
}

/// 每 10 股配 3 股：比例 300_000 micros。
fn all_shareholders_ratio(micros: u64) -> RightsOfferingMode {
    RightsOfferingMode::RightsToAllShareholders {
        shares_per_existing_share_micros: micros,
    }
}

#[test]
fn rights_entitlements_exclude_issuer_treasury_and_use_micro_ratio() {
    let record_snapshot = snapshot(
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("a1", 10)],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(2)),
                lots: vec![lot("a2", 10)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![lot("treasury", 5)],
            },
        ],
        25,
    );
    let plan = rights_plan(all_shareholders_ratio(300_000));
    let receipt = allocate_rights_entitlements(&record_snapshot, &plan, 7).unwrap();
    assert_eq!(receipt.issuer_treasury_shares_excluded, 5);
    assert_eq!(receipt.planned_total_rights_shares, 6);
    assert_eq!(
        receipt.entitlements,
        vec![
            RightsEntitlement {
                holder: HolderId::Account(AccountId(1)),
                rights_shares: 3,
                lock_until: None,
            },
            RightsEntitlement {
                holder: HolderId::Account(AccountId(2)),
                rights_shares: 3,
                lock_until: None,
            },
        ],
        "回购专户（IssuerTreasury）股份不享有配售权（沪市指南第 2.4 节）"
    );
    assert_eq!(receipt.open_subscription_shares, 0);
}

#[test]
fn fractional_rights_are_awarded_descending_with_seeded_tie_break() {
    // 7 名持有人各 1 股、每 10 股配 3 股（0.3 股/股）：整数为 0、碎片 300_000；
    // 拟配售 3 股按碎权证降序（同额）＋seed 洗牌各登记 1 份（深市 2.4.4（二））。
    let record_snapshot = snapshot(
        (1..=7)
            .map(|id| ShareHolding {
                holder: HolderId::Account(AccountId(id)),
                lots: vec![lot(&format!("a{id}"), 1)],
            })
            .collect(),
        7,
    );
    let plan = rights_plan(all_shareholders_ratio(300_000));
    let receipt = allocate_rights_entitlements(&record_snapshot, &plan, 42).unwrap();
    let mut winners: Vec<_> = receipt
        .entitlements
        .iter()
        .map(|entitlement| entitlement.holder.clone())
        .collect();
    winners.sort();
    assert_eq!(winners.len(), 3, "只补足 3 份碎权证");
    let repeated = allocate_rights_entitlements(&record_snapshot, &plan, 42).unwrap();
    assert_eq!(
        receipt.entitlements, repeated.entitlements,
        "同 seed 重放分配可复现"
    );
}

#[test]
fn directed_placement_entitlements_follow_explicit_company_targets() {
    let record_snapshot = snapshot(
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![lot("a1", 100)],
        }],
        100,
    );
    let lock_until = date("2031-01-02");
    let plan = rights_plan(RightsOfferingMode::DirectedPlacement {
        targets: vec![
            DirectedPlacementTarget::NamedHolder {
                holder: HolderId::Account(AccountId(2)),
                shares: 40,
                lock_until: Some(lock_until),
            },
            DirectedPlacementTarget::NamedHolder {
                holder: HolderId::External("anchor-investor".to_owned()),
                shares: 30,
                lock_until: None,
            },
            DirectedPlacementTarget::OpenPublicSubscription { shares: 30 },
        ],
    });
    let receipt = allocate_rights_entitlements(&record_snapshot, &plan, 5).unwrap();
    assert_eq!(receipt.planned_total_rights_shares, 100);
    assert_eq!(receipt.open_subscription_shares, 30);
    assert_eq!(
        receipt.entitlements,
        vec![
            RightsEntitlement {
                holder: HolderId::Account(AccountId(2)),
                rights_shares: 40,
                lock_until: Some(lock_until),
            },
            RightsEntitlement {
                holder: HolderId::External("anchor-investor".to_owned()),
                rights_shares: 30,
                lock_until: None,
            },
        ],
        "定向名单按显式对象生成权利（HolderId 排序稳定）；公开配售部分只保留额度"
    );
}

#[test]
fn unimplemented_npc_strategy_is_rejected_at_plan_validation() {
    let mut plan = rights_plan(all_shareholders_ratio(300_000));
    plan.npc_subscription_strategy = RightsSubscriptionStrategy::StrategyBased;
    assert!(
        matches!(
            plan.validate(),
            Err(RightsOfferingError::SubscriptionStrategyUnimplemented { .. })
        ),
        "策略选择器中未实现的策略值必须在受理时显式拒绝，不静默降级"
    );
    plan.npc_subscription_strategy = RightsSubscriptionStrategy::FullByDefault;
    plan.validate().unwrap();
}

#[test]
fn calendar_validation_rejects_non_trading_and_misordered_schedule() {
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    // 2030-06-07 是周五（交易日）；把 L 改到 2030-06-08（周六，非交易日）。
    let mut plan = rights_plan(all_shareholders_ratio(300_000));
    plan.payment_deadline_on = date("2030-06-08");
    assert!(matches!(
        plan.validate_calendar(&calendar),
        Err(RightsOfferingError::InvalidTradingDate { .. })
    ));
    // 除权日必须为 L 次一交易日；划款/入账日为除权日次一交易日。
    let mut plan = rights_plan(all_shareholders_ratio(300_000));
    plan.ex_rights_on = date("2030-06-13");
    assert!(matches!(
        plan.validate_calendar(&calendar),
        Err(RightsOfferingError::InvalidExRightsDate { .. })
    ));
    let mut plan = rights_plan(all_shareholders_ratio(300_000));
    plan.settlement_on = date("2030-06-14");
    plan.listing_on = date("2030-06-14");
    assert!(matches!(
        plan.validate_calendar(&calendar),
        Err(RightsOfferingError::InvalidSettlementDate { .. })
    ));
    rights_plan(all_shareholders_ratio(300_000))
        .validate_calendar(&calendar)
        .unwrap();
}

#[test]
fn derive_schedule_from_registration_and_payment_days_matches_guide_defaults() {
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let mut plan = rights_plan(all_shareholders_ratio(300_000));
    RightsOfferingEventPlan::derive_schedule(
        &mut plan,
        &calendar,
        crate::calendar::CalendarExchange::Sse,
        5,
    )
    .unwrap();
    assert_eq!(plan.registered_on, date("2030-06-03"));
    assert_eq!(plan.payment_start_on, date("2030-06-04"));
    // 默认模拟日历下 06-05 休市、06-08/09 周末：5 个交易日为 04、06、07、10、11
    // → L=06-11、除权 L+1=06-12、划款/入账 L+2=06-13。
    assert_eq!(plan.payment_deadline_on, date("2030-06-11"));
    assert_eq!(plan.ex_rights_on, date("2030-06-12"));
    assert_eq!(plan.settlement_on, date("2030-06-13"));
    assert_eq!(plan.listing_on, date("2030-06-13"));
    plan.validate_calendar(&calendar).unwrap();
}

#[test]
fn repurchase_account_facts_are_frozen_into_the_entitlement_receipt() {
    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId("issuer-1".to_owned()),
        10,
        date("2030-06-03"),
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![lot("a1", 10)],
        }],
    )
    .unwrap();
    registry
        .set_issuer_repurchase_account(IssuerRepurchaseAccountFacts {
            account_reference: "repurchase-account-1".to_owned(),
            source_evidence: "board-resolution-1".to_owned(),
            established_on: date("2030-06-01"),
        })
        .unwrap();
    let record_snapshot = registry
        .register("rights-1".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone();
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let receipt = allocate_rights_entitlements(&record_snapshot, &plan, 3).unwrap();
    assert_eq!(receipt.issuer_treasury_shares_excluded, 0);
    assert_eq!(receipt.planned_total_rights_shares, 5);
}

// ==== 认购与结算状态机 ====

fn book_with_entitlement(
    plan: RightsOfferingEventPlan,
    snapshot: &RegistrationSnapshot,
) -> RightsOfferingBook {
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let mut book = RightsOfferingBook::new(plan).unwrap();
    book.announce(book.plan().announced_on).unwrap();
    book.entitle(snapshot.clone(), &calendar).unwrap();
    book
}

fn record(account: u64, requested: u64, paid: u64) -> RightsSubscriptionRecord {
    RightsSubscriptionRecord {
        holder: HolderId::Account(AccountId(account)),
        requested_shares: requested,
        price_per_share: Money::from_cents(800),
        submitted_on: date("2030-06-04"),
        origin: SubscriptionOrigin::NpcFullByDefault,
        paid_shares: paid,
        paid_amount: Money::from_cents(800 * paid as i64),
        waived_shares: requested - paid,
    }
}

/// 两户各 5 股、每 10 股配 5 股（50% 上限）：每户 2.5 → floor 2 + 聚合 5，
/// 同额碎权证随机 1 户 +1；户 1 足额认购、户 2 只缴 1 股（现金不足弃配）。
fn subscribed_book() -> RightsOfferingBook {
    let record_snapshot = snapshot(
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("a1", 5)],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(2)),
                lots: vec![lot("a2", 5)],
            },
        ],
        10,
    );
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let mut book = book_with_entitlement(plan, &record_snapshot);
    assert_eq!(book.entitlement().unwrap().planned_total_rights_shares, 5);
    let rights_of = |account: u64| {
        book.entitlement()
            .unwrap()
            .entitlements
            .iter()
            .find(|entry| entry.holder == HolderId::Account(AccountId(account)))
            .unwrap()
            .rights_shares
    };
    let (rights_1, rights_2) = (rights_of(1), rights_of(2));
    assert_eq!(rights_1 + rights_2, 5);
    book.record_subscription(record(1, rights_1, rights_1))
        .unwrap();
    book.record_subscription(record(2, rights_2, 1)).unwrap();
    book
}

#[test]
fn payment_window_subscription_and_close_state_machine() {
    let mut book = subscribed_book();
    assert_eq!(book.status(), &RightsOfferingStatus::Entitled);
    // 窗口日外拒绝。
    let mut late = record(3, 1, 1);
    late.holder = HolderId::Account(AccountId(1));
    late.submitted_on = date("2030-06-20");
    assert!(book.record_subscription(late).is_err());
    // 超权利认购拒绝。
    let mut over = record(1, 99, 99);
    assert!(book.record_subscription(over).is_err());
    // 关窗（L=2030-06-11）。
    book.close_payment_window(date("2030-06-11")).unwrap();
    assert_eq!(book.status(), &RightsOfferingStatus::Closed);
    book.close_payment_window(date("2030-06-11")).unwrap();
    // 关窗后不再受理。
    assert!(book
        .record_subscription(record(1, 1, 1))
        .is_err());
}

#[test]
fn seventy_percent_failure_is_determined_from_paid_shares_only() {
    let book = subscribed_book();
    // 期望值由实际认购计算（paid×10 与 planned×7 的精确整数比较），不依赖洗牌走向。
    let paid: u64 = book.subscriptions().iter().map(|row| row.paid_shares).sum();
    let planned = book.entitlement().unwrap().planned_total_rights_shares;
    assert_eq!(
        book.determine_failure().unwrap(),
        paid * 10 < planned * 7,
        "70% 判定必须只看已缴款认购数"
    );
    // 全额认购必成功。
    let record_snapshot = snapshot(
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("a1", 5)],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(2)),
                lots: vec![lot("a2", 5)],
            },
        ],
        10,
    );
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let mut book = book_with_entitlement(plan, &record_snapshot);
    for account in [1_u64, 2] {
        let rights = book
            .entitlement()
            .unwrap()
            .entitlements
            .iter()
            .find(|entry| entry.holder == HolderId::Account(AccountId(account)))
            .unwrap()
            .rights_shares;
        book.record_subscription(record(account, rights, rights)).unwrap();
    }
    assert!(!book.determine_failure().unwrap(), "全额认购（5/5）必须成功");
    // 认购不足：单户 5 股权利只缴 1 股（1×10 < 5×7）→ 失败。
    let record_snapshot = snapshot(
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![lot("a1", 10)],
        }],
        10,
    );
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let mut book = book_with_entitlement(plan, &record_snapshot);
    assert_eq!(book.entitlement().unwrap().planned_total_rights_shares, 5);
    book.record_subscription(record(1, 5, 1)).unwrap();
    assert!(book.determine_failure().unwrap());
}

fn settlement_from_book(book: &RightsOfferingBook) -> RightsSettlementReceipt {
    let failed = book.determine_failure().unwrap();
    let mut holders = Vec::new();
    let mut total_shares = 0_u64;
    let mut total_amount = Money::ZERO;
    let mut refunded_total = Money::ZERO;
    for subscription in book.subscriptions() {
        let refunded = if failed {
            subscription.paid_amount
        } else {
            Money::ZERO
        };
        total_shares += subscription.paid_shares;
        total_amount = total_amount.add(subscription.paid_amount).unwrap();
        refunded_total = refunded_total.add(refunded).unwrap();
        holders.push(HolderRightsSettlement {
            holder: subscription.holder.clone(),
            paid_shares: subscription.paid_shares,
            paid_amount: subscription.paid_amount,
            waived_shares: subscription.waived_shares,
            refunded_amount: refunded,
        });
    }
    RightsSettlementReceipt {
        event_id: book.plan().event_id.clone(),
        settlement_on: book.plan().settlement_on,
        failed,
        total_paid_shares: total_shares,
        total_paid_amount: total_amount,
        refunded_total,
        holders,
    }
}

#[test]
fn settle_records_success_and_rejects_incoherent_refunds() {
    // 成功路径用全额认购构造，确定性成立。
    let record_snapshot = snapshot(
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("a1", 5)],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(2)),
                lots: vec![lot("a2", 5)],
            },
        ],
        10,
    );
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let mut book = book_with_entitlement(plan, &record_snapshot);
    for account in [1_u64, 2] {
        let rights = book
            .entitlement()
            .unwrap()
            .entitlements
            .iter()
            .find(|entry| entry.holder == HolderId::Account(AccountId(account)))
            .unwrap()
            .rights_shares;
        book.record_subscription(record(account, rights, rights)).unwrap();
    }
    book.close_payment_window(date("2030-06-11")).unwrap();
    let receipt = settlement_from_book(&book);
    assert!(!receipt.failed);
    book.settle(date("2030-06-13"), receipt.clone()).unwrap();
    assert_eq!(book.status(), &RightsOfferingStatus::Settled);
    book.mark_credited(date("2030-06-13")).unwrap();
    // 重复结算拒绝（错误而非静默）。
    assert!(book.settle(date("2030-06-13"), receipt).is_err());
    book.validate().unwrap();

    // 失败结算必须全额退款且不再入账。
    let record_snapshot = snapshot(
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![lot("a1", 10)],
        }],
        10,
    );
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let mut book = book_with_entitlement(plan, &record_snapshot);
    book.record_subscription(record(1, 5, 1)).unwrap();
    book.close_payment_window(date("2030-06-11")).unwrap();
    let receipt = settlement_from_book(&book);
    assert!(receipt.failed, "认购不足 70% 的结算回执必须标记失败");
    let mut wrong = receipt.clone();
    wrong.holders[0].refunded_amount = Money::ZERO;
    assert!(
        book.settle(date("2030-06-13"), wrong).is_err(),
        "失败退款必须等于已缴款"
    );
    book.settle(date("2030-06-13"), receipt).unwrap();
    assert!(
        book.mark_credited(date("2030-06-13")).is_err(),
        "失败发行不产生新股"
    );
    book.validate().unwrap();
}

#[test]
fn book_restored_from_serialized_state_replays_identical_facts() {
    // 成功路径（全额认购）保证结算不失败，覆盖 credited 事实的序列化往返。
    let record_snapshot = snapshot(
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("a1", 5)],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(2)),
                lots: vec![lot("a2", 5)],
            },
        ],
        10,
    );
    let plan = rights_plan(all_shareholders_ratio(500_000));
    let mut book = book_with_entitlement(plan, &record_snapshot);
    for account in [1_u64, 2] {
        let rights = book
            .entitlement()
            .unwrap()
            .entitlements
            .iter()
            .find(|entry| entry.holder == HolderId::Account(AccountId(account)))
            .unwrap()
            .rights_shares;
        book.record_subscription(record(account, rights, rights)).unwrap();
    }
    book.close_payment_window(date("2030-06-11")).unwrap();
    let receipt = settlement_from_book(&book);
    book.settle(date("2030-06-13"), receipt).unwrap();
    book.mark_credited(date("2030-06-13")).unwrap();
    let json = serde_json::to_string(&book).unwrap();
    let restored: RightsOfferingBook = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, book, "严格 JSON 往返保持账簿深等");
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    restored.validate_with_calendar(&calendar).unwrap();
    // 篡改认购划扣额后恢复必须显式失败（聚合校验拒绝不一致事实）。
    let tampered = json.replace("\"paid_shares\":\"3\"", "\"paid_shares\":\"2\"");
    if tampered != json {
        assert!(serde_json::from_str::<RightsOfferingBook>(&tampered).is_err());
    }
}
