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
    reports: Vec<(PublicationRequest, ReportSet, PublicationSource)>,
    operation_sources: Vec<BusinessEventId>,
}

impl<'a> ConfirmedDisclosureCorrections<'a> {
    /// 消费尚未对外发布的 owner 私有 library；不得把其中临时 ID 回填外层事实。
    pub(in crate::company) fn from_owner_result(
        base: &'a PublicLibrary,
        private_library: PublicLibrary,
        main_publication: PublicationId,
        complete_publications: Option<&[PublicationId]>,
        company: CompanyId,
        source: PublicationSource,
        operation_sources: Vec<BusinessEventId>,
    ) -> Result<Self, DisclosureCorrectionProofError> {
        use std::collections::BTreeSet;
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
        if !appended_ids.contains(&main_publication) {
            return Err(DisclosureCorrectionProofError::InvalidMainPublication {
                publication: main_publication,
            });
        }
        if let Some(complete) = complete_publications {
            if complete.len() != appended.len()
                || complete.iter().copied().collect::<BTreeSet<_>>() != appended_ids
            {
                return Err(DisclosureCorrectionProofError::IncompleteCoverage);
            }
        }
        let mut operation_sources = operation_sources;
        operation_sources.sort_unstable();
        if operation_sources.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(DisclosureCorrectionProofError::InvalidAppend {
                detail: "实际操作来源不得重复",
            });
        }
        let mut reports = Vec::with_capacity(appended.len());
        for report in appended {
            if report.company != company {
                return Err(DisclosureCorrectionProofError::WrongCompany {
                    expected: company,
                    actual: report.company,
                });
            }
            if report.source != source {
                return Err(DisclosureCorrectionProofError::WrongSource {
                    expected: source,
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
        Ok(Self {
            base,
            reports,
            operation_sources,
        })
    }

    pub(crate) fn base(&self) -> &PublicLibrary {
        self.base
    }

    pub(crate) fn reports(&self) -> &[(PublicationRequest, ReportSet, PublicationSource)] {
        &self.reports
    }

    pub(crate) fn operation_sources(&self) -> &[BusinessEventId] {
        &self.operation_sources
    }
}

#[cfg(test)]
#[path = "disclosure_correction_proof_tests.rs"]
mod tests;
