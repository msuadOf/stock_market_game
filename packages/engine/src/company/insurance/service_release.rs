//! 责任单元释放处理器（K3 保险，任务 10；CAS 25 §29–§32）。
//!
//! 保险服务收入按责任单元（保障天数）释放 = 预期赔付分量 + 风险调整分量 +
//! CSM 分量（亏损组无 CSM 分量）；保险财务损益 = 贴现差 F 的当期回拨
//! （Dr 6541 / Cr 2501）。两笔分录同一事件原子过账；逐组件 rhe 分摊 +
//! 余数链守恒；末批（单元用尽）精确清零（不弃尾差）。
//!
//! 简化登记：亏损成分不循环计入收入（见 groups.rs 头注）；收入释放跟随
//! 责任单元而非赔案事件时点（§31 挣得口径的单元法）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::insurance::csm::unit_release;
use crate::company::insurance::groups::ReleaseBatch;
use crate::company::insurance::{chart, InsuranceBooks, InsuranceError};

impl InsuranceBooks {
    /// 释放一批责任单元（过账成功才推进子账）：
    /// 1. 守卫：单元为正、不越剩余保障；
    /// 2. 逐组件分摊（claims/RA/CSM/财务；末批精确清零；亏损成分备查随比例）；
    /// 3. 过账：收入分录（Dr 2501 / Cr 6051）+ 财务分录（Dr 6541 / Cr 2501），
    ///    零金额分录跳过；两笔都为零 → `Ok(None)`（事件 id 不复用）。
    pub fn release_service(
        &mut self,
        group: &ContractId,
        units: i64,
        date: CivilDate,
    ) -> Result<Option<BusinessEventId>, InsuranceError> {
        if units <= 0 {
            return Err(InsuranceError::NonPositiveServiceUnits { units });
        }
        let state = self.group(group).ok_or(InsuranceError::UnknownGroup {
            group: group.clone(),
        })?;
        let remaining_units = state.units_remaining();
        if units > remaining_units {
            return Err(InsuranceError::ServiceUnitsBeyondCoverage {
                group: group.clone(),
                requested: units,
                remaining: remaining_units,
            });
        }
        let final_batch = units == remaining_units;
        let units_total = state.units_total();
        let (claims, carried_claims) = unit_release(
            state.expected_claims_remaining().cents(),
            units,
            units_total,
            state.carried_claims(),
            final_batch,
        )?;
        let (risk_adjustment, carried_ra) = unit_release(
            state.risk_adjustment_remaining().cents(),
            units,
            units_total,
            state.carried_risk_adjustment(),
            final_batch,
        )?;
        let (csm, carried_csm) = if state.csm().is_positive() {
            unit_release(
                state.csm().cents(),
                units,
                units_total,
                state.carried_csm(),
                final_batch,
            )?
        } else {
            (0, state.carried_csm())
        };
        let (finance, carried_finance) = unit_release(
            state.finance_remaining().cents(),
            units,
            units_total,
            state.carried_finance(),
            final_batch,
        )?;
        let (loss_memo, carried_loss) = if state.loss_component().is_positive() {
            unit_release(
                state.loss_component().cents(),
                units,
                units_total,
                state.carried_loss(),
                final_batch,
            )?
        } else {
            (0, state.carried_loss())
        };
        let revenue = claims + risk_adjustment + csm;
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let mut entries = Vec::with_capacity(2);
        if revenue > 0 {
            entries.push(JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InsuranceServiceRevenue,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(
                        chart::acct::LRC,
                        PostingSide::Debit,
                        AccountingAmount::from_cents(revenue),
                    ),
                    super::line(
                        chart::acct::INSURANCE_REVENUE,
                        PostingSide::Credit,
                        AccountingAmount::from_cents(revenue),
                    ),
                ],
            });
        }
        if finance > 0 {
            entries.push(JournalEntry {
                source: BusinessEventId::new(base + 1),
                date,
                kind: BusinessKind::InsuranceFinance,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(
                        chart::acct::INSURANCE_FINANCE,
                        PostingSide::Debit,
                        AccountingAmount::from_cents(finance),
                    ),
                    super::line(
                        chart::acct::LRC,
                        PostingSide::Credit,
                        AccountingAmount::from_cents(finance),
                    ),
                ],
            });
        }
        let posted = if entries.is_empty() {
            None
        } else {
            Some(event)
        };
        // 事件 id 槽位按本事件最多消耗数推进（与 ecl.rs 同语义：零过账也消耗，
        // 恒单调、不复用）。
        self.post_with_commit(base + 2, entries)?;
        if let Some(state) = self.groups.get_mut(group) {
            state.apply_release(ReleaseBatch {
                claims,
                risk_adjustment,
                csm,
                finance,
                loss_memo,
                units,
                carried_claims,
                carried_risk_adjustment: carried_ra,
                carried_csm,
                carried_finance,
                carried_loss,
            })?;
        }
        Ok(posted)
    }
}
