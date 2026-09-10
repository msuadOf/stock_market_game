//! 借款偿付处理器（K3 工商）：付息与还本（计提侧在 `interest.rs`、状态/数学
//! 在 `loans.rs`）。付息/还本属**筹资活动**现金（CAS 31 口径：偿付利息在筹资
//! 列报）；还本前自动计提至还本日（避免已用本金漏计利息）。资金不足 →
//! `PaymentFailed`（公司继续运行，不透支不补钱）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::industrial::loans::{apply_accrual, loan_account, LoanState, RepaymentOutcome};
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

impl IndustrialBooks {
    /// 付息（单合同、全额已提未付）：Dr 应付利息 / Cr 现金（筹资活动）。
    /// 无已提利息 → `NothingAccrued`；资金不足 → `PaymentFailed`。
    pub fn pay_interest(
        &mut self,
        contract: &ContractId,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        let state = self.loan(contract).ok_or(IndustrialError::UnknownLoan {
            contract: contract.clone(),
        })?;
        if state.accrued_unpaid().is_zero() {
            return Err(IndustrialError::NothingAccrued {
                contract: contract.clone(),
            });
        }
        let amount = state.accrued_unpaid();
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InterestPayment,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    super::line(chart::acct::INT_PAYABLE, PostingSide::Debit, amount),
                    super::line(chart::acct::BANK, PostingSide::Credit, amount),
                ],
            }],
        )?;
        let lender = self.contract_counterparty(contract)?;
        if let Some(state) = self.loans_mut().get_mut(contract) {
            state.settle_accrued();
        }
        self.counterparties_mut().record_flow(super::flow(
            date,
            &lender,
            FlowDirection::Outbound,
            amount,
            "interest payment",
        ))?;
        Ok(event)
    }

    /// 还本：先自动计提该合同至 `date`（非现金），再 Dr 2001|2501 / Cr 现金
    /// （筹资活动）。金额超未偿本金 → 类型化拒绝；资金不足 → `PaymentFailed`。
    pub fn repay_principal(
        &mut self,
        contract: &ContractId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<RepaymentOutcome, IndustrialError> {
        let state = self.loan(contract).ok_or(IndustrialError::UnknownLoan {
            contract: contract.clone(),
        })?;
        if !amount.is_positive() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "principal repayment",
                amount,
            });
        }
        if amount > state.outstanding() {
            return Err(IndustrialError::PrincipalBeyondOutstanding {
                contract: contract.clone(),
                requested: amount,
                outstanding: state.outstanding(),
            });
        }
        let accrual = self.accrue_loan(contract, state, date)?;
        let account = self.loan_account_of(contract);

        let base = self.next_event_id;
        let mut events = Vec::new();
        let mut entries = Vec::new();
        let mut next_id = base;
        if let Some(item) = &accrual {
            if item.amount.is_positive() {
                let event = BusinessEventId::new(next_id);
                next_id += 1;
                entries.push(JournalEntry {
                    source: event,
                    date,
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(chart::acct::FIN_EXP, PostingSide::Debit, item.amount),
                        super::line(chart::acct::INT_PAYABLE, PostingSide::Credit, item.amount),
                    ],
                });
                events.push(event);
            }
        }
        let repay_event = BusinessEventId::new(next_id);
        next_id += 1;
        entries.push(JournalEntry {
            source: repay_event,
            date,
            kind: BusinessKind::LoanRepayment,
            cash_flow: CashFlowClass::Financing,
            lines: vec![
                super::line(account, PostingSide::Debit, amount),
                super::line(chart::acct::BANK, PostingSide::Credit, amount),
            ],
        });
        events.push(repay_event);
        self.post_with_commit(next_id, entries)?;
        if let Some(item) = accrual {
            apply_accrual(self.loans_mut(), &item, date)?;
        }
        if let Some(state) = self.loans_mut().get_mut(contract) {
            state.repay(amount)?;
        }
        let lender = self.contract_counterparty(contract)?;
        self.counterparties_mut().record_flow(super::flow(
            date,
            &lender,
            FlowDirection::Outbound,
            amount,
            "principal repayment",
        ))?;
        Ok(RepaymentOutcome { events })
    }

    pub(super) fn contract_counterparty(
        &self,
        contract: &ContractId,
    ) -> Result<CounterpartyId, IndustrialError> {
        self.contracts()
            .get(contract)
            .map(|c| c.counterparty.clone())
            .ok_or(IndustrialError::UnknownLoan {
                contract: contract.clone(),
            })
    }

    /// 借款的过账科目：开局隐式合同恒为 2001（构造时已与 2001 贷方余额精确
    /// 对账）；其余合同按期限（≤ 365 天 → 2001，否则 2501）。
    pub(super) fn loan_account_of(&self, contract: &ContractId) -> &'static str {
        if contract.0 == crate::company::industrial::OPENING_DEBT_CONTRACT_ID {
            return chart::acct::ST_DEBT;
        }
        let contract = self
            .contracts()
            .get(contract)
            .expect("loan state implies registered contract");
        loan_account(contract.start_date, contract.maturity_date)
    }

    /// 单合同计提预览（纯读；天数 ≤ 0 → None/错误）。计提/还本共用。
    pub(super) fn accrue_loan(
        &self,
        contract: &ContractId,
        state: &LoanState,
        through: CivilDate,
    ) -> Result<Option<crate::company::industrial::InterestAccrualItem>, IndustrialError> {
        let days = through.days_since(state.last_accrual_date());
        if days < 0 {
            return Err(IndustrialError::AccrualNotForward {
                contract: contract.clone(),
                through,
                last_accrual: state.last_accrual_date(),
            });
        }
        if days == 0 {
            return Ok(None);
        }
        let rate_bp = self
            .contracts()
            .get(contract)
            .map(|c| c.annual_rate_bp)
            .expect("loan state implies registered contract");
        let (amount, remaining_carried) = crate::company::industrial::loans::accrue_act_365f(
            state.outstanding(),
            rate_bp,
            days,
            state.carried(),
        )?;
        Ok(Some(crate::company::industrial::InterestAccrualItem {
            contract: contract.clone(),
            days,
            amount,
            remaining_carried,
        }))
    }
}
