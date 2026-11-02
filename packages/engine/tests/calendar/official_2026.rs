//! 2026 年沪深交易所官方休市通知的全年边界。
//!
//! 依据：上证公告〔2025〕45号与深证会〔2025〕481号，均于 2025-12-22 发布；
//! 本仓库于 2026-11-02 独立取证。两所公告的休市日期一致，但来源标识必须
//! 分别绑定本所通知，不能用一所公告冒充另一所的 Official 依据。

use crate::{d, default_calendar};
use engine::calendar::{
    CalendarExchange, CalendarPolicy, ClosedReason, DayStatus, TradingCalendar, YearCoverageLabel,
};

fn assert_official_closed(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    iso: &str,
    citation_id: &str,
) {
    match calendar.day_status(exchange, d(iso)).unwrap() {
        DayStatus::Closed(ClosedReason::OfficialHoliday {
            citation_id: actual,
        }) => {
            assert_eq!(actual, citation_id, "{exchange:?} {iso} 的来源标签错误");
        }
        other => panic!("{exchange:?} {iso} 应为 OfficialHoliday，实际 {other:?}"),
    }
}

#[test]
fn official_2026_boundaries_match_both_exchange_notices() {
    let calendar = default_calendar();
    for (exchange, citation_id) in [
        (CalendarExchange::Sse, "sse-2026-holiday-notice"),
        (CalendarExchange::Szse, "szse-2026-holiday-notice"),
    ] {
        assert_eq!(
            calendar.year_label(exchange, 2026).unwrap(),
            YearCoverageLabel::Official
        );

        // 节假日区间内的工作日必须由官方覆盖休市，不能用模拟假日顶替。
        for iso in [
            "2026-01-01", // 元旦区间 01-01..=01-03
            "2026-01-02",
            "2026-02-16", // 春节区间 02-15..=02-23
            "2026-02-17",
            "2026-02-18",
            "2026-02-19",
            "2026-02-20",
            "2026-02-23",
            "2026-04-06", // 清明区间 04-04..=04-06
            "2026-05-01", // 劳动节区间 05-01..=05-05
            "2026-05-04",
            "2026-05-05",
            "2026-06-19", // 端午区间 06-19..=06-21
            "2026-09-25", // 中秋区间 09-25..=09-27
            "2026-10-01", // 国庆区间 10-01..=10-07
            "2026-10-02",
            "2026-10-05",
            "2026-10-06",
            "2026-10-07",
        ] {
            assert_official_closed(&calendar, exchange, iso, citation_id);
        }

        // 每个休市区间前后的最近工作日边界。
        for iso in [
            "2025-12-31",
            "2026-01-05",
            "2026-02-13",
            "2026-02-24",
            "2026-03-02",
            "2026-04-03",
            "2026-04-07",
            "2026-04-30",
            "2026-05-06",
            "2026-05-11",
            "2026-06-18",
            "2026-06-22",
            "2026-09-24",
            "2026-09-28",
            "2026-09-30",
            "2026-10-08",
            "2026-10-12",
        ] {
            assert_eq!(
                calendar.day_status(exchange, d(iso)).unwrap(),
                DayStatus::Trading,
                "{exchange:?} {iso} 应照常开市"
            );
        }
    }
}

#[test]
fn official_2026_explicit_weekend_closures_remain_closed() {
    let calendar = default_calendar();
    // 通知逐日列出的“另外”周末休市日：即使政府调休上班，交易所仍休市。
    // 周末规则优先于官方区间，因此原因标签是 Weekend；日期不得开市。
    for iso in [
        "2026-01-03",
        "2026-01-04",
        "2026-02-14",
        "2026-02-15",
        "2026-02-21",
        "2026-02-22",
        "2026-02-28",
        "2026-04-04",
        "2026-04-05",
        "2026-05-02",
        "2026-05-03",
        "2026-05-09",
        "2026-06-20",
        "2026-06-21",
        "2026-09-20",
        "2026-09-26",
        "2026-09-27",
        "2026-10-03",
        "2026-10-04",
        "2026-10-10",
    ] {
        for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
            assert_eq!(
                calendar.day_status(exchange, d(iso)).unwrap(),
                DayStatus::Closed(ClosedReason::Weekend),
                "{exchange:?} {iso} 必须是周末休市"
            );
        }
    }
}

#[test]
fn simulated_years_outside_2026_are_unchanged() {
    let current = default_calendar();
    let mut simulated_spec = current.policy().spec();
    simulated_spec.official_coverage.clear();
    let simulated = TradingCalendar::from_policy(
        CalendarPolicy::from_parts(simulated_spec).expect("模拟政策必须有效"),
    )
    .unwrap();

    // 去掉官方覆盖后正是升级前的政策；2026 保留旧档兼容标签。
    for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        assert_eq!(
            simulated.year_label(exchange, 2026).unwrap(),
            YearCoverageLabel::NoticeTextUnverified
        );
        assert_eq!(
            simulated.day_status(exchange, d("2026-01-02")).unwrap(),
            DayStatus::Trading,
            "升级前的模拟规则在 2026-01-02 开市"
        );
    }

    // 年份标签与逐日状态都不得把 2026 Official 泄漏到模拟年份。
    for year in [1998, 2025, 2027, 2099] {
        for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
            assert_eq!(
                current.year_label(exchange, year).unwrap(),
                simulated.year_label(exchange, year).unwrap(),
                "{exchange:?} {year} 的年份标签被 2026 官方覆盖改变"
            );
        }
    }
    for year in (1998..=2025).chain(2027..=2099) {
        let mut day = d(&format!("{year}-01-01"));
        let end = d(&format!("{year}-12-31"));
        while day <= end {
            for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
                assert_eq!(
                    current.day_status(exchange, day).unwrap(),
                    simulated.day_status(exchange, day).unwrap(),
                    "{exchange:?} {day:?} 的模拟回退被 2026 官方覆盖改变"
                );
            }
            day = day.next().unwrap();
        }
    }
}
