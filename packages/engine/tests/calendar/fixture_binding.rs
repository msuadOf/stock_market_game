//! 实现常量与任务 2 冻结 fixture 的机器对账（policy-sources.json calendar 节）。
//!
//! fixture 值由 `tests/policy_manifest.rs` 独立锁定；本用例把 engine::calendar
//! 的运行时常量与 fixture 绑定，防止实现与已核验政策基线漂移。

use crate::default_calendar;

const MANIFEST_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/company-model/policy-sources.json"
);

#[test]
fn policy_constants_match_frozen_fixture() {
    let raw = std::fs::read_to_string(MANIFEST_PATH).expect("fixture readable");
    let manifest: serde_json::Value = serde_json::from_str(&raw).expect("fixture valid JSON");
    let calendar = &manifest["calendar"];

    let cal = default_calendar();
    let policy = cal.policy();
    assert_eq!(
        calendar["default_start_date"].as_str().unwrap(),
        policy.default_start().to_iso()
    );
    assert_eq!(
        calendar["runtime_min_start"].as_str().unwrap(),
        policy.runtime_min_start().to_iso()
    );
    assert_eq!(
        calendar["runtime_max_end"].as_str().unwrap(),
        policy.runtime_max_end().to_iso()
    );
    assert_eq!(
        calendar["init_only_min_start"].as_str().unwrap(),
        policy.init_only_min_start().to_iso()
    );
    assert_eq!(
        calendar["day_count_basis"].as_str().unwrap(),
        engine::calendar::DAY_COUNT_BASIS_ACT_365F
    );

    // fixture 2026 条目状态 notice-text-unverified ⇔ 引擎年标签一致。
    let coverage_2026 = calendar["official_coverage"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["year"].as_i64() == Some(2026))
        .expect("fixture has 2026 coverage entry");
    assert_eq!(
        coverage_2026["status"].as_str().unwrap(),
        "notice-text-unverified"
    );
    assert_eq!(
        default_calendar()
            .year_label(engine::calendar::CalendarExchange::Sse, 2026)
            .unwrap(),
        engine::calendar::YearCoverageLabel::NoticeTextUnverified
    );
}

#[test]
fn lunar_fact_table_provenance_and_coverage() {
    // 来源可追溯：官方来源 URL + 取证日期（与 fixture 同一取证批次）。
    let url = engine::calendar::data::LUNAR_SOURCE_URL_EN;
    assert!(url.starts_with("https://www.hko.gov.hk/"), "官方来源域名");
    assert!(
        url.contains("T{year}e.txt") || url.contains("{year}"),
        "年度文件模式"
    );
    assert_eq!(engine::calendar::data::LUNAR_RETRIEVAL_DATE, "2026-09-10");

    // 覆盖 1998–2099 连续 102 年；两端与初始化下界/运行上界对齐。
    let facts = engine::calendar::data::embedded_lunar_facts().unwrap();
    let years: Vec<i32> = facts.facts().iter().map(|f| f.year).collect();
    assert_eq!(years.first(), Some(&1998));
    assert_eq!(years.last(), Some(&2099));
    assert_eq!(years.len(), 2100 - 1998, "无缺年");
    for pair in years.windows(2) {
        assert_eq!(pair[1] - pair[0], 1, "年份必须连续");
    }

    // 每年事实形状：春节在 1-21..2-21；清明 4 月 4/5 日；日期均落在当年。
    for f in facts.facts() {
        let cny = f.lunar_new_year;
        assert_eq!(cny.year(), f.year);
        let in_window =
            (cny.month() == 1 && cny.day() >= 21) || (cny.month() == 2 && cny.day() <= 21);
        assert!(in_window, "{} 春节越界: {cny:?}", f.year);
        assert_eq!(f.qingming.month(), 4);
        assert!(
            (4..=5).contains(&f.qingming.day()),
            "{} 清明非 4/5 日",
            f.year
        );
        for date in [f.dragon_boat, f.mid_autumn, f.qingming] {
            assert_eq!(date.year(), f.year);
        }
        // 除夕（春节前一日）与春节同年（春节不早于 1-21 保证）。
        assert_eq!(cny.prev().unwrap().year(), f.year);
    }

    // 内嵌表与默认政策内嵌副本一致（政策冻结的就是这张表）。
    let default_cal = default_calendar();
    let policy_facts = &default_cal.policy().simulated_fallback().lunar_facts;
    assert_eq!(policy_facts.digest(), facts.digest());
}
