//! 公开值类型与公布请求（K4，任务 15）：`PublishedReport` / `Announcement`
//! 与时点/关系校验。
//!
//! 发生 / 报告期 / 批准 / 公布四个时点显式分离：
//! - 发生（公告 `occurred_on`）/ 报告期（`reports.window`）是业务事实；
//! - 批准（`approved_at`，本游戏约定为公布日 08:00）必须**严格晚于报告期
//!   期末**——期未结束即批准 = 时间逆序；
//! - 公布（`published_at`）必须落在 18:00 披露相位、不早于批准，且排期
//!   披露必须与排期时点逐分吻合。
//!
//! 公开更正 = **新版本关联旧 ID**（`supersedes`）；历史版本永不覆写。
//! `PublishedReport` 不重复存储 scope/period/kind/version（单一真源 =
//! 内嵌的任务 13 `ReportSet`，恢复边界零一致性检查负担）。公告内容只含
//! 该时点已确认事实（事件条款）；未来合同现金流是预测，类型上就没有
//! 「已实现」标记位。

use crate::accounting::consolidation::ScopeId;
use crate::accounting::reports::{ReportKind, ReportSet};
use crate::accounting::AccountingPeriod;
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::CompanyId;
use crate::information::{
    scheduled_instant, InformationError, ScheduledReportKind, SCHEDULE_OFFSET_MAX,
};

/// 批准时点的游戏约定（公布日 08:00；排期/装配/派发统一使用）。
pub const APPROVAL_HOUR: u32 = 8;
/// 18:00 披露相位的当日秒。
pub const DISCLOSURE_PHASE_SECOND: u32 = 18 * 3600;

/// 公布 id：库内单调分配，绝不复用（`PlanId`/`DueBusinessId` 先例）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct PublicationId(u32);

impl PublicationId {
    /// 测试/恢复边界构造（生产路径由库单调分配）。
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

/// 会计政策引用（公开报告的政策基线最小承载：科目表/列报口径版本；
/// 更丰富的政策标识随任务 16/29 的消费需要再扩展）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct AccountingPolicyRef {
    /// 科目表版本（v2 工业 / v3 银行 / v4 保险 / v5 地产）。
    pub chart_version: u32,
}

/// 公布来源。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PublicationOrigin {
    /// 开局前已按真实历史排期公布的版本（前史装配注入）。
    SeededPrehistory {
        fiscal_year: i32,
        kind: ScheduledReportKind,
        offset_days: u8,
    },
    /// 开局后按排期 live 公布的版本。
    ScheduledDisclosure {
        fiscal_year: i32,
        kind: ScheduledReportKind,
        offset_days: u8,
    },
    /// 公开更正（必须且只能伴随 `supersedes` 链接）。
    Correction,
}

impl PublicationOrigin {
    /// 排期字段集（Seeded/Scheduled 两种来源共用；Correction 无排期）。
    pub fn scheduled(&self) -> Option<(i32, ScheduledReportKind, u8)> {
        match self {
            Self::SeededPrehistory {
                fiscal_year,
                kind,
                offset_days,
            }
            | Self::ScheduledDisclosure {
                fiscal_year,
                kind,
                offset_days,
            } => Some((*fiscal_year, *kind, *offset_days)),
            Self::Correction => None,
        }
    }
}

/// 已公开的定期报告（不可变值；scope/period/kind/version 由内嵌 `reports`
/// 单一承载）。库永不删除被引用版本。
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct PublishedReport {
    pub id: PublicationId,
    pub company: CompanyId,
    pub policy: AccountingPolicyRef,
    pub approved_at: CivilInstant,
    pub published_at: CivilInstant,
    pub origin: PublicationOrigin,
    /// 更正关系：新版本关联的旧公布 id（历史不可覆写）。
    pub supersedes: Option<PublicationId>,
    /// 任务 13 五产物（含 scope/period/kind/window/version）。
    pub reports: ReportSet,
}

/// 公告事件条款（该时点已确认事实；复用任务 14 事件目录类型，不镜像）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct AnnouncedEvent {
    pub kind: crate::company::ShockKind,
    pub amplitude_bp: i32,
    pub starts_on: CivilDate,
    pub expires_on: CivilDate,
}

impl AnnouncedEvent {
    /// 由已激活冲击构造（条款在激活时即确定，是已确认事实）。
    pub fn from_active(shock: &crate::company::ActiveShock) -> Self {
        Self {
            kind: shock.kind.clone(),
            amplitude_bp: shock.amplitude_bp,
            starts_on: shock.starts_on,
            expires_on: shock.expires_on,
        }
    }
}

/// 已公布临时公告（不可变值；按发生后的下一个 18:00 相位公布）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Announcement {
    pub id: PublicationId,
    pub company: CompanyId,
    pub occurred_on: CivilDate,
    pub published_at: CivilInstant,
    pub event: AnnouncedEvent,
}

/// 定期报告公布请求（显式时点——类型上无默认值，杜绝补默认日期）。
#[derive(Clone)]
pub struct PublicationRequest {
    pub company: CompanyId,
    pub scope: ScopeId,
    pub period: AccountingPeriod,
    pub kind: ReportKind,
    /// 结账登记簿版本序号（未定稿版本在登记簿查询边界被拒）。
    pub sequence: u32,
    pub policy: AccountingPolicyRef,
    pub approved_at: CivilInstant,
    pub published_at: CivilInstant,
    pub origin: PublicationOrigin,
    pub supersedes: Option<PublicationId>,
}

/// 临时公告公布请求。
#[derive(Clone)]
pub struct AnnouncementRequest {
    pub company: CompanyId,
    pub occurred_on: CivilDate,
    pub published_at: CivilInstant,
    pub event: AnnouncedEvent,
}

/// 报告期末自然日（窗口末月的最后一天）。
pub(crate) fn period_end_date(period: AccountingPeriod) -> Result<CivilDate, InformationError> {
    let (year, month) = (period.year(), period.month());
    let next_month = if month == 12 {
        CivilDate::from_ymd(year + 1, 1, 1)?
    } else {
        CivilDate::from_ymd(year, month + 1, 1)?
    };
    Ok(next_month.prev()?)
}

/// origin 与 supersedes 关系一致性（更正必须且只能带 supersedes）。
pub(crate) fn ensure_origin_supersedes(
    origin: &PublicationOrigin,
    supersedes: Option<PublicationId>,
) -> Result<(), InformationError> {
    if matches!(origin, PublicationOrigin::Correction) == supersedes.is_some() {
        Ok(())
    } else {
        Err(InformationError::OriginSupersedesMismatch)
    }
}

/// 报告期/批准/公布三时点校验（相位 → 顺序 → 期末）。
pub(crate) fn ensure_publication_times(
    period: AccountingPeriod,
    approved_at: CivilInstant,
    published_at: CivilInstant,
) -> Result<(), InformationError> {
    if published_at.second_of_day() != DISCLOSURE_PHASE_SECOND {
        return Err(InformationError::PublicationOutsidePhase { published_at });
    }
    if published_at < approved_at {
        return Err(InformationError::PublishBeforeApproval {
            approved_at,
            published_at,
        });
    }
    let period_end = period_end_date(period)?;
    if approved_at.date() <= period_end {
        return Err(InformationError::ApprovalPrecedesPeriodEnd {
            approved_at,
            period_end,
        });
    }
    Ok(())
}

/// 排期窗口一致性：落点吻合 + 偏移域内（不涉及时点；先于时间校验，
/// 非法窗口优先报窗口错而不是时间错）。
pub(crate) fn ensure_schedule_window(
    origin: &PublicationOrigin,
    period: AccountingPeriod,
) -> Result<(), InformationError> {
    let Some((fiscal_year, kind, offset_days)) = origin.scheduled() else {
        return Ok(());
    };
    if offset_days > SCHEDULE_OFFSET_MAX {
        return Err(InformationError::IllegalScheduleOffset {
            offset: offset_days,
        });
    }
    let expected_period = kind.landing_period(fiscal_year)?;
    if expected_period != period {
        return Err(InformationError::IllegalWindowPublication {
            expected: expected_period,
            actual: period,
        });
    }
    Ok(())
}

/// 排期时点一致性：公布时点与排期逐分吻合（相位/顺序/期末校验之后）。
pub(crate) fn ensure_schedule_instant(
    origin: &PublicationOrigin,
    published_at: CivilInstant,
) -> Result<(), InformationError> {
    let Some((fiscal_year, kind, offset_days)) = origin.scheduled() else {
        return Ok(());
    };
    let expected_instant = scheduled_instant(kind, fiscal_year, offset_days)?;
    if expected_instant != published_at {
        return Err(InformationError::OffSchedulePublication {
            expected: expected_instant,
            actual: published_at,
        });
    }
    Ok(())
}

/// scope 携带的 MemberId 必须镜像公司 id（单体与合并根同规则）。
pub(crate) fn ensure_scope_mirrors_company(
    company: &CompanyId,
    scope: &ScopeId,
) -> Result<(), InformationError> {
    let member = match scope {
        ScopeId::Standalone(member) | ScopeId::Consolidated(member) => member,
    };
    if member.0 != company.0 {
        return Err(InformationError::ScopeCompanyMismatch {
            company: company.clone(),
            scope: scope.clone(),
        });
    }
    Ok(())
}

/// 公告时序校验：18:00 相位 + 恰好落在发生日的下一个披露相位。
pub(crate) fn ensure_announcement_timing(
    occurred_on: CivilDate,
    published_at: CivilInstant,
) -> Result<(), InformationError> {
    if published_at.second_of_day() != DISCLOSURE_PHASE_SECOND {
        return Err(InformationError::PublicationOutsidePhase { published_at });
    }
    if published_at.date() < occurred_on {
        return Err(InformationError::AnnouncementBeforeOccurrence {
            occurred_on,
            published_at,
        });
    }
    if published_at.date() > occurred_on {
        return Err(InformationError::AnnouncementNotNextPhase {
            occurred_on,
            published_at,
        });
    }
    Ok(())
}

/// 已存报告的完整形状校验（publish 与恢复边界共用；不含 id/链接检查）。
pub(crate) fn ensure_report_shape(report: &PublishedReport) -> Result<(), InformationError> {
    ensure_origin_supersedes(&report.origin, report.supersedes)?;
    ensure_scope_mirrors_company(&report.company, &report.reports.scope)?;
    ensure_schedule_window(&report.origin, report.reports.period)?;
    ensure_publication_times(
        report.reports.period,
        report.approved_at,
        report.published_at,
    )?;
    ensure_schedule_instant(&report.origin, report.published_at)?;
    report
        .reports
        .validate()
        .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
    Ok(())
}
