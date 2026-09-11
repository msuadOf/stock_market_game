//! 失败路径（方法层）：溢出/每股值域与筹资口径拆分。

use super::guards::{as_after_publication, company, fy2030_facts, neutral_assumptions};
use crate::hand::{HandReportSpec, hand_report};
use engine::accounting::AccountingAmount;
use engine::strategy::{
    FundamentalMethod, ValuationOutcome, ValuationUnavailable, cash_flow, earnings_multiple,
    estimate_by_method, extract_annual_facts,
};

/// 溢出与每股值域：类型化，绝不截断。
#[test]
fn overflow_and_per_share_range_are_typed() {
    let mut huge = fy2030_facts();
    // 10^36 分 × 质量系数 10^4 ⇒ 10^40 越出 i128。
    huge.net_income_to_parent =
        AccountingAmount::from_cents(1_000_000_000_000_000_000_000_000_000_000_000_000);
    assert_eq!(
        earnings_multiple(&huge, &neutral_assumptions()),
        Err(ValuationUnavailable::Overflow {
            step: "sustainable earnings x quality coefficient".into(),
        })
    );

    let mut big = fy2030_facts();
    // 10^30 分 ⇒ 估计 10^31 分；除以 1 股 ⇒ 每股 10^31 分越出 Money i64 值域。
    big.net_income_to_parent =
        AccountingAmount::from_cents(1_000_000_000_000_000_000_000_000_000_000_000);
    assert_eq!(
        estimate_by_method(
            FundamentalMethod::EarningsMultiple,
            &big,
            &neutral_assumptions(),
            Some(0),
            1,
        ),
        ValuationOutcome::Unavailable {
            reason: ValuationUnavailable::PerShareOutOfRange
        }
    );
}

/// 筹资口径拆分：利息支付 = (新借−还本) − 筹资CF 不可为负 ⇒ 负 ⇒ Undeterminable；
/// 正常利息 ⇒ FCFE = 经营CF − capex + 净新借 − 利息（手算 10,216,933 分）。
#[test]
fn financing_split_undeterminable_and_valid_interest_path() {
    let undeterminable = extract_annual_facts(
        &hand_report(HandReportSpec {
            operating_cf_cents: 1_000_000,
            financing_cf_cents: 30_000,
            ni_total_cents: 100_000,
            ..HandReportSpec::default()
        }),
        &company(),
        as_after_publication(),
    )
    .expect("extract");
    assert_eq!(
        cash_flow(&undeterminable, &neutral_assumptions(), 0),
        Err(ValuationUnavailable::FinancingSplitUndeterminable)
    );

    let with_interest = extract_annual_facts(
        &hand_report(HandReportSpec {
            operating_cf_cents: 1_000_000,
            financing_cf_cents: -50_000,
            ni_total_cents: 100_000,
            ..HandReportSpec::default()
        }),
        &company(),
        as_after_publication(),
    )
    .expect("extract");
    let estimates = cash_flow(&with_interest, &neutral_assumptions(), 0).expect("available");
    assert_eq!(estimates.central, 10_216_933);
}
