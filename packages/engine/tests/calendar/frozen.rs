//! 存档冻结语义：恢复的日历政策是权威，新默认表不得覆盖。
//!
//! 2026 官方升级既影响新局默认表，也影响“升级前存档”可观察到的开休边界；
//! 恢复必须继续使用存档内嵌的 policy spec，而不是改用新的 Official 表。

use crate::{d, default_calendar};
use engine::calendar::{
    CalendarExchange, CalendarPolicy, ClosedReason, DayStatus, TradingCalendar, YearCoverageLabel,
};
use engine::session::{CivilClock, CivilClockSave, CivilPhase};

#[test]
fn frozen_calendar_survives_new_defaults() {
    // 已保存政策：当前发布默认表（内嵌完整政策数据：官方覆盖 + 模拟回退 + 农历事实表）。
    let saved_policy = engine::calendar::CalendarPolicy::current_default_policy().unwrap();
    let saved_policy_bytes = serde_json::to_vec(&saved_policy).unwrap();

    // 模拟未来默认政策：假设算法升级、通知未核验年边界前移，并为沪市增加
    // 2030 官方覆盖（额外休市 2030-09-30）。与已保存政策在探针查询上有真实
    // 分歧，否则本测试没有区分力。
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
    spec.official_coverage
        .sort_by_key(|entry| (entry.exchange, entry.year));
    let future_default_policy = engine::calendar::CalendarPolicy::from_parts(spec).unwrap();
    let future_calendar = TradingCalendar::from_policy(future_default_policy.clone()).unwrap();

    // 模拟未来默认政策自身的查询结果确有分歧。
    assert_eq!(
        future_calendar
            .year_label(CalendarExchange::Sse, 2026)
            .unwrap(),
        YearCoverageLabel::Official
    );
    assert_eq!(
        future_calendar
            .year_label(CalendarExchange::Sse, 2025)
            .unwrap(),
        YearCoverageLabel::NoticeTextUnverified
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
        YearCoverageLabel::Official
    );
    assert_eq!(
        restored_calendar
            .year_label(CalendarExchange::Sse, 2025)
            .unwrap(),
        YearCoverageLabel::SimulatedHistorical,
        "恢复政策必须保留升级前的历史年份标签"
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
    assert_ne!(
        restored_calendar.policy().algorithm_version(),
        future_calendar.policy().algorithm_version()
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

#[test]
fn pre_upgrade_2026_policy_stays_frozen_through_civil_clock_restore() {
    let current_default = CalendarPolicy::current_default_policy().unwrap();

    // 升级前存档：没有 2026 官方覆盖，2026 仍是通知原文未核验年。
    let mut old_spec = current_default.spec();
    old_spec.official_coverage.clear();
    let saved_policy = CalendarPolicy::from_parts(old_spec).unwrap();

    // 新默认表使用 2026 官方通知，并与旧档在真实开休边界上分歧。
    let current = TradingCalendar::from_policy(current_default).unwrap();
    for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        assert_eq!(
            current.year_label(exchange, 2026).unwrap(),
            YearCoverageLabel::Official
        );
        assert!(matches!(
            current.day_status(exchange, d("2026-01-02")).unwrap(),
            DayStatus::Closed(ClosedReason::OfficialHoliday { .. })
        ));
    }
    assert_eq!(
        saved_policy
            .year_label(CalendarExchange::Sse, 2026)
            .unwrap(),
        YearCoverageLabel::NoticeTextUnverified
    );
    let restored_calendar = TradingCalendar::from_policy(saved_policy.clone()).unwrap();
    assert_eq!(
        restored_calendar
            .day_status(CalendarExchange::Sse, d("2026-01-02"))
            .unwrap(),
        DayStatus::Trading
    );

    // 会话级恢复同样使用 CivilClockSave 内嵌的完整 spec。
    let saved_clock = CivilClockSave {
        current_date: d("2026-01-02"),
        settled_through: Some(d("2026-01-01")),
        policy: saved_policy.spec(),
        policy_digest: None,
    };
    let restored_clock = CivilClock::from_parts_for_exchanges(
        d("2026-01-01"),
        &saved_clock,
        [CalendarExchange::Sse, CalendarExchange::Szse],
    )
    .unwrap();
    assert_eq!(restored_clock.phase(), CivilPhase::IntradayTrading);
    assert_eq!(restored_clock.save().policy, saved_clock.policy);
    assert!(
        restored_clock.save().policy_digest.is_some(),
        "旧档恢复后再保存必须补齐 policy digest"
    );
    assert_eq!(
        restored_clock.save().policy,
        saved_policy.spec(),
        "恢复后再次保存必须保持升级前政策"
    );

    // 旧档缺 digest 不能掩盖其它 policy 字段改写：只有精确的升级前默认政策
    // 才允许走兼容分支。
    let mut tampered_legacy = saved_clock.clone();
    tampered_legacy.policy.algorithm_version += 1;
    assert!(matches!(
        CivilClock::from_parts_for_exchanges(
            d("2026-01-01"),
            &tampered_legacy,
            [CalendarExchange::Sse, CalendarExchange::Szse],
        ),
        Err(engine::session::CivilClockError::SaveInconsistent { .. })
    ));
}
