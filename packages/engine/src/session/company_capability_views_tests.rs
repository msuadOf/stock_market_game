//! B4e 只读能力及本人配股查询：账簿投影、边界与不改写存档。
include!("../../test-support/simple_company.rs");
use super::*;
use crate::company::capabilities::{ActivePlanStage, PaymentWindowState};
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::rights_offering::*;
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::{HotParams, InstParams, RetailParams};

fn plain_setup() -> SessionSetup {
    SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600101".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 100_000,
            float_shares: 100_000,
        }],
        npcs: NpcSetup {
            retail_count: 4,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: crate::CivilDate::from_iso("2030-06-03").unwrap(),
        simulation_policy_id: crate::SIMULATION_POLICY_ID.to_owned(),
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
    }
}

/// 含公司配置的新局 setup：Flat 简税 1000bp + 面值 1 元 + 回购启用 +
/// 开局自动装配公司行为基础。
fn company_setup() -> SessionSetup {
    SessionSetup {
        company_system: Some(simple_company_fixture!(crate; ["600101"])),
        dividend_tax_mode: Some(CashDividendTaxMode::FlatWithholding),
        flat_withholding_bp: Some(1000),
        rights_offering_enabled: false,
        issuer_repurchase_enabled: true,
        par_value_per_share: Some(Money::from_cents(100)),
        auto_corporate_foundation: true,
        report_frequency: Some(crate::information::ReportFrequency::Quarterly),
        ..plain_setup()
    }
}

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}
fn session() -> GameSession {
    GameSession::new(company_setup(), 71).unwrap()
}
fn issuer(session: &GameSession) -> CompanyId {
    session
        .company_system()
        .unwrap()
        .issuers()
        .id_of_code("C-600101")
        .unwrap()
}
fn book(company: CompanyId, event: &str, public: bool) -> RightsOfferingBook {
    let mode = if public {
        RightsOfferingMode::DirectedPlacement {
            targets: vec![
                DirectedPlacementTarget::NamedHolder {
                    holder: HolderId::Account(AccountId(1)),
                    shares: 10,
                    lock_until: None,
                },
                DirectedPlacementTarget::OpenPublicSubscription { shares: 20 },
            ],
        }
    } else {
        RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        }
    };
    let plan = RightsOfferingEventPlan {
        event_id: event.into(),
        approval_reference: "显式董事会决议".into(),
        issuer: company,
        stock: StockCode(
            if company == CompanyId(0) {
                "600101"
            } else {
                "600889"
            }
            .into(),
        ),
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
    };
    let holding = |account, qty| ShareHolding {
        holder: HolderId::Account(AccountId(account)),
        lots: vec![ShareLot {
            id: format!("opening-{account}"),
            qty,
            acquired_on: date("2020-01-02"),
            source: AcquisitionSource::InitialAllocation {
                evidence: "开局分配".into(),
            },
            restriction: ShareRestriction::Unrestricted,
        }],
    };
    let mut registry = ShareRegistry::new(
        plan.stock.clone(),
        company,
        200,
        plan.registered_on,
        vec![holding(1, 100), holding(2, 100)],
    )
    .unwrap();
    let snapshot = registry
        .register(event.into(), plan.registered_on)
        .unwrap()
        .clone();
    let mut book = RightsOfferingBook::new(plan).unwrap();
    book.announce(book.plan().announced_on).unwrap();
    book.entitle(
        snapshot,
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .unwrap();
    book
}
/// wrapper 测试从实际 Session 名册登记，发行人、证券、总股本与全部账户持仓一致。
fn install(session: &mut GameSession, book: RightsOfferingBook) {
    let plan = book.plan().clone();
    let actions = session.state.corporate_actions.as_mut().unwrap();
    let registry = actions
        .registries
        .iter_mut()
        .find(|registry| registry.stock() == &plan.stock)
        .unwrap();
    registry
        .close_day(crate::company::share_registry::ShareDayRequest {
            event_id: format!("projection-day:{}", plan.event_id),
            day: plan.registered_on,
            scope: crate::company::share_registry::MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    let snapshot = registry
        .register(plan.event_id.clone(), plan.registered_on)
        .unwrap()
        .clone();
    let mut installed = RightsOfferingBook::new(plan).unwrap();
    installed.announce(installed.plan().announced_on).unwrap();
    installed
        .entitle(
            snapshot,
            &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        )
        .unwrap();
    actions.rights_offerings.push(installed);
}

#[test]
fn capability_views_reflect_current_settlement_limit_and_do_not_mutate_save() {
    let mut session = session();
    let company = issuer(&session);
    install(&mut session, book(company, "rights-b4e", false));
    let before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let view = session
        .company_capabilities(&company, AccountId(1))
        .unwrap();
    assert!(!view.cash_settlement, "当前尚未接入真实投资者结算");
    assert!(view.unsupported_reason.contains("待接线"));
    assert_eq!(view.issued_shares, 100_000);
    assert_eq!(view.active_plans.len(), 1);
    assert_eq!(view.active_plans[0].stage, ActivePlanStage::Entitled);
    let actual = session.corporate_actions().unwrap().rights_offerings[0]
        .entitlement()
        .unwrap()
        .entitlements
        .iter()
        .find(|entry| entry.holder == HolderId::Account(AccountId(1)))
        .unwrap()
        .rights_shares;
    assert_eq!(
        view.owner_rights[0].entitled_shares.as_deref(),
        Some(actual.to_string().as_str())
    );
    assert_eq!(view.action_readiness.len(), 5);
    assert!(
        view.action_readiness
            .iter()
            .all(|r| !r.ready && !r.blockers.is_empty()),
        "未接命令/结算不能冒充可受理"
    );
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        before
    );
}

#[test]
fn capability_views_reject_unknown_account_company_and_absent_company_line() {
    let session = session();
    let company = issuer(&session);
    assert!(session
        .company_capabilities(&company, AccountId(u64::MAX))
        .unwrap_err()
        .to_string()
        .contains("账户"));
    assert!(session
        .company_capabilities(&CompanyId(u32::MAX), AccountId(0))
        .unwrap_err()
        .to_string()
        .contains("未知公司"));
    assert!(session.owner_rights_offerings(AccountId(u64::MAX)).is_err());
    let plain = GameSession::new(plain_setup(), 71).unwrap();
    assert!(plain
        .company_capabilities(&company, AccountId(0))
        .unwrap_err()
        .to_string()
        .contains("未配置公司"));
    assert!(plain
        .owner_rights_offerings(AccountId(0))
        .unwrap_err()
        .to_string()
        .contains("未配置公司"));
    assert!(plain.corporate_actions().is_none());
}

#[test]
fn capability_views_keep_owner_and_issuer_scopes_separate() {
    let mut setup = company_setup();
    let mut second = setup.stocks[0].clone();
    second.code = StockCode("600889".into());
    setup.stocks.push(second);
    setup.company_system = Some(simple_company_fixture!(crate; ["600101", "600889"]));
    let mut session = GameSession::new(setup, 71).unwrap();
    let company = issuer(&session);
    let other = session
        .company_system()
        .unwrap()
        .issuers()
        .id_of_code("C-600889")
        .unwrap();
    install(&mut session, book(company, "own-company", true));
    install(&mut session, book(other, "other-company", true));
    let before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let own = session
        .company_capabilities(&company, AccountId(1))
        .unwrap();
    assert_eq!(
        own.owner_rights.len(),
        1,
        "公司能力不泄漏其他发行人的本人权利"
    );
    assert_eq!(own.active_plans.len(), 1);
    let non_owner = session.owner_rights_offerings(AccountId(2)).unwrap();
    assert!(non_owner.iter().all(|v| v.owner_entitlement.is_none()));
    assert!(non_owner
        .iter()
        .all(|v| v.open_subscription_remaining_shares.as_deref() == Some("20")));
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        before
    );
}

#[test]
fn capability_views_project_window_endpoints_and_owner_queue_only() {
    let mut actions = corporate_actions::SessionCorporateActions::default();
    let b = book(CompanyId(0), "public", true);
    actions.rights_offerings.push(b);
    actions
        .rights_subscription_queue
        .push(corporate_actions::QueuedRightsSubscription {
            event_id: "public".into(),
            account: AccountId(2),
            requested_shares: 7,
            submitted_on: date("2030-06-04"),
        });
    for (day, expected) in [
        ("2030-06-03", PaymentWindowState::BeforeOpen),
        ("2030-06-04", PaymentWindowState::Open),
        ("2030-06-11", PaymentWindowState::Open),
        ("2030-06-12", PaymentWindowState::Closed),
    ] {
        let views = company_capability_views::owner_rights_views(&actions, AccountId(2), date(day))
            .unwrap();
        assert_eq!(views[0].payment_window, expected);
        assert_eq!(
            views[0].open_subscription_remaining_shares.as_deref(),
            Some("13")
        );
        assert_eq!(
            views[0]
                .queued_subscription
                .as_ref()
                .unwrap()
                .requested_shares,
            7
        );
        let other = company_capability_views::owner_rights_views(&actions, AccountId(1), date(day))
            .unwrap();
        assert!(other[0].queued_subscription.is_none());
        assert!(other[0].open_subscription_remaining_shares.is_none());
    }
}

#[test]
fn capability_views_project_recorded_paid_and_waived_shares_and_reject_overdraw() {
    let mut actions = corporate_actions::SessionCorporateActions::default();
    let mut b = book(CompanyId(0), "public", true);
    b.record_subscription(RightsSubscriptionRecord {
        holder: HolderId::Account(AccountId(2)),
        requested_shares: 8,
        price_per_share: Money::from_cents(800),
        submitted_on: date("2030-06-04"),
        origin: SubscriptionOrigin::Explicit,
        paid_shares: 5,
        paid_amount: Money::from_cents(4000),
        waived_shares: 3,
    })
    .unwrap();
    actions.rights_offerings.push(b);
    let v =
        company_capability_views::owner_rights_views(&actions, AccountId(2), date("2030-06-05"))
            .unwrap();
    let paid = v[0].settled_subscription.as_ref().unwrap();
    assert_eq!(
        (paid.requested_shares, paid.paid_shares, paid.waived_shares),
        (8, 5, 3)
    );
    assert_eq!(
        v[0].open_subscription_remaining_shares.as_deref(),
        Some("12")
    );
    actions
        .rights_subscription_queue
        .push(corporate_actions::QueuedRightsSubscription {
            event_id: "public".into(),
            account: AccountId(3),
            requested_shares: 13,
            submitted_on: date("2030-06-04"),
        });
    assert!(company_capability_views::owner_rights_views(
        &actions,
        AccountId(2),
        date("2030-06-05")
    )
    .is_err());
}

#[test]
fn capability_views_distinguish_unregistered_zero_public_and_settled_books() {
    let b = book(CompanyId(0), "unregistered", false);
    let approved = RightsOfferingBook::new(b.plan().clone()).unwrap();
    let mut actions = corporate_actions::SessionCorporateActions::default();
    actions.rights_offerings.push(approved);
    actions.rights_offerings[0]
        .announce(date("2030-06-02"))
        .unwrap();
    let before =
        company_capability_views::owner_rights_views(&actions, AccountId(2), date("2030-06-03"))
            .unwrap();
    assert!(before[0].owner_entitlement.is_none());
    assert!(before[0].open_subscription_remaining_shares.is_none());
    actions.rights_offerings[0] = b;
    let no_public =
        company_capability_views::owner_rights_views(&actions, AccountId(0), date("2030-06-04"))
            .unwrap();
    assert!(no_public[0].owner_entitlement.is_none());
    assert!(
        no_public[0].open_subscription_remaining_shares.is_none(),
        "无公开额度不等于公开额度已耗尽"
    );
    let b = &mut actions.rights_offerings[0];
    b.close_payment_window(date("2030-06-11")).unwrap();
    b.settle(
        date("2030-06-13"),
        RightsSettlementReceipt {
            event_id: "unregistered".into(),
            settlement_on: date("2030-06-13"),
            failed: true,
            total_paid_shares: 0,
            total_paid_amount: Money::ZERO,
            refunded_total: Money::ZERO,
            holders: vec![],
        },
    )
    .unwrap();
    assert!(company_capability_views::owner_rights_views(
        &actions,
        AccountId(2),
        date("2030-06-13")
    )
    .unwrap()
    .is_empty());
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(0)).is_empty());
}

#[test]
fn capability_views_keep_money_share_units_and_numeric_issuer_on_wire() {
    let mut actions = corporate_actions::SessionCorporateActions::default();
    actions
        .rights_offerings
        .push(book(CompanyId(0), "wire", true));
    let views =
        company_capability_views::owner_rights_views(&actions, AccountId(1), date("2030-06-04"))
            .unwrap();
    let wire = serde_json::to_value(&views[0]).unwrap();
    assert_eq!(wire["issuer"], serde_json::json!(0));
    assert_eq!(wire["price_per_share"], serde_json::json!("800"));
    assert_eq!(
        wire["owner_entitlement"]["rights_shares"],
        serde_json::json!("10")
    );
    let roundtrip: OwnerRightsOfferingView = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(roundtrip, views[0]);
    let mut invalid = wire;
    invalid["owner_entitlement"]["rights_shares"] = serde_json::json!(10);
    assert!(serde_json::from_value::<OwnerRightsOfferingView>(invalid).is_err());
}

/// 独立领域 fixture：只校验账簿状态投影，不声称执行了 Session 权威结算。
fn snapshot_for(event: &str) -> crate::company::share_registry::RegistrationSnapshot {
    let mut registry = ShareRegistry::new(
        StockCode("600101".into()),
        CompanyId(0),
        200,
        date("2030-06-03"),
        (1..=2)
            .map(|account| ShareHolding {
                holder: HolderId::Account(AccountId(account)),
                lots: vec![ShareLot {
                    id: format!("lot-{account}"),
                    qty: 100,
                    acquired_on: date("2020-01-02"),
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "投影夹具".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            })
            .collect(),
    )
    .unwrap();
    registry
        .register(event.into(), date("2030-06-03"))
        .unwrap()
        .clone()
}

#[test]
fn capability_views_project_dividend_partial_payment_and_paid_terminal() {
    use crate::company::cash_dividend::{CashDividendBook, CashDividendPlan, HolderPaymentOutcome};
    use crate::company::ex_reference_price::CashDividendFormula;
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let plan = CashDividendPlan::new(
        "cash-view".into(),
        CompanyId(0),
        StockCode("600101".into()),
        crate::calendar::CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-01"),
        date("2030-06-02"),
        date("2030-06-03"),
        date("2030-06-04"),
        date("2030-06-05"),
        Money::from_cents(2),
        Money::from_cents(400),
        &calendar,
    )
    .unwrap();
    let mut actions = corporate_actions::SessionCorporateActions::default();
    actions.dividends.push(CashDividendBook::new(plan).unwrap());
    assert_eq!(
        company_capability_views::active_plan_views(&actions, &CompanyId(0))[0].stage,
        ActivePlanStage::Approved
    );
    let b = &mut actions.dividends[0];
    b.announce(date("2030-06-02")).unwrap();
    b.register(snapshot_for("cash-view"), &calendar).unwrap();
    b.mark_payable(date("2030-06-05")).unwrap();
    b.settle(
        "partial".into(),
        date("2030-06-05"),
        vec![
            HolderPaymentOutcome::Paid {
                holder: HolderId::Account(AccountId(1)),
                amount: Money::from_cents(200),
            },
            HolderPaymentOutcome::Failed {
                holder: HolderId::Account(AccountId(2)),
                reason: "结算通道拒绝".into(),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        company_capability_views::active_plan_views(&actions, &CompanyId(0))[0].stage,
        ActivePlanStage::PartiallyPaid
    );
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(1)).is_empty());
    actions.dividends[0]
        .settle(
            "retry".into(),
            date("2030-06-06"),
            vec![HolderPaymentOutcome::Paid {
                holder: HolderId::Account(AccountId(2)),
                amount: Money::from_cents(200),
            }],
        )
        .unwrap();
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(0)).is_empty());
}

#[test]
fn capability_views_project_distribution_split_and_terminal_filters() {
    use crate::company::share_split::{ShareSplitBook, ShareSplitDirection, ShareSplitEventPlan};
    use crate::company::stock_distribution::{
        StockDistributionBook, StockDistributionEventPlan, StockDistributionKind,
    };
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let mut actions = corporate_actions::SessionCorporateActions::default();
    actions.stock_distributions.push(
        StockDistributionBook::new(StockDistributionEventPlan {
            event_id: "stock-view".into(),
            approval_reference: "股东决议".into(),
            issuer: CompanyId(0),
            stock: StockCode("600101".into()),
            exchange: crate::calendar::CalendarExchange::Sse,
            kind: StockDistributionKind::BonusShares,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            registered_on: date("2030-06-03"),
            ex_rights_on: date("2030-06-04"),
            shares_per_existing_share_micros: 100_000,
            approved_total_new_shares: 20,
        })
        .unwrap(),
    );
    actions.share_splits.push(
        ShareSplitBook::new(ShareSplitEventPlan {
            event_id: "split-view".into(),
            approval_reference: "股东决议".into(),
            issuer: CompanyId(0),
            stock: StockCode("600101".into()),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            registered_on: date("2030-06-03"),
            ex_rights_on: date("2030-06-04"),
        })
        .unwrap(),
    );
    for b in &mut actions.stock_distributions {
        b.announce(date("2030-06-02")).unwrap();
        b.register(snapshot_for("stock-view"), &calendar).unwrap();
    }
    for b in &mut actions.share_splits {
        b.announce(date("2030-06-02")).unwrap();
        b.register(snapshot_for("split-view"), &calendar).unwrap();
    }
    let views = company_capability_views::active_plan_views(&actions, &CompanyId(0));
    assert_eq!(views.len(), 2);
    assert!(views.iter().all(|v| v.stage == ActivePlanStage::Registered));
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(1)).is_empty());
    actions.stock_distributions[0]
        .mark_credited(date("2030-06-04"))
        .unwrap();
    actions.share_splits[0]
        .mark_settled(date("2030-06-04"))
        .unwrap();
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(0)).is_empty());
}

#[test]
fn capability_views_keep_completed_repurchase_until_cancelled() {
    use crate::company::issuer_repurchase::{
        IssuerRepurchaseBook, IssuerRepurchasePlan, RepurchaseCompletionPolicy,
        RepurchaseFillRecord, RepurchasePurpose,
    };
    let mut actions = corporate_actions::SessionCorporateActions::default();
    actions.issuer_repurchases.push(
        IssuerRepurchaseBook::new(IssuerRepurchasePlan {
            event_id: "repurchase-view".into(),
            approval_reference: "董事会决议".into(),
            issuer: CompanyId(0),
            stock: StockCode("600101".into()),
            exchange: crate::calendar::CalendarExchange::Sse,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            window_start_on: date("2030-06-03"),
            window_deadline_on: date("2030-06-11"),
            price_cap_per_share: Money::from_cents(1000),
            total_budget: Money::from_cents(10000),
            max_shares: 10,
            purpose: RepurchasePurpose::ReduceCapital,
            completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
            approval_unit_event_id: None,
        })
        .unwrap(),
    );
    actions.issuer_repurchases[0]
        .announce(date("2030-06-02"))
        .unwrap();
    actions.issuer_repurchases[0]
        .record_fill(
            RepurchaseFillRecord {
                stock: StockCode("600101".into()),
                day: date("2030-06-03"),
                shares: 10,
                gross: Money::from_cents(9000),
                fees: Money::from_cents(10),
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        company_capability_views::active_plan_views(&actions, &CompanyId(0))[0].stage,
        ActivePlanStage::Executing
    );
    actions.issuer_repurchases[0]
        .complete(date("2030-06-03"), Money::from_cents(990))
        .unwrap();
    assert_eq!(
        company_capability_views::active_plan_views(&actions, &CompanyId(0))[0].stage,
        ActivePlanStage::Completed
    );
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(1)).is_empty());
    actions.issuer_repurchases[0]
        .record_cancellation(date("2030-06-04"), 10, &[])
        .unwrap();
    assert!(company_capability_views::active_plan_views(&actions, &CompanyId(0)).is_empty());
}
