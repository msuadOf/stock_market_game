//! 恢复已完成更正的最少事实；只校验历史关联，不重放或重算历史报表。

use super::company_state::SessionCompanies;
use super::CompletedReportCorrection;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::VersionKind;
use crate::accounting::BusinessEventId;
use crate::information::{PublicLibrary, PublicationId, PublicationOrigin, PublicationSource};
use std::collections::{BTreeMap, BTreeSet};

/// 区分坏档的具体关联，不以 live 事务的可恢复性替代导入校验。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportCorrectionRestoreViolation {
    IdentityMismatch,
    IncompleteFact,
    CommitTime,
    UnsupportedModel,
    OriginalMismatch,
    Journal { entry_index: usize },
    DuplicateEntrySource { source: BusinessEventId },
    EntryAfterCommit { entry_index: usize },
    CompanyFact,
    PublicationRead { publication: PublicationId },
    DuplicatePublication { publication: PublicationId },
    PublicationMismatch { publication: PublicationId },
    SourcePolicyMismatch { publication: PublicationId },
    ReportVersionMismatch { publication: PublicationId },
    MissingMainLink,
}

/// 外层不再套 Box<T>；原 cause 通过 Box<dyn Error> 保留可 downcast 的真实节点。
#[derive(Debug, thiserror::Error)]
#[error("报表更正恢复校验失败：operation_id={operation_id}，violation={violation:?}")]
pub struct ReportCorrectionRestoreError {
    operation_id: String,
    violation: ReportCorrectionRestoreViolation,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl ReportCorrectionRestoreError {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn violation(&self) -> &ReportCorrectionRestoreViolation {
        &self.violation
    }

    fn new(operation_id: &str, violation: ReportCorrectionRestoreViolation) -> Self {
        Self {
            operation_id: operation_id.to_owned(),
            violation,
            source: None,
        }
    }

    fn with_source(
        operation_id: &str,
        violation: ReportCorrectionRestoreViolation,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            operation_id: operation_id.to_owned(),
            violation,
            source: Some(Box::new(source)),
        }
    }
}

pub(super) fn validate_completed(
    completed: &BTreeMap<String, CompletedReportCorrection>,
    companies: &SessionCompanies,
    library: &PublicLibrary,
    current_date: crate::CivilDate,
) -> Result<(), ReportCorrectionRestoreError> {
    use ReportCorrectionRestoreError as Fault;
    use ReportCorrectionRestoreViolation as Violation;
    // Journal source 属于公司自己的 Books，只有同公司跨操作复用才冲突。
    let mut seen_entry_sources = BTreeSet::new();
    let mut seen_publications = BTreeSet::new();
    for (identity, item) in completed {
        let request = &item.request;
        if identity != &request.operation_id {
            return Err(Fault::new(identity, Violation::IdentityMismatch));
        }
        if identity.trim().is_empty()
            || request.reason.trim().is_empty()
            || request.entries.is_empty()
            || item.publications.is_empty()
        {
            return Err(Fault::new(identity, Violation::IncompleteFact));
        }
        if item.committed_at.second_of_day() != 18 * 3_600
            || item.committed_at.date() >= current_date
        {
            return Err(Fault::new(identity, Violation::CommitTime));
        }
        let system = companies
            .company_system()
            .ok_or_else(|| Fault::new(identity, Violation::UnsupportedModel))?;
        let original = library
            .report(request.supersedes, item.committed_at)
            .map_err(|source| {
                Fault::with_source(
                    identity,
                    Violation::PublicationRead {
                        publication: request.supersedes,
                    },
                    source,
                )
            })?;
        let scope = ScopeId::Standalone(MemberId(request.company.0));
        if original.company != request.company
            || original.source != PublicationSource::SimpleGenerated
            || original.reports.scope != scope
        {
            return Err(Fault::new(identity, Violation::OriginalMismatch));
        }
        let policy = system
            .accounting_policy_for(&request.company)
            .map_err(|source| {
                Fault::with_source(
                    identity,
                    Violation::SourcePolicyMismatch {
                        publication: original.id,
                    },
                    source,
                )
            })?;
        let closing = system.closing_for(&request.company).map_err(|source| {
            Fault::with_source(
                identity,
                Violation::ReportVersionMismatch {
                    publication: original.id,
                },
                source,
            )
        })?;
        if original.policy != policy {
            return Err(Fault::new(
                identity,
                Violation::SourcePolicyMismatch {
                    publication: original.id,
                },
            ));
        }
        if closing.version(
            &scope,
            original.reports.period,
            original.reports.kind,
            original.reports.version.sequence,
        ) != Some(&original.reports)
        {
            return Err(Fault::new(
                identity,
                Violation::ReportVersionMismatch {
                    publication: original.id,
                },
            ));
        }
        for (entry_index, entry) in request.entries.iter().enumerate() {
            entry.validate_invariants().map_err(|source| {
                Fault::with_source(identity, Violation::Journal { entry_index }, source)
            })?;
            if !seen_entry_sources.insert((request.company, entry.source)) {
                return Err(Fault::new(
                    identity,
                    Violation::DuplicateEntrySource {
                        source: entry.source,
                    },
                ));
            }
            if entry.date > item.committed_at.date() {
                return Err(Fault::new(
                    identity,
                    Violation::EntryAfterCommit { entry_index },
                ));
            }
        }
        // 此共同查询比较真实 Journal 的完整载荷、有效期间及税务／结账重述关系。
        system
            .validate_report_correction_fact(
                &request.company,
                &request.entries,
                original.reports.period,
            )
            .map_err(|source| Fault::with_source(identity, Violation::CompanyFact, source))?;
        let mut has_main_link = false;
        for publication in &item.publications {
            if !seen_publications.insert(*publication) {
                return Err(Fault::new(
                    identity,
                    Violation::DuplicatePublication {
                        publication: *publication,
                    },
                ));
            }
            let report = library
                .report(*publication, item.committed_at)
                .map_err(|source| {
                    Fault::with_source(
                        identity,
                        Violation::PublicationRead {
                            publication: *publication,
                        },
                        source,
                    )
                })?;
            if report.company != request.company
                || report.source != original.source
                || report.reports.scope != scope
                || report.reports.period < original.reports.period
                || report.published_at != item.committed_at
                || report.origin != PublicationOrigin::Correction
                || !matches!(&report.reports.version.kind, VersionKind::Correction { reason } if reason == &request.reason)
            {
                return Err(Fault::new(
                    identity,
                    Violation::PublicationMismatch {
                        publication: *publication,
                    },
                ));
            }
            if report.policy != policy {
                return Err(Fault::new(
                    identity,
                    Violation::SourcePolicyMismatch {
                        publication: *publication,
                    },
                ));
            }
            // 合法后续更正不会覆盖旧 version；不能用 current latest 验历史完成记录。
            if closing.version(
                &scope,
                report.reports.period,
                report.reports.kind,
                report.reports.version.sequence,
            ) != Some(&report.reports)
            {
                return Err(Fault::new(
                    identity,
                    Violation::ReportVersionMismatch {
                        publication: *publication,
                    },
                ));
            }
            if report.reports.period == original.reports.period
                && report.reports.kind == original.reports.kind
                && report.supersedes == Some(request.supersedes)
            {
                has_main_link = true;
            }
        }
        if !has_main_link {
            return Err(Fault::new(identity, Violation::MissingMainLink));
        }
    }
    Ok(())
}
