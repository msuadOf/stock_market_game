//! NPC 策略抽象（ADR-0006）。本文件为 trait 骨架：定义 Strategy/Intent/MarketView 等类型，
//! 不含任何策略实现（ZI/Value/Momentum 三策略留作下一批次）。
//!
//! 设计：策略是纯函数式决策——看多股市场快照 + 自己的快照 + 注入的 RNG，返回 0..N 个「意图」(Intent)。
//! 策略不直接碰 orderbook，只产 Intent，由 account/market 层执行 → 可单测/可插拔/可并行。

use crate::account::{AccountKind, StockCode};
use crate::money::Money;
use crate::orderbook::{OrderId, Side};
use std::collections::BTreeMap;

// ─── 数据驱动策略（ADR-0006 数据化改造，为 GPU 化铺路）──────────────────────────
//
// 设计目标：把「每类一个 struct + impl Strategy」收敛为「统一参数表 StrategyData +
// 统一 decide 内核」。StrategyData 是一个扁平可序列化 struct——未来可直接映射到 GPU
// StorageBuffer（每个 NPC 一份参数 + 状态）。旧的 trait/struct 路径保留为兼容层，
// 内部全部委托给这里的纯函数实现，保证「同种子同输出」不漂移。

/// 统一策略参数 + 可变状态（数据驱动，可 serde → 未来塞进 GPU buffer）。
///
/// `kind` 决定走哪个 decide 分支；其余字段是三类 NPC 参数的并集（无关字段对该 kind 无效）。
/// `ticks` 是机构 DriftUp 目标价漂移的运行时计数器——它是**状态**而非参数，但为了 GPU 路径
/// 能把「参数+状态」一次性灌进 buffer，这里把它和数据放一起（CPU 路径每 tick 自增）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrategyData {
    /// 账户种类：决定走哪个 decide 分支。
    pub kind: AccountKind,
    // ── 散户（Retail / ZiNoise）参数 ──
    /// 每 tick 到达概率，∈[0,1]。
    pub arrival_rate: f64,
    /// 每单股数（均值，v1 直接取定值）。
    pub order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    pub tick_cents: i64,
    // ── 机构（Inst / Value）参数 ──
    /// 容忍带宽度，∈[0,1)。
    pub margin: f64,
    /// 每单股数，>0（机构/游资共用字段名 order_size）。
    pub order_size: u32,
    /// 机构目标价策略。
    pub target_policy: TargetPolicy,
    // ── 游资（Hot / Momentum）参数 ──
    /// 回看点数，≥2。
    pub lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    pub trend_threshold: f64,
    // ── 运行时状态（机构 DriftUp 用）──
    /// 已参与的 tick 数（DriftUp 目标价漂移；CPU 路径每 decide 自增）。
    pub ticks: u64,
}

impl StrategyData {
    /// 构造散户参数集（inst/hot 字段填 0 占位，对该 kind 无效）。
    pub fn retail(arrival_rate: f64, order_size_mean: u32, chase_prob: f64, tick_cents: i64) -> Self {
        StrategyData {
            kind: AccountKind::Retail,
            arrival_rate,
            order_size_mean,
            chase_prob,
            tick_cents,
            margin: 0.0,
            order_size: 0,
            target_policy: TargetPolicy::Fixed(Money::ZERO),
            lookback: 0,
            trend_threshold: 0.0,
            ticks: 0,
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
            margin,
            order_size,
            target_policy,
            lookback: 0,
            trend_threshold: 0.0,
            ticks: 0,
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
            margin: 0.0,
            order_size,
            target_policy: TargetPolicy::Fixed(Money::ZERO),
            lookback,
            trend_threshold,
            ticks: 0,
        }
    }
}

/// 统一 decide 内核（数据驱动入口）。按 `strategy.kind` 分派到三类纯函数实现。
///
/// Player → 恒空 Vec（玩家不持算法策略）。
/// **不改变行为**：与旧 trait 路径同种子同输出（铁律三）。
pub fn decide_data(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
    rng: &mut dyn Rng,
) -> Vec<Intent> {
    match strategy.kind {
        AccountKind::Retail => decide_retail(strategy, market, rng),
        AccountKind::Inst => decide_inst(strategy, market, own),
        AccountKind::Hot => decide_hot(strategy, market, own),
        AccountKind::Player => Vec::new(),
    }
}

/// 散户 decide 内核（零智力泊松到达 + 追涨杀跌）。与旧 `ZiNoiseStrategy::decide` 逐行等价。
fn decide_retail(strategy: &StrategyData, market: &MarketView, rng: &mut dyn Rng) -> Vec<Intent> {
    if market.stocks.is_empty() || rng.next_f64() >= strategy.arrival_rate {
        return Vec::new();
    }
    let (code, sv) = match market.stocks.first_key_value() {
        Some((c, v)) => (c.clone(), v),
        None => return Vec::new(),
    };
    if rng.next_f64() < strategy.chase_prob {
        if trend_up(sv) {
            return vec![Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: sv.last_price,
                qty: strategy.order_size_mean,
            }];
        } else if trend_down(sv) {
            return vec![Intent::PlaceLimit {
                code,
                side: Side::Sell,
                price: sv.last_price,
                qty: strategy.order_size_mean,
            }];
        }
    }
    let side = if rng.next_f64() < 0.5 {
        Side::Buy
    } else {
        Side::Sell
    };
    let price = match side {
        Side::Buy => Money::from_cents(sv.best_bid.unwrap_or(sv.last_price).cents() + strategy.tick_cents),
        Side::Sell => Money::from_cents(
            (sv.best_ask.unwrap_or(sv.last_price).cents() - strategy.tick_cents).max(0),
        ),
    };
    vec![Intent::PlaceLimit {
        code,
        side,
        price,
        qty: strategy.order_size_mean,
    }]
}

/// 机构 decide 内核（基本面价值策略）。与旧 `ValueStrategy::decide` 逐行等价。
/// 注意：DriftUp 的 ticks 自增由调用方负责（数据驱动下状态外置，便于 GPU 路径统一灌 buffer）。
fn decide_inst(strategy: &StrategyData, market: &MarketView, own: &SelfView) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let target = match target_cents(&strategy.target_policy, sv.fundamental_value, strategy.ticks) {
            Some(t) => t,
            None => continue,
        };
        let last = sv.last_price.cents() as f64;
        let low = target * (1.0 - strategy.margin);
        let high = target * (1.0 + strategy.margin);
        if last < low {
            out.push(Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: sv.last_price,
                qty: strategy.order_size,
            });
        } else if last > high {
            let sellable = own.positions.get(code).map(|p| p.sellable_qty).unwrap_or(0);
            if sellable > 0 {
                let qty = strategy.order_size.min(sellable);
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sv.last_price,
                    qty,
                });
            }
        }
    }
    out
}

/// 游资 decide 内核（动量策略）。与旧 `MomentumStrategy::decide` 逐行等价。
fn decide_hot(strategy: &StrategyData, market: &MarketView, own: &SelfView) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let p = &sv.recent_prices;
        if p.len() < 2 {
            continue;
        }
        let start = p.len().saturating_sub(strategy.lookback);
        let first = p[start].cents() as f64;
        let last = p.last().unwrap().cents() as f64;
        if first <= 0.0 {
            continue;
        }
        let change = (last - first) / first;
        if change > strategy.trend_threshold {
            out.push(Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: sv.last_price,
                qty: strategy.order_size,
            });
        } else if change < -strategy.trend_threshold {
            let sellable = own.positions.get(code).map(|pp| pp.sellable_qty).unwrap_or(0);
            if sellable > 0 {
                let qty = strategy.order_size.min(sellable);
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sv.last_price,
                    qty,
                });
            }
        }
    }
    out
}

/// 依目标价策略算目标价（返回 cents 的 f64）。V 不可见且 TrackV → None。
/// 抽出为自由函数，供数据驱动内核与旧 ValueStrategy 共用（保证两者永不漂移）。
fn target_cents(policy: &TargetPolicy, v: Option<Money>, ticks: u64) -> Option<f64> {
    match policy {
        TargetPolicy::Fixed(m) => Some(m.cents() as f64),
        TargetPolicy::TrackV { bias } => v.map(|vv| vv.cents() as f64 * (1.0 + bias)),
        TargetPolicy::DriftUp { rate, base } => {
            Some(base.cents() as f64 * (1.0 + rate * ticks as f64))
        }
    }
}

/// 单只股票的市场视图（多股 MarketView 的元素）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockView {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
    /// 隐藏公允价 V；Some 仅对该策略可见（编排层决定：机构 Some、散户/游资/玩家 None）。
    pub fundamental_value: Option<Money>,
    /// 最近 N 个 last_price（滚动窗口，游资趋势检测用）。
    pub recent_prices: Vec<Money>,
}

/// 整个市场快照（多股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketView {
    pub stocks: BTreeMap<StockCode, StockView>,
}

/// 策略所属账户的自身快照（跨股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SelfView {
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
    Cancel {
        code: StockCode,
        id: OrderId,
    },
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
    fn decide(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        rng: &mut dyn Rng,
    ) -> Vec<Intent>;
}

/// 策略构造/参数失败。绝不静默吞掉（铁律二）：非法参数一律 Err + 上报。
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    /// 参数非法（如 arrival_rate∉[0,1]、order_size=0）。param 指明哪个参数、reason 说明为何非法。
    #[error("invalid param {param}: {reason}")]
    InvalidParam { param: &'static str, reason: String },
}

/// 散户：零智力(ZI)泊松到达 + 少量追涨杀跌。
///
/// 每 tick 以 arrival_rate 概率「到达」并下一单；若到达，按 chase_prob 概率
/// 顺近期趋势（追涨/杀跌），否则随机买卖各半。价格在最优买卖盘基础上跨一个 tick。
/// 纯逻辑：所有随机经注入 `&mut dyn Rng`（可重放、可单测）；价格用 Money，禁止 f64 存储权威状态。
pub struct ZiNoiseStrategy {
    /// 每 tick 到达概率，∈[0,1]。0 → 永不动作。
    arrival_rate: f64,
    /// 每单股数（均值，v1 直接取定值）。
    order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    tick_cents: i64,
}

impl ZiNoiseStrategy {
    /// 构造并校验参数。任一非法 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(
        arrival_rate: f64,
        order_size_mean: u32,
        chase_prob: f64,
        tick_cents: i64,
    ) -> Result<Self, StrategyError> {
        if !(0.0..=1.0).contains(&arrival_rate) {
            return Err(StrategyError::InvalidParam {
                param: "arrival_rate",
                reason: format!("{arrival_rate} not in [0,1]"),
            });
        }
        if order_size_mean == 0 {
            return Err(StrategyError::InvalidParam {
                param: "order_size_mean",
                reason: "must be > 0".to_string(),
            });
        }
        if !(0.0..=1.0).contains(&chase_prob) {
            return Err(StrategyError::InvalidParam {
                param: "chase_prob",
                reason: format!("{chase_prob} not in [0,1]"),
            });
        }
        if tick_cents <= 0 {
            return Err(StrategyError::InvalidParam {
                param: "tick_cents",
                reason: "must be > 0".to_string(),
            });
        }
        Ok(ZiNoiseStrategy {
            arrival_rate,
            order_size_mean,
            chase_prob,
            tick_cents,
        })
    }
}

impl Strategy for ZiNoiseStrategy {
    fn decide(&mut self, market: &MarketView, _own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）：旧 struct 字段映射成 StrategyData，
        // 调统一纯函数 decide_retail，保证「同种子同输出」不漂移。
        let data = StrategyData::retail(
            self.arrival_rate,
            self.order_size_mean,
            self.chase_prob,
            self.tick_cents,
        );
        decide_retail(&data, market, rng)
    }
}

/// recent_prices 末段是否上升（至少 2 个点且末>首）。趋势判断纯整数，无 f64。
fn trend_up(sv: &StockView) -> bool {
    let p = &sv.recent_prices;
    p.len() >= 2 && p.last().unwrap().cents() > p.first().unwrap().cents()
}

/// recent_prices 末段是否下跌（至少 2 个点且末<首）。
fn trend_down(sv: &StockView) -> bool {
    let p = &sv.recent_prices;
    p.len() >= 2 && p.last().unwrap().cents() < p.first().unwrap().cents()
}

/// 机构目标价策略（每实例一种，机构看法各异）。
///
/// - `Fixed(m)`：固定目标价 m。
/// - `TrackV { bias }`：跟随隐藏公允价 V，target = V × (1 + bias)。
/// - `DriftUp { rate, base }`：认为大致上涨，target = base × (1 + rate × ticks)。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum TargetPolicy {
    /// 固定目标价。
    Fixed(Money),
    /// 跟随 V：target = V × (1 + bias)。
    TrackV { bias: f64 },
    /// 认为大致上涨：target = base × (1 + rate × ticks)。
    DriftUp { rate: f64, base: Money },
}

/// 机构：基本面价值策略。读隐藏 V + 目标价策略，低买高卖（带 margin 容忍带）。
///
/// 纯逻辑：价格/qty 用 Money/u32；f64 仅在 target/band 计算边界，立即落回 Intent。
/// 不直接碰 orderbook，只产 Intent。
pub struct ValueStrategy {
    policy: TargetPolicy,
    /// 容忍带宽度，∈[0,1)。last 落在 [target×(1-margin), target×(1+margin)] 内不动作。
    margin: f64,
    /// 每单股数（>0）。
    order_size: u32,
    /// 已参与的 tick 数（DriftUp 目标价漂移用；decide 自增）。
    ticks: u64,
}

impl ValueStrategy {
    /// 构造并校验参数。margin∉[0,1) 或 order_size=0 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(
        policy: TargetPolicy,
        margin: f64,
        order_size: u32,
    ) -> Result<Self, StrategyError> {
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
            policy,
            margin,
            order_size,
            ticks: 0,
        })
    }
}

impl Strategy for ValueStrategy {
    fn decide(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        _rng: &mut dyn Rng,
    ) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）。
        // 注意：旧实现先自增 ticks 再算目标价（target 读自增后的值）。为保持逐 Intent 等价，
        // 这里同样先自增，再把自增后的 ticks 灌进 StrategyData（decide_inst 不再自增）。
        self.ticks += 1;
        let mut data = StrategyData::inst(self.policy.clone(), self.margin, self.order_size);
        data.ticks = self.ticks;
        decide_inst(&data, market, own)
    }
}

/// 游资：短期趋势/动量策略，快进快出。
///
/// 看每只股票的近期成交价窗口：取最近 `lookback` 个点，算相对变化 change=(末-首)/首。
/// change > threshold → 追涨买入；change < -threshold 且有可卖持仓 → 杀跌卖出。
/// 持仓不足或趋势不明（|change| ≤ threshold、点数不足）→ 不动作。
///
/// 纯逻辑：随机经注入 `&mut dyn Rng`（本策略实际不消费 RNG，签名对齐 trait）；价格/qty 用 Money/u32；
/// f64 仅在 change 计算边界，立即落回 Intent。不直接碰 orderbook，只产 Intent。
pub struct MomentumStrategy {
    /// 回看点数，≥2。
    lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    trend_threshold: f64,
    /// 每单股数，>0。
    order_size: u32,
}

impl MomentumStrategy {
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
            lookback,
            trend_threshold,
            order_size,
        })
    }
}

impl Strategy for MomentumStrategy {
    fn decide(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        _rng: &mut dyn Rng,
    ) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）：字段映射成 StrategyData，
        // 调统一纯函数 decide_hot，保证「同种子同输出」不漂移。
        let data = StrategyData::hot(self.lookback, self.trend_threshold, self.order_size);
        decide_hot(&data, market, own)
    }
}

/// 散户策略分布参数（每实例从中采样/直接取）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RetailParams {
    /// 每 tick 到达概率，∈[0,1]。
    pub arrival_rate: f64,
    /// 每单股数（均值，v1 直接取定值）。
    pub order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    pub tick_cents: i64,
}

/// 机构策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InstParams {
    /// 容忍带宽度，∈[0,1)。
    pub margin: f64,
    /// 每单股数，>0。
    pub order_size: u32,
}

/// 游资策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HotParams {
    /// 回看点数，≥2。
    pub lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    pub trend_threshold: f64,
    /// 每单股数，>0。
    pub order_size: u32,
}

/// 每类 NPC 的策略参数（v1 同类 NPC 参数相同，直接从配置取）。
///
/// 后续可扩展为分布（均值/方差），由 `StrategyFactory` 经注入 RNG 对每实例微扰——
/// 本批次先打通工厂链路，参数差异化是后续增强。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrategyParams {
    /// 散户参数。
    pub retail: RetailParams,
    /// 机构参数。
    pub inst: InstParams,
    /// 游资参数。
    pub hot: HotParams,
}

/// 策略工厂：按账户种类构造策略实例。
///
/// Player → `None`（玩家不持算法策略，UI 动作直接产 Intent）；
/// Retail/Inst/Hot → 按各自 `StrategyParams` 构造对应策略。
/// 构造失败（参数非法）→ 该分支返回 `None`（`?` 在返回 `Option` 的 fn 内），
/// **不静默用默认值**：参数合法性应在配置层把关，工厂把构造 `Err` 显式上抛为 `None`。
pub struct StrategyFactory;

impl StrategyFactory {
    /// 按种类构造策略。`_rng` 预留（v1 同类参数相同；后续差异化采样用）。
    pub fn build(
        kind: AccountKind,
        params: &StrategyParams,
        _rng: &mut dyn Rng,
    ) -> Option<Box<dyn Strategy + Send + Sync>> {
        match kind {
            AccountKind::Retail => {
                let r = &params.retail;
                Some(Box::new(
                    ZiNoiseStrategy::new(r.arrival_rate, r.order_size_mean, r.chase_prob, r.tick_cents).ok()?,
                ))
            }
            AccountKind::Inst => {
                let i = &params.inst;
                // 机构目标价：v1 用 TrackV{bias:0}（跟随隐藏 V）；后续可按实例采样 bias。
                Some(Box::new(
                    ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, i.margin, i.order_size).ok()?,
                ))
            }
            AccountKind::Hot => {
                let h = &params.hot;
                Some(Box::new(
                    MomentumStrategy::new(h.lookback, h.trend_threshold, h.order_size).ok()?,
                ))
            }
            AccountKind::Player => None,
        }
    }
}
