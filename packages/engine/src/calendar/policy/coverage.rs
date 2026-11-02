//! 沪深官方休市覆盖与模拟回退的**数据类型**（政策装配/校验见同目录 mod 与
//! validation）。

use crate::calendar::data::LunarYearFacts;
use crate::calendar::date::CivilDate;
use crate::calendar::{fnv1a64_hex, CalendarError};

/// 沪深两所标识（日历覆盖维度；与证券上市的 `StockExchange` 分属不同语境，
/// 由会话的自然日时钟接线建立映射，避免本模块反向依赖 session）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[serde(rename_all = "lowercase")]
pub enum CalendarExchange {
    Sse,
    Szse,
}

impl CalendarExchange {
    pub fn id(self) -> &'static str {
        match self {
            CalendarExchange::Sse => "sse",
            CalendarExchange::Szse => "szse",
        }
    }
}

/// 年份覆盖语义标签（§3.4）。四种必须可区分地进入存档与诊断。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum YearCoverageLabel {
    /// 当年存在已核验原文的交易所休市通知。
    Official,
    /// 历史年份缺已核验通知，按模拟回退（1998–2025）。
    SimulatedHistorical,
    /// 未来年份通知未发布（2027+），按模拟回退。
    SimulatedFuture,
    /// 历史上通知客观存在但原文未核验的专态；保留用于恢复升级前冻结政策。
    NoticeTextUnverified,
}

/// 闭市区间（含两端；from ≤ to，同年）。
#[derive(
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub struct DateRange {
    pub from: CivilDate,
    pub to: CivilDate,
}

/// 一条已核验的交易所年度休市覆盖。**必须**带原文出处标识与摘要——没有
/// 已核验通知原文的年份不得登记。
#[derive(
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub struct OfficialCoverageEntry {
    pub exchange: CalendarExchange,
    pub year: i32,
    /// 休市区间（按通知原文；区间内全部休市，含周末）。
    pub closed_ranges: Vec<DateRange>,
    /// 原文出处标识（对 policy-sources.json 的 source id）。
    pub source_citation_id: String,
    /// 出处内容摘要（绑定通知文本，防"改日期不改出处"）。
    pub source_digest: String,
}

impl OfficialCoverageEntry {
    pub fn new(
        exchange: CalendarExchange,
        year: i32,
        closed_ranges: Vec<(CivilDate, CivilDate)>,
        source_citation_id: String,
        source_digest: String,
    ) -> Self {
        Self {
            exchange,
            year,
            closed_ranges: closed_ranges
                .into_iter()
                .map(|(from, to)| DateRange { from, to })
                .collect(),
            source_citation_id,
            source_digest,
        }
    }

    /// 区间是否覆盖某日期。
    pub fn covers(&self, date: CivilDate) -> bool {
        self.closed_ranges
            .iter()
            .any(|r| r.from <= date && date <= r.to)
    }
}

/// 模拟假日回退规则集（游戏假设而非交易所公告）。
///
/// 规则本体在 holidays.rs 固定实现；本类型承载其**版本化数据**：
/// 通知未核验年界限 + 内嵌农历事实表 + 内容 digest。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct SimulatedFallbackRuleset {
    pub version: u32,
    /// 升级前“通知原文未核验”的年份边界（晚于此年 → `SimulatedFuture`）。
    /// 2026 已升级为官方覆盖；该值继续保留，使升级前的冻结政策可按原语义恢复。
    pub notice_unverified_year: i32,
    /// 内嵌农历/节气事实表（随存档冻结）。
    pub lunar_facts: LunarYearFacts,
    pub digest: String,
}

impl SimulatedFallbackRuleset {
    /// 构造（校验 + 计算 digest）。
    pub fn new(
        version: u32,
        notice_unverified_year: i32,
        lunar_facts: LunarYearFacts,
    ) -> Result<Self, CalendarError> {
        let digest = Self::compute_digest(version, notice_unverified_year, &lunar_facts);
        Self::rehydrated(version, notice_unverified_year, lunar_facts, digest)
    }

    /// 恢复路径构造：只做形状校验，digest 以存档为准（`validate` 复核）。
    pub fn rehydrated(
        version: u32,
        notice_unverified_year: i32,
        lunar_facts: LunarYearFacts,
        digest: String,
    ) -> Result<Self, CalendarError> {
        lunar_facts.validate()?;
        Ok(Self {
            version,
            notice_unverified_year,
            lunar_facts,
            digest,
        })
    }

    /// 形状 + digest 校验。
    pub fn validate(&self) -> Result<(), CalendarError> {
        self.lunar_facts.validate()?;
        let computed =
            Self::compute_digest(self.version, self.notice_unverified_year, &self.lunar_facts);
        if computed != self.digest {
            return Err(CalendarError::DigestMismatch {
                field: "simulated_fallback",
                stored: self.digest.clone(),
                computed,
            });
        }
        Ok(())
    }

    fn compute_digest(
        version: u32,
        notice_unverified_year: i32,
        lunar_facts: &LunarYearFacts,
    ) -> String {
        fnv1a64_hex(&[
            "simulated-fallback",
            &version.to_string(),
            &notice_unverified_year.to_string(),
            lunar_facts.digest(),
        ])
    }
}

/// 2026 年沪深交易所年度休市覆盖。
///
/// 两所公告日期相同、休市安排一致，但来源标识与正文摘要分别绑定本所原文：
/// - SSE：上证公告〔2025〕45号，2025-12-22 发布；
/// - SZSE：深证会〔2025〕481号，2025-12-22 发布。
///
/// `closed_ranges` 是公告逐字列出的节假日闭市区间与“另外”周末休市日的并集。
/// 交易日解析仍先判周末，因此补班周末保持 `Weekend` 原因，不会变为交易日。
/// `source_digest` 是公告正文规范化文本的 SHA-256；取证记录与规范化方法见
/// `agents/calendar-2026-official-20261102/evidence.md`。
pub(super) fn official_2026_coverage() -> Result<Vec<OfficialCoverageEntry>, CalendarError> {
    const SSE_2026_NOTICE_SOURCE: &str = "sse-2026-holiday-notice";
    const SSE_2026_NOTICE_DIGEST: &str =
        "3ecade6151ea19fe223b747eea08212be430f69ae7a52056f1961b2087e939bc";
    const SZSE_2026_NOTICE_SOURCE: &str = "szse-2026-holiday-notice";
    const SZSE_2026_NOTICE_DIGEST: &str =
        "87ecf6071c1aa57eb6dc70327e7d763481976300079d622739b27766b789535f";

    let ranges = [
        ("2026-01-01", "2026-01-03"), // 元旦
        ("2026-01-04", "2026-01-04"), // 元旦相邻周末
        ("2026-02-14", "2026-02-14"), // 春节调休周末
        ("2026-02-15", "2026-02-23"), // 春节
        ("2026-02-28", "2026-02-28"), // 春节调休周末
        ("2026-04-04", "2026-04-06"), // 清明
        ("2026-05-01", "2026-05-05"), // 劳动节
        ("2026-05-09", "2026-05-09"), // 劳动节调休周末
        ("2026-06-19", "2026-06-21"), // 端午
        ("2026-09-20", "2026-09-20"), // 国庆调休周末
        ("2026-09-25", "2026-09-27"), // 中秋
        ("2026-10-01", "2026-10-07"), // 国庆
        ("2026-10-10", "2026-10-10"), // 国庆调休周末
    ]
    .into_iter()
    .map(|(from, to)| Ok((CivilDate::from_iso(from)?, CivilDate::from_iso(to)?)))
    .collect::<Result<Vec<_>, CalendarError>>()?;

    Ok(vec![
        OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2026,
            ranges.clone(),
            SSE_2026_NOTICE_SOURCE.to_string(),
            SSE_2026_NOTICE_DIGEST.to_string(),
        ),
        OfficialCoverageEntry::new(
            CalendarExchange::Szse,
            2026,
            ranges,
            SZSE_2026_NOTICE_SOURCE.to_string(),
            SZSE_2026_NOTICE_DIGEST.to_string(),
        ),
    ])
}
