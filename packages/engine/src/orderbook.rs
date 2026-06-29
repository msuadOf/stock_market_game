//! 订单簿撮合引擎：价格-时间优先的限价撮合。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-orderbook-design.md。
//! ADR-0005 §3 撮合驱动价格的核心；纯逻辑，只依赖 Money，与 account/market/strategy 解耦。
//!
//! 当前为 Task 1/2：Side/OrderId/OrderError + Order/Trade/MatchResult 数据结构。
//! OrderBook 与撮合逻辑在后续 Task 3-7 补齐。

use thiserror::Error;

use crate::money::Money;

/// 买卖方向。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side {
    /// 买单（愿意买入）。
    Buy,
    /// 卖单（愿意卖出）。
    Sell,
}

/// 订单 id（单调自增）。本模块自带 newtype，不依赖未来 account 模块。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct OrderId(pub u64);

/// orderbook 操作失败。绝不静默吞掉（铁律二），错误携带字段名 / 实际值 / 原因。
#[derive(Debug, Error)]
pub enum OrderError {
    /// 价格非法：为负 / 非 tick 整数倍 / 超范围。
    #[error("invalid price {price:?}: {reason} (tick {tick:?})")]
    InvalidPrice {
        /// 被拒的订单价格。
        price: Money,
        /// 当前簿的最小变动单位（用于诊断「非整数倍」）。
        tick: Money,
        /// 失败原因（如 "negative" / "not a multiple of tick"）。
        reason: String,
    },
    /// 数量非法：qty == 0。
    #[error("invalid qty: {0} (must be > 0)")]
    InvalidQty(u32),
    /// 重复订单 id（防御式，自增分配器正常时不应触发）。
    #[error("duplicate order id: {0:?}")]
    DuplicateOrderId(OrderId),
    /// 撤单时 id 不存在。
    #[error("order not found: {0:?}")]
    OrderNotFound(OrderId),
    /// 构造订单簿时 tick 非法：<= 0（价格最小变动必须为正）。
    #[error("invalid tick: {tick:?} (must be > 0)")]
    InvalidTick {
        /// 被拒的 tick 值。
        tick: Money,
    },
}

/// 账户 id 占位 newtype（待 account 模块统一；本模块不依赖 account）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct AccountId(pub u64);

/// 单笔限价挂单（撮合发生时为不可变快照；簿内以 OrderId/seq 引用）。
///
/// 价格全程定点 [`Money`]（分），绝不存 f64（money 模块铁律）。可序列化供存档/快照。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Order {
    /// 订单唯一 id。
    pub id: OrderId,
    /// 买/卖方向。
    pub side: Side,
    /// 限价（愿意成交的价格）。
    pub price: Money,
    /// 剩余数量（股）。
    pub qty: u32,
    /// 挂单所属账户。
    pub owner: AccountId,
    /// 时间序：同价位排序键（先挂先成交，price-time priority）。
    pub seq: u64,
}

/// 一笔成交。成交价取被动方（maker）的价格。
///
/// 派生 `Eq`+`PartialEq` 便于测试整体比较与去重；可序列化供成交历史/存档。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Trade {
    /// 成交价（= maker 挂单价）。
    pub price: Money,
    /// 本次成交数量（股）。
    pub qty: u32,
    /// 被动方（较早挂单、提供流动性）。
    pub maker: AccountId,
    /// 主动方（新进入、吃流动性）。
    pub taker: AccountId,
}

/// 撮合一笔新单的结果。
///
/// `trades` 为本次产生的成交（按发生顺序）；`resting` 为新单若有剩余量挂入簿的残留订单，
/// `None` 表示全成交。设计为值类型，便于上层（account/strategy）据此回写状态。
pub struct MatchResult {
    /// 本次撮合产生的成交（按发生顺序）。
    pub trades: Vec<Trade>,
    /// 新单若有剩余量，挂入簿的残留订单；None 表示全成交。
    pub resting: Option<Order>,
}
