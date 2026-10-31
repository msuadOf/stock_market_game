//! APPROVAL-C 真实 Session 首红；InternalScenario 仅显式测试来源，不走玩家意图注入。
use super::issuer_repurchase_approval::RepurchaseApprovalSource;
use super::*;
use crate::company::issuer_repurchase::{
    IssuerRepurchasePlan, RepurchaseCompletionPolicy, RepurchasePurpose,
};
use std::sync::Arc;
include!("../../test-support/simple_company.rs");
fn session(enabled: bool) -> GameSession {
    session_at(enabled, None)
}
fn session_at(enabled: bool, start: Option<CivilDate>) -> GameSession {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.company_system = Some(simple_company_fixture!(crate; ["000001", "300001"]));
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.issuer_repurchase_enabled = enabled;
    setup.auto_corporate_foundation = true;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    if let Some(start) = start {
        setup.start_date = start;
    }
    GameSession::new_with_runtime_groups(setup, 531, 2).unwrap()
}
fn plan(session: &GameSession, event: &str) -> IssuerRepurchasePlan {
    let day = session.civil_date();
    IssuerRepurchasePlan {
        event_id: event.into(),
        approval_reference: format!("批准-{event}"),
        issuer: crate::company::CompanyId(0),
        stock: crate::account::StockCode("000001".into()),
        exchange: crate::calendar::CalendarExchange::Szse,
        approved_on: day,
        announced_on: day,
        window_start_on: day,
        window_deadline_on: day,
        price_cap_per_share: Money::from_cents(1000),
        total_budget: Money::from_cents(100_000),
        max_shares: 1000,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
        approval_unit_event_id: None,
    }
}
fn treasury(session: &GameSession) -> AccountId {
    session
        .account_roster()
        .into_iter()
        .find(|id| session.account(*id).unwrap().kind() == crate::AccountKind::IssuerRepurchase)
        .unwrap()
}
// 仅测试取证启用外部 Web 回写；严格读取完整公司状态，并与同次真实生产候选逐字段比较。
fn assert_web_companies_readback(session: &GameSession, variable: &str) {
    if let Some(path) = std::env::var_os(variable) {
        let bytes = std::fs::read(path).expect("Web 回写公司 JSON 必须存在且可读");
        let decoded: SavedSessionCompanies =
            serde_json::from_slice(&bytes).expect("Web 回写公司状态必须通过 Rust strict serde");
        assert_eq!(
            decoded,
            session.state.companies.save(),
            "Web 往返必须保留完整真实公司状态及回购 binding"
        );
        eprintln!("APPROVAL_C_WEB_READBACK_OK={variable}");
    }
}
#[test]
fn approval_c_session_01_return_means_actual_credit_book_finance_and_next_start_ready() {
    let mut session = session(true);
    let plan = plan(&session, "批准一");
    let owner = treasury(&session);
    let before_tick = session.tick();
    let receipt = session
        .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    let RepurchaseApprovalReceipt::Approved {
        event_id,
        plan_id,
        funding_amount,
    } = receipt
    else {
        panic!("首次批准必须返回 Approved")
    };
    assert_eq!(event_id, plan.event_id);
    assert_eq!(plan_id.owner, owner);
    assert_eq!(plan_id.sequence, 0);
    assert_eq!(funding_amount, plan.total_budget);
    assert_eq!(session.account(owner).unwrap().cash(), plan.total_budget);
    let binding = session
        .issuer_repurchase_execution_binding(&plan.event_id)
        .unwrap();
    assert_eq!(binding.event_id, plan.event_id);
    assert_eq!(binding.issuer, plan.issuer);
    assert_eq!(binding.stock, plan.stock);
    assert_eq!(binding.account, owner);
    assert_eq!(binding.plan_id, plan_id);
    assert!(binding.funding_committed);
    assert_eq!(binding.funding_amount, plan.total_budget);
    assert_eq!(binding.applied_fact_cursor, 0);
    assert_eq!(
        binding.closing_state,
        crate::company::issuer_repurchase::RepurchaseBindingClosingState::Active
    );
    let books = &session.corporate_actions().unwrap().issuer_repurchases;
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].plan(), &plan);
    assert!(matches!(
        session.runtime_plans_snapshot().entry(plan_id).unwrap(),
        crate::plans::PlanEntry::IssuerRepurchase(_)
    ));
    let finance = session
        .company_system()
        .unwrap()
        .issuer_repurchase_facts(&plan.issuer)
        .unwrap();
    assert_eq!(finance.len(), 1);
    assert_eq!(finance[0].event_id, plan.event_id);
    assert_eq!(
        finance[0].synthetic_funding.to_money().unwrap(),
        plan.total_budget
    );
    // 同次真实成功输出供 Web 严格契约取证；不是手工拼接的 DTO。
    eprintln!(
        "APPROVAL_C_FUNDED_COMPANIES_WIRE={}",
        serde_json::to_string(&session.state.companies.save()).unwrap()
    );
    assert_web_companies_readback(&session, "STOCK_GAME_APPROVAL_C_FUNDED_WEB_READBACK");
    assert_eq!(session.tick(), before_tick);
    assert!(session.runtime_driver_for_test().is_some());
    session.step().unwrap();
    assert_eq!(session.account(owner).unwrap().cash(), plan.total_budget);
}
#[test]
fn approval_c_session_02_same_event_same_input_returns_committed_identity_without_second_credit() {
    let mut session = session(true);
    let plan = plan(&session, "幂等");
    let owner = treasury(&session);
    let first = session
        .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    let RepurchaseApprovalReceipt::Approved { plan_id, .. } = first else {
        panic!("首次批准")
    };
    let cash = session.account(owner).unwrap().cash();
    let revision = session.external_revision();
    let book = session.runtime_plans_snapshot().clone();
    assert_eq!(
        session
            .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
            .unwrap(),
        RepurchaseApprovalReceipt::AlreadyApproved {
            event_id: plan.event_id,
            plan_id,
            funding_amount: plan.total_budget
        }
    );
    assert_eq!(session.account(owner).unwrap().cash(), cash);
    assert_eq!(session.external_revision(), revision);
    assert_eq!(session.runtime_plans_snapshot(), &book);
}
#[test]
fn approval_c_session_03_same_event_different_input_is_typed_conflict_without_mutation() {
    let mut session = session(true);
    let mut plan = plan(&session, "异输入");
    session
        .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    let hash = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    plan.total_budget = Money::from_cents(100_001);
    let error = session
        .approve_issuer_repurchase(plan, RepurchaseApprovalSource::InternalScenario)
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::RepurchaseApproval(RepurchaseApprovalError::EventConflict { .. })
        ),
        "{error}"
    );
    assert_eq!(session.business_state_hash().unwrap(), hash);
    assert_eq!(session.external_revision(), revision);
}
#[test]
fn approval_c_session_04_same_stock_new_event_returns_actual_conflict_id_and_zero_extra_funding() {
    let mut session = session(true);
    let first = plan(&session, "甲");
    let second = plan(&session, "乙");
    let receipt = session
        .approve_issuer_repurchase(first, RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    let RepurchaseApprovalReceipt::Approved { plan_id, .. } = receipt else {
        panic!("首次批准")
    };
    let hash = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    let error = session
        .approve_issuer_repurchase(second, RepurchaseApprovalSource::InternalScenario)
        .unwrap_err();
    assert!(
        matches!(error, SessionError::RepurchaseApproval(RepurchaseApprovalError::PlanConflict { existing_plan }) if existing_plan == plan_id),
        "{error}"
    );
    assert_eq!(session.business_state_hash().unwrap(), hash);
    assert_eq!(session.external_revision(), revision);
}
#[test]
fn approval_c_session_05_requester_permission_precedes_already_approved_idempotence() {
    let mut session = session(true);
    let plan = plan(&session, "权限");
    session
        .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    assert!(
        session
            .account(AccountId(0))
            .unwrap()
            .position(&StockId(0))
            .is_none(),
        "测试玩家确实没有该股当前持仓"
    );
    let hash = session.business_state_hash().unwrap();
    let error = session
        .approve_issuer_repurchase(
            plan,
            RepurchaseApprovalSource::Player {
                requester: AccountId(0),
            },
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::RepurchaseApproval(RepurchaseApprovalError::InsufficientHolding {
                requester: AccountId(0),
                ..
            })
        ),
        "{error}"
    );
    assert_eq!(session.business_state_hash().unwrap(), hash);
}
#[test]
fn approval_c_session_06_disabled_command_is_typed_and_does_not_construct_runtime() {
    let mut session = session(false);
    let plan = plan(&session, "关闭");
    let hash = session.business_state_hash().unwrap();
    let error = session
        .approve_issuer_repurchase(plan, RepurchaseApprovalSource::InternalScenario)
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::RepurchaseApproval(RepurchaseApprovalError::Disabled)
        ),
        "{error}"
    );
    assert_eq!(session.business_state_hash().unwrap(), hash);
    assert!(session.runtime_driver_for_test().is_none());
    // 禁用机制仍须输出严格必填的空 binding 集合，保留实际 CompanySystem 原件。
    eprintln!(
        "APPROVAL_C_DISABLED_COMPANIES_WIRE={}",
        serde_json::to_string(&session.state.companies.save()).unwrap()
    );
    assert_web_companies_readback(&session, "STOCK_GAME_APPROVAL_C_DISABLED_WEB_READBACK");
}
#[test]
fn approval_c_session_07_player_current_total_holding_includes_frozen_shares() {
    let mut session = session(true);
    let seller = session
        .account_roster()
        .into_iter()
        .find(|id| {
            let account = session.account(*id).unwrap();
            account.kind() == crate::AccountKind::Retail
                && account
                    .position(&StockId(0))
                    .is_some_and(|position| position.sellable() >= 100)
        })
        .expect("真实初始 float 中存在至少一手可卖股份");
    session.seed_order_for_test(
        seller,
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Sell,
            price: LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1000)),
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Trade { .. })),
        "持股必须来自真实成交"
    );
    let position = session
        .account(AccountId(0))
        .unwrap()
        .position(&StockId(0))
        .unwrap();
    assert_eq!(
        (position.qty(), position.t1_locked(), position.sellable()),
        (100, 100, 0)
    );
    let plan = plan(&session, "冻结持股权限");
    let receipt = session
        .approve_issuer_repurchase(
            plan,
            RepurchaseApprovalSource::Player {
                requester: AccountId(0),
            },
        )
        .unwrap();
    assert!(
        matches!(receipt, RepurchaseApprovalReceipt::Approved { .. }),
        "当前总持仓>0即可提案，不要求可卖股份"
    );
}
#[test]
fn approval_c_session_08_unknown_and_npc_requesters_cannot_bypass_trusted_player_source() {
    let mut session = session(true);
    let plan = plan(&session, "来源权限");
    let hash = session.business_state_hash().unwrap();
    for requester in [AccountId(999), AccountId(1)] {
        let error = session
            .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::Player { requester })
            .unwrap_err();
        assert!(
            matches!(error, SessionError::RepurchaseApproval(RepurchaseApprovalError::InvalidSource { requester: actual }) if actual == requester),
            "{error}"
        );
        assert_eq!(session.business_state_hash().unwrap(), hash);
        assert!(session.runtime_driver_for_test().is_none());
    }
}
#[test]
fn approval_c_session_09_company_preference_uses_same_common_producer_and_actual_issuer_source() {
    let mut session = session(true);
    let plan = plan(&session, "模型偏好");
    let issuer = plan.issuer;
    let receipt = session
        .approve_issuer_repurchase(
            plan.clone(),
            RepurchaseApprovalSource::CompanyPreference { issuer },
        )
        .unwrap();
    assert!(
        matches!(receipt, RepurchaseApprovalReceipt::Approved { funding_amount, .. } if funding_amount == plan.total_budget)
    );
    assert_eq!(
        session.account(treasury(&session)).unwrap().cash(),
        plan.total_budget
    );
    let mut fresh = super::issuer_repurchase_approval_tests::session(true);
    let hash = fresh.business_state_hash().unwrap();
    let error = fresh
        .approve_issuer_repurchase(
            plan,
            RepurchaseApprovalSource::CompanyPreference {
                issuer: crate::company::CompanyId(1),
            },
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::RepurchaseApproval(RepurchaseApprovalError::SourceIssuerMismatch { .. })
        ),
        "{error}"
    );
    assert_eq!(fresh.business_state_hash().unwrap(), hash);
    assert!(fresh.runtime_driver_for_test().is_none());
}
#[test]
fn approval_c_session_10_approval_static_borrow_preserves_queued_player_intent_until_market_start()
{
    let mut session = session(true);
    let plan = plan(&session, "静点借用");
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockId(1),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(2000)),
                qty: 100,
            },
        )
        .unwrap();
    let queued = serde_json::to_value(&session.state.pending_player).unwrap();
    let tick = session.tick();
    let player_cash = session.account(AccountId(0)).unwrap().cash();
    session
        .approve_issuer_repurchase(plan, RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.pending_player).unwrap(),
        queued
    );
    assert_eq!(session.tick(), tick);
    assert_eq!(session.account(AccountId(0)).unwrap().cash(), player_cash);
}
#[test]
fn approval_c_session_11_group_and_inspect_failure_restore_all_company_and_driver_candidates_then_retry(
) {
    use super::issuer_repurchase_approval::{
        approval_fault_pending, set_approval_fault, ApprovalFault,
    };
    for fault in [ApprovalFault::GroupPrepare, ApprovalFault::Inspect] {
        let mut session = session(true);
        session.construct_runtime_driver_lazy_for_test().unwrap();
        let plan = plan(&session, "失败重试");
        let hash = session.business_state_hash().unwrap();
        let companies = session.state.companies.save();
        let library = session.state.library.clone();
        let plans = session.runtime_plans_snapshot().clone();
        let revision = session.external_revision();
        let owner = treasury(&session);
        let cash = session.account(owner).unwrap().cash();
        let (belief, source, pending, published) = {
            let driver = session.runtime_driver_for_test().unwrap().lock().unwrap();
            (
                driver.collect_belief().unwrap(),
                driver.mirror_source(),
                driver.pending_mirror_domains(),
                driver.publication().load(),
            )
        };
        let guard = set_approval_fault(fault);
        let error = session
            .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
            .unwrap_err();
        assert!(
            matches!(
                error,
                SessionError::RepurchaseApproval(RepurchaseApprovalError::InjectedFailure { .. })
            ),
            "必须到达真实故障门：{error}"
        );
        assert_eq!(
            approval_fault_pending(),
            None,
            "Prepare/inspect 必须确实消费对应故障请求"
        );
        drop(guard);
        assert_eq!(session.state.companies.save(), companies);
        assert_eq!(session.runtime_plans_snapshot(), &plans);
        assert_eq!(session.business_state_hash().unwrap(), hash);
        assert_eq!(session.external_revision(), revision);
        assert_eq!(session.account(owner).unwrap().cash(), cash);
        assert!(Arc::ptr_eq(&session.state.library, &library));
        {
            let driver = session.runtime_driver_for_test().unwrap().lock().unwrap();
            assert_eq!(driver.collect_belief().unwrap(), belief);
            assert_eq!(driver.mirror_source(), source);
            assert_eq!(driver.pending_mirror_domains(), pending);
            assert!(Arc::ptr_eq(&driver.publication().load(), &published));
        }
        let receipt = session
            .approve_issuer_repurchase(plan, RepurchaseApprovalSource::InternalScenario)
            .unwrap();
        assert!(
            matches!(receipt, RepurchaseApprovalReceipt::Approved { plan_id, .. } if plan_id.sequence == 0)
        );
    }
}

#[test]
fn approval_c_session_12_registered_split_receipt_does_not_advance_current_unit_anchor() {
    use crate::company::share_split::{
        ShareSplitBook, ShareSplitDeclaration, ShareSplitDirection, ShareSplitEventPlan,
    };
    let ex_on = CivilDate::from_iso("2030-06-04").unwrap();
    let registered_on = CivilDate::from_iso("2030-06-03").unwrap();
    let mut session = session_at(true, Some(ex_on));
    let context = super::capital_approval_context::CapitalApprovalContext::prepare(
        &session,
        crate::company::CompanyId(0),
    )
    .unwrap();
    let old_par = context.par_value();
    let old_capital = context.registered_capital();
    let split = ShareSplitEventPlan {
        event_id: "已登记未入账拆股".into(),
        approval_reference: "真实领域登记尚未执行重新计值".into(),
        issuer: crate::company::CompanyId(0),
        stock: crate::account::StockCode("000001".into()),
        exchange: crate::calendar::CalendarExchange::Szse,
        direction: ShareSplitDirection::Split,
        ratio: 2,
        approved_on: registered_on,
        announced_on: registered_on,
        registered_on,
        ex_rights_on: ex_on,
    };
    let mut book = ShareSplitBook::new(split.clone()).unwrap();
    book.announce(registered_on).unwrap();
    let calendar = session.state.civil_clock.calendar().clone();
    let super::company_state::SessionCompanies::CompanySystem(state) = &mut session.state.companies
    else {
        panic!("必须为真实 CompanySystem")
    };
    Arc::make_mut(&mut state.system)
        .declare_share_split(
            &split.issuer,
            ShareSplitDeclaration {
                event_id: split.event_id.clone(),
                approval_reference: split.approval_reference.clone(),
                direction: split.direction.clone(),
                approved_on: registered_on,
                ratio: 2,
                par_value_before: old_par,
                par_value_after: Money::from_cents(old_par.cents() / 2),
                registered_capital_at_approval: old_capital,
            },
        )
        .unwrap();
    let registry = state
        .corporate_actions
        .registries
        .iter_mut()
        .find(|registry| registry.stock() == &split.stock)
        .unwrap();
    assert_eq!(registry.settled_on(), registered_on);
    let old_shares = registry.issued_shares();
    let snapshot = registry
        .register(split.event_id.clone(), registered_on)
        .unwrap()
        .clone();
    book.register(snapshot, &calendar).unwrap();
    assert!(book.receipt().is_some());
    assert_eq!(
        book.settled_on(),
        None,
        "冻结分配结果并未向实际账户应用新单位"
    );
    state.corporate_actions.share_splits.push(book);
    assert_eq!(
        state
            .corporate_actions
            .registries
            .iter()
            .find(|registry| registry.stock() == &split.stock)
            .unwrap()
            .issued_shares(),
        old_shares
    );
    // 必须先通过完整资本来源门，再检验唯一共同 producer；不以手填 book 绕过前置。
    let capital =
        super::capital_approval_context::CapitalApprovalContext::prepare(&session, split.issuer)
            .expect("真实 Registered 方案及其 finance/registration 必须通过完整资本前置");
    assert_eq!(capital.par_value(), old_par);
    assert_eq!(capital.registered_capital(), old_capital);
    let request = plan(&session, "登记未入账时批准回购");
    assert_eq!(request.approval_unit_event_id, None);
    let approved = session
        .approve_issuer_repurchase(request.clone(), RepurchaseApprovalSource::InternalScenario);
    assert!(
        matches!(&approved, Ok(RepurchaseApprovalReceipt::Approved { .. })),
        "Registered receipt 不代表已应用股份单位，旧单位方案应通过真实共同批准：{approved:?}"
    );
    let actions = session.corporate_actions().unwrap();
    assert_eq!(
        actions
            .issuer_repurchases
            .iter()
            .find(|book| book.plan().event_id == request.event_id)
            .unwrap()
            .plan()
            .approval_unit_event_id,
        None
    );
    assert_eq!(actions.share_splits[0].settled_on(), None);
    assert_eq!(
        session.account(treasury(&session)).unwrap().cash(),
        request.total_budget
    );
    let projections = session.repurchase_restore_projections().unwrap();
    let projection = projections.values().next().unwrap();
    assert_eq!(projection.issuance_in_progress, None);
    assert_eq!(
        projection.material_restriction,
        crate::runtime::world::RepurchaseMaterialRestriction::NotModelled
    );
    assert!(!projection.announced);
    let before_tick = session.tick();
    let before_actions = session.corporate_actions().unwrap().clone();
    let before_cash = session.account(treasury(&session)).unwrap().cash();
    let before_position = session
        .account(treasury(&session))
        .unwrap()
        .position(&StockId(0))
        .map(|position| position.qty());
    let error = session.step().unwrap_err();
    assert!(error.to_string().contains("拆缩股"), "{error}");
    assert_eq!(session.tick(), before_tick);
    assert_eq!(session.corporate_actions().unwrap(), &before_actions);
    assert_eq!(
        session.account(treasury(&session)).unwrap().cash(),
        before_cash
    );
    assert_eq!(
        session
            .account(treasury(&session))
            .unwrap()
            .position(&StockId(0))
            .map(|position| position.qty()),
        before_position
    );
}
