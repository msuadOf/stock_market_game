//! 借款与计息处理器（K3 工商）：授信借款、计提（偿付侧在 `repayment.rs`、
//! 状态与 ACT/365F 数学在 `loans.rs`）。
//!
//! 计提为非现金；授信（K2）：新借款必须落在 `限额 − Σ未偿本金（含开局隐式
//! 合同）` 内——无授信/超授信均类型化拒绝，不允许无限信用兜底。短期
//! （≤365 天）记 2001、长期记 2501；开局隐式合同恒记 2001（构造时与 2001
//! 余额精确对账）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::{ContractId, DayCountBasis, OperatingContract};
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::industrial::loans::{
    apply_accrual, loan_account, InterestAccrualItem, LoanState,
};
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

impl IndustrialBooks {
    /// 授信借款：三重校验（对手方已登记、合同数据合法 + id 唯一、授信容量含
    /// 开局债务）→ Dr 现金 / Cr 2001|2501（筹资）→ 登记合同与借款状态。
    #[allow(clippy::too_many_arguments)]
    pub fn borrow(
        &mut self,
        contract_id: ContractId,
        lender: &CounterpartyId,
        principal: AccountingAmount,
        annual_rate_bp: i32,
        start: CivilDate,
        maturity: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        if self.counterparties().get(lender).is_none() {
            return Err(IndustrialError::Company(
                crate::company::CompanyError::UnknownCounterparty {
                    counterparty: lender.clone(),
                },
            ));
        }
        let contract = OperatingContract {
            id: contract_id.clone(),
            role: crate::company::ContractRole::Borrowing,
            counterparty: lender.clone(),
            principal,
            annual_rate_bp,
            start_date: start,
            maturity_date: maturity,
            basis: DayCountBasis::Act365F,
        };
        contract.validate()?;
        if self.contracts().get(&contract_id).is_some() {
            return Err(IndustrialError::Company(
                crate::company::CompanyError::DuplicateContract {
                    contract: contract_id,
                },
            ));
        }
        let outstanding = self.loans_outstanding_total()?;
        if let Some(limit) = self.budget().credit_line(lender) {
            let projected = outstanding.add(principal)?;
            if projected > limit {
                return Err(IndustrialError::DebtBeyondCreditLine {
                    lender: lender.clone(),
                    outstanding,
                    requested: principal,
                    limit,
                });
            }
        } else {
            return Err(IndustrialError::NoCreditLine {
                lender: lender.clone(),
                requested: principal,
            });
        }

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let account = loan_account(start, maturity);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date: start,
                kind: BusinessKind::LoanDisbursement,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    super::line(chart::acct::BANK, PostingSide::Debit, principal),
                    super::line(account, PostingSide::Credit, principal),
                ],
            }],
        )?;
        self.contracts_mut().register(contract)?;
        self.loans_mut()
            .insert(contract_id, LoanState::new(principal, start));
        self.counterparties_mut().record_flow(super::flow(
            start,
            lender,
            FlowDirection::Inbound,
            principal,
            "loan disbursement",
        ))?;
        Ok(event)
    }

    /// 计提全部借款利息至 `through`（时间回拨 → 类型化拒绝；天数 0 的合同
    /// 自然跳过）。零金额但正天数的计提推进余数与计提日（守恒所需），不产生
    /// 分录；返回实际计提项（按合同 id 稳定顺序）。
    pub fn accrue_interest(
        &mut self,
        through: CivilDate,
    ) -> Result<Vec<InterestAccrualItem>, IndustrialError> {
        let base = self.next_event_id;
        let mut items = Vec::new();
        let mut entries = Vec::new();
        let loans: Vec<(ContractId, LoanState)> = self
            .loans()
            .map(|(id, state)| (id.clone(), state.clone()))
            .collect();
        for (contract_id, state) in loans {
            let Some(item) = self.accrue_loan(&contract_id, &state, through)? else {
                continue;
            };
            let event = BusinessEventId::new(base + items.len() as u64);
            if item.amount.is_positive() {
                entries.push(JournalEntry {
                    source: event,
                    date: through,
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(chart::acct::FIN_EXP, PostingSide::Debit, item.amount),
                        super::line(chart::acct::INT_PAYABLE, PostingSide::Credit, item.amount),
                    ],
                });
            }
            items.push(item);
        }
        self.post_with_commit(base + items.len() as u64, entries)?;
        for item in &items {
            apply_accrual(self.loans_mut(), item, through)?;
        }
        Ok(items)
    }
}
