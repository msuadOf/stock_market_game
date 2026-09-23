//! 借款费用处理器（K3 地产）：项目借款（授信约束）与计提（资本化/费用化
//! 双链拆分——窗口判定见 `projects.rs`）。付息/还本在 `debt_service.rs`。
//!
//! 分录：资本化 Dr 1541 / Cr 2231（非现金，游戏假设政策）；费用化
//! Dr 6603 / Cr 2231（非现金）。
//!
//! 红线（K3）：**不无限资本化**——完工后/窗口外的利息必须进损益；
//! 未指定项目的借款恒费用化。

use std::collections::BTreeMap;

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::{ContractId, ContractRole, DayCountBasis, OperatingContract};
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::real_estate::loans::{
    accrue_act_365f, apply_split, loan_account, InterestSplitItem, ProjectLoanState,
};
use crate::company::real_estate::projects::ProjectId;
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

impl RealEstateBooks {
    /// 项目借款：校验（对手方已登记、合同条款 + id 唯一、指定项目存在、
    /// 授信容量）→ Dr 现金 / Cr 2001|2501（筹资）→ 登记借款状态。
    #[allow(clippy::too_many_arguments)]
    pub fn borrow_project_loan(
        &mut self,
        contract: ContractId,
        lender: &CounterpartyId,
        principal: AccountingAmount,
        annual_rate_bp: i32,
        start: CivilDate,
        maturity: CivilDate,
        project: Option<ProjectId>,
    ) -> Result<BusinessEventId, RealEstateError> {
        self.ensure_counterparty(lender)?;
        let loan_contract = OperatingContract {
            id: contract.clone(),
            role: ContractRole::Borrowing,
            counterparty: lender.clone(),
            principal,
            annual_rate_bp,
            start_date: start,
            maturity_date: maturity,
            basis: DayCountBasis::Act365F,
        };
        loan_contract.validate()?;
        if self.loans_map().contains_key(&contract) {
            return Err(RealEstateError::DuplicateContract {
                contract: contract.clone(),
            });
        }
        if let Some(project_id) = &project {
            if self.project(project_id).is_none() {
                return Err(RealEstateError::UnknownProject {
                    project: project_id.clone(),
                });
            }
        }
        let outstanding = self.loans_outstanding_total()?;
        if let Some(limit) = self.budget().credit_line(lender) {
            let projected = outstanding.add(principal)?;
            if projected > limit {
                return Err(RealEstateError::DebtBeyondCreditLine {
                    lender: lender.clone(),
                    outstanding,
                    requested: principal,
                    limit,
                });
            }
        } else {
            return Err(RealEstateError::NoCreditLine {
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
                    super::line(chart::acct::CASH, PostingSide::Debit, principal),
                    super::line(account, PostingSide::Credit, principal),
                ],
            }],
        )?;
        self.loans_map_mut().insert(
            contract.clone(),
            ProjectLoanState::new(
                principal,
                annual_rate_bp,
                start,
                lender.clone(),
                project,
                account,
            ),
        );
        self.record_flow(
            start,
            lender,
            FlowDirection::Inbound,
            principal,
            "project loan disbursement",
        )?;
        Ok(event)
    }

    /// 计提全部借款利息至 `through`，按各借款指定项目的资本化窗口把天数
    /// 拆分为资本化/费用化两链（时间回拨 → 类型化拒绝；天数 0 的合同自然
    /// 跳过）。零金额链仍推进其余数（守恒所需），不产生分录。
    pub fn accrue_interest(
        &mut self,
        through: CivilDate,
    ) -> Result<Vec<InterestSplitItem>, RealEstateError> {
        let base = self.next_event_id;
        let min_days = self.capitalization_policy().suspension_min_days;
        let loans: Vec<(ContractId, ProjectLoanState)> = self
            .loans_map()
            .iter()
            .map(|(id, state)| (id.clone(), state.clone()))
            .collect();
        let mut items = Vec::new();
        let mut entries = Vec::new();
        for (contract_id, state) in loans {
            if through < state.last_accrual_date() {
                return Err(RealEstateError::AccrualNotForward {
                    contract: contract_id,
                    through,
                    last_accrual: state.last_accrual_date(),
                });
            }
            let days = through.days_since(state.last_accrual_date());
            if days == 0 {
                continue;
            }
            let cap_days = match state.project() {
                Some(project_id) => {
                    let project = self.project(project_id).ok_or_else(|| {
                        RealEstateError::UnknownProject {
                            project: project_id.clone(),
                        }
                    })?;
                    project.capitalizable_days(state.last_accrual_date(), through, min_days)?
                }
                None => 0,
            };
            let exp_days = days - cap_days;
            let (capitalized_amount, carried_cap) = if cap_days > 0 {
                accrue_act_365f(
                    state.outstanding(),
                    state.annual_rate_bp(),
                    cap_days,
                    state.carried_cap(),
                )?
            } else {
                (AccountingAmount::ZERO, state.carried_cap())
            };
            let (expensed_amount, carried_exp) = if exp_days > 0 {
                accrue_act_365f(
                    state.outstanding(),
                    state.annual_rate_bp(),
                    exp_days,
                    state.carried_exp(),
                )?
            } else {
                (AccountingAmount::ZERO, state.carried_exp())
            };
            let item = InterestSplitItem {
                contract: contract_id.clone(),
                days,
                capitalized_days: cap_days,
                expensed_days: exp_days,
                capitalized_amount,
                expensed_amount,
                carried_cap,
                carried_exp,
            };
            if capitalized_amount.is_positive() {
                entries.push(JournalEntry {
                    source: BusinessEventId::new(base + entries.len() as u64),
                    date: through,
                    kind: BusinessKind::BorrowingCostCapitalized,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(
                            chart::acct::DEV_INVENTORY,
                            PostingSide::Debit,
                            capitalized_amount,
                        ),
                        super::line(
                            chart::acct::INT_PAYABLE,
                            PostingSide::Credit,
                            capitalized_amount,
                        ),
                    ],
                });
            }
            if expensed_amount.is_positive() {
                entries.push(JournalEntry {
                    source: BusinessEventId::new(base + entries.len() as u64),
                    date: through,
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(chart::acct::FIN_EXP, PostingSide::Debit, expensed_amount),
                        super::line(
                            chart::acct::INT_PAYABLE,
                            PostingSide::Credit,
                            expensed_amount,
                        ),
                    ],
                });
            }
            items.push(item);
        }
        self.post_with_commit(base + entries.len() as u64, entries)?;
        for item in &items {
            apply_split(self.loans_map_mut(), item, through)?;
        }
        let mut capital_by_project: BTreeMap<ProjectId, AccountingAmount> = BTreeMap::new();
        for item in &items {
            if !item.capitalized_amount.is_positive() {
                continue;
            }
            if let Some(state) = self.loans_map().get(&item.contract) {
                if let Some(project_id) = state.project() {
                    let total = capital_by_project
                        .entry(project_id.clone())
                        .or_insert(AccountingAmount::ZERO);
                    *total = total.add(item.capitalized_amount)?;
                }
            }
        }
        for (project_id, total) in capital_by_project {
            self.projects_mut()
                .get_mut(&project_id)
                .expect("validated at borrow")
                .add_capitalized_interest(total);
        }
        Ok(items)
    }
}
