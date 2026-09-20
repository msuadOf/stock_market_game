//! 游资 MomentumStrategy 与复用动量内核的机构积极交易策略。

use super::*;

use super::hot::{decide_hot, decide_hot_reversal};

/// 游资：短期趋势/动量策略，快进快出。
///
/// 看每只股票的完整交易分钟收盘窗口：取最近 `lookback` 个分钟点，算相对变化 change=(末-首)/首。
/// change > threshold、量能确认且盘口没有严重偏空 → 追涨买入；
/// change < -threshold 且有可卖持仓 → 杀跌卖出。
/// 持仓不足或趋势不明（|change| ≤ threshold、点数不足）→ 不动作。
///
/// 纯逻辑：随机经注入 `&mut dyn Rng`（本策略实际不消费 RNG，签名对齐 trait）；价格/qty 用 Money/u32；
/// f64 仅在 change 计算边界，立即落回 Intent。不直接碰 orderbook，只产 Intent。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MomentumStrategy {
    pub(super) style: HotStyle,
    /// 回看完整交易分钟数，≥2。
    #[serde(with = "super::state::js_safe_usize")]
    lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    #[serde(with = "super::state::exact_float")]
    trend_threshold: f64,
    /// 个体每单股数，>0；工厂以配置值为群体中心采样。
    order_size: u32,
    /// 追涨所需的最低相对成交量。
    #[serde(with = "super::state::exact_float")]
    pub(super) volume_confirmation: f64,
    #[serde(with = "super::state::exact_float")]
    pub(super) max_stock_fraction: f64,
    #[serde(with = "super::state::exact_float")]
    pub(super) base_observation_probability: f64,
}

impl MomentumStrategy {
    pub(super) fn validate_state(&self) -> Result<(), StrategyStateError> {
        Self::new(self.lookback, self.trend_threshold, self.order_size)
            .map_err(|error| StrategyStateError::InvalidParameters(error.to_string()))?;
        if u64::try_from(self.lookback).map_or(true, |value| {
            value > super::state::MAX_JAVASCRIPT_SAFE_INTEGER
        }) {
            return Err(StrategyStateError::InvalidParameters(
                "lookback exceeds the JavaScript safe integer range".to_owned(),
            ));
        }
        if !self.trend_threshold.is_finite()
            || !self.volume_confirmation.is_finite()
            || self.volume_confirmation < 0.0
            || !(0.0..=1.0).contains(&self.max_stock_fraction)
            || !(0.0..=1.0).contains(&self.base_observation_probability)
        {
            return Err(StrategyStateError::InvalidParameters(
                "invalid momentum risk or observation parameters".to_owned(),
            ));
        }
        Ok(())
    }

    /// 构造并校验参数。lookback<2 / threshold<0 / order_size=0 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(
        lookback: usize,
        trend_threshold: f64,
        order_size: u32,
    ) -> Result<Self, StrategyError> {
        if lookback < 2 {
            return Err(StrategyError::InvalidParam {
                param: "lookback",
                reason: "must be >= 2".to_string(),
            });
        }
        if trend_threshold < 0.0 {
            return Err(StrategyError::InvalidParam {
                param: "trend_threshold",
                reason: "must be >= 0".to_string(),
            });
        }
        if order_size == 0 {
            return Err(StrategyError::InvalidParam {
                param: "order_size",
                reason: "must be > 0".to_string(),
            });
        }
        Ok(MomentumStrategy {
            style: HotStyle::Momentum,
            lookback,
            trend_threshold,
            order_size,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.25,
            base_observation_probability: 1.0,
        })
    }
}

impl ProductionStrategy for MomentumStrategy {
    fn state(&self) -> Result<StrategyState, StrategyStateError> {
        Ok(StrategyState::Momentum(self.clone()))
    }
}

impl Strategy for MomentumStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Hot(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::Momentum
    }

    fn hot_style(&self) -> Option<HotStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.base_observation_probability
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）：字段映射成 StrategyData，
        // 调统一纯函数 decide_hot，保证「同种子同输出」不漂移。
        let data = StrategyData::hot(self.lookback, self.trend_threshold, self.order_size);
        let mut data = data;
        data.volume_confirmation = self.volume_confirmation;
        data.max_stock_fraction = self.max_stock_fraction;
        data.base_observation_probability = self.base_observation_probability;
        match self.style {
            HotStyle::Momentum => decide_hot(&data, market, own),
            HotStyle::Reversal => decide_hot_reversal(&data, market, own),
        }
    }
}

/// 身份为机构、但按动量执行的积极交易策略。
///
/// 它复用游资动量内核，却保留机构身份、资金规模、注意力敏感度与具名机构风格；因此既不
/// 把账户改成游资，也不读取隐藏公允价值 V。它不使用价值机构的母单执行，而是走普通工作报价。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstitutionMomentumStrategy {
    pub(super) style: InstitutionStyle,
    pub(super) inner: MomentumStrategy,
}

impl ProductionStrategy for InstitutionMomentumStrategy {
    fn state(&self) -> Result<StrategyState, StrategyStateError> {
        Ok(StrategyState::InstitutionMomentum(self.clone()))
    }
}

impl Strategy for InstitutionMomentumStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Institution(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::Momentum
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.inner.base_observation_probability()
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent> {
        self.inner.decide(market, own, rng)
    }
}
