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

use super::claims::{ClaimId, ClaimRegister, ClaimState};
use super::csm::{simple_discount_pv, unit_release};
use super::InsuranceError;
use crate::accounting::{AccountingAmount, AccountingError, FractionUnits};
use crate::calendar::CivilDate;
use crate::company::counterparty::CounterpartyId;

/// 单个合同组聚合根：保费身份 + GMM 计量/进度 + 赔案账。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ContractGroupState {
    policyholder: CounterpartyId,
    premium: AccountingAmount,
    premium_collected: AccountingAmount,
    measurement: ContractMeasurementState,
    claims: ClaimRegister,
}

/// 同组的五组件、责任单元/余数与调节累计由一个 owner 共同维护。
#[derive(Clone, Eq, PartialEq, Debug)]
pub(super) struct ContractMeasurementState {
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
    /// 调节表累计：已释放保险服务收入（预期赔付/风险调整/CSM）。
    released_revenue: AccountingAmount,
    /// 调节表累计：已回拨保险财务损益。
    released_finance: AccountingAmount,
    /// 调节表累计：重估即时过账的财务分量（Dr 6541 为正）。
    remeasure_finance: AccountingAmount,
    /// 调节表累计：重估亏损成分变动（Dr 6451 为正；转回为负）。
    remeasure_loss: AccountingAmount,
    /// 调节表累计：被 CSM 吸收的再计量绝对量。
    reestimated_csm: AccountingAmount,
    /// 五类余数的单位均为分 × 责任单元，与释放进度共同持有。
    carried_claims: FractionUnits,
    carried_risk_adjustment: FractionUnits,
    carried_csm: FractionUnits,
    carried_finance: FractionUnits,
    carried_loss: FractionUnits,
}

/// 重估的临时差额；不保存第二份权威余额，也不持有凭证/事件槽。
pub(super) struct RemeasureDelta {
    new_claims_remaining: AccountingAmount,
    csm_after: AccountingAmount,
    loss_after: AccountingAmount,
    reestimated_abs: AccountingAmount,
    pub finance_posted: AccountingAmount,
    pub loss_posted: AccountingAmount,
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
            measurement: ContractMeasurementState {
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
            },
            claims: ClaimRegister::new(),
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
        self.measurement.expected_claims_remaining
    }

    pub fn risk_adjustment_remaining(&self) -> AccountingAmount {
        self.measurement.risk_adjustment_remaining
    }

    pub fn csm(&self) -> AccountingAmount {
        self.measurement.csm
    }

    pub fn loss_component(&self) -> AccountingAmount {
        self.measurement.loss_component
    }

    pub fn finance_remaining(&self) -> AccountingAmount {
        self.measurement.finance_remaining
    }

    pub fn units_total(&self) -> i64 {
        self.measurement.units_total
    }

    pub fn units_released(&self) -> i64 {
        self.measurement.units_released
    }

    pub fn units_remaining(&self) -> i64 {
        self.measurement.units_remaining()
    }

    pub fn coverage_start(&self) -> CivilDate {
        self.measurement.coverage_start
    }

    pub fn coverage_end(&self) -> CivilDate {
        self.measurement.coverage_end
    }

    pub fn day_one_loss(&self) -> AccountingAmount {
        self.measurement.day_one_loss
    }

    pub fn released_revenue_total(&self) -> AccountingAmount {
        self.measurement.released_revenue
    }

    pub fn released_finance_total(&self) -> AccountingAmount {
        self.measurement.released_finance
    }

    pub fn remeasure_finance_total(&self) -> AccountingAmount {
        self.measurement.remeasure_finance
    }

    pub fn remeasure_loss_total(&self) -> AccountingAmount {
        self.measurement.remeasure_loss
    }

    pub fn reestimated_csm_total(&self) -> AccountingAmount {
        self.measurement.reestimated_csm
    }

    pub fn claim(&self, id: &ClaimId) -> Option<&ClaimState> {
        self.claims.get(id)
    }

    pub fn claims(&self) -> impl Iterator<Item = (&ClaimId, &ClaimState)> {
        self.claims.iter()
    }

    pub(super) fn measurement(&self) -> &ContractMeasurementState {
        &self.measurement
    }

    pub(super) fn measurement_mut(&mut self) -> &mut ContractMeasurementState {
        &mut self.measurement
    }

    pub(super) fn claim_register_mut(&mut self) -> &mut ClaimRegister {
        &mut self.claims
    }

    pub(super) fn apply_premium_collection(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.premium_collected = self.premium_collected.add(amount)?;
        Ok(())
    }
}

impl ContractMeasurementState {
    fn units_remaining(&self) -> i64 {
        self.units_total - self.units_released
    }

    /// 纯预览：沿用逐组件运算顺序与末批/零 CSM/零亏损余数口径。
    pub(super) fn preview_release(&self, units: i64) -> Result<ReleaseBatch, AccountingError> {
        let final_batch = units == self.units_remaining();
        let units_total = self.units_total;
        let (claims, carried_claims) = unit_release(
            self.expected_claims_remaining.cents(),
            units,
            units_total,
            self.carried_claims,
            final_batch,
        )?;
        let (risk_adjustment, carried_ra) = unit_release(
            self.risk_adjustment_remaining.cents(),
            units,
            units_total,
            self.carried_risk_adjustment,
            final_batch,
        )?;
        let (csm, carried_csm) = if self.csm.is_positive() {
            unit_release(
                self.csm.cents(),
                units,
                units_total,
                self.carried_csm,
                final_batch,
            )?
        } else {
            (0, self.carried_csm)
        };
        let (finance, carried_finance) = unit_release(
            self.finance_remaining.cents(),
            units,
            units_total,
            self.carried_finance,
            final_batch,
        )?;
        let (loss_memo, carried_loss) = if self.loss_component.is_positive() {
            unit_release(
                self.loss_component.cents(),
                units,
                units_total,
                self.carried_loss,
                final_batch,
            )?
        } else {
            (0, self.carried_loss)
        };
        Ok(ReleaseBatch {
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
        })
    }

    /// 纯预览：ΔPV 先分流到 CSM/备查亏损，ΔF 留给账套过账。
    pub(super) fn preview_remeasure(
        &self,
        new_remaining_claims: AccountingAmount,
        rate_bp: i32,
    ) -> Result<Option<RemeasureDelta>, InsuranceError> {
        let delta_e = new_remaining_claims
            .sub(self.expected_claims_remaining)?
            .cents();
        if delta_e == 0 {
            return Ok(None);
        }
        let delta_pv = simple_discount_pv(delta_e, rate_bp, self.units_remaining())?;
        let delta_finance = delta_e
            .checked_sub(delta_pv)
            .ok_or(InsuranceError::Accounting(
                AccountingError::AmountOverflow {
                    op: "remeasure finance delta",
                    detail: format!("{delta_e} − {delta_pv}"),
                },
            ))?;
        let csm_before = self.csm;
        let loss_before = self.loss_component;
        // CSM 吸收 / 亏损成分分流（三种转换：盈利↔亏损、亏损加重/转回）。
        let (csm_after, loss_after, loss_posted, reestimated_abs) = if delta_pv <= 0 {
            let improvement = -delta_pv;
            if loss_before.is_zero() {
                // 盈利组：CSM 增加（纯组合成分变化，不过账）。
                (
                    csm_before.add(AccountingAmount::from_cents(improvement))?,
                    AccountingAmount::ZERO,
                    AccountingAmount::ZERO,
                    AccountingAmount::from_cents(improvement),
                )
            } else {
                // 亏损组转回：先冲亏损（过账利得），盈余转 CSM。
                let loss_cents = loss_before.cents();
                let recovery = loss_cents.min(improvement);
                let new_csm = improvement - recovery;
                (
                    AccountingAmount::from_cents(new_csm),
                    AccountingAmount::from_cents(loss_cents - recovery),
                    AccountingAmount::from_cents(-recovery),
                    AccountingAmount::from_cents(new_csm),
                )
            }
        } else {
            let worsening = delta_pv;
            if loss_before.is_zero() {
                // 盈利组：CSM 吸收，耗尽部分即期入损益并转亏损组。
                let absorbed = csm_before.cents().min(worsening);
                let loss_add = worsening - absorbed;
                (
                    AccountingAmount::from_cents(csm_before.cents() - absorbed),
                    AccountingAmount::from_cents(loss_before.cents() + loss_add),
                    AccountingAmount::from_cents(loss_add),
                    AccountingAmount::from_cents(absorbed),
                )
            } else {
                // 亏损组加重：全额即期入损益。
                (
                    AccountingAmount::ZERO,
                    AccountingAmount::from_cents(loss_before.cents() + worsening),
                    AccountingAmount::from_cents(worsening),
                    AccountingAmount::ZERO,
                )
            }
        };
        Ok(Some(RemeasureDelta {
            new_claims_remaining: new_remaining_claims,
            csm_after,
            loss_after,
            reestimated_abs,
            finance_posted: AccountingAmount::from_cents(delta_finance),
            loss_posted,
        }))
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
    pub(super) fn apply_remeasure(&mut self, delta: RemeasureDelta) -> Result<(), AccountingError> {
        self.expected_claims_remaining = delta.new_claims_remaining;
        self.csm = delta.csm_after;
        self.loss_component = delta.loss_after;
        self.reestimated_csm = self.reestimated_csm.add(delta.reestimated_abs)?;
        self.remeasure_finance = self.remeasure_finance.add(delta.finance_posted)?;
        self.remeasure_loss = self.remeasure_loss.add(delta.loss_posted)?;
        Ok(())
    }
}

/// 保留原扁平存档字段、顺序和 derive 接受集；子对象不新增持久化字段。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "ContractGroupState")]
struct ContractGroupSnapshot<Policyholder = CounterpartyId, Claims = BTreeMap<ClaimId, ClaimState>>
{
    policyholder: Policyholder,
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
    released_revenue: AccountingAmount,
    released_finance: AccountingAmount,
    remeasure_finance: AccountingAmount,
    remeasure_loss: AccountingAmount,
    reestimated_csm: AccountingAmount,
    carried_claims: FractionUnits,
    carried_risk_adjustment: FractionUnits,
    carried_csm: FractionUnits,
    carried_finance: FractionUnits,
    carried_loss: FractionUnits,
    claims: Claims,
}

impl serde::Serialize for ContractGroupState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ContractGroupSnapshot {
            policyholder: &self.policyholder,
            premium: self.premium,
            premium_collected: self.premium_collected,
            expected_claims_remaining: self.measurement.expected_claims_remaining,
            risk_adjustment_remaining: self.measurement.risk_adjustment_remaining,
            csm: self.measurement.csm,
            loss_component: self.measurement.loss_component,
            finance_remaining: self.measurement.finance_remaining,
            units_total: self.measurement.units_total,
            units_released: self.measurement.units_released,
            coverage_start: self.measurement.coverage_start,
            coverage_end: self.measurement.coverage_end,
            day_one_loss: self.measurement.day_one_loss,
            released_revenue: self.measurement.released_revenue,
            released_finance: self.measurement.released_finance,
            remeasure_finance: self.measurement.remeasure_finance,
            remeasure_loss: self.measurement.remeasure_loss,
            reestimated_csm: self.measurement.reestimated_csm,
            carried_claims: self.measurement.carried_claims,
            carried_risk_adjustment: self.measurement.carried_risk_adjustment,
            carried_csm: self.measurement.carried_csm,
            carried_finance: self.measurement.carried_finance,
            carried_loss: self.measurement.carried_loss,
            claims: &self.claims.claims,
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ContractGroupState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let saved =
            ContractGroupSnapshot::<CounterpartyId, BTreeMap<ClaimId, ClaimState>>::deserialize(
                deserializer,
            )?;
        Ok(Self {
            policyholder: saved.policyholder,
            premium: saved.premium,
            premium_collected: saved.premium_collected,
            measurement: ContractMeasurementState {
                expected_claims_remaining: saved.expected_claims_remaining,
                risk_adjustment_remaining: saved.risk_adjustment_remaining,
                csm: saved.csm,
                loss_component: saved.loss_component,
                finance_remaining: saved.finance_remaining,
                units_total: saved.units_total,
                units_released: saved.units_released,
                coverage_start: saved.coverage_start,
                coverage_end: saved.coverage_end,
                day_one_loss: saved.day_one_loss,
                released_revenue: saved.released_revenue,
                released_finance: saved.released_finance,
                remeasure_finance: saved.remeasure_finance,
                remeasure_loss: saved.remeasure_loss,
                reestimated_csm: saved.reestimated_csm,
                carried_claims: saved.carried_claims,
                carried_risk_adjustment: saved.carried_risk_adjustment,
                carried_csm: saved.carried_csm,
                carried_finance: saved.carried_finance,
                carried_loss: saved.carried_loss,
            },
            claims: ClaimRegister::from_claims(saved.claims),
        })
    }
}
