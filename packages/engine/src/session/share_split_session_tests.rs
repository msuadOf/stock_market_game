//! 拆股／缩股 Session 全链路测试（S1 批 2026-10-07）。
//!
//! 覆盖：受理（面值权威链、同除权日碰撞三层预检）、公告 → R 日冻结快照换算 →
//! R+1 重新计值入账（registry `ShareReDenomination`、账户持仓、税账同日续记／
//! 非应税核减、Simple 账面回填与注册资本口径）、除权锚（拆股÷比例、缩股×比例、
//! 与同日现金红利合并）、碎股换算边界、限售消耗与取得日延续、restore 深等。

use super::*;
use crate::account::Position;
use crate::accounting::AccountingAmount;
use crate::company::rights_offering::{
    RightsOfferingEventPlan, RightsOfferingMode, RightsSubscriptionStrategy,
};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::company::share_split::{ShareSplitDirection, ShareSplitEventPlan};
use crate::orderbook::AccountId;

fn split_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 1;
    setup.ticks_per_day = 1;
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    setup
}

fn fixture_lot(id: &str, qty: u64, acquired_on: CivilDate) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on,
        source: AcquisitionSource::InitialAllocation {
            evidence: "share split fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

/// 玩家(0) 3,000,000 股 + 机构 NPC(1) 1,000,000 股 + Treasury 6,000,000 股
/// （总股本 10,000,000）；玩家已开个人税账。面值 1 分
/// （注册资本 = 总股本 × 1 分，与面值权威推导口径一致）。
fn session_with_registry() -> (GameSession, StockCode, CompanyId) {
    session_with_registry_par(1)
}

/// `par_cents`：每股面值（分）。法定注册资本 = 总股本 × 面值。
fn session_with_registry_par(par_cents: i64) -> (GameSession, StockCode, CompanyId) {
    session_with_registry_setup(split_setup(), par_cents)
}

/// [`session_with_registry_par`] 的可定制 setup 变体：split×配股碰撞用例需在
/// setup 上打开新局配股开关（其余名册／法定事实装配完全一致）。
fn session_with_registry_setup(
    setup: SessionSetup,
    par_cents: i64,
) -> (GameSession, StockCode, CompanyId) {
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
            lots: vec![fixture_lot("treasury-lot", total_shares - 4_000_000, date)],
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
            AccountingAmount::from_cents(i128::from(par_cents) * i128::from(total_shares)),
            "share split test legal fact".into(),
        )
        .unwrap();
    (session, stock, issuer)
}

/// 1 拆 2：公告 2030-01-03、登记 2030-01-04、除权 2030-01-07（周一）。
fn split_plan(issuer: &CompanyId, stock: &StockCode) -> ShareSplitEventPlan {
    ShareSplitEventPlan {
        event_id: "split-2030-01".into(),
        approval_reference: "board-2030-01".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        direction: ShareSplitDirection::Split,
        ratio: 2,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-03").unwrap(),
        registered_on: CivilDate::from_iso("2030-01-04").unwrap(),
        ex_rights_on: CivilDate::from_iso("2030-01-07").unwrap(),
    }
}

/// 7 并 1：碎股边界（3,000,000 % 7 = 3、1,000,000 % 7 = 1、6,000,000 % 7 = 6）。
fn consolidation_plan(issuer: &CompanyId, stock: &StockCode) -> ShareSplitEventPlan {
    ShareSplitEventPlan {
        event_id: "consolidation-2030-01".into(),
        approval_reference: "board-2030-01-c".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        direction: ShareSplitDirection::Consolidate,
        ratio: 7,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-03").unwrap(),
        registered_on: CivilDate::from_iso("2030-01-04").unwrap(),
        ex_rights_on: CivilDate::from_iso("2030-01-07").unwrap(),
    }
}

fn complete_day(session: &mut GameSession) {
    if session.civil_clock().phase() != crate::session::CivilPhase::ClosedDay {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

/// 与 [`split_plan`] 同除权日（2030-01-07）的配股方案：登记 2030-01-03、缴款期
/// 1 个交易日（2030-01-04）、除权 2030-01-07，面向全体股东每 10 股配 1 股，
/// 发行价 10 分/股（高于面值）。日期由 `derive_schedule` 从登记日推导。
fn same_ex_date_rights_plan(issuer: &CompanyId, stock: &StockCode) -> RightsOfferingEventPlan {
    let mut plan = RightsOfferingEventPlan {
        event_id: "rights-same-ex-collision".into(),
        approval_reference: "rights collision fixture".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-03").unwrap(),
        registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
        payment_start_on: CivilDate::from_iso("2030-01-03").unwrap(),
        payment_deadline_on: CivilDate::from_iso("2030-01-03").unwrap(),
        ex_rights_on: CivilDate::from_iso("2030-01-07").unwrap(),
        settlement_on: CivilDate::from_iso("2030-01-07").unwrap(),
        listing_on: CivilDate::from_iso("2030-01-07").unwrap(),
        price_per_share: crate::money::Money::from_cents(10),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        crate::calendar::CalendarExchange::Sse,
        1,
    )
    .unwrap();
    assert_eq!(plan.ex_rights_on, CivilDate::from_iso("2030-01-07").unwrap());
    plan
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

fn split_book<'a>(
    session: &'a GameSession,
    event_id: &str,
) -> &'a crate::company::share_split::ShareSplitBook {
    session
        .state
        .corporate_actions
        .share_splits
        .iter()
        .find(|book| book.plan().event_id == event_id)
        .unwrap()
}

#[test]
fn split_full_chain_settles_registry_accounts_tax_ex_reference_and_restores() {
    // 面值 1 分（注册资本 10,000,000 分 ÷ 10,000,000 股）：1 拆 2 不能整除 1 分
    // → 受理必须显式拒绝（面值最小单位为分）。
    let (mut session, stock, issuer) = session_with_registry_par(1);
    let mut infeasible = split_plan(&issuer, &stock);
    infeasible.event_id = "split-infeasible".into();
    let error = session.approve_share_split(infeasible).unwrap_err();
    assert!(
        error.to_string().contains("整除"),
        "面值不能整除缩小的拆股必须显式拒绝：{error}"
    );
    let _ = (&mut session, &stock, &issuer);

    // 面值可整除的 1 拆 2：面值 2 分口径（注册资本 20,000,000 分）。
    let (mut session, stock, issuer) = session_with_registry_par(2);
    let plan = split_plan(&issuer, &stock);
    let ex_date = plan.ex_rights_on;
    session.approve_share_split(plan.clone()).unwrap();
    assert_eq!(session.state.corporate_actions.share_splits.len(), 1);

    // 推进到除权日前一自然日：捕获除权前收盘（除权参考价的公式前收）。
    while session.civil_date() < ex_date {
        complete_day(&mut session);
    }
    let close_before_split = session
        .state
        .markets
        .get(&stock)
        .unwrap()
        .last_close();
    complete_day(&mut session);
    let book = split_book(&session, "split-2030-01");
    assert_eq!(
        book.settled_on(),
        Some(ex_date),
        "R+1 换算入账必须完成（当前 {}）",
        session.civil_date()
    );
    let receipt = book.receipt().unwrap();
    assert_eq!(receipt.issued_shares_before, 10_000_000);
    assert_eq!(receipt.issued_shares_after, 20_000_000);

    // 账户持仓翻倍；回购专户同规则换算（不排除）。
    assert_eq!(position_of(&session, AccountId(0), &stock), 6_000_000);
    assert_eq!(position_of(&session, AccountId(1), &stock), 2_000_000);
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &stock)
        .unwrap();
    assert_eq!(registry.issued_shares(), 20_000_000);
    let treasury = registry
        .holdings()
        .iter()
        .find(|holding| holding.holder == HolderId::IssuerTreasury)
        .unwrap();
    let treasury_qty: u64 = treasury.lots.iter().map(|lot| lot.qty).sum();
    assert_eq!(treasury_qty, 12_000_000, "专户按同一比例换算");

    // 发行人已发行股数翻倍。
    let issuer_spec = session
        .state
        .company_system
        .issuers()
        .get(&issuer)
        .unwrap();
    assert_eq!(issuer_spec.issued_shares, 20_000_000);

    // 税账：玩家增量 3,000,000 股按取得日 R+1 同日正向续记。
    let tax_book = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0))
        .unwrap();
    assert!(tax_book
        .lots()
        .iter()
        .any(|lot| lot.id.contains("share-split")));

    // Simple 账面：拆股注册资本不变、面值重锚为 1 分。
    let finance_fact = session
        .state
        .company_system
        .share_split_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "split-2030-01")
        .unwrap();
    assert_eq!(finance_fact.settled_on, Some(ex_date));
    assert_eq!(finance_fact.registered_capital_reduction.cents(), 0);
    assert_eq!(
        finance_fact.par_value_before,
        crate::money::Money::from_cents(2)
    );
    assert_eq!(
        finance_fact.par_value_after,
        crate::money::Money::from_cents(1)
    );
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital
            .cents(),
        20_000_000,
        "拆股后注册资本不变"
    );

    // 除权锚组：split_event_ids 记录、参考价 = 前收 ÷ 2。
    let group = session
        .state
        .corporate_actions
        .applied_ex_reference_groups
        .iter()
        .find(|group| group.split_event_ids.iter().any(|id| id == "split-2030-01"))
        .expect("拆股事件必须产生除权锚组");
    assert_eq!(group.date, ex_date);
    let expected_reference = crate::money::Money::from_cents(close_before_split.cents() / 2);
    assert_eq!(group.reference.reference_price, expected_reference);

    // restore 深等（整份 corporate_actions 与持仓）。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.share_splits).unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.share_splits).unwrap(),
        "拆股账簿恢复深等"
    );
    assert_eq!(
        serde_json::to_value(session.state.markets.get(&stock).unwrap().last_cash_ex_reference())
            .unwrap(),
        serde_json::to_value(restored.state.markets.get(&stock).unwrap().last_cash_ex_reference())
            .unwrap(),
        "除权锚恢复深等"
    );
    assert_eq!(position_of(&restored, AccountId(0), &stock), 6_000_000);
    let _ = &session; // 抑制第一段 fixture 的未用告警（前段只测受理拒绝）
}

#[test]
fn consolidation_full_chain_reduces_positions_tax_and_capital_with_fragments() {
    let (mut session, stock, issuer) = session_with_registry();
    let plan = consolidation_plan(&issuer, &stock);
    let ex_date = plan.ex_rights_on;
    session.approve_share_split(plan).unwrap();

    while session.civil_date() <= ex_date {
        complete_day(&mut session);
    }
    let book = split_book(&session, "consolidation-2030-01");
    assert_eq!(book.settled_on(), Some(ex_date));
    let receipt = book.receipt().unwrap();
    // 基准：3,000,000/7=428,571 r3；1,000,000/7=142,857 r1；6,000,000/7=857,142 r6；
    // 合计基准 1,428,570，floor(10,000,000/7)=1,428,571 → extra 1 给余数最大的专户。
    assert_eq!(receipt.issued_shares_after, 1_428_571);
    let treasury_outcome = receipt
        .holders
        .iter()
        .find(|outcome| outcome.holder == HolderId::IssuerTreasury)
        .unwrap();
    assert_eq!(treasury_outcome.new_shares, 857_143);
    assert_eq!(treasury_outcome.tie_break_award, 1);

    // 账户持仓按换算核减（T+1 锁定等比缩小由实现保证，此处断言总量）。
    assert_eq!(position_of(&session, AccountId(0), &stock), 428_571);
    assert_eq!(position_of(&session, AccountId(1), &stock), 142_857);
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &stock)
        .unwrap();
    assert_eq!(registry.issued_shares(), 1_428_571);

    // 税账：玩家非应税核减 3,000,000 − 428,571 = 2,571,429 股，无新增取得。
    let tax_book = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0))
        .unwrap();
    let total_tax_qty: u64 = tax_book.lots().iter().map(|lot| lot.qty).sum();
    assert_eq!(total_tax_qty, 428_571, "缩股核减后税基股数一致");
    assert!(
        !tax_book.lots().iter().any(|lot| lot.id.contains("share-split")),
        "缩股不产生新取得批次"
    );

    // Simple 账面：注册资本按旧股口径消灭面值核减。
    // S_before − 7×S_after = 10,000,000 − 9,999,997 = 3 → 核减 3 分。
    let finance_fact = session
        .state
        .company_system
        .share_split_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "consolidation-2030-01")
        .unwrap();
    assert_eq!(finance_fact.registered_capital_reduction.cents(), 3);
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital
            .cents(),
        10_000_000 - 3
    );
    // 新面值 = 1 分 × 7 = 7 分；注册资本 = 7 分 × 1,428,571 + 3 分核减勾稽。
    assert_eq!(
        finance_fact.par_value_after,
        crate::money::Money::from_cents(7)
    );

    // restore 深等。
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&session.state.corporate_actions.share_splits).unwrap(),
        serde_json::to_value(&restored.state.corporate_actions.share_splits).unwrap(),
        "缩股账簿恢复深等"
    );
    assert_eq!(position_of(&restored, AccountId(0), &stock), 428_571);
}

#[test]
fn split_and_distribution_same_ex_date_are_rejected_at_approval_both_ways() {
    let (mut session, stock, issuer) = session_with_registry_par(2);
    // 拆股先声明。
    session
        .approve_share_split(split_plan(&issuer, &stock))
        .unwrap();
    // 同除权日送转后声明：显式拒绝。
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let registered_on = calendar
        .previous_trading_day(crate::calendar::CalendarExchange::Sse, CivilDate::from_iso("2030-01-07").unwrap())
        .unwrap();
    let stock_plan = crate::company::stock_distribution::StockDistributionEventPlan {
        event_id: "stock-collision".into(),
        approval_reference: "collision fixture".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
        registered_on,
        ex_rights_on: CivilDate::from_iso("2030-01-07").unwrap(),
        shares_per_existing_share_micros: 100_000,
        approved_total_new_shares: 400_000,
    };
    let error = session
        .approve_stock_distribution(stock_plan)
        .expect_err("同除权日已有拆股事件时送转受理必须显式拒绝");
    assert!(
        error.to_string().contains("同除权日") && error.to_string().contains("拆股"),
        "送转受理拒绝必须指明同除权日拆股碰撞：{error}"
    );
    // 第二起拆股同除权日：同样拒绝。
    let mut second = split_plan(&issuer, &stock);
    second.event_id = "split-second".into();
    let error = session
        .approve_share_split(second)
        .expect_err("同除权日已有拆股事件时第二起拆股受理必须显式拒绝");
    assert!(error.to_string().contains("同除权日"));
    assert_eq!(session.state.corporate_actions.share_splits.len(), 1);
}

#[test]
fn split_and_rights_offering_same_ex_date_are_rejected_at_approval_both_ways() {
    // 拆股先声明、配股后声明且同除权日（2030-01-07）：approve_rights_offering
    // 受理时显式拒绝（拆股／缩股×配股同日的合并除权口径未核实），被拒配股
    // 不得入账簿，既有拆股账簿不受影响。
    let mut setup = split_setup();
    setup.rights_offering_enabled = true;
    let (mut session, stock, issuer) = session_with_registry_setup(setup, 2);
    session
        .approve_share_split(split_plan(&issuer, &stock))
        .unwrap();
    let error = session
        .approve_rights_offering(same_ex_date_rights_plan(&issuer, &stock))
        .expect_err("同除权日已有拆股事件时配股受理必须显式拒绝");
    assert!(
        error.to_string().contains("同除权日") && error.to_string().contains("拆股"),
        "配股受理拒绝必须指明同除权日拆股碰撞：{error}"
    );
    assert_eq!(
        session.state.corporate_actions.rights_offerings.len(),
        0,
        "被拒配股不得入账簿"
    );
    assert_eq!(
        session.state.corporate_actions.share_splits.len(),
        1,
        "既有拆股账簿不受被拒配股影响"
    );

    // 配股先声明、拆股后声明且同除权日：approve_share_split 受理时同样显式
    // 拒绝（与 approve_rights_offering 的碰撞预检对称），账簿保持不变。
    let mut setup = split_setup();
    setup.rights_offering_enabled = true;
    let (mut session, stock, issuer) = session_with_registry_setup(setup, 2);
    session
        .approve_rights_offering(same_ex_date_rights_plan(&issuer, &stock))
        .unwrap();
    let error = session
        .approve_share_split(split_plan(&issuer, &stock))
        .expect_err("同除权日已有配股事件时拆股受理必须显式拒绝");
    assert!(
        error.to_string().contains("同除权日") && error.to_string().contains("配股"),
        "拆股受理拒绝必须指明同除权日配股碰撞：{error}"
    );
    assert_eq!(
        session.state.corporate_actions.share_splits.len(),
        0,
        "被拒拆股不得入账簿"
    );
    assert_eq!(
        session.state.corporate_actions.rights_offerings.len(),
        1,
        "既有配股账簿不受被拒拆股影响"
    );
}

#[test]
fn split_with_cash_dividend_same_ex_date_merges_into_one_group() {
    // 拆股与同日现金红利合并成组：参考价 = (前收 − 红利) ÷ 2。
    let (mut session, stock, issuer) = session_with_registry_par(2);
    // 同除权日的现金分红（公告 01-03、登记 01-04、除息 01-07）。
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let registered_on = CivilDate::from_iso("2030-01-04").unwrap();
    assert!(calendar.is_trading_day(crate::calendar::CalendarExchange::Sse, registered_on).unwrap());
    assert_eq!(
        calendar
            .next_trading_day(crate::calendar::CalendarExchange::Sse, registered_on)
            .unwrap(),
        CivilDate::from_iso("2030-01-07").unwrap(),
        "fixture 登记日的次一交易日必须是除权日"
    );
    let plan = crate::company::cash_dividend::CashDividendPlan {
        plan_id: "cash-same-ex".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        gross_per_share: crate::money::Money::from_cents(1),
        distributable_amount: crate::money::Money::from_cents(40_000_000),
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-03").unwrap(),
        registered_on,
        ex_dividend_on: CivilDate::from_iso("2030-01-07").unwrap(),
        payable_on: CivilDate::from_iso("2030-01-07").unwrap(),
        formula: crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
    };
    let declaration = crate::company::DividendDeclaration {
        plan_id: plan.plan_id.clone(),
        approved_on: plan.approved_on,
        total_gross: AccountingAmount::from_cents(4_000_000),
        registered_capital: AccountingAmount::from_cents(2 * 10_000_000),
    };
    session
        .approve_cash_dividend(declaration, plan)
        .expect("同日现金分红方案受理成功");
    session
        .approve_share_split(split_plan(&issuer, &stock))
        .unwrap();

    let ex_date = CivilDate::from_iso("2030-01-07").unwrap();
    while session.civil_date() < ex_date {
        complete_day(&mut session);
    }
    let close_before = session
        .state
        .markets
        .get(&stock)
        .unwrap()
        .last_close();
    complete_day(&mut session);
    let group = session
        .state
        .corporate_actions
        .applied_ex_reference_groups
        .iter()
        .find(|group| group.date == ex_date)
        .expect("拆股×现金同日必须合并成组");
    assert!(group.split_event_ids.contains(&"split-2030-01".to_owned()));
    assert!(group.cash_plan_ids.contains(&"cash-same-ex".to_owned()));
    // (前收 − 1 分) ÷ 2，银行家舍入到分。
    let numerator = close_before.cents() - 1;
    let half = numerator / 2;
    let reference = if numerator % 2 == 0 || half % 2 == 0 {
        half
    } else {
        half + 1
    };
    assert_eq!(group.reference.reference_price.cents(), reference);
}

#[test]
fn split_tax_continuity_uses_r_plus_one_acquisition_for_the_increment() {
    // 税账连续性：拆股增量按 R+1 取得日记新批次（与送转同口径的保守解释）；
    // 原批次取得日不变。缩股核减不触发补税（处置计数为零）。
    let (mut session, stock, issuer) = session_with_registry_par(2);
    // 先登记一笔分红（使税账有应税事实可供连续性断言），再拆股。
    let registered_on = CivilDate::from_iso("2030-01-04").unwrap();
    let plan = crate::company::cash_dividend::CashDividendPlan {
        plan_id: "cash-before-split".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        gross_per_share: crate::money::Money::from_cents(2),
        distributable_amount: crate::money::Money::from_cents(40_000_000),
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-03").unwrap(),
        registered_on,
        ex_dividend_on: CivilDate::from_iso("2030-01-07").unwrap(),
        payable_on: CivilDate::from_iso("2030-01-07").unwrap(),
        formula: crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
    };
    let declaration = crate::company::DividendDeclaration {
        plan_id: plan.plan_id.clone(),
        approved_on: plan.approved_on,
        total_gross: AccountingAmount::from_cents(8_000_000),
        registered_capital: AccountingAmount::from_cents(2 * 10_000_000),
    };
    session.approve_cash_dividend(declaration, plan).unwrap();
    session
        .approve_share_split(split_plan(&issuer, &stock))
        .unwrap();
    let ex_date = CivilDate::from_iso("2030-01-07").unwrap();
    while session.civil_date() <= ex_date {
        complete_day(&mut session);
    }
    let tax_book = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0))
        .unwrap();
    let split_lot = tax_book
        .lots()
        .iter()
        .find(|lot| lot.id.contains("share-split"))
        .expect("拆股增量必须进入税账");
    assert_eq!(split_lot.qty, 3_000_000);
    assert_eq!(split_lot.acquired_on, ex_date);
    // 原批次仍在、取得日不变（2030-01-02 fixture）。
    assert!(tax_book
        .lots()
        .iter()
        .any(|lot| lot.acquired_on == CivilDate::from_iso("2030-01-02").unwrap()));
}

#[test]
fn restricted_lots_are_consumed_by_consolidation_and_survivors_keep_acquisition_dates() {
    // 限售股份参与缩股：未解禁 lot 允许被重新计值消耗（不是转让）；
    // 存活 lot 保留原 id 与取得日（取得日延续）。
    let setup = split_setup();
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
    let mut restricted_player_lot = fixture_lot("player-lot", 3_000_000, date);
    restricted_player_lot.restriction = ShareRestriction::Restricted {
        reason: "ipo-lock".into(),
        release_on: CivilDate::from_iso("2031-06-01").unwrap(),
    };
    let holdings = vec![
        ShareHolding {
            holder: HolderId::Account(AccountId(0)),
            lots: vec![restricted_player_lot],
        },
        ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![fixture_lot("institution-lot", 1_000_000, date)],
        },
        ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![fixture_lot("treasury-lot", total_shares - 4_000_000, date)],
        },
    ];
    let registry =
        ShareRegistry::new(stock.clone(), issuer.clone(), total_shares, date, holdings).unwrap();
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
            AccountingAmount::from_cents(2 * i128::from(total_shares)),
            "par-two-cents legal fact".into(),
        )
        .unwrap();
    // 2 并 1：面值 2 分 → 4 分；玩家 3,000,000 → 1,500,000。
    let mut plan = split_plan(&issuer, &stock);
    plan.direction = ShareSplitDirection::Consolidate;
    plan.ratio = 2;
    plan.event_id = "consolidation-restricted".into();
    session.approve_share_split(plan).unwrap();
    let ex_date = CivilDate::from_iso("2030-01-07").unwrap();
    while session.civil_date() <= ex_date {
        complete_day(&mut session);
    }
    let book = split_book(&session, "consolidation-restricted");
    assert_eq!(book.settled_on(), Some(ex_date));
    assert_eq!(position_of(&session, AccountId(0), &stock), 1_500_000);
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &stock)
        .unwrap();
    let player_holding = registry
        .holdings()
        .iter()
        .find(|holding| holding.holder == HolderId::Account(AccountId(0)))
        .unwrap();
    let survivor = player_holding
        .lots
        .iter()
        .find(|lot| lot.id == "player-lot")
        .expect("FIFO 消耗后存活 lot 保留原 id");
    assert_eq!(survivor.qty, 1_500_000);
    assert_eq!(survivor.acquired_on, CivilDate::from_iso("2030-01-02").unwrap());
    assert!(matches!(
        survivor.restriction,
        ShareRestriction::Restricted { .. }
    ));
}

#[test]
fn split_failure_before_settlement_leaves_no_partial_state() {
    // 受理后至 R+1 前不产生名册回执与持仓变化；除权日前账簿停在 Registered。
    let (mut session, stock, issuer) = session_with_registry_par(2);
    session
        .approve_share_split(split_plan(&issuer, &stock))
        .unwrap();
    // 推进到登记日日终（R 日）：账簿 Registered、名册无重新计值回执。
    let registered_on = CivilDate::from_iso("2030-01-04").unwrap();
    while session.civil_date() < registered_on {
        complete_day(&mut session);
    }
    complete_day(&mut session);
    let book = split_book(&session, "split-2030-01");
    assert_eq!(
        book.status(),
        &crate::company::share_split::ShareSplitStatus::Registered
    );
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &stock)
        .unwrap();
    assert!(
        registry
            .receipts()
            .iter()
            .all(|receipt| !matches!(
                receipt.request.scope,
                crate::company::share_registry::MovementScope::ShareReDenomination { .. }
            )),
        "R 日只冻结快照，不产生重新计值回执"
    );
    assert_eq!(position_of(&session, AccountId(0), &stock), 3_000_000);
}
