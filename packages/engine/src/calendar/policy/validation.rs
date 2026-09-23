//! 政策装配与校验的不变量检查（边界序、官方覆盖冲突、事实表覆盖、digest）。
//!
//! 全部为 `CalendarPolicy` 装配路径（`from_parts`/`validate`）的私有支持函数。

use super::coverage::OfficialCoverageEntry;
use super::CalendarPolicySpec;
use crate::calendar::data::LunarYearFacts;
use crate::calendar::date::CivilDate;
use crate::calendar::{fnv1a64_hex, CalendarError};

impl super::CalendarPolicy {
    /// 全量校验（含各级 digest 复核）——存档恢复的必经入口。
    /// 内层 digest 先行（定位最具体的篡改点），再复核外层 content digest。
    pub fn validate(&self) -> Result<(), CalendarError> {
        self.simulated_fallback.validate()?;
        let computed = Self::compute_content_digest(&self.spec());
        if computed != self.content_digest {
            return Err(CalendarError::DigestMismatch {
                field: "content_digest",
                stored: self.content_digest.clone(),
                computed,
            });
        }
        super::CalendarPolicy::from_parts(self.spec()).map(|_| ())
    }

    pub(super) fn check_bounds(
        default_start: CivilDate,
        runtime_min_start: CivilDate,
        runtime_max_end: CivilDate,
        init_only_min_start: CivilDate,
    ) -> Result<(), CalendarError> {
        let invalid = |reason: String| CalendarError::PolicyInvalid { reason };
        if init_only_min_start >= runtime_min_start {
            return Err(invalid(format!(
                "init-only floor {init_only_min_start} must precede runtime min start {runtime_min_start}"
            )));
        }
        if !(runtime_min_start <= default_start && default_start <= runtime_max_end) {
            return Err(invalid(format!(
                "default start {default_start} must lie in runtime window [{runtime_min_start}, {runtime_max_end}]"
            )));
        }
        Ok(())
    }

    pub(super) fn check_coverage_entries(
        entries: &[OfficialCoverageEntry],
        floor: CivilDate,
        ceiling: CivilDate,
    ) -> Result<(), CalendarError> {
        let conflicting = |detail: String| CalendarError::ConflictingOfficialCoverage { detail };
        for pair in entries.windows(2) {
            let a = (pair[0].exchange, pair[0].year);
            let b = (pair[1].exchange, pair[1].year);
            if a >= b {
                return Err(conflicting(format!(
                    "official coverage entries must be unique and sorted by (exchange, year): {a:?} vs {b:?}"
                )));
            }
        }
        for entry in entries {
            check_single_entry(entry, floor, ceiling)?;
        }
        Ok(())
    }

    pub(super) fn check_facts_coverage(
        facts: &LunarYearFacts,
        floor: CivilDate,
        ceiling: CivilDate,
    ) -> Result<(), CalendarError> {
        for year in floor.year()..=ceiling.year() {
            facts.fact_for_year(year)?;
        }
        Ok(())
    }

    pub(super) fn compute_content_digest(spec: &CalendarPolicySpec) -> String {
        let mut parts = vec![
            "calendar-policy".to_string(),
            spec.algorithm_version.to_string(),
            spec.default_start.to_iso(),
            spec.runtime_min_start.to_iso(),
            spec.runtime_max_end.to_iso(),
            spec.init_only_min_start.to_iso(),
        ];
        for entry in &spec.official_coverage {
            let ranges = entry
                .closed_ranges
                .iter()
                .map(|r| format!("{}..{}", r.from.to_iso(), r.to.to_iso()))
                .collect::<Vec<_>>()
                .join(";");
            parts.push(format!(
                "{}/{}/{}/{}",
                entry.exchange.id(),
                entry.year,
                entry.source_citation_id,
                ranges
            ));
        }
        parts.push(spec.simulated_fallback.digest.clone());
        let refs = parts.iter().map(String::as_str).collect::<Vec<_>>();
        fnv1a64_hex(&refs)
    }
}

/// 单条官方覆盖条目的完整性：年份在窗、出处非空、区间非空/同年/有序/不重叠。
fn check_single_entry(
    entry: &OfficialCoverageEntry,
    floor: CivilDate,
    ceiling: CivilDate,
) -> Result<(), CalendarError> {
    let invalid = |reason: String| CalendarError::PolicyInvalid { reason };
    let conflicting = |detail: String| CalendarError::ConflictingOfficialCoverage { detail };
    if entry.year < floor.year() || entry.year > ceiling.year() {
        return Err(invalid(format!(
            "official coverage year {} outside applicability [{}, {}]",
            entry.year,
            floor.year(),
            ceiling.year()
        )));
    }
    if entry.source_citation_id.trim().is_empty() || entry.source_digest.trim().is_empty() {
        return Err(invalid(format!(
            "official coverage {}/{} lacks source citation (未核验通知不得标记 Official)",
            entry.exchange.id(),
            entry.year
        )));
    }
    if entry.closed_ranges.is_empty() {
        return Err(invalid(format!(
            "official coverage {}/{} has no closed ranges",
            entry.exchange.id(),
            entry.year
        )));
    }
    let mut sorted = entry.closed_ranges.clone();
    sorted.sort();
    for pair in sorted.windows(2) {
        if pair[0].to >= pair[1].from {
            return Err(conflicting(format!(
                "overlapping closed ranges in {}/{}: {}..{} vs {}..{}",
                entry.exchange.id(),
                entry.year,
                pair[0].from,
                pair[0].to,
                pair[1].from,
                pair[1].to
            )));
        }
    }
    for range in &sorted {
        if range.from > range.to || range.from.year() != entry.year || range.to.year() != entry.year
        {
            return Err(invalid(format!(
                "closed range {}..{} must be in-year and ordered for {}/{}",
                range.from,
                range.to,
                entry.exchange.id(),
                entry.year
            )));
        }
    }
    Ok(())
}
