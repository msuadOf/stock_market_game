//! 贷款发放与收本处理器（K3 银行，任务 9；状态/数学在 `loans.rs`、计息在
//! `interest.rs`、ECL 计量在 `ecl.rs`）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::bank::ecl::ecl_allowance_target;
use crate::company::bank::loans::{validate_terms, BankLoanState};
use crate::company::bank::{chart, BankBooks, BankError, BankProductKind};
use crate::company::contracts::ContractId;
use crate::company::counterparty::{CounterpartyId, FlowDirection};

impl BankBooks {
    /// 发放贷款（摊余成本、固定利率、明确合同现金流——CAS 22 §16/§17）：
    /// Dr 1301 / Cr 1003（经营——CAS 30 (2026) §45–§47 向客户提供融资归类）
    /// 并附带初始 12 个月 ECL（CAS 22 §60 初始确认处于阶段一的假设；政策
    /// 默认情景，非零才入账）。资金不足 → `PaymentFailed`（银行不能贷出
    /// 没有的钱）。
    #[allow(clippy::too_many_arguments)]
    pub fn issue_loan(
        &mut self,
        kind: BankProductKind,
        loan: ContractId,
        borrower: &CounterpartyId,
        principal: AccountingAmount,
        annual_rate_bp: i32,
        start: CivilDate,
        maturity: CivilDate,
    ) -> Result<Vec<BusinessEventId>, BankError> {
        kind.require_loan()?;
        validate_terms(&loan, principal, annual_rate_bp, start, maturity)?;
        self.ensure_counterparty(borrower)?;
        if self.loans.contains_key(&loan) || self.deposits.contains_key(&loan) {
            return Err(BankError::DuplicateContract { contract: loan });
        }
        let day_one = ecl_allowance_target(principal, &self.ecl_policy.stage1_default.clone())?;
        let base = self.next_event_id;
        let disbursement = BusinessEventId::new(base);
        let mut events = vec![disbursement];
        let mut entries = vec![JournalEntry {
            source: disbursement,
            date: start,
            kind: BusinessKind::LoanIssued,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                super::line(chart::acct::LOAN_PRINCIPAL, PostingSide::Debit, principal),
                super::line(chart::acct::CASH, PostingSide::Credit, principal),
            ],
        }];
        if day_one.is_positive() {
            let impairment = BusinessEventId::new(base + 1);
            events.push(impairment);
            entries.push(JournalEntry {
                source: impairment,
                date: start,
                kind: BusinessKind::CreditImpairment,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::CREDIT_IMPAIR, PostingSide::Debit, day_one),
                    super::line(chart::acct::LOAN_ALLOWANCE, PostingSide::Credit, day_one),
                ],
            });
        }
        self.post_with_commit(base + events.len() as u64, entries)?;
        let mut state = BankLoanState::new(principal, annual_rate_bp, borrower.clone(), start);
        state.allowance = day_one;
        self.loans.insert(loan.clone(), state);
        self.record_flow(
            start,
            borrower,
            FlowDirection::Outbound,
            principal,
            "loan disbursement",
        )?;
        Ok(events)
    }

    /// 收回贷款本金（不重复计收入）：Dr 1003 / Cr 1301（经营）。
    pub fn collect_loan_principal(
        &mut self,
        loan: &ContractId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, BankError> {
        let state = self.loan(loan).ok_or(BankError::UnknownLoan {
            contract: loan.clone(),
        })?;
        if state.is_written_off() {
            return Err(BankError::LoanAlreadyWrittenOff {
                contract: loan.clone(),
            });
        }
        if !amount.is_positive() {
            return Err(BankError::NonPositiveAmount {
                what: "principal collection",
                amount,
            });
        }
        if amount > state.principal() {
            return Err(BankError::PrincipalBeyondOutstanding {
                contract: loan.clone(),
                requested: amount,
                outstanding: state.principal(),
            });
        }
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::LoanPrincipalCollected,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::LOAN_PRINCIPAL, PostingSide::Credit, amount),
                ],
            }],
        )?;
        let borrower = self.loan_counterparty(loan)?;
        if let Some(state) = self.loans.get_mut(loan) {
            state.collect_principal(amount)?;
        }
        self.record_flow(
            date,
            &borrower,
            FlowDirection::Inbound,
            amount,
            "loan principal collected",
        )?;
        Ok(event)
    }

    pub(super) fn loan_counterparty(&self, loan: &ContractId) -> Result<CounterpartyId, BankError> {
        self.loans
            .get(loan)
            .map(|state| state.counterparty.clone())
            .ok_or(BankError::UnknownLoan {
                contract: loan.clone(),
            })
    }
}
