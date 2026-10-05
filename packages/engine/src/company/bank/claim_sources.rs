use std::collections::{BTreeMap, BTreeSet};

use super::{BankBooks, BankError};
use crate::accounting::{BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide};
use crate::company::ContractId;

#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum BankLoanClaimKind {
    Principal,
    Interest,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankLoanClaimSource {
    pub loan: ContractId,
    pub source: BusinessEventId,
    pub kind: BankLoanClaimKind,
}

impl super::BankBooks {
    pub fn loan_claim_sources(&self) -> &[BankLoanClaimSource] {
        &self.loan_claim_sources
    }

    pub(super) fn validate_loan_claim_sources(&self) -> Result<(), BankError> {
        let entries: BTreeMap<_, _> = self
            .books
            .journal()
            .entries()
            .map(|entry| (entry.source, entry))
            .collect();
        if entries
            .keys()
            .any(|source| source.value() >= self.next_event_id)
        {
            return Err(invalid("银行事件游标不得落在已过账来源之内".into()));
        }
        let expected: BTreeSet<_> = entries
            .values()
            .filter(|entry| {
                matches!(
                    entry.kind,
                    BusinessKind::LoanIssued | BusinessKind::LoanInterestAccrued
                )
            })
            .map(|entry| entry.source)
            .collect();
        let mut covered = BTreeSet::new();
        let mut principals = BTreeSet::new();
        for fact in &self.loan_claim_sources {
            if fact.loan.0.trim().is_empty() || !covered.insert(fact.source) {
                return Err(invalid(format!(
                    "贷款来源 {:?} 的合同身份为空或来源重复",
                    fact.source
                )));
            }
            let loan = self.loans.get(&fact.loan).ok_or_else(|| {
                invalid(format!(
                    "来源 {:?} 关联未知贷款 {:?}",
                    fact.source, fact.loan
                ))
            })?;
            let entry = entries.get(&fact.source).ok_or_else(|| {
                invalid(format!(
                    "贷款 {:?} 来源 {:?} 未真实过账",
                    fact.loan, fact.source
                ))
            })?;
            let (kind, credit, flow) = match fact.kind {
                BankLoanClaimKind::Principal => {
                    if !principals.insert(&fact.loan) || entry.date != loan.start_date() {
                        return Err(invalid(format!(
                            "贷款 {:?} 本金来源重复或日期不是实际发放日",
                            fact.loan
                        )));
                    }
                    (BusinessKind::LoanIssued, "1003", CashFlowClass::Operating)
                }
                BankLoanClaimKind::Interest => (
                    BusinessKind::LoanInterestAccrued,
                    "6011",
                    CashFlowClass::NonCash,
                ),
            };
            let debit = match fact.kind {
                BankLoanClaimKind::Principal => "1301",
                BankLoanClaimKind::Interest => "1131",
            };
            if entry.kind != kind
                || entry.cash_flow != flow
                || entry.date < loan.start_date()
                || entry.date > loan.last_accrual_date()
                || !matches_entry(entry, debit, credit)
            {
                return Err(invalid(format!(
                    "贷款 {:?} 来源 {:?} 与真实业务种类／日期／正额科目不符",
                    fact.loan, fact.source
                )));
            }
        }
        if covered != expected || principals.len() != self.loans.len() {
            return Err(invalid(
                "真实贷款本金／利息凭证未得到唯一完整来源覆盖".into(),
            ));
        }
        for (id, loan) in &self.loans {
            if id.0.trim().is_empty()
                || loan.start_date() >= loan.maturity_date()
                || loan.last_accrual_date() < loan.start_date()
                || loan.rate_bp() < 0
            {
                return Err(invalid(format!("贷款 {id:?} 原始期限或计息状态非法")));
            }
            self.ensure_counterparty(loan.counterparty())?;
        }
        Ok(())
    }
}

fn matches_entry(entry: &JournalEntry, debit: &str, credit: &str) -> bool {
    if entry.lines.len() != 2 {
        return false;
    }
    let debit_line = entry
        .lines
        .iter()
        .find(|line| line.account.0 == debit && line.side == PostingSide::Debit);
    let credit_line = entry
        .lines
        .iter()
        .find(|line| line.account.0 == credit && line.side == PostingSide::Credit);
    match (debit_line, credit_line) {
        (Some(debit), Some(credit)) => debit.amount.is_positive() && debit.amount == credit.amount,
        _ => false,
    }
}

fn invalid(detail: String) -> BankError {
    BankError::OwnershipStateInconsistent { detail }
}

impl<'de> serde::Deserialize<'de> for BankBooks {
    fn deserialize<Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<Self, Deserializer::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Facts {
            books: crate::accounting::Books,
            deposits: super::deposits::DepositMap,
            loans: BTreeMap<ContractId, super::BankLoanState>,
            counterparties: crate::company::CounterpartyLedger,
            ecl_policy: super::EclPolicy,
            income_tax_policy: crate::accounting::IncomeTaxPolicy,
            income_tax_position: crate::company::income_tax::IncomeTaxPosition,
            next_event_id: u64,
            loan_claim_sources: Vec<BankLoanClaimSource>,
        }
        let facts = <Facts as serde::Deserialize>::deserialize(deserializer)?;
        let bank = BankBooks {
            books: facts.books,
            deposits: facts.deposits,
            loans: facts.loans,
            counterparties: facts.counterparties,
            ecl_policy: facts.ecl_policy,
            income_tax_policy: facts.income_tax_policy,
            income_tax_position: facts.income_tax_position,
            next_event_id: facts.next_event_id,
            loan_claim_sources: facts.loan_claim_sources,
        };
        bank.validate_loan_claim_sources()
            .map_err(serde::de::Error::custom)?;
        Ok(bank)
    }
}
