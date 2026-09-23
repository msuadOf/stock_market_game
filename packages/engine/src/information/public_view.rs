//! 不可变公开信息库（K4，任务 15）：插入与恢复边界。
//!
//! 库的不变量（每条插入与恢复都独力维护——公开层是信任边界，不是
//! task-13 登记簿的内部冗余）：
//! - 每条报告来自结账引擎登记簿（未定稿 = `ReportNotFinalized`）且勾稽
//!   通过（`ReportSet::validate`）；
//! - 时点/排期/更正关系合法（publication.rs 校验族）；
//! - `PublicationId` 单调不复用、报告与公告共享一个计数器；
//! - 版本永不删除（无删除路径），更正 = 新条目 `supersedes` 旧 id。
//!
//! 查询面在 `queries.rs`；开局装配在 `prehistory.rs`。

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::publication::{
    ensure_announcement_timing, ensure_origin_supersedes, ensure_publication_times,
    ensure_report_shape, ensure_schedule_instant, ensure_schedule_window,
    ensure_scope_mirrors_company,
};
use crate::information::{
    Announcement, AnnouncementRequest, InformationError, PublicationId, PublicationRequest,
    PublishedReport,
};

/// 不可变公开信息库。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PublicLibrary {
    pub(in crate::information) next_seq: u32,
    pub(in crate::information) reports: BTreeMap<PublicationId, PublishedReport>,
    pub(in crate::information) announcements: BTreeMap<PublicationId, Announcement>,
    pub(in crate::information) by_company: BTreeMap<CompanyId, BTreeSet<PublicationId>>,
}

/// 存档 DTO（恢复走 [`PublicLibrary::from_parts`] 全量校验）。
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct PublicLibrarySave {
    pub next_seq: u32,
    pub reports: Vec<PublishedReport>,
    pub announcements: Vec<Announcement>,
}

impl PublicLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// 公开一条结账登记簿中的定稿版本（未定稿/不平/时间逆序/偏离排期/
    /// 更正关系错配 = 类型化拒绝）。
    pub fn publish_closed(
        &mut self,
        closing: &crate::accounting::closing::ClosingEngine,
        request: PublicationRequest,
    ) -> Result<PublicationId, InformationError> {
        ensure_origin_supersedes(&request.origin, request.supersedes)?;
        ensure_scope_mirrors_company(&request.company, &request.scope)?;
        ensure_schedule_window(&request.origin, request.period)?;
        ensure_publication_times(request.period, request.approved_at, request.published_at)?;
        ensure_schedule_instant(&request.origin, request.published_at)?;
        if let Some(target) = request.supersedes {
            let original = self
                .reports
                .get(&target)
                .ok_or(InformationError::CorrectionTargetUnknown { target })?;
            if original.company != request.company
                || original.reports.period != request.period
                || original.reports.kind != request.kind
                || original.reports.scope != request.scope
            {
                return Err(InformationError::CorrectionTargetMismatch {
                    target,
                    company: request.company.clone(),
                });
            }
        }
        let set = closing
            .version(
                &request.scope,
                request.period,
                request.kind,
                request.sequence,
            )
            .ok_or(InformationError::ReportNotFinalized {
                scope: request.scope.clone(),
                period: request.period,
                kind: request.kind,
                sequence: request.sequence,
            })?
            .clone();
        set.validate()
            .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
        let id = PublicationId::new(self.next_seq);
        let report = PublishedReport {
            id,
            company: request.company.clone(),
            policy: request.policy,
            approved_at: request.approved_at,
            published_at: request.published_at,
            origin: request.origin,
            supersedes: request.supersedes,
            reports: set,
        };
        self.insert_report(report)?;
        Ok(id)
    }

    /// 公开一条临时公告（发生日的下一个 18:00 相位；时序错配 = 拒绝）。
    pub fn publish_announcement(
        &mut self,
        request: AnnouncementRequest,
    ) -> Result<PublicationId, InformationError> {
        ensure_announcement_timing(request.occurred_on, request.published_at)?;
        let id = PublicationId::new(self.next_seq);
        let announcement = Announcement {
            id,
            company: request.company.clone(),
            occurred_on: request.occurred_on,
            published_at: request.published_at,
            event: request.event,
        };
        self.next_seq += 1;
        self.by_company
            .entry(request.company)
            .or_default()
            .insert(id);
        self.announcements.insert(id, announcement);
        Ok(id)
    }

    /// 恢复边界：全量校验（id 域一致 + 无重复、逐条形状校验、更正链
    /// 完整、计数器与最大 id 严格衔接、索引重建）。任何失败 ⇒ 库不产生。
    pub fn from_parts(save: PublicLibrarySave) -> Result<Self, InformationError> {
        let mut library = Self {
            next_seq: save.next_seq,
            ..Self::default()
        };
        for report in save.reports {
            if report.id.value() >= save.next_seq {
                return Err(InformationError::InconsistentLibrary {
                    detail: format!(
                        "report id {} is not below next_seq {}",
                        report.id.value(),
                        save.next_seq
                    ),
                });
            }
            library.insert_report(report)?;
        }
        for announcement in save.announcements {
            if announcement.id.value() >= save.next_seq {
                return Err(InformationError::InconsistentLibrary {
                    detail: format!(
                        "announcement id {} is not below next_seq {}",
                        announcement.id.value(),
                        save.next_seq
                    ),
                });
            }
            ensure_announcement_timing(announcement.occurred_on, announcement.published_at)?;
            let company = announcement.company.clone();
            let id = announcement.id;
            if library.announcements.insert(id, announcement).is_some() {
                return Err(InformationError::DuplicatePublicationId { id });
            }
            library.by_company.entry(company).or_default().insert(id);
        }
        // 计数器与已存 id 严格衔接（无删除路径 ⇒ 无空隙；幻影分配 = 不一致）。
        let expected_next = library
            .by_company
            .values()
            .flat_map(|ids| ids.iter())
            .copied()
            .max()
            .map_or(0, |max_id| max_id.value() + 1);
        if library.next_seq != expected_next {
            return Err(InformationError::InconsistentLibrary {
                detail: format!(
                    "next_seq {} does not continue after max id (expected {expected_next})",
                    library.next_seq
                ),
            });
        }
        Ok(library)
    }

    /// 导出存档 DTO（报告/公告按 id 序——BTreeMap 迭代序确定性）。
    pub fn save(&self) -> PublicLibrarySave {
        PublicLibrarySave {
            next_seq: self.next_seq,
            reports: self.reports.values().cloned().collect(),
            announcements: self.announcements.values().cloned().collect(),
        }
    }

    /// 公布（报告或公告）的发表时点；id 不存在 = None（恢复边界交叉校验面，
    /// 任务 27——不做 as_of 前视守卫，时序比较由调用方显式执行）。
    pub fn publication_instant(&self, id: PublicationId) -> Option<CivilInstant> {
        if let Some(report) = self.reports.get(&id) {
            return Some(report.published_at);
        }
        self.announcements.get(&id).map(|a| a.published_at)
    }

    /// 库内最晚发表时点（空库 = None；恢复边界校验披露游标覆盖全部公布）。
    pub fn latest_published_instant(&self) -> Option<CivilInstant> {
        self.reports
            .values()
            .map(|report| report.published_at)
            .chain(self.announcements.values().map(|a| a.published_at))
            .max()
    }

    fn insert_report(&mut self, report: PublishedReport) -> Result<(), InformationError> {
        ensure_report_shape(&report)?;
        let id = report.id;
        let company = report.company.clone();
        if self.reports.insert(id, report).is_some() {
            return Err(InformationError::DuplicatePublicationId { id });
        }
        self.by_company.entry(company).or_default().insert(id);
        self.next_seq = self.next_seq.max(id.value() + 1);
        Ok(())
    }
}

impl serde::Serialize for PublicLibrary {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.save().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PublicLibrary {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let save = PublicLibrarySave::deserialize(deserializer)?;
        Self::from_parts(save).map_err(serde::de::Error::custom)
    }
}
