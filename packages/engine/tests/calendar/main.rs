//! 任务 4（company-information-npc-intentions）：真实公历与冻结交易日历集成测试。
//!
//! 政策与法源基线：`docs/simulation-calendar.md`（K1）与
//! `tests/fixtures/company-model/policy-sources.json` 的 calendar 冻结值
//! （默认 2030-01-01 开局、2000-01-01 至 2099-12-31 运行区间、1998-01-01
//! 初始化专用下界、ACT/365F）。农历/节气事实表来源与取证记录见
//! `packages/engine/src/calendar/data/mod.rs` 头注（香港天文台官方对照表）。
//!
//! 按场景拆分：`gregorian`（纯公历算法段）、`exchange_days`（交易所交易日段）、
//! `frozen`（存档冻结政策不被新默认表覆盖）、`failures`（负向拒绝）、
//! `fixture_binding`（实现常量与任务 2 冻结 fixture 对账）。计划指定的两个
//! QA 入口测试：`gregorian_and_exchange_days` 在本文件，另一个在 frozen.rs。

mod exchange_days;
mod failures;
mod fixture_binding;
mod frozen;
mod gregorian;

use engine::calendar::{CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar};

/// 计划 QA happy 入口：公历算法事实 + 交易所交易日主路径（分段断言）。
#[test]
fn gregorian_and_exchange_days() {
    gregorian::leap_year_rule();
    gregorian::illegal_dates_rejected();
    gregorian::weekday_anchors();
    gregorian::day_counts_and_ordinals();
    gregorian::iso_roundtrip_and_civil_instant();
    exchange_days::default_start_and_runtime_bounds();
    exchange_days::prehistory_360_trading_days_locatable();
    exchange_days::makeup_weekends_and_holiday_fallback();
    exchange_days::year_labels();
    exchange_days::same_policy_same_results();
}

/// 解析测试用 ISO 日期；输入本身必须合法（否则测试夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 当前发布默认政策 v1 构建的日历。
pub(crate) fn default_calendar() -> TradingCalendar {
    TradingCalendar::default_v1().expect("default v1 calendar must construct")
}

/// 在默认政策之上为指定交易所/年份追加一条**测试专用合成**官方覆盖。
///
/// 真实 `Official` 必须有已核验通知原文（docs/simulation-calendar.md §3.1，
/// 2026 年通知原文未取得，保持 notice-text-unverified）；默认表当前不含任何
/// Official 条目。合成条目仅用于验证“官方覆盖优先于模拟回退”的机制。
pub(crate) fn policy_with_synthetic_coverage(
    exchange: engine::calendar::CalendarExchange,
    year: i32,
    ranges: &[(&str, &str)],
) -> CalendarPolicy {
    let base = CalendarPolicy::default_v1().expect("default policy");
    let mut spec = base.spec();
    let closed_ranges = ranges
        .iter()
        .map(|(from, to)| (d(from), d(to)))
        .collect::<Vec<_>>();
    spec.official_coverage.push(OfficialCoverageEntry::new(
        exchange,
        year,
        closed_ranges,
        "synthetic-test-notice".to_string(),
        "synthetic-test-digest".to_string(),
    ));
    CalendarPolicy::from_parts(spec).expect("synthetic coverage policy must validate")
}
