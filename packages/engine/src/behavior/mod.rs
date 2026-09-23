//! NPC 的可解释行为计划。
//!
//! 本模块只把权威市场观测与独立账户处境解释为目标仓位，不直接修改订单簿、成交或账户。

use std::collections::BTreeMap;

use crate::observation::{
    AccountRiskObservation, EqualWeightMarketObservation, PricePathObservation,
};
use crate::strategy::{MarketView, RetailStyle, Rng, SelfView, StrategyData};
use crate::{RetailExperienceState, StockCode};

/// 一个 tick 内可由所有 NPC 共享的只读市场背景。
#[derive(Clone, Debug, PartialEq)]
pub struct BehaviorMarketObservation {
    pub price_paths: BTreeMap<StockCode, PricePathObservation>,
    pub thirty_minute_market: EqualWeightMarketObservation,
}

/// 判断层的动作；`Hold` 与 `Watch` 都不会自动生成委托，但语义不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionAction {
    Hold,
    Watch,
    TryBuy,
    Add,
    Reduce,
    Exit,
}

/// 从实际输入生成的主要判断理由，不是事后编造的心理描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionReason {
    PositionRisk,
    TakeProfit,
    Momentum,
    Pullback,
    BroadMarketRisk,
    /// 价格以真实完整分钟窗口突破此前区间高点，且量能确认。
    RangeBreakout,
    /// 价格以真实完整分钟窗口跌破此前区间低点。
    RangeBreakdown,
    /// 账户相对其可恢复净值峰值的回撤触发整体去风险；不表示某一只股票必然亏损。
    AccountDrawdown,
    BaselinePositioning,
    NoSignal,
    InsufficientHistory,
    T1Locked,
    LowConfidence,
    PostExitCooldown,
    BreakEvenRelief,
    ProfitGiveback,
}

/// 判断层输出。数量是相对当前持仓的目标差额，正数买入、负数卖出。
#[derive(Clone, Debug, PartialEq)]
pub struct PositionDecision {
    pub code: Option<StockCode>,
    pub action: PositionAction,
    pub reason: DecisionReason,
    pub target_position_fraction: f64,
    /// 相对当前持仓的完整目标差额；正数买入、负数卖出。
    pub desired_delta_shares: i64,
    /// 本轮在 T+1 与库存边界内可交给执行层的差额；仍需现金、整手和费用校验。
    pub executable_delta_shares: i64,
}

/// 固定行情与账户输入下构造散户目标仓位；暂由测试驱动补全。
pub fn decide_retail_position(
    strategy: &StrategyData,
    style: RetailStyle,
    market: &MarketView,
    own: &SelfView,
    observations: &BehaviorMarketObservation,
    account_risk: &AccountRiskObservation,
    rng: &mut dyn Rng,
) -> PositionDecision {
    decide_retail_position_inner(
        strategy,
        style,
        market,
        own,
        observations,
        account_risk,
        None,
        0,
        rng,
    )
}

/// 在 B02 瞬时判断上叠加该自然人的真实成交/观察经历。
#[allow(clippy::too_many_arguments)]
pub fn decide_retail_position_with_experience(
    strategy: &StrategyData,
    style: RetailStyle,
    market: &MarketView,
    own: &SelfView,
    observations: &BehaviorMarketObservation,
    account_risk: &AccountRiskObservation,
    experience: &RetailExperienceState,
    market_minute: u64,
    rng: &mut dyn Rng,
) -> PositionDecision {
    decide_retail_position_inner(
        strategy,
        style,
        market,
        own,
        observations,
        account_risk,
        Some(experience),
        market_minute,
        rng,
    )
}

mod decision;
mod heuristics;

use decision::decide_retail_position_inner;
