//! 配股／增发 Session 全链路测试（2026-10-07 M 批）。
//!
//! 覆盖：开关禁用时显式拒绝（错误指明本局未启用）、公告→R 日权证→缴款期认购
//! 划扣→L 关窗→L+1 除权→L+2 划款/新股入账/税账、NPC 默认足额认购与现金不足
//! 弃配、失败退款原子、restore 深等。

use super::*;
use crate::account::Position;
use crate::accounting::AccountingAmount;
use crate::company::rights_offering::{
    DirectedPlacementTarget, RightsOfferingError, RightsOfferingEventPlan, RightsOfferingMode,
    RightsSubscriptionStrategy,
};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::orderbook::AccountId;

fn rights_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    // 机构 NPC 作为默认足额认购的持有人（散户持有人直插持仓会缺自然人经历，
    // 触发存档校验——分红 fixture 同一边界）。
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 1;
    setup.ticks_per_day = 1;
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    setup.rights_offering_enabled = true;
    // 本文件覆盖个人差别化税账（配股取得日入账等）：显式大 A 方式，
    // 装配名册即自动为玩家开个人税账。
    setup.dividend_tax_mode =
        crate::company::cash_dividend_tax::CashDividendTaxMode::AShareIndividual;
    setup.flat_withholding_bp = None;
    setup
}

fn fixture_lot(id: &str, qty: u64, acquired_on: CivilDate) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on,
        source: AcquisitionSource::InitialAllocation {
            evidence: "rights fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

/// 玩家(0) 3,000,000 股 + 机构 NPC(1) 1,000,000 股 + Treasury 6,000,000 股
/// （总股本 10,000,000）；玩家已开个人税账。面值 1 分（注册资本 = 总股本×1 分），
/// 配股价 10 分/股（高于面值）。
fn session_with_registry(rights_enabled: bool) -> (GameSession, StockCode, CompanyId) {
    let mut setup = rights_setup();
    setup.rights_offering_enabled = rights_enabled;
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
    for (account, qty) in [(AccountId(0), 3_000_000_u64), (AccountId(1), 1_000_000_u64)] {
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_insert_position(
                stock.clone(),
                Position::from_restored_parts(qty.try_into().unwrap(), 0, 1_000, 0),
            );
    }
    let holdings = vec![
        ShareHolding {
            holder: HolderId::Account(AccountId(0)),
            lots: vec![fixture_lot("player-lot", 3_000_000, date)],
        },
        ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![fixture_lot("institution-lot", 1_000_000, date)],
        },
        ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![fixture_lot(
                "treasury-lot",
                total_shares - 4_000_000,
                date,
            )],
        },
    ];
    let registry = ShareRegistry::new(stock.clone(), issuer.clone(), total_shares, date, holdings)
        .unwrap();
    session.configure_share_registry(registry).unwrap();
    // AShareIndividual 模式下装配名册已自动为玩家开个人税账，不再显式配置。
    session
        .define_dividend_legal_facts(
            &issuer,
            AccountingAmount::from_cents(i128::from(total_shares)),
            "rights test legal fact".into(),
        )
        .unwrap();
    (session, stock, issuer)
}

/// 面向全体股东：每 10 股配 5 股（50%），价格 8 元/股；R=2030-01-04，
/// 缴款期 5 个交易日。
fn all_shareholders_plan(issuer: &CompanyId, stock: &StockCode) -> RightsOfferingEventPlan {
    let mut plan = RightsOfferingEventPlan {
        event_id: "rights-2030".into(),
        approval_reference: "board-2030".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-03").unwrap(),
        registered_on: CivilDate::from_iso("2030-01-04").unwrap(),
        payment_start_on: CivilDate::from_iso("2030-01-04").unwrap(),
        payment_deadline_on: CivilDate::from_iso("2030-01-04").unwrap(),
        ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        settlement_on: CivilDate::from_iso("2030-01-04").unwrap(),
        listing_on: CivilDate::from_iso("2030-01-04").unwrap(),
        price_per_share: crate::money::Money::from_cents(10),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 500_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        crate::calendar::CalendarExchange::Sse,
        5,
    )
    .unwrap();
    plan
}

fn complete_day(session: &mut GameSession) {
    // 休市自然日不推进市场 tick，直接日结（双时钟语义）。
    if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

fn cash_of(session: &GameSession, account: AccountId) -> i64 {
    session.state.accounts.get(&account).unwrap().cash().cents()
}

fn position_of(session: &GameSession, account: AccountId, stock: &StockCode) -> u64 {
    u64::from(
        session
            .state
            .accounts
            .get(&account)
            .unwrap()
            .positions()
            .get(stock)
            .map(|position| position.qty())
            .unwrap_or(0),
    )
}

#[test]
fn disabled_switch_rejects_rights_apis_explicitly() {
    let (mut session, stock, issuer) = session_with_registry(false);
    let plan = all_shareholders_plan(&issuer, &stock);
    let error = session.approve_rights_offering(plan).unwrap_err();
    assert!(
        error.to_string().contains("本局未启用"),
        "禁用开关的错误必须指明本局未启用：{error}"
    );
    let error = session
        .subscribe_rights_offering("rights-2030", AccountId(0), 1)
        .unwrap_err();
    assert!(
        error.to_string().contains("本局未启用"),
        "禁用开关的认购错误必须指明本局未启用：{error}"
    );
}

#[test]
fn strategy_based_plan_is_rejected_at_approval() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let mut plan = all_shareholders_plan(&issuer, &stock);
    plan.npc_subscription_strategy = RightsSubscriptionStrategy::StrategyBased;
    let error = session.approve_rights_offering(plan).unwrap_err();
    assert!(
        error.to_string().contains("StrategyBased"),
        "未实现策略必须显式拒绝并指明策略值：{error}"
    );
}

/// 与 [`all_shareholders_plan`] 同除权日（2030-01-14）的送转事件：登记日取
/// 配股除权日前一交易日，使送转除权日与配股除权日同日。
fn same_ex_date_stock_plan(
    issuer: &CompanyId,
    stock: &StockCode,
    ex_rights_on: CivilDate,
) -> crate::company::stock_distribution::StockDistributionEventPlan {
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let registered_on = calendar
        .previous_trading_day(crate::calendar::CalendarExchange::Sse, ex_rights_on)
        .unwrap();
    crate::company::stock_distribution::StockDistributionEventPlan {
        event_id: "stock-same-ex-collision".into(),
        approval_reference: "stock collision fixture".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
        registered_on,
        ex_rights_on,
        shares_per_existing_share_micros: 100_000,
        approved_total_new_shares: 400_000,
    }
}

#[test]
fn stock_distribution_rejected_when_same_ex_date_rights_offering_exists() {
    // 配股先声明、送转后声明且同除权日：approve_stock_distribution 受理时显式
    // 拒绝（配股×送转同日合并除权口径未核实），不得推迟到除权日日结才以
    // StepFatal 暴露；账簿保持不变。
    let (mut session, stock, issuer) = session_with_registry(true);
    let rights = all_shareholders_plan(&issuer, &stock);
    let ex_on = rights.ex_rights_on;
    session.approve_rights_offering(rights).unwrap();
    let collision = same_ex_date_stock_plan(&issuer, &stock, ex_on);
    let error = session
        .approve_stock_distribution(collision)
        .expect_err("同除权日已有配股事件时送转受理必须显式拒绝");
    assert!(
        error.to_string().contains("同除权日") && error.to_string().contains("配股"),
        "送转受理拒绝必须指明同除权日配股碰撞：{error}"
    );
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        0,
        "被拒送转不得入账簿"
    );
    assert_eq!(
        session.state.corporate_actions.rights_offerings.len(),
        1,
        "既有配股账簿不受被拒送转影响"
    );
}

#[test]
fn rights_plan_rejected_when_same_ex_date_stock_event_exists() {
    // 送转先声明、配股后声明且同除权日：approve_rights_offering 受理时显式
    // 拒绝（与 approve_stock_distribution 的碰撞预检对称，不把碰撞推迟到
    // 除权日日结才以 StepFatal 暴露）；账簿保持不变。
    let (mut session, stock, issuer) = session_with_registry(true);
    let stock_first =
        same_ex_date_stock_plan(&issuer, &stock, CivilDate::from_iso("2030-01-14").unwrap());
    let ex_on = stock_first.ex_rights_on;
    session.approve_stock_distribution(stock_first).unwrap();
    let rights = all_shareholders_plan(&issuer, &stock);
    assert_eq!(
        rights.ex_rights_on, ex_on,
        "配股除权日必须与既有送转事件除权日重合"
    );
    let error = session
        .approve_rights_offering(rights)
        .expect_err("同除权日已有送转事件时配股受理必须显式拒绝");
    assert!(
        error.to_string().contains("同除权日") && error.to_string().contains("送转"),
        "配股受理拒绝必须指明同除权日送转碰撞：{error}"
    );
    assert_eq!(
        session.state.corporate_actions.rights_offerings.len(),
        0,
        "被拒配股不得入账簿"
    );
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        1,
        "既有送转账簿不受被拒配股影响"
    );
}

#[test]
fn second_rights_plan_rejected_when_same_ex_date_rights_exists() {
    // 同一发行人同证券同除权日的第二起配股（不同 event_id）：受理时显式
    // 拒绝（同日多起配股的合并除权口径未核实）；账簿保持不变。
    let (mut session, stock, issuer) = session_with_registry(true);
    let first = all_shareholders_plan(&issuer, &stock);
    let ex_on = first.ex_rights_on;
    session.approve_rights_offering(first).unwrap();
    let mut second = all_shareholders_plan(&issuer, &stock);
    second.event_id = "rights-second-same-ex".into();
    assert_eq!(second.ex_rights_on, ex_on);
    let error = session
        .approve_rights_offering(second)
        .expect_err("同除权日已存在配股事件时第二起配股受理必须显式拒绝");
    assert!(
        error.to_string().contains("同除权日") && error.to_string().contains("配股"),
        "第二起配股受理拒绝必须指明同除权日配股碰撞：{error}"
    );
    assert_eq!(
        session.state.corporate_actions.rights_offerings.len(),
        1,
        "仅第一起配股在账簿"
    );
}

#[test]
fn rights_full_chain_announces_entitles_charges_settles_credits_and_restores() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = all_shareholders_plan(&issuer, &stock);
    let registered_on = plan.registered_on;
    let deadline = plan.payment_deadline_on;
    let ex_date = plan.ex_rights_on;
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan.clone()).unwrap();

    let player_cash_before = cash_of(&session, AccountId(0));

    // 推进到缴款截止日：途中自然经过公告日与 R 日；玩家在窗口内显式认购全部权利
    // （3,000,000 × 0.5 = 1,500,000 股，成本 1,500万分，玩家现金远超）。
    while session.civil_date() < deadline {
        if session.civil_date() >= session
            .state
            .corporate_actions
            .rights_offerings
            .iter()
            .find(|book| book.plan().event_id == "rights-2030")
            .map(|book| book.plan().payment_start_on)
            .unwrap()
        {
            let _ = session.subscribe_rights_offering("rights-2030", AccountId(0), 1_500_000);
        }
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    assert!(
        book.entitlement().is_some(),
        "R 日 {} 必须完成权证派发",
        registered_on
    );
    assert_eq!(
        book.entitlement().unwrap().planned_total_rights_shares,
        2_000_000
    );
    // 玩家显式 + NPC 默认（FullByDefault）各一条；NPC 现金不足部分弃配如实记录。
    assert_eq!(book.subscriptions().len(), 2, "玩家显式 + NPC 默认各一条");
    let player_subscription = book
        .subscriptions()
        .iter()
        .find(|record| record.holder == HolderId::Account(AccountId(0)))
        .unwrap();
    assert_eq!(player_subscription.requested_shares, 1_500_000);
    assert_eq!(player_subscription.paid_shares, 1_500_000);
    let npc_paid = book
        .subscriptions()
        .iter()
        .find(|record| record.holder == HolderId::Account(AccountId(1)))
        .map(|record| record.paid_shares)
        .unwrap();
    assert_eq!(
        player_subscription.paid_amount.cents(),
        15_000_000,
        "玩家缴款按发行价×股数划扣"
    );
    assert_eq!(cash_of(&session, AccountId(0)), player_cash_before - 15_000_000);

    // L+1 除权。
    while session.civil_date() < ex_date {
        complete_day(&mut session);
    }
    let _ = ex_date;
    // L+2 结算与入账。
    while session.civil_date() <= settlement_on {
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    let settlement = book.settlement().expect("L+2 必须完成结算");
    assert!(
        !settlement.failed,
        "玩家全额认购 1.5M ≥ 70%×2M 已保证发行成功"
    );
    let paid = settlement.total_paid_shares;
    assert_eq!(paid, 1_500_000 + npc_paid, "玩家显式 + NPC 默认合计");
    assert!(book.credited_on().is_some(), "新股必须在上市日入账");
    assert_eq!(
        position_of(&session, AccountId(0), &stock),
        3_000_000 + 1_500_000,
        "玩家原持股 + 新股"
    );
    // 税账：玩家新股按取得日进个人税账。
    let tax_lots = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0))
        .unwrap()
        .lots();
    let rights_lot = tax_lots
        .iter()
        .find(|lot| lot.id.contains("rights-offering"));
    assert!(rights_lot.is_some(), "配股新股必须进入个人税账 FIFO");
    // Simple 账面：募集资金事实。
    let finance_fact = session
        .state
        .company_system
        .rights_offering_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "rights-2030")
        .unwrap();
    assert_eq!(finance_fact.issued_shares, paid);
    assert_eq!(
        finance_fact.proceeds.unwrap().to_money().unwrap().cents(),
        i64::try_from(paid).unwrap() * 10
    );

    // 除权锚组。
    assert!(session
        .state
        .corporate_actions
        .applied_ex_reference_groups
        .iter()
        .any(|group| group.rights_event_ids.iter().any(|id| id == "rights-2030")));

    // restore 深等。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.rights_offerings).unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.rights_offerings).unwrap(),
        "配股账簿恢复深等"
    );
    assert_eq!(
        position_of(&restored, AccountId(0), &stock),
        3_000_000 + 1_500_000
    );
}

#[test]
fn subscription_beyond_rights_or_window_is_rejected() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = all_shareholders_plan(&issuer, &stock);
    session.approve_rights_offering(plan.clone()).unwrap();
    // 窗口未开始（尚处 Approved）即认购 → 明确拒绝。
    let error = session
        .subscribe_rights_offering("rights-2030", AccountId(0), 1)
        .unwrap_err();
    assert!(!error.to_string().is_empty());
    // 超权利认购在 R 日后被拒。
    while session.civil_date() < plan.payment_start_on {
        complete_day(&mut session);
    }
    let error = session
        .subscribe_rights_offering("rights-2030", AccountId(0), 1_500_001)
        .unwrap_err();
    assert!(
        error.to_string().contains("权利"),
        "超权利认购必须显式拒绝：{error}"
    );
}

#[test]
fn npc_full_by_default_waives_when_cash_insufficient() {
    let (mut session, stock, issuer) = session_with_registry(true);
    // NPC(1) 现金压到 5 分（< 1 股×10 分）：全额弃配（只动现金，不动持仓）。
    {
        let account = session.state.accounts.get_mut(&AccountId(1)).unwrap();
        let drain = account.cash().sub(crate::money::Money::from_cents(5)).unwrap();
        account.debit_cash(drain).unwrap();
    }
    let plan = all_shareholders_plan(&issuer, &stock);
    let deadline = plan.payment_deadline_on;
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan).unwrap();
    while session.civil_date() < deadline {
        complete_day(&mut session);
    }
    while session.civil_date() <= settlement_on {
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    let npc_subscription = book
        .subscriptions()
        .iter()
        .find(|record| record.holder == HolderId::Account(AccountId(1)))
        .unwrap();
    assert_eq!(npc_subscription.paid_shares, 0, "现金不足时弃配");
    assert!(npc_subscription.waived_shares > 0, "弃配部分必须如实记录");
    assert_eq!(npc_subscription.paid_amount.cents(), 0);
    // 玩家未认购 → 认购 < 70% → 发行失败 → 退款（NPC 缴 0，退款 0）。
    let settlement = book.settlement().unwrap();
    assert!(settlement.failed, "认购不足 70% 必须失败");
    assert_eq!(settlement.refunded_total.cents(), 0);
    assert!(book.credited_on().is_none(), "失败发行不产生新股");
    assert_eq!(position_of(&session, AccountId(1), &stock), 1_000_000, "失败不加股");
}

#[test]
fn failed_offering_refunds_paid_cash_atomically_at_settlement() {
    let (mut session, stock, issuer) = session_with_registry(true);
    // 玩家现金充足但只认购 100,000 股（拟配售 2M，100,000 < 1.4M → 失败）；NPC 现金压 0。
    {
        let account = session.state.accounts.get_mut(&AccountId(1)).unwrap();
        account.debit_cash(account.cash()).unwrap();
    }
    let plan = all_shareholders_plan(&issuer, &stock);
    let deadline = plan.payment_deadline_on;
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan.clone()).unwrap();
    let player_cash_before = cash_of(&session, AccountId(0));
    while session.civil_date() < deadline {
        if session.civil_date() == plan.payment_start_on {
            session
                .subscribe_rights_offering("rights-2030", AccountId(0), 100_000)
                .unwrap();
        }
        complete_day(&mut session);
    }
    // 缴款期已划扣 100,000 × 10 分。
    assert_eq!(cash_of(&session, AccountId(0)), player_cash_before - 1_000_000);
    while session.civil_date() <= settlement_on {
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    let settlement = book.settlement().unwrap();
    assert!(settlement.failed);
    // 失败退款把已缴款真实退回。
    assert_eq!(cash_of(&session, AccountId(0)), player_cash_before, "失败退款必须全额退回");
    assert_eq!(position_of(&session, AccountId(0), &stock), 3_000_000, "失败不加股");
    let _ = stock;
}

/// 公开配售专用最小方案：定向对象只有一笔公开配售额度（无具名权利）。
fn open_public_only_plan(
    issuer: &CompanyId,
    stock: &StockCode,
    open_shares: u64,
) -> RightsOfferingEventPlan {
    let mut plan = all_shareholders_plan(issuer, stock);
    plan.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![DirectedPlacementTarget::OpenPublicSubscription {
            shares: open_shares,
        }],
    };
    plan
}

#[test]
fn open_public_subscription_is_capped_by_remaining_headroom_at_acceptance() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = open_public_only_plan(&issuer, &stock, 100);
    session.approve_rights_offering(plan.clone()).unwrap();
    while session.civil_date() < plan.payment_start_on {
        complete_day(&mut session);
    }
    // 玩家（无具名权利）按公开额度认购 60 股，剩余 40 股。
    session
        .subscribe_rights_offering("rights-2030", AccountId(0), 60)
        .unwrap();
    // 41 > 剩余 40：必须显式拒绝、不入队、不截断。
    let error = session
        .subscribe_rights_offering("rights-2030", AccountId(1), 41)
        .unwrap_err();
    assert!(
        error.to_string().contains("剩余"),
        "超剩余额度认购必须在受理时显式拒绝并说明剩余额度：{error}"
    );
    let queued: Vec<_> = session
        .state
        .corporate_actions
        .rights_subscription_queue
        .iter()
        .filter(|queued| queued.event_id == "rights-2030")
        .collect();
    assert_eq!(queued.len(), 1, "被拒绝的认购不得入队");
    assert_eq!(queued[0].account, AccountId(0));
    assert_eq!(queued[0].requested_shares, 60);
    // 剩余额度内（40 股）的认购仍可受理。
    session
        .subscribe_rights_offering("rights-2030", AccountId(1), 40)
        .unwrap();
}

#[test]
fn open_public_oversubscribed_queue_is_rejected_with_receipt_at_day_end() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let plan = open_public_only_plan(&issuer, &stock, 100);
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan.clone()).unwrap();
    while session.civil_date() < plan.payment_start_on {
        complete_day(&mut session);
    }
    let player_cash_before = cash_of(&session, AccountId(0));
    session
        .subscribe_rights_offering("rights-2030", AccountId(0), 60)
        .unwrap();
    // 模拟恢复后队列与额度不一致的极端竞态：直接注入一笔超额度排队
    // （60 + 41 > 100），日终不得因它整日失败。
    session
        .state
        .corporate_actions
        .rights_subscription_queue
        .push(crate::session::corporate_actions::QueuedRightsSubscription {
            event_id: "rights-2030".into(),
            account: AccountId(1),
            requested_shares: 41,
            submitted_on: session.civil_date(),
        });
    complete_day(&mut session);
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    // 额度内成交：玩家 60 股足额划扣；超额者不产生认购记录。
    let player_subscription = book
        .subscriptions()
        .iter()
        .find(|record| record.holder == HolderId::Account(AccountId(0)))
        .unwrap();
    assert_eq!(player_subscription.paid_shares, 60);
    assert_eq!(cash_of(&session, AccountId(0)), player_cash_before - 600);
    assert!(
        !book
            .subscriptions()
            .iter()
            .any(|record| record.holder == HolderId::Account(AccountId(1))),
        "超额认购不得进入账簿"
    );
    // 超额者显式失败回执：留痕、说明原因、从队列移除。
    let rejections = &session
        .state
        .corporate_actions
        .rejected_rights_subscriptions;
    assert_eq!(rejections.len(), 1, "超额认购必须留下显式拒绝回执");
    assert_eq!(rejections[0].event_id, "rights-2030");
    assert_eq!(rejections[0].account, AccountId(1));
    assert_eq!(rejections[0].requested_shares, 41);
    assert!(
        rejections[0].reason.contains("额度"),
        "拒绝回执必须说明额度原因：{}",
        rejections[0].reason
    );
    assert!(
        session
            .state
            .corporate_actions
            .rights_subscription_queue
            .iter()
            .all(|queued| queued.event_id != "rights-2030"),
        "已处理的排队（含被拒）必须移除，避免日终反复失败"
    );
    // 推进到结算：总额度内成交 60 股，发行成功。
    while session.civil_date() <= settlement_on {
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    assert_eq!(book.settlement().unwrap().total_paid_shares, 60);
    assert_eq!(position_of(&session, AccountId(0), &stock), 3_000_060);
    // restore 深等（含拒绝回执）并重放幂等。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.rejected_rights_subscriptions)
            .unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.rejected_rights_subscriptions)
            .unwrap(),
        "拒绝回执恢复深等"
    );
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.rights_offerings).unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.rights_offerings).unwrap(),
        "配股账簿恢复深等"
    );
}

/// 微量认购×同日现金分红的除权勾稽边界 fixture：小持仓名册（玩家 5 股 +
/// 机构 1 股 + Treasury 其余，总股本 10,000,000）、Simple 预历史利润足以授权
/// 现金分红（每股 1 分 × 6 股）、玩家已开个人税账、法定注册资本 = 总股本×1 分
/// （面值 1 分，配股价 10 分高于面值）。
fn session_with_tiny_rights_and_cash_dividend() -> (GameSession, StockCode, CompanyId) {
    let mut setup = rights_setup();
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system
    {
        config.prehistory_periods = 24;
        config.settlement_cycle = crate::company::simple::period::SettlementCycle::Monthly;
    }
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
    for (account, qty) in [(AccountId(0), 5_u64), (AccountId(1), 1_u64)] {
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_insert_position(
                stock.clone(),
                Position::from_restored_parts(qty.try_into().unwrap(), 0, 1_000, 0),
            );
    }
    let holdings = vec![
        ShareHolding {
            holder: HolderId::Account(AccountId(0)),
            lots: vec![fixture_lot("player-lot", 5, date)],
        },
        ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![fixture_lot("institution-lot", 1, date)],
        },
        ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![fixture_lot("treasury-lot", total_shares - 6, date)],
        },
    ];
    let registry = ShareRegistry::new(stock.clone(), issuer.clone(), total_shares, date, holdings)
        .unwrap();
    session.configure_share_registry(registry).unwrap();
    // AShareIndividual 模式下装配名册已自动为玩家开个人税账，不再显式配置。
    session
        .define_dividend_legal_facts(
            &issuer,
            AccountingAmount::from_cents(i128::from(total_shares)),
            "tiny rights test legal fact".into(),
        )
        .unwrap();
    (session, stock, issuer)
}

/// 除权锚分量谓词一致性边界：实际认购 1 股 < 配股前股本 10,000,000 的百万分之
/// 一，整数截位比例为 0；同日另有已登记现金分红时，首 tick 准备按纯现金组
/// 建锚，恢复勾稽必须使用同一谓词，不得按 `paid > 0` 期待配股组分量。
#[test]
fn tiny_paid_rights_with_same_day_cash_dividend_reconciles_at_restore() {
    let (mut session, stock, issuer) = session_with_tiny_rights_and_cash_dividend();
    // 配股日程先行推导（定向玩家 1 股：paid=1 → 整数截位比例 0），现金分红的
    // 登记日取配股除权日的前一交易日，使两者同日除权除息。
    let mut plan = all_shareholders_plan(&issuer, &stock);
    plan.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![DirectedPlacementTarget::NamedHolder {
            holder: HolderId::Account(AccountId(0)),
            shares: 1,
            lock_until: None,
        }],
    };
    let calendar = session.state.civil_clock.calendar().clone();
    let ex_date = plan.ex_rights_on;
    let dividend_registered_on = calendar
        .previous_trading_day(crate::calendar::CalendarExchange::Sse, ex_date)
        .unwrap();
    let dividend_payable_on = calendar
        .next_trading_day(crate::calendar::CalendarExchange::Sse, ex_date)
        .unwrap();
    // 现金分红：R = 除权日前一交易日登记、与配股同日除息、次一交易日派息。
    let eligible_gross_cents = 6_i64;
    let dividend = crate::company::cash_dividend::CashDividendPlan::new(
        "cash-2030".into(),
        issuer.clone(),
        stock.clone(),
        crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        CivilDate::from_iso("2030-01-02").unwrap(),
        CivilDate::from_iso("2030-01-03").unwrap(),
        dividend_registered_on,
        ex_date,
        dividend_payable_on,
        crate::money::Money::from_cents(1),
        crate::money::Money::from_cents(eligible_gross_cents),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    session
        .approve_cash_dividend(
            crate::company::DividendDeclaration {
                plan_id: dividend.plan_id.clone(),
                approved_on: dividend.approved_on,
                total_gross: AccountingAmount::from_cents(i128::from(eligible_gross_cents)),
                registered_capital: AccountingAmount::from_cents(i128::from(
                    session.state.setup.stocks[0].total_shares,
                )),
            },
            dividend,
        )
        .unwrap();
    // 配股：定向玩家 1 股，批准并推进到缴款期认购 1 股。
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan.clone()).unwrap();
    while session.civil_date() < plan.payment_start_on {
        complete_day(&mut session);
    }
    session
        .subscribe_rights_offering("rights-2030", AccountId(0), 1)
        .unwrap();
    while session.civil_date() <= settlement_on {
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    assert_eq!(
        book.subscriptions()
            .iter()
            .map(|record| record.paid_shares)
            .sum::<u64>(),
        1
    );
    assert!(book.credited_on().is_some(), "微量认购仍须完成入账");
    // 除权除息组：纯现金分量（截位为零的配股不构成组分量）。
    let group = session
        .state
        .corporate_actions
        .applied_ex_reference_groups
        .iter()
        .find(|group| group.stock == stock && group.date == plan.ex_rights_on)
        .unwrap();
    assert_eq!(group.cash_plan_ids, vec!["cash-2030".to_string()]);
    assert!(
        group.rights_event_ids.is_empty(),
        "整数截位比例为零的配股不产生除权组分量"
    );
    // 恢复勾稽必须与首 tick 准备同谓词（红：旧勾稽按 paid>0 期待组分量）。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        position_of(&restored, AccountId(0), &stock),
        5 + 1,
        "微量配股认购恢复后仍真实入账"
    );
}

#[test]
fn directed_placement_allows_named_targets_outside_registry_and_open_headroom() {
    let (mut session, stock, issuer) = session_with_registry(true);
    let mut plan = all_shareholders_plan(&issuer, &stock);
    plan.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![
            DirectedPlacementTarget::NamedHolder {
                holder: HolderId::Account(AccountId(0)),
                shares: 1_000_000,
                lock_until: None,
            },
            DirectedPlacementTarget::OpenPublicSubscription {
                shares: 1_000_000,
            },
        ],
    };
    let deadline = plan.payment_deadline_on;
    let settlement_on = plan.settlement_on;
    session.approve_rights_offering(plan.clone()).unwrap();
    while session.civil_date() < plan.payment_start_on {
        complete_day(&mut session);
    }
    // 玩家以定向对象身份认购 1,000,000 股。
    session
        .subscribe_rights_offering("rights-2030", AccountId(0), 1_000_000)
        .unwrap();
    // 玩家再次认购被拒：同一持有人只能有一条净认购记录。
    assert!(
        session
            .subscribe_rights_offering("rights-2030", AccountId(0), 1)
            .is_err(),
        "同一持有人只能有一条净认购记录"
    );
    while session.civil_date() < deadline {
        complete_day(&mut session);
    }
    while session.civil_date() <= settlement_on {
        complete_day(&mut session);
    }
    let book = session
        .state
        .corporate_actions
        .rights_offerings
        .iter()
        .find(|book| book.plan().event_id == "rights-2030")
        .unwrap();
    let settlement = book.settlement().unwrap();
    assert!(!settlement.failed, "定向增发不适用 70% 代销门槛");
    assert_eq!(settlement.total_paid_shares, 1_000_000);
    assert_eq!(
        position_of(&session, AccountId(0), &stock),
        3_000_000 + 1_000_000
    );
}
