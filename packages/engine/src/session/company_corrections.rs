//! 报表更正的 Session 共同命令；18:00 提交由自然日父事务负责。

use super::{CompanyReportCorrection, GameSession, ReportCorrectionStatus, StepFatal};
use std::sync::{Arc, Weak};

/// 仅用于同一内存 Session 的请求归属，不序列化为宿主 generation。
#[derive(Clone, Debug)]
pub struct ReportCorrectionEpoch(Weak<()>);

#[derive(Debug, thiserror::Error)]
pub enum ReportCorrectionError {
    #[error("{0}")]
    Step(#[from] StepFatal),
    #[error("更正请求来自已经切换的市场或存档")]
    StaleEpoch,
    #[error("更正操作 {operation_id} 已使用其他载荷：completed={completed}")]
    OperationConflict {
        operation_id: String,
        completed: bool,
    },
    #[error("待处理更正操作不存在：{operation_id}")]
    NotPending { operation_id: String },
    #[error("已经完成的更正不可撤销：{operation_id}")]
    AlreadyCompleted { operation_id: String },
    #[error("更正操作身份必须非空")]
    EmptyOperationId,
    #[error("更正理由必须非空")]
    EmptyReason,
    #[error("更正凭证必须非空")]
    EmptyEntries,
    #[error("更正凭证不满足会计不变量：{source}")]
    Journal {
        #[source]
        source: crate::accounting::AccountingError,
    },
    #[error("更正凭证日期晚于当前自然日：source={source_id:?}, date={date}, current={current}")]
    FutureEntry {
        source_id: crate::accounting::BusinessEventId,
        date: crate::CivilDate,
        current: crate::CivilDate,
    },
    #[error("更正命令无法受理：Session 外部变更序号已耗尽")]
    RevisionExhausted,
}

impl GameSession {
    pub fn report_correction_epoch(&self) -> ReportCorrectionEpoch {
        ReportCorrectionEpoch(Arc::downgrade(&self.report_correction_epoch))
    }

    pub fn report_corrections(&self) -> Result<ReportCorrectionStatus, ReportCorrectionError> {
        self.require_healthy()?;
        Ok(self.state.report_corrections.clone())
    }

    /// 供保存边界判断，不克隆队列，也不改变健康状态。
    pub(super) fn has_pending_report_corrections(&self) -> bool {
        !self.state.report_corrections.pending.is_empty()
    }

    /// 18:00 父事务按实际接收顺序准备同一候选，不提前修改公司或公开材料。
    pub(super) fn pending_report_corrections(&self) -> &[CompanyReportCorrection] {
        &self.state.report_corrections.pending
    }

    fn require_report_correction_epoch(
        &self,
        epoch: &ReportCorrectionEpoch,
    ) -> Result<(), ReportCorrectionError> {
        if Weak::ptr_eq(&epoch.0, &Arc::downgrade(&self.report_correction_epoch)) {
            Ok(())
        } else {
            Err(ReportCorrectionError::StaleEpoch)
        }
    }

    // 新鲜度序号必须在任何写入之前检查；耗尽不能回绕或留下部分 queue 变更。
    fn mark_report_correction_mutation(&self) -> Result<(), ReportCorrectionError> {
        self.revision
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |revision| revision.checked_add(1),
            )
            .map(|_| ())
            .map_err(|_| ReportCorrectionError::RevisionExhausted)
    }

    pub fn enqueue_report_correction(
        &mut self,
        epoch: &ReportCorrectionEpoch,
        request: CompanyReportCorrection,
    ) -> Result<(), ReportCorrectionError> {
        self.require_healthy()?;
        self.require_report_correction_epoch(epoch)?;
        if request.operation_id.trim().is_empty() {
            return Err(ReportCorrectionError::EmptyOperationId);
        }
        if request.reason.trim().is_empty() {
            return Err(ReportCorrectionError::EmptyReason);
        }
        if request.entries.is_empty() {
            return Err(ReportCorrectionError::EmptyEntries);
        }
        for entry in &request.entries {
            entry
                .validate_invariants()
                .map_err(|source| ReportCorrectionError::Journal { source })?;
            if entry.date > self.civil_date() {
                return Err(ReportCorrectionError::FutureEntry {
                    source_id: entry.source,
                    date: entry.date,
                    current: self.civil_date(),
                });
            }
        }
        if let Some(existing) = self
            .state
            .report_corrections
            .completed
            .get(&request.operation_id)
        {
            return if existing.request == request {
                Ok(())
            } else {
                Err(ReportCorrectionError::OperationConflict {
                    operation_id: request.operation_id,
                    completed: true,
                })
            };
        }
        if let Some(existing) = self
            .state
            .report_corrections
            .pending
            .iter()
            .find(|item| item.operation_id == request.operation_id)
        {
            return if *existing == request {
                Ok(())
            } else {
                Err(ReportCorrectionError::OperationConflict {
                    operation_id: request.operation_id,
                    completed: false,
                })
            };
        }
        self.mark_report_correction_mutation()?;
        self.state.report_corrections.pending.push(request);
        Ok(())
    }

    pub fn cancel_report_correction(
        &mut self,
        epoch: &ReportCorrectionEpoch,
        operation_id: &str,
    ) -> Result<(), ReportCorrectionError> {
        self.require_healthy()?;
        self.require_report_correction_epoch(epoch)?;
        if self
            .state
            .report_corrections
            .completed
            .contains_key(operation_id)
        {
            return Err(ReportCorrectionError::AlreadyCompleted {
                operation_id: operation_id.to_owned(),
            });
        }
        let position = self
            .state
            .report_corrections
            .pending
            .iter()
            .position(|item| item.operation_id == operation_id)
            .ok_or_else(|| ReportCorrectionError::NotPending {
                operation_id: operation_id.to_owned(),
            })?;
        self.mark_report_correction_mutation()?;
        self.state.report_corrections.pending.remove(position);
        Ok(())
    }
}

/// 完成状态只接受真实 owner 包与同次 canonical 披露候选。
#[derive(Debug, thiserror::Error)]
pub(super) enum ReportCorrectionCompletionError {
    #[error("完成更正的时点必须是当日 18:00：{actual:?}")]
    CommitTime { actual: crate::CivilInstant },
    #[error("构造更正的真实 18:00 时点失败：{source}")]
    Calendar {
        #[source]
        source: crate::calendar::CivilDateError,
    },
    #[error("完成更正的真实 base 与 Session 公开库不一致")]
    BaseMismatch,
    #[error("更正操作数量不一致：pending={pending}, prepared={prepared}")]
    OperationCount { pending: usize, prepared: usize },
    #[error("更正操作 {index} 的公司、来源或真实凭证身份不一致")]
    OperationMismatch { index: usize },
    #[error("更正操作 {operation_index} 的完整凭证与 owner 金融候选不一致：{source}")]
    CompanyFact {
        operation_index: usize,
        #[source]
        source: crate::company::CompanyCorrectionError,
    },
    #[error("更正操作 {operation_id} 已有完成事实")]
    AlreadyCompleted { operation_id: String },
    #[error("更正操作 {operation_index} 的报告 {report_index} 缺少唯一完整 canonical 结果")]
    CanonicalMismatch {
        operation_index: usize,
        report_index: usize,
    },
    #[error("更正公开报告 {publication:?} 被重复归属")]
    DuplicatePublication {
        publication: crate::information::PublicationId,
    },
    #[error("更正操作 {operation_index} 的 canonical 报告不可读：{source}")]
    Information {
        operation_index: usize,
        #[source]
        source: crate::information::InformationError,
    },
}

impl GameSession {
    /// 不安装公司或公开材料；父事务只在全部候选成功后提交返回的单一状态域。
    pub(super) fn prepare_report_correction_completion(
        &self,
        prepared: &crate::company::PreparedSimpleDisclosureCorrection<'_>,
        candidate: &super::disclosures::candidate::DisclosureCandidate,
        report: &super::CivilDayEndReport,
    ) -> Result<ReportCorrectionStatus, ReportCorrectionCompletionError> {
        use crate::information::PublicationSource;
        use std::collections::BTreeSet;
        // Monthly 的扫描上界可到日末；更正公布始终是父报告 settled_date 的 18:00。
        let committed_at = crate::CivilInstant::from_hms(report.settled_date, 18, 0, 0)
            .map_err(|source| ReportCorrectionCompletionError::Calendar { source })?;
        if report.disclosure_instant < committed_at
            || report.disclosure_instant.date() != report.settled_date
        {
            return Err(ReportCorrectionCompletionError::CommitTime {
                actual: report.disclosure_instant,
            });
        }
        let proof = prepared.proof();
        if proof.base() != self.state.library.as_ref() {
            return Err(ReportCorrectionCompletionError::BaseMismatch);
        }
        let pending = self.pending_report_corrections();
        if pending.len() != proof.operations().len() {
            return Err(ReportCorrectionCompletionError::OperationCount {
                pending: pending.len(),
                prepared: proof.operations().len(),
            });
        }
        let mut status = self.state.report_corrections.clone();
        let mut assigned = BTreeSet::new();
        for (operation_index, (request, operation)) in
            pending.iter().zip(proof.operations()).enumerate()
        {
            let mut expected_sources = request
                .entries
                .iter()
                .map(|entry| entry.source)
                .collect::<Vec<_>>();
            expected_sources.sort_unstable();
            if operation.company() != request.company
                || operation.source() != PublicationSource::SimpleGenerated
                || operation.operation_sources() != expected_sources
                || operation.reports().is_empty()
            {
                return Err(ReportCorrectionCompletionError::OperationMismatch {
                    index: operation_index,
                });
            }
            let original = proof
                .base()
                .report(request.supersedes, committed_at)
                .map_err(|source| ReportCorrectionCompletionError::Information {
                    operation_index,
                    source,
                })?;
            if !operation.reports().iter().any(|(publication, set, _)| {
                publication.company == request.company
                    && publication.supersedes == Some(request.supersedes)
                    && publication.scope == original.reports.scope
                    && publication.period == original.reports.period
                    && publication.kind == original.reports.kind
                    && matches!(&set.version.kind,
                        crate::accounting::reports::VersionKind::Correction { reason }
                            if reason == &request.reason)
            }) {
                return Err(ReportCorrectionCompletionError::OperationMismatch {
                    index: operation_index,
                });
            }
            prepared
                .source()
                .validate_report_correction_fact(
                    &request.company,
                    &request.entries,
                    original.reports.period,
                )
                .map_err(|source| ReportCorrectionCompletionError::CompanyFact {
                    operation_index,
                    source,
                })?;
            if status.completed.contains_key(&request.operation_id) {
                return Err(ReportCorrectionCompletionError::AlreadyCompleted {
                    operation_id: request.operation_id.clone(),
                });
            }
            let mut publications = Vec::with_capacity(operation.reports().len());
            for (report_index, (publication, set, source)) in operation.reports().iter().enumerate()
            {
                let mut matches = Vec::new();
                for fact in &candidate.publications {
                    if fact.company != publication.company || fact.published_at != committed_at {
                        continue;
                    }
                    let super::disclosures::candidate::DisclosurePublicationKind::Report {
                        scope,
                        period,
                        kind,
                        version,
                        supersedes,
                    } = &fact.kind
                    else {
                        continue;
                    };
                    if scope != &publication.scope
                        || *period != publication.period
                        || *kind != publication.kind
                        || version != &set.version
                        || supersedes != &publication.supersedes
                    {
                        continue;
                    }
                    let actual =
                        candidate
                            .library
                            .report(fact.id, committed_at)
                            .map_err(|source| ReportCorrectionCompletionError::Information {
                                operation_index,
                                source,
                            })?;
                    if actual.company == publication.company
                        && actual.source == *source
                        && actual.policy == publication.policy
                        && actual.approved_at == publication.approved_at
                        && actual.published_at == publication.published_at
                        && actual.origin == publication.origin
                        && actual.supersedes == publication.supersedes
                        && actual.reports == *set
                        && set.version.sequence == publication.sequence
                    {
                        matches.push(fact.id);
                    }
                }
                if matches.len() != 1 {
                    return Err(ReportCorrectionCompletionError::CanonicalMismatch {
                        operation_index,
                        report_index,
                    });
                }
                let publication = matches[0];
                if !assigned.insert(publication) {
                    return Err(ReportCorrectionCompletionError::DuplicatePublication {
                        publication,
                    });
                }
                publications.push(publication);
            }
            status.completed.insert(
                request.operation_id.clone(),
                super::CompletedReportCorrection {
                    request: request.clone(),
                    publications,
                    committed_at,
                },
            );
        }
        status.pending.clear();
        Ok(status)
    }
}
