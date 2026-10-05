use super::fixtures::*;
use engine::company::operations::{CompanyOperations, CompanyOperationsConfig};

fn operations(date: engine::calendar::CivilDate, broke: bool) -> CompanyOperations {
    let as_of = engine::calendar::CivilDate::from_ymd(date.year() - 3, 12, 31).unwrap();
    let company = if broke { industrial_broke(as_of) } else { industrial_a(as_of) };
    CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap()
}

#[test]
fn operations_failure_history_is_saved_restored_and_queryable() {
    let date = d("2030-01-05");
    let mut operations = operations(date, true);
    operations.advance_civil_day(date).unwrap();
    let failures = operations.payment_failures_on(date).to_vec();
    assert!(!failures.is_empty());
    let restored: CompanyOperations =
        serde_json::from_value(serde_json::to_value(&operations).unwrap()).unwrap();
    assert_eq!(restored.payment_failures_on(date), failures);
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&operations).unwrap()
    );
}

#[test]
fn default_trading_calendar_rejects_dates_outside_runtime_window() {
    let calendar = engine::calendar::TradingCalendar::current_default_calendar().unwrap();
    for invalid in ["1997-12-31", "2100-01-01"] {
        assert!(calendar.validate_runtime_start(d(invalid)).is_err());
    }
}

#[test]
fn repeated_operations_day_rejects_without_mutating_the_completed_day() {
    let date = d("2030-01-05");
    let mut operations = operations(date, false);
    operations.advance_civil_day(date).unwrap();
    let before = serde_json::to_value(&operations).unwrap();
    assert!(operations.advance_civil_day(date).is_err());
    assert_eq!(serde_json::to_value(&operations).unwrap(), before);
}
