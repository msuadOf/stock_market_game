use std::collections::BTreeMap;

use super::{chart, InsuranceBooks, InsuranceError};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    IncomeTaxPolicy, JournalEntry, LedgerAccountId, LossEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::income_tax::{preview_tax_cascade, IncomeTaxOwnerError, IncomeTaxPosition};
use crate::company::IncomeTaxOutcome;

pub(super) fn map_tax_error(error: IncomeTaxOwnerError) -> InsuranceError {
    match error {
        IncomeTaxOwnerError::Accounting(error) => InsuranceError::Accounting(error),
        IncomeTaxOwnerError::StateInconsistent { detail } => {
            InsuranceError::InvalidRestoredState { detail }
        }
    }
}

pub(super) fn validate_policy(policy: &IncomeTaxPolicy) -> Result<(), InsuranceError> {
    if !(0..=10_000).contains(&policy.rate_bp) || policy.loss_carryforward_years == 0 {
        return Err(InsuranceError::InvalidRestoredState {
            detail: format!("保险所得税政策非法：{policy:?}"),
        });
    }
    Ok(())
}

impl InsuranceBooks {
    pub fn income_tax_policy(&self) -> &IncomeTaxPolicy {
        &self.income_tax_policy
    }

    pub fn loss_pool(&self) -> &[LossEntry] {
        self.income_tax_position.loss_pool()
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

    pub fn accrue_income_tax(
        &mut self,
        date: CivilDate,
    ) -> Result<IncomeTaxOutcome, InsuranceError> {
        self.reassess_income_tax(date.year(), date, &BTreeMap::new())
    }

    pub(crate) fn reassess_income_tax(
        &mut self,
        from_year: i32,
        posted_on: CivilDate,
        adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
    ) -> Result<IncomeTaxOutcome, InsuranceError> {
        let preview = preview_tax_cascade(
            &self.books,
            &self.income_tax_position,
            &self.income_tax_policy,
            self.next_event_id,
            from_year,
            posted_on,
            adjustments,
        )
        .map_err(map_tax_error)?;
        let event = preview.entries.first().map(|entry| entry.source);
        self.post_with_commit(preview.next_event_id, preview.entries)?;
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
    ) -> Result<BusinessEventId, InsuranceError> {
        if !amount.is_positive() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "income tax payment",
                amount,
            });
        }
        let payable = self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(chart::acct::CIT_PAYABLE.into()))?
            .neg()?;
        if amount > payable {
            return Err(InsuranceError::TaxOverpayment {
                requested: amount,
                payable,
            });
        }
        let next = self.next_event_id.checked_add(1).ok_or_else(|| {
            InsuranceError::InvalidRestoredState {
                detail: "保险税务事件身份空间耗尽".into(),
            }
        })?;
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            next,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::TaxPayment,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CIT_PAYABLE, PostingSide::Debit, amount),
                    super::line(chart::acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        Ok(event)
    }
}
