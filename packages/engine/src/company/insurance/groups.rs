//! 合同组子账状态（K3 保险，任务 10；CAS 25 §11/§12/§20 合同分组）。
//!
//! 组合恒等式（过账不变量，测试逐点断言）：
//! - 盈利组：LRC 余额 = `expected_claims_remaining + risk_adjustment_remaining
//!   + csm − finance_remaining`；
//! - 亏损组（CSM 为零）：LRC 余额 = `expected_claims_remaining +
//!   risk_adjustment_remaining − finance_remaining`；亏损成分（`loss_component`）
//!   是**备查组合成分**（CAS 25 §46–§49 披露项），从不加总到余额。
//!
//! 保障期满时全部组件精确清零（末批释放精确清零 + 余数守恒）。
//!
//! 简化登记：亏损成分不循环计入保险服务收入（首日一次入损益，净损益与
//! 循环摊销等价——列报口径简化）；备查亏损随责任单元比例释放归零。

use std::collections::BTreeMap;

use super::claims::{ClaimId, ClaimState};
use crate::accounting::{AccountingAmount, AccountingError, FractionUnits};
use crate::calendar::CivilDate;
use crate::company::counterparty::CounterpartyId;

/// 单个合同组状态：GMM 组件 + 责任单元进度 + 赔案子账 + 调节表累计量。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContractGroupState {
    policyholder: CounterpartyId,
    premium: AccountingAmount,
    premium_collected: AccountingAmount,
    expected_claims_remaining: AccountingAmount,
    risk_adjustment_remaining: AccountingAmount,
    csm: AccountingAmount,
    loss_component: AccountingAmount,
    finance_remaining: AccountingAmount,
    units_total: i64,
    units_released: i64,
    coverage_start: CivilDate,
    coverage_end: CivilDate,
    day_one_loss: AccountingAmount,
    /// 调节表累计：已释放保险服务收入（含预期赔付/风险调整/CSM 释放）。
    released_revenue: AccountingAmount,
    /// 调节表累计：已回拨保险财务损益（贴现释放）。
    released_finance: AccountingAmount,
    /// 调节表累计：重估即时过账的财务分量（Dr 6541 为正）。
    remeasure_finance: AccountingAmount,
    /// 调节表累计：重估亏损成分变动（Dr 6451 为正；转回为负）。
    remeasure_loss: AccountingAmount,
    /// 调节表累计：被 CSM 吸收的再计量绝对量。
    reestimated_csm: AccountingAmount,
    carried_claims: FractionUnits,
    carried_risk_adjustment: FractionUnits,
    carried_csm: FractionUnits,
    carried_finance: FractionUnits,
    carried_loss: FractionUnits,
    claims: BTreeMap<ClaimId, ClaimState>,
}

/// 一批责任单元释放的结果（过账金额 + 新余数 + 单元数；子账一次落地）。
pub(super) struct ReleaseBatch {
    pub claims: i128,
    pub risk_adjustment: i128,
    pub csm: i128,
    pub finance: i128,
    pub loss_memo: i128,
    pub units: i64,
    pub carried_claims: FractionUnits,
    pub carried_risk_adjustment: FractionUnits,
    pub carried_csm: FractionUnits,
    pub carried_finance: FractionUnits,
    pub carried_loss: FractionUnits,
}

impl ContractGroupState {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        policyholder: CounterpartyId,
        premium: AccountingAmount,
        expected_claims: AccountingAmount,
        risk_adjustment: AccountingAmount,
        csm: AccountingAmount,
        loss_component: AccountingAmount,
        finance_remaining: AccountingAmount,
        coverage_start: CivilDate,
        coverage_end: CivilDate,
        units_total: i64,
    ) -> Self {
        Self {
            policyholder,
            premium,
            premium_collected: AccountingAmount::ZERO,
            expected_claims_remaining: expected_claims,
            risk_adjustment_remaining: risk_adjustment,
            csm,
            loss_component,
            finance_remaining,
            units_total,
            units_released: 0,
            coverage_start,
            coverage_end,
            day_one_loss: loss_component,
            released_revenue: AccountingAmount::ZERO,
            released_finance: AccountingAmount::ZERO,
            remeasure_finance: AccountingAmount::ZERO,
            remeasure_loss: AccountingAmount::ZERO,
            reestimated_csm: AccountingAmount::ZERO,
            carried_claims: FractionUnits::ZERO,
            carried_risk_adjustment: FractionUnits::ZERO,
            carried_csm: FractionUnits::ZERO,
            carried_finance: FractionUnits::ZERO,
            carried_loss: FractionUnits::ZERO,
            claims: BTreeMap::new(),
        }
    }

    pub fn policyholder(&self) -> &CounterpartyId {
        &self.policyholder
    }

    pub fn premium(&self) -> AccountingAmount {
        self.premium
    }

    pub fn premium_collected(&self) -> AccountingAmount {
        self.premium_collected
    }

    pub fn premium_outstanding(&self) -> Result<AccountingAmount, AccountingError> {
        self.premium.sub(self.premium_collected)
    }

    pub fn expected_claims_remaining(&self) -> AccountingAmount {
        self.expected_claims_remaining
    }

    pub fn risk_adjustment_remaining(&self) -> AccountingAmount {
        self.risk_adjustment_remaining
    }

    pub fn csm(&self) -> AccountingAmount {
        self.csm
    }

    pub fn loss_component(&self) -> AccountingAmount {
        self.loss_component
    }

    pub fn finance_remaining(&self) -> AccountingAmount {
        self.finance_remaining
    }

    pub fn units_total(&self) -> i64 {
        self.units_total
    }

    pub fn units_released(&self) -> i64 {
        self.units_released
    }

    pub fn units_remaining(&self) -> i64 {
        self.units_total - self.units_released
    }

    pub fn coverage_start(&self) -> CivilDate {
        self.coverage_start
    }

    pub fn coverage_end(&self) -> CivilDate {
        self.coverage_end
    }

    pub fn day_one_loss(&self) -> AccountingAmount {
        self.day_one_loss
    }

    pub fn released_revenue_total(&self) -> AccountingAmount {
        self.released_revenue
    }

    pub fn released_finance_total(&self) -> AccountingAmount {
        self.released_finance
    }

    pub fn remeasure_finance_total(&self) -> AccountingAmount {
        self.remeasure_finance
    }

    pub fn remeasure_loss_total(&self) -> AccountingAmount {
        self.remeasure_loss
    }

    pub fn reestimated_csm_total(&self) -> AccountingAmount {
        self.reestimated_csm
    }

    pub(super) fn carried_claims(&self) -> FractionUnits {
        self.carried_claims
    }

    pub(super) fn carried_risk_adjustment(&self) -> FractionUnits {
        self.carried_risk_adjustment
    }

    pub(super) fn carried_csm(&self) -> FractionUnits {
        self.carried_csm
    }

    pub(super) fn carried_finance(&self) -> FractionUnits {
        self.carried_finance
    }

    pub(super) fn carried_loss(&self) -> FractionUnits {
        self.carried_loss
    }

    pub fn claim(&self, id: &ClaimId) -> Option<&ClaimState> {
        self.claims.get(id)
    }

    pub fn claims(&self) -> impl Iterator<Item = (&ClaimId, &ClaimState)> {
        self.claims.iter()
    }

    pub(super) fn insert_claim(&mut self, id: ClaimId, state: ClaimState) {
        self.claims.insert(id, state);
    }

    /// 赔款支付落地（存在性已由处理器 validate 段保证——bank deposits 同款
    /// validate→apply 约定；单线程内不缺失）。
    pub(super) fn apply_claim_payment(
        &mut self,
        claim: &ClaimId,
        amount: AccountingAmount,
    ) -> Option<Result<(), AccountingError>> {
        self.claims
            .get_mut(claim)
            .map(|state| state.apply_payment(amount))
    }

    pub(super) fn apply_premium_collection(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.premium_collected = self.premium_collected.add(amount)?;
        Ok(())
    }

    /// 落地一批释放（调用方已按组件金额过账成功）。
    pub(super) fn apply_release(&mut self, batch: ReleaseBatch) -> Result<(), AccountingError> {
        let c = AccountingAmount::from_cents(batch.claims);
        let r = AccountingAmount::from_cents(batch.risk_adjustment);
        let m = AccountingAmount::from_cents(batch.csm);
        let f = AccountingAmount::from_cents(batch.finance);
        let revenue = c.add(r)?.add(m)?;
        self.expected_claims_remaining = self.expected_claims_remaining.sub(c)?;
        self.risk_adjustment_remaining = self.risk_adjustment_remaining.sub(r)?;
        self.csm = self.csm.sub(m)?;
        self.finance_remaining = self.finance_remaining.sub(f)?;
        self.loss_component = self
            .loss_component
            .sub(AccountingAmount::from_cents(batch.loss_memo))?;
        self.released_revenue = self.released_revenue.add(revenue)?;
        self.released_finance = self.released_finance.add(f)?;
        self.carried_claims = batch.carried_claims;
        self.carried_risk_adjustment = batch.carried_risk_adjustment;
        self.carried_csm = batch.carried_csm;
        self.carried_finance = batch.carried_finance;
        self.carried_loss = batch.carried_loss;
        self.units_released += batch.units;
        Ok(())
    }

    /// 落地一次重估（调用方已按财务/亏损分量过账成功；CSM 吸收不过账）。
    pub(super) fn apply_remeasure(
        &mut self,
        new_claims_remaining: AccountingAmount,
        csm_after: AccountingAmount,
        loss_after: AccountingAmount,
        reestimated_abs: AccountingAmount,
        finance_posted: AccountingAmount,
        loss_posted: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.expected_claims_remaining = new_claims_remaining;
        self.csm = csm_after;
        self.loss_component = loss_after;
        self.reestimated_csm = self.reestimated_csm.add(reestimated_abs)?;
        self.remeasure_finance = self.remeasure_finance.add(finance_posted)?;
        self.remeasure_loss = self.remeasure_loss.add(loss_posted)?;
        Ok(())
    }
}
