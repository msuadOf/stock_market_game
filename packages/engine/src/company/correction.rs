use crate::accounting::closing::CorrectionRequest;
use crate::accounting::consolidation::ScopeId;
use crate::accounting::{AccountingPeriod, JournalEntry};
use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::{
    InformationError, PublicLibrary, PublicationId, PublicationOrigin, PublicationRequest,
    PublicationSource,
};

impl super::CompanySystem {
    pub fn correct_report(
        &mut self,
        company: &CompanyId,
        correction: CorrectionRequest,
        supersedes: PublicationId,
        now: CivilInstant,
        library: &mut PublicLibrary,
    ) -> Result<Vec<PublicationId>, super::CompanyCorrectionError> {
        let reason = correction.reason.clone();
        let invalid = |detail: String| super::CompanyCorrectionError::InvalidInput {
            reason: reason.clone(),
            detail,
        };
        if reason.trim().is_empty() || correction.entries.is_empty() {
            return Err(invalid("更正理由和调整凭证均不能为空".into()));
        }
        if now
            != CivilInstant::from_hms(now.date(), 18, 0, 0)
                .map_err(|error| invalid(error.to_string()))?
        {
            return Err(invalid("更正公布时点必须为披露日 18:00".into()));
        }
        let mut candidate = self.clone();
        let mut candidate_library = library.clone();
        let finance = candidate
            .finance_mut(company)
            .map_err(|error| invalid(error.to_string()))?;
        let original = match candidate_library.report(supersedes, now) {
            Ok(original) => original.clone(),
            Err(
                error @ (InformationError::UnknownPublication { .. }
                | InformationError::EarlyRead { .. }),
            ) => {
                return Err(invalid(error.to_string()));
            }
            Err(cause) => return Err(information(reason.clone(), cause)),
        };
        let expected_scope = ScopeId::Standalone(finance.member_id());
        let member = finance.member_id();
        if original.company != *company
            || original.reports.scope != expected_scope
            || original.source != PublicationSource::SimpleGenerated
        {
            return Err(invalid("更正目标不是该 Simple 公司的单体公开报告".into()));
        }
        let latest = candidate_library
            .latest_report(company, original.reports.kind, original.reports.period, now)
            .ok_or_else(|| invalid("更正目标公开报告不存在".into()))?;
        if latest.id != supersedes {
            return Err(invalid("只能更正该公司该期间该类型的最新公开版本".into()));
        }
        if correction
            .entries
            .iter()
            .any(|entry| entry.date > now.date())
        {
            return Err(invalid("调整凭证实际日期不得晚于披露日".into()));
        }
        for entry in &correction.entries {
            entry
                .validate_invariants()
                .map_err(|cause| invalid(cause.to_string()))?;
        }
        let sequence = finance
            .closing()
            .versions(
                &expected_scope,
                original.reports.period,
                original.reports.kind,
            )
            .last()
            .ok_or_else(|| invalid("更正目标缺少结账版本".into()))?
            .version
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("更正报告版本序号耗尽".into()))?;
        let approved_at = CivilInstant::from_hms(now.date(), 8, 0, 0)
            .map_err(|error| invalid(error.to_string()))?;
        let publication = PublicationRequest {
            company: *company,
            scope: expected_scope,
            period: original.reports.period,
            kind: original.reports.kind,
            sequence,
            policy: original.policy.clone(),
            approved_at,
            published_at: now,
            origin: PublicationOrigin::Correction,
            supersedes: Some(supersedes),
        };
        let scratch = crate::company::report_correction::prepare_correction(
            crate::company::report_correction::CompanyCorrectionInput {
                books: finance.books(),
                position: finance.income_tax_position(),
                policy: &finance.config().tax_policy.income_tax,
                next_event_id: finance.next_event_id(),
                closing: finance.closing(),
                library: &candidate_library,
                member: &member,
                industry: finance.industry(),
                correction,
                publication,
                posted_on: now.date(),
            },
        )?;
        let publications = scratch.publications.clone();
        candidate_library = scratch.library;
        candidate
            .finance_mut(company)
            .map_err(|error| invalid(error.to_string()))?
            .install_correction(
                scratch.books,
                scratch.closing,
                scratch.position,
                scratch.next_event_id,
            )
            .map_err(|cause| super::CompanyCorrectionError::SimpleFinance {
                reason: reason.clone(),
                cause: Box::new(cause),
            })?;
        *self = candidate;
        *library = candidate_library;
        Ok(publications)
    }

    pub(crate) fn validate_report_correction_fact(
        &self,
        company: &CompanyId,
        entries: &[JournalEntry],
        period: AccountingPeriod,
    ) -> Result<(), super::CompanyCorrectionError> {
        let finance =
            self.finance(company)
                .map_err(|error| super::CompanyCorrectionError::InvalidInput {
                    reason: "校验已完成的 Simple 报表更正".into(),
                    detail: error.to_string(),
                })?;
        for expected in entries {
            let actual = finance
                .books()
                .journal()
                .entries()
                .find(|entry| entry.source == expected.source);
            if actual != Some(expected) {
                return Err(super::CompanyCorrectionError::InvalidInput {
                    reason: "校验已完成的 Simple 报表更正".into(),
                    detail: format!(
                        "调整凭证来源 {} 不存在或内容不一致",
                        expected.source.value()
                    ),
                });
            }
        }
        let scope = ScopeId::Standalone(finance.member_id());
        let closing_restatements = finance
            .closing()
            .restatement_periods(&scope)
            .cloned()
            .unwrap_or_default();
        if finance.income_tax_position().restatements() != &closing_restatements {
            return Err(super::CompanyCorrectionError::InvalidInput {
                reason: "校验已完成的 Simple 报表更正".into(),
                detail: "所得税与结账重述状态不一致".into(),
            });
        }
        for expected in entries {
            if finance
                .closing()
                .restatement_periods(&scope)
                .and_then(|periods| periods.get(&expected.source))
                != Some(&period)
            {
                return Err(super::CompanyCorrectionError::InvalidInput {
                    reason: "校验已完成的 Simple 报表更正".into(),
                    detail: format!("调整凭证来源 {} 的有效期间不匹配", expected.source.value()),
                });
            }
        }
        Ok(())
    }
}

fn information(reason: String, cause: InformationError) -> super::CompanyCorrectionError {
    super::CompanyCorrectionError::Information {
        reason,
        cause: Box::new(cause),
    }
}

/// 真实 Simple owner 的私有金融候选与完整公开授权；不得拆改其中任一侧。
pub(crate) struct PreparedSimpleDisclosureCorrection<'a> {
    system: super::CompanySystem,
    proof: super::report_correction::ConfirmedDisclosureCorrections<'a>,
}

/// Session 按接收顺序传入的公司域更正；operation_id 留在 Session。
pub(crate) struct SimpleDisclosureCorrectionInput {
    pub company: CompanyId,
    pub correction: CorrectionRequest,
    pub supersedes: PublicationId,
}

impl super::CompanySystem {
    pub(crate) fn prepare_disclosure_corrections<'a>(
        &self,
        inputs: Vec<SimpleDisclosureCorrectionInput>,
        now: CivilInstant,
        base: &'a PublicLibrary,
    ) -> Result<
        PreparedSimpleDisclosureCorrection<'a>,
        super::report_correction::DisclosureCorrectionPrepareError,
    > {
        if inputs.is_empty() {
            return Err(
                super::report_correction::DisclosureCorrectionProofError::InvalidAppend {
                    detail: "真实 owner operation 集合不得为空",
                }
                .into(),
            );
        }
        let mut system = self.clone();
        let mut private_library = base.clone();
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            let reason = input.correction.reason.clone();
            let operation_sources = input
                .correction
                .entries
                .iter()
                .map(|entry| entry.source)
                .collect();
            let publications = system.correct_report(
                &input.company,
                input.correction,
                input.supersedes,
                now,
                &mut private_library,
            )?;
            let main_publication = publications.first().copied().ok_or_else(|| {
                super::CompanyCorrectionError::InvalidInput {
                    reason,
                    detail: "真实更正 owner 返回空 publication 集合".into(),
                }
            })?;
            results.push(super::report_correction::DisclosureCorrectionOwnerResult {
                main_publication,
                publications,
                company: input.company,
                source: PublicationSource::SimpleGenerated,
                operation_sources,
            });
        }
        let proof = super::report_correction::ConfirmedDisclosureCorrections::from_owner_results(
            base,
            private_library,
            results,
        )?;
        Ok(PreparedSimpleDisclosureCorrection { system, proof })
    }
}

impl<'a> PreparedSimpleDisclosureCorrection<'a> {
    pub(crate) fn source(&self) -> &super::CompanySystem {
        &self.system
    }
    pub(crate) fn proof(&self) -> &super::report_correction::ConfirmedDisclosureCorrections<'a> {
        &self.proof
    }
    /// 更正先入账，再由同一包推进经营与封账；失败时丢弃整个私有候选。
    pub(crate) fn advance_day(
        mut self,
        date: crate::calendar::CivilDate,
    ) -> Result<Self, super::CompanySystemError> {
        self.system.advance_day(date)?;
        Ok(self)
    }
    pub(crate) fn into_source(self) -> super::CompanySystem {
        self.system
    }
}

impl super::CompanySystem {
    #[cfg(test)]
    pub(crate) fn prepare_disclosure_correction<'a>(
        &self,
        company: &CompanyId,
        correction: CorrectionRequest,
        supersedes: PublicationId,
        now: CivilInstant,
        base: &'a PublicLibrary,
    ) -> Result<
        PreparedSimpleDisclosureCorrection<'a>,
        super::report_correction::DisclosureCorrectionPrepareError,
    > {
        self.prepare_disclosure_corrections(
            vec![SimpleDisclosureCorrectionInput {
                company: *company,
                correction,
                supersedes,
            }],
            now,
            base,
        )
    }
}

#[cfg(test)]
#[path = "disclosure_correction_owner_tests.rs"]
mod disclosure_tests;

#[cfg(test)]
#[path = "disclosure_correction_batch_tests.rs"]
mod disclosure_batch_tests;
