//! 到期派发（`CompanyOperations` 的 due 分派拆分）：滚动利息计提与合同
//! 到期引用（`AR:`/`LN:`/`DL:`）到行业处理器的路由。逐日循环在 `day.rs`。

use crate::calendar::CivilDate;
use crate::company::bank::BankError;
use crate::company::industrial::IndustrialError;
use crate::company::operations::config::IndustryBooks;
use crate::company::operations::core::{CompanyOperations, OperatingCompany};
use crate::company::operations::error::OperationsError;
use crate::company::scheduler::ScheduledAction;
use crate::company::spec::CompanyId;
use crate::company::ContractId;

impl CompanyOperations {
    /// 派发当日到期（(due_date, id) 稳定序）并分派到行业处理器。
    pub(crate) fn dispatch_due_on(&mut self, date: CivilDate) -> Result<usize, OperationsError> {
        let dues = self.scheduler.pop_due_on(date)?;
        let count = dues.len();
        for due in dues {
            match due.action {
                ScheduledAction::InterestAccrual { company } => {
                    self.accrue_interest_for(&company, date)?;
                }
                ScheduledAction::ContractMaturity { company, reference } => {
                    self.dispatch_maturity(&company, &reference, date)?;
                }
            }
        }
        Ok(count)
    }

    pub(crate) fn accrue_interest_for(
        &mut self,
        company: &CompanyId,
        date: CivilDate,
    ) -> Result<(), OperationsError> {
        let target = self.company_or_err(company, "interest due")?;
        match &mut target.books {
            IndustryBooks::Industrial(books) => {
                books.accrue_interest(date)?;
            }
            IndustryBooks::Bank(books) => {
                books.accrue_loan_interest(date)?;
                books.accrue_deposit_interest(date)?;
            }
            IndustryBooks::RealEstate(books) => {
                books.accrue_interest(date)?;
            }
            IndustryBooks::Insurance(_) => {
                return Err(OperationsError::InterestAccrualWithoutDebtModel {
                    company: company.clone(),
                });
            }
        }
        Ok(())
    }

    /// 到期引用分派：`AR:` 工商回款 / `LN:` 银行收本收息 / `DL:` 地产交付。
    fn dispatch_maturity(
        &mut self,
        company: &CompanyId,
        reference: &str,
        date: CivilDate,
    ) -> Result<(), OperationsError> {
        let target = self.company_or_err(company, "maturity due")?;
        let (prefix, rest) =
            reference
                .split_once(':')
                .ok_or_else(|| OperationsError::UnknownMaturityReference {
                    company: company.clone(),
                    reference: reference.to_string(),
                })?;
        match (&mut target.books, prefix) {
            (IndustryBooks::Industrial(books), "AR") => {
                let id = crate::accounting::OpenItemId(rest.to_string());
                let open = books
                    .receivables()
                    .open_amount(&id)
                    .map_err(IndustrialError::from)?;
                if open.is_positive() {
                    books.collect(&id, open, date)?;
                }
                Ok(())
            }
            (IndustryBooks::Bank(books), "LN") => {
                let loan = ContractId(rest.to_string());
                let accrued = books
                    .loan(&loan)
                    .map(|state| state.accrued_receivable())
                    .ok_or_else(|| BankError::UnknownLoan {
                        contract: loan.clone(),
                    })?;
                if accrued.is_positive() {
                    books.collect_loan_interest(&loan, accrued, date)?;
                }
                let principal = books
                    .loan(&loan)
                    .map(|state| state.principal())
                    .ok_or_else(|| BankError::UnknownLoan {
                        contract: loan.clone(),
                    })?;
                if principal.is_positive() {
                    books.collect_loan_principal(&loan, principal, date)?;
                }
                Ok(())
            }
            (IndustryBooks::RealEstate(books), "DL") => {
                let contract = ContractId(rest.to_string());
                let outcome = books.deliver(&contract, date)?;
                if let Some(receivable) = &outcome.receivable {
                    books.collect_final(receivable, outcome.receivable_amount, date)?;
                }
                Ok(())
            }
            _ => Err(OperationsError::UnknownMaturityReference {
                company: company.clone(),
                reference: reference.to_string(),
            }),
        }
    }

    fn company_or_err(
        &mut self,
        company: &CompanyId,
        what: &str,
    ) -> Result<&mut OperatingCompany, OperationsError> {
        self.companies
            .get_mut(company)
            .ok_or(OperationsError::Company(
                crate::company::CompanyError::SpecInvalid {
                    detail: format!("{what} for unknown company {company:?}"),
                },
            ))
    }
}
