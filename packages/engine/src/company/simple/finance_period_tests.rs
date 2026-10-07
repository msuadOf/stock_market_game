use super::tests::{amount, date, fixture};
use super::*;
use crate::accounting::{IncomeTaxPolicy, TaxPolicy, VatPolicy};

#[test]
fn actual_quarter_summary_is_not_a_fabricated_march_monthly_report() {
    let mut state = fixture();
    state
        .apply_period(
            date(1, 1),
            date(3, 31),
            &PeriodAmounts {
                revenue: amount(30_000),
                fixed_expense: amount(9_000),
                variable_expense: amount(12_000),
            },
        )
        .unwrap();
    let period = AccountingPeriod::from_ymd(2030, 3).unwrap();
    assert!(state.report(period, ReportKind::Monthly).is_none());
    let report = state.report(period, ReportKind::Quarter).unwrap();
    report.validate().unwrap();
    assert_eq!(report.income.cumulative.net_income, amount(6_750));
    assert!(
        state
            .apply_period(
                date(2, 1),
                date(6, 30),
                &PeriodAmounts {
                    revenue: amount(1),
                    fixed_expense: amount(0),
                    variable_expense: amount(0)
                }
            )
            .is_err()
    );
}

#[test]
fn report_period_income_and_roe_use_each_report_window_not_cumulative_roe() {
    let mut state = fixture();
    state.config.tax_policy.income_tax.rate_bp = 0;
    for month in 1..=12 {
        let next_month = if month == 12 {
            CivilDate::from_ymd(2031, 1, 1).unwrap()
        } else {
            date(month + 1, 1)
        };
        state
            .apply_month(
                next_month.prev().unwrap(),
                &PeriodAmounts {
                    revenue: amount(i128::from(month) * 1_000),
                    fixed_expense: amount(0),
                    variable_expense: amount(0),
                },
            )
            .unwrap();
    }

    let february = state
        .report(
            AccountingPeriod::from_ymd(2030, 2).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    assert_eq!(february.income.report_period.net_income, amount(2_000));
    assert_eq!(
        february.roe.ordinary_roe,
        crate::accounting::reports::ReportRoeValue::Available(
            crate::accounting::reports::ReportRational {
                numerator: "4000".into(),
                denominator: "24000".into(),
            }
        )
    );

    let quarter = state
        .report(
            AccountingPeriod::from_ymd(2030, 3).unwrap(),
            ReportKind::Quarter,
        )
        .unwrap();
    assert_eq!(quarter.income.report_period.net_income, amount(6_000));
    assert_eq!(quarter.income.quarter.net_income, amount(6_000));
    assert_eq!(
        quarter.roe.ordinary_roe,
        crate::accounting::reports::ReportRoeValue::Available(
            crate::accounting::reports::ReportRational {
                numerator: "36000".into(),
                denominator: "78000".into(),
            }
        )
    );

    let annual = state
        .report(
            AccountingPeriod::from_ymd(2030, 12).unwrap(),
            ReportKind::Annual,
        )
        .unwrap();
    assert_eq!(annual.income.report_period.net_income, amount(78_000));
    assert_eq!(annual.income.cumulative.net_income, amount(78_000));
    assert_eq!(
        annual.roe.ordinary_roe,
        crate::accounting::reports::ReportRoeValue::Available(
            crate::accounting::reports::ReportRational {
                numerator: "1872000".into(),
                denominator: "1176000".into(),
            }
        )
    );
    assert_eq!(
        annual.roe.adjusted_roe,
        crate::accounting::reports::ReportRoeValue::Unavailable {
            reason: crate::accounting::reports::ReportRoeUnavailable::MissingNonRecurringIncomeFacts,
        }
    );
}

#[test]
fn report_validation_rejects_roe_drift_and_fabricated_adjusted_roe() {
    let mut state = fixture();
    state.config.tax_policy.income_tax.rate_bp = 0;
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(1_000),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    let period = AccountingPeriod::from_ymd(2030, 1).unwrap();
    let mut incorrect_rate = state.report(period, ReportKind::Monthly).unwrap().clone();
    let crate::accounting::reports::ReportRoeValue::Available(rate) =
        &mut incorrect_rate.roe.ordinary_roe
    else {
        panic!("ordinary report ROE must be available")
    };
    rate.numerator = "999".into();
    assert!(matches!(
        incorrect_rate.validate(),
        Err(crate::accounting::reports::ReportError::InvalidRoe { .. })
    ));

    let mut fabricated_adjusted = state.report(period, ReportKind::Monthly).unwrap().clone();
    fabricated_adjusted.roe.adjusted_roe =
        crate::accounting::reports::ReportRoeValue::Available(
            crate::accounting::reports::ReportRational {
                numerator: "0".into(),
                denominator: "1".into(),
            },
        );
    assert!(matches!(
        fabricated_adjusted.validate(),
        Err(crate::accounting::reports::ReportError::InvalidRoe { .. })
    ));
}

#[test]
fn report_roe_marks_nonpositive_average_equity_unavailable() {
    let mut state = SimpleFinanceState::create(
        CompanyId("C-NEGATIVE-EQUITY".into()),
        CompanyKind::Industrial,
        &SimpleFinanceConfig {
            opening_lines: vec![
                line("1002", PostingSide::Debit, amount(10_000)),
                line("4103", PostingSide::Debit, amount(20_000)),
                line("2202", PostingSide::Credit, amount(30_000)),
            ],
            tax_policy: TaxPolicy {
                version: 1,
                vat: VatPolicy {
                    output_rate_bp: 0,
                    input_rate_bp: 0,
                    deductible_share_bp: 10_000,
                },
                income_tax: IncomeTaxPolicy {
                    rate_bp: 0,
                    loss_carryforward_years: 5,
                },
            },
            summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
            book_display: SimpleBookDisplayConfig::DEFAULT,
        },
        date(1, 1).prev().unwrap(),
    )
    .unwrap();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(1_000),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();

    let report = state
        .report(
            AccountingPeriod::from_ymd(2030, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    assert_eq!(
        report.roe.ordinary_roe,
        crate::accounting::reports::ReportRoeValue::Unavailable {
            reason: crate::accounting::reports::ReportRoeUnavailable::NonPositiveAverageEquity,
        }
    );
}

#[test]
fn zero_summary_still_closes_period_without_zero_amount_voucher() {
    let mut state = fixture();
    let count = state.books().journal().entry_count();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(0),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    assert_eq!(state.books().journal().entry_count(), count);
    state
        .report(
            AccountingPeriod::from_ymd(2030, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap()
        .validate()
        .unwrap();
}

#[test]
fn future_tax_position_or_reused_cursor_is_rejected_after_restore() {
    let mut state = fixture();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(100),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    let mut value = serde_json::to_value(&state).unwrap();
    value["next_event_id"] = serde_json::Value::String("1".into());
    let corrupt: SimpleFinanceState = serde_json::from_value(value).unwrap();
    assert!(corrupt.validate().is_err());
    let mut value = serde_json::to_value(&state).unwrap();
    let assessment = value["income_tax_position"]["assessments"]
        .as_object_mut()
        .unwrap()
        .remove("2030")
        .unwrap();
    value["income_tax_position"]["assessments"]["2031"] = assessment;
    let corrupt: SimpleFinanceState = serde_json::from_value(value).unwrap();
    assert!(corrupt.validate().is_err());
}

fn settled_month(revenue: i128) -> SimpleFinanceState {
    let mut state = fixture();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(revenue),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    state
}

#[test]
fn restored_valid_report_from_different_books_is_rejected() {
    let state = settled_month(100);
    let other = settled_month(200);
    other
        .report(AccountingPeriod::of_date(date(1, 31)), ReportKind::Monthly)
        .unwrap()
        .validate()
        .unwrap();
    let mut saved = serde_json::to_value(state).unwrap();
    saved["closing"] = serde_json::to_value(other.closing()).unwrap();
    let corrupt: SimpleFinanceState = serde_json::from_value(saved).unwrap();
    assert!(corrupt.validate().is_err());
}

#[test]
fn restored_foreign_scope_report_is_rejected() {
    let state = settled_month(100);
    let mut other = settled_month(100);
    other.company = CompanyId("foreign-company".into());
    let mut saved = serde_json::to_value(state).unwrap();
    let mut closing = serde_json::to_value(other.closing()).unwrap();
    let foreign = serde_json::to_value(ScopeId::Standalone(other.member_id())).unwrap();
    closing["versions"][0][0] = foreign.clone();
    closing["versions"][0][3][0]["scope"] = foreign;
    saved["closing"] = closing;
    let corrupt: SimpleFinanceState = serde_json::from_value(saved).unwrap();
    assert!(corrupt.validate().is_err());
}

#[test]
fn restored_report_outside_recognized_coverage_is_rejected() {
    let state = fixture();
    let other = settled_month(100);
    let mut saved = serde_json::to_value(state).unwrap();
    saved["closing"] = serde_json::to_value(other.closing()).unwrap();
    let corrupt: SimpleFinanceState = serde_json::from_value(saved).unwrap();
    assert!(corrupt.validate().is_err());
}

#[test]
fn restored_missing_generated_report_is_rejected() {
    let state = settled_month(100);
    let mut saved = serde_json::to_value(state).unwrap();
    saved["closing"] = serde_json::to_value(fixture().closing()).unwrap();
    let corrupt: SimpleFinanceState = serde_json::from_value(saved).unwrap();
    assert!(corrupt.validate().is_err());
}
