//! 单只股票的市场状态层（ADR-0005 §3）：包装 OrderBook + 价格/涨跌停/隐藏公允价 V。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-market-design.md。
//! 只做单股状态：涨跌停拒单、last_price 记录、V 演化、日终重置。
//! 不碰 account 结算、不碰全 tick 编排（session/simulator）。

use crate::account::StockCode;
use crate::money::{Money, MoneyError};
use crate::orderbook::OrderError;
use thiserror::Error;

/// market 操作失败。绝不静默吞掉（铁律二）。
#[derive(Debug, Error)]
pub enum MarketError {
    /// 下单价超出涨跌停范围 [down_stop, up_stop]。
    #[error("limit exceeded for {code:?}: price {price:?} not in [{down:?}, {up:?}]")]
    LimitExceeded {
        code: StockCode,
        price: Money,
        down: Money,
        up: Money,
    },
    /// 透传 orderbook 错误（非法价格/数量/tick）。
    #[error(transparent)]
    OrderBook(#[from] OrderError),
    /// 透传 money 错误（涨跌停价/apply_rate 溢出）。
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// V 演化参数非法（volatility/mean_reversion 负或非有限；V 跨零）。
    #[error("invalid v params: {reason}")]
    InvalidVParams { reason: String },
}

/// V（隐藏公允价）演化参数。均值回复几何随机游走。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VParams {
    /// 长期均值（V 向其回复）。
    pub long_run_mean: Money,
    /// 回复速度 α（≥0）。
    pub mean_reversion: f64,
    /// 随机扰动幅度 σ（≥0）。
    pub volatility: f64,
}
