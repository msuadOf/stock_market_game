use std::collections::BTreeMap;

use super::{BankBooks, BankError};
use crate::accounting::{AccountingAmount, AccountingPeriod, BusinessEventId, IncomeTaxPolicy};
use crate::calendar::CivilDate;
use crate::company::income_tax::IncomeTaxOwnerError;
use crate::company::IncomeTaxOutcome;

pub(super) fn map_tax_error(error: IncomeTaxOwnerError) -> BankError {
    match error {
        IncomeTaxOwnerError::Accounting(error) => BankError::Accounting(error),
        IncomeTaxOwnerError::StateInconsistent { detail } => {
            BankError::IncomeTaxStateInconsistent { detail }
        }
    }
}

pub(super) fn validate_policy(policy: &IncomeTaxPolicy) -> Result<(), BankError> {
    if !(0..=10000).contains(&policy.rate_bp) || policy.loss_carryforward_years == 0 {
        return Err(BankError::IncomeTaxStateInconsistent {
            detail: "所得税税率必须在0..=10000bp，亏损结转年限必须为正".into(),
        });
    }
    Ok(())
}

impl BankBooks {
    pub(crate) fn income_tax_owner(
        &self,
    ) -> (
        &crate::accounting::Books,
        &crate::company::income_tax::IncomeTaxPosition,
        &IncomeTaxPolicy,
        u64,
    ) {
        (
            &self.books,
            &self.income_tax_position,
            &self.income_tax_policy,
            self.next_event_id,
        )
    }

    pub(crate) fn install_income_tax_owner(
        &mut self,
        books: crate::accounting::Books,
        position: crate::company::income_tax::IncomeTaxPosition,
        next_event_id: u64,
    ) {
        self.books = books;
        self.income_tax_position = position;
        self.next_event_id = next_event_id;
    }

    pub(crate) fn validate_owner_state(&self) -> Result<(), BankError> {
        self.validate_loan_claim_sources()?;
        self.ecl_policy.validate()?;
        self.validate_income_tax_state()?;
        let mut balances = BTreeMap::from([
            (super::chart::acct::LOAN_INT_RCV, AccountingAmount::ZERO),
            (super::chart::acct::LOAN_PRINCIPAL, AccountingAmount::ZERO),
            (super::chart::acct::LOAN_ALLOWANCE, AccountingAmount::ZERO),
            (super::chart::acct::ST_DEPOSIT, AccountingAmount::ZERO),
            (super::chart::acct::LT_DEPOSIT, AccountingAmount::ZERO),
            (super::chart::acct::DEP_INT_PAYABLE, AccountingAmount::ZERO),
        ]);
        for (contract, state) in &self.deposits {
            if self.loans.contains_key(contract)
                || state.principal().is_negative()
                || state.accrued_payable().is_negative()
                || state.rate_bp() < 0
                || state.maturity_date() <= state.start_date()
                || state.last_accrual_date() < state.start_date()
                || state.last_accrual_date() > state.maturity_date()
            {
                return Err(owner_error(format!(
                    "存款合同 {contract:?} 状态非法或合同身份重叠"
                )));
            }
            self.ensure_counterparty(state.counterparty())?;
            add_balance(&mut balances, state.deposit_account(), state.principal())?;
            add_balance(
                &mut balances,
                super::chart::acct::DEP_INT_PAYABLE,
                state.accrued_payable(),
            )?;
        }
        for (contract, state) in &self.loans {
            if state.principal().is_negative()
                || state.accrued_receivable().is_negative()
                || state.allowance().is_negative()
                || state.recoverable().is_negative()
                || (state.is_written_off()
                    && (state.principal().is_positive()
                        || state.accrued_receivable().is_positive()))
                || (!state.is_written_off() && state.recoverable().is_positive())
            {
                return Err(owner_error(format!("贷款合同 {contract:?} 状态非法")));
            }
            self.ensure_counterparty(state.counterparty())?;
            add_balance(
                &mut balances,
                super::chart::acct::LOAN_INT_RCV,
                state.accrued_receivable(),
            )?;
            add_balance(
                &mut balances,
                super::chart::acct::LOAN_PRINCIPAL,
                state.principal(),
            )?;
            add_balance(
                &mut balances,
                super::chart::acct::LOAN_ALLOWANCE,
                state.allowance(),
            )?;
        }
        for (code, expected) in balances {
            let debit = self
                .books()
                .ledger()
                .account_net_debit(&crate::accounting::LedgerAccountId(code.into()))?;
            let actual = if [
                super::chart::acct::LOAN_ALLOWANCE,
                super::chart::acct::ST_DEPOSIT,
                super::chart::acct::LT_DEPOSIT,
                super::chart::acct::DEP_INT_PAYABLE,
            ]
            .contains(&code)
            {
                debit.neg()?
            } else {
                debit
            };
            if actual != expected {
                return Err(owner_error(format!(
                    "银行科目 {code} 总账余额 {actual:?} 与合同子账合计 {expected:?} 不符"
                )));
            }
        }
        if self
            .books()
            .journal()
            .entries()
            .any(|entry| entry.source.value() >= self.next_event_id)
        {
            return Err(owner_error("银行事件游标不得落在已过账来源之内".into()));
        }
        Ok(())
    }
    pub fn income_tax_policy(&self) -> &IncomeTaxPolicy {
        &self.income_tax_policy
    }

    pub fn income_tax_restatements(&self) -> &BTreeMap<BusinessEventId, AccountingPeriod> {
        self.income_tax_position.restatements()
    }

    pub(crate) fn validate_income_tax_state(&self) -> Result<(), BankError> {
        validate_policy(&self.income_tax_policy)?;
        self.income_tax_position
            .validate(&self.income_tax_policy)
            .map_err(map_tax_error)?;
        self.income_tax_position
            .validate_books(self.books())
            .map_err(map_tax_error)
    }

    pub fn accrue_income_tax(&mut self, date: CivilDate) -> Result<IncomeTaxOutcome, BankError> {
        self.reassess_income_tax(date.year(), date, &BTreeMap::new())
    }

    pub(crate) fn reassess_income_tax(
        &mut self,
        from_year: i32,
        posted_on: CivilDate,
        adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
    ) -> Result<IncomeTaxOutcome, BankError> {
        self.validate_income_tax_state()?;
        let preview = crate::company::income_tax::preview_tax_cascade(
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
    ) -> Result<BusinessEventId, BankError> {
        self.validate_income_tax_state()?;
        if !amount.is_positive() {
            return Err(BankError::NonPositiveAmount {
                what: "income tax payment",
                amount,
            });
        }
        let payable = self
            .books()
            .ledger()
            .account_net_debit(&crate::accounting::LedgerAccountId(
                super::chart::acct::CIT_PAYABLE.into(),
            ))?
            .neg()?;
        if amount > payable {
            return Err(BankError::TaxOverpayment {
                requested: amount,
                payable,
            });
        }
        let next = self
            .next_event_id
            .checked_add(1)
            .ok_or_else(|| owner_error("银行税务事件身份空间耗尽".into()))?;
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            next,
            vec![crate::accounting::JournalEntry {
                source: event,
                date,
                kind: crate::accounting::BusinessKind::TaxPayment,
                cash_flow: crate::accounting::CashFlowClass::Operating,
                lines: vec![
                    super::line(
                        super::chart::acct::CIT_PAYABLE,
                        crate::accounting::PostingSide::Debit,
                        amount,
                    ),
                    super::line(
                        super::chart::acct::CASH,
                        crate::accounting::PostingSide::Credit,
                        amount,
                    ),
                ],
            }],
        )?;
        Ok(event)
    }
}

fn owner_error(detail: String) -> BankError {
    BankError::OwnershipStateInconsistent { detail }
}

fn add_balance(
    balances: &mut BTreeMap<&'static str, AccountingAmount>,
    code: &'static str,
    amount: AccountingAmount,
) -> Result<(), BankError> {
    let balance = balances
        .get_mut(code)
        .ok_or_else(|| owner_error(format!("银行子账科目 {code} 未登记")))?;
    *balance = balance.add(amount)?;
    Ok(())
}
