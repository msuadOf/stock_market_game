use super::tests::{amount, date, fixture};
use super::*;

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
