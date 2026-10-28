//! 在既有 civil shadow 内准备更正、完整披露与完成状态；不另开 Admin 事务。

use super::disclosures::candidate::{
    prepare_disclosures, DisclosurePrepareCtx, DisclosurePublicationKind, DisclosureSourceRef,
};
use super::{CivilDayEndReport, DayEndDisclosures, GameSession, SessionError};
use crate::accounting::{closing::CorrectionRequest, AccountingError};
use crate::company::report_correction::DisclosureCorrectionPrepareError;
use crate::company::{CompanyCorrectionError, CompanyErrorClass, CompanySystemError};
use std::sync::Arc;

/// 更正日终准备错误；仅暴露分类，真实 owner／proof／披露 cause 保持私有。
#[derive(Debug, thiserror::Error)]
#[error("报表更正日终准备失败：settled_date={settled_date}，{source}")]
pub struct ReportCorrectionDayEndError {
    settled_date: crate::CivilDate,
    classification: CompanyErrorClass,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl ReportCorrectionDayEndError {
    /// 分类不替换 typed cause；系统状态错误仍须关闭 authority。
    pub const fn classification(&self) -> CompanyErrorClass {
        self.classification
    }
}

fn reject(
    settled_date: crate::CivilDate,
    classification: CompanyErrorClass,
    source: Box<dyn std::error::Error + Send + Sync>,
) -> SessionError {
    SessionError::ReportCorrectionDayEnd(ReportCorrectionDayEndError {
        settled_date,
        classification,
        source,
    })
}

fn accounting_classification(error: &AccountingError) -> CompanyErrorClass {
    use AccountingError::*;
    match error {
        BatchAborted { cause, .. } => accounting_classification(cause),
        SimpleSettlementCashFlow { .. }
        | Unbalanced { .. }
        | NonPositiveLine { .. }
        | MissingSide { .. }
        | EmptyLines { .. }
        | UnknownAccount { .. }
        | NonCashTouchesCash { .. }
        | DuplicatePosting { .. } => CompanyErrorClass::InvalidInput,
        NegativeCashProhibited { .. } | ClosedPeriod { .. } => CompanyErrorClass::BusinessCondition,
        // 数值溢出、科目表／期间内部错误等没有业务恢复承诺，不按文本猜测原因。
        _ => CompanyErrorClass::SystemState,
    }
}

fn prepare_classification(error: &DisclosureCorrectionPrepareError) -> CompanyErrorClass {
    match error {
        DisclosureCorrectionPrepareError::Owner(CompanyCorrectionError::InvalidInput {
            ..
        }) => CompanyErrorClass::InvalidInput,
        DisclosureCorrectionPrepareError::Owner(CompanyCorrectionError::SimpleFinance {
            cause,
            ..
        }) => cause.classification(),
        DisclosureCorrectionPrepareError::Owner(CompanyCorrectionError::Accounting {
            cause,
            ..
        }) => accounting_classification(cause),
        // Serialization、TaxState、Closing、未知 owner 与 proof 都保守按系统失败处理。
        _ => CompanyErrorClass::SystemState,
    }
}

impl GameSession {
    /// 仅由 civil shadow 调用；先更正入账，再唯一一次经营关账，最后准备完整披露。
    pub(super) fn prepare_report_correction_day_end(
        &mut self,
        report: &CivilDayEndReport,
    ) -> Result<DayEndDisclosures, SessionError> {
        let settled_date = report.settled_date;
        let Some(company) = self.state.companies.system_arc() else {
            return Err(reject(
                settled_date,
                CompanyErrorClass::UnsupportedOperation,
                Box::new(CompanySystemError::Unsupported(
                    "当前待处理报表更正仅支持 Simple 公司源；请取消更正后继续日终".to_owned(),
                )),
            ));
        };
        let now = crate::CivilInstant::from_hms(settled_date, 18, 0, 0)
            .map_err(crate::calendar::CalendarError::from)?;
        let inputs = self
            .pending_report_corrections()
            .iter()
            .map(|request| crate::company::SimpleDisclosureCorrectionInput {
                company: request.company,
                correction: CorrectionRequest {
                    entries: request.entries.clone(),
                    reason: request.reason.clone(),
                },
                supersedes: request.supersedes,
            })
            .collect();
        let prepared = company
            .prepare_disclosure_corrections(inputs, now, self.state.library.as_ref())
            .map_err(|source| {
                reject(
                    settled_date,
                    prepare_classification(&source),
                    Box::new(source),
                )
            })?;
        let prepared = prepared.advance_day(settled_date).map_err(|source| {
            // 本期经营／关账失败不是更正输入拒绝，不开放健康业务重试通道。
            reject(
                settled_date,
                CompanyErrorClass::SystemState,
                Box::new(source),
            )
        })?;
        let candidate = prepare_disclosures(DisclosurePrepareCtx {
            source: DisclosureSourceRef::PreparedSimple(&prepared),
            previous_dispatch: &self.state.disclosures,
            previous_library: self.state.library.as_ref(),
            seed: self.state.seed,
            report_frequency: self
                .state
                .setup
                .report_frequency
                .unwrap_or(crate::information::ReportFrequency::Quarterly),
            through: report.disclosure_instant,
            prehistory_opening: None,
            settled_announcements: Some(settled_date),
            corporate_actions: self.state.companies.corporate_actions(),
        })
        .map_err(|source| {
            reject(
                settled_date,
                CompanyErrorClass::SystemState,
                Box::new(source),
            )
        })?;
        let status = self
            .prepare_report_correction_completion(&prepared, &candidate, report)
            .map_err(|source| {
                reject(
                    settled_date,
                    CompanyErrorClass::SystemState,
                    Box::new(source),
                )
            })?;
        let mut disclosures = DayEndDisclosures::default();
        for publication in &candidate.publications {
            match &publication.kind {
                DisclosurePublicationKind::Announcement => {
                    disclosures.announcements_published.push(publication.id);
                }
                DisclosurePublicationKind::Report { .. } => {
                    disclosures.reports_published.push(publication.id);
                }
            }
        }
        let company = Arc::new(prepared.into_source());
        let library = Arc::new(candidate.library);
        // 此前没有可变外部 callback；所选源在本方法内始终保持同一 Simple 分支。
        *self
            .state
            .companies
            .system_arc_mut()
            .expect("更正准备期间所选 Simple 源不能改变") = company;
        self.state.library = library;
        self.state.disclosures = candidate.dispatch;
        self.state.report_corrections = status;
        Ok(disclosures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn correction_owner_rejections_keep_typed_source_and_internal_failures_stay_fatal() {
        let source = DisclosureCorrectionPrepareError::Owner(CompanyCorrectionError::Accounting {
            reason: "测试未知科目拒绝".to_owned(),
            cause: Box::new(AccountingError::UnknownAccount {
                event: crate::accounting::BusinessEventId::new(17),
                account: crate::accounting::LedgerAccountId("unknown".to_owned()),
            }),
        });
        let error = reject(
            crate::CivilDate::from_iso("2030-06-08").unwrap(),
            prepare_classification(&source),
            Box::new(source),
        );
        let opaque = error.source().unwrap();
        assert_eq!(
            opaque
                .downcast_ref::<ReportCorrectionDayEndError>()
                .unwrap()
                .classification(),
            CompanyErrorClass::InvalidInput
        );
        let owner = opaque.source().unwrap();
        assert!(matches!(
            owner.downcast_ref::<DisclosureCorrectionPrepareError>(),
            Some(DisclosureCorrectionPrepareError::Owner(
                CompanyCorrectionError::Accounting { .. }
            ))
        ));
        assert!(owner.source().unwrap().is::<CompanyCorrectionError>());
        assert!(owner
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<AccountingError>());

        let overflow =
            DisclosureCorrectionPrepareError::Owner(CompanyCorrectionError::Accounting {
                reason: "测试数值内部失败".to_owned(),
                cause: Box::new(AccountingError::AmountOverflow {
                    op: "add",
                    detail: "private-overflow-operands".to_owned(),
                }),
            });
        assert_eq!(
            prepare_classification(&overflow),
            CompanyErrorClass::SystemState
        );
        let invalid_json = serde_json::from_str::<u64>("{").unwrap_err();
        let serialization =
            DisclosureCorrectionPrepareError::Owner(CompanyCorrectionError::Serialization {
                reason: "测试 owner 内部失败".to_owned(),
                cause: Box::new(invalid_json),
            });
        assert_eq!(
            prepare_classification(&serialization),
            CompanyErrorClass::SystemState
        );
        let proof = DisclosureCorrectionPrepareError::Proof(
            crate::company::report_correction::DisclosureCorrectionProofError::IncompleteCoverage,
        );
        assert_eq!(
            prepare_classification(&proof),
            CompanyErrorClass::SystemState
        );
    }
}
