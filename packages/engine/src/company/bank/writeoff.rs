//! 贷款核销与回收处理器（K3 银行，任务 9；ECL 计量在 `ecl.rs`）。
//!
//! 核销（中国银行实务）：准备必须先足额覆盖账面余额（`InsufficientAllowance`
//! ——先经 `assess_credit` 100% 情景计提）；核销 Dr 1303 / Cr 1301 / Cr 1131
//! （非现金，一笔三行）。回收：Dr 1003 / Cr 1303（现金流入贷记准备——
//! 教科书分录；准备超过账面余额的部分由下一次 `assess_credit` 重估转回）。
//! 重复核销 / 非核销贷款回收 / 回收超可回收额 → 类型化拒绝。

use crate::accounting::{BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide};
use crate::calendar::CivilDate;
use crate::company::bank::{chart, BankBooks, BankError};
use crate::company::contracts::ContractId;
use crate::company::counterparty::FlowDirection;

impl BankBooks {
    /// 核销贷款（冲减准备与全部账面余额：本金 + 应计利息）。
    pub fn write_off(
        &mut self,
        loan: &ContractId,
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
        let principal = state.principal();
        let accrued = state.accrued_receivable();
        let gross = state.gross_carrying();
        if state.allowance() < gross {
            return Err(BankError::InsufficientAllowance {
                contract: loan.clone(),
                allowance: state.allowance(),
                gross,
            });
        }
        let event = BusinessEventId::new(self.next_event_id);
        // Dr 1303（准备）/ Cr 1301（本金）/ Cr 1131（应计利息）——借贷平衡由
        // 准备足额守卫保证（allowance ≥ gross = principal + accrued）。
        let mut lines = vec![
            super::line(chart::acct::LOAN_ALLOWANCE, PostingSide::Debit, gross),
            super::line(chart::acct::LOAN_PRINCIPAL, PostingSide::Credit, principal),
        ];
        if accrued.is_positive() {
            lines.push(super::line(
                chart::acct::LOAN_INT_RCV,
                PostingSide::Credit,
                accrued,
            ));
        }
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::LoanWriteOff,
                cash_flow: CashFlowClass::NonCash,
                lines,
            }],
        )?;
        if let Some(state) = self.loans.get_mut(loan) {
            state.apply_write_off()?;
        }
        Ok(event)
    }

    /// 回收已核销贷款：Dr 1003 / Cr 1303（经营现金流入；准备贷记）。
    /// 可回收余额 = 核销账面 − 已回收；回收后重估转回超额准备（gold 钉死）。
    pub fn recover_written_off(
        &mut self,
        loan: &ContractId,
        amount: crate::accounting::AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, BankError> {
        let state = self.loan(loan).ok_or(BankError::UnknownLoan {
            contract: loan.clone(),
        })?;
        if !state.is_written_off() {
            return Err(BankError::LoanNotWrittenOff {
                contract: loan.clone(),
            });
        }
        if !amount.is_positive() {
            return Err(BankError::NonPositiveAmount {
                what: "recovery",
                amount,
            });
        }
        if amount > state.recoverable() {
            return Err(BankError::RecoveryBeyondRecoverable {
                contract: loan.clone(),
                requested: amount,
                recoverable: state.recoverable(),
            });
        }
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::WriteOffRecovery,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::LOAN_ALLOWANCE, PostingSide::Credit, amount),
                ],
            }],
        )?;
        let borrower = self.loan_counterparty(loan)?;
        if let Some(state) = self.loans.get_mut(loan) {
            state.apply_recovery(amount)?;
        }
        self.record_flow(
            date,
            &borrower,
            FlowDirection::Inbound,
            amount,
            "write-off recovery",
        )?;
        Ok(event)
    }
}
