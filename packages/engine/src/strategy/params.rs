//! 每类 NPC 的群体基准分布参数。

use super::*;

use super::sampling::validate_order_size_sampling_range;

/// 散户策略分布参数（每实例从中采样/直接取）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct RetailParams {
    /// 每次观察时的到达概率，∈[0,1]。
    pub arrival_rate: f64,
    /// 群体订单规模基准，>0；基准不少于一手时，每个散户实例在其 60%–140%
    /// 范围内采样整手数量；不足一手时保持原值，不静默放大。
    pub order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    pub tick_cents: i64,
}

/// 机构策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct InstParams {
    /// 容忍带宽度，∈[0,1)。
    pub margin: f64,
    /// 群体订单规模基准，>0；基准不少于一手时，每个机构实例在其 60%–140%
    /// 范围内采样整手数量；不足一手时保持原值，不静默放大。
    pub order_size: u32,
}

/// 游资策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct HotParams {
    /// 回看完整交易分钟数，≥2。
    pub lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    pub trend_threshold: f64,
    /// 群体订单规模基准，>0；基准不少于一手时，每个游资实例在其 60%–140%
    /// 范围内采样整手数量；不足一手时保持原值，不静默放大。
    pub order_size: u32,
}

/// 每类 NPC 的群体基准参数。
///
/// `StrategyFactory` 在这些基准之上，经注入 RNG 为每个实例采样行为阈值；同一会话 seed
/// 可重建相同的个体性格，不同 NPC 则不会在同一个跌幅或量能点同步行动。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct StrategyParams {
    /// 散户参数。
    pub retail: RetailParams,
    /// 机构参数。
    pub inst: InstParams,
    /// 游资参数。
    pub hot: HotParams,
}

impl StrategyParams {
    /// 校验可能由 serde 直接构造的全部策略参数，不创建或静默禁用 NPC。
    pub fn validate(&self) -> Result<(), StrategyError> {
        ZiNoiseStrategy::new(
            self.retail.arrival_rate,
            self.retail.order_size_mean,
            self.retail.chase_prob,
            self.retail.tick_cents,
        )?;
        ValueStrategy::new(
            TargetPolicy::TrackV { bias: 0.0 },
            self.inst.margin,
            self.inst.order_size,
        )?;
        MomentumStrategy::new(
            self.hot.lookback,
            self.hot.trend_threshold,
            self.hot.order_size,
        )?;
        validate_order_size_sampling_range(self.retail.order_size_mean, "order_size_mean")?;
        validate_order_size_sampling_range(self.inst.order_size, "order_size")?;
        validate_order_size_sampling_range(self.hot.order_size, "order_size")?;
        Ok(())
    }
}
