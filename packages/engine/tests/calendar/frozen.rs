//! 存档冻结语义：恢复的日历政策是权威，新默认表不得覆盖。
//!
//! 在日历类型层验证冻结机制；session 恢复仍沿用存档政策。

use crate::{d, default_calendar};
use engine::calendar::{CalendarExchange, DayStatus, TradingCalendar, YearCoverageLabel};

#[test]
fn frozen_calendar_survives_new_defaults() {
    // 已保存政策：当前发布默认表（内嵌完整政策数据：官方覆盖 + 模拟回退 + 农历事实表）。
    let saved_policy = engine::calendar::CalendarPolicy::current_default_policy().unwrap();
    let saved_policy_bytes = serde_json::to_vec(&saved_policy).unwrap();

    // 模拟未来默认政策：假设算法升级、2026 已补证（通知未核验年
    // 前移到 2025），并加入沪市 2030 模拟覆盖 metadata（额外休市 2030-09-30）。与已保存政策在
    // 探针查询上有真实分歧，否则本测试没有区分力。
    let mut spec = saved_policy.spec();
    spec.algorithm_version = 2;
    spec.simulated_fallback = engine::calendar::SimulatedFallbackRuleset::new(
        spec.simulated_fallback.version + 1,
        2025,
        spec.simulated_fallback.lunar_facts.clone(),
    )
    .unwrap();
    spec.official_coverage
        .push(engine::calendar::OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2030,
            vec![(d("2030-09-30"), d("2030-09-30"))],
            "sse-2030-holiday-notice".to_string(),
            "v2-source-digest".to_string(),
        ));
    let future_default_policy = engine::calendar::CalendarPolicy::from_parts(spec).unwrap();
    let future_calendar = TradingCalendar::from_policy(future_default_policy.clone()).unwrap();

    // 模拟未来默认政策自身的查询结果确有分歧。
    assert_eq!(
        future_calendar
            .year_label(CalendarExchange::Sse, 2026)
            .unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    assert_eq!(
        future_calendar
            .year_label(CalendarExchange::Sse, 2030)
            .unwrap(),
        YearCoverageLabel::Official
    );
    assert!(matches!(
        future_calendar
            .day_status(CalendarExchange::Sse, d("2030-09-30"))
            .unwrap(),
        DayStatus::Closed(engine::calendar::ClosedReason::OfficialHoliday { .. })
    ));

    // 恢复已保存政策字节，不读任何新默认表；所有查询仍以保存的政策为准。
    let restored: engine::calendar::CalendarPolicy =
        serde_json::from_slice(&saved_policy_bytes).unwrap();
    let restored_calendar = TradingCalendar::from_policy(restored).unwrap();
    assert_eq!(
        restored_calendar
            .year_label(CalendarExchange::Sse, 2026)
            .unwrap(),
        YearCoverageLabel::NoticeTextUnverified
    );
    assert_eq!(
        restored_calendar
            .year_label(CalendarExchange::Sse, 2030)
            .unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    assert_eq!(
        restored_calendar
            .day_status(CalendarExchange::Sse, d("2030-09-30"))
            .unwrap(),
        DayStatus::Trading
    );
    assert_eq!(
        restored_calendar.policy().algorithm_version(),
        saved_policy.algorithm_version(),
        "恢复政策不得被新算法版本覆盖"
    );

    // 恢复结果与同版本全新构建逐日一致（确定性）。
    let fresh = default_calendar();
    let mut probe = d("2030-01-01");
    while probe <= d("2030-12-31") {
        assert_eq!(
            restored_calendar
                .day_status(CalendarExchange::Sse, probe)
                .unwrap(),
            fresh.day_status(CalendarExchange::Sse, probe).unwrap(),
            "恢复日历与按保存政策重新构建的日历在 {probe:?} 处不一致"
        );
        probe = probe.next().unwrap();
    }
}
