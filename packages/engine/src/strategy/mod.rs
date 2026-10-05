//! NPC 策略抽象与实现（ADR-0006）：统一数据模型与策略 trait（ZI/价值/动量）。
//!
//! 设计：策略是纯函数式决策——看多股市场快照 + 自己的快照 + 注入的 RNG，返回 0..N 个「意图」(Intent)。
//! 策略不直接碰 orderbook，只产 Intent，由 account/market 层执行 → 可单测/可插拔/可并行。

mod analysis_profile;
mod beliefs;
mod data;
mod factory;
mod factory_profiles;
mod fundamental;
pub(crate) use fundamental::{own_known_report_priority, preferred_own_report};
mod hot;
mod institution;
mod institution_experience_policy;
mod momentum;
mod params;
mod profile;
mod retail;
mod sampling;
mod sizing;
mod state;
mod technical;
mod value;
mod zi_noise;

pub use analysis_profile::{
    AnalysisProfile, AnalysisProfileError, AnalysisWeights, FundamentalMethod,
    PersistedAnalysisProfile,
};
pub use beliefs::{BeliefBook, BeliefEntry, BeliefError, BeliefInputs};
pub use data::{decide_data, StrategyData};
pub use factory::StrategyFactory;
pub use factory_profiles::{
    default_analysis_weights, derive_analysis_profile, largest_remainder_normalize,
};
pub use fundamental::{
    belief_horizon_days, capability_center, cash_flow, draw_personal_assumptions,
    earnings_multiple, equity_roe, estimate_by_method, extract_annual_facts, initial_forecast,
    observe_growth, per_share_price, revise_forecast, revision_lambda_bp, to_per_share_range,
    AnnualFacts, BeliefCause, CapabilityCenter, CauseRecord, ForecastBasis, ForecastState,
    GrowthObservation, PerShareRange, PersonalAssumptions, PriorRevenue, ScenarioEstimates,
    ValuationOutcome, ValuationUnavailable, GROWTH_PRIOR_CLAMP_BP,
};
pub use institution_experience_policy::{
    InstitutionExperiencePolicy, InstitutionExperiencePolicyError, InstitutionLossResponse,
    PersistedInstitutionExperiencePolicy,
};
pub use momentum::MomentumStrategy;
pub use params::{HotParams, InstParams, RetailParams, StrategyParams};
pub use profile::{HotStyle, InstitutionStyle, RetailStyle, StrategyFamily, StrategyProfile};
pub(crate) use sizing::{a_share_sell_qty, affordable_buy_qty};
pub use state::{ProductionStrategy, StrategyState, StrategyStateError};
pub use technical::{
    atr14, rsi14, sma, AverageTrueRange, RelativeStrengthIndex, SimpleMovingAverage,
    TechnicalDailyBar, TechnicalError, ATR_WINDOW, RSI_WINDOW, SMA_LONG_WINDOW, SMA_SHORT_WINDOW,
};
pub use value::{BeliefInstitutionStrategy, TargetPolicy};
pub use zi_noise::ZiNoiseStrategy;

use crate::account::StockCode;
use crate::behavior::{BehaviorMarketObservation, PositionDecision};
use crate::config::GameConfig;
use crate::experience::RetailExperienceState;
use crate::money::Money;
use crate::observation::AccountRiskObservation;
use crate::orderbook::{OrderId, Side};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// 单只股票的市场视图（多股 MarketView 的元素）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockView {
    pub is_trading: bool,
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
    /// 本观察版本与交易阶段允许的最高买入限价，已取涨停价与启用的价格笼子交集。
    /// 到达市场前盘口仍可能改变，权威受理会再次校验。
    pub max_buy_price: Money,
    /// 本交易日的涨停价；符号最高买价在受理时可能高于观察到的笼子上限，买量据此预留资金。
    pub daily_upper_limit: Money,
    /// 本观察版本与交易阶段允许的最低卖出限价，已取跌停价与启用的价格笼子交集。
    pub min_sell_price: Money,
    /// 最近 N 个 last_price（滚动窗口，供 tick 级观察使用）。
    ///
    /// 仅保留给短期盘口、注意力等 tick 级观测；游资趋势不得读取它。
    pub recent_prices: Vec<Money>,
    /// 当前交易日已经完成的标准交易分钟收盘价；游资趋势窗口使用该序列。
    /// 不足两个完整分钟时趋势显式不可用，不能以 tick 点数补齐时间跨度。
    pub recent_market_minute_prices: Vec<Money>,
    /// 当前交易日截至本 tick 的成交量，相对于历史同期预期量的倍率。
    pub relative_volume: f64,
    /// 前五档买卖盘数量失衡，范围 [-1,1]；正值表示买盘更厚，负值表示卖盘更厚。
    pub order_book_imbalance: f64,
}

/// 整个市场快照（多股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketView {
    pub stocks: BTreeMap<StockCode, StockView>,
    /// 当前权威游戏 tick；用于逐 tick 的注意力等采样。
    pub tick: u64,
    /// 从开局累计的已完成标准交易分钟；用于依赖市场时间的策略（例如 DriftUp）。
    pub market_minute: u64,
}

/// 策略所属账户的自身快照（跨股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SelfView {
    /// 当前决策可用现金：账户现金扣除不可撤换委托的冻结额；本观察点会原子撤换的
    /// NPC 工作单冻结额可继续用于同一轮目标报价。
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionView>,
}

/// 单只股票的持仓视图。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PositionView {
    pub qty: u32,
    pub sellable_qty: u32,
    pub cost_price: Option<Money>,
}

/// 限价意图的价格选择；符号价格在受理时按权威盘口与当前交易规则解析。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum LimitPrice {
    Fixed(Money),
    Highest,
    Lowest,
}

/// 策略决策产物。account/market 层据此执行（下单/撤单）；返回空 Vec 表示本 tick 不动作。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum Intent {
    /// 限价单：Fixed 指定价格；Highest/Lowest 在权威受理时解析为当前合法边界。
    PlaceLimit {
        code: StockCode,
        side: Side,
        price: LimitPrice,
        qty: u32,
    },
    /// 市价意图：以当日涨跌停价作保护，逐档即时成交，未成交余量撤销。
    /// 当前统一处理沪深市场，具体市价申报类型的差异见 docs/trading-rules.md 的游戏简化。
    PlaceMarket {
        code: StockCode,
        side: Side,
        qty: u32,
    },
    /// 撤单。
    Cancel { code: StockCode, id: OrderId },
}

/// 一次策略评估的委托与工作单对齐范围。
///
/// `reviewed_stocks` 指出本轮实际复核的股票；Session 必须把这些股票的
/// 买卖两侧工作单都与最新目标对齐，即使本次判断是 Hold/Watch 且没有新委托。
#[derive(Clone, Debug, Default)]
pub struct StrategyDecision {
    pub intents: Vec<Intent>,
    pub reviewed_stocks: BTreeSet<StockCode>,
    /// 仅散户目标仓位与经历调整路径提供目标仓位解释。Session 将其作为非权威诊断样本读取；
    /// 它不参与存档、撮合或下一 tick 的决策。
    pub position_decision: Option<PositionDecision>,
}

/// 随机源抽象。生产用种子化 PRNG（ADR-0005），测试可注入固定实现。
/// 本 trait 让 Strategy 不绑定具体 RNG 实现（SplitMix64 等待 market 模块引入）。
pub trait Rng {
    /// 返回 [0, 1) 的 f64（用于泊松到达率、参数采样等）。NaN/Inf 不允许（实现方保证）。
    fn next_f64(&mut self) -> f64;
    /// 返回 [lo, hi) 的 u32。
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32;
}

/// 信念机构决策链参数（会话侧候选评分与计划执行读取；非信念策略恒 `None`）。
///
/// 机构的估值与方向只来自账户的 [`BeliefBook`] 与混合分析
/// 聚合；本结构只暴露个体规模/容忍带等行为参数，不携带任何隐藏市场信息。
#[derive(Clone, Copy, Debug)]
pub struct BeliefChainParams {
    /// 最强方向信号的一次目标调整步幅（100 bp = 1 个百分点）。
    pub position_step_bp: u32,
    /// 容忍带宽度（保留为个体行为参数）。
    pub margin: f64,
    /// 个体每单股数（子单上限）。
    pub order_size: u32,
    /// Whether an existing plan is reviewed at the first tick of every trading day.
    pub daily_plan_review: bool,
}

/// NPC 下单策略的统一抽象（ADR-0006）。看多股市场 + 自身快照 + 注入 RNG，返回 0..N 个 Intent。
/// 玩家账户不实现此 trait（strategy = None，UI 动作直接产 Intent）。
pub trait Strategy: Send + Sync {
    /// 可序列化的策略身份；会话把它写入权威存档以防恢复时静默换策略。
    fn profile(&self) -> StrategyProfile;
    /// 当前实例的决策策略族；用于审计身份与策略不再强制一一对应。
    fn strategy_family(&self) -> StrategyFamily;

    /// 信念决策链参数（能力探针）：仅信念机构策略返回 `Some`。
    fn belief_chain_params(&self) -> Option<BeliefChainParams> {
        None
    }

    /// 是否把策略给出的限价目标交给会话层按母单执行。
    ///
    /// 默认的逐笔意图语义保持不变；只有主动选择该模式的策略才会由
    /// `GameSession` 将目标拆分为可恢复的子单，并以真实成交推进进度。
    fn uses_parent_order_execution(&self) -> bool {
        false
    }
    fn retail_style(&self) -> Option<RetailStyle> {
        None
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        None
    }

    fn hot_style(&self) -> Option<HotStyle> {
        None
    }

    /// 平静市场下每 tick 至少观察一次的基础概率。Session 使用它初始化个体注意力调度；
    /// 行情刺激只会在此基础上提高概率。
    fn base_observation_probability(&self) -> f64 {
        1.0
    }

    /// 空决策是否代表“已重新评估且现有报价失效”。事件到达型策略的空结果包含
    /// 未发生随机到达、到达后无可执行意图，均不能据此伪造撤单；持续扫描型策略返回 true。
    fn updates_working_quotes_on_empty_decision(&self) -> bool {
        true
    }

    /// 带标准市场时间与账户风险观测的决策入口。未接入新闭环的策略沿用原决定；
    /// 会话对散户始终提供两类观测，不能只提供其中之一。
    fn decide_with_behavior(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        behavior_market: Option<&BehaviorMarketObservation>,
        account_risk: Option<&AccountRiskObservation>,
        rng: &mut dyn Rng,
        config: &GameConfig,
    ) -> StrategyDecision {
        assert_eq!(
            behavior_market.is_some(),
            account_risk.is_some(),
            "behavior market and account-risk observations must be supplied together"
        );
        StrategyDecision {
            intents: self.decide(market, own, rng, config),
            reviewed_stocks: BTreeSet::new(),
            position_decision: None,
        }
    }

    /// 带可恢复个体经历的决策入口；未接入经历的策略保持瞬时目标仓位判断行为。
    #[allow(clippy::too_many_arguments)]
    fn decide_with_experience(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        behavior_market: Option<&BehaviorMarketObservation>,
        account_risk: Option<&AccountRiskObservation>,
        _experience: Option<&RetailExperienceState>,
        _market_minute: u64,
        rng: &mut dyn Rng,
        config: &GameConfig,
    ) -> StrategyDecision {
        self.decide_with_behavior(market, own, behavior_market, account_risk, rng, config)
    }

    /// `config` 为本局权威费用配置，数量估算须包含真实费用。
    fn decide(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        rng: &mut dyn Rng,
        config: &GameConfig,
    ) -> Vec<Intent>;
}

/// 策略构造/参数失败。绝不静默吞掉（铁律二）：非法参数一律 Err + 上报。
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    /// 参数非法（如 arrival_rate∉[0,1]、order_size=0）。param 指明哪个参数、reason 说明为何非法。
    #[error("invalid param {param}: {reason}")]
    InvalidParam { param: &'static str, reason: String },
}
