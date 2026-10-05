use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use crate::accounting::closing::CorrectionRequest;
use crate::accounting::JournalEntry;
use crate::calendar::CivilInstant;
use crate::company::operations::OperatingReportCorrection;
use crate::company::CompanyId;
use crate::information::{PublicationId, PublicationOrigin, PublicationRequest};

use super::{CivilDayEndReport, GameSession, SessionError};

#[derive(Clone, Debug)]
pub struct ReportCorrectionEpoch(Weak<()>);

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReportCorrection {
    pub operation_id: String,
    pub company: CompanyId,
    pub supersedes: PublicationId,
    pub entries: Vec<JournalEntry>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedReportCorrection {
    pub request: CompanyReportCorrection,
    pub publications: Vec<PublicationId>,
    pub committed_at: CivilInstant,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportCorrectionStatus {
    pub pending: Vec<CompanyReportCorrection>,
    pub completed: BTreeMap<String, CompletedReportCorrection>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReportCorrectionError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error(transparent)]
    Company(#[from] Box<crate::company::CompanyCorrectionError>),
    #[error(transparent)]
    Information(#[from] Box<crate::information::InformationError>),
}

fn invalid(detail: impl Into<String>) -> SessionError {
    SessionError::ReportCorrection(ReportCorrectionError::InvalidRequest(detail.into()))
}

fn recoverable_accounting(error: &crate::accounting::AccountingError) -> bool {
    use crate::accounting::AccountingError;
    match error {
        AccountingError::BatchAborted { cause, .. } => recoverable_accounting(cause),
        AccountingError::Unbalanced { .. }
        | AccountingError::NonPositiveLine { .. }
        | AccountingError::MissingSide { .. }
        | AccountingError::EmptyLines { .. }
        | AccountingError::UnknownAccount { .. }
        | AccountingError::NonCashTouchesCash { .. }
        | AccountingError::DuplicatePosting { .. }
        | AccountingError::NegativeCashProhibited { .. }
        | AccountingError::ClosedPeriod { .. } => true,
        _ => false,
    }
}

fn company_failure(error: crate::company::CompanyCorrectionError) -> SessionError {
    use crate::company::CompanyCorrectionError;
    match error {
        CompanyCorrectionError::InvalidInput { reason, detail } => {
            invalid(format!("更正理由：{reason}；拒绝原因：{detail}"))
        }
        CompanyCorrectionError::Accounting { reason, cause }
            if recoverable_accounting(&cause) =>
        {
            SessionError::ReportCorrection(ReportCorrectionError::Company(Box::new(
                CompanyCorrectionError::Accounting { reason, cause },
            )))
        }
        error => SessionError::CorrectionInvariant(Box::new(error)),
    }
}

impl GameSession {
    pub fn report_corrections(&self) -> Result<ReportCorrectionStatus, SessionError> {
        self.require_healthy()?;
        Ok(ReportCorrectionStatus {
            pending: self.state.pending_report_corrections.clone(),
            completed: self.state.report_correction_operations.clone(),
        })
    }
    pub fn report_correction_epoch(&self) -> ReportCorrectionEpoch {
        ReportCorrectionEpoch(Arc::downgrade(&self.report_correction_epoch))
    }

    pub fn cancel_report_correction(
        &mut self,
        epoch: &ReportCorrectionEpoch,
        operation_id: &str,
    ) -> Result<(), SessionError> {
        self.require_healthy()?;
        if !Weak::ptr_eq(&epoch.0, &Arc::downgrade(&self.report_correction_epoch)) {
            return Err(invalid("取消请求来自已经切换的市场或存档"));
        }
        if self
            .state
            .report_correction_operations
            .contains_key(operation_id)
        {
            return Err(invalid("已经完成的更正不可撤销；需要另行提交新的更正"));
        }
        let position = self
            .state
            .pending_report_corrections
            .iter()
            .position(|item| item.operation_id == operation_id)
            .ok_or_else(|| invalid("待处理更正操作不存在"))?;
        self.state.pending_report_corrections.remove(position);
        Ok(())
    }

    pub fn enqueue_report_correction(
        &mut self,
        epoch: &ReportCorrectionEpoch,
        request: CompanyReportCorrection,
    ) -> Result<(), SessionError> {
        self.require_healthy()?;
        if !Weak::ptr_eq(&epoch.0, &Arc::downgrade(&self.report_correction_epoch)) {
            return Err(invalid("更正请求来自已经切换的市场或存档"));
        }
        if request.operation_id.trim().is_empty()
            || request.reason.trim().is_empty()
            || request.entries.is_empty()
        {
            return Err(invalid("更正操作身份、原因与凭证必须非空"));
        }
        if let Some(existing) = self
            .state
            .report_correction_operations
            .get(&request.operation_id)
        {
            return if existing.request == request {
                Ok(())
            } else {
                Err(invalid("已经完成的更正操作身份不得改用其他载荷"))
            };
        }
        if let Some(existing) = self
            .state
            .pending_report_corrections
            .iter()
            .find(|item| item.operation_id == request.operation_id)
        {
            return if *existing == request {
                Ok(())
            } else {
                Err(invalid("待处理的更正操作身份不得改用其他载荷"))
            };
        }
        for entry in &request.entries {
            entry
                .validate_invariants()
                .map_err(|error| invalid(error.to_string()))?;
        }
        self.state.pending_report_corrections.push(request);
        Ok(())
    }

    pub(super) fn apply_report_corrections_at_day_end(
        &mut self,
        report: &CivilDayEndReport,
    ) -> Result<Vec<PublicationId>, SessionError> {
        if self.state.pending_report_corrections.is_empty() {
            return Ok(Vec::new());
        }
        let mut system = self.state.company_system.export_state();
        let mut library = self.state.library.as_ref().clone();
        let mut completed = self.state.report_correction_operations.clone();
        let mut publications = Vec::new();
        for request in &self.state.pending_report_corrections {
            let generated = system.correct_report(
                &request.company,
                CorrectionRequest { entries: request.entries.clone(), reason: request.reason.clone() },
                request.supersedes,
                report.disclosure_instant,
                &mut library,
            ).map_err(company_failure)?;
            if generated.is_empty() {
                return Err(SessionError::InvalidSave("更正执行没有返回公开报告事实".into()));
            }
            publications.extend(generated.iter().copied());
            completed.insert(request.operation_id.clone(), CompletedReportCorrection {
                request: request.clone(),
                publications: generated,
                committed_at: report.disclosure_instant,
            });
        }
        self.state.company_system = Arc::new(system);
        self.state.library = Arc::new(library);
        self.state.report_correction_operations = completed;
        self.state.pending_report_corrections.clear();
        Ok(publications)
    }
}

pub(super) fn validate_completed(
    completed: &BTreeMap<String, CompletedReportCorrection>,
    system: &crate::company::CompanySystem,
    library: &crate::information::PublicLibrary,
    current_date: crate::calendar::CivilDate,
) -> Result<(), SessionError> {
    let mut seen = std::collections::BTreeSet::new();
    for (identity, item) in completed {
        if identity != &item.request.operation_id
            || identity.trim().is_empty()
            || item.request.reason.trim().is_empty()
            || item.request.entries.is_empty()
            || item.publications.is_empty()
            || item.committed_at.date() >= current_date
        {
            return Err(SessionError::InvalidSave(
                "日终更正操作事实不完整或日期未结束".into(),
            ));
        }
        let original = library
            .report(item.request.supersedes, item.committed_at)
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        if original.source != crate::information::PublicationSource::SimpleGenerated
            || original.company != item.request.company
            || original.reports.scope
                != crate::accounting::consolidation::ScopeId::Standalone(
                    crate::accounting::consolidation::MemberId(item.request.company.0.clone()),
                )
        {
            return Err(SessionError::InvalidSave(
                "更正操作公司与原始公开报告不一致".into(),
            ));
        }
        for entry in &item.request.entries {
            entry
                .validate_invariants()
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        }
        system.validate_report_correction_fact(&item.request.company, &item.request.entries, original.reports.period)
            .map_err(|error| SessionError::InvalidSave(format!("更正操作输入与实际凭证和有效期间不一致：{error}")))?;
        for publication in &item.publications {
            if !seen.insert(*publication) {
                return Err(SessionError::InvalidSave(
                    "更正公开版本被多个操作重复归属".into(),
                ));
            }
            let report = library
                .report(*publication, item.committed_at)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            let standalone = report.company == item.request.company
                && report.reports.scope == original.reports.scope;
            if report.source != original.source
                || report.published_at != item.committed_at
                || report.origin != PublicationOrigin::Correction
                || !standalone
                || report.reports.period < original.reports.period
            {
                return Err(SessionError::InvalidSave(
                    "更正完成事实与真实公开报告不一致".into(),
                ));
            }
        }
        if !item.publications.iter().any(|id| {
            library.report(*id, item.committed_at).is_ok_and(|report| {
                report.company == item.request.company
                    && report.reports.scope == original.reports.scope
                    && report.reports.period == original.reports.period
                    && report.supersedes == Some(item.request.supersedes)
            })
        }) {
            return Err(SessionError::InvalidSave(
                "更正操作缺少关联原版的真实公开结果".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::{
        AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalLine, PostingSide,
    };

    fn session_and_request() -> (GameSession, CompanyReportCorrection) {
        session_and_request_on("2030-01-05")
    }

    fn session_and_request_on(start_date: &str) -> (GameSession, CompanyReportCorrection) {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::calendar::CivilDate::from_iso(start_date).unwrap();
        setup.ticks_per_day = 2;
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        let session = GameSession::new(setup, 7181).unwrap();
        let original = session
            .state
            .library
            .save()
            .reports
            .into_iter()
            .find(|report| {
                session
                    .state
                    .company_system
                    .finance(&report.company)
                    .is_ok()
                    && report.source == crate::information::PublicationSource::SimpleGenerated
                    && matches!(
                        report.reports.scope,
                        crate::accounting::consolidation::ScopeId::Standalone(_)
                    )
            })
            .unwrap();
        let request = CompanyReportCorrection {
            operation_id: "q23-correction-1".into(),
            company: original.company,
            supersedes: original.id,
            entries: vec![JournalEntry {
                source: BusinessEventId::new(900_000_001),
                date: session.state.civil_clock.current_date(),
                kind: BusinessKind::CashRevenue,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    JournalLine {
                        account: crate::accounting::LedgerAccountId("1002".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_yuan_str("10").unwrap(),
                    },
                    JournalLine {
                        account: crate::accounting::LedgerAccountId("6001".into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_yuan_str("10").unwrap(),
                    },
                ],
            }],
            reason: "补记前期已经收现收入".into(),
        };
        (session, request)
    }

    #[test]
    fn pending_correction_deduplicates_exact_inputs_rejects_conflicts_and_old_market_epoch() {
        let (mut session, request) = session_and_request();
        let epoch = session.report_correction_epoch();
        let original_save = session.save().unwrap();
        assert!(session.cancel_report_correction(&epoch, "不存在").is_err());
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        assert_eq!(session.state.pending_report_corrections.len(), 1);
        assert!(session.save().is_err());
        let mut conflicting = request.clone();
        conflicting.reason = "另一操作".into();
        assert!(session
            .enqueue_report_correction(&epoch, conflicting)
            .is_err());
        let mut restored = GameSession::restore(&original_save).unwrap();
        assert!(restored
            .cancel_report_correction(&epoch, "q23-correction-1")
            .is_err());
        assert!(restored.enqueue_report_correction(&epoch, request).is_err());
        assert!(restored.state.pending_report_corrections.is_empty());
    }

    #[test]
    fn correction_input_rejections_are_recoverable_but_tax_invariants_remain_fatal() {
        use crate::company::{CompanyCorrectionError, IncomeTaxOwnerError};
        let rejection = CompanyCorrectionError::InvalidInput {
            reason: "补记真实业务".into(),
            detail: "应收科目需要结构化子账事实".into(),
        };
        let rejection = company_failure(rejection);
        assert!(matches!(
            rejection,
            SessionError::ReportCorrection(ReportCorrectionError::InvalidRequest(detail))
                if detail.contains("更正理由：补记真实业务")
                    && detail.contains("拒绝原因：应收科目需要结构化子账事实")
        ));
        let invariant = CompanyCorrectionError::TaxState {
            reason: "补记真实业务".into(),
            cause: Box::new(IncomeTaxOwnerError::StateInconsistent {
                detail: "现有税务与结账映射不同".into(),
            }),
        };
        assert!(matches!(
            company_failure(invariant),
            SessionError::CorrectionInvariant(_)
        ));
        let overflow = CompanyCorrectionError::Accounting {
            reason: "补记真实业务".into(),
            cause: Box::new(crate::accounting::AccountingError::AmountOverflow {
                op: "税额级联",
                detail: "i128超出".into(),
            }),
        };
        assert!(matches!(
            company_failure(overflow),
            SessionError::CorrectionInvariant(_)
        ));
    }

    #[test]
    fn closed_day_correction_publishes_at_real_day_end_and_restores_exact_deduplication() {
        let (mut session, request) = session_and_request();
        let epoch = session.report_correction_epoch();
        let original = session.state.library.save();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let report = session.end_civil_day().unwrap();
        let completed = &session.state.report_correction_operations[&request.operation_id];
        assert_eq!(completed.committed_at, report.disclosure_instant);
        assert!(!completed.publications.is_empty());
        for old in original.reports {
            assert_eq!(
                session
                    .state
                    .library
                    .report(old.id, report.disclosure_instant)
                    .unwrap(),
                &old
            );
        }
        assert!(session.state.pending_report_corrections.is_empty());
        let save = session.save().unwrap();
        let mut restored = GameSession::restore(&save).unwrap();
        let restored_epoch = restored.report_correction_epoch();
        let before = restored.business_state_hash().unwrap();
        restored
            .enqueue_report_correction(&restored_epoch, request)
            .unwrap();
        assert_eq!(restored.business_state_hash().unwrap(), before);
        assert!(restored.state.pending_report_corrections.is_empty());
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            serde_json::to_value(save).unwrap()
        );
    }

    #[test]
    fn completed_correction_restore_rejects_a_forged_company_publication_identity() {
        let (mut session, request) = session_and_request();
        let epoch = session.report_correction_epoch();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        session.end_civil_day().unwrap();
        let save = session.save().unwrap();
        let mut corrupted = save;
        corrupted
            .report_correction_operations
            .get_mut(&request.operation_id)
            .unwrap()
            .request
            .company = CompanyId("伪造的公开报告公司身份".into());
        assert!(matches!(
            GameSession::restore(&corrupted),
            Err(SessionError::InvalidSave(detail))
                if detail == "更正操作公司与原始公开报告不一致"
        ));
    }

    #[test]
    fn later_correction_failure_rolls_back_the_whole_day_and_keeps_pending_inputs() {
        let (mut session, request) = session_and_request();
        let epoch = session.report_correction_epoch();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let mut invalid_request = request;
        invalid_request.operation_id = "q23-correction-2".into();
        invalid_request.company = CompanyId("不存在的经营公司".into());
        session
            .enqueue_report_correction(&epoch, invalid_request)
            .unwrap();
        let before = session.business_state_hash().unwrap();
        assert!(session.end_civil_day().is_err());
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(session.state.pending_report_corrections.len(), 2);
        assert!(session.state.report_correction_operations.is_empty());
        session
            .cancel_report_correction(&epoch, "q23-correction-2")
            .unwrap();
        session.end_civil_day().unwrap();
        assert_eq!(session.state.report_correction_operations.len(), 1);
    }

    #[test]
    fn month_end_applies_pending_corrections_before_closing_and_restores_real_sources() {
        let (mut session, request) = session_and_request_on("2030-01-31");
        let epoch = session.report_correction_epoch();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        session.step().unwrap();
        session.step().unwrap();
        let report = session.end_civil_day().unwrap();
        assert_eq!(report.settled_date.to_string(), "2030-01-31");
        let period = crate::accounting::AccountingPeriod::of_date(report.settled_date);
        let finance = session
            .state
            .company_system
            .finance(&request.company)
            .unwrap()
            ;
        assert_eq!(
            finance.books().journal().period_status(period),
            crate::accounting::PeriodStatus::Closed
        );
        assert!(finance
            .books()
            .journal()
            .entries()
            .any(|entry| entry.source == request.entries[0].source));
        let save = session.save().unwrap();
        GameSession::restore(&save).unwrap();
        let mut missing_source = save.clone();
        missing_source
            .report_correction_operations
            .get_mut(&request.operation_id)
            .unwrap()
            .request
            .entries[0]
            .source = crate::accounting::BusinessEventId::new(900_000_002);
        assert!(GameSession::restore(&missing_source).is_err());
        let mut future_commit = save.clone();
        future_commit
            .report_correction_operations
            .get_mut(&request.operation_id)
            .unwrap()
            .committed_at =
            crate::calendar::CivilInstant::from_hms(save.civil_clock.current_date, 18, 0, 0)
                .unwrap();
        assert!(GameSession::restore(&future_commit).is_err());
        let mut repeated_publication = save;
        let completed = repeated_publication
            .report_correction_operations
            .get_mut(&request.operation_id)
            .unwrap();
        completed.publications.push(completed.publications[0]);
        assert!(GameSession::restore(&repeated_publication).is_err());
    }

    #[test]
    fn stale_publication_is_a_recoverable_input_conflict_and_can_be_cancelled_and_reselected() {
        let (mut session, mut request) = session_and_request();
        let epoch = session.report_correction_epoch();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        session.end_civil_day().unwrap();
        let first = session.state.report_correction_operations[&request.operation_id].clone();
        request.operation_id = "q23-correction-next".into();
        request.entries[0].source = BusinessEventId::new(900_010_003);
        request.entries[0].date = session.state.civil_clock.current_date();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let before = session.business_state_hash().unwrap();
        let result = session.end_civil_day();
        assert!(matches!(
            &result,
            Err(SessionError::ReportCorrection(
                ReportCorrectionError::InvalidRequest(_)
            ))
        ), "{result:?}");
        assert_eq!(session.business_state_hash().unwrap(), before);
        session
            .cancel_report_correction(&epoch, &request.operation_id)
            .unwrap();
        let instant = first.committed_at;
        request.supersedes = first
            .publications
            .into_iter()
            .find(|id| {
                session
                    .state
                    .library
                    .report(*id, instant)
                    .is_ok_and(|report| {
                        report.company == request.company
                            && report.supersedes == Some(request.supersedes)
                    })
            })
            .unwrap();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        session.end_civil_day().unwrap();
        let save = session.save().unwrap();
        GameSession::restore(&save).unwrap();
        assert_eq!(session.state.report_correction_operations.len(), 2);
        assert!(session
            .cancel_report_correction(&epoch, &request.operation_id)
            .is_err());
    }
}
