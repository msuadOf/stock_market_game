//! 赔案子账与处理器（K3 保险，任务 10；CAS 25 §28 已发生赔款负债）。
//!
//! 赔案**发生**（Dr 保险服务费用 / Cr 已发生赔款负债——实际金额，权责发生）
//! 与**支付**（Dr 已发生赔款负债 / Cr 银行存款——现金出）是两个独立事件；
//! 支付超未付余额 → `ClaimPaymentBeyondOutstanding`，超可支付现金 →
//! `PaymentFailed`（K2 客户流动性约束）。现金流分类：赔款支付 = 经营活动。
//!
//! 简化登记：预期赔付的释放按责任单元推进（挣得口径），赔案发生不自动改写
//! 剩余预期（差异经显式重估事件分流——remeasure.rs）。

use crate::accounting::{
    AccountingAmount, AccountingError, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::FlowDirection;
use crate::company::insurance::{chart, InsuranceBooks, InsuranceError};

/// 赔案 id newtype（组内唯一；随存档序列化）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct ClaimId(pub String);

/// 单个赔案状态：发生额（已确认负债）、已支付、发生日期。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ClaimState {
    incurred: AccountingAmount,
    paid: AccountingAmount,
    date_incurred: CivilDate,
}

impl ClaimState {
    pub(super) fn new(incurred: AccountingAmount, date: CivilDate) -> Self {
        Self {
            incurred,
            paid: AccountingAmount::ZERO,
            date_incurred: date,
        }
    }

    pub fn incurred(&self) -> AccountingAmount {
        self.incurred
    }

    pub fn paid(&self) -> AccountingAmount {
        self.paid
    }

    /// 未付余额 = 发生 − 已支付（构造不变量下恒 ≥ 0）。
    pub fn unpaid(&self) -> Result<AccountingAmount, AccountingError> {
        self.incurred.sub(self.paid)
    }

    pub fn date_incurred(&self) -> CivilDate {
        self.date_incurred
    }

    pub(super) fn apply_payment(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.paid = self.paid.add(amount)?;
        Ok(())
    }
}

impl InsuranceBooks {
    /// 赔案发生（负债确认，不动现金）：Dr 6451 / Cr 2502（非现金）。
    pub fn record_claim(
        &mut self,
        group: &ContractId,
        claim: ClaimId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, InsuranceError> {
        if !amount.is_positive() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "claim",
                amount,
            });
        }
        if self.group(group).is_none() {
            return Err(InsuranceError::UnknownGroup {
                group: group.clone(),
            });
        }
        if self
            .group(group)
            .and_then(|state| state.claim(&claim))
            .is_some()
        {
            return Err(InsuranceError::DuplicateClaim {
                group: group.clone(),
                claim,
            });
        }
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InsuranceClaimIncurred,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::INSURANCE_EXPENSE, PostingSide::Debit, amount),
                    super::line(chart::acct::LIC, PostingSide::Credit, amount),
                ],
            }],
        )?;
        if let Some(state) = self.groups.get_mut(group) {
            state.insert_claim(claim, ClaimState::new(amount, date));
        }
        Ok(event)
    }

    /// 赔款支付（现金出，负债降，不重复计费用）：Dr 2502 / Cr 1002（经营）。
    /// 超未付余额 → `ClaimPaymentBeyondOutstanding`；超可支付现金 →
    /// `PaymentFailed`（负现金禁令，险企继续运行——K2 不透支不补钱）。
    pub fn pay_claim(
        &mut self,
        group: &ContractId,
        claim: &ClaimId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, InsuranceError> {
        let state = self.group(group).ok_or(InsuranceError::UnknownGroup {
            group: group.clone(),
        })?;
        let claim_state = state.claim(claim).ok_or(InsuranceError::UnknownClaim {
            group: group.clone(),
            claim: claim.clone(),
        })?;
        if !amount.is_positive() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "claim payment",
                amount,
            });
        }
        let outstanding = claim_state.unpaid()?;
        if amount > outstanding {
            return Err(InsuranceError::ClaimPaymentBeyondOutstanding {
                claim: claim.clone(),
                requested: amount,
                outstanding,
            });
        }
        let policyholder = state.policyholder().clone();
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InsuranceClaimPaid,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::LIC, PostingSide::Debit, amount),
                    super::line(chart::acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        if let Some(state) = self.groups.get_mut(group) {
            if let Some(applied) = state.apply_claim_payment(claim, amount) {
                applied?;
            }
        }
        self.record_flow(
            date,
            &policyholder,
            FlowDirection::Outbound,
            amount,
            "claim payment",
        )?;
        Ok(event)
    }
}
