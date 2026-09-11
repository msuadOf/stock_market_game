//! 方法/抽取层的类型化不可用：g≥r、零/负分母、亏损与负权益（绝不补正值）、
//! 不支持报告种类/缺历史、未来信息、溢出、筹资口径不可拆分、退化收入基数。

use crate::hand::{HandReportSpec, hand_report};
use crate::{COMPANY, d};
use engine::accounting::AccountingAmount;
use engine::calendar::CivilInstant;
use engine::company::CompanyId;
use engine::information::PublicationId;
use engine::strategy::{
    AnnualFacts, ForecastBasis, FundamentalMethod, GrowthObservation, PersonalAssumptions,
    PriorRevenue, ValuationOutcome, ValuationUnavailable, cash_flow, earnings_multiple, equity_roe,
    estimate_by_method, extract_annual_facts, initial_forecast, observe_growth,
};

pub(super) fn company() -> CompanyId {
    CompanyId(COMPANY.to_string())
}

pub(super) fn as_after_publication() -> CivilInstant {
    CivilInstant::from_hms(d("2031-03-21"), 12, 0, 0).expect("as_of")
}

pub(super) fn neutral_assumptions() -> PersonalAssumptions {
    PersonalAssumptions {
        growth_deviation_bp: 0,
        quality_coefficient_bp: 10_000,
        pe_multiple: 10,
        equity_cost_bp: 1_000,
        terminal_growth_bp: 0,
        roe_deviation_bp: 0,
    }
}

/// FY2030 等价事实（分）：FCFE 17,820,000、归母权益 60,220,000、
/// 期初归母权益 42,400,000、归母净利 17,820,000。
pub(super) fn fy2030_facts() -> AnnualFacts {
    extract_annual_facts(
        &hand_report(HandReportSpec {
            equity_to_parent_cents: 60_220_000,
            total_equity_cents: 60_220_000,
            opening_parent_cents: 42_400_000,
            ni_total_cents: 17_820_000,
            revenue_cents: 19_800_000,
            prior_revenue_cents: Some(18_000_000),
            operating_cf_cents: 17_820_000,
            ..HandReportSpec::default()
        }),
        &company(),
        as_after_publication(),
    )
    .expect("facts extract")
}

/// g ≥ r（终值增长不低于权益资本成本）⇒ 该法显式不可用。
#[test]
fn terminal_growth_not_below_cost_is_unavailable() {
    let mut assumptions = neutral_assumptions();
    assumptions.terminal_growth_bp = 1_200;
    assert_eq!(
        cash_flow(&fy2030_facts(), &assumptions, 0),
        Err(ValuationUnavailable::TerminalGrowthNotBelowCost {
            terminal_bp: 1_200,
            cost_bp: 1_000,
        })
    );
    // 相等也拒绝（必须严格小于）。
    assumptions.terminal_growth_bp = 1_000;
    assert_eq!(
        cash_flow(&fy2030_facts(), &assumptions, 0),
        Err(ValuationUnavailable::TerminalGrowthNotBelowCost {
            terminal_bp: 1_000,
            cost_bp: 1_000,
        })
    );
}

/// 零/负分母：PE ≤ 0、资本成本 ≤ 0、总股本 = 0 ⇒ 类型化不可用。
#[test]
fn zero_and_negative_denominators_are_unavailable() {
    let facts = fy2030_facts();
    let mut assumptions = neutral_assumptions();
    assumptions.pe_multiple = 0;
    assert_eq!(
        earnings_multiple(&facts, &assumptions),
        Err(ValuationUnavailable::NonPositivePe { pe: 0 })
    );
    assumptions = neutral_assumptions();
    assumptions.equity_cost_bp = 0;
    assert_eq!(
        equity_roe(&facts, &assumptions),
        Err(ValuationUnavailable::NonPositiveCost { cost_bp: 0 })
    );
    // 总股本 0：估值除法分母（发行人/股数联动校验）。
    assert_eq!(
        estimate_by_method(
            FundamentalMethod::EarningsMultiple,
            &facts,
            &neutral_assumptions(),
            Some(0),
            0,
        ),
        ValuationOutcome::Unavailable {
            reason: ValuationUnavailable::ZeroIssuedShares
        }
    );
}

/// 亏损与负权益绝不产生正值 fallback（不取绝对值、不补正）。
#[test]
fn losses_and_negative_equity_never_fall_back_positive() {
    let assumptions = neutral_assumptions();

    let mut lossy = fy2030_facts();
    lossy.net_income_to_parent = AccountingAmount::from_cents(-1);
    assert_eq!(
        earnings_multiple(&lossy, &assumptions),
        Err(ValuationUnavailable::NonPositiveNetIncome)
    );

    let mut negative_equity = fy2030_facts();
    negative_equity.equity_to_parent = AccountingAmount::from_cents(-1);
    assert_eq!(
        equity_roe(&negative_equity, &assumptions),
        Err(ValuationUnavailable::NonPositiveBookEquity)
    );

    // 平均权益非正：观察 ROE 分母（期初 −1.7 亿 + 期末 8 千万 ⇒ 平均 −4.5 千万）。
    let mut negative_average = fy2030_facts();
    negative_average.opening_equity_to_parent = AccountingAmount::from_cents(-170_000_000);
    negative_average.equity_to_parent = AccountingAmount::from_cents(80_000_000);
    assert_eq!(
        equity_roe(&negative_average, &assumptions),
        Err(ValuationUnavailable::NonPositiveAverageEquity)
    );

    // 预期 ROE 非正（亏损 ⇒ 观察 ROE −1250bp，无偏差）⇒ 高风险显式不可用。
    let mut loss_roe = fy2030_facts();
    loss_roe.equity_to_parent = AccountingAmount::from_cents(80_000_000);
    loss_roe.opening_equity_to_parent = AccountingAmount::from_cents(70_000_000);
    loss_roe.net_income_to_parent = AccountingAmount::from_cents(-9_375_000);
    assert_eq!(
        equity_roe(&loss_roe, &assumptions),
        Err(ValuationUnavailable::NonPositiveExpectedRoe {
            expected_bp: -1_250
        })
    );

    // 负 FCFE 折现出负估值 ⇒ 显式不可用（不补正值）。
    let mut burning = fy2030_facts();
    burning.operating_cf = AccountingAmount::from_cents(-100_000);
    burning.net_income_to_parent = AccountingAmount::from_cents(17_820_000);
    assert_eq!(
        cash_flow(&burning, &assumptions, 0),
        Err(ValuationUnavailable::NonPositiveValuationEstimate)
    );
}

/// 不支持报告种类（估值事实只吃年报）与公司归属错配 ⇒ 类型化拒绝。
#[test]
fn unsupported_report_kind_and_company_mismatch() {
    let quarterly = hand_report(HandReportSpec {
        kind: engine::accounting::reports::ReportKind::Quarter,
        ..HandReportSpec::default()
    });
    assert_eq!(
        extract_annual_facts(&quarterly, &company(), as_after_publication()),
        Err(ValuationUnavailable::UnsupportedReportKind {
            kind: engine::accounting::reports::ReportKind::Quarter,
        })
    );

    let annual = hand_report(HandReportSpec::default());
    let other = CompanyId("C-OTHER".to_string());
    assert_eq!(
        extract_annual_facts(&annual, &other, as_after_publication()),
        Err(ValuationUnavailable::CompanyMismatch {
            expected: other.clone(),
            report: company(),
        })
    );
}

/// 未来信息：as_of 早于公布时点 ⇒ 类型化拒绝（即使直接传报表也不许前视）。
#[test]
fn future_dated_material_is_rejected() {
    let annual = hand_report(HandReportSpec::default());
    let early = CivilInstant::from_hms(d("2031-03-20"), 17, 0, 0).expect("early as_of");
    assert_eq!(
        extract_annual_facts(&annual, &company(), early),
        Err(ValuationUnavailable::FutureDatedMaterial {
            report: PublicationId::new(901),
        })
    );
}

/// 缺历史与退化基数：比较项缺 ⇒ PriorWithoutHistory（先验 0 + 偏差、信心 3000）；
/// 上年收入为 0 ⇒ 增长先验退化（现金流法不可用，盈利倍数法不受影响）。
#[test]
fn missing_history_vs_degenerate_revenue_base() {
    let no_history = extract_annual_facts(
        &hand_report(HandReportSpec {
            prior_revenue_cents: None,
            revenue_cents: 19_800_000,
            ni_total_cents: 17_820_000,
            ..HandReportSpec::default()
        }),
        &company(),
        as_after_publication(),
    )
    .expect("extract");
    assert_eq!(no_history.prior_revenue, PriorRevenue::NoHistory);
    assert_eq!(
        observe_growth(&no_history),
        GrowthObservation::WithoutHistory
    );
    let forecast = initial_forecast(GrowthObservation::WithoutHistory, -1_000);
    assert_eq!(
        forecast.growth_bp,
        Some(-1_000),
        "0 prior + personal deviation"
    );
    assert_eq!(forecast.basis, ForecastBasis::InitialWithoutHistory);
    assert_eq!(forecast.initial_confidence_bp(), 3_000);
    let with_history = initial_forecast(GrowthObservation::TwoYear(1_000), -1_000);
    assert_eq!(with_history.initial_confidence_bp(), 6_000);

    // 退化基数：上年收入为 0（零收入分母）⇒ 增长观察显式退化。
    let degenerate = extract_annual_facts(
        &hand_report(HandReportSpec {
            prior_revenue_cents: Some(0),
            revenue_cents: 19_800_000,
            ni_total_cents: 17_820_000,
            ..HandReportSpec::default()
        }),
        &company(),
        as_after_publication(),
    )
    .expect("extract");
    assert_eq!(observe_growth(&degenerate), GrowthObservation::Degenerate);
    assert_eq!(
        initial_forecast(GrowthObservation::Degenerate, 500).growth_bp,
        None
    );
    let assumptions = neutral_assumptions();
    assert_eq!(
        estimate_by_method(
            FundamentalMethod::CashFlow,
            &degenerate,
            &assumptions,
            None,
            100_000
        ),
        ValuationOutcome::Unavailable {
            reason: ValuationUnavailable::GrowthPriorUnavailable
        }
    );
    assert!(matches!(
        estimate_by_method(
            FundamentalMethod::EarningsMultiple,
            &degenerate,
            &assumptions,
            None,
            100_000
        ),
        ValuationOutcome::Available { .. }
    ));
}
