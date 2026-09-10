//! K4 定期披露排期（任务 15）：自然年会计年度的游戏排期表。
//!
//! 基准日（+偏移前）：年报 = 次年 3-20、Q1 = 4-20、半年 = 8-15、
//! Q3 = 10-20，18:00 公布；各公司稳定偏移 0–7 自然日，由 seed+公司 id
//! 纯函数派生（**从不重采样**——同 seed 同公司恒等，任务 14 rng.rs 的
//! FNV-1a+SplitMix64 终结器孪生）。法定窗口依据（任务 2 已核验）：年报
//! ≤4-30、半年报 ≤8-31（证监会令 182 号第十三条）；Q1/Q3 无法定校验来源
//! （fixture blocked），游戏排期不声称法定。
//!
//! 合法公告可在休市日发布（公布是 civil 域事件，不是市场事件）；不推迟
//! 到开市日。Q1 不早于上一年年报是排期契约守卫（当前基准下结构性成立，
//! 基准漂移时显式失败——铁律二优先于静默容忍）。

use crate::accounting::AccountingPeriod;
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::CompanyId;
use crate::information::InformationError;

/// 公司排期偏移上界（自然日）。
pub const SCHEDULE_OFFSET_MAX: u8 = 7;
/// 披露相位的当日秒（18:00:00）。
const PHASE_SECOND: u32 = 18 * 3600;

/// 定期披露种类（游戏排期表目；报表种类映射见 [`Self::report_kind`]）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum ScheduledReportKind {
    /// 年报（窗口 = 会计年度 1..=12 月；**次年** 3-20 基准公布）。
    Annual,
    /// 一季报（窗口 = 1..=3 月；同年 4-20 基准）。
    Q1,
    /// 半年报（窗口 = 1..=6 月；同年 8-15 基准）。
    HalfYear,
    /// 三季报（窗口 = 7..=9 月；同年 10-20 基准）。
    Q3,
}

impl ScheduledReportKind {
    /// 全部排期表目（确定序：派发/装配枚举用）。
    pub const ALL: [Self; 4] = [Self::Q1, Self::HalfYear, Self::Q3, Self::Annual];

    /// 排期表目 → 报表种类（任务 13 的窗口形状语义）。
    pub fn report_kind(self) -> crate::accounting::reports::ReportKind {
        use crate::accounting::reports::ReportKind;
        match self {
            Self::Annual => ReportKind::Annual,
            Self::Q1 | Self::Q3 => ReportKind::Quarter,
            Self::HalfYear => ReportKind::HalfYear,
        }
    }

    /// 报告期落点（窗口末月）：年报 = 12 月、Q1 = 3 月、半年 = 6 月、
    /// Q3 = 9 月（半年/年报的落月合法性由 `ReportKind::resolve` 兜底）。
    pub fn landing_period(self, fiscal_year: i32) -> Result<AccountingPeriod, InformationError> {
        let month = match self {
            Self::Annual => 12,
            Self::Q1 => 3,
            Self::HalfYear => 6,
            Self::Q3 => 9,
        };
        Ok(AccountingPeriod::from_ymd(fiscal_year, month)?)
    }

    /// 公布所在自然年（年报滚入次年；其余同年）。
    fn publication_year(self, fiscal_year: i32) -> i32 {
        match self {
            Self::Annual => fiscal_year + 1,
            _ => fiscal_year,
        }
    }

    /// 基准月/日（+偏移前）。
    const fn base_month_day(self) -> (u8, u8) {
        match self {
            Self::Annual => (3, 20),
            Self::Q1 => (4, 20),
            Self::HalfYear => (8, 15),
            Self::Q3 => (10, 20),
        }
    }
}

/// 公司稳定排期偏移（0..=7 自然日）：seed + 公司 id 的纯函数派生。
///
/// FNV-1a(流标签 + 公司 id) 与 seed 混合后过一次 SplitMix64 终结器
/// （`company::rng::OperatingRng::derive` 同算法孪生；此处无状态、
/// 每次调用重 derive，从不推进、从不重采样）。
pub fn stable_company_offset(seed: u64, company: &CompanyId) -> u8 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in b"publication-schedule-offset"
        .iter()
        .chain(company.0.as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut z = seed ^ hash;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let finalized = z ^ (z >> 31);
    (finalized % u64::from(SCHEDULE_OFFSET_MAX + 1)) as u8
}

/// 排期公布时点：基准日 + 偏移个自然日，18:00 相位。
///
/// 校验（失败 = 类型化拒绝，绝不钳位）：偏移域；K4 契约守卫（年报 ≤
/// 次年 4-30、半年 ≤ 8-31、Q1 晚于上一年年报——当前基准下结构性成立，
/// 基准表漂移时在此显式失败）。非交易日公布合法（civil 域语义）。
pub fn scheduled_instant(
    kind: ScheduledReportKind,
    fiscal_year: i32,
    offset: u8,
) -> Result<CivilInstant, InformationError> {
    if offset > SCHEDULE_OFFSET_MAX {
        return Err(InformationError::IllegalScheduleOffset { offset });
    }
    let (month, day) = kind.base_month_day();
    let mut date = CivilDate::from_ymd(kind.publication_year(fiscal_year), month, day)?;
    for _ in 0..offset {
        date = date.next()?;
    }
    // K4 契约守卫：法定窗口（任务 2 核验）+ Q1 排期先于上一年年报。
    match kind {
        ScheduledReportKind::Annual => {
            let deadline = CivilDate::from_ymd(fiscal_year + 1, 4, 30)?;
            if date > deadline {
                return Err(InformationError::IllegalScheduleWindow {
                    kind,
                    fiscal_year,
                    detail: "annual publication would exceed the statutory Apr-30 window",
                });
            }
        }
        ScheduledReportKind::HalfYear => {
            let deadline = CivilDate::from_ymd(fiscal_year, 8, 31)?;
            if date > deadline {
                return Err(InformationError::IllegalScheduleWindow {
                    kind,
                    fiscal_year,
                    detail: "half-year publication would exceed the statutory Aug-31 window",
                });
            }
        }
        ScheduledReportKind::Q1 => {
            let prior_annual =
                scheduled_instant(ScheduledReportKind::Annual, fiscal_year - 1, offset)?;
            let q1 = CivilInstant::from_hms(date, 18, 0, 0)?;
            if q1 <= prior_annual {
                return Err(InformationError::IllegalScheduleWindow {
                    kind,
                    fiscal_year,
                    detail: "Q1 publication must not precede the prior-year annual report",
                });
            }
        }
        ScheduledReportKind::Q3 => {}
    }
    Ok(CivilInstant::from_hms(date, PHASE_SECOND / 3600, 0, 0)?)
}
