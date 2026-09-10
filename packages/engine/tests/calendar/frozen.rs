//! 存档冻结语义：恢复的日历政策是权威，新默认表不得覆盖（K1）。
//!
//! 任务 4 在类型层证明机制；session 存档接线在任务 27。

use crate::{d, default_calendar};
use engine::calendar::{CalendarExchange, DayStatus, TradingCalendar, YearCoverageLabel};

#[test]
fn frozen_calendar_survives_new_defaults() {
    // v1：当前发布默认政策（内嵌完整政策数据：官方覆盖 + 模拟回退 + 农历事实表）。
    let v1 = engine::calendar::CalendarPolicy::default_v1().unwrap();
    let v1_bytes = serde_json::to_vec(&v1).unwrap();

    // v2：模拟“未来发布的新默认表”——算法版本升级、2026 已补证（通知未核验年
    // 前移到 2025）、沪市 2030 新增官方覆盖（额外休市 2030-09-30）。与 v1 在
    // 探针查询上有真实分歧，否则本测试没有区分力。
    let mut spec = v1.spec();
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
    let v2 = engine::calendar::CalendarPolicy::from_parts(spec).unwrap();
    let cal_v2 = TradingCalendar::from_policy(v2.clone()).unwrap();

    // 分歧确实存在（v2 自身语义生效）。
    assert_eq!(
        cal_v2.year_label(CalendarExchange::Sse, 2026).unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    assert_eq!(
        cal_v2.year_label(CalendarExchange::Sse, 2030).unwrap(),
        YearCoverageLabel::Official
    );
    assert!(matches!(
        cal_v2
            .day_status(CalendarExchange::Sse, d("2030-09-30"))
            .unwrap(),
        DayStatus::Closed(engine::calendar::ClosedReason::OfficialHoliday { .. })
    ));

    // 恢复：反序列化 v1 字节 → 不读任何新默认表，v1 对所有查询保持权威。
    let restored: engine::calendar::CalendarPolicy = serde_json::from_slice(&v1_bytes).unwrap();
    let cal_v1 = TradingCalendar::from_policy(restored).unwrap();
    assert_eq!(
        cal_v1.year_label(CalendarExchange::Sse, 2026).unwrap(),
        YearCoverageLabel::NoticeTextUnverified
    );
    assert_eq!(
        cal_v1.year_label(CalendarExchange::Sse, 2030).unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    assert_eq!(
        cal_v1
            .day_status(CalendarExchange::Sse, d("2030-09-30"))
            .unwrap(),
        DayStatus::Trading
    );
    assert_eq!(
        cal_v1.policy().algorithm_version(),
        v1.algorithm_version(),
        "恢复政策不得被新算法版本覆盖"
    );

    // 恢复结果与同版本全新构建逐日一致（确定性）。
    let fresh = default_calendar();
    let mut probe = d("2030-01-01");
    while probe <= d("2030-12-31") {
        assert_eq!(
            cal_v1.day_status(CalendarExchange::Sse, probe).unwrap(),
            fresh.day_status(CalendarExchange::Sse, probe).unwrap(),
            "restored v1 diverged from fresh v1 at {probe:?}"
        );
        probe = probe.next().unwrap();
    }
}
