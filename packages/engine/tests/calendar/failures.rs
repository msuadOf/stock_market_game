//! 负向路径：越界、缺事实表年份、冲突官方覆盖、错误 digest、调休周末、非法日期。
//!
//! 全部断言**类型化错误**——无静默钳位、无默认值顶替、无状态变更。

use crate::{d, default_calendar, policy_with_synthetic_coverage};
use engine::calendar::{
    CalendarError, CalendarExchange, CalendarPolicy, CivilDate, DayStatus, LunarYearFact,
    LunarYearFacts, SimulatedFallbackRuleset,
};

#[test]
fn rejects_illegal_dates() {
    use engine::calendar::CivilDateError;
    for (y, m, day) in [(2030, 2, 30), (2030, 4, 31), (1999, 2, 29)] {
        assert!(
            matches!(
                CivilDate::from_ymd(y, m, day),
                Err(CivilDateError::DayOutOfRange { .. })
            ),
            "{y}-{m}-{day} must be rejected"
        );
    }
    // 跨世纪算法：2100-02-29 在算法层非法（整百年不闰）。
    assert!(matches!(
        CivilDate::from_ymd(2100, 2, 29),
        Err(CivilDateError::DayOutOfRange {
            days_in_month: 28,
            ..
        })
    ));
}

#[test]
fn out_of_range_dates_are_typed_errors() {
    let cal = default_calendar();
    // 早于初始化专用下界 1998-01-01。
    assert!(matches!(
        cal.day_status(CalendarExchange::Sse, d("1997-12-31")),
        Err(CalendarError::BeforeInitFloor { date, .. }) if date == d("1997-12-31")
    ));
    // 晚于运行上界 2099-12-31（CivilDate 本身允许 2100，政策层拒绝）。
    assert!(matches!(
        cal.day_status(CalendarExchange::Sse, d("2100-01-01")),
        Err(CalendarError::AfterRuntimeCeiling { .. })
    ));
    assert!(matches!(
        cal.is_trading_day(CalendarExchange::Szse, d("2100-06-01")),
        Err(CalendarError::AfterRuntimeCeiling { .. })
    ));
    // 2099-12-31（周四，最后一个合法日）之后无交易日。
    assert!(matches!(
        cal.next_trading_day(CalendarExchange::Sse, d("2099-12-31")),
        Err(CalendarError::AfterRuntimeCeiling { .. })
    ));
    // 1998-01-02 之前无交易日（1998-01-01 为元旦+元旦前无覆盖）。
    assert!(matches!(
        cal.previous_trading_day(CalendarExchange::Sse, d("1998-01-02")),
        Err(CalendarError::BeforeInitFloor { .. })
    ));
    // 1998–1999 仅供初始化：不可作为运行时开局（含下界当日）。
    for bad in ["1998-01-01", "1998-06-15", "1999-12-31"] {
        assert!(
            matches!(
                cal.validate_runtime_start(d(bad)),
                Err(CalendarError::RuntimeStartOutOfRange { .. })
            ),
            "{bad} must not be a runtime start"
        );
    }
}

#[test]
fn missing_fact_table_year_is_typed_error() {
    let cal = default_calendar();
    let facts = &cal.policy().simulated_fallback().lunar_facts;
    // 内嵌表覆盖 1998–2099；上下界之外查询 → 缺年份错误（不用邻年顶替）。
    for missing in [1997, 2100, 9999] {
        assert!(matches!(
            facts.fact_for_year(missing),
            Err(CalendarError::MissingLunarFact { year }) if year == missing
        ));
    }

    // 截断的事实表自身合法，但查询缺口年份必须显式失败。
    let truncated = LunarYearFacts::from_facts(facts.facts()[..40].to_vec()).unwrap();
    assert_eq!(truncated.fact_for_year(2037).unwrap().year, 2037);
    assert!(matches!(
        truncated.fact_for_year(2038),
        Err(CalendarError::MissingLunarFact { year: 2038 })
    ));

    // 用截断表构造覆盖 2099 的政策 → 在装配边界以"缺 2038 年"显式失败。
    let base = CalendarPolicy::default_v1().unwrap();
    let mut spec = base.spec();
    spec.simulated_fallback = SimulatedFallbackRuleset::new(1, 2026, truncated).unwrap();
    assert!(matches!(
        CalendarPolicy::from_parts(spec),
        Err(CalendarError::MissingLunarFact { year: 2038 })
    ));
}

#[test]
fn rejects_duplicate_and_conflicting_official_coverage() {
    let base = CalendarPolicy::default_v1().unwrap();

    // 同一交易所同一年重复登记。
    let mut spec = base.spec();
    for _ in 0..2 {
        spec.official_coverage
            .push(engine::calendar::OfficialCoverageEntry::new(
                CalendarExchange::Sse,
                2030,
                vec![(d("2030-10-01"), d("2030-10-03"))],
                "sse-2030".to_string(),
                "digest-a".to_string(),
            ));
    }
    assert!(matches!(
        CalendarPolicy::from_parts(spec),
        Err(CalendarError::ConflictingOfficialCoverage { .. })
    ));

    // 单条目内闭市区间互相重叠。
    let mut spec = base.spec();
    spec.official_coverage
        .push(engine::calendar::OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2030,
            vec![
                (d("2030-10-01"), d("2030-10-03")),
                (d("2030-10-02"), d("2030-10-06")),
            ],
            "sse-2030".to_string(),
            "digest-a".to_string(),
        ));
    assert!(matches!(
        CalendarPolicy::from_parts(spec),
        Err(CalendarError::ConflictingOfficialCoverage { .. })
    ));

    // 闭市区间跨年。
    let mut spec = base.spec();
    spec.official_coverage
        .push(engine::calendar::OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2030,
            vec![(d("2030-12-30"), d("2031-01-02"))],
            "sse-2030".to_string(),
            "digest-a".to_string(),
        ));
    assert!(matches!(
        CalendarPolicy::from_parts(spec),
        Err(CalendarError::PolicyInvalid { .. })
    ));

    // Official 条目缺原文出处标识：宁可拒绝也不冒充已核验。
    let mut spec = base.spec();
    spec.official_coverage
        .push(engine::calendar::OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2030,
            vec![(d("2030-10-01"), d("2030-10-03"))],
            String::new(),
            "digest-a".to_string(),
        ));
    assert!(matches!(
        CalendarPolicy::from_parts(spec),
        Err(CalendarError::PolicyInvalid { .. })
    ));
}

#[test]
fn rejects_wrong_digest_on_restore() {
    let v1 = CalendarPolicy::default_v1().unwrap();
    let json = serde_json::to_string(&v1).unwrap();

    // 篡改事实表内容但保留 digest → 校验失败。
    let mut tampered: serde_json::Value = serde_json::from_str(&json).unwrap();
    tampered["simulated_fallback"]["lunar_facts"]["facts"][0]["lunar_new_year"] =
        serde_json::json!("1998-01-29");
    let restored: Result<CalendarPolicy, _> = serde_json::from_value(tampered);
    assert!(matches!(
        engine::calendar::TradingCalendar::from_policy(restored.unwrap()),
        Err(CalendarError::DigestMismatch { .. })
    ));

    // 篡改政策级 digest。
    let mut tampered: serde_json::Value = serde_json::from_str(&json).unwrap();
    tampered["content_digest"] = serde_json::json!("0000000000000000");
    let restored: CalendarPolicy = serde_json::from_value(tampered).unwrap();
    assert!(matches!(
        engine::calendar::TradingCalendar::from_policy(restored),
        Err(CalendarError::DigestMismatch {
            field: "content_digest",
            ..
        })
    ));

    // 篡改回退规则集 digest。
    let mut tampered: serde_json::Value = serde_json::from_str(&json).unwrap();
    tampered["simulated_fallback"]["digest"] = serde_json::json!("deadbeefdeadbeef");
    let restored: CalendarPolicy = serde_json::from_value(tampered).unwrap();
    assert!(matches!(
        engine::calendar::TradingCalendar::from_policy(restored),
        Err(CalendarError::DigestMismatch {
            field: "simulated_fallback",
            ..
        })
    ));

    // 直接构造内部不一致的 LunarYearFacts（digest 与内容不符）。
    let facts = engine::calendar::data::embedded_lunar_facts().unwrap();
    let f0 = facts.facts()[0].clone();
    let mutated = LunarYearFact {
        year: f0.year,
        lunar_new_year: d("1998-01-29"),
        dragon_boat: f0.dragon_boat,
        mid_autumn: f0.mid_autumn,
        qingming: f0.qingming,
    };
    let mut raw = facts.facts().to_vec();
    raw[0] = mutated;
    let inconsistent = LunarYearFacts::rehydrated(raw, facts.digest().to_string()).unwrap();
    assert!(matches!(
        inconsistent.validate(),
        Err(CalendarError::DigestMismatch {
            field: "lunar_facts",
            ..
        })
    ));
}

#[test]
fn makeup_weekends_never_trading_days() {
    let cal = default_calendar();
    // 真实政府调休上班的周末（2024-02-04 周日、2025-01-26 周日）。
    for makeup in ["2024-02-04", "2025-01-26"] {
        assert_eq!(
            cal.day_status(CalendarExchange::Sse, d(makeup)).unwrap(),
            DayStatus::Closed(engine::calendar::ClosedReason::Weekend),
            "{makeup} 是调休补班周末，但不是交易日（K1 不模拟补班）"
        );
    }

    // 即使是（合成的）官方覆盖年份，周末仍休市：官方公告只加休市日，
    // 覆盖区间内的周末报告 Weekend，工作日报告 OfficialHoliday。
    let cal = engine::calendar::TradingCalendar::from_policy(policy_with_synthetic_coverage(
        CalendarExchange::Sse,
        2030,
        &[("2030-10-01", "2030-10-09")],
    ))
    .unwrap();
    assert_eq!(
        cal.day_status(CalendarExchange::Sse, d("2030-10-05"))
            .unwrap(),
        DayStatus::Closed(engine::calendar::ClosedReason::Weekend),
        "2030-10-05 是周六"
    );
    assert!(matches!(
        cal.day_status(CalendarExchange::Sse, d("2030-10-07"))
            .unwrap(),
        DayStatus::Closed(engine::calendar::ClosedReason::OfficialHoliday { .. })
    ));
    assert_eq!(
        cal.day_status(CalendarExchange::Sse, d("2030-10-10"))
            .unwrap(),
        DayStatus::Trading,
        "覆盖区间外的工作日照常开市"
    );
}
