//! 公开信息域（K4，任务 15）：定期报告、临时公告与不可变公开信息库。
//!
//! 职责边界（依赖方向：information → {accounting, calendar, company}，
//! 被 session/disclosures 与任务 16/18/29 消费）：
//! - [`schedule`]：K4 定期披露游戏排期（基准日 + 公司稳定偏移 + 18:00 相位）。
//! - [`publication`]：`PublishedReport` / `Announcement` 值类型与公布请求/
//!   时点校验（发生/报告期/批准/公布时点分离；更正 = 新版本关联旧 ID）。
//! - [`public_view`]：`PublicLibrary` 不可变公开库（插入 + 恢复边界全量
//!   校验）；查询面在 `queries`（按 id/公司/期间 + 提前读取守卫）；开局
//!   已公开集合装配在 `prehistory`（真实历史排期，`SeededPrehistory`
//!   标记；开局未来才公布的报告不提前纳入）。
//! - [`acquisition`]：`NpcInformationState` 个人获知登记（一次获知只记一次；
//!   公共曝光只改变发现机会——`discovery_candidates` 候选面，任务 25 接线）。
//! - [`npc_view`]：`NpcObservationContext` 本人已知公开信息 + 可见行情的
//!   引用面（策略不可达 CompanyState/总账；历史版本按获知时点钉死）。
//!
//! 公布是**纯 civil 域事件**：不产生市场事件/tick/RNG 消费，非交易日
//! 18:00 照常发布（K4 明文）。公开边界只收结账引擎登记簿中勾稽通过的
//! 版本（任务 13 契约）；普通查询面只见公开版本，不见未披露总账。

mod acquisition;
mod npc_view;
mod prehistory;
mod public_view;
mod publication;
mod queries;
mod schedule;

pub use acquisition::{
    discovery_candidates, AcquiredKind, AcquisitionError, AcquisitionOutcome, AcquisitionRecord,
    NpcInformationState, NpcInformationStateSave,
};
pub use npc_view::{AcquiredEntry, NpcObservationContext};
pub use prehistory::{
    assemble_seeded_prehistory, ensure_original_registered, industry_presentation, SeededPrehistory,
};
pub use public_view::{PublicLibrary, PublicLibrarySave};
pub use publication::{
    AccountingPolicyRef, AnnouncedEvent, Announcement, AnnouncementRequest, PublicationId,
    PublicationOrigin, PublicationRequest, PublishedReport, APPROVAL_HOUR, DISCLOSURE_PHASE_SECOND,
};
pub use schedule::{
    scheduled_instant, stable_company_offset, ScheduledReportKind, SCHEDULE_OFFSET_MAX,
};

use crate::accounting::closing::ClosingError;
use crate::accounting::consolidation::ScopeId;
use crate::accounting::reports::ReportError;
use crate::accounting::AccountingPeriod;
use crate::calendar::{CivilDateError, CivilInstant};
use crate::company::operations::OperationsError;
use thiserror::Error;

/// 公开信息域错误（类型化，绝不静默；大枚举装箱压缩 Err 值域——任务 12
/// `DeclaredSide` / 任务 13 `Report` 先例）。
#[derive(Debug, Error)]
pub enum InformationError {
    /// 日期算法层错误（排期加法越出 1900–2199 验证窗等）。
    #[error("civil date error: {0}")]
    Date(#[from] CivilDateError),
    /// 前史/经营装配失败（任务 14 透传，装箱）。
    #[error("operations failure: {0}")]
    Operations(Box<OperationsError>),
    /// 结账登记簿操作失败（快照/登记，装箱）。
    #[error("closing failure: {0}")]
    Closing(Box<ClosingError>),
    /// 会计期间构造失败（年月越界等）。
    #[error("accounting failure: {0}")]
    Accounting(#[from] crate::accounting::AccountingError),
    /// 报表勾稽失败（公开边界独力维护「库内一切版本勾稽通过」不变量；
    /// 恢复边界同样强制——篡改注入的不平衡版本在此显式失败）。
    #[error("report not publishable: {0}")]
    ReportNotPublishable(Box<ReportError>),

    /// 提前读取：查询时点早于公布时点（NPC/宿主不可读未公开信息）。
    #[error("early read of {id:?}: published at {published_at:?}, queried at {as_of:?}")]
    EarlyRead {
        id: PublicationId,
        published_at: CivilInstant,
        as_of: CivilInstant,
    },
    /// 查询的公布 id 不在库中。
    #[error("no publication {id:?} in the library")]
    UnknownPublication { id: PublicationId },

    /// 结账登记簿不存在该版本（期间未结账/序号越界 = 未定稿，不可公开）。
    #[error("no finalized version for {scope:?} {period} {kind:?} sequence {sequence}")]
    ReportNotFinalized {
        scope: ScopeId,
        period: AccountingPeriod,
        kind: crate::accounting::reports::ReportKind,
        sequence: u32,
    },
    /// 公布时点不在 18:00 披露相位。
    #[error("publication {published_at:?} is outside the 18:00 disclosure phase")]
    PublicationOutsidePhase { published_at: CivilInstant },
    /// 公布早于批准（时间逆序）。
    #[error("publication {published_at:?} precedes approval {approved_at:?}")]
    PublishBeforeApproval {
        approved_at: CivilInstant,
        published_at: CivilInstant,
    },
    /// 批准不严格晚于报告期末（报告期尚未结束即批准 = 时间逆序）。
    #[error("approval {approved_at:?} is not after the report period end {period_end}")]
    ApprovalPrecedesPeriodEnd {
        approved_at: CivilInstant,
        period_end: crate::calendar::CivilDate,
    },
    /// 排期引用与请求期间不一致（非法报告窗口）。
    #[error("scheduled window {expected:?} does not match the requested period {actual:?}")]
    IllegalWindowPublication {
        expected: AccountingPeriod,
        actual: AccountingPeriod,
    },
    /// 公布时点偏离排期（正确相位、错误日期）。
    #[error("publication {actual:?} deviates from the schedule {expected:?}")]
    OffSchedulePublication {
        expected: CivilInstant,
        actual: CivilInstant,
    },
    /// 公司排期偏移越界（> 7 自然日）。
    #[error("schedule offset {offset} exceeds the K4 maximum of {SCHEDULE_OFFSET_MAX}")]
    IllegalScheduleOffset { offset: u8 },
    /// 排期基准漂移导致非法窗口（K4 契约守卫：年报 ≤4-30、半年 ≤8-31、
    /// Q1 不早于上一年年报；当前常数下结构性成立，漂移时在此显式失败）。
    #[error("schedule window illegal for {kind:?} fiscal {fiscal_year}: {detail}")]
    IllegalScheduleWindow {
        kind: crate::information::ScheduledReportKind,
        fiscal_year: i32,
        detail: &'static str,
    },

    /// origin 与 supersedes 关系不一致（更正必须且只能带 supersedes）。
    #[error("publication origin and supersedes link disagree")]
    OriginSupersedesMismatch,
    /// 更正目标不在库中。
    #[error("correction target {target:?} is not in the library")]
    CorrectionTargetUnknown { target: PublicationId },
    /// 更正目标与本公司/范围/期间/种类不一致。
    #[error("correction target {target:?} does not match the corrected publication")]
    CorrectionTargetMismatch {
        target: PublicationId,
        company: crate::company::CompanyId,
    },
    /// 报表范围与公司 id 不一致（scope 携带的 MemberId 必须镜像公司 id）。
    #[error("scope {scope:?} does not mirror company {company:?}")]
    ScopeCompanyMismatch {
        company: crate::company::CompanyId,
        scope: ScopeId,
    },

    /// 公告公布早于发生日。
    #[error("announcement published {published_at:?} before occurrence {occurred_on:?}")]
    AnnouncementBeforeOccurrence {
        occurred_on: crate::calendar::CivilDate,
        published_at: CivilInstant,
    },
    /// 公告未落在发生后的下一个 18:00 相位（迟到发布）。
    #[error("announcement for {occurred_on:?} must publish at the next 18:00 phase, got {published_at:?}")]
    AnnouncementNotNextPhase {
        occurred_on: crate::calendar::CivilDate,
        published_at: CivilInstant,
    },

    /// 库内重复公布 id（恢复边界拒绝；单调计数器不得复用）。
    #[error("duplicate publication id {id:?}")]
    DuplicatePublicationId { id: PublicationId },
    /// 库状态自相矛盾（id ≥ next_seq、supersedes 断链等恢复边界失败）。
    #[error("inconsistent public library: {detail}")]
    InconsistentLibrary { detail: String },
}
