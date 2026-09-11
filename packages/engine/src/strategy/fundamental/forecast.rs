//! 个人增长预测（K5a 行 148/154）：先验 = 本人已知最近 2 个年度可比收入
//! 增长 clamp [-2000,2000]bp + 一次性个人偏差；不足 2 年显式
//! `PriorWithoutHistory`（0 + 偏差，不称历史事实）；新信息按 λ 修订
//! `new = round_even((1-λ)·old + λ·observed)`。

use super::div_round_half_even;
use super::facts::{AnnualFacts, PriorRevenue};

/// 先验 clamp 边界（K5a 行 148 的计划锁定值）。
pub const GROWTH_PRIOR_CLAMP_BP: i32 = 2_000;

/// 增长观察（由年报事实推导）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GrowthObservation {
    /// 两个年度可比收入（观察增长已 clamp 到 [-2000,2000]bp）。
    TwoYear(i32),
    /// 上年比较项缺历史：显式 0 先验 + 个人偏差（不称历史事实）。
    WithoutHistory,
    /// 上年收入基数非正（零/负收入分母——K5 行 133 显式退化）或观察算术
    /// 溢出：增长观察无定义。
    Degenerate,
}

/// 从年报事实观察可比收入增长：`rhe((r_t − r_{t−1}) × 10000, r_{t−1})`，
/// 再 clamp。基数非正/算术溢出 ⇒ [`GrowthObservation::Degenerate`]（不填零、
/// 不静默 fallback）。
pub fn observe_growth(facts: &AnnualFacts) -> GrowthObservation {
    let PriorRevenue::Comparative(prior) = facts.prior_revenue else {
        return GrowthObservation::WithoutHistory;
    };
    if prior.cents() <= 0 {
        return GrowthObservation::Degenerate;
    }
    let Some(delta) = facts.revenue.cents().checked_sub(prior.cents()) else {
        return GrowthObservation::Degenerate;
    };
    let Some(scaled) = delta.checked_mul(10_000) else {
        return GrowthObservation::Degenerate;
    };
    let bp = div_round_half_even(scaled, prior.cents());
    let Some(bp) = i32::try_from(bp).ok() else {
        return GrowthObservation::Degenerate;
    };
    GrowthObservation::TwoYear(bp.clamp(-GROWTH_PRIOR_CLAMP_BP, GROWTH_PRIOR_CLAMP_BP))
}

/// 预测当前值的成因（provenance）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ForecastBasis {
    /// 初始：有 2 年可比历史（观察增长 + 个人偏差）。
    InitialTwoYear { observed_bp: i32 },
    /// 初始：不足 2 年（0 + 个人偏差）。
    InitialWithoutHistory,
    /// λ 修订（记录本次观察增长）。
    Revised { observed_bp: i32 },
    /// 退化（增长先验无定义——依赖增长的方法显式不可用）。
    Degenerate,
}

/// 个人增长预测状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ForecastState {
    /// `None` = 增长先验退化（现金流法不可用；盈利倍数/权益 ROE 不消费增长）。
    pub growth_bp: Option<i32>,
    pub basis: ForecastBasis,
}

impl ForecastState {
    /// 初始信心（K5a 行 148）：有 2 年可比历史 6000bp，否则（含退化）3000bp。
    /// 只在形成/直接重估路径调用——λ 修订不重置信心。
    pub fn initial_confidence_bp(&self) -> u16 {
        match self.basis {
            ForecastBasis::InitialTwoYear { .. } => 6_000,
            _ => 3_000,
        }
    }
}

/// 初始预测：观察 + 个人一次性偏差。
pub fn initial_forecast(observation: GrowthObservation, deviation_bp: i32) -> ForecastState {
    match observation {
        GrowthObservation::TwoYear(observed_bp) => ForecastState {
            growth_bp: i32::try_from(i64::from(observed_bp) + i64::from(deviation_bp)).ok(),
            basis: ForecastBasis::InitialTwoYear { observed_bp },
        },
        GrowthObservation::WithoutHistory => ForecastState {
            growth_bp: Some(deviation_bp),
            basis: ForecastBasis::InitialWithoutHistory,
        },
        GrowthObservation::Degenerate => ForecastState {
            growth_bp: None,
            basis: ForecastBasis::Degenerate,
        },
    }
}

/// λ 修订：`new = rhe((10000−λ)·old + λ·observed, 10000)`。
///
/// - 观察退化 ⇒ 修订后退化（最新材料无法支持增长，绝不沿用旧值冒充）；
/// - 观察缺历史（无可比 ⇒ 无 observed）⇒ 不修订，保留旧预测（估值仍按新
///   事实重估）；
/// - 旧预测退化 + 新观察可得 ⇒ 以新观察作初始先验（重新起步，偏差沿用
///   本人一次性假设）。
pub fn revise_forecast(
    old: &ForecastState,
    observation: GrowthObservation,
    lambda_bp: i32,
    deviation_bp: i32,
) -> ForecastState {
    match observation {
        GrowthObservation::Degenerate => ForecastState {
            growth_bp: None,
            basis: ForecastBasis::Degenerate,
        },
        GrowthObservation::WithoutHistory => *old,
        GrowthObservation::TwoYear(observed_bp) => match old.growth_bp {
            None => initial_forecast(GrowthObservation::TwoYear(observed_bp), deviation_bp),
            Some(old_bp) => {
                let blended = i64::from(10_000 - lambda_bp) * i64::from(old_bp)
                    + i64::from(lambda_bp) * i64::from(observed_bp);
                ForecastState {
                    growth_bp: i32::try_from(div_round_half_even(i128::from(blended), 10_000)).ok(),
                    basis: ForecastBasis::Revised { observed_bp },
                }
            }
        },
    }
}
