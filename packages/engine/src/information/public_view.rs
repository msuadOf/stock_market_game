//! 不可变公开信息库：插入与恢复边界。
//!
//! 库的不变量（每条插入与恢复都独力维护——公开层是信任边界，不是
//! 结账登记簿的内部冗余）：
//! - 每条报告来自结账引擎登记簿（未定稿 = `ReportNotFinalized`）且勾稽
//!   通过（`ReportSet::validate`）；
//! - 时点/排期/更正关系合法（publication.rs 校验族）；
//! - `PublicationId` 单调不复用、报告与公告共享一个计数器；
//! - 版本永不删除（无删除路径），更正 = 新条目 `supersedes` 旧 id。
//!
//! 查询面在 `queries.rs`；开局装配在 `prehistory.rs`。

use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::Books;
use crate::accounting::closing::{
    ClosingEngine, CorrectionRequest, CorrectionTransactionError, ReportHandle,
};
use crate::accounting::consolidation::MemberId;
use crate::accounting::reports::IndustryPresentation;
use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::publication::{
    ensure_announcement_timing, ensure_origin_supersedes, ensure_publication_times,
    ensure_report_shape, ensure_schedule_instant, ensure_schedule_window,
    ensure_scope_mirrors_company,
};
use crate::information::{
    Announcement, AnnouncementContent, AnnouncementRequest, InformationError, PublicationId,
    PublicationRequest, PublishedReport,
};

/// 不可变公开信息库。
#[derive(Clone, PartialEq, Debug)]
pub struct PublicLibrary {
    pub(in crate::information) next_seq: u32,
    pub(in crate::information) reports: BTreeMap<PublicationId, PublishedReport>,
    pub(in crate::information) announcements: BTreeMap<PublicationId, Announcement>,
    pub(in crate::information) by_company: BTreeMap<CompanyId, BTreeSet<PublicationId>>,
    content_digest: u64,
}

fn validate_announcement_content(
    content: &AnnouncementContent,
    company: &CompanyId,
    occurred_on: crate::calendar::CivilDate,
) -> Result<(), InformationError> {
    match content {
        AnnouncementContent::Shock(event) => event.validate_payment_failure(occurred_on),
        AnnouncementContent::CashDividend(dividend) => {
            dividend
                .plan
                .validate()
                .map_err(|error| InformationError::InconsistentLibrary {
                    detail: format!("invalid cash dividend announcement: {error}"),
                })?;
            if &dividend.plan.issuer != company
                || dividend.plan.announced_on != occurred_on
                || dividend.total_gross.cents() <= 0
                || dividend.total_gross > dividend.plan.distributable_amount
            {
                return Err(InformationError::InconsistentLibrary {
                    detail: format!(
                        "cash dividend announcement does not match issuer/date/authorized amount on {occurred_on}"
                    ),
                });
            }
            Ok(())
        }
        AnnouncementContent::RightsOffering(rights) => {
            rights
                .plan
                .validate()
                .map_err(|error| InformationError::InconsistentLibrary {
                    detail: format!("invalid rights offering announcement: {error}"),
                })?;
            if &rights.plan.issuer != company || rights.plan.announced_on != occurred_on {
                return Err(InformationError::InconsistentLibrary {
                    detail: format!(
                        "rights offering announcement does not match issuer/date on {occurred_on}"
                    ),
                });
            }
            Ok(())
        }
        AnnouncementContent::IssuerRepurchase(repurchase) => {
            repurchase
                .plan
                .validate()
                .map_err(|error| InformationError::InconsistentLibrary {
                    detail: format!("invalid issuer repurchase announcement: {error}"),
                })?;
            if &repurchase.plan.issuer != company || repurchase.plan.announced_on != occurred_on {
                return Err(InformationError::InconsistentLibrary {
                    detail: format!(
                        "issuer repurchase announcement does not match issuer/date on {occurred_on}"
                    ),
                });
            }
            Ok(())
        }
    }
}

const CONTENT_DIGEST_SEED: u64 = 0xcbf29ce484222325;
const CONTENT_DIGEST_PRIME: u64 = 0x100000001b3;

impl Default for PublicLibrary {
    fn default() -> Self {
        Self {
            next_seq: 0,
            reports: BTreeMap::new(),
            announcements: BTreeMap::new(),
            by_company: BTreeMap::new(),
            content_digest: CONTENT_DIGEST_SEED,
        }
    }
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
        self.publish_closed_with_source(
            closing,
            request,
            super::PublicationSource::SimulationAccounting,
        )
    }

    pub fn publish_simple_closed(
        &mut self,
        closing: &crate::accounting::closing::ClosingEngine,
        request: PublicationRequest,
    ) -> Result<PublicationId, InformationError> {
        self.publish_closed_with_source(closing, request, super::PublicationSource::SimpleGenerated)
    }

    fn publish_closed_with_source(
        &mut self,
        closing: &crate::accounting::closing::ClosingEngine,
        request: PublicationRequest,
        source: super::PublicationSource,
    ) -> Result<PublicationId, InformationError> {
        ensure_origin_supersedes(&request.origin, request.supersedes)?;
        ensure_scope_mirrors_company(&request.company, &request.scope)?;
        ensure_schedule_window(&request.origin, request.period)?;
        ensure_publication_times(
            request.period,
            request.approved_at,
            request.published_at,
            &request.origin,
        )?;
        ensure_schedule_instant(&request.origin, request.published_at, request.period)?;
        if let Some(target) = request.supersedes {
            let original = self
                .reports
                .get(&target)
                .ok_or(InformationError::CorrectionTargetUnknown { target })?;
            if original.company != request.company
                || original.source != source
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
            source,
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

    /// 原子完成调整凭证过账、重述报表生成和公开更正发布。
    pub fn correct_and_publish(
        &mut self,
        closing: &mut ClosingEngine,
        books: &mut Books,
        member: &MemberId,
        industry: IndustryPresentation,
        correction: CorrectionRequest,
        publication: PublicationRequest,
    ) -> Result<(ReportHandle, PublicationId), super::CorrectionPublicationError> {
        self.correct_and_publish_with_periods(
            closing,
            books,
            member,
            industry,
            correction,
            publication,
            &BTreeMap::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn correct_and_publish_with_periods(
        &mut self,
        closing: &mut ClosingEngine,
        books: &mut Books,
        member: &MemberId,
        industry: IndustryPresentation,
        correction: CorrectionRequest,
        publication: PublicationRequest,
        effective_periods: &BTreeMap<
            crate::accounting::BusinessEventId,
            crate::accounting::AccountingPeriod,
        >,
    ) -> Result<(ReportHandle, PublicationId), super::CorrectionPublicationError> {
        let reason = correction.reason.clone();
        let target = (publication.period, publication.kind);
        let published_snapshot = if matches!(
            publication.kind,
            crate::accounting::reports::ReportKind::Quarter
                | crate::accounting::reports::ReportKind::HalfYear
                | crate::accounting::reports::ReportKind::Annual
        ) {
            let validate_snapshot = || -> Result<(), InformationError> {
                ensure_origin_supersedes(&publication.origin, publication.supersedes)?;
                ensure_scope_mirrors_company(&publication.company, &publication.scope)?;
                ensure_publication_times(
                    publication.period,
                    publication.approved_at,
                    publication.published_at,
                    &publication.origin,
                )?;
                let id = publication
                    .supersedes
                    .ok_or(InformationError::OriginSupersedesMismatch)?;
                let original = self
                    .reports
                    .get(&id)
                    .ok_or(InformationError::CorrectionTargetUnknown { target: id })?;
                if original.company != publication.company
                    || original.reports.scope != publication.scope
                    || original.reports.period != publication.period
                    || original.reports.kind != publication.kind
                {
                    return Err(InformationError::CorrectionTargetMismatch {
                        target: id,
                        company: publication.company.clone(),
                    });
                }
                if original.published_at > publication.published_at {
                    return Err(InformationError::InconsistentLibrary {
                        detail: "更正公布时点早于被更正的公开快照".into(),
                    });
                }
                Ok(())
            };
            validate_snapshot().map_err(|cause| {
                super::CorrectionPublicationError::Information {
                    reason: reason.clone(),
                    cause: Box::new(cause),
                }
            })?;
            true
        } else {
            false
        };
        let prepared = closing.correct_with_periods_and_publication(
            books,
            member,
            industry,
            target,
            correction,
            effective_periods,
            published_snapshot,
            |set| self.prepare_correction_publication(set, publication),
        );
        match prepared {
            Ok((handle, pending)) => {
                let publication_id = self.commit_prepared_report(pending);
                Ok((handle, publication_id))
            }
            Err(CorrectionTransactionError::Closing(cause)) => {
                Err(super::CorrectionPublicationError::Closing {
                    reason,
                    cause: Box::new(cause),
                })
            }
            Err(CorrectionTransactionError::Accounting(cause)) => {
                Err(super::CorrectionPublicationError::Accounting {
                    reason,
                    cause: Box::new(cause),
                })
            }
            Err(CorrectionTransactionError::Publication(cause)) => {
                Err(super::CorrectionPublicationError::Information {
                    reason,
                    cause: Box::new(cause),
                })
            }
        }
    }

    fn prepare_correction_publication(
        &self,
        set: &crate::accounting::reports::ReportSet,
        request: PublicationRequest,
    ) -> Result<PreparedReportPublication, InformationError> {
        ensure_origin_supersedes(&request.origin, request.supersedes)?;
        ensure_scope_mirrors_company(&request.company, &request.scope)?;
        ensure_schedule_window(&request.origin, request.period)?;
        ensure_publication_times(
            request.period,
            request.approved_at,
            request.published_at,
            &request.origin,
        )?;
        ensure_schedule_instant(&request.origin, request.published_at, request.period)?;
        let target = request
            .supersedes
            .ok_or(InformationError::OriginSupersedesMismatch)?;
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
                company: request.company,
            });
        }
        if original.published_at > request.published_at {
            return Err(InformationError::InconsistentLibrary {
                detail: format!(
                    "更正公布 {:?} 早于原公开报告 {target:?} 的 {:?}",
                    request.published_at, original.published_at
                ),
            });
        }
        if set.scope != request.scope || set.period != request.period || set.kind != request.kind {
            return Err(InformationError::ReportNotFinalized {
                scope: request.scope,
                period: request.period,
                kind: request.kind,
                sequence: set.version.sequence,
            });
        }
        if set.version.sequence != request.sequence {
            return Err(InformationError::ReportNotFinalized {
                scope: request.scope,
                period: request.period,
                kind: request.kind,
                sequence: request.sequence,
            });
        }
        set.validate()
            .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
        let id = PublicationId::new(self.next_seq);
        let next_seq =
            self.next_seq
                .checked_add(1)
                .ok_or_else(|| InformationError::InconsistentLibrary {
                    detail: format!("publication id space exhausted at {id:?}"),
                })?;
        let report = PublishedReport {
            id,
            company: request.company,
            source: original.source,
            policy: request.policy,
            approved_at: request.approved_at,
            published_at: request.published_at,
            origin: request.origin,
            supersedes: request.supersedes,
            reports: set.clone(),
        };
        ensure_report_shape(&report)?;
        self.validate_correction_target(&report)?;
        let digest = extend_content_digest(self.content_digest, b'R', id, &report)?;
        Ok(PreparedReportPublication {
            report,
            next_seq,
            digest,
        })
    }

    fn commit_prepared_report(&mut self, prepared: PreparedReportPublication) -> PublicationId {
        let id = prepared.report.id;
        let company = prepared.report.company.clone();
        self.reports.insert(id, prepared.report);
        self.by_company.entry(company).or_default().insert(id);
        self.next_seq = prepared.next_seq;
        self.content_digest = prepared.digest;
        id
    }

    /// 公开一条临时公告（发生日的下一个 18:00 相位；时序错配 = 拒绝）。
    pub fn publish_announcement(
        &mut self,
        request: AnnouncementRequest,
    ) -> Result<PublicationId, InformationError> {
        ensure_announcement_timing(request.occurred_on, request.published_at)?;
        validate_announcement_content(&request.content, &request.company, request.occurred_on)?;
        let id = PublicationId::new(self.next_seq);
        let announcement = Announcement {
            id,
            company: request.company.clone(),
            occurred_on: request.occurred_on,
            published_at: request.published_at,
            content: request.content,
        };
        let content_digest = extend_content_digest(self.content_digest, b'A', id, &announcement)?;
        self.next_seq += 1;
        self.by_company
            .entry(request.company)
            .or_default()
            .insert(id);
        self.announcements.insert(id, announcement);
        self.content_digest = content_digest;
        Ok(id)
    }

    /// 恢复边界：全量校验（id 域一致 + 无重复、逐条形状校验、更正链
    /// 完整、计数器与最大 id 严格衔接、索引重建）。任何失败 ⇒ 库不产生。
    pub fn from_parts(mut save: PublicLibrarySave) -> Result<Self, InformationError> {
        let mut library = Self {
            next_seq: save.next_seq,
            ..Self::default()
        };
        save.reports.sort_by_key(|report| report.id);
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
            validate_announcement_content(
                &announcement.content,
                &announcement.company,
                announcement.occurred_on,
            )?;
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
        library.content_digest = library.recompute_content_digest()?;
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

    /// Constant-size canonical projection for diagnostic state hashes.
    ///
    /// Publications are immutable and globally sequenced, so insertion updates the digest once;
    /// restore recomputes it from the canonical id order. This avoids serializing the full report
    /// corpus several times on every market tick while preserving content sensitivity.
    pub(crate) fn hash_projection(&self) -> (u32, usize, usize, u64) {
        (
            self.next_seq,
            self.reports.len(),
            self.announcements.len(),
            self.content_digest,
        )
    }

    /// 公布（报告或公告）的发表时点；id 不存在 = None（恢复边界交叉校验面，
    /// 存档恢复校验不做 as_of 前视守卫，时序比较由调用方显式执行）。
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
        self.validate_correction_target(&report)?;
        let id = report.id;
        let company = report.company.clone();
        if self.reports.contains_key(&id) || self.announcements.contains_key(&id) {
            return Err(InformationError::DuplicatePublicationId { id });
        }
        let following =
            id.value()
                .checked_add(1)
                .ok_or_else(|| InformationError::InconsistentLibrary {
                    detail: "公开材料 ID 空间耗尽".into(),
                })?;
        let content_digest = if id.value() == self.next_seq {
            extend_content_digest(self.content_digest, b'R', id, &report)?
        } else {
            self.recompute_content_digest_with_report(&report)?
        };
        self.reports.insert(id, report);
        self.by_company.entry(company).or_default().insert(id);
        self.next_seq = self.next_seq.max(following);
        self.content_digest = content_digest;
        Ok(())
    }

    fn validate_correction_target(&self, report: &PublishedReport) -> Result<(), InformationError> {
        let Some(target) = report.supersedes else {
            return Ok(());
        };
        let original = self
            .reports
            .get(&target)
            .ok_or(InformationError::CorrectionTargetUnknown { target })?;
        if original.company != report.company
            || original.source != report.source
            || original.reports.scope != report.reports.scope
            || original.reports.period != report.reports.period
            || original.reports.kind != report.reports.kind
            || target >= report.id
            || original.reports.version.sequence >= report.reports.version.sequence
            || !matches!(&report.reports.version.kind, crate::accounting::reports::VersionKind::Correction { reason } if !reason.trim().is_empty())
            || !report.reports.version.supersedes.is_some_and(|prior| {
                prior >= original.reports.version.sequence
                    && prior < report.reports.version.sequence
            })
        {
            return Err(InformationError::CorrectionTargetMismatch {
                target,
                company: report.company.clone(),
            });
        }
        if original.published_at > report.published_at {
            return Err(InformationError::CorrectionPrecedesOriginal {
                original: target,
                original_at: original.published_at,
                published_at: report.published_at,
            });
        }
        Ok(())
    }

    fn recompute_content_digest(&self) -> Result<u64, InformationError> {
        self.content_digest_including(None)
    }

    fn recompute_content_digest_with_report(
        &self,
        report: &PublishedReport,
    ) -> Result<u64, InformationError> {
        self.content_digest_including(Some(report))
    }

    fn content_digest_including(
        &self,
        additional: Option<&PublishedReport>,
    ) -> Result<u64, InformationError> {
        let mut publications = self
            .reports
            .keys()
            .copied()
            .map(|id| (id, b'R'))
            .chain(self.announcements.keys().copied().map(|id| (id, b'A')))
            .chain(additional.map(|report| (report.id, b'R')))
            .collect::<Vec<_>>();
        publications.sort_unstable();
        publications
            .into_iter()
            .try_fold(CONTENT_DIGEST_SEED, |digest, (id, kind)| match kind {
                b'R' => {
                    let report = if additional.is_some_and(|report| report.id == id) {
                        additional.expect("额外报告 ID 已验证")
                    } else {
                        &self.reports[&id]
                    };
                    extend_content_digest(digest, kind, id, report)
                }
                b'A' => extend_content_digest(digest, kind, id, &self.announcements[&id]),
                _ => unreachable!("publication digest kind is closed"),
            })
    }
}

struct PreparedReportPublication {
    report: PublishedReport,
    next_seq: u32,
    digest: u64,
}

fn extend_content_digest(
    mut digest: u64,
    kind: u8,
    id: PublicationId,
    publication: &impl serde::Serialize,
) -> Result<u64, InformationError> {
    let bytes =
        serde_json::to_vec(publication).map_err(|error| InformationError::InconsistentLibrary {
            detail: format!("publication {id:?} cannot be hashed: {error}"),
        })?;
    let length =
        u64::try_from(bytes.len()).map_err(|error| InformationError::InconsistentLibrary {
            detail: format!("publication {id:?} hash length is invalid: {error}"),
        })?;
    for byte in [kind]
        .into_iter()
        .chain(id.value().to_le_bytes())
        .chain(length.to_le_bytes())
        .chain(bytes)
    {
        digest = (digest ^ u64::from(byte)).wrapping_mul(CONTENT_DIGEST_PRIME);
    }
    Ok(digest)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{CivilDate, CivilInstant};
    use crate::company::ShockKind;
    use crate::information::AnnouncedEvent;

    fn announcement(amplitude_bp: i32) -> AnnouncementRequest {
        let occurred_on = CivilDate::from_iso("2030-04-20").unwrap();
        AnnouncementRequest {
            company: CompanyId("digest-fixture".to_owned()),
            occurred_on,
            published_at: CivilInstant::from_hms(occurred_on, 18, 0, 0).unwrap(),
            content: AnnouncementContent::Shock(AnnouncedEvent {
                kind: ShockKind::CreditDeterioration,
                amplitude_bp,
                starts_on: occurred_on,
                expires_on: occurred_on,
            }),
        }
    }

    #[test]
    fn hash_projection_is_content_sensitive_and_restore_stable() {
        let mut first = PublicLibrary::new();
        let empty = first.hash_projection();
        first.publish_announcement(announcement(500)).unwrap();
        assert_ne!(first.hash_projection(), empty);

        let restored: PublicLibrary =
            serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
        assert_eq!(restored.hash_projection(), first.hash_projection());
        assert_eq!(restored, first);

        let mut different = PublicLibrary::new();
        different.publish_announcement(announcement(501)).unwrap();
        assert_eq!(different.next_seq, first.next_seq);
        assert_eq!(different.announcements.len(), first.announcements.len());
        assert_ne!(different.hash_projection(), first.hash_projection());
    }

    #[test]
    fn cash_dividend_announcement_is_typed_validated_and_restore_stable() {
        let announced_on = CivilDate::from_iso("2030-04-20").unwrap();
        let mut plan = crate::company::cash_dividend::CashDividendPlan {
            plan_id: "dividend-1".into(),
            issuer: CompanyId("issuer-1".into()),
            stock: crate::account::StockCode("600001".into()),
            exchange: crate::calendar::CalendarExchange::Sse,
            formula: crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
            approved_on: CivilDate::from_iso("2030-04-01").unwrap(),
            announced_on,
            registered_on: announced_on.next().unwrap(),
            ex_dividend_on: CivilDate::from_iso("2030-04-22").unwrap(),
            payable_on: CivilDate::from_iso("2030-04-23").unwrap(),
            gross_per_share: crate::money::Money::from_cents(10),
            distributable_amount: crate::money::Money::from_cents(1000),
        };
        let request =
            |company: CompanyId, plan: crate::company::cash_dividend::CashDividendPlan| {
                AnnouncementRequest {
                    company,
                    occurred_on: announced_on,
                    published_at: CivilInstant::from_hms(announced_on, 18, 0, 0).unwrap(),
                    content: AnnouncementContent::CashDividend(
                        crate::information::CashDividendAnnouncement {
                            plan,
                            total_gross: crate::money::Money::from_cents(100),
                        },
                    ),
                }
            };
        let mut library = PublicLibrary::new();
        let invalid_company = request(CompanyId("other-issuer".into()), plan.clone());
        let before = library.clone();
        assert!(library.publish_announcement(invalid_company).is_err());
        assert_eq!(library, before);
        let valid_plan = plan.clone();
        plan.announced_on = announced_on.next().unwrap();
        assert!(
            library
                .publish_announcement(request(CompanyId("issuer-1".into()), plan))
                .is_err()
        );
        assert_eq!(library, before);
        library
            .publish_announcement(request(CompanyId("issuer-1".into()), valid_plan))
            .unwrap();
        let restored: PublicLibrary =
            serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
        assert_eq!(restored, library);
    }
}
