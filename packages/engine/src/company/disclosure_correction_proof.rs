//! 从一次真实 owner 私有结果抽取完整更正授权；临时 PublicationId 不离开构造器。

use crate::accounting::reports::ReportSet;
use crate::accounting::BusinessEventId;
use crate::company::CompanyId;
use crate::information::{PublicLibrary, PublicationId, PublicationRequest, PublicationSource};

/// 更正 proof 的精确失败原因；不以任意 Err 冒充覆盖或来源检查。
#[derive(Debug, thiserror::Error)]
pub(crate) enum DisclosureCorrectionProofError {
    #[error("更正 proof 的 base 报告或公告前缀发生变化")]
    BaseChanged,
    #[error("更正 proof 的完整 publication 返回集合没有覆盖私有库全部新增报告")]
    IncompleteCoverage,
    #[error("更正 proof 的主 publication {publication:?} 不属于新增报告集合")]
    InvalidMainPublication { publication: PublicationId },
    #[error("更正 proof 的 company 不一致：期望 {expected}，实际 {actual}")]
    WrongCompany {
        expected: CompanyId,
        actual: CompanyId,
    },
    #[error("更正 proof 的 source 不一致：期望 {expected:?}，实际 {actual:?}")]
    WrongSource {
        expected: PublicationSource,
        actual: PublicationSource,
    },
    #[error("更正 proof 不支持引用同批临时 publication {target:?}")]
    TemporaryPredecessor { target: PublicationId },
    #[error("更正 proof 的私有新增事实非法：{detail}")]
    InvalidAppend { detail: &'static str },
}

/// 只由公司 owner 构造的完整公开更正集合；与同一不可变 source candidate 成包持有。
pub(crate) struct ConfirmedDisclosureCorrections<'a> {
    base: &'a PublicLibrary,
    operations: Vec<ConfirmedDisclosureOperation>,
}

/// 同一次真实 owner operation 的完整报告与来源；顺序对应调用方输入。
pub(crate) struct ConfirmedDisclosureOperation {
    company: CompanyId,
    source: PublicationSource,
    reports: Vec<(PublicationRequest, ReportSet, PublicationSource)>,
    operation_sources: Vec<BusinessEventId>,
}

/// 仅在 owner 到 proof 构造器之间传递；验证后丢弃全部临时 ID。
#[derive(Clone)]
pub(in crate::company) struct DisclosureCorrectionOwnerResult {
    pub main_publication: PublicationId,
    pub publications: Vec<PublicationId>,
    pub company: CompanyId,
    pub source: PublicationSource,
    pub operation_sources: Vec<BusinessEventId>,
}

impl ConfirmedDisclosureOperation {
    pub(crate) fn company(&self) -> CompanyId {
        self.company
    }
    pub(crate) fn source(&self) -> PublicationSource {
        self.source
    }
    pub(crate) fn reports(&self) -> &[(PublicationRequest, ReportSet, PublicationSource)] {
        &self.reports
    }
    pub(crate) fn operation_sources(&self) -> &[BusinessEventId] {
        &self.operation_sources
    }
}

impl<'a> ConfirmedDisclosureCorrections<'a> {
    /// 一次从原 base 提取全部 owner 分区；临时 ID 只参与此处完整性校验。
    pub(in crate::company) fn from_owner_results(
        base: &'a PublicLibrary,
        private_library: PublicLibrary,
        results: Vec<DisclosureCorrectionOwnerResult>,
    ) -> Result<Self, DisclosureCorrectionProofError> {
        use std::collections::{BTreeMap, BTreeSet};
        let before = base.save();
        let after = private_library.save();
        if after.next_seq < before.next_seq
            || after.announcements != before.announcements
            || after
                .reports
                .iter()
                .filter(|report| report.id.value() < before.next_seq)
                .ne(before.reports.iter())
        {
            return Err(DisclosureCorrectionProofError::BaseChanged);
        }
        let appended = after
            .reports
            .into_iter()
            .filter(|report| report.id.value() >= before.next_seq)
            .collect::<Vec<_>>();
        let appended_ids = appended
            .iter()
            .map(|report| report.id)
            .collect::<BTreeSet<_>>();
        if appended_ids.len() != appended.len()
            || appended_ids
                .iter()
                .map(|id| id.value())
                .ne(before.next_seq..after.next_seq)
        {
            return Err(DisclosureCorrectionProofError::InvalidAppend {
                detail: "新增 ID 必须连续覆盖整个差集且只含报告",
            });
        }
        if results.is_empty() {
            return Err(DisclosureCorrectionProofError::InvalidAppend {
                detail: "真实 owner operation 集合不得为空",
            });
        }
        for result in &results {
            if !appended_ids.contains(&result.main_publication) {
                return Err(DisclosureCorrectionProofError::InvalidMainPublication {
                    publication: result.main_publication,
                });
            }
        }
        let mut assigned = BTreeSet::new();
        for result in &results {
            for id in &result.publications {
                if !assigned.insert(*id) {
                    return Err(DisclosureCorrectionProofError::IncompleteCoverage);
                }
            }
        }
        if assigned != appended_ids {
            return Err(DisclosureCorrectionProofError::IncompleteCoverage);
        }
        let mut appended = appended
            .into_iter()
            .map(|report| (report.id, report))
            .collect::<BTreeMap<_, _>>();
        let mut operations = Vec::with_capacity(results.len());
        for result in results {
            if !result.publications.contains(&result.main_publication) {
                return Err(DisclosureCorrectionProofError::InvalidMainPublication {
                    publication: result.main_publication,
                });
            }
            let mut operation_sources = result.operation_sources;
            operation_sources.sort_unstable();
            if operation_sources.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(DisclosureCorrectionProofError::InvalidAppend {
                    detail: "实际操作来源不得重复",
                });
            }
            let mut reports = Vec::with_capacity(result.publications.len());
            for id in result.publications {
                let report = appended
                    .remove(&id)
                    .ok_or(DisclosureCorrectionProofError::IncompleteCoverage)?;
                if report.company != result.company {
                    return Err(DisclosureCorrectionProofError::WrongCompany {
                        expected: result.company,
                        actual: report.company,
                    });
                }
                if report.source != result.source {
                    return Err(DisclosureCorrectionProofError::WrongSource {
                        expected: result.source,
                        actual: report.source,
                    });
                }
                if !matches!(
                    report.origin,
                    crate::information::PublicationOrigin::Correction
                ) {
                    return Err(DisclosureCorrectionProofError::InvalidAppend {
                        detail: "新增报告必须来自同次已批准公开更正",
                    });
                }
                let target =
                    report
                        .supersedes
                        .ok_or(DisclosureCorrectionProofError::InvalidAppend {
                            detail: "更正缺少公开 predecessor",
                        })?;
                if target.value() >= before.next_seq {
                    return Err(DisclosureCorrectionProofError::TemporaryPredecessor { target });
                }
                if !before.reports.iter().any(|original| original.id == target) {
                    return Err(DisclosureCorrectionProofError::InvalidAppend {
                        detail: "公开 predecessor 不属于真实 base 报告",
                    });
                }
                let request = PublicationRequest {
                    company: report.company,
                    scope: report.reports.scope.clone(),
                    period: report.reports.period,
                    kind: report.reports.kind,
                    sequence: report.reports.version.sequence,
                    policy: report.policy,
                    approved_at: report.approved_at,
                    published_at: report.published_at,
                    origin: report.origin,
                    supersedes: report.supersedes,
                };
                reports.push((request, report.reports, report.source));
            }
            operations.push(ConfirmedDisclosureOperation {
                company: result.company,
                source: result.source,
                reports,
                operation_sources,
            });
        }
        Ok(Self { base, operations })
    }

    /// Legacy 缺少完整返回列表时仍从同一私有库差集提取，不收紧空 entries 政策。
    pub(in crate::company) fn from_owner_result(
        base: &'a PublicLibrary,
        private_library: PublicLibrary,
        main_publication: PublicationId,
        complete_publications: Option<&[PublicationId]>,
        company: CompanyId,
        source: PublicationSource,
        operation_sources: Vec<BusinessEventId>,
    ) -> Result<Self, DisclosureCorrectionProofError> {
        let publications = match complete_publications {
            Some(complete) => complete.to_vec(),
            None => private_library
                .save()
                .reports
                .into_iter()
                .filter(|report| report.id.value() >= base.version())
                .map(|report| report.id)
                .collect(),
        };
        Self::from_owner_results(
            base,
            private_library,
            vec![DisclosureCorrectionOwnerResult {
                main_publication,
                publications,
                company,
                source,
                operation_sources,
            }],
        )
    }

    pub(crate) fn base(&self) -> &PublicLibrary {
        self.base
    }

    pub(crate) fn operations(&self) -> &[ConfirmedDisclosureOperation] {
        &self.operations
    }
}

#[cfg(test)]
#[path = "disclosure_correction_proof_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "disclosure_correction_batch_proof_tests.rs"]
mod batch_tests;
