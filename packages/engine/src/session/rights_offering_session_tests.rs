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
    session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            stock.clone(),
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
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
