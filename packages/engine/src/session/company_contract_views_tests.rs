//! 共同契约 session 视图（F 批收口）测试。
//!
//! 覆盖：`company_capabilities`（公司事实+未完成方案阶段+各行为条件+本人权利
//! 摘要，owner 隔离）、`owner_rights_offerings`（有/无具名权利、窗口前后、
//! 排队与已结算认购）、`owner_flat_withholding_receipts`（Flat 模式过滤与非
//! Flat 显式拒绝）、`company_period_explanation` 会话层委托。全部为既有事实
//! 的只读投影，不产生新状态。

use super::*;
use crate::company::capabilities::{
    ActivePlanStage, CapabilityAmount, CorporateActionKind, DistributableProfitSnapshot,
    OwnerRightsSummaryView, PaymentWindowState,
};
use crate::company::rights_offering::{DirectedPlacementTarget, RightsOfferingMode};
use crate::company::share_registry::HolderId;
use crate::orderbook::AccountId;
use super::dividend_tax_mode_tests::{
    advance_until_dividend_paid, flat_dividend_fixture, flat_fixture_setup,
};
use super::rights_offering_session_tests::{
    all_shareholders_plan, complete_day, session_with_registry,
};

/// 推进到指定自然日的日终完成**之后**（含该日日结；结束后自然日为次日）。
fn advance_through(session: &mut GameSession, date: CivilDate) {
    while session.civil_date() <= date {
        complete_day(session);
    }
}

#[test]
fn capabilities_snapshot_carries_facts_active_plan_and_owner_rights() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let total_shares = session.state.setup.stocks[0].total_shares;
    let plan = all_shareholders_plan(&issuer, &stock);
    let registered_on = plan.registered_on;
    let payment_start = plan.payment_start_on;
    let deadline = plan.payment_deadline_on;
    session.approve_rights_offering(plan).unwrap();
    // 推进到缴款期首日内（R 日已日终派发权证 → Entitled，窗口开放）。
    advance_through(&mut session, payment_start);

    let caps = session
        .company_capabilities(&issuer, AccountId(0))
        .unwrap();
    // 公司事实：总股本与注册资本（fixture 按 1 分/股绑定法定事实）。
    assert_eq!(caps.issued_shares, total_shares);
    match &caps.registered_capital {
        CapabilityAmount::Available { amount_yuan } => {
            assert_eq!(
                amount_yuan,
                &crate::accounting::AccountingAmount::from_cents(i128::from(total_shares))
                    .to_yuan_string()
            );
        }
        CapabilityAmount::Unavailable { reason } => panic!("已绑定法定事实仍不可用：{reason}"),
    }
    // 未完成方案：恰一条配股，阶段 Entitled，关键日期含登记日与缴款截止日。
    assert_eq!(caps.active_plans.len(), 1, "只应有一起未完成方案");
    let plan_view = &caps.active_plans[0];
    assert_eq!(plan_view.kind, CorporateActionKind::RightsOffering);
    assert_eq!(plan_view.identity, "rights-2030");
    assert_eq!(plan_view.stage, ActivePlanStage::Entitled);
    let labels: Vec<&str> = plan_view
        .key_dates
        .iter()
        .map(|entry| entry.label.as_str())
        .collect();
    assert!(labels.contains(&"股权登记日"));
    assert!(labels.contains(&"缴款截止日"));
    let registered = plan_view
        .key_dates
        .iter()
        .find(|entry| entry.label == "股权登记日")
        .unwrap();
    assert_eq!(registered.date, registered_on.to_iso());

    // 本人权利摘要（owner 隔离：只含 AccountId(0) 的事实）。
    assert_eq!(caps.owner_rights.len(), 1);
    let owner: &OwnerRightsSummaryView = &caps.owner_rights[0];
    assert_eq!(owner.event_id, "rights-2030");
    assert_eq!(owner.stage, ActivePlanStage::Entitled);
    assert_eq!(owner.payment_window, PaymentWindowState::Open);
    assert_eq!(owner.entitled_shares.as_deref(), Some("1500000"));
    assert!(
        owner.open_subscription_remaining.is_none(),
        "本人有具名权利时公开配售额度不适用"
    );
    let _ = deadline;

    // 五类行为条件快照顺序固定。
    let kinds: Vec<CorporateActionKind> = caps
        .action_readiness
        .iter()
        .map(|entry| entry.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            CorporateActionKind::CashDividend,
            CorporateActionKind::StockDistribution,
            CorporateActionKind::RightsOffering,
            CorporateActionKind::IssuerRepurchase,
            CorporateActionKind::ShareSplit,
        ]
    );
    let by_kind = |kind: CorporateActionKind| {
        caps.action_readiness
            .iter()
            .find(|entry| entry.kind == kind)
            .unwrap()
    };
    assert!(
        by_kind(CorporateActionKind::RightsOffering).ready,
        "开关已启用且法定事实已绑定，配股前置条件应满足"
    );
    let repurchase = by_kind(CorporateActionKind::IssuerRepurchase);
    assert!(!repurchase.ready);
    assert!(
        repurchase.blockers.iter().any(|blocker| blocker.contains("未启用")),
        "回购未启用必须在 blockers 中显式给出：{:?}",
        repurchase.blockers
    );
    // 现金分红条件与可分配利润快照自洽（不冒充）。
    let dividend = by_kind(CorporateActionKind::CashDividend);
    let distributable_positive = match &caps.distributable_profit {
        DistributableProfitSnapshot::Available {
            available_for_distribution_yuan,
            ..
        } => !available_for_distribution_yuan.starts_with('-')
            && available_for_distribution_yuan != "0.00"
            && available_for_distribution_yuan != "0",
        DistributableProfitSnapshot::Unavailable { .. } => false,
    };
    assert_eq!(dividend.ready, distributable_positive);
}

#[test]
fn capabilities_unknown_company_is_rejected() {
    let (session, _stock, _issuer) = session_with_registry(true);
    let error = session
        .company_capabilities(&CompanyId("不存在".into()), AccountId(0))
        .unwrap_err();
    assert!(error.to_string().contains("未知公司"), "须显式报错：{error}");
}

#[test]
fn owner_rights_view_reports_before_open_window_without_entitlement() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = all_shareholders_plan(&issuer, &stock);
    let payment_start = plan.payment_start_on;
    session.approve_rights_offering(plan).unwrap();
    // 批准后、R 日前：阶段 Approved、窗口未开、无权证。
    let views = session.owner_rights_offerings(AccountId(0)).unwrap();
    assert_eq!(views.len(), 1);
    let view = &views[0];
    assert_eq!(view.stage, ActivePlanStage::Approved);
    assert_eq!(view.payment_window, PaymentWindowState::BeforeOpen);
    assert!(view.owner_entitlement.is_none());
    assert!(view.queued_subscription.is_none());
    assert!(view.settled_subscription.is_none());
    let _ = payment_start;
}

#[test]
fn owner_rights_view_reports_open_headroom_when_no_named_entitlement() {
    // 定向增发只给机构 NPC 具名权利，留公开配售额度：玩家无具名权利但可按
    // 剩余公开额度申购。
    let (mut session, stock, issuer) = session_with_registry(true);
    let mut plan = all_shareholders_plan(&issuer, &stock);
    plan.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![
            DirectedPlacementTarget::NamedHolder {
                holder: HolderId::Account(AccountId(1)),
                shares: 1_000_000,
                lock_until: None,
            },
            DirectedPlacementTarget::OpenPublicSubscription { shares: 1_000_000 },
        ],
    };
    let payment_start = plan.payment_start_on;
    session.approve_rights_offering(plan).unwrap();
    advance_through(&mut session, payment_start);
    let views = session.owner_rights_offerings(AccountId(0)).unwrap();
    assert_eq!(views.len(), 1);
    let view = &views[0];
    assert_eq!(view.stage, ActivePlanStage::Entitled);
    assert_eq!(view.payment_window, PaymentWindowState::Open);
    assert!(
        view.owner_entitlement.is_none(),
        "玩家无具名权利（定向对象是机构 NPC）"
    );
    assert_eq!(
        view.open_subscription_remaining_shares.as_deref(),
        Some("1000000"),
        "公开配售剩余额度按「额度 − 已结算 − 已排队」计算"
    );
}

#[test]
fn owner_rights_view_reports_queued_then_settled_subscription_and_closed_window() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = all_shareholders_plan(&issuer, &stock);
    let payment_start = plan.payment_start_on;
    let deadline = plan.payment_deadline_on;
    session.approve_rights_offering(plan).unwrap();
    advance_through(&mut session, payment_start);
    session
        .subscribe_rights_offering("rights-2030", AccountId(0), 1_500_000)
        .unwrap();

    // 已排队未日终划扣：queued 可见（wire 为规范 u64 十进制字符串）。
    let views = session.owner_rights_offerings(AccountId(0)).unwrap();
    let view = &views[0];
    let queued = view.queued_subscription.as_ref().unwrap();
    assert_eq!(queued.requested_shares, 1_500_000);
    assert!(view.settled_subscription.is_none());

    // 过缴款截止日终后：排队转已结算认购记录，窗口关闭。
    advance_through(&mut session, deadline);
    let views = session.owner_rights_offerings(AccountId(0)).unwrap();
    let view = &views[0];
    assert_eq!(view.payment_window, PaymentWindowState::Closed);
    let settled = view.settled_subscription.as_ref().unwrap();
    assert_eq!(settled.requested_shares, 1_500_000);
    assert_eq!(settled.paid_shares, 1_500_000);
    assert!(view.queued_subscription.is_none());
}

#[test]
fn owner_rights_view_hides_settled_plans() {
    // 结算完成的配股不再进入本人权利视图（历史事实由存档与公告承载）。
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = all_shareholders_plan(&issuer, &stock);
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan).unwrap();
    advance_through(&mut session, settlement_on);
    let views = session.owner_rights_offerings(AccountId(0)).unwrap();
    assert!(
        views.is_empty(),
        "已结算方案不得继续占用本人权利视图：{views:?}"
    );
}

#[test]
fn flat_withholding_receipts_owner_query_filters_account() {
    let holders = vec![
        (AccountId(0), "player-lot", 5_u64),
        (AccountId(1), "retail-lot", 3),
    ];
    let (mut session, _player_gross, _retail_gross, _inst_gross, _external_gross) =
        flat_dividend_fixture(flat_fixture_setup(1000), 10, &holders);
    advance_until_dividend_paid(&mut session);
    let receipts = session
        .owner_flat_withholding_receipts(AccountId(0))
        .unwrap();
    assert_eq!(receipts.len(), 1, "owner 隔离只返回本人回执");
    let receipt = &receipts[0];
    assert_eq!(receipt.account, AccountId(0));
    assert_eq!(receipt.gross.cents(), 50);
    assert_eq!(receipt.withheld.cents(), 5);
    assert_eq!(receipt.rate_bp, 1000);
}

#[test]
fn flat_withholding_receipts_rejects_non_flat_modes() {
    // AShareIndividual 模式无简税代扣回执概念：显式拒绝，不冒充空台账。
    let (session, _stock, _issuer) = session_with_registry(true);
    let error = session
        .owner_flat_withholding_receipts(AccountId(0))
        .unwrap_err();
    assert!(
        error.to_string().contains("FlatWithholding"),
        "非简税模式须显式指明：{error}"
    );
}

#[test]
fn session_period_explanation_delegates_to_company_system() {
    let (session, _stock, issuer) = session_with_registry(true);
    // 开局前史月末（结算月度制）：解释可读；未来期间按业务条件拒绝。
    let last_settled = CivilDate::from_iso("2029-12-31").unwrap();
    let explanation = session
        .company_period_explanation(&issuer, last_settled)
        .unwrap();
    assert_eq!(
        explanation.cycle,
        crate::company::simple::period::SettlementCycle::Monthly
    );
    let error = session
        .company_period_explanation(&issuer, CivilDate::from_iso("2031-01-31").unwrap())
        .unwrap_err();
    assert!(error.to_string().contains("尚未结算"), "须指明未结算：{error}");
}
