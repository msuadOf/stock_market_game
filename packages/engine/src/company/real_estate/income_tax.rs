use std::collections::BTreeMap;

use super::{chart::acct, line, RealEstateBooks, RealEstateError};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    IncomeTaxPolicy, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::income_tax::{IncomeTaxOwnerError, IncomeTaxPosition};
use crate::company::IncomeTaxOutcome;

pub(super) fn map_tax_error(error: IncomeTaxOwnerError) -> RealEstateError {
    match error {
        IncomeTaxOwnerError::Accounting(cause) => RealEstateError::Accounting(cause),
        IncomeTaxOwnerError::StateInconsistent { detail } => {
            RealEstateError::IncomeTaxStateInconsistent { detail }
        }
    }
}

pub(super) fn validate_policy(policy: &IncomeTaxPolicy) -> Result<(), RealEstateError> {
    if !(0..=10_000).contains(&policy.rate_bp) || policy.loss_carryforward_years == 0 {
        return Err(RealEstateError::IncomeTaxStateInconsistent {
            detail: "所得税率须在0至10000bp，亏损结转年限须为正".into(),
        });
    }
    Ok(())
}

impl RealEstateBooks {
    pub fn income_tax_policy(&self) -> &IncomeTaxPolicy {
        &self.income_tax_policy
    }

    pub fn income_tax_restatements(&self) -> &BTreeMap<BusinessEventId, AccountingPeriod> {
        self.income_tax_position.restatements()
    }

    pub(crate) fn income_tax_owner(&self) -> (&Books, &IncomeTaxPosition, &IncomeTaxPolicy, u64) {
        (
            &self.books,
            &self.income_tax_position,
            &self.income_tax_policy,
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

    pub(crate) fn validate_income_tax_state(&self) -> Result<(), RealEstateError> {
        validate_policy(self.income_tax_policy())?;
        self.income_tax_position
            .validate(self.income_tax_policy())
            .map_err(map_tax_error)?;
        self.income_tax_position
            .validate_books(self.books())
            .map_err(map_tax_error)
    }

    pub fn accrue_income_tax(
        &mut self,
        date: CivilDate,
    ) -> Result<IncomeTaxOutcome, RealEstateError> {
        self.reassess_income_tax(date.year(), date, &BTreeMap::new())
    }

    pub(crate) fn reassess_income_tax(
        &mut self,
        from_year: i32,
        posted_on: CivilDate,
        adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
    ) -> Result<IncomeTaxOutcome, RealEstateError> {
        let preview = crate::company::income_tax::preview_tax_cascade(
            self.books(),
            &self.income_tax_position,
            self.income_tax_policy(),
            self.next_event_id,
            from_year,
            posted_on,
            adjustments,
        )
        .map_err(map_tax_error)?;
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

    pub fn pay_income_tax(
        &mut self,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        if !amount.is_positive() {
            return Err(RealEstateError::NonPositiveAmount {
                what: "income tax payment",
                amount,
            });
        }
        let payable = self.net_of(acct::CIT_PAYABLE)?.neg()?;
        if amount > payable {
            return Err(RealEstateError::TaxOverpayment {
                requested: amount,
                payable,
            });
        }
        let next_event_id = self.next_event_id.checked_add(1).ok_or_else(|| {
            RealEstateError::IncomeTaxStateInconsistent {
                detail: "税务事件身份空间耗尽".into(),
            }
        })?;
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            next_event_id,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::TaxPayment,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    line(acct::CIT_PAYABLE, PostingSide::Debit, amount),
                    line(acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        Ok(event)
    }
}
