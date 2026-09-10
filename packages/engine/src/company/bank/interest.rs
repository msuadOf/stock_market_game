//! 银行计息与结息处理器（K3 银行，任务 9；状态/数学在 `loans.rs`）。
//!
//! 实际利率法（CAS 22 §38/§39）：本游戏存贷款均为平价发放、无费用/折溢价的
//! 固定利率合同 ⇒ 合同利率 = 实际利率（文档化等价，不虚构重定价模型）。
//! 贷款计息：阶段一/二按毛额（本金）计息；阶段三按净额（账面余额 − 减值
//! 准备）计息（CAS 22 已发生信用减值金融资产的净额法；净基数为负时计息
//! 基数为零——显式规则，回收后重估即转回）。存款计提是利息支出；到期
//! 停息（计提日截断至到期日，无自动转存模型）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, FractionUnits, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::bank::deposits::{DepositAccrualItem, DepositState};
use crate::company::bank::ecl::EclStage;
use crate::company::bank::loans::{accrue_act_365f, BankLoanState};
use crate::company::bank::{chart, BankBooks, BankError};
use crate::company::contracts::ContractId;
use crate::company::counterparty::FlowDirection;

/// 单笔贷款计提结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LoanAccrualItem {
    pub loan: ContractId,
    pub days: i64,
    pub amount: AccountingAmount,
    pub remaining_carried: FractionUnits,
}

impl BankBooks {
    /// 计提全部贷款利息至 `through`（非现金：Dr 1131 / Cr 6011）。
    /// 阶段三用净额基数；时间回拨 → `AccrualNotForward`；天数 0 跳过；
    /// 零金额计提仍推进余数与计提日（守恒所需）。**贷款利息是收入**
    /// （应计制，非收现制）。已核销贷款基数为零自然跳过。
    pub fn accrue_loan_interest(
        &mut self,
        through: CivilDate,
    ) -> Result<Vec<LoanAccrualItem>, BankError> {
        let base = self.next_event_id;
        let mut items = Vec::new();
        let mut entries = Vec::new();
        let states: Vec<(ContractId, BankLoanState)> = self
            .loans
            .iter()
            .map(|(id, state)| (id.clone(), state.clone()))
            .collect();
        for (loan_id, state) in states {
            let days = through.days_since(state.last_accrual_date());
            if days < 0 {
                return Err(BankError::AccrualNotForward {
                    contract: loan_id,
                    through,
                    last_accrual: state.last_accrual_date(),
                });
            }
            if days == 0 {
                continue;
            }
            let base_amount = match state.stage() {
                EclStage::Stage1 | EclStage::Stage2 => state.principal(),
                EclStage::Stage3 => state.net_accrual_base(),
            };
            let (amount, remaining) =
                accrue_act_365f(base_amount, state.rate_bp, days, state.carried())?;
            let event = BusinessEventId::new(base + items.len() as u64);
            if amount.is_positive() {
                entries.push(JournalEntry {
                    source: event,
                    date: through,
                    kind: BusinessKind::LoanInterestAccrued,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(chart::acct::LOAN_INT_RCV, PostingSide::Debit, amount),
                        super::line(chart::acct::INTEREST_INCOME, PostingSide::Credit, amount),
                    ],
                });
            }
            items.push(LoanAccrualItem {
                loan: loan_id,
                days,
                amount,
                remaining_carried: remaining,
            });
        }
        self.post_with_commit(base + items.len() as u64, entries)?;
        for item in &items {
            if let Some(state) = self.loans.get_mut(&item.loan) {
                state.apply_accrual(item.amount, item.remaining_carried, through)?;
            }
        }
        Ok(items)
    }

    /// 收妥贷款利息（不重复计收入）：Dr 1003 / Cr 1131（经营）。
    /// 超已提应收 → `InterestBeyondAccrued`。
    pub fn collect_loan_interest(
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
                what: "interest collection",
                amount,
            });
        }
        if amount > state.accrued_receivable() {
            return Err(BankError::InterestBeyondAccrued {
                contract: loan.clone(),
                requested: amount,
                accrued: state.accrued_receivable(),
            });
        }
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::LoanInterestCollected,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::LOAN_INT_RCV, PostingSide::Credit, amount),
                ],
            }],
        )?;
        let borrower = self.loan_counterparty(loan)?;
        if let Some(state) = self.loans.get_mut(loan) {
            state.collect_interest(amount)?;
        }
        self.record_flow(
            date,
            &borrower,
            FlowDirection::Inbound,
            amount,
            "loan interest collected",
        )?;
        Ok(event)
    }

    /// 计提全部存款利息至 `through`（非现金：Dr 6411 / Cr 2231）。
    /// 定期存款到期停息（计提日截断至到期日；无自动转存模型）。
    /// 时间回拨 → `AccrualNotForward`；天数 0 自然跳过；零金额计提仍推进
    /// 余数与计提日（守恒所需）。返回实际计提项（按合同 id 稳定顺序）。
    pub fn accrue_deposit_interest(
        &mut self,
        through: CivilDate,
    ) -> Result<Vec<DepositAccrualItem>, BankError> {
        let base = self.next_event_id;
        let mut items = Vec::new();
        let mut entries = Vec::new();
        let states: Vec<(ContractId, DepositState)> = self
            .deposits
            .iter()
            .map(|(id, state)| (id.clone(), state.clone()))
            .collect();
        for (deposit_id, state) in states {
            // 到期停息：有效计提截止 = min(through, maturity)。
            let effective = if through > state.maturity_date() {
                state.maturity_date()
            } else {
                through
            };
            let days = effective.days_since(state.last_accrual_date());
            if days < 0 {
                return Err(BankError::AccrualNotForward {
                    contract: deposit_id,
                    through,
                    last_accrual: state.last_accrual_date(),
                });
            }
            if days == 0 {
                continue;
            }
            let (amount, remaining) =
                accrue_act_365f(state.principal(), state.rate_bp(), days, state.carried())?;
            let event = BusinessEventId::new(base + items.len() as u64);
            if amount.is_positive() {
                entries.push(JournalEntry {
                    source: event,
                    date: through,
                    kind: BusinessKind::DepositInterestAccrued,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(chart::acct::INTEREST_EXPENSE, PostingSide::Debit, amount),
                        super::line(chart::acct::DEP_INT_PAYABLE, PostingSide::Credit, amount),
                    ],
                });
            }
            items.push(DepositAccrualItem {
                deposit: deposit_id,
                days,
                amount,
                remaining_carried: remaining,
                accrued_through: effective,
            });
        }
        self.post_with_commit(base + items.len() as u64, entries)?;
        for item in &items {
            if let Some(state) = self.deposits.get_mut(&item.deposit) {
                state.apply_accrual(item.amount, item.remaining_carried, item.accrued_through)?;
            }
        }
        Ok(items)
    }

    /// 支付已提存款利息（全额结清 2231）：Dr 2231 / Cr 1003（经营）。
    /// 无已提利息 → `NothingAccrued`；资金不足 → `PaymentFailed`。
    pub fn pay_deposit_interest(
        &mut self,
        deposit: &ContractId,
        date: CivilDate,
    ) -> Result<BusinessEventId, BankError> {
        let state = self.deposit(deposit).ok_or(BankError::UnknownDeposit {
            contract: deposit.clone(),
        })?;
        if state.accrued_payable().is_zero() {
            return Err(BankError::NothingAccrued {
                contract: deposit.clone(),
            });
        }
        let amount = state.accrued_payable();
        let depositor = state.counterparty().clone();
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::DepositInterestPaid,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::DEP_INT_PAYABLE, PostingSide::Debit, amount),
                    super::line(chart::acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        if let Some(state) = self.deposits.get_mut(deposit) {
            state.settle_accrued();
        }
        self.record_flow(
            date,
            &depositor,
            FlowDirection::Outbound,
            amount,
            "deposit interest paid",
        )?;
        Ok(event)
    }
}
