use super::config::IndustryBooks;
use super::core::{CompanyOperations, PaymentFailureRecord};
use super::error::OperationsError;
use crate::accounting::LedgerAccountId;
use crate::calendar::CivilDate;
use crate::company::industrial::IndustrialError;
use crate::company::scheduler::{ScheduledAction, SchedulerRequest};

impl CompanyOperations {
    pub(crate) fn settle_period_end(
        &mut self,
        date: CivilDate,
        failures: &mut Vec<PaymentFailureRecord>,
    ) -> Result<(), OperationsError> {
        let month_end = date.next()?.month() != date.month();
        for (id, company) in &mut self.companies {
            if let IndustryBooks::Industrial(books) = &mut company.books {
                if month_end {
                    books.depreciate_month(date)?;
                    if date.month() == 12 {
                        books.accrue_income_tax(date)?;
                    }
                }
                let payable = books
                    .books()
                    .ledger()
                    .account_net_debit(&LedgerAccountId("222104".into()))?
                    .neg()?;
                if payable.is_positive() {
                    if let Err(error) = books.pay_income_tax(payable, date) {
                        if matches!(error, IndustrialError::PaymentFailed { .. }) {
                            failures.push(PaymentFailureRecord {
                                obligation_status: crate::company::events::PaymentObligationStatus::StatutoryPaymentFailure,
                                company: id.clone(),
                                what: "overdue income tax payment".into(),
                                amount: payable,
                            });
                        } else {
                            return Err(error.into());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn schedule_debt_maturities(
        &mut self,
        first_due: CivilDate,
    ) -> Result<(), OperationsError> {
        let mut requests = Vec::new();
        for (id, company) in &self.companies {
            if let IndustryBooks::Bank(books) = &company.books {
                for (contract, deposit) in books.deposits() {
                    if deposit.principal().is_zero() && deposit.accrued_payable().is_zero() {
                        continue;
                    }
                    let key = format!("DEP:{}:{}", id.0, contract.0);
                    if self.scheduler.pending().iter().any(|due| due.key == key) {
                        continue;
                    }
                    requests.push(SchedulerRequest::Due {
                        key,
                        due_date: deposit.maturity_date().max(first_due),
                        action: ScheduledAction::ContractMaturity {
                            company: id.clone(),
                            reference: format!("DEP:{}", contract.0),
                        },
                    });
                }
            }
            let debts: Vec<_> = match &company.books {
                IndustryBooks::Industrial(books) => {
                    let mut debts = Vec::new();
                    for (contract, state) in books.loans() {
                        if state.outstanding().is_zero() && state.accrued_unpaid().is_zero() {
                            continue;
                        }
                        let terms = books.contracts().get(contract).ok_or_else(|| {
                            IndustrialError::UnknownLoan {
                                contract: contract.clone(),
                            }
                        })?;
                        debts.push((contract.clone(), terms.maturity_date));
                    }
                    debts
                }
                IndustryBooks::RealEstate(books) => books
                    .loans()
                    .filter(|(_, state)| {
                        state.outstanding().is_positive() || state.accrued_unpaid().is_positive()
                    })
                    .map(|(contract, state)| (contract.clone(), state.maturity_date()))
                    .collect(),
                _ => Vec::new(),
            };
            for (contract, maturity) in debts {
                let key = format!("DEBT:{}:{}", id.0, contract.0);
                if self.scheduler.pending().iter().any(|due| due.key == key) {
                    continue;
                }
                requests.push(SchedulerRequest::Due {
                    key,
                    due_date: maturity.max(first_due),
                    action: ScheduledAction::ContractMaturity {
                        company: id.clone(),
                        reference: format!("DEBT:{}", contract.0),
                    },
                });
            }
        }
        for request in requests {
            self.scheduler.submit(request)?;
        }
        Ok(())
    }
}
