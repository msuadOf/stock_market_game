//! NPC 策略抽象与实现（ADR-0006）：统一数据模型、兼容 trait 与 ZI/价值/动量策略。
//!
//! 设计：策略是纯函数式决策——看多股市场快照 + 自己的快照 + 注入的 RNG，返回 0..N 个「意图」(Intent)。
//! 策略不直接碰 orderbook，只产 Intent，由 account/market 层执行 → 可单测/可插拔/可并行。

mod analysis_profile;
mod data;
mod factory;
mod factory_profiles;
mod hot;
mod institution;
mod momentum;
mod params;
mod profile;
mod retail;
mod sampling;
mod sizing;
mod value;
mod zi_noise;

pub use analysis_profile::{
    AnalysisProfile, AnalysisProfileError, AnalysisWeights, FundamentalMethod,
    PersistedAnalysisProfile,
};
pub use data::{decide_data, StrategyData};
pub use factory::StrategyFactory;
pub use factory_profiles::{
    default_analysis_weights, derive_analysis_profile, largest_remainder_normalize,
};
pub use momentum::MomentumStrategy;
pub use params::{HotParams, InstParams, RetailParams, StrategyParams};
pub use profile::{HotStyle, InstitutionStyle, RetailStyle, StrategyFamily, StrategyProfile};
pub(crate) use sizing::{a_share_sell_qty, risk_capped_buy_qty};
pub use value::{TargetPolicy, ValueStrategy};
pub use zi_noise::ZiNoiseStrategy;

use crate::account::StockCode;
use crate::behavior::{BehaviorMarketObservation, PositionDecision};
use crate::experience::RetailExperienceState;
use crate::money::Money;
use crate::observation::AccountRiskObservation;
use crate::orderbook::{OrderId, Side};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// 单只股票的市场视图（多股 MarketView 的元素）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockView {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
    /// 隐藏公允价 V；Some 仅对该策略可见（编排层决定：机构 Some、散户/游资/玩家 None）。
    pub fundamental_value: Option<Money>,
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

/// 策略决策产物。account/market 层据此执行（下单/撤单）；返回空 Vec 表示本 tick 不动作。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum Intent {
    /// 限价单：在 price 挂 qty 股。
    PlaceLimit {
        code: StockCode,
        side: Side,
        price: Money,
        qty: u32,
    },
    /// 市价单（按对手最优即时成交）：挂 qty 股。（市价单延后支持，类型先就位）
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
/// `reviewed_stocks` 为空表示旧策略没有声明完整目标；非空时 Session 必须把这些股票的
/// 买卖两侧工作单都与最新目标对齐，即使本次判断是 Hold/Watch 且没有新委托。
#[derive(Clone, Debug, Default)]
pub struct StrategyDecision {
    pub intents: Vec<Intent>,
    pub reviewed_stocks: BTreeSet<StockCode>,
    /// 仅散户 B02/B03 判断路径提供的目标仓位解释。Session 将其作为非权威诊断样本读取；
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

/// NPC 下单策略的统一抽象（ADR-0006）。看多股市场 + 自身快照 + 注入 RNG，返回 0..N 个 Intent。
/// 玩家账户不实现此 trait（strategy = None，UI 动作直接产 Intent）。
pub trait Strategy: Send + Sync {
    /// 可序列化的策略身份；会话把它写入权威存档以防恢复时静默换策略。
    fn profile(&self) -> StrategyProfile;
    /// 当前实例的决策策略族；用于审计身份与策略不再强制一一对应。
    fn strategy_family(&self) -> StrategyFamily;

    /// 是否需要读取隐藏公允价值 V。此能力由策略、而不是账户身份决定；会话层据此选择
    /// 含 V 或公共市场视图，防止非价值策略未来意外获得内部信息。
    fn needs_fundamental_value(&self) -> bool {
        self.strategy_family() == StrategyFamily::FundamentalValue
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
    ) -> StrategyDecision {
        assert_eq!(
            behavior_market.is_some(),
            account_risk.is_some(),
            "behavior market and account-risk observations must be supplied together"
        );
        StrategyDecision {
            intents: self.decide(market, own, rng),
            reviewed_stocks: BTreeSet::new(),
            position_decision: None,
        }
    }

    /// 带可恢复个体经历的决策入口；未接入经历的策略保持 B02 行为。
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
    ) -> StrategyDecision {
        self.decide_with_behavior(market, own, behavior_market, account_risk, rng)
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent>;
}

/// 策略构造/参数失败。绝不静默吞掉（铁律二）：非法参数一律 Err + 上报。
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    /// 参数非法（如 arrival_rate∉[0,1]、order_size=0）。param 指明哪个参数、reason 说明为何非法。
    #[error("invalid param {param}: {reason}")]
    InvalidParam { param: &'static str, reason: String },
}
