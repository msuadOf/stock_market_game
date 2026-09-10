//! 冻结交易日历政策（K1）：算法版本、适用范围、沪深官方休市覆盖、模拟回退。
//!
//! 政策与法源基线见 docs/simulation-calendar.md；机器可读冻结值见
//! tests/fixtures/company-model/policy-sources.json `calendar` 节（任务 2 已
//! test-enforced，本模块运行时值与其对账——见 tests/calendar/fixture_binding.rs）。
//!
//! **冻结语义**：`CalendarPolicy` 内嵌完整政策数据（官方覆盖 + 模拟回退 +
//! 农历事实表 + 各级 digest）。恢复存档时反序列化出的政策即权威：
//! `TradingCalendar::from_policy` 只做自洽校验（digest/形状），不读、也不被
//! 更新版默认表覆盖（session 存档接线在任务 27，机制由本模块类型保证）。
//!
//! 分层：`coverage`（覆盖/回退数据类型）、`validation`（装配不变量 + digest），
//! 本文件负责政策类型本体与装配入口。

mod coverage;
mod validation;

pub use coverage::{
    CalendarExchange, DateRange, OfficialCoverageEntry, SimulatedFallbackRuleset, YearCoverageLabel,
};

use super::data::embedded_lunar_facts;
use super::date::CivilDate;
use super::CalendarError;

/// 合同计息基准（K2 游戏假设，fixture `calendar.day_count_basis`）。
pub const DAY_COUNT_BASIS_ACT_365F: &str = "ACT/365F";

/// 政策装配入参（`from_parts` 全量校验后生成带 digest 的 `CalendarPolicy`）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CalendarPolicySpec {
    pub algorithm_version: u32,
    pub default_start: CivilDate,
    pub runtime_min_start: CivilDate,
    pub runtime_max_end: CivilDate,
    pub init_only_min_start: CivilDate,
    pub official_coverage: Vec<OfficialCoverageEntry>,
    pub simulated_fallback: SimulatedFallbackRuleset,
}

/// 完整、可冻结、可序列化的交易日历政策。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CalendarPolicy {
    algorithm_version: u32,
    default_start: CivilDate,
    runtime_min_start: CivilDate,
    runtime_max_end: CivilDate,
    init_only_min_start: CivilDate,
    official_coverage: Vec<OfficialCoverageEntry>,
    simulated_fallback: SimulatedFallbackRuleset,
    content_digest: String,
}

impl CalendarPolicy {
    /// 当前发布默认政策 v1（内嵌 HKO 事实表；2026 通知原文未取得 → 无
    /// Official 条目，2026 按通知未核验年处理）。
    pub fn default_v1() -> Result<Self, CalendarError> {
        Self::from_parts(CalendarPolicySpec {
            algorithm_version: 1,
            default_start: CivilDate::from_iso("2030-01-01")?,
            runtime_min_start: CivilDate::from_iso("2000-01-01")?,
            runtime_max_end: CivilDate::from_iso("2099-12-31")?,
            init_only_min_start: CivilDate::from_iso("1998-01-01")?,
            official_coverage: Vec::new(),
            simulated_fallback: SimulatedFallbackRuleset::new(1, 2026, embedded_lunar_facts()?)?,
        })
    }

    /// 装配 + 全量校验（顺序/覆盖冲突/事实表覆盖年/出处完整性），失败即 Err
    /// （校验实现见 validation.rs）。
    pub fn from_parts(spec: CalendarPolicySpec) -> Result<Self, CalendarError> {
        spec.simulated_fallback.validate()?;
        Self::check_bounds(
            spec.default_start,
            spec.runtime_min_start,
            spec.runtime_max_end,
            spec.init_only_min_start,
        )?;
        Self::check_coverage_entries(
            &spec.official_coverage,
            spec.init_only_min_start,
            spec.runtime_max_end,
        )?;
        Self::check_facts_coverage(
            &spec.simulated_fallback.lunar_facts,
            spec.init_only_min_start,
            spec.runtime_max_end,
        )?;
        let content_digest = Self::compute_content_digest(&spec);
        Ok(Self {
            algorithm_version: spec.algorithm_version,
            default_start: spec.default_start,
            runtime_min_start: spec.runtime_min_start,
            runtime_max_end: spec.runtime_max_end,
            init_only_min_start: spec.init_only_min_start,
            official_coverage: spec.official_coverage,
            simulated_fallback: spec.simulated_fallback,
            content_digest,
        })
    }

    /// 导出可再装配的入参（不含 content_digest）。
    pub fn spec(&self) -> CalendarPolicySpec {
        CalendarPolicySpec {
            algorithm_version: self.algorithm_version,
            default_start: self.default_start,
            runtime_min_start: self.runtime_min_start,
            runtime_max_end: self.runtime_max_end,
            init_only_min_start: self.init_only_min_start,
            official_coverage: self.official_coverage.clone(),
            simulated_fallback: self.simulated_fallback.clone(),
        }
    }

    pub fn algorithm_version(&self) -> u32 {
        self.algorithm_version
    }

    pub fn default_start(&self) -> CivilDate {
        self.default_start
    }

    pub fn runtime_min_start(&self) -> CivilDate {
        self.runtime_min_start
    }

    pub fn runtime_max_end(&self) -> CivilDate {
        self.runtime_max_end
    }

    pub fn init_only_min_start(&self) -> CivilDate {
        self.init_only_min_start
    }

    pub fn official_coverage(&self) -> &[OfficialCoverageEntry] {
        &self.official_coverage
    }

    pub fn simulated_fallback(&self) -> &SimulatedFallbackRuleset {
        &self.simulated_fallback
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    /// 年份覆盖标签：官方条目 → `Official`；否则按通知未核验年界限分
    /// `NoticeTextUnverified` / `SimulatedFuture` / `SimulatedHistorical`。
    pub fn year_label(
        &self,
        exchange: CalendarExchange,
        year: i32,
    ) -> Result<YearCoverageLabel, CalendarError> {
        if year < self.init_only_min_start.year() || year > self.runtime_max_end.year() {
            return Err(CalendarError::PolicyInvalid {
                reason: format!("year {year} outside applicability"),
            });
        }
        if self
            .official_coverage
            .iter()
            .any(|e| e.exchange == exchange && e.year == year)
        {
            return Ok(YearCoverageLabel::Official);
        }
        match year.cmp(&self.simulated_fallback.notice_unverified_year) {
            std::cmp::Ordering::Equal => Ok(YearCoverageLabel::NoticeTextUnverified),
            std::cmp::Ordering::Greater => Ok(YearCoverageLabel::SimulatedFuture),
            std::cmp::Ordering::Less => Ok(YearCoverageLabel::SimulatedHistorical),
        }
    }
}
