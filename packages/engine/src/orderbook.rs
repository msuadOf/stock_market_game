//! 订单簿撮合引擎：价格-时间优先的限价撮合。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-orderbook-design.md。
//! ADR-0005 §3 撮合驱动价格的核心；纯逻辑，只依赖 Money，与 account/market/strategy 解耦。
//!
//! 当前为 Task 1 骨架：仅 Side/OrderId/OrderError，让 lib.rs 导出编译通过。
//! Order/Trade/MatchResult/OrderBook 与撮合逻辑在后续 Task 2-7 补齐。

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
