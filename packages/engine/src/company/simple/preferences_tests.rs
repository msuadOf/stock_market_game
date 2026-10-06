//! `preferences.rs` 的定向短单测：配置域、评估数学、频率与幂等键、台账记录。
//! 全链路（日结接线、approve 状态机、持久化）见
//! `session/simple_preferences_session_tests.rs`。

use super::preferences::{
    combined_group_required_positive_cents, evaluate_cash_dividend_preference,
    evaluate_stock_distribution_preference, exposure_trading_days, limit_down_margin_floor_cents,
    proposal_due, SimpleCashDividendEvaluation, SimpleCashDividendExposure,
    SimpleCashDividendOutcome, SimpleCashDividendPreference, SimpleCompanyPreferences,
    SimplePreferenceProposalKind, SimpleStockDistributionEvaluation,
    SimpleStockDistributionExposure, SimpleStockDistributionOutcome,
    SimpleStockDistributionPreference,
};
use crate::account::StockCode;
use crate::accounting::AccountingAmount;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::company::dividend::DistributableProfit;
use crate::company::CompanyId;
use crate::money::Money;

fn d(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

/// 评估输入持有的公司/证券身份的进程级固定引用（OnceLock 保证存续）。
fn fixture_ids() -> (&'static CompanyId, &'static StockCode) {
    static IDS: std::sync::OnceLock<(CompanyId, StockCode)> = std::sync::OnceLock::new();
    let ids = IDS.get_or_init(|| (CompanyId("co-a".into()), StockCode("600101".into())));
    (&ids.0, &ids.1)
}

fn trading_calendar() -> &'static TradingCalendar {
    static CALENDAR: std::sync::OnceLock<TradingCalendar> = std::sync::OnceLock::new();
    CALENDAR.get_or_init(|| TradingCalendar::current_default_calendar().unwrap())
}

/// `date` 起（含）的第一个交易日（与评估函数同推导，用于测试锚定）。
fn first_trading_day_on_or_after(exchange: CalendarExchange, date: CivilDate) -> CivilDate {
    let calendar = trading_calendar();
    if calendar.is_trading_day(exchange, date).unwrap() {
        date
    } else {
        calendar.next_trading_day(exchange, date).unwrap()
    }
}

/// 自动提案的登记日推导（与评估函数同规则：公告日 ≠ 登记日）。
fn preference_registered_on(exchange: CalendarExchange, announced_on: CivilDate) -> CivilDate {
    let calendar = trading_calendar();
    let first = first_trading_day_on_or_after(exchange, announced_on);
    if first == announced_on {
        calendar.next_trading_day(exchange, first).unwrap()
    } else {
        first
    }
}

fn profit(available_cents: i128) -> DistributableProfit {
    DistributableProfit {
        accumulated_after_loss: AccountingAmount::from_cents(available_cents),
        statutory_reserve: AccountingAmount::ZERO,
        available_for_distribution: AccountingAmount::from_cents(available_cents),
        reserve_basis_year: None,
    }
}

fn cash_preference(payout_bp: u16, min_cents: i64, cycles: u16) -> SimpleCashDividendPreference {
    SimpleCashDividendPreference {
        target_payout_bp: payout_bp,
        min_distributable_profit: Money::from_cents(min_cents),
        cycles_between_proposals: cycles,
    }
}

fn stock_preference(
    min_cents: i64,
    ratio_micros: u64,
    max_expansion_micros: u64,
    cycles: u16,
) -> SimpleStockDistributionPreference {
    SimpleStockDistributionPreference {
        min_distributable_profit: Money::from_cents(min_cents),
        shares_per_existing_share_micros: ratio_micros,
        max_cumulative_expansion_micros: max_expansion_micros,
        cycles_between_proposals: cycles,
    }
}

fn cash_exposure(last_close_cents: i64) -> SimpleCashDividendExposure {
    SimpleCashDividendExposure {
        last_close: Money::from_cents(last_close_cents),
        limit_bps: 1_000,
        same_ex_date_gross_per_share: Money::ZERO,
        same_ex_date_ratio_micros: None,
    }
}

fn cash_input<'a>(
    preference: &'a SimpleCashDividendPreference,
    distributable: DistributableProfit,
    eligible_shares: u64,
    registered_capital: Option<AccountingAmount>,
    last_proposal_period_end: Option<CivilDate>,
    suppress_duplicate: bool,
) -> SimpleCashDividendEvaluation<'a> {
    SimpleCashDividendEvaluation {
        preference,
        company: fixture_ids().0,
        stock: fixture_ids().1,
        exchange: CalendarExchange::Sse,
        // 2030-01-31 为月度结算周期末日；次日为批准/公告日（2030-02-01 为
        // 周五交易日，登记日为其下一交易日，敞露交易日数 n = 2）。
        period_end: d("2030-01-31"),
        approve_on: d("2030-02-01"),
        calendar: trading_calendar(),
        distributable,
        eligible_shares,
        exposure: Some(cash_exposure(1_000)),
        registered_capital,
        last_proposal_period_end,
        cycle_months: 1,
        suppress_duplicate,
    }
}

/// 测试侧独立计数：(from_exclusive, to_inclusive] 内的交易所交易日数。
/// 独立于实现（逐自然日查询日历），避免与 `exposure_trading_days` 循环自证。
fn independent_exposed_days(
    exchange: CalendarExchange,
    from_exclusive: CivilDate,
    to_inclusive: CivilDate,
) -> u32 {
    let calendar = trading_calendar();
    let mut count = 0_u32;
    let mut cursor = from_exclusive;
    while cursor < to_inclusive {
        cursor = cursor.next().unwrap();
        if calendar.is_trading_day(exchange, cursor).unwrap() {
            count += 1;
        }
    }
    count
}

#[test]
fn none_preferences_validate_and_carry_no_items() {
    assert!(SimpleCompanyPreferences::none().validate().is_ok());
    assert!(SimpleCompanyPreferences::none().cash_dividend.is_none());
    assert!(SimpleCompanyPreferences::none()
        .stock_distribution
        .is_none());
}

#[test]
fn preference_config_ranges_are_enforced() {
    let mut preferences = SimpleCompanyPreferences::none();
    preferences.cash_dividend = Some(cash_preference(0, 100, 1));
    assert!(preferences.validate().is_err(), "派息比例 0bp 必须拒绝");
    preferences.cash_dividend = Some(cash_preference(10_001, 100, 1));
    assert!(
        preferences.validate().is_err(),
        "派息比例超 10000bp 必须拒绝"
    );
    preferences.cash_dividend = Some(cash_preference(5_000, 0, 1));
    assert!(preferences.validate().is_err(), "门槛 0 分必须拒绝");
    preferences.cash_dividend = Some(cash_preference(5_000, 100, 0));
    assert!(preferences.validate().is_err(), "间隔周期 0 必须拒绝");
    preferences.cash_dividend = Some(cash_preference(5_000, 100, 1));
    assert!(preferences.validate().is_ok());

    preferences.stock_distribution = Some(stock_preference(0, 100_000, 1_000_000, 1));
    assert!(preferences.validate().is_err(), "送转门槛 0 分必须拒绝");
    preferences.stock_distribution = Some(stock_preference(100, 0, 1_000_000, 1));
    assert!(preferences.validate().is_err(), "送转比例 0 必须拒绝");
    preferences.stock_distribution = Some(stock_preference(100, 10_000_001, 1_000_000, 1));
    assert!(preferences.validate().is_err(), "送转比例超上限必须拒绝");
    preferences.stock_distribution = Some(stock_preference(100, 100_000, 0, 1));
    assert!(preferences.validate().is_err(), "扩张上限 0 必须拒绝");
    preferences.stock_distribution = Some(stock_preference(100, 100_000, 1_000_000_000, 1));
    assert!(preferences.validate().is_ok());
}

#[test]
fn cash_evaluation_floors_per_share_and_rebuilds_exact_total() {
    let preference = cash_preference(3_000, 1, 1);
    // 可分配 100_000 分（1000.00 元）× 30% = 30_000 分；300 股每股 100 分，整除。
    let input = cash_input(
        &preference,
        profit(100_000),
        300,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    let SimpleCashDividendOutcome::Proposal { declaration, plan } =
        evaluate_cash_dividend_preference(&input)
    else {
        panic!("应构造出现金分红提案");
    };
    assert_eq!(plan.gross_per_share, Money::from_cents(100));
    assert_eq!(
        declaration.total_gross,
        AccountingAmount::from_cents(30_000)
    );
    assert_eq!(plan.distributable_amount, Money::from_cents(100_000));
    assert_eq!(
        declaration.registered_capital,
        AccountingAmount::from_cents(1_000_000)
    );
    // 日期链：批准=公告=评估次日；登记为公告日后的首个交易日且不与公告日
    // 同日（保证 typed 公告在公告日日结仍处于 Announced 状态），除息=登记日
    // 次一交易日，派发同除息日。
    let expected_registered = preference_registered_on(CalendarExchange::Sse, d("2030-02-01"));
    let expected_ex = trading_calendar()
        .next_trading_day(CalendarExchange::Sse, expected_registered)
        .unwrap();
    assert!(expected_registered > d("2030-02-01"));
    assert_eq!(plan.approved_on, d("2030-02-01"));
    assert_eq!(plan.announced_on, d("2030-02-01"));
    assert_eq!(plan.registered_on, expected_registered);
    assert_eq!(plan.ex_dividend_on, expected_ex);
    assert_eq!(plan.payable_on, expected_ex);
    assert_eq!(
        declaration.plan_id,
        "simple-preference:co-a:dividend:2030-01-31"
    );
}

#[test]
fn cash_evaluation_rounds_down_per_share_total() {
    let preference = cash_preference(1_000, 1, 1);
    // 1001 分 × 10% = 100.1 分 → 每股 100 分（向下取整），总额 100 分。
    let input = cash_input(
        &preference,
        profit(1001),
        1,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    let SimpleCashDividendOutcome::Proposal { declaration, plan } =
        evaluate_cash_dividend_preference(&input)
    else {
        panic!("应构造出现金分红提案");
    };
    assert_eq!(plan.gross_per_share, Money::from_cents(100));
    assert_eq!(declaration.total_gross, AccountingAmount::from_cents(100));
}

#[test]
fn cash_evaluation_caps_per_share_by_schedule_limit_down_margin() {
    // 游戏化保护：每股派息封顶 = floor(锚收盘 × (1 − n×涨跌幅)) − 必留正参考价
    // 分子，n 为 (周期末, 登记日] 的交易日数（本日程公告日 2030-02-01 为交易
    // 日，n=2）。锚 1120 分、10% 限制：floor(1120×0.8) − 1 = 895 分。
    let preference = cash_preference(10_000, 1, 1);
    let mut input = cash_input(
        &preference,
        profit(10_000_000),
        1,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    input.exposure = Some(cash_exposure(1_120));
    let expected_days = independent_exposed_days(CalendarExchange::Sse, d("2030-01-31"), {
        let registered = preference_registered_on(CalendarExchange::Sse, d("2030-02-01"));
        registered
    });
    assert_eq!(expected_days, 2, "公告日为交易日时敞露 2 个交易日");
    let expected_cap = 1_120_i128 * (10_000 - i128::from(expected_days) * 1_000) / 10_000 - 1;
    let SimpleCashDividendOutcome::Proposal { declaration, plan } =
        evaluate_cash_dividend_preference(&input)
    else {
        panic!("应构造出现金分红提案");
    };
    assert_eq!(
        i128::from(plan.gross_per_share.cents()),
        expected_cap,
        "目标派息远超余量时应按日程跌停余量封顶"
    );
    // 单股基数：决议总额 = 每股红利 × 1 股，反乘保持精确相等。
    assert_eq!(
        declaration.total_gross,
        AccountingAmount::from_cents(i128::from(plan.gross_per_share.cents()))
    );

    // 公告日休市（2030-02-02 为周六）：敞露交易日数降为 1（仅登记日），
    // 封顶随之放宽——证明 n 按日程推导而非拍脑袋固定 2 天。
    let input = SimpleCashDividendEvaluation {
        period_end: d("2030-02-01"),
        approve_on: d("2030-02-02"),
        exposure: Some(cash_exposure(1_120)),
        ..input
    };
    let registered = preference_registered_on(CalendarExchange::Sse, d("2030-02-02"));
    let expected_days =
        independent_exposed_days(CalendarExchange::Sse, d("2030-02-01"), registered);
    assert_eq!(expected_days, 1, "公告日休市时仅登记日 1 个敞露交易日");
    let expected_cap = 1_120_i128 * (10_000 - i128::from(expected_days) * 1_000) / 10_000 - 1;
    let SimpleCashDividendOutcome::Proposal { plan, .. } =
        evaluate_cash_dividend_preference(&input)
    else {
        panic!("应构造出现金分红提案");
    };
    assert_eq!(i128::from(plan.gross_per_share.cents()), expected_cap);

    // 同除息日既有方案 gross 合并计入：既有 500 分时本提案被封到
    // floor(1120×0.8) − 1 − 500 = 395 分。
    let mut input = cash_input(
        &preference,
        profit(10_000_000),
        1,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    input.exposure = Some(SimpleCashDividendExposure {
        last_close: Money::from_cents(1_120),
        limit_bps: 1_000,
        same_ex_date_gross_per_share: Money::from_cents(500),
        same_ex_date_ratio_micros: None,
    });
    let SimpleCashDividendOutcome::Proposal { plan, .. } =
        evaluate_cash_dividend_preference(&input)
    else {
        panic!("应构造出现金分红提案");
    };
    assert_eq!(plan.gross_per_share, Money::from_cents(395));

    // 既有 gross 占满封顶：每股不足一分，如实拒绝。
    input.exposure = Some(SimpleCashDividendExposure {
        last_close: Money::from_cents(1_120),
        limit_bps: 1_000,
        same_ex_date_gross_per_share: Money::from_cents(895),
        same_ex_date_ratio_micros: None,
    });
    match evaluate_cash_dividend_preference(&input) {
        SimpleCashDividendOutcome::Rejected { detail } => {
            assert!(
                detail.contains("占满") && detail.contains("封顶"),
                "拒绝原因应指向合并封顶：{detail}"
            );
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }

    // 同除息日既有送转事件（10 送 10）：合并除权组的分子须保留
    // 1 + ceil(1e6/1e6) = 2 分，封顶收紧 1 分。
    input.exposure = Some(SimpleCashDividendExposure {
        last_close: Money::from_cents(1_120),
        limit_bps: 1_000,
        same_ex_date_gross_per_share: Money::ZERO,
        same_ex_date_ratio_micros: Some(1_000_000),
    });
    let SimpleCashDividendOutcome::Proposal { plan, .. } =
        evaluate_cash_dividend_preference(&input)
    else {
        panic!("应构造出现金分红提案");
    };
    assert_eq!(plan.gross_per_share, Money::from_cents(894));

    // 缺行情锚：无法约束上限，如实记录拒绝。
    let mut input = input;
    input.exposure = None;
    assert!(matches!(
        evaluate_cash_dividend_preference(&input),
        SimpleCashDividendOutcome::Rejected { .. }
    ));
}

#[test]
fn limit_down_margin_helpers_match_schedule_and_reject_exhausted_margin() {
    // 敞口计数与日程一致：公告日为交易日 n=2；公告日休市 n=1。
    let registered_trading_announce =
        preference_registered_on(CalendarExchange::Sse, d("2030-02-01"));
    assert_eq!(
        exposure_trading_days(
            trading_calendar(),
            CalendarExchange::Sse,
            d("2030-01-31"),
            registered_trading_announce
        )
        .unwrap(),
        2
    );
    let registered_after_weekend = preference_registered_on(CalendarExchange::Sse, d("2030-02-02"));
    assert_eq!(
        exposure_trading_days(
            trading_calendar(),
            CalendarExchange::Sse,
            d("2030-02-01"),
            registered_after_weekend
        )
        .unwrap(),
        1
    );
    // 线性下界：floor(1120 × (1 − 2×10%)) = 896；n=1 时 floor(1120×0.9) = 1008。
    assert_eq!(limit_down_margin_floor_cents(1_120, 1_000, 2), Some(896));
    assert_eq!(limit_down_margin_floor_cents(1_120, 1_000, 1), Some(1_008));
    // 余量耗尽（n × limit ≥ 10000）返回 None，调用方如实拒绝。
    assert_eq!(limit_down_margin_floor_cents(1_120, 5_000, 2), None);
    // 合并组分子下界：ratio=0 → 1；0 < ratio ≤ 1e6 → 2；2e6 < ratio ≤ 3e6 → 4。
    assert_eq!(combined_group_required_positive_cents(0), 1);
    assert_eq!(combined_group_required_positive_cents(1), 2);
    assert_eq!(combined_group_required_positive_cents(1_000_000), 2);
    assert_eq!(combined_group_required_positive_cents(2_000_001), 4);
    // 组合出的极端敞口（n=2、50% 涨跌幅）下余量耗尽的评估拒绝。
    let preference = cash_preference(10_000, 1, 1);
    let mut input = cash_input(
        &preference,
        profit(10_000_000),
        1,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    input.exposure = Some(SimpleCashDividendExposure {
        last_close: Money::from_cents(1_120),
        limit_bps: 5_000,
        same_ex_date_gross_per_share: Money::ZERO,
        same_ex_date_ratio_micros: None,
    });
    match evaluate_cash_dividend_preference(&input) {
        SimpleCashDividendOutcome::Rejected { detail } => {
            assert!(
                detail.contains("余量耗尽"),
                "拒绝原因应如实说明敞口：{detail}"
            );
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }
}

#[test]
fn cash_evaluation_skips_below_threshold_or_duplicate_or_frequency() {
    let preference = cash_preference(3_000, 10_000, 1);
    // 门槛 10000 分 > 可分配 9_999 分：触发条件不满足，静默不提案。
    let input = cash_input(
        &preference,
        profit(9_999),
        300,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    assert!(matches!(
        evaluate_cash_dividend_preference(&input),
        SimpleCashDividendOutcome::Skip
    ));
    // 同周期已提案：幂等跳过。
    let input = cash_input(
        &preference,
        profit(10_000),
        300,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        true,
    );
    assert!(matches!(
        evaluate_cash_dividend_preference(&input),
        SimpleCashDividendOutcome::Skip
    ));
    // 频率：上一提案即本周期末日，间隔不足一个完整周期。
    let input = cash_input(
        &preference,
        profit(10_000),
        300,
        Some(AccountingAmount::from_cents(1_000_000)),
        Some(d("2030-01-31")),
        false,
    );
    assert!(matches!(
        evaluate_cash_dividend_preference(&input),
        SimpleCashDividendOutcome::Skip
    ));
}

#[test]
fn cash_evaluation_records_rejection_when_plan_cannot_be_built() {
    // 目标派息摊到每股不足一分：无法构造整数分方案，如实记录拒绝。
    let preference = cash_preference(1, 1, 1);
    let input = cash_input(
        &preference,
        profit(100),
        300,
        Some(AccountingAmount::from_cents(1_000_000)),
        None,
        false,
    );
    match evaluate_cash_dividend_preference(&input) {
        SimpleCashDividendOutcome::Rejected { detail } => {
            assert!(
                detail.contains("每股不足一分"),
                "拒绝原因应如实说明：{detail}"
            );
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }
    // 缺注册资本法定事实：无法构造分红决议，如实记录拒绝。
    let preference = cash_preference(3_000, 1, 1);
    let input = cash_input(&preference, profit(10_000), 300, None, None, false);
    match evaluate_cash_dividend_preference(&input) {
        SimpleCashDividendOutcome::Rejected { detail } => {
            assert!(
                detail.contains("注册资本法定事实"),
                "拒绝原因应如实说明：{detail}"
            );
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }
}

#[test]
fn proposal_due_counts_full_settlement_cycles() {
    assert!(proposal_due(None, d("2030-03-31"), 1, 1));
    // 月度周期：上月末提案后，本月末已隔一个完整周期。
    assert!(proposal_due(Some(d("2030-01-31")), d("2030-02-28"), 1, 1));
    assert!(!proposal_due(Some(d("2030-01-31")), d("2030-02-28"), 2, 1));
    // 季度周期（3 个自然月）：2029Q4 末日提案，2030Q1 末日才满一个周期。
    assert!(!proposal_due(Some(d("2029-12-31")), d("2030-02-28"), 1, 3));
    assert!(proposal_due(Some(d("2029-12-31")), d("2030-03-31"), 1, 3));
}

fn stock_exposure() -> SimpleStockDistributionExposure {
    SimpleStockDistributionExposure {
        last_close: Money::from_cents(10_000),
        limit_bps: 1_000,
        same_ex_date_gross_per_share: Money::ZERO,
        same_ex_date_stock_event: false,
    }
}

#[test]
fn stock_evaluation_sizes_by_ratio_and_honors_expansion_cap() {
    let preference = stock_preference(1, 100_000, 1_000_000, 1);
    let input = SimpleStockDistributionEvaluation {
        preference: &preference,
        company: fixture_ids().0,
        stock: fixture_ids().1,
        exchange: CalendarExchange::Sse,
        period_end: d("2030-01-31"),
        approve_on: d("2030-02-01"),
        calendar: trading_calendar(),
        distributable: profit(1_000_000_00),
        eligible_shares: 10_000,
        initial_issued_shares: 12_000,
        cumulative_distributed_shares: 0,
        exposure: Some(stock_exposure()),
        last_proposal_period_end: None,
        cycle_months: 1,
        suppress_duplicate: false,
    };
    // 10 送 1：10_000 × 100_000 / 1_000_000 = 1_000 股；累计扩张 1000/12000
    // ≈ 8.33% < 100% 上限，应成案。
    let SimpleStockDistributionOutcome::Proposal { plan } =
        evaluate_stock_distribution_preference(&input)
    else {
        panic!("应构造出送转提案");
    };
    assert_eq!(plan.approved_total_new_shares, 1_000);
    assert_eq!(plan.shares_per_existing_share_micros, 100_000);
    assert_eq!(
        plan.event_id,
        "simple-preference:co-a:stock-distribution:2030-01-31"
    );
    assert_eq!(
        plan.kind,
        crate::company::stock_distribution::StockDistributionKind::BonusShares
    );
    let expected_registered = preference_registered_on(CalendarExchange::Sse, d("2030-02-01"));
    assert_eq!(plan.approved_on, d("2030-02-01"));
    assert_eq!(plan.announced_on, d("2030-02-01"));
    assert_eq!(plan.registered_on, expected_registered);
    assert_eq!(
        plan.ex_rights_on,
        trading_calendar()
            .next_trading_day(CalendarExchange::Sse, expected_registered)
            .unwrap()
    );
    assert!(!plan.approval_reference.is_empty());

    // 累计扩张上限触发：已有 11_900 股送转 + 本案 1_000 股 > 12000×100%，
    // 触发条件不满足，静默不提案。
    let input = SimpleStockDistributionEvaluation {
        cumulative_distributed_shares: 11_900,
        ..input
    };
    assert!(matches!(
        evaluate_stock_distribution_preference(&input),
        SimpleStockDistributionOutcome::Skip
    ));
}

#[test]
fn stock_evaluation_records_rejection_when_ratio_yields_no_share() {
    let preference = stock_preference(1, 1, 1_000_000_000, 1);
    let input = SimpleStockDistributionEvaluation {
        preference: &preference,
        company: fixture_ids().0,
        stock: fixture_ids().1,
        exchange: CalendarExchange::Sse,
        period_end: d("2030-01-31"),
        approve_on: d("2030-02-01"),
        calendar: trading_calendar(),
        distributable: profit(1_000_000),
        eligible_shares: 500_000,
        initial_issued_shares: 600_000,
        cumulative_distributed_shares: 0,
        exposure: Some(stock_exposure()),
        last_proposal_period_end: None,
        cycle_months: 1,
        suppress_duplicate: false,
    };
    // 500_000 股 × 1 微分股 = 0.5 股 → 不足一股，无法构造方案，如实记录拒绝。
    match evaluate_stock_distribution_preference(&input) {
        SimpleStockDistributionOutcome::Rejected { detail } => {
            assert!(detail.contains("不足一股"), "拒绝原因应如实说明：{detail}");
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }
}

#[test]
fn stock_evaluation_rejects_same_ex_date_event_and_insufficient_combined_margin() {
    // 同除权日已存在送转事件（显式或自动）：合并除权口径未核实，叠加会在
    // 除权日日结显式失败，自动提案必须拒绝。
    let preference = stock_preference(1, 100_000, 1_000_000_000, 1);
    let mut input = SimpleStockDistributionEvaluation {
        preference: &preference,
        company: fixture_ids().0,
        stock: fixture_ids().1,
        exchange: CalendarExchange::Sse,
        period_end: d("2030-01-31"),
        approve_on: d("2030-02-01"),
        calendar: trading_calendar(),
        distributable: profit(1_000_000_00),
        eligible_shares: 10_000,
        initial_issued_shares: 12_000,
        cumulative_distributed_shares: 0,
        exposure: Some(SimpleStockDistributionExposure {
            same_ex_date_stock_event: true,
            ..stock_exposure()
        }),
        last_proposal_period_end: None,
        cycle_months: 1,
        suppress_duplicate: false,
    };
    match evaluate_stock_distribution_preference(&input) {
        SimpleStockDistributionOutcome::Rejected { detail } => {
            assert!(
                detail.contains("同除权日"),
                "拒绝原因应指向同除权日送转叠加：{detail}"
            );
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }
    // 同除权日现金红利吃掉大部分余量：10 送 1（ratio=1e5）需保留
    // 1 + ceil(1e5/1e6) = 2 分分子，而 floor(10000×(1−2×10%)) − 9999
    // = 8000 − 9999 < 0 < 2 分 → 拒绝。
    input.exposure = Some(SimpleStockDistributionExposure {
        same_ex_date_gross_per_share: Money::from_cents(9_999),
        ..stock_exposure()
    });
    match evaluate_stock_distribution_preference(&input) {
        SimpleStockDistributionOutcome::Rejected { detail } => {
            assert!(
                detail.contains("安全余量不足"),
                "拒绝原因应指向合并除权余量：{detail}"
            );
        }
        other => panic!("应记录拒绝，实际 {other:?}"),
    }
    // 缺行情锚：无法推导安全余量，如实拒绝。
    input.exposure = None;
    assert!(matches!(
        evaluate_stock_distribution_preference(&input),
        SimpleStockDistributionOutcome::Rejected { .. }
    ));
}

#[test]
fn ledger_records_rejections_idempotently_and_rejects_drift() {
    let company = CompanyId("co-a".into());
    let mut ledger = super::preferences::SimplePreferenceLedger::default();
    ledger
        .record_rejection(
            &company,
            SimplePreferenceProposalKind::CashDividend,
            d("2030-01-31"),
            "决议总额超过已核定的 Simple 可分配利润".into(),
        )
        .unwrap();
    // 同键同因：幂等，不重复记录。
    ledger
        .record_rejection(
            &company,
            SimplePreferenceProposalKind::CashDividend,
            d("2030-01-31"),
            "决议总额超过已核定的 Simple 可分配利润".into(),
        )
        .unwrap();
    assert_eq!(ledger.rejections.len(), 1);
    assert_eq!(
        ledger.last_rejection_on(&company, SimplePreferenceProposalKind::CashDividend),
        Some(d("2030-01-31"))
    );
    assert_eq!(
        ledger.last_rejection_on(&company, SimplePreferenceProposalKind::StockDistribution),
        None
    );
    // 同键异因：状态漂移，显式报错（铁律 2）。
    assert!(ledger
        .record_rejection(
            &company,
            SimplePreferenceProposalKind::CashDividend,
            d("2030-01-31"),
            "不同原因".into(),
        )
        .is_err());
    let known = std::collections::BTreeSet::from([company.clone()]);
    assert!(ledger.validate(&known, d("2030-01-31")).is_ok());
    assert!(
        ledger.validate(&known, d("2030-01-30")).is_err(),
        "未来日期必须拒绝"
    );
    let unknown_company = std::collections::BTreeSet::new();
    assert!(ledger.validate(&unknown_company, d("2030-01-31")).is_err());
}

#[test]
fn preference_evaluation_of_500_companies_stays_lightweight() {
    // 性能代表性用例：500 公司 × 2 偏好项的单周期评估为轻量整数比较；
    // 断言宽松墙钟上限（debug 构建下实际远低于该值），防退化为逐 tick 或
    // 全量克隆级开销。既有日结用例不被显著抬高的证据见 Session 侧组测。
    let company_count = 500;
    let companies: Vec<CompanyId> = (0..company_count)
        .map(|index| CompanyId(format!("co-{index}")))
        .collect();
    let cash_preference = cash_preference(3_000, 100, 1);
    let stock_preference = stock_preference(1_000_000, 50_000, 500_000, 1);
    let capital = AccountingAmount::from_cents(1_000_000);
    let start = std::time::Instant::now();
    let mut proposals = 0_usize;
    for company in &companies {
        let stock = StockCode("600101".into());
        let input = SimpleCashDividendEvaluation {
            preference: &cash_preference,
            company,
            stock: &stock,
            exchange: CalendarExchange::Sse,
            period_end: d("2030-01-31"),
            approve_on: d("2030-02-01"),
            calendar: trading_calendar(),
            distributable: profit(1_000_000_00),
            eligible_shares: 1_000_000,
            exposure: Some(cash_exposure(10_000)),
            registered_capital: Some(capital),
            last_proposal_period_end: None,
            cycle_months: 1,
            suppress_duplicate: false,
        };
        if matches!(
            evaluate_cash_dividend_preference(&input),
            SimpleCashDividendOutcome::Proposal { .. }
        ) {
            proposals += 1;
        }
        let input = SimpleStockDistributionEvaluation {
            preference: &stock_preference,
            company,
            stock: &stock,
            exchange: CalendarExchange::Sse,
            period_end: d("2030-01-31"),
            approve_on: d("2030-02-01"),
            calendar: trading_calendar(),
            distributable: profit(1_000_000_00),
            eligible_shares: 1_000_000,
            initial_issued_shares: 1_200_000,
            cumulative_distributed_shares: 0,
            exposure: Some(stock_exposure()),
            last_proposal_period_end: None,
            cycle_months: 1,
            suppress_duplicate: false,
        };
        if matches!(
            evaluate_stock_distribution_preference(&input),
            SimpleStockDistributionOutcome::Proposal { .. }
        ) {
            proposals += 1;
        }
    }
    let elapsed = start.elapsed();
    assert_eq!(proposals, company_count * 2, "全部偏好项都应成案");
    assert!(
        elapsed < std::time::Duration::from_millis(1_000),
        "500 公司偏好评估耗时 {elapsed:?} 超出代表性上限"
    );
}
