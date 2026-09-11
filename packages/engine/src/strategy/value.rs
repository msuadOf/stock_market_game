//! 机构目标价策略 TargetPolicy 与信念驱动机构策略（任务 26）。
//!
//! 共同 V（隐藏公允价）已删除：机构的方向判断不再读取任何市场级隐藏价值，
//! 而是由会话决策链接线（session/decision_chain.rs）按账户的
//! [`BeliefBook`](super::beliefs::BeliefBook) 个人每股估值区间与 K5a 五路
//! 信号聚合驱动。本文件只保留：
//! - [`TargetPolicy`]：Fixed/DriftUp 两种显式目标价（数据驱动内核与测试用）；
//! - [`BeliefInstitutionStrategy`]：DeepValue/Defensive/Growth/Balanced 的
//!   机构策略壳——身份、观察概率与个体规模参数；其 `decide` 不产意图，
//!   订单完全由决策链经真实计划执行提交（绝不与母单物化路径混用）。

use super::*;

/// 依目标价策略算目标价（返回 cents 的 f64）。抽出为自由函数，供数据驱动
/// 内核共用（保证各入口永不漂移）。
pub(super) fn target_cents(policy: &TargetPolicy, elapsed_market_minutes: u64) -> Option<f64> {
    match policy {
        TargetPolicy::Fixed(m) => Some(m.cents() as f64),
        TargetPolicy::DriftUp { rate, base } => {
            Some(base.cents() as f64 * (1.0 + rate * elapsed_market_minutes as f64))
        }
    }
}

/// 机构目标价策略（每实例一种，机构看法各异）。
///
/// - `Fixed(m)`：固定目标价 m。
/// - `DriftUp { rate, base }`：认为大致上涨，target = base × (1 + rate × 已完成标准交易分钟)。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum TargetPolicy {
    /// 固定目标价。
    Fixed(Money),
    /// 认为大致上涨：target = base × (1 + rate × 已完成标准交易分钟)。
    DriftUp { rate: f64, base: Money },
}

/// 信念驱动的机构策略壳（DeepValue/Defensive/Growth/Balanced）。
///
/// 估值与方向由账户的 `BeliefBook`（个人已知公开报告推导的每股估值区间）
/// 与 K5a 混合分析给出，全部状态在会话侧按账户持有；本类型只承载身份、
/// 观察节奏与个体规模参数。`decide` 恒空且不触碰工作单——计划驱动的账户
/// 绝不对同一 (账户,股票) 走普通意图物化路径（任务 24 复核遗留的硬约束）。
pub struct BeliefInstitutionStrategy {
    pub(super) style: InstitutionStyle,
    /// 容忍带宽度，∈[0,1)。保留为个体行为参数（执行/复核语义）。
    pub(super) margin: f64,
    /// 个体每单股数（>0）；工厂以配置值为群体中心采样。
    pub(super) order_size: u32,
    pub(super) max_stock_fraction: f64,
    pub(super) base_observation_probability: f64,
}

impl BeliefInstitutionStrategy {
    /// 构造并校验参数。margin∉[0,1) 或 order_size=0 → `StrategyError::InvalidParam`
    /// （防御式：不静默用默认值）。
    pub fn new(margin: f64, order_size: u32) -> Result<Self, StrategyError> {
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
        Ok(BeliefInstitutionStrategy {
            style: InstitutionStyle::Balanced,
            margin,
            order_size,
            max_stock_fraction: 0.60,
            base_observation_probability: 1.0,
        })
    }

    /// 为测试或显式配置保留机构身份；不会改变估值来源。
    pub fn with_institution_style(mut self, style: InstitutionStyle) -> Self {
        self.style = style;
        self
    }

    pub fn institution_style(&self) -> InstitutionStyle {
        self.style
    }

    /// 个体单笔申报股数（决策链的子单上限）。
    pub fn order_size(&self) -> u32 {
        self.order_size
    }

    /// 单只股票市值占总资产的上限（决策链的目标权重上限）。
    pub fn max_stock_fraction(&self) -> f64 {
        self.max_stock_fraction
    }

    /// 容忍带宽度（保留为个体行为参数）。
    pub fn margin(&self) -> f64 {
        self.margin
    }
}

impl Strategy for BeliefInstitutionStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Institution(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::FundamentalValue
    }

    fn belief_chain_params(&self) -> Option<BeliefChainParams> {
        Some(BeliefChainParams {
            max_stock_fraction: self.max_stock_fraction,
            margin: self.margin,
            order_size: self.order_size,
        })
    }

    /// 方向与订单完全由会话决策链驱动；本策略壳不产意图。
    fn decide(&mut self, _market: &MarketView, _own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        Vec::new()
    }

    /// 空决策不代表「现有报价失效」：计划子单由计划执行器管理。
    fn updates_working_quotes_on_empty_decision(&self) -> bool {
        false
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.base_observation_probability
    }
}
