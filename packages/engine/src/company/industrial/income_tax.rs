use std::collections::BTreeMap;

use super::{IncomeTaxOutcome, IndustrialBooks, IndustrialError};
use crate::accounting::{AccountingPeriod, Books, BusinessEventId, IncomeTaxPolicy};
use crate::calendar::CivilDate;
use crate::company::income_tax::IncomeTaxOwnerError;
pub(super) use crate::company::income_tax::IncomeTaxPosition;

pub(super) fn map_owner_error(error: IncomeTaxOwnerError) -> IndustrialError {
    match error {
        IncomeTaxOwnerError::Accounting(cause) => IndustrialError::Accounting(cause),
        IncomeTaxOwnerError::StateInconsistent { detail } => {
            IndustrialError::IncomeTaxStateInconsistent { detail }
        }
    }
}

impl IndustrialBooks {
    pub(crate) fn reassess_income_tax(
        &mut self,
        from_year: i32,
        posted_on: CivilDate,
        adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
    ) -> Result<IncomeTaxOutcome, IndustrialError> {
        let preview = crate::company::income_tax::preview_tax_cascade(
            self.books(),
            &self.income_tax_position,
            &self.tax_policy().income_tax,
            self.next_event_id,
            from_year,
            posted_on,
            adjustments,
        )
        .map_err(map_owner_error)?;
        let event = preview.entries.first().map(|entry| entry.source);
        if !preview.entries.is_empty() {
            self.post_with_commit(preview.next_event_id, preview.entries)?;
        }
        self.income_tax_position = preview.position;
        Ok(IncomeTaxOutcome {
            event,
            pretax: preview.computation.pretax,
            current_tax: preview.computation.current_tax,
            current_tax_delta: preview.current_tax_delta,
            loss_offset_used: preview.computation.loss_offset_used,
            loss_added: preview.computation.loss_added,
            losses_expired: preview.computation.losses_expired,
            deferred_delta: preview.deferred_delta,
        })
    }

    pub fn income_tax_restatements(&self) -> &BTreeMap<BusinessEventId, AccountingPeriod> {
        self.income_tax_position.restatements()
    }

    pub(crate) fn income_tax_owner(&self) -> (&Books, &IncomeTaxPosition, &IncomeTaxPolicy, u64) {
        (
            self.books(),
            &self.income_tax_position,
            &self.tax_policy().income_tax,
            self.next_event_id,
        )
    }

    pub(crate) fn install_income_tax_owner(
        &mut self,
        books: Books,
        position: IncomeTaxPosition,
        next_event_id: u64,
    ) {
        self.books = books;
        self.income_tax_position = position;
        self.next_event_id = next_event_id;
    }

    pub(crate) fn validate_income_tax_state(&self) -> Result<(), IndustrialError> {
        self.income_tax_position
            .validate(&self.tax_policy().income_tax)
            .map_err(map_owner_error)?;
        self.income_tax_position
            .validate_books(self.books())
            .map_err(map_owner_error)
    }
}

pub struct IndustrialReportCorrection<'a> {
    pub closing: &'a mut crate::accounting::closing::ClosingEngine,
    pub library: &'a mut crate::information::PublicLibrary,
    pub member: &'a crate::accounting::consolidation::MemberId,
    pub correction: crate::accounting::closing::CorrectionRequest,
    pub publication: crate::information::PublicationRequest,
    pub posted_on: CivilDate,
}

#[derive(Debug, thiserror::Error)]
pub enum IndustrialCorrectionError {
    #[error("industrial correction failed for reason '{reason}': {cause}")]
    Industrial {
        reason: String,
        #[source]
        cause: IndustrialError,
    },
    #[error(transparent)]
    Publication(#[from] crate::information::CorrectionPublicationError),
    #[error(transparent)]
    Composite(Box<crate::company::CompanyCorrectionError>),
}

impl IndustrialBooks {
    pub(crate) fn correct_and_publish_with_tax(
        &mut self,
        request: IndustrialReportCorrection<'_>,
    ) -> Result<
        (
            crate::accounting::closing::ReportHandle,
            crate::information::PublicationId,
        ),
        IndustrialCorrectionError,
    > {
        let reason = request.correction.reason.clone();
        for entry in &request.correction.entries {
            for line in &entry.lines {
                if [
                    "1122", "2202", "1403", "1405", "5001", "1601", "1602", "1603", "2001", "2501",
                    "1811", "222104", "6801",
                ]
                .contains(&line.account.0.as_str())
                    || (line.account.0 == "2231" && self.loans().next().is_some())
                {
                    return Err(IndustrialCorrectionError::Industrial {
                        reason,
                        cause: IndustrialError::StructuredCorrectionRequired {
                            account: line.account.clone(),
                        },
                    });
                }
            }
        }
        let (books, position, policy, next_event_id) = self.income_tax_owner();
        let scratch = crate::company::report_correction::prepare_correction(
            crate::company::report_correction::CompanyCorrectionInput {
                books,
                position,
                policy,
                next_event_id,
                closing: request.closing,
                library: request.library,
                member: request.member,
                industry: crate::accounting::reports::IndustryPresentation::Industrial,
                correction: request.correction,
                publication: request.publication,
                posted_on: request.posted_on,
            },
        )
        .map_err(map_correction_error)?;
        let mut trial = self.clone();
        trial.install_income_tax_owner(scratch.books, scratch.position, scratch.next_event_id);
        let candidate = serde_json::to_value(&trial)
            .and_then(serde_json::from_value::<IndustrialBooks>)
            .map_err(|cause| {
                IndustrialCorrectionError::Composite(Box::new(
                    crate::company::CompanyCorrectionError::Serialization {
                        reason: reason.clone(),
                        cause: Box::new(cause),
                    },
                ))
            })?;
        let result = (scratch.report, scratch.publication);
        *self = candidate;
        *request.closing = scratch.closing;
        *request.library = scratch.library;
        Ok(result)
    }
}

fn map_correction_error(
    error: crate::company::CompanyCorrectionError,
) -> IndustrialCorrectionError {
    use crate::company::CompanyCorrectionError;
    use crate::information::CorrectionPublicationError;
    match error {
        CompanyCorrectionError::Accounting { reason, cause } => {
            IndustrialCorrectionError::Industrial {
                reason,
                cause: IndustrialError::Accounting(*cause),
            }
        }
        CompanyCorrectionError::TaxState { reason, cause } => {
            IndustrialCorrectionError::Industrial {
                reason,
                cause: map_owner_error(*cause),
            }
        }
        CompanyCorrectionError::InvalidInput { reason, detail } => {
            IndustrialCorrectionError::Industrial {
                reason,
                cause: invalid(&detail),
            }
        }
        CompanyCorrectionError::Closing { reason, cause } => {
            IndustrialCorrectionError::Publication(CorrectionPublicationError::Closing {
                reason,
                cause,
            })
        }
        CompanyCorrectionError::Information { reason, cause } => {
            IndustrialCorrectionError::Publication(CorrectionPublicationError::Information {
                reason,
                cause,
            })
        }
        error @ (CompanyCorrectionError::SimpleFinance { .. }
        | CompanyCorrectionError::Owner { .. }
        | CompanyCorrectionError::Serialization { .. }) => {
            IndustrialCorrectionError::Composite(Box::new(error))
        }
    }
}
fn invalid(detail: &str) -> IndustrialError {
    IndustrialError::IncomeTaxStateInconsistent {
        detail: detail.into(),
    }
}
