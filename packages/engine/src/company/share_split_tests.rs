use super::{
    allocate_share_split, HolderSplitOutcome, ShareSplitBook, ShareSplitDirection, ShareSplitError,
    ShareSplitEventPlan, ShareSplitStatus,
};
use crate::company::{
    share_registry::{
        AcquisitionSource, HolderId, RegistrationSnapshot, ShareHolding, ShareLot, ShareRegistry,
        ShareRestriction,
    },
    CompanyId,
};
use crate::{account::StockCode, calendar::CivilDate, orderbook::AccountId};

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

fn snapshot(holdings: Vec<ShareHolding>, issued_shares: u64) -> RegistrationSnapshot {
    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId(0),
        issued_shares,
        date("2030-06-03"),
        holdings,
    )
    .unwrap();
    registry
        .register("split-1".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone()
}

fn holding(holder: HolderId, lots: Vec<ShareLot>) -> ShareHolding {
    ShareHolding { holder, lots }
}

fn plan(direction: ShareSplitDirection, ratio: u64) -> ShareSplitEventPlan {
    ShareSplitEventPlan {
        event_id: "split-1".to_owned(),
        approval_reference: "shareholders-resolution-1".to_owned(),
        issuer: CompanyId(0),
        stock: StockCode("600001".to_owned()),
        exchange: crate::calendar::CalendarExchange::Sse,
        direction,
        ratio,
        approved_on: date("2030-06-01"),
        announced_on: date("2030-06-02"),
        registered_on: date("2030-06-03"),
        ex_rights_on: date("2030-06-04"),
    }
}

fn holder_outcome<'a>(
    receipt: &'a super::ShareSplitReceipt,
    holder: &HolderId,
) -> &'a HolderSplitOutcome {
    receipt
        .holders
        .iter()
        .find(|outcome| &outcome.holder == holder)
        .unwrap_or_else(|| panic!("holder {holder:?} missing from allocation"))
}

#[test]
fn split_multiplies_every_holder_including_issuer_treasury_without_fragments() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 3)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 7)]),
            // 拆股／缩股不是权益分派：回购专户按同一比例换算（不排除）。
            holding(HolderId::IssuerTreasury, vec![lot("treasury", 2)]),
        ],
        12,
    );

    let receipt = allocate_share_split(&snapshot, &plan(ShareSplitDirection::Split, 10), 17)
        .expect("integer split allocation");

    assert_eq!(receipt.issued_shares_before, 12);
    assert_eq!(receipt.issued_shares_after, 120);
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(1))).new_shares,
        30
    );
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(2))).new_shares,
        70
    );
    assert_eq!(
        holder_outcome(&receipt, &HolderId::IssuerTreasury).new_shares,
        20
    );
    assert!(receipt
        .holders
        .iter()
        .all(|outcome| { outcome.fractional_numerator == 0 && outcome.tie_break_award == 0 }));
}

#[test]
fn consolidation_floors_each_holder_and_awards_aggregate_whole_shares_by_remainder() {
    // 3 + 3 + 2 = 8 股，2 并 1：基准 1 + 1 + 1，汇总目标 4 → extra 1；
    // 余数 1 = 1 > 0，同为 1 的两户由 seed 洗牌决出多得 1 股的一户。
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 3)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 3)]),
            holding(HolderId::Account(AccountId(3)), vec![lot("a3", 2)]),
        ],
        8,
    );

    let receipt =
        allocate_share_split(&snapshot, &plan(ShareSplitDirection::Consolidate, 2), 17).unwrap();

    assert_eq!(receipt.issued_shares_before, 8);
    assert_eq!(receipt.issued_shares_after, 4);
    assert_eq!(
        receipt
            .holders
            .iter()
            .map(|outcome| outcome.fractional_numerator)
            .collect::<Vec<_>>(),
        vec![1, 1, 0]
    );
    let awarded = receipt
        .holders
        .iter()
        .filter(|outcome| outcome.tie_break_award == 1)
        .count();
    assert_eq!(awarded, 1);
    assert_eq!(
        receipt
            .holders
            .iter()
            .map(|outcome| outcome.new_shares)
            .sum::<u64>(),
        4
    );
    // 余数为零的持有人不参与整股奖励（碎股分子为零不进候选）。
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(3))).tie_break_award,
        0
    );
}

#[test]
fn consolidation_remainder_tie_break_is_repeatable_per_seed() {
    let snapshot = snapshot(
        (1..=8)
            .map(|id| {
                holding(
                    HolderId::Account(AccountId(id)),
                    vec![lot(&format!("a{id}"), 1)],
                )
            })
            .collect(),
        8,
    );
    let plan = plan(ShareSplitDirection::Consolidate, 4);
    // 8 股 4 并 1：基准全 0、余数全 1，汇总目标 2 → extra 2。
    let first = allocate_share_split(&snapshot, &plan, 42).unwrap();
    let repeated = allocate_share_split(&snapshot, &plan, 42).unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.issued_shares_after, 2);
    assert_eq!(
        first
            .holders
            .iter()
            .filter(|outcome| outcome.tie_break_award == 1)
            .count(),
        2
    );
    let different_seed = allocate_share_split(&snapshot, &plan, 43).unwrap();
    assert_ne!(first.holders, different_seed.holders);
    assert_eq!(different_seed.issued_shares_after, 2);
}

#[test]
fn allocation_rejects_snapshot_identity_mismatch_and_bad_ratio() {
    let snapshot = snapshot(
        vec![holding(HolderId::Account(AccountId(1)), vec![lot("a1", 4)])],
        4,
    );
    let mut wrong_stock = plan(ShareSplitDirection::Split, 2);
    wrong_stock.stock = StockCode("000002".to_owned());
    assert!(matches!(
        allocate_share_split(&snapshot, &wrong_stock, 1),
        Err(ShareSplitError::SnapshotMismatch { .. })
    ));
    let mut ratio_one = plan(ShareSplitDirection::Split, 1);
    ratio_one.ratio = 1;
    assert!(matches!(
        allocate_share_split(&snapshot, &ratio_one, 1),
        Err(ShareSplitError::InvalidPlan { .. })
    ));
}

#[test]
fn book_state_machine_advances_and_replays_restored_receipt() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 5)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 4)]),
        ],
        9,
    );
    let mut book = ShareSplitBook::new(plan(ShareSplitDirection::Consolidate, 3)).unwrap();
    assert_eq!(*book.status(), ShareSplitStatus::Approved);
    book.announce(date("2030-06-02")).unwrap();
    assert_eq!(*book.status(), ShareSplitStatus::Announced);
    // 登记在公告前被拒绝。
    let mut premature = book.clone();
    premature.status = ShareSplitStatus::Approved;
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    assert!(premature.register(snapshot.clone(), &calendar).is_err());
    let receipt = book
        .register(snapshot, &calendar)
        .expect("registration freezes allocation")
        .clone();
    // 9 股 3 并 1 → 3 股；5→1 余 2，4→1 余 1；extra = 3 − 2 = 1 给余数最大者。
    assert_eq!(receipt.issued_shares_after, 3);
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(1))).tie_break_award,
        1
    );
    assert_eq!(*book.status(), ShareSplitStatus::Registered);
    // 恢复（serde 往返 + validate）必须重放出同一回执。
    let restored: ShareSplitBook = serde_json::from_str(&serde_json::to_string(&book).unwrap())
        .expect("serialized book round-trips");
    assert_eq!(restored.receipt(), Some(&receipt));
    // 早于除权日结算被拒绝。
    assert!(book.mark_settled(date("2030-06-03")).is_err());
    book.mark_settled(date("2030-06-04")).unwrap();
    assert_eq!(*book.status(), ShareSplitStatus::Settled);
    let settled: ShareSplitBook =
        serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
    assert_eq!(settled.settled_on(), Some(date("2030-06-04")));
    // 篡改回执在恢复校验中被拒。
    let mut tampered = book.clone();
    if let Some(mut_receipt) = tampered.receipt.as_mut() {
        mut_receipt.issued_shares_after += 1;
    }
    assert!(tampered.validate().is_err());
}

#[test]
fn restored_book_without_registration_serializes_strictly() {
    let book = ShareSplitBook::new(plan(ShareSplitDirection::Split, 2)).unwrap();
    let serialized = serde_json::to_string(&book).unwrap();
    let restored: ShareSplitBook = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored, book);
    // 严格持久化：未知字段显式拒绝。
    let mut injected = serde_json::from_str::<serde_json::Value>(&serialized).unwrap();
    injected
        .as_object_mut()
        .unwrap()
        .insert("surprise".into(), serde_json::json!(1));
    assert!(serde_json::from_str::<ShareSplitBook>(&injected.to_string()).is_err());
}

/// Treasury 分桶换算（Task174）：拆股逐桶精确放大；缩股共享汇总目标（商+余数
/// 降序+seed 洗牌发奖励），Σ桶 == 汇总 new_shares，不逐桶独立 floor。
#[test]
fn treasury_buckets_split_exactly_and_consolidation_shares_the_target() {
    use super::ShareSplitDirection as Dir;
    use super::{allocate_treasury_buckets, TreasuryBucket};
    let buckets = vec![
        TreasuryBucket {
            key: "initial".into(),
            shares: 9_999_000,
        },
        TreasuryBucket {
            key: "plan-a".into(),
            shares: 60,
        },
        TreasuryBucket {
            key: "plan-b".into(),
            shares: 100,
        },
    ];
    // 拆股 2:1：逐桶 ×2，Σ == 汇总。
    let split = allocate_treasury_buckets(&buckets, &Dir::Split, 2, 19_998_320, 7).unwrap();
    assert_eq!(split, vec![19_998_000, 120, 200]);
    // 缩股 7:1：base = [1_428_428, 8, 14]；Σrem = 4+4+2 = 10 ≥ 7 → 恰一次 +1
    // 落在余数并列最高的 initial/plan-a 之一（seed 决定），Σ == 汇总目标。
    let total = 9_999_160_u64;
    let target = total / 7;
    let consolidated =
        allocate_treasury_buckets(&buckets, &Dir::Consolidate, 7, target, 42).unwrap();
    assert_eq!(consolidated[2], 14, "余 2 的桶不得获奖");
    let awarded = (consolidated[0] == 1_428_429) as u64 + (consolidated[1] == 9) as u64;
    assert_eq!(awarded, 1, "恰一个最高余数桶获得 +1");
    assert_eq!(
        consolidated.iter().sum::<u64>(),
        target,
        "分桶合计必须等于汇总目标"
    );
    // 同输入同 seed 确定性复算。
    let again = allocate_treasury_buckets(&buckets, &Dir::Consolidate, 7, target, 42).unwrap();
    assert_eq!(consolidated, again);
    // Σbase 超过汇总目标（数量篡改）拒绝。
    assert!(allocate_treasury_buckets(&buckets, &Dir::Consolidate, 7, 1_428_449, 42).is_err());
}

#[test]
fn allocation_rejects_zero_total_but_allows_an_account_to_receive_zero() {
    let plan = plan(ShareSplitDirection::Consolidate, 3);
    for (second, expected_total) in [(1, None), (2, Some(1)), (3, Some(1)), (5, Some(2))] {
        let snapshot = snapshot(
            vec![
                holding(HolderId::Account(AccountId(1)), vec![lot("a1", 1)]),
                holding(HolderId::Account(AccountId(2)), vec![lot("a2", second)]),
            ],
            1 + second,
        );
        match expected_total {
            None => {
                let allocation = allocate_share_split(&snapshot, &plan, 17);
                let mut book = ShareSplitBook::new(plan.clone()).unwrap();
                book.announce(plan.announced_on).unwrap();
                let before = book.clone();
                let calendar =
                    crate::calendar::TradingCalendar::current_default_calendar().unwrap();
                let registered = book.register(snapshot, &calendar).map(|_| ());
                assert_eq!((allocation.is_err(), registered.is_err()), (true, true));
                assert_eq!(
                    allocation.unwrap_err(),
                    ShareSplitError::ZeroIssuedShares {
                        event_id: "split-1".into(),
                        stock: StockCode("600001".into()),
                        direction: ShareSplitDirection::Consolidate,
                        ratio: 3,
                        issued_shares_before: 2,
                        issued_shares_after: 0,
                    }
                );
                assert_eq!(book, before, "失败不得冻结 registration 或发布成功回执");
            }
            Some(total) => {
                let receipt = allocate_share_split(&snapshot, &plan, 17).unwrap();
                assert_eq!(receipt.issued_shares_after, total);
                assert_eq!(
                    holder_outcome(&receipt, &HolderId::Account(AccountId(1))).new_shares,
                    0
                );
            }
        }
    }
}

/// 构造修复前可保存的 Registered 事实，不能依赖修复后的受理入口制造反例。
fn legacy_zero_book(snapshot: RegistrationSnapshot, plan: ShareSplitEventPlan) -> ShareSplitBook {
    let receipt = super::ShareSplitReceipt {
        event_id: plan.event_id.clone(),
        approval_reference: plan.approval_reference.clone(),
        registration_event_id: snapshot.event_id().to_owned(),
        stock: plan.stock.clone(),
        issuer: plan.issuer,
        registered_on: plan.registered_on,
        direction: plan.direction.clone(),
        ratio: plan.ratio,
        issued_shares_before: snapshot.issued_shares(),
        issued_shares_after: 0,
        tie_break_seed: super::tie_break_seed(&plan.event_id),
        holders: snapshot
            .holdings()
            .iter()
            .map(|holding| {
                let original_shares = holding.lots.iter().map(|lot| lot.qty).sum();
                HolderSplitOutcome {
                    holder: holding.holder.clone(),
                    original_shares,
                    new_shares: 0,
                    fractional_numerator: original_shares % plan.ratio,
                    tie_break_award: 0,
                    original_lots: holding.lots.clone(),
                }
            })
            .collect(),
    };
    ShareSplitBook {
        plan,
        status: ShareSplitStatus::Registered,
        registration: Some(snapshot),
        receipt: Some(receipt),
        settled_on: None,
    }
}

#[test]
fn registered_zero_total_is_rejected_by_validation_and_strict_json() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 1)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 1)]),
        ],
        2,
    );
    let book = legacy_zero_book(snapshot, plan(ShareSplitDirection::Consolidate, 3));
    assert!(book.validate().is_err());
    assert!(
        serde_json::from_str::<ShareSplitBook>(&serde_json::to_string(&book).unwrap()).is_err()
    );
}

mod whole_save {
    use super::*;
    use crate::session::*;
    use crate::{GameConfig, HotParams, InstParams, Money, RetailParams, StrategyParams};
    include!("../../test-support/simple_company.rs");

    fn session() -> (GameSession, ShareRegistry) {
        let mut session = GameSession::new(
            SessionSetup {
                stocks: vec![StockSpec {
                    code: StockCode("600001".into()),
                    exchange: StockExchange::Shanghai,
                    initial_price: Money::from_cents(1_000),
                    category: SecurityCategory::MainBoard,
                    limit_pct: 0.10,
                    tick: Money::from_cents(1),
                    total_shares: 100_000,
                    float_shares: 0,
                }],
                npcs: NpcSetup {
                    retail_count: 1,
                    inst_count: 0,
                    hot_count: 0,
                    retail_cash_median: Money::from_cents(10_000_000),
                },
                config: GameConfig::proposed_defaults(),
                strategy_params: StrategyParams {
                    retail: RetailParams {
                        arrival_rate: 0.0,
                        order_size_mean: 100,
                        chase_prob: 0.0,
                    },
                    inst: InstParams {
                        margin: 0.05,
                        order_size: 100,
                    },
                    hot: HotParams {
                        lookback: 2,
                        trend_threshold: 0.01,
                        order_size: 100,
                    },
                },
                company_operations: None,
                groups: Vec::new(),
                ticks_per_day: 1,
                auction_ticks: 0,
                closing_auction_ticks: 0,
                history_len: 2,
                t1_enabled: true,
                float_allocation: FloatAllocation::random(),
                start_date: date("2030-06-03"),
                simulation_policy_id: SIMULATION_POLICY_ID.to_owned(),
                runtime_sync_policy: Default::default(),
                company_system: Some(simple_company_fixture!(crate; ["600001"])),
                dividend_tax_mode: Some(
                    crate::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
                ),
                flat_withholding_bp: Some(1000),
                rights_offering_enabled: false,
                issuer_repurchase_enabled: false,
                par_value_per_share: Some(Money::from_cents(100)),
                auto_corporate_foundation: true,
                report_frequency: Some(crate::information::ReportFrequency::Quarterly),
            },
            71,
        )
        .unwrap();
        let system = std::sync::Arc::make_mut(session.state.companies.system_arc_mut().unwrap());
        system
            .declare_share_split(
                &CompanyId(0),
                crate::company::share_split::ShareSplitDeclaration {
                    event_id: "split-1".into(),
                    approval_reference: "shareholders-resolution-1".into(),
                    direction: ShareSplitDirection::Consolidate,
                    approved_on: date("2030-06-01"),
                    ratio: 2,
                    par_value_before: Money::from_cents(100),
                    par_value_after: Money::from_cents(200),
                    registered_capital_at_approval:
                        crate::accounting::AccountingAmount::from_cents(10_000_000),
                },
            )
            .unwrap();

        let registry_before = session
            .save()
            .unwrap()
            .company_state
            .actions()
            .unwrap()
            .registries[0]
            .clone();
        // 先完成真实 R 日日终；零流通股与零到达率保证当日没有成交确认。
        session.step().unwrap();
        session.end_civil_day().unwrap();
        (session, registry_before)
    }

    fn save_with_split_capture(
        session: &GameSession,
        registry_before: &ShareRegistry,
    ) -> SaveSlot {
        use crate::company::share_registry::settlement_day::{
            ConfirmedShareMarketClose, ShareMarketSource, ShareMarketSourceContext,
            SharePostAction, ShareSettlementDay,
        };
        use crate::company::share_registry::MovementScope;

        let mut save = session.save().unwrap();
        let settled_registry = save.company_state.actions().unwrap().registries[0].clone();
        let market_receipt = settled_registry
            .receipts()
            .iter()
            .find(|receipt| {
                matches!(
                    receipt.request.scope,
                    MovementScope::SettlementDayMarket { .. }
                )
            })
            .expect("真实 R 日日结应保留 typed 市场回执")
            .clone();
        let MovementScope::SettlementDayMarket { binding } = &market_receipt.request.scope else {
            unreachable!()
        };
        assert!(binding.opening.is_empty());
        assert!(binding.post.is_empty());
        assert!(matches!(
            &binding.market_source,
            ShareMarketSource::FinalConfirmations { receipts } if receipts.is_empty()
        ));
        assert!(market_receipt
            .request
            .changes
            .iter()
            .all(|change| change.change == 0));

        let mut market_request = market_receipt.request.clone();
        market_request.scope = MovementScope::PublicMarket;
        let day = market_request.day;
        let settlement = ShareSettlementDay {
            stock: settled_registry.stock().clone(),
            day,
            predecessor: binding.predecessor.clone(),
            opening: vec![],
            actual_market_close: ConfirmedShareMarketClose {
                unit: binding.market_unit.clone(),
                source: binding.market_source.clone(),
                request: market_request,
            },
            post_market_actions: vec![SharePostAction::CaptureRegistration {
                event_id: "split-1".into(),
            }],
        };
        let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
        let captured_registry = registry_before
            .prepare_settlement_day(
                &settlement,
                &ShareMarketSourceContext {
                    calendar: &calendar,
                    exchange: crate::calendar::CalendarExchange::Sse,
                    confirmations: &[],
                    treasury_account: None,
                    completed_through: Some(day),
                },
            )
            .unwrap();
        save.company_state
            .actions_for_test_mut()
            .registries[0] = captured_registry;
        save
    }

    #[test]
    fn whole_save_rejects_unexecuted_zero_receipt_without_changing_positive_issuer() {
        let (session, registry_before) = session();
        let original_live = serde_json::to_vec(&session.save().unwrap()).unwrap();
        let mut save = save_with_split_capture(&session, &registry_before);
        let original_synthetic = serde_json::to_vec(&save).unwrap();
        let actions = save.company_state.actions_for_test_mut();
        let registry = &mut actions.registries[0];
        assert_eq!(registry.issued_shares(), 100_000);
        // 使用夹具中真实 typed settlement 已登记的快照，避免手工补造冻结事实。
        let snapshot = registry
            .registration("split-1")
            .expect("typed settlement capture must freeze the split registration")
            .clone();
        actions.share_splits.push(legacy_zero_book(
            snapshot,
            plan(ShareSplitDirection::Consolidate, 100_001),
        ));
        let edited = serde_json::to_vec(&save).unwrap();
        let restored = GameSession::restore(&save);
        let decoded = serde_json::from_slice::<SaveSlot>(&edited);
        assert_eq!(
            (restored.is_err(), decoded.is_err()),
            (true, true),
            "内存 SaveSlot 与完整 JSON 均不得导入 Registered 零发行量回执"
        );
        let error = restored.err().unwrap();
        let cause = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<ShareSplitError>()
            .unwrap();
        assert_eq!(
            cause,
            &ShareSplitError::ZeroIssuedShares {
                event_id: "split-1".into(),
                stock: StockCode("600001".into()),
                direction: ShareSplitDirection::Consolidate,
                ratio: 100_001,
                issued_shares_before: 100_000,
                issued_shares_after: 0,
            }
        );
        assert_eq!(
            save.company_state.actions().unwrap().registries[0].issued_shares(),
            100_000
        );
        assert_eq!(serde_json::to_vec(&save).unwrap(), edited);
        assert_eq!(
            serde_json::to_vec(&session.save().unwrap()).unwrap(),
            original_live
        );
        assert_ne!(original_synthetic, edited);
        assert!(session.poison_reason().is_none());
    }

    #[test]
    fn whole_save_preserves_nonzero_registered_consolidation() {
        let (session, registry_before) = session();
        let mut save = save_with_split_capture(&session, &registry_before);
        let actions = save.company_state.actions_for_test_mut();
        let snapshot = actions.registries[0]
            .registration("split-1")
            .expect("typed settlement capture must freeze the split registration")
            .clone();
        let mut book = ShareSplitBook::new(plan(ShareSplitDirection::Consolidate, 2)).unwrap();
        book.announce(date("2030-06-02")).unwrap();
        book.register(
            snapshot,
            &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        )
        .unwrap();
        actions.share_splits.push(book);
        let parsed: SaveSlot = serde_json::from_slice(&serde_json::to_vec(&save).unwrap()).unwrap();
        let restored = GameSession::restore(&parsed).unwrap();
        assert_eq!(
            restored.corporate_actions().unwrap().share_splits,
            save.company_state.actions().unwrap().share_splits.clone()
        );
    }
}
