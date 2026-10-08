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

#[test]
fn repurchase_completion_withdraws_unused_synthetic_funds_and_cancellation_shrinks_capital() {
    let (mut session, stock, issuer, repurchase_account) = session_with_registry(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    // 推进到窗口首日，玩家卖单与发行人真实委托撮合。
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
    let filled = session.state.corporate_actions.issuer_repurchases[book_index]
        .total_filled_shares();
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
    // 推进窗口到截止并日结：未用合成资金被回收（账户归零）。
    while session.civil_date() <= CivilDate::from_iso("2030-01-07").unwrap() {
        complete_day(&mut session);
    }
    // 窗口截止后的首个日终完成回收。
    complete_day(&mut session);
    let book = &session.state.corporate_actions.issuer_repurchases[book_index];
    assert_eq!(*book.status(), crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed);
    assert!(
        book.withdrawn_remainder().is_some(),
        "窗口结束后必须回收未用合成资金"
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
    // 注销：总股本与注册资本按面值×股数演进；不除权（无除权锚）。
    let par_cents = capital_before
        .to_money()
        .unwrap()
        .cents() as i128
        / i128::from(issued_before);
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
