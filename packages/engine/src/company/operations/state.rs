//! 单公司经济状态（活跃冲击聚合 → 经济字段乘数）+ 数量域缩放。

use crate::calendar::CivilDate;
use crate::company::events::{ActiveShock, ShockKind};

/// 活跃冲击集合（持久化；到期移除即恢复）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CompanyEconomicState {
    active: Vec<ActiveShock>,
}

/// 聚合后的经济字段（经营流只读这个派生态；乘数饱和于 0——需求/成本不
/// 为负是模型定义，不是静默钳位）。
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct EconomyAggregates {
    /// 需求乘数（10000 = 中性）。
    pub demand_mult_bp: i64,
    /// 成本乘数（10000 = 中性）。
    pub cost_mult_bp: i64,
    /// 信用风险加成（ECL 率增量，bp）。
    pub credit_risk_add_bp: i32,
    /// 生产/开发中断（任一活跃中断冲击即停）。
    pub interrupted: bool,
}

impl CompanyEconomicState {
    pub fn active(&self) -> &[ActiveShock] {
        &self.active
    }

    pub fn activate(&mut self, shock: ActiveShock) {
        self.active.push(shock);
    }

    /// 移除 `date` 当日已到期的冲击（`expires_on < date`），返回被移除者
    /// （恢复钩子用：中断到期 → 复工）。
    pub fn expire_before(&mut self, date: CivilDate) -> Vec<ActiveShock> {
        let mut expired = Vec::new();
        let mut kept = Vec::new();
        for shock in self.active.drain(..) {
            if shock.expires_on < date {
                expired.push(shock);
            } else {
                kept.push(shock);
            }
        }
        self.active = kept;
        expired
    }

    pub fn aggregates(&self) -> EconomyAggregates {
        let mut demand: i64 = 10_000;
        let mut cost: i64 = 10_000;
        let mut credit: i32 = 0;
        let mut interrupted = false;
        for shock in &self.active {
            match shock.kind {
                ShockKind::MarketDemandShift
                | ShockKind::CompanyDemandShift
                | ShockKind::ContractWon
                | ShockKind::ContractCancelled => demand += i64::from(shock.amplitude_bp),
                ShockKind::IndustryCostShift { .. } => cost += i64::from(shock.amplitude_bp),
                ShockKind::CreditDeterioration => credit += shock.amplitude_bp,
                ShockKind::ProductionInterruption => interrupted = true,
                ShockKind::AssetImpairmentSignal => {}
            }
        }
        EconomyAggregates {
            demand_mult_bp: demand.max(0),
            cost_mult_bp: cost.max(0),
            credit_risk_add_bp: credit,
            interrupted,
        }
    }

    /// 资产减值迹象幅度（取活跃信号的最大幅度；经营流一次性消费）。
    pub fn impairment_signal_bp(&self) -> i32 {
        self.active
            .iter()
            .filter(|shock| matches!(shock.kind, ShockKind::AssetImpairmentSignal))
            .map(|shock| shock.amplitude_bp)
            .max()
            .unwrap_or(0)
    }
}

/// 数量域基点缩放（整数半偶舍入；`accounting::amount::div_round_half_even`
/// 的数量域孪生副本——多行业 rhe 副本先例，任务 13 统一入口时一并收编）。
pub(in crate::company::operations) fn scale_units(units: i128, mult_bp: i64) -> i128 {
    let negative = units < 0;
    let magnitude = units.unsigned_abs();
    let scaled = magnitude.saturating_mul(u128::from(mult_bp.unsigned_abs()));
    let quotient = scaled / 10_000;
    let remainder = scaled % 10_000;
    let mut rounded =
        quotient + u128::from(remainder > 5_000 || (remainder == 5_000 && quotient % 2 == 1));
    if negative {
        rounded = rounded.wrapping_neg();
    }
    i128::try_from(rounded).expect("scaled quantity within i128 (inputs are small)")
}

/// 自然日前进 N 日（N ≥ 0；负数类型化拒绝——回拨没有业务含义）。
pub(in crate::company::operations) fn add_days(
    date: CivilDate,
    days: i64,
) -> Result<CivilDate, crate::company::operations::OperationsError> {
    use crate::company::operations::OperationsError;
    if days < 0 {
        return Err(OperationsError::NegativeDayOffset { days });
    }
    let mut cursor = date;
    for _ in 0..days {
        cursor = cursor.next()?;
    }
    Ok(cursor)
}
