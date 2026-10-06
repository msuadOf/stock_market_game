//! 公司行为偏好自动提案的 Session 级测试（ADR-0037，P 批）。
//!
//! 覆盖：结算周期完成后按偏好自动产生现金分红/送转方案并走与显式 API 完全
//! 相同的 approve/announce/登记/入账状态机（含全链路到账/入账）、偏好提案被
//! 制度拒绝时如实记录且不重试风暴、同周期恢复重放幂等、未配置不产生方案、
//! 严格持久化（新字段缺失的旧档显式拒绝）与恢复后不重复产生历史方案。

use super::*;
use crate::account::Position;
use crate::company::config::CompanySystemConfig;
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::company::simple::preferences::{
    SimpleCashDividendPreference, SimpleCompanyPreferences, SimplePreferenceProposalKind,
    SimpleStockDistributionPreference,
};
use crate::company::{CompanyId, DividendDeclaration};
use crate::money::Money;

fn d(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn preference_setup(preferences: SimpleCompanyPreferences) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.start_date = d("2030-01-20");
    // 缩短每个交易日的 tick 数：本组只验证跨自然日的公司行为链路，
    // 无需完整 100 tick 行情日（单 case 10 秒纪律）。
    setup.ticks_per_day = 2;
    if let CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.companies[0].preferences = preferences;
    }
    setup
}

fn cash_preference(payout_bp: u16, min_cents: i64, cycles: u16) -> SimpleCompanyPreferences {
    SimpleCompanyPreferences {
        cash_dividend: Some(SimpleCashDividendPreference {
            target_payout_bp: payout_bp,
            min_distributable_profit: Money::from_cents(min_cents),
            cycles_between_proposals: cycles,
        }),
        stock_distribution: None,
    }
}

fn stock_preference(
    min_cents: i64,
    ratio_micros: u64,
    max_expansion_micros: u64,
    cycles: u16,
) -> SimpleCompanyPreferences {
    SimpleCompanyPreferences {
        cash_dividend: None,
        stock_distribution: Some(SimpleStockDistributionPreference {
            min_distributable_profit: Money::from_cents(min_cents),
            shares_per_existing_share_micros: ratio_micros,
            max_cumulative_expansion_micros: max_expansion_micros,
            cycles_between_proposals: cycles,
        }),
    }
}

/// 装配带完整股东名册的会话：玩家持有 `player_qty` 股、具名外部股东 1 股、
/// 其余为库藏股。注册资本法定事实按 `registered_capital_cents` 显式绑定。
fn session_with_registry(
    preferences: SimpleCompanyPreferences,
    player_qty: u64,
    registered_capital_cents: i128,
) -> (GameSession, CompanyId, StockCode) {
    let mut session = GameSession::new(preference_setup(preferences), 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let total_shares = session.state.setup.stocks[0].total_shares;
    session
        .define_dividend_legal_facts(
            &issuer,
            crate::accounting::AccountingAmount::from_cents(registered_capital_cents),
            "simple-preference fixture legal fact".into(),
        )
        .unwrap();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            Position::from_restored_parts(player_qty.try_into().unwrap(), 0, 1_000, 0),
        );
    let lot = |id: &str, qty: u64| ShareLot {
        id: id.into(),
        qty,
        acquired_on: session.civil_date(),
        source: AcquisitionSource::InitialAllocation {
            evidence: "simple-preference fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        total_shares,
        session.civil_date(),
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![lot("player-lot", player_qty)],
            },
            ShareHolding {
                holder: HolderId::External("external-holder".into()),
                lots: vec![lot("external-lot", 1)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![lot("treasury-lot", total_shares - player_qty - 1)],
            },
        ],
    )
    .unwrap();
    session.configure_share_registry(registry).unwrap();
    (session, issuer, stock)
}

/// 完成一个自然日：交易日步进当日全部 tick 后日结；休市日直接日结
/// （全部交易所休市的自然日不能推进市场 tick）。
fn run_day(session: &mut GameSession) {
    if session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

/// 推进到 `target`（不含）为止：每个自然日完整步进并日结。
fn advance_to(session: &mut GameSession, target: CivilDate) {
    while session.civil_date() < target {
        run_day(session);
    }
}

/// 从 2030-01-20 起推进到首个结算周期（月度）末日 2030-01-31 完成日结之后。
fn advance_through_first_period_end(session: &mut GameSession) {
    advance_to(session, d("2030-02-01"));
}

fn rejections(
    session: &GameSession,
    issuer: &CompanyId,
) -> Vec<crate::company::simple::preferences::SimplePreferenceRejection> {
    session
        .state
        .company_system
        .preference_rejections(issuer)
        .unwrap()
        .to_vec()
}

#[test]
fn cash_preference_auto_proposes_after_settlement_and_pays_full_chain() {
    let (mut session, issuer, stock) =
        session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000);
    let player_cash_before = session.account(AccountId(0)).unwrap().cash();
    advance_through_first_period_end(&mut session);
    // 周期末日 2030-01-31 结算完成后自动产生方案：确定性 id、批准=公告=次日。
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "应恰好产生一个自动分红方案"
    );
    assert_eq!(
        plan.plan_id,
        "simple-preference:C-600888:dividend:2030-01-31"
    );
    assert_eq!(plan.approved_on, d("2030-02-01"));
    assert_eq!(plan.announced_on, d("2030-02-01"));
    assert!(
        plan.gross_per_share.cents() > 0,
        "派息额应为正：可分配利润充足且非库藏股基数庞大"
    );
    assert!(rejections(&session, &issuer).is_empty());
    // 与显式方案同一状态机：Simple 批准事实同步存在。
    let facts = session
        .state
        .company_system
        .dividend_plan_facts(&issuer)
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].plan_id, plan.plan_id);
    // 推进过公告、登记、除息与派发：公告走既有 typed 通道，玩家真实税前到账。
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash(),
        player_cash_before
            .add(Money::from_cents(plan.gross_per_share.cents() * 6_000_000))
            .unwrap(),
        "玩家按登记持股 6,000,000 股真实税前到账（Exempt 模式不扣税）"
    );
    let receipts: Vec<_> = session
        .state
        .corporate_actions
        .account_gross_receipts
        .iter()
        .filter(|receipt| receipt.account == AccountId(0) && receipt.plan_id == plan.plan_id)
        .collect();
    assert_eq!(receipts.len(), 1, "玩家到账凭证恰好一条");
    assert_eq!(
        receipts[0].gross,
        Money::from_cents(plan.gross_per_share.cents() * 6_000_000)
    );
    assert_eq!(stock.0, "600888");
    let instant = crate::calendar::CivilInstant::new(plan.announced_on, 64_800)
        .expect("18:00:00 是合法自然日瞬间");
    let announcements = session
        .state
        .library
        .announcements_for_company(&issuer, instant);
    assert!(
        announcements.iter().any(|announcement| matches!(
            &announcement.content,
            crate::information::AnnouncementContent::CashDividend(typed)
                if typed.plan.plan_id == plan.plan_id
        )),
        "偏好自动方案必须走既有 typed 现金分红公告通道"
    );
}

#[test]
fn cash_preference_records_rejection_when_payout_dilutes_below_one_cent() {
    // 派息 1bp 摊到近全部流通股（约 10^7 股）：目标派息额每股不足一分，
    // 构造失败被如实记录为拒绝，日结不失败，且同一结算周期不重复记录。
    let (mut session, issuer, _stock) =
        session_with_registry(cash_preference(1, 1, 1), 9_999_998, 10_000_000);
    advance_through_first_period_end(&mut session);
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "无法成案时不得产生分红账簿"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1, "同周期恰好一条拒绝记录");
    assert_eq!(rows[0].company, issuer);
    assert_eq!(rows[0].evaluated_on, d("2030-01-31"));
    assert_eq!(rows[0].kind, SimplePreferenceProposalKind::CashDividend);
    assert!(
        rows[0].detail.contains("每股不足一分"),
        "拒绝原因应如实保留：{}",
        rows[0].detail
    );
    // 恢复重放同一日结：不新增记录。
    let save = session.save().unwrap();
    let mut replayed = GameSession::restore(&save).unwrap();
    advance_to(&mut replayed, d("2030-02-02"));
    let rows = rejections(&replayed, &issuer);
    assert_eq!(rows.len(), 1, "下一周期（02-28）前不得重复评估记录");
}

#[test]
fn stock_preference_rejection_is_recorded_when_par_value_exceeds_profit() {
    // 注册资本法定事实取 10^17 分（面值 10^10 分/股）：送股 6 股面值总额远超
    // 可分配利润，制度校验拒绝提案并如实记录，日结照常成功。
    let (mut session, issuer, _stock) = session_with_registry(
        stock_preference(1, 1_000_000, 1_000_000_000, 1),
        5,
        100_000_000_000_000_000,
    );
    advance_through_first_period_end(&mut session);
    assert!(
        session
            .state
            .corporate_actions
            .stock_distributions
            .is_empty(),
        "送转被拒时不得产生送转账簿"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].kind,
        SimplePreferenceProposalKind::StockDistribution
    );
    assert!(
        rows[0].detail.contains("超过可分配利润"),
        "拒绝原因应指向可分配利润制度校验：{}",
        rows[0].detail
    );
}

#[test]
fn stock_preference_auto_proposes_and_credits_shares_full_chain() {
    // 10 送 10：玩家 5 股 + 外部 1 股 = 6 股基数，获批新增 6 股；复用 B1 的
    // R 日冻结分配与 R+1 非交易过户入账、发行股数守恒与注册资本演进语义。
    let (mut session, issuer, stock) = session_with_registry(
        stock_preference(1, 1_000_000, 1_000_000_000, 1),
        5,
        10_000_000,
    );
    advance_through_first_period_end(&mut session);
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        1,
        "应恰好产生一个自动送转方案"
    );
    let plan = session.state.corporate_actions.stock_distributions[0]
        .plan()
        .clone();
    assert_eq!(
        plan.event_id,
        "simple-preference:C-600888:stock-distribution:2030-01-31"
    );
    assert_eq!(plan.approved_total_new_shares, 6);
    assert_eq!(
        plan.kind,
        crate::company::stock_distribution::StockDistributionKind::BonusShares
    );
    advance_to(&mut session, plan.ex_rights_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.stock_distributions[0].status(),
        &crate::company::stock_distribution::StockDistributionStatus::Credited
    );
    assert_eq!(
        u64::from(
            session
                .account(AccountId(0))
                .unwrap()
                .position(&stock)
                .unwrap()
                .qty()
        ),
        10_u64,
        "玩家 5 股按 1:1 送转后应为 10 股"
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        10_000_000 + 6,
        "发行股数守恒演进"
    );
    let legal = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap();
    assert_eq!(
        legal.registered_capital,
        crate::accounting::AccountingAmount::from_cents(10_000_000 + 6),
        "注册资本按面值×新增股数演进"
    );
}

#[test]
fn same_period_replay_after_restore_produces_single_proposal() {
    // 同日幂等：日结前保存、恢复、完成周期末日结——两次独立重放各产生恰好
    // 一个同 id 方案。市场轨迹允许因并发受理调度合法不同（ADR-0017 修订），
    // 因此深等断言只覆盖确定性的公司系统子树（偏好配置、台账与批准事实）。
    let mut first = session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000).0;
    advance_to(&mut first, d("2030-01-31"));
    let save = first.save().unwrap();
    let mut run_one = GameSession::restore(&save).unwrap();
    run_day(&mut run_one);
    assert_eq!(run_one.state.corporate_actions.dividends.len(), 1);
    let encoded_one = serde_json::to_value(run_one.save().unwrap()).unwrap();

    let mut run_two = GameSession::restore(&save).unwrap();
    run_day(&mut run_two);
    assert_eq!(run_two.state.corporate_actions.dividends.len(), 1);
    assert_eq!(
        run_two.state.corporate_actions.dividends[0].plan().plan_id,
        run_one.state.corporate_actions.dividends[0].plan().plan_id,
        "同周期重放必须复用确定性方案 id"
    );
    assert_eq!(
        run_two.state.corporate_actions.dividends[0]
            .plan()
            .gross_per_share,
        run_one.state.corporate_actions.dividends[0]
            .plan()
            .gross_per_share
    );
    let encoded_two = serde_json::to_value(run_two.save().unwrap()).unwrap();
    assert_eq!(
        encoded_one["company_system"], encoded_two["company_system"],
        "同一保存点的两次日结重放必须产生深度相等的公司系统状态"
    );
}

#[test]
fn unconfigured_preferences_produce_no_proposals_or_rejections() {
    let (mut session, issuer, _stock) =
        session_with_registry(SimpleCompanyPreferences::none(), 5, 10_000_000);
    advance_to(&mut session, d("2030-03-01"));
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "未配置偏好不得自动产生现金分红方案"
    );
    assert!(
        session
            .state
            .corporate_actions
            .stock_distributions
            .is_empty(),
        "未配置偏好不得自动产生送转方案"
    );
    assert!(
        rejections(&session, &issuer).is_empty(),
        "未配置偏好不评估、不产生拒绝记录"
    );
}

#[test]
fn restore_preserves_ledger_and_does_not_duplicate_historical_proposals() {
    let (mut session, issuer, _stock) =
        session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000);
    advance_through_first_period_end(&mut session);
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    // 恢复深等：再次保存与原保存深度相等（偏好配置与台账进入存档契约）。
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap()
    );
    // 恢复后不重复产生历史方案；下一周期产生新的确定性方案。
    advance_to(&mut restored, d("2030-03-01"));
    let books = &restored.state.corporate_actions.dividends;
    assert_eq!(books.len(), 2, "两个结算周期各一个方案");
    assert_eq!(
        books[0].plan().plan_id,
        "simple-preference:C-600888:dividend:2030-01-31"
    );
    assert_eq!(
        books[1].plan().plan_id,
        "simple-preference:C-600888:dividend:2030-02-28"
    );
    assert!(rejections(&restored, &issuer).is_empty());
}

#[test]
fn old_save_without_preferences_fields_is_explicitly_rejected() {
    let (session, _issuer, _stock) =
        session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000);
    let encoded = serde_json::to_value(session.save().unwrap()).unwrap();

    // 1) setup.company_system.config.companies[0] 缺 preferences。
    let mut legacy = encoded.clone();
    legacy["setup"]["company_system"]["config"]["companies"][0]
        .as_object_mut()
        .unwrap()
        .remove("preferences");
    assert!(
        serde_json::from_value::<SaveSlot>(legacy).is_err(),
        "旧档 setup 缺 preferences 字段必须显式拒绝"
    );

    // 2) company_system 状态内 config.companies[0] 缺 preferences。
    let mut legacy = encoded.clone();
    legacy["company_system"]["implementation"]["state"]["config"]["companies"][0]
        .as_object_mut()
        .unwrap()
        .remove("preferences");
    assert!(
        serde_json::from_value::<SaveSlot>(legacy).is_err(),
        "旧档公司系统状态缺 preferences 字段必须显式拒绝"
    );

    // 3) 公司状态缺 preference_ledger。
    let mut legacy = encoded.clone();
    let company_key = legacy["company_system"]["implementation"]["state"]["companies"]
        .as_object_mut()
        .unwrap()
        .keys()
        .next()
        .cloned()
        .unwrap();
    legacy["company_system"]["implementation"]["state"]["companies"][&company_key]
        .as_object_mut()
        .unwrap()
        .remove("preference_ledger");
    assert!(
        serde_json::from_value::<SaveSlot>(legacy).is_err(),
        "旧档公司状态缺 preference_ledger 字段必须显式拒绝"
    );

    // 对照：完整档可解析（正例）。
    assert!(serde_json::from_value::<SaveSlot>(encoded).is_ok());
}

#[test]
fn preference_configuration_validates_through_setup() {
    let mut setup = preference_setup(cash_preference(0, 1, 1));
    assert!(setup.validate().is_err(), "0bp 派息比例必须在装配期被拒绝");
    let mut setup = preference_setup(cash_preference(3_000, 1, 1));
    assert!(setup.validate().is_ok());
    // 显式 API 与偏好自动提案共享入口：显式构造仍可正常提交（并存语义）。
    let (mut session, issuer, stock) =
        session_with_registry(SimpleCompanyPreferences::none(), 5, 10_000_000);
    advance_to(&mut session, d("2030-01-28"));
    let approved_on = session.civil_date();
    let calendar = session.state.civil_clock.calendar().clone();
    let registered_on = if calendar
        .is_trading_day(
            crate::calendar::CalendarExchange::Sse,
            approved_on.next().unwrap(),
        )
        .unwrap()
    {
        approved_on.next().unwrap()
    } else {
        calendar
            .next_trading_day(
                crate::calendar::CalendarExchange::Sse,
                approved_on.next().unwrap(),
            )
            .unwrap()
    };
    let ex_date = calendar
        .next_trading_day(crate::calendar::CalendarExchange::Sse, registered_on)
        .unwrap();
    let plan = crate::company::cash_dividend::CashDividendPlan::new(
        "explicit-plan-coexists".into(),
        issuer.clone(),
        stock.clone(),
        crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        approved_on,
        approved_on.next().unwrap(),
        registered_on,
        ex_date,
        ex_date,
        Money::from_cents(1),
        Money::from_cents(10_000_000),
        &calendar,
    )
    .unwrap();
    session
        .approve_cash_dividend(
            DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on,
                total_gross: crate::accounting::AccountingAmount::from_cents(6),
                registered_capital: crate::accounting::AccountingAmount::from_cents(10_000_000),
            },
            plan,
        )
        .unwrap();
    assert_eq!(session.state.corporate_actions.dividends.len(), 1);
}
