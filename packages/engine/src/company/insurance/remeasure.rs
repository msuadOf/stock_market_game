//! 期末重估处理器（K3 保险，任务 10；CAS 25 §29(b)/§33/§34/§46–§49）。
//!
//! 估计改变的三向分流（`ΔE` = 剩余预期赔付变动、`ΔPV` = 剩余期限贴现值）：
//! - **CSM 再计量**：盈利组吸收 ΔPV（组合成分变化，**不过账**）；
//! - **保险财务损益**：贴现差分量 ΔF = ΔE − ΔPV 立即过账（Dr/Cr 6541）；
//! - **亏损成分**：CSM 耗尽后的加重 / 亏损组转回即期过账（Dr/Cr 6451，
//!   CAS 25 §46–§49；全额转回后盈余转入 CSM）。
//!
//! 组合恒等式在重估后保持（LRC 余额变动 = ΔF + 亏损分量；测试逐点断言）。

use crate::accounting::{
    AccountingAmount, AccountingError, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::insurance::{chart, InsuranceBooks, InsuranceError};

impl InsuranceBooks {
    /// 重估剩余预期赔付（`new_remaining_claims`，不折现口径）。过账成功才
    /// 推进子账；零变动（ΔE = 0）→ `Ok(None)`。
    pub fn remeasure(
        &mut self,
        group: &ContractId,
        date: CivilDate,
        new_remaining_claims: AccountingAmount,
    ) -> Result<Option<BusinessEventId>, InsuranceError> {
        if new_remaining_claims.is_negative() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "remaining claims estimate",
                amount: new_remaining_claims,
            });
        }
        let state = self.group(group).ok_or(InsuranceError::UnknownGroup {
            group: group.clone(),
        })?;
        if state.units_remaining() == 0 && new_remaining_claims.is_positive() {
            return Err(InsuranceError::NoRemainingCoverage {
                group: group.clone(),
                estimate: new_remaining_claims,
            });
        }
        let Some(delta) = state
            .measurement()
            .preview_remeasure(new_remaining_claims, self.discount.rate_bp)?
        else {
            self.next_event_id += 1; // 槽位照常消耗（ecl.rs 同语义，恒单调）。
            return Ok(None);
        };
        let delta_finance = delta.finance_posted.cents();
        let loss_posted = delta.loss_posted;
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let mut entries = Vec::with_capacity(2);
        if delta_finance != 0 {
            let magnitude = AccountingAmount::from_cents(checked_abs(delta_finance)?);
            let (finance_side, lrc_side) = if delta_finance > 0 {
                (PostingSide::Debit, PostingSide::Credit)
            } else {
                (PostingSide::Credit, PostingSide::Debit)
            };
            entries.push(JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InsuranceFinance,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::INSURANCE_FINANCE, finance_side, magnitude),
                    super::line(chart::acct::LRC, lrc_side, magnitude),
                ],
            });
        }
        if !loss_posted.is_zero() {
            let magnitude = if loss_posted.is_positive() {
                loss_posted
            } else {
                loss_posted.neg()?
            };
            let (loss_side, lrc_side) = if loss_posted.is_positive() {
                (PostingSide::Debit, PostingSide::Credit)
            } else {
                (PostingSide::Credit, PostingSide::Debit)
            };
            entries.push(JournalEntry {
                source: BusinessEventId::new(base + 1),
                date,
                kind: BusinessKind::InsuranceLossComponent,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::INSURANCE_EXPENSE, loss_side, magnitude),
                    super::line(chart::acct::LRC, lrc_side, magnitude),
                ],
            });
        }
        let posted = if entries.is_empty() {
            None
        } else {
            Some(event)
        };
        self.post_with_commit(base + 2, entries)?;
        if let Some(state) = self.groups.get_mut(group) {
            state.measurement_mut().apply_remeasure(delta)?;
        }
        Ok(posted)
    }
}

/// i128 绝对值（i128::MIN 无对应正数 → 类型化溢出拒绝，不 panic）。
fn checked_abs(value: i128) -> Result<i128, InsuranceError> {
    value.checked_abs().ok_or(InsuranceError::Accounting(
        AccountingError::AmountOverflow {
            op: "abs",
            detail: value.to_string(),
        },
    ))
}
