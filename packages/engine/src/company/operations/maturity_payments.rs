use super::config::IndustryBooks;
use super::core::PaymentFailureRecord;
use super::error::OperationsError;
use crate::calendar::CivilDate;
use crate::company::bank::BankError;
use crate::company::industrial::IndustrialError;
use crate::company::real_estate::RealEstateError;
use crate::company::{CompanyId, ContractId};

pub(super) fn pay_maturity(
    books: &mut IndustryBooks,
    company: &CompanyId,
    prefix: &str,
    reference: &str,
    date: CivilDate,
    failures: &mut Vec<PaymentFailureRecord>,
) -> Result<(), OperationsError> {
    let contract = ContractId(reference.into());
    let mut failed = |what: &str, amount| {
        failures.push(PaymentFailureRecord {
            company: company.clone(),
            what: format!("overdue {what}:{}", contract.0),
            amount,
        })
    };
    match (books, prefix) {
        (IndustryBooks::Industrial(books), "DEBT") => {
            let state = books
                .loan(&contract)
                .ok_or_else(|| IndustrialError::UnknownLoan {
                    contract: contract.clone(),
                })?;
            let interest = state.accrued_unpaid();
            let principal = state.outstanding();
            if interest.is_positive() {
                if let Err(error) = books.pay_interest(&contract, date) {
                    if matches!(error, IndustrialError::PaymentFailed { .. }) {
                        failed("commercial debt interest", interest);
                        return Ok(());
                    }
                    return Err(error.into());
                }
            }
            if principal.is_positive() {
                if let Err(error) = books.repay_principal(&contract, principal, date) {
                    if matches!(error, IndustrialError::PaymentFailed { .. }) {
                        failed("commercial debt principal", principal);
                    } else {
                        return Err(error.into());
                    }
                }
            }
        }
        (IndustryBooks::RealEstate(books), "DEBT") => {
            let state = books
                .loan(&contract)
                .ok_or_else(|| RealEstateError::UnknownLoan {
                    contract: contract.clone(),
                })?;
            let interest = state.accrued_unpaid();
            let principal = state.outstanding();
            if interest.is_positive() {
                if let Err(error) = books.pay_interest(&contract, date) {
                    if matches!(error, RealEstateError::PaymentFailed { .. }) {
                        failed("project debt interest", interest);
                        return Ok(());
                    }
                    return Err(error.into());
                }
            }
            if principal.is_positive() {
                if let Err(error) = books.repay_principal(&contract, principal, date) {
                    if matches!(error, RealEstateError::PaymentFailed { .. }) {
                        failed("project debt principal", principal);
                    } else {
                        return Err(error.into());
                    }
                }
            }
        }
        (IndustryBooks::Bank(books), "DEP") => {
            let state = books
                .deposit(&contract)
                .ok_or_else(|| BankError::UnknownDeposit {
                    contract: contract.clone(),
                })?;
            let interest = state.accrued_payable();
            let principal = state.principal();
            if interest.is_positive() {
                if let Err(error) = books.pay_deposit_interest(&contract, date) {
                    if matches!(error, BankError::PaymentFailed { .. }) {
                        failed("deposit interest", interest);
                        return Ok(());
                    }
                    return Err(error.into());
                }
            }
            if principal.is_positive() {
                if let Err(error) = books.withdraw_deposit(&contract, principal, date) {
                    if matches!(error, BankError::PaymentFailed { .. }) {
                        failed("deposit principal", principal);
                    } else {
                        return Err(error.into());
                    }
                }
            }
        }
        _ => {
            return Err(OperationsError::UnknownMaturityReference {
                company: company.clone(),
                reference: format!("{prefix}:{reference}"),
            })
        }
    }
    Ok(())
}
