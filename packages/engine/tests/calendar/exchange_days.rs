//! 交易所交易日段：开局边界与前史定位、模拟假日与调休周末、年份标签、
//! 同政策确定性。入口见 main.rs `gregorian_and_exchange_days`。

use super::{d, default_calendar, policy_with_synthetic_coverage};
use engine::calendar::{
    CalendarError, CalendarExchange, ClosedReason, DayStatus, HolidayKind, TradingCalendar,
    TradingDayOrdinal, YearCoverageLabel,
};
use std::collections::BTreeSet;

pub(crate) fn default_start_and_runtime_bounds() {
    let cal = default_calendar();
    let policy = cal.policy();
    // 与 fixture 冻结值一致（另见 fixture_binding 用例的对账）。
    assert_eq!(policy.default_start(), d("2030-01-01"));
    assert_eq!(policy.runtime_min_start(), d("2000-01-01"));
    assert_eq!(policy.runtime_max_end(), d("2099-12-31"));
    assert_eq!(policy.init_only_min_start(), d("1998-01-01"));

    // 不同起点：合法开局区间 [2000-01-01, 2099-12-31] 内任意日期通过；
    // 1998–1999 仅供初始化前史查询，不是合法开局（越界即类型化错误，不改状态）。
    for ok in ["2000-01-01", "2030-01-01", "2050-07-15", "2099-12-31"] {
        assert!(
            cal.validate_runtime_start(d(ok)).is_ok(),
            "{ok} must be a legal start"
        );
    }
    for bad in ["1997-12-31", "1998-01-01", "1999-12-31", "2100-01-01"] {
        assert!(
            cal.validate_runtime_start(d(bad)).is_err(),
            "{bad} must be rejected as a session start"
        );
    }
}

pub(crate) fn prehistory_360_trading_days_locatable() {
    let cal = default_calendar();
    // 最早开局 2000-01-01 之前的 360 个交易日必须全部可定位且不早于 1998-01-01。
    let start = d("2000-01-01");
    let t360 = cal
        .trading_days_before(CalendarExchange::Sse, start, 360)
        .expect("360 prehistory trading days before 2000-01-01");
    assert!(
        t360 >= d("1998-01-01"),
        "prehistory floor violated: {t360:?}"
    );

    // 序数锚定：0 号交易日 = 1998-01-02（1998-01-01 周四但元旦休市）。
    assert_eq!(
        cal.date_of_ordinal(CalendarExchange::Sse, TradingDayOrdinal::from_u32(0))
            .unwrap(),
        d("1998-01-02")
    );
    assert_eq!(
        cal.trading_day_ordinal(CalendarExchange::Sse, d("1998-01-02"))
            .unwrap()
            .value(),
        0
    );

    // 1998+1999 兜底交易日恰 504 个（春节/端午/中秋/清明按事实表逐日扣除后）：
    // 第 504 个可定位（= 1998-01-02），第 505 个触发前史耗尽的类型化错误。
    let t504 = cal
        .trading_days_before(CalendarExchange::Sse, start, 504)
        .expect("all 504 fallback trading days of 1998-1999 are locatable");
    assert_eq!(t504, d("1998-01-02"));
    assert!(matches!(
        cal.trading_days_before(CalendarExchange::Sse, start, 505),
        Err(CalendarError::PrehistoryExhausted {
            needed: 505,
            available: 504,
            ..
        })
    ));
    assert!(matches!(
        cal.trading_days_before(CalendarExchange::Sse, start, 0),
        Err(CalendarError::InvalidCount { .. })
    ));

    // 深交所同一兜底规则下结果一致。
    assert_eq!(
        cal.trading_days_before(CalendarExchange::Szse, start, 360)
            .unwrap(),
        t360
    );
}

pub(crate) fn makeup_weekends_and_holiday_fallback() {
    let cal = default_calendar();

    // 政府调休补班周末（如 2024-02-04、2025-01-26 为真实调休上班日）不是交易日。
    for makeup in ["2024-02-04", "2025-01-26", "2030-01-05"] {
        let status = cal.day_status(CalendarExchange::Sse, d(makeup)).unwrap();
        assert_eq!(
            status,
            DayStatus::Closed(ClosedReason::Weekend),
            "makeup workday {makeup} must stay closed"
        );
    }

    // 模拟假日：多年份对照事实表（含闰年春节 2032、远端 2051/2099）。
    // 春节除夕至正月初三 = 事实表正月初一的前 1 后 2 共 4 天。
    for year in [2000, 2024, 2025, 2030, 2032, 2051, 2099] {
        let facts = &cal.policy().simulated_fallback().lunar_facts;
        let f = facts.fact_for_year(year).unwrap();
        let closed_dates = BTreeSet::from([
            d(&format!("{year}-01-01")),
            d(&format!("{year}-05-01")),
            d(&format!("{year}-05-02")),
            d(&format!("{year}-10-01")),
            d(&format!("{year}-10-02")),
            d(&format!("{year}-10-03")),
            f.lunar_new_year.prev().unwrap(),
            f.lunar_new_year,
            f.lunar_new_year.next().unwrap(),
            f.lunar_new_year.next().unwrap().next().unwrap(),
            f.qingming,
            f.dragon_boat,
            f.mid_autumn,
        ]);
        // 全年逐日对照：closed ⇔（周末 ∨ 假日集）。规则引擎与独立推导必须等价。
        let mut day = d(&format!("{year}-01-01"));
        let year_end = d(&format!("{year}-12-31"));
        while day <= year_end {
            let weekend = day.weekday().is_weekend();
            let holiday = closed_dates.contains(&day);
            let trading = cal.is_trading_day(CalendarExchange::Sse, day).unwrap();
            assert_eq!(
                trading,
                !weekend && !holiday,
                "{day:?} trading={trading}, weekend={weekend}, holiday={holiday}"
            );
            day = day.next().unwrap();
        }
    }

    // 独立已知锚点（非事实表回声）：2024 春节 2-10、2025 除夕 1-28/春节 1-29、
    // 规则边界精确到除夕（1-27 周一开市）、端午中秋当日。
    assert!(!cal
        .is_trading_day(CalendarExchange::Sse, d("2024-02-10"))
        .unwrap());
    assert!(!cal
        .is_trading_day(CalendarExchange::Szse, d("2025-01-28"))
        .unwrap());
    assert!(!cal
        .is_trading_day(CalendarExchange::Sse, d("2025-01-29"))
        .unwrap());
    assert!(cal
        .is_trading_day(CalendarExchange::Sse, d("2025-01-27"))
        .unwrap());
    assert!(
        !cal.is_trading_day(CalendarExchange::Sse, d("2030-06-05"))
            .unwrap(),
        "2030 端午"
    );
    assert!(
        !cal.is_trading_day(CalendarExchange::Sse, d("2030-09-12"))
            .unwrap(),
        "2030 中秋"
    );
    assert!(
        !cal.is_trading_day(CalendarExchange::Sse, d("2030-04-05"))
            .unwrap(),
        "2030 清明"
    );

    // 假日 reason 可区分（取必然为工作日的假日断言）。
    let status = cal
        .day_status(CalendarExchange::Sse, d("2025-01-29"))
        .unwrap();
    assert_eq!(
        status,
        DayStatus::Closed(ClosedReason::SimulatedHoliday(HolidayKind::SpringFestival))
    );
    let may_day = cal
        .day_status(CalendarExchange::Sse, d("2030-05-01"))
        .unwrap();
    assert_eq!(
        may_day,
        DayStatus::Closed(ClosedReason::SimulatedHoliday(HolidayKind::LabourDay))
    );

    // 下一/上一交易日：跨周末与跨假日。
    assert_eq!(
        cal.next_trading_day(CalendarExchange::Sse, d("2030-01-01"))
            .unwrap(),
        d("2030-01-02")
    );
    assert_eq!(
        cal.next_trading_day(CalendarExchange::Sse, d("2030-10-01"))
            .unwrap(),
        d("2030-10-04")
    );
    assert_eq!(
        cal.previous_trading_day(CalendarExchange::Sse, d("2030-01-01"))
            .unwrap(),
        d("2029-12-31"),
        "元旦前一日（周一，非假日）开市"
    );
}

pub(crate) fn year_labels() {
    let cal = default_calendar();
    // 语义三分（§3.4）：历史缺通知 / 2026 通知原文未核验 / 未来未发布。
    assert_eq!(
        cal.year_label(CalendarExchange::Sse, 1998).unwrap(),
        YearCoverageLabel::SimulatedHistorical
    );
    assert_eq!(
        cal.year_label(CalendarExchange::Sse, 2024).unwrap(),
        YearCoverageLabel::SimulatedHistorical
    );
    assert_eq!(
        cal.year_label(CalendarExchange::Szse, 2026).unwrap(),
        YearCoverageLabel::NoticeTextUnverified
    );
    assert_eq!(
        cal.year_label(CalendarExchange::Sse, 2027).unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    assert_eq!(
        cal.year_label(CalendarExchange::Sse, 2099).unwrap(),
        YearCoverageLabel::SimulatedFuture
    );

    // 官方覆盖优先：合成 2030 沪市 Official 条目只改沪市语义（机制验证，
    // 非默认表状态；深市不受影响）。
    let sse_official = policy_with_synthetic_coverage(
        CalendarExchange::Sse,
        2030,
        &[("2030-09-30", "2030-09-30")],
    );
    let cal = TradingCalendar::from_policy(sse_official).unwrap();
    assert_eq!(
        cal.year_label(CalendarExchange::Sse, 2030).unwrap(),
        YearCoverageLabel::Official
    );
    assert_eq!(
        cal.year_label(CalendarExchange::Szse, 2030).unwrap(),
        YearCoverageLabel::SimulatedFuture
    );
    // 官方覆盖增加休市日：2030-09-30（周一）模拟下本应开市。
    assert!(default_calendar()
        .is_trading_day(CalendarExchange::Sse, d("2030-09-30"))
        .unwrap());
    assert!(matches!(
        cal.day_status(CalendarExchange::Sse, d("2030-09-30"))
            .unwrap(),
        DayStatus::Closed(ClosedReason::OfficialHoliday { .. })
    ));
    assert!(cal
        .is_trading_day(CalendarExchange::Szse, d("2030-09-30"))
        .unwrap());
}

pub(crate) fn same_policy_same_results() {
    let cal_a = default_calendar();
    let policy_bytes = serde_json::to_vec(cal_a.policy()).unwrap();
    let restored =
        TradingCalendar::from_policy(serde_json::from_slice(&policy_bytes).unwrap()).unwrap();
    let mut probe = d("2030-01-01");
    while probe <= d("2030-12-31") {
        assert_eq!(
            cal_a.day_status(CalendarExchange::Sse, probe).unwrap(),
            restored.day_status(CalendarExchange::Sse, probe).unwrap(),
            "{probe:?} diverged"
        );
        probe = probe.next().unwrap();
    }
}
