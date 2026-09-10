//! 银行日常经营流（K4）：外部存入、贷款发放（到期事件排队）、手续费收入、
//! 信用恶化驱动的 ECL 重估。
//!
//! **跨行业红线**：存贷/手续费流不读商品需求/成本字段（K4 明文——银行存款
//! 流不受商品需求冲击影响）；银行响应的经济字段是信用参数（ECL 重估）。

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::bank::{BankBooks, BankError, BankProductKind, EclScenario, EclStage};
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;
use crate::company::events::ActiveShock;
use crate::company::operations::core::PaymentFailureRecord;
use crate::company::operations::error::OperationsError;
use crate::company::operations::state::{add_days, EconomyAggregates};
use crate::company::scheduler::{OperatingScheduler, ScheduledAction, SchedulerRequest};
use crate::company::spec::CompanyId;

/// 银行经营流参数（显式配置；PD/LGD 情景为 Fixture 游戏假设）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct BankFlowParams {
    pub depositor: CounterpartyId,
    pub borrower: CounterpartyId,
    pub fee_customer: CounterpartyId,
    pub deposit_principal: AccountingAmount,
    pub deposit_rate_bp: i32,
    pub deposit_term_days: i64,
    /// 每隔 N 个经营日吸收一笔新存款（确定性日程，非随机）。
    pub deposit_every_days: i64,
    pub loan_principal: AccountingAmount,
    pub loan_rate_bp: i32,
    pub loan_term_days: i64,
    /// 每隔 N 个经营日发放一笔新贷款。
    pub lending_every_days: i64,
    pub daily_fee_income: AccountingAmount,
    /// 信用恶化事件驱动的一次性压力重估情景（Σ权重 = 10000bp）。
    pub credit_deterioration_scenarios: Vec<EclScenario>,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::company::operations) fn advance_day(
    company: CompanyId,
    books: &mut BankBooks,
    params: &BankFlowParams,
    date: CivilDate,
    aggregates: EconomyAggregates,
    newly: &[ActiveShock],
    scheduler: &mut OperatingScheduler,
    failures: &mut Vec<PaymentFailureRecord>,
    next_flow_seq: &mut i64,
) -> Result<(), OperationsError> {
    let day_seq = *next_flow_seq;
    *next_flow_seq += 1;
    let scheduled = |every: i64| every >= 1 && (day_seq - 1) % every == 0;
    // 1. 吸收定期存款（负债，不是收入——处理器红线）。
    if scheduled(params.deposit_every_days) {
        let maturity = add_days(date, params.deposit_term_days)?;
        books.accept_deposit(
            BankProductKind::TermDeposit,
            ContractId(format!("DEP-{day_seq}")),
            &params.depositor,
            params.deposit_principal,
            params.deposit_rate_bp,
            date,
            maturity,
        )?;
    }

    // 2. 发放定期贷款（资产；现金不足 = 失败记录，银行继续运行）+ 到期排队。
    if scheduled(params.lending_every_days) {
        let maturity = add_days(date, params.loan_term_days)?;
        let loan = ContractId(format!("LN-{day_seq}"));
        if let Err(error) = books.issue_loan(
            BankProductKind::TermLoan,
            loan.clone(),
            &params.borrower,
            params.loan_principal,
            params.loan_rate_bp,
            date,
            maturity,
        ) {
            if matches!(error, BankError::PaymentFailed { .. }) {
                failures.push(PaymentFailureRecord {
                    company: company.clone(),
                    what: "loan disbursement".to_string(),
                    amount: params.loan_principal,
                });
            } else {
                return Err(error.into());
            }
        } else {
            scheduler.submit(SchedulerRequest::Due {
                key: format!("MAT:{}:{}", company.0, loan.0),
                due_date: maturity,
                action: ScheduledAction::ContractMaturity {
                    company: company.clone(),
                    reference: format!("LN:{}", loan.0),
                },
            })?;
        }
    }

    // 3. 手续费收入（服务履约收现）。
    books.earn_fee(&params.fee_customer, params.daily_fee_income, date)?;

    // 4. 信用恶化：对全部在库贷款做压力情景重估（阶段二；幂等目标化差额）。
    //    仅在恶化事件激活当日执行（事件驱动，不是每日重抽）。
    let deteriorated = newly.iter().any(|shock| {
        matches!(
            shock.kind,
            crate::company::events::ShockKind::CreditDeterioration
        )
    }) || aggregates.credit_risk_add_bp > 0;
    if deteriorated {
        let loans: Vec<ContractId> = books.loans().map(|(id, _)| id.clone()).collect();
        for loan in loans {
            books.assess_credit(
                &loan,
                date,
                EclStage::Stage2,
                "customer credit deterioration event",
                params.credit_deterioration_scenarios.clone(),
            )?;
        }
    }
    Ok(())
}
