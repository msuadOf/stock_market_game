//! NPC 策略抽象（ADR-0006）。本文件仅为 trait 骨架：定义 Strategy/Intent/MarketView 类型，
//! 不含任何策略实现（ZI/Value/Momentum 三策略留作下一批次）。
//!
//! 设计：策略是纯函数式决策——看市场快照 + 自己的 RNG，返回一个「意图」(Intent)。
//! 策略不直接碰 orderbook，只产 Intent，由 account/market 层执行 → 可单测/可插拔/可并行。

use crate::money::Money;
use crate::orderbook::{OrderId, Side};

/// 策略决策产物。account/market 层据此执行（下单/撤单/不动）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Intent {
    /// 限价单：在 price 挂 qty 股。
    PlaceLimit { side: Side, price: Money, qty: u32 },
    /// 市价单（按对手最优即时成交）：挂 qty 股。（市价单延后支持，类型先就位）
    PlaceMarket { side: Side, qty: u32 },
    /// 撤单。
    Cancel { id: OrderId },
    /// 本 tick 不动作。
    Pass,
}

/// 策略可见的市场快照（只读）。最小字段集，后续随策略需求扩展（深度、近期价序列、公允价 V 等）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketView {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
}

/// 随机源抽象。生产用种子化 PRNG（ADR-0005），测试可注入固定实现。
/// 本 trait 让 Strategy 不绑定具体 RNG 实现（SplitMix64 等待 market 模块引入）。
pub trait Rng {
    /// 返回 [0, 1) 的 f64（用于泊松到达率、参数采样等）。NaN/Inf 不允许（实现方保证）。
    fn next_f64(&mut self) -> f64;
    /// 返回 [lo, hi) 的 u32。
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32;
}

/// NPC 下单策略的统一抽象（ADR-0006）。看市场快照 + 自己的 RNG，返回一个 Intent。
/// 玩家账户不实现此 trait（strategy = None，UI 动作直接产 Intent）。
pub trait Strategy {
    fn decide(&mut self, ctx: &MarketView, rng: &mut dyn Rng) -> Intent;
}
