//! 数据驱动统一参数表与 decide 内核入口（ADR-0006，为 GPU 化铺路）。

use super::*;

use super::{hot::decide_hot, institution::decide_inst, retail::decide_retail};
use crate::account::AccountKind;

// ─── 数据驱动策略（ADR-0006 数据化改造，为 GPU 化铺路）──────────────────────────
//
// 设计目标：把「每类一个 struct + impl Strategy」收敛为「统一参数表 StrategyData +
// 统一 decide 内核」。StrategyData 是一个扁平可序列化 struct——未来可直接映射到 GPU
// StorageBuffer（每个 NPC 一份参数）。CPU trait/struct 路径继续作为权威实现，
// 内部全部委托给这里的纯函数实现，保证「同种子同输出」不漂移。

/// 统一策略参数（数据驱动，可 serde → 未来塞进 GPU buffer）。
///
/// `kind` 决定走哪个 `decide` 分支；其余字段是三类 NPC 参数的并集（无关字段对该 kind 无效）。
/// 运行进度不存入策略数据；DriftUp 只读取 [`MarketView::market_minute`]，避免形成无法随存档恢复的
/// 第二套时钟。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrategyData {
    /// 账户种类：决定走哪个 decide 分支。
    pub kind: AccountKind,
    // ── 散户（Retail / ZiNoise）参数 ──
    /// 每次观察时的下单到达概率，∈[0,1]。
    pub arrival_rate: f64,
    /// 个体每单股数；由群体基准采样后写入。
    pub order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    pub tick_cents: i64,
    /// 近期跌幅达到该阈值后开始尝试抄底。
    pub dip_threshold: f64,
    /// 个人持仓亏损达到该阈值后触发止损。
    pub stop_loss_threshold: f64,
    /// 个人持仓盈利达到该阈值后倾向获利了结。
    pub take_profit_threshold: f64,
    /// 追涨所需的最低相对成交量。
    pub volume_confirmation: f64,
    /// 单只股票市值占总资产的上限。
    pub max_stock_fraction: f64,
    /// 个体在平静市场下每 tick 至少观察一次的基础概率，∈(0,1]。
    pub base_observation_probability: f64,
    // ── 机构（Inst / Value）参数 ──
    /// 容忍带宽度，∈[0,1)。
    pub margin: f64,
    /// 每单股数，>0（机构/游资共用字段名 order_size）。
    pub order_size: u32,
    /// 机构目标价策略。
    pub target_policy: TargetPolicy,
    // ── 游资（Hot / Momentum）参数 ──
    /// 回看完整交易分钟数，≥2。
    pub lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    pub trend_threshold: f64,
}

impl StrategyData {
    /// 构造散户参数集（inst/hot 字段填 0 占位，对该 kind 无效）。
    pub fn retail(
        arrival_rate: f64,
        order_size_mean: u32,
        chase_prob: f64,
        tick_cents: i64,
    ) -> Self {
        StrategyData {
            kind: AccountKind::Retail,
            arrival_rate,
            order_size_mean,
            chase_prob,
            tick_cents,
            dip_threshold: 0.02,
            stop_loss_threshold: 0.05,
            take_profit_threshold: 0.08,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.35,
            base_observation_probability: 1.0,
            margin: 0.0,
            order_size: 0,
            target_policy: TargetPolicy::Fixed(Money::ZERO),
            lookback: 0,
            trend_threshold: 0.0,
        }
    }

    /// 构造机构参数集（retail/hot 字段填占位）。
    pub fn inst(target_policy: TargetPolicy, margin: f64, order_size: u32) -> Self {
        StrategyData {
            kind: AccountKind::Inst,
            arrival_rate: 0.0,
            order_size_mean: 0,
            chase_prob: 0.0,
            tick_cents: 0,
            dip_threshold: 0.0,
            stop_loss_threshold: 0.0,
            take_profit_threshold: 0.0,
            volume_confirmation: 0.0,
            max_stock_fraction: 0.60,
            base_observation_probability: 1.0,
            margin,
            order_size,
            target_policy,
            lookback: 0,
            trend_threshold: 0.0,
        }
    }

    /// 构造游资参数集（retail/inst 字段填占位）。
    pub fn hot(lookback: usize, trend_threshold: f64, order_size: u32) -> Self {
        StrategyData {
            kind: AccountKind::Hot,
            arrival_rate: 0.0,
            order_size_mean: 0,
            chase_prob: 0.0,
            tick_cents: 0,
            dip_threshold: 0.0,
            stop_loss_threshold: 0.0,
            take_profit_threshold: 0.0,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.25,
            base_observation_probability: 1.0,
            margin: 0.0,
            order_size,
            target_policy: TargetPolicy::Fixed(Money::ZERO),
            lookback,
            trend_threshold,
        }
    }
}

/// 统一 decide 内核（数据驱动入口）。按 `strategy.kind` 分派到三类纯函数实现。
///
/// Player → 恒空 Vec（玩家不持算法策略）。
/// 数据路径与 trait 路径共享同一决策内核，确保同种子同输出（铁律三）。
pub fn decide_data(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
    rng: &mut dyn Rng,
) -> Vec<Intent> {
    match strategy.kind {
        AccountKind::Retail => decide_retail(strategy, market, own, rng),
        AccountKind::Inst => decide_inst(strategy, market, own),
        AccountKind::Hot => decide_hot(strategy, market, own),
        AccountKind::Player => Vec::new(),
    }
}
