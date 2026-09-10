//! 机构目标价策略 TargetPolicy 与基本面价值策略 ValueStrategy。

use super::*;

use super::institution::decide_inst;

/// 依目标价策略算目标价（返回 cents 的 f64）。V 不可见且 TrackV → None。
/// 抽出为自由函数，供数据驱动内核与旧 ValueStrategy 共用（保证两者永不漂移）。
pub(super) fn target_cents(
    policy: &TargetPolicy,
    v: Option<Money>,
    elapsed_market_minutes: u64,
) -> Option<f64> {
    match policy {
        TargetPolicy::Fixed(m) => Some(m.cents() as f64),
        TargetPolicy::TrackV { bias } => v.map(|vv| vv.cents() as f64 * (1.0 + bias)),
        TargetPolicy::DriftUp { rate, base } => {
            Some(base.cents() as f64 * (1.0 + rate * elapsed_market_minutes as f64))
        }
    }
}

/// 机构目标价策略（每实例一种，机构看法各异）。
///
/// - `Fixed(m)`：固定目标价 m。
/// - `TrackV { bias }`：跟随隐藏公允价 V，target = V × (1 + bias)。
/// - `DriftUp { rate, base }`：认为大致上涨，target = base × (1 + rate × 已完成标准交易分钟)。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum TargetPolicy {
    /// 固定目标价。
    Fixed(Money),
    /// 跟随 V：target = V × (1 + bias)。
    TrackV { bias: f64 },
    /// 认为大致上涨：target = base × (1 + rate × 已完成标准交易分钟)。
    DriftUp { rate: f64, base: Money },
}

/// 机构：基本面价值策略。读隐藏 V + 目标价策略，低买高卖（带 margin 容忍带）。
///
/// 纯逻辑：价格/qty 用 Money/u32；f64 仅在 target/band 计算边界，立即落回 Intent。
/// 不直接碰 orderbook，只产 Intent。
pub struct ValueStrategy {
    pub(super) style: InstitutionStyle,
    policy: TargetPolicy,
    /// 容忍带宽度，∈[0,1)。last 落在 [target×(1-margin), target×(1+margin)] 内不动作。
    margin: f64,
    /// 个体每单股数（>0）；工厂以配置值为群体中心采样。
    order_size: u32,
    pub(super) max_stock_fraction: f64,
    pub(super) base_observation_probability: f64,
}

impl ValueStrategy {
    /// 构造并校验参数。margin∉[0,1) 或 order_size=0 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(policy: TargetPolicy, margin: f64, order_size: u32) -> Result<Self, StrategyError> {
        if !(0.0..1.0).contains(&margin) {
            return Err(StrategyError::InvalidParam {
                param: "margin",
                reason: format!("{margin} not in [0,1)"),
            });
        }
        if order_size == 0 {
            return Err(StrategyError::InvalidParam {
                param: "order_size",
                reason: "must be > 0".to_string(),
            });
        }
        Ok(ValueStrategy {
            style: InstitutionStyle::Balanced,
            policy,
            margin,
            order_size,
            max_stock_fraction: 0.60,
            base_observation_probability: 1.0,
        })
    }

    /// 为测试或显式配置保留机构身份；不会改变估值策略本身。
    pub fn with_institution_style(mut self, style: InstitutionStyle) -> Self {
        self.style = style;
        self
    }
}

impl Strategy for ValueStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Institution(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::FundamentalValue
    }

    fn uses_parent_order_execution(&self) -> bool {
        true
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.base_observation_probability
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）；DriftUp 从 MarketView 读取权威标准交易分钟。
        let mut data = StrategyData::inst(self.policy.clone(), self.margin, self.order_size);
        data.max_stock_fraction = self.max_stock_fraction;
        data.base_observation_probability = self.base_observation_probability;
        decide_inst(&data, market, own)
    }
}
