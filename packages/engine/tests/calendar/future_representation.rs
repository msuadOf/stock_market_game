use crate::d;
use engine::calendar::{
    CalendarExchange, CalendarPolicy, ClosedReason, DayStatus, HolidayKind, LunarYearFact,
    LunarYearFacts, SimulatedFallbackRuleset, TradingCalendar, YearCoverageLabel,
};

#[test]
fn explicit_test_calendar_can_represent_and_restore_2100_without_official_claims() {
    let base = CalendarPolicy::current_default_policy().unwrap();
    let mut spec = base.spec();
    let mut facts = spec.simulated_fallback.lunar_facts.facts().to_vec();
    facts.push(LunarYearFact {
        year: 2100,
        lunar_new_year: d("2100-02-01"),
        dragon_boat: d("2100-06-01"),
        mid_autumn: d("2100-09-01"),
        qingming: d("2100-04-05"),
    });
    spec.runtime_max_end = d("2100-12-31");
    spec.simulated_fallback =
        SimulatedFallbackRuleset::new(1, 2026, LunarYearFacts::from_facts(facts).unwrap()).unwrap();
    let calendar = TradingCalendar::from_policy(CalendarPolicy::from_parts(spec).unwrap()).unwrap();
    calendar.validate_runtime_start(d("2100-01-01")).unwrap();
    assert_eq!(
        calendar.year_label(CalendarExchange::Sse, 2100).unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    assert_eq!(
        calendar
            .day_status(CalendarExchange::Sse, d("2100-01-01"))
            .unwrap(),
        DayStatus::Closed(ClosedReason::SimulatedHoliday(HolidayKind::NewYearDay))
    );
    assert_eq!(
        calendar
            .day_status(CalendarExchange::Szse, d("2100-01-04"))
            .unwrap(),
        DayStatus::Trading
    );
    assert!(
        calendar
            .trading_days_before(CalendarExchange::Szse, d("2100-01-04"), 360)
            .unwrap()
            < d("2099-01-01")
    );
    let encoded = serde_json::to_string(calendar.policy()).unwrap();
    let decoded: CalendarPolicy = serde_json::from_str(&encoded).unwrap();
    assert_eq!(TradingCalendar::from_policy(decoded).unwrap(), calendar);
}
