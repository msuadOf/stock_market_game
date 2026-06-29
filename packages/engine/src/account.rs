//! 统一账户（ADR-0005 §2）：NPC 与玩家是同一种 Account，区别只在 strategy（NPC=算法、玩家=None）。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-account-design.md。
//! 只做单账户账务：现金、持仓、成本价（净投入/持仓）、T+1 锁定、资金/持仓校验。
//! 不碰撮合(orderbook)/行情(market)/策略实现(strategy，仅持 trait 引用)。

use crate::config::ConfigError;
use crate::money::{Money, MoneyError};
use crate::strategy::Strategy;
use std::collections::BTreeMap;
use thiserror::Error;

/// 股票代码占位 newtype（待 market 模块统一；本模块不依赖 market）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct StockCode(pub String);

/// 账户种类：NPC 三类 + 玩家。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AccountKind {
    /// 散户 NPC。
    Retail,
    /// 机构 NPC。
    Inst,
    /// 热钱 NPC。
    Hot,
    /// 玩家（真人，strategy=None）。
    Player,
}

/// 账户操作失败。绝不静默吞掉（铁律二），错误携带上下文。
#[derive(Debug, Error)]
pub enum AccountError {
    /// 买入资金不足：成交额+佣金 > 现金。
    #[error("insufficient cash: needed {needed:?}, have {have:?}")]
    InsufficientCash { needed: Money, have: Money },
    /// 卖出超卖：qty > 可卖持仓（持仓 − T+1锁定）。
    #[error("insufficient shares for {code:?}: needed {needed}, sellable {have}")]
    InsufficientShares { code: StockCode, needed: u32, have: u32 },
    /// 卖出/查询时无持仓。
    #[error("no position for {0:?}")]
    NoPosition(StockCode),
    /// 透传 money 错误（溢出/非法）。
    #[error(transparent)]
    MoneyErr(#[from] MoneyError),
    /// 透传 config 错误（费用计算）。
    #[error(transparent)]
    ConfigErr(#[from] ConfigError),
}

/// 占位：account 持 `Option<Strategy>`（ADR-0005 §2）。`Strategy` 在后续 Task 4 起被 Account 消费，
/// 当前 Task 2 仅建类型骨架。此处显式 `use` 避免 unused import 被误删，并在 clippy `-D warnings`
/// 下保持零告警：用 `Box<dyn Strategy>` 作为幻影字段，确保本任务编译期即绑定 strategy 依赖方向。
#[allow(dead_code)]
struct _AccountPlaceholder {
    _strategy: Option<Box<dyn Strategy>>,
    _positions: BTreeMap<StockCode, ()>,
}
