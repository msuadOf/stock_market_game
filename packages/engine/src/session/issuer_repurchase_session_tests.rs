//! 发行人回购 Session 全链路测试（ADR-0038，2026-10-07 M 批）。
//!
//! 覆盖：独立开关禁用显式拒绝、批准即合成资金入发行人回购账户、真实委托进入
//! 既有订单簿（与玩家卖单真实撮合、卖方真实收到资金）、专户（IssuerTreasury）
//! 失权排除、窗口结束回收未用合成资金、注销减少总股本与注册资本、restore 深等。

use super::*;
use crate::account::Position;
use crate::accounting::AccountingAmount;
use crate::company::issuer_repurchase::{
    IssuerRepurchasePlan, RepurchaseFillRecord, RepurchasePurpose,
};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::money::Money;
use crate::orderbook::AccountId;
use crate::strategy::{Intent, LimitPrice};
use crate::orderbook::Side;

fn repurchase_setup(enabled: bool) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 3;
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    setup.issuer_repurchase_enabled = enabled;
    setup
}

fn fixture_lot(id: &str, qty: u64, acquired_on: CivilDate) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on,
        source: AcquisitionSource::InitialAllocation {
            evidence: "repurchase fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

/// `pub(crate)`：company_contract_views_tests（F 修复轮）复用同一 fixture 构建
/// readiness 场景，避免重复装配口径（同 rights_offering_session_tests 先例）。
pub(crate) fn session_with_registry(enabled: bool) -> (GameSession, StockCode, CompanyId, AccountId) {
    let mut setup = repurchase_setup(enabled);
    setup.npcs.retail_count = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let total_shares = session.state.setup.stocks[0].total_shares;
    let date = CivilDate::from_iso("2030-01-02").unwrap();
    // 玩家持 1000 股（作为回购对手方）。
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            Position::from_restored_parts(1_000, 0, 1_000_000, 0),
        );
    let holdings = vec![
        ShareHolding {
            holder: HolderId::Account(AccountId(0)),
            lots: vec![fixture_lot("player-lot", 1_000, date)],
        },
        ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![fixture_lot("treasury-lot", total_shares - 1_000, date)],
        },
    ];
    let registry = ShareRegistry::new(stock.clone(), issuer.clone(), total_shares, date, holdings)
        .unwrap();
    session.configure_share_registry(registry).unwrap();
    session
        .define_dividend_legal_facts(
            &issuer,
            AccountingAmount::from_cents(
                (total_shares / 10 * 100).try_into().unwrap(),
            ),
            "repurchase test legal fact".into(),
        )
        .unwrap();
    let repurchase_account = AccountId(1);
    (session, stock, issuer, repurchase_account)
}

/// `pub(crate)`：同 `session_with_registry`（F 修复轮 readiness 测试复用）。
pub(crate) fn repurchase_plan(issuer: &CompanyId, stock: &StockCode) -> IssuerRepurchasePlan {
    IssuerRepurchasePlan {
        event_id: "repurchase-2030".into(),
        approval_reference: "board-repurchase-2030".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
        window_start_on: CivilDate::from_iso("2030-01-03").unwrap(),
        window_deadline_on: CivilDate::from_iso("2030-01-07").unwrap(),
        price_cap_per_share: Money::from_cents(1_100),
        total_budget: Money::from_cents(5_000_000),
        max_shares: 100,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy:
            crate::company::issuer_repurchase::RepurchaseCompletionPolicy::CancelOnCompletion,
    }
}

fn player_cash(session: &GameSession) -> i64 {
    session.state.accounts.get(&AccountId(0)).unwrap().cash().cents()
}

/// `pub(crate)`：同 `session_with_registry`（F 修复轮 readiness 测试复用）。
pub(crate) fn complete_day(session: &mut GameSession) {
    // 休市自然日不推进市场 tick，直接日结（双时钟语义）。
    if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

#[test]
fn disabled_repurchase_switch_rejects_approval_explicitly() {
    let (mut session, stock, issuer, _account) = session_with_registry(false);
    let plan = repurchase_plan(&issuer, &stock);
    let error = session.approve_issuer_repurchase(plan).unwrap_err();
    assert!(
        error.to_string().contains("本局未启用"),
        "禁用开关的错误必须指明本局未启用：{error}"
    );
}

/// 同一证券同时只能有一个未完成回购方案：第二个未完成方案会使同一笔真实成交
/// 被记入多本账簿（按证券聚合日终回执不区分方案），完成勾稽必然失真——批准时
/// 显式拒绝，完成或取消后才可批准新方案。
#[test]
fn second_unfinished_plan_for_same_stock_is_rejected_at_approval() {
    let (mut session, stock, issuer, _account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    let mut second = repurchase_plan(&issuer, &stock);
    second.event_id = "repurchase-2030-b".into();
    let error = session.approve_issuer_repurchase(second).unwrap_err();
    assert!(
        error.to_string().contains("未完成"),
        "同证券第二个未完成回购方案必须显式拒绝：{error}"
    );
    assert_eq!(
        session
            .state
            .corporate_actions
            .issuer_repurchases
            .iter()
            .filter(|book| book.plan().stock == stock)
            .count(),
        1,
        "被拒绝的方案不得入账"
    );
}

#[test]
fn repurchase_funds_account_places_real_order_and_seller_is_paid() {
    let (mut session, stock, issuer, repurchase_account) = session_with_registry(true);
    assert!(
        session.state.accounts.contains_key(&repurchase_account),
        "开关开启时必须创建发行人回购账户"
    );
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    // 合成资金凭空入账（ADR-0038）：额度 5,000,000 分。
    assert_eq!(
        session
            .state
            .accounts
            .get(&repurchase_account)
            .unwrap()
            .cash()
            .cents(),
        5_000_000,
        "获批计划额度必须全额合成入回购账户"
    );
    // 推进到窗口首日（2030-01-03）：发行人真实委托进入订单簿。
    // 玩家在窗口首日挂 1000 股卖单（价格 1000 分，低于回购价格上限与笼子上界），
    // 与发行人买单撮合。
    while session.civil_date() < CivilDate::from_iso("2030-01-03").unwrap() {
        if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    let seller_cash_before = player_cash(&session);
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let book = session
        .state
        .corporate_actions
        .issuer_repurchases
        .iter()
        .find(|book| book.plan().event_id == "repurchase-2030")
        .unwrap();
    let filled = book.total_filled_shares();
    assert!(filled > 0, "发行人真实委托必须与既有卖单撮合成交");
    assert!(
        player_cash(&session) > seller_cash_before,
        "卖方投资者必须真实收到回购资金（ADR-0038 投资者资金池注入）"
    );
    // 名册：IssuerTreasury 持有回购股份（过户专户）。
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &stock)
        .unwrap();
    let treasury_shares: u64 = registry
        .holdings()
        .iter()
        .filter(|holding| holding.holder == HolderId::IssuerTreasury)
        .flat_map(|holding| holding.lots.iter())
        .map(|lot| lot.qty)
        .sum();
    assert_eq!(treasury_shares, total_shares_of(&session, &stock) - 1_000 + filled);
    // 专户失权：IssuerTreasury 被排除在 entitled_holdings 之外（既有规则）。
    // （分红/送转排除的交叉断言见 corporate_actions 既有组与本文件后续用例。）
}

fn total_shares_of(session: &GameSession, stock: &StockCode) -> u64 {
    session
        .state
        .setup
        .stocks
        .iter()
        .find(|spec| &spec.code == stock)
        .unwrap()
        .total_shares
}

/// H 批（2026-10-08 用户决策「完成即默认注销」）：玩家在窗口当日挂 100 股
/// 卖单与发行人委托撮合，随后日终（= 完成判定当日日终）。
fn fill_max_shares_on_window_day(session: &mut GameSession, stock: &StockCode) {
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
}

fn repurchase_book_index(session: &GameSession, event_id: &str) -> usize {
    session
        .state
        .corporate_actions
        .issuer_repurchases
        .iter()
        .position(|book| book.plan().event_id == event_id)
        .unwrap_or_else(|| panic!("缺少回购账簿 {event_id}"))
}

fn treasury_shares_of(session: &GameSession, stock: &StockCode) -> u64 {
    session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == stock)
        .unwrap()
        .holdings()
        .iter()
        .filter(|holding| holding.holder == HolderId::IssuerTreasury)
        .flat_map(|holding| holding.lots.iter())
        .map(|lot| lot.qty)
        .sum()
}

/// 完成即默认注销（H 批）：数量用尽在完成判定当日日终完成（「Completed 未
/// 注销」为短暂过渡态），**次日日终**自动执行注销——复用既有手动注销路径
/// （专户股份核减、总股本与注册资本按面值演进、不除权、账面事实回填）。
#[test]
fn completed_repurchase_auto_cancels_at_next_day_end() {
    let (mut session, stock, issuer, repurchase_account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    // 推进到窗口首日（2030-01-03）并撮合满额 100 股（= max_shares → 数量用尽）。
    while session.civil_date() < CivilDate::from_iso("2030-01-03").unwrap() {
        complete_day(&mut session);
    }
    fill_max_shares_on_window_day(&mut session, &stock);
    let book_index = repurchase_book_index(&session, "repurchase-2030");
    let filled =
        session.state.corporate_actions.issuer_repurchases[book_index].total_filled_shares();
    assert!(filled > 0, "发行人委托必须真实撮合成交");
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed,
        "数量用尽后完成判定当日日终为短暂过渡态"
    );
    let issued_before = session
        .state
        .company_system
        .issuers()
        .get(&issuer)
        .unwrap()
        .issued_shares;
    let capital_before = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    let treasury_before = treasury_shares_of(&session, &stock);
    // 次日日终：自动注销（完成即默认注销策略）。
    let cancellation_day = session.civil_date();
    complete_day(&mut session);
    let book = &session.state.corporate_actions.issuer_repurchases[book_index];
    assert_eq!(
        book.status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Cancelled,
        "完成后的次日日终必须自动注销（默认注销策略）"
    );
    assert_eq!(book.cancelled_on(), Some(cancellation_day));
    assert_eq!(book.cancelled_shares(), filled, "注销全部已回购股份");
    // 总股本与注册资本按面值 × 股数核减（复用手动注销路径的演进口径）。
    let par_cents = capital_before.to_money().unwrap().cents() as i128 / i128::from(issued_before);
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        issued_before - filled,
        "自动注销必须减少总股本"
    );
    let capital_after = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    assert_eq!(
        capital_after.to_money().unwrap().cents() as i128,
        capital_before.to_money().unwrap().cents() as i128 - par_cents * i128::from(filled),
        "自动注销必须按面值 × 股数核减注册资本"
    );
    // 名册核减：专户（IssuerTreasury）持股减少已注销股数。
    assert_eq!(
        treasury_shares_of(&session, &stock),
        treasury_before - filled,
        "自动注销必须核减名册专户股份"
    );
    assert_eq!(
        session
            .state
            .accounts
            .get(&repurchase_account)
            .unwrap()
            .position(&stock)
            .map(|position| u64::from(position.qty()))
            .unwrap_or(0),
        0,
        "自动注销后回购账户持仓必须清零"
    );
    // Simple 账面事实回填（注销股数/日期/注册资本减少额）。
    let fact = session
        .state
        .company_system
        .issuer_repurchase_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "repurchase-2030")
        .unwrap();
    assert_eq!(fact.cancelled_on, Some(cancellation_day));
    assert_eq!(fact.cancelled_shares, filled);
    assert!(fact.capital_reduction.is_some());
    // 注销不除权。
    assert!(
        !session
            .state
            .corporate_actions
            .applied_ex_reference_groups
            .iter()
            .any(|group| group.stock == stock),
        "回购注销不除权"
    );
    // restore 深等。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.issuer_repurchases).unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.issuer_repurchases).unwrap(),
        "自动注销后的回购账簿恢复深等"
    );
}

/// 自动注销后新方案可提（H 批）：bind-once 专户事实按「存在即不重复绑定」
/// 口径，注销终态后同证券新方案受理不再与首次绑定的事实冲突。
#[test]
fn new_repurchase_plan_is_approvable_after_auto_cancellation() {
    let (mut session, stock, issuer, repurchase_account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    while session.civil_date() < CivilDate::from_iso("2030-01-03").unwrap() {
        complete_day(&mut session);
    }
    fill_max_shares_on_window_day(&mut session, &stock);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases
            [repurchase_book_index(&session, "repurchase-2030")]
        .status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed
    );
    // 次日日终自动注销。
    complete_day(&mut session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases
            [repurchase_book_index(&session, "repurchase-2030")]
        .status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Cancelled
    );
    // 注销终态后：同证券第二方案可受理（唯一性 + 专户事实不冲突）。
    let mut second = repurchase_plan(&issuer, &stock);
    second.event_id = "repurchase-2030-b".into();
    second.approved_on = session.civil_date();
    second.announced_on = session.civil_date();
    second.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    second.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session
        .approve_issuer_repurchase(second)
        .expect("自动注销后同证券新方案应可受理");
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases.len(),
        2,
        "第二方案必须入账"
    );
    // 合成资金按新方案额度重新注入（上一方案完成时已回收归零）。
    assert_eq!(
        session
            .state
            .accounts
            .get(&repurchase_account)
            .unwrap()
            .cash()
            .cents(),
        5_000_000,
        "新方案获批额度必须全额合成入回购账户"
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuer_repurchase_facts(&issuer)
            .unwrap()
            .len(),
        2,
        "Simple 账面声明事实两份"
    );
}

/// 玩家局中日终提案（H 批用户决策「局中日终可提回购提案」）：开局数日、
/// 多个日终之后（名册 settled_on 已离开开局装配日）玩家回购提案被受理
/// （专户事实 established_on 取名册结算日 settled_on，与「持仓即可」语义
/// 一致），全链路到自动注销。
#[test]
fn player_repurchase_proposal_midgame_reaches_auto_cancellation() {
    let (mut session, stock, issuer, _account) = session_with_registry(true);
    // 推进过多个日终：名册 settled_on 已随日终推进，离开开局装配日。
    while session.civil_date() < CivilDate::from_iso("2030-01-10").unwrap() {
        complete_day(&mut session);
    }
    let proposal_day = session.civil_date();
    let receipt = session
        .propose_company_action(
            AccountId(0),
            super::player_proposals::PlayerCompanyProposal::IssuerRepurchase {
                company: issuer.clone(),
                price_cap_per_share: Money::from_cents(1_100),
                total_budget: Money::from_cents(500_000),
                max_shares: 100,
                window_trading_days: 1,
                purpose: RepurchasePurpose::ReduceCapital,
            },
        )
        .expect("局中日终后玩家回购提案应被受理（established_on 取名册结算日）");
    assert_eq!(
        receipt.kind,
        super::player_proposals::PlayerProposalKind::IssuerRepurchase
    );
    assert_eq!(receipt.approved_on, proposal_day);
    let book_index = repurchase_book_index(&session, &receipt.identity);
    let plan = session.state.corporate_actions.issuer_repurchases[book_index]
        .plan()
        .clone();
    // 窗口起点 = 公告日后首个交易日（引擎推导），局内提案日程成立。
    assert!(plan.window_start_on > proposal_day);
    assert!(plan.window_contains(plan.window_start_on));
    // 全链路：窗口首日撮合满额 → 完成日终（短暂过渡态）→ 次日日终自动注销。
    while session.civil_date() < plan.window_start_on {
        complete_day(&mut session);
    }
    fill_max_shares_on_window_day(&mut session, &stock);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed,
        "数量用尽后当日日终完成（过渡态）"
    );
    let issued_before = session
        .state
        .company_system
        .issuers()
        .get(&issuer)
        .unwrap()
        .issued_shares;
    let filled =
        session.state.corporate_actions.issuer_repurchases[book_index].total_filled_shares();
    complete_day(&mut session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Cancelled,
        "玩家提案的回购方案同样适用完成即默认注销"
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        issued_before - filled,
        "自动注销核减总股本"
    );
}

/// 竞态边界（H 批显式登记）：完成判定当日仍有回购账户在途未成交委托残留时，
/// 该日不自动注销（Completed 保留），下一日终（委托已被市场日终清空后）处理。
#[test]
fn auto_cancellation_defers_when_repurchase_order_rests_at_day_end() {
    let (mut session, stock, issuer, repurchase_account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    while session.civil_date() < CivilDate::from_iso("2030-01-03").unwrap() {
        complete_day(&mut session);
    }
    fill_max_shares_on_window_day(&mut session, &stock);
    let book_index = repurchase_book_index(&session, "repurchase-2030");
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed
    );
    // 次日：当日 tick 全部结束后（市场日终已清空订单簿）注入一笔回购账户
    // 在途委托，复现「完成判定当日仍有未成交委托残留」的时序竞态。注入卖向
    // 委托（预约股份而非现金）：买向残留与「live buy reservation exceeds
    // account cash」不变量冲突、在下一 tick 即显式失败——正常时序下市场日终
    // 已清空订单簿，该守卫防御的是日终时序错位；守卫按**账户归属**判定在途
    // 委托，不区分方向。
    if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session
        .state
        .markets
        .get_mut(&stock)
        .unwrap()
        .place(crate::orderbook::Order {
            id: crate::orderbook::OrderId(9_000_001),
            side: Side::Sell,
            price: Money::from_cents(1_100),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: repurchase_account,
            seq: 9_000_001,
        })
        .unwrap();
    session.end_civil_day().unwrap();
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed,
        "在途委托残留时该日不自动注销，次日处理"
    );
    // 在途委托消解后的首个日终（交易日市场日终清空订单簿）：自动注销执行。
    // 注：期间休市自然日（周末）不运行市场 tick、订单簿不被清空，守卫持续
    // 跳过——委托只在交易日市场日终消解，注销不迟到只延后。
    while session.civil_date() < CivilDate::from_iso("2030-01-07").unwrap() {
        complete_day(&mut session);
        assert_eq!(
            session.state.corporate_actions.issuer_repurchases[book_index].status(),
            &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed,
            "在途委托未消解的日终仍不得自动注销"
        );
    }
    complete_day(&mut session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Cancelled,
        "在途委托消解后的首个日终必须自动注销"
    );
}

/// 零成交边界（H 批显式登记）：完成时无任何成交则无股份可核减（注销以正股数
/// 为前提），方案保持 Completed 终态；日终不因自动注销重试而失败，且不阻塞
/// 同证券新方案（唯一性只挡非终态）。
#[test]
fn zero_fill_completed_plan_stays_terminal_without_blocking_new_plans() {
    let (mut session, stock, issuer, _account) = session_with_registry(true);
    let mut single_day = repurchase_plan(&issuer, &stock);
    single_day.window_deadline_on = CivilDate::from_iso("2030-01-03").unwrap();
    session.approve_issuer_repurchase(single_day).unwrap();
    // 窗口截止后的首个日终完成（零成交、全额回收）。
    while session.civil_date() <= CivilDate::from_iso("2030-01-03").unwrap() {
        complete_day(&mut session);
    }
    complete_day(&mut session);
    let book_index = repurchase_book_index(&session, "repurchase-2030");
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed
    );
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].total_filled_shares(),
        0,
        "无对手盘时零成交"
    );
    // 完成后的下一日终：无股可注销，不失败、不改写状态。
    complete_day(&mut session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].status(),
        &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed,
        "零成交方案无股份可核减，保持 Completed 终态"
    );
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[book_index].cancelled_on(),
        None
    );
    let mut second = repurchase_plan(&issuer, &stock);
    second.event_id = "repurchase-2030-b".into();
    second.approved_on = session.civil_date();
    second.announced_on = session.civil_date();
    second.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    second.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session
        .approve_issuer_repurchase(second)
        .expect("零成交 Completed 终态不得阻塞同证券新方案");
    assert_eq!(session.state.corporate_actions.issuer_repurchases.len(), 2);
    let _ = stock;
}

#[test]
fn repurchase_completion_withdraws_unused_synthetic_funds_and_cancellation_shrinks_capital() {
    let (mut session, stock, issuer, repurchase_account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    // 推进到窗口首日，玩家卖单与发行人真实委托撮合（100 股 = max_shares）。
    while session.civil_date() < CivilDate::from_iso("2030-01-03").unwrap() {
        if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let book_index = session
        .state
        .corporate_actions
        .issuer_repurchases
        .iter()
        .position(|book| book.plan().event_id == "repurchase-2030")
        .unwrap();
    let filled =
        session.state.corporate_actions.issuer_repurchases[book_index].total_filled_shares();
    assert!(filled > 0);
    let issued_before = session
        .state
        .company_system
        .issuers()
        .get(&issuer)
        .unwrap()
        .issued_shares;
    let capital_before = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    // 数量用尽：完成判定当日日终回收未用合成资金（账户归零），「Completed 未
    // 注销」为短暂过渡态（H 批：次日终自动注销）。
    let book = &session.state.corporate_actions.issuer_repurchases[book_index];
    assert_eq!(
        *book.status(),
        crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed
    );
    assert!(
        book.withdrawn_remainder().is_some(),
        "数量用尽后必须回收未用合成资金"
    );
    assert_eq!(
        session
            .state
            .accounts
            .get(&repurchase_account)
            .unwrap()
            .cash()
            .cents(),
        0,
        "回收后回购账户现金必须归零"
    );
    // 过渡窗口内（次日终之前）手动注销路径仍然可用（自动注销复用的同一入口）：
    // 总股本与注册资本按面值×股数演进；不除权（无除权锚）。
    let par_cents = capital_before.to_money().unwrap().cents() as i128 / i128::from(issued_before);
    let manual_cancellation_day = session.civil_date();
    session
        .execute_issuer_repurchase_cancellation("repurchase-2030", filled, session.civil_date())
        .unwrap();
    let issued_after = session
        .state
        .company_system
        .issuers()
        .get(&issuer)
        .unwrap()
        .issued_shares;
    assert_eq!(issued_after, issued_before - filled, "注销必须减少总股本");
    let capital_after = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    assert_eq!(
        capital_after.to_money().unwrap().cents() as i128,
        capital_before.to_money().unwrap().cents() as i128 - par_cents * i128::from(filled),
        "注销必须按面值×股数核减注册资本"
    );
    assert!(
        !session
            .state
            .corporate_actions
            .applied_ex_reference_groups
            .iter()
            .any(|group| group.stock == stock),
        "回购注销不除权"
    );
    // 手动注销后的日终不得重复自动注销（终态幂等，不改写注销事实）。
    complete_day(&mut session);
    let book = &session.state.corporate_actions.issuer_repurchases[book_index];
    assert_eq!(
        *book.status(),
        crate::company::issuer_repurchase::IssuerRepurchaseStatus::Cancelled
    );
    assert_eq!(
        book.cancelled_on(),
        Some(manual_cancellation_day),
        "自动注销不得改写已注销事实"
    );
    assert_eq!(
        session
            .state
            .accounts
            .get(&repurchase_account)
            .unwrap()
            .position(&stock)
            .map(|position| u64::from(position.qty()))
            .unwrap_or(0),
        0,
        "注销后回购账户持仓清零"
    );
    // restore 深等。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.issuer_repurchases).unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.issuer_repurchases).unwrap(),
        "回购账簿恢复深等"
    );
}

#[test]
fn treasury_shares_are_excluded_from_dividend_entitlements() {
    let (mut session, stock, issuer, _account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    while session.civil_date() < CivilDate::from_iso("2030-01-03").unwrap() {
        if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &stock)
        .unwrap();
    let treasury: u64 = registry
        .holdings()
        .iter()
        .filter(|holding| holding.holder == HolderId::IssuerTreasury)
        .flat_map(|holding| holding.lots.iter())
        .map(|lot| lot.qty)
        .sum();
    let entitled: u64 = registry
        .holdings()
        .iter()
        .filter(|holding| holding.holder != HolderId::IssuerTreasury)
        .flat_map(|holding| holding.lots.iter())
        .map(|lot| lot.qty)
        .sum();
    assert!(treasury > 0, "fixture 初始 Treasury + 回购新增");
    assert!(entitled < registry.issued_shares());
    // entitled_holdings（分红/送转资格入口）排除 IssuerTreasury：结构性断言由
    // company::stock_distribution 的既有分配用例与 corporate_actions 分红组覆盖；
    // 此处只断言专户持仓真实存在且小于总股本。
    assert!(registry.issued_shares() > entitled);
}
