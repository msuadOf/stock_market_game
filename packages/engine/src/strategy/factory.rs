//! 策略工厂：按账户种类与序号采样个体策略实例。

use super::*;

use super::momentum::InstitutionMomentumStrategy;
use super::sampling::{
    daily_observations_to_tick_probability, sample_between, sample_individual_order_size,
};
use crate::account::AccountKind;

/// 策略工厂：按账户种类构造策略实例。
///
/// Player → `None`（玩家不持算法策略，UI 动作直接产 Intent）；
/// Retail/Inst/Hot → 按各自 `StrategyParams` 构造对应策略。
/// 构造失败（参数非法）→ 返回带上下文的 [`StrategyError`]；绝不静默禁用 NPC 或回退默认值。
pub struct StrategyFactory;

impl StrategyFactory {
    /// 按种类构造策略；个体阈值在创建时从会话 RNG 采样，同一 seed 可重建同一性格。
    pub fn build(
        kind: AccountKind,
        params: &StrategyParams,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn Strategy + Send + Sync>>, StrategyError> {
        Self::build_for_market_day(kind, params, 15_300, rng)
    }

    /// 使用权威交易日 tick 数把“每天观察次数”换算成逐 tick 概率。
    pub fn build_for_market_day(
        kind: AccountKind,
        params: &StrategyParams,
        ticks_per_day: u64,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn Strategy + Send + Sync>>, StrategyError> {
        Self::build_for_market_day_with_ordinal(kind, params, ticks_per_day, 0, rng)
    }

    /// 按账户在同类 NPC 中的序号分配稳定风格；序号只决定风格，不合并账户状态。
    pub fn build_for_market_day_with_ordinal(
        kind: AccountKind,
        params: &StrategyParams,
        ticks_per_day: u64,
        ordinal: u32,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn Strategy + Send + Sync>>, StrategyError> {
        debug_assert!(ticks_per_day > 0);
        match kind {
            AccountKind::Retail => {
                let r = &params.retail;
                let mut strategy = ZiNoiseStrategy::new(
                    r.arrival_rate,
                    sample_individual_order_size(rng, r.order_size_mean, "order_size_mean")?,
                    r.chase_prob,
                    r.tick_cents,
                )?;
                let style_draw = rng.next_f64();
                strategy.retail_style = if style_draw < 0.35 {
                    RetailStyle::Dormant
                } else if style_draw < 0.55 {
                    RetailStyle::LongTerm
                } else if style_draw < 0.75 {
                    RetailStyle::Noise
                } else if style_draw < 0.85 {
                    RetailStyle::DipBuyer
                } else if style_draw < 0.95 {
                    RetailStyle::Momentum
                } else {
                    RetailStyle::Panic
                };
                strategy.dip_threshold = sample_between(rng, 0.015, 0.08);
                strategy.stop_loss_threshold = sample_between(rng, 0.03, 0.12);
                strategy.take_profit_threshold = sample_between(rng, 0.05, 0.18);
                strategy.volume_confirmation = sample_between(rng, 0.35, 1.25);
                strategy.max_stock_fraction = sample_between(rng, 0.15, 0.40);
                let observations_per_day = match strategy.retail_style {
                    RetailStyle::Dormant => sample_between(rng, 0.1, 0.5),
                    RetailStyle::LongTerm => sample_between(rng, 0.5, 2.0),
                    RetailStyle::Noise | RetailStyle::DipBuyer => sample_between(rng, 2.0, 8.0),
                    RetailStyle::Momentum => sample_between(rng, 10.0, 40.0),
                    RetailStyle::Panic => sample_between(rng, 5.0, 20.0),
                };
                match strategy.retail_style {
                    RetailStyle::Dormant => {
                        strategy.arrival_rate *= 0.20;
                        strategy.chase_prob *= 0.25;
                        strategy.max_stock_fraction = 0.80;
                    }
                    RetailStyle::LongTerm => {
                        strategy.arrival_rate *= 0.50;
                        strategy.chase_prob *= 0.40;
                        strategy.take_profit_threshold *= 1.50;
                        strategy.stop_loss_threshold *= 1.50;
                        strategy.max_stock_fraction = 0.65;
                    }
                    RetailStyle::Noise => {}
                    RetailStyle::DipBuyer => {
                        strategy.chase_prob = strategy.chase_prob.max(0.75);
                        strategy.dip_threshold *= 0.70;
                    }
                    RetailStyle::Momentum => {
                        strategy.chase_prob = strategy.chase_prob.max(0.85);
                        strategy.volume_confirmation *= 0.80;
                    }
                    RetailStyle::Panic => {
                        strategy.chase_prob = strategy.chase_prob.max(0.70);
                        strategy.stop_loss_threshold *= 0.55;
                    }
                }
                strategy.base_observation_probability =
                    daily_observations_to_tick_probability(observations_per_day, ticks_per_day);
                Ok(Some(Box::new(strategy)))
            }
            AccountKind::Inst => {
                let i = &params.inst;
                let style = match ordinal % 5 {
                    0 => InstitutionStyle::DeepValue,
                    1 => InstitutionStyle::Growth,
                    2 => InstitutionStyle::Balanced,
                    3 => InstitutionStyle::Defensive,
                    _ => InstitutionStyle::ActiveTrader,
                };
                let (bias_center, margin_factor, max_fraction, observations) = match style {
                    InstitutionStyle::DeepValue => (-0.025, 1.35, (0.45, 0.70), (15.0, 40.0)),
                    InstitutionStyle::Growth => (0.030, 1.00, (0.45, 0.75), (20.0, 55.0)),
                    InstitutionStyle::Balanced => (0.000, 1.00, (0.35, 0.60), (20.0, 50.0)),
                    InstitutionStyle::Defensive => (-0.010, 1.60, (0.25, 0.45), (10.0, 30.0)),
                    InstitutionStyle::ActiveTrader => (0.005, 0.65, (0.25, 0.50), (50.0, 100.0)),
                };
                // 同风格内部仍保留独立估值误差，避免机构同步行动。
                let individual_order_size =
                    sample_individual_order_size(rng, i.order_size, "order_size")?;
                let base_observation_probability = daily_observations_to_tick_probability(
                    sample_between(rng, observations.0, observations.1),
                    ticks_per_day,
                );
                if style == InstitutionStyle::ActiveTrader {
                    // 积极交易机构采用同一动量内核，但身份、资金账户与调度参数仍是机构。
                    // 不把隐藏 V 传入该策略，也不把它的限价目标交给价值机构母单。
                    let h = &params.hot;
                    let mut inner = MomentumStrategy::new(
                        h.lookback,
                        h.trend_threshold,
                        individual_order_size,
                    )?;
                    inner.style = HotStyle::Momentum;
                    inner.volume_confirmation = sample_between(rng, 0.50, 1.50);
                    inner.max_stock_fraction = sample_between(rng, max_fraction.0, max_fraction.1);
                    inner.base_observation_probability = base_observation_probability;
                    return Ok(Some(Box::new(InstitutionMomentumStrategy { style, inner })));
                }
                let bias = bias_center + sample_between(rng, -0.01, 0.01);
                let mut strategy = ValueStrategy::new(
                    TargetPolicy::TrackV { bias },
                    (i.margin * margin_factor).min(0.95),
                    individual_order_size,
                )?;
                strategy.style = style;
                strategy.max_stock_fraction = sample_between(rng, max_fraction.0, max_fraction.1);
                strategy.base_observation_probability = base_observation_probability;
                Ok(Some(Box::new(strategy)))
            }
            AccountKind::Hot => {
                let h = &params.hot;
                let individual_order_size =
                    sample_individual_order_size(rng, h.order_size, "order_size")?;
                let mut strategy =
                    MomentumStrategy::new(h.lookback, h.trend_threshold, individual_order_size)?;
                strategy.style = if ordinal.is_multiple_of(2) {
                    HotStyle::Momentum
                } else {
                    HotStyle::Reversal
                };
                strategy.volume_confirmation = sample_between(rng, 0.50, 1.50);
                strategy.max_stock_fraction = sample_between(rng, 0.10, 0.30);
                strategy.base_observation_probability = daily_observations_to_tick_probability(
                    sample_between(rng, 80.0, 300.0),
                    ticks_per_day,
                );
                Ok(Some(Box::new(strategy)))
            }
            AccountKind::Player => Ok(None),
        }
    }
}
