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

/// 单只股票的持仓。权威状态全整数分（invested/recovered 累加器），无 f64。
///
/// 成本价 = (invested − recovered) / qty（净投入/持仓模型，ADR-0005/spec §2）：
/// 卖出收回超过投入时分子为负 → 成本价为负（允许，反映已实现盈利超过成本）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Position {
    /// 总持仓股数（含 t1_locked）。
    pub qty: u32,
    /// 当日买入锁定（T+1 日终解锁；T+0 时始终 0）。
    pub t1_locked: u32,
    /// 总买入成交额（分）。仅成交额，不含费用。
    pub invested_cents: i64,
    /// 总卖出成交额（分）。仅成交额，不含费用。
    pub recovered_cents: i64,
}

impl Position {
    /// 可卖股数 = 总持仓 − T+1 锁定。
    pub fn sellable(&self) -> u32 {
        self.qty - self.t1_locked
    }

    /// 派生只读成本价（分/股）= (invested − recovered) / qty。
    /// qty == 0 → None。用纯整数银行家舍入（half-to-even），绝不引入 f64。
    pub fn cost_price(&self) -> Option<Money> {
        if self.qty == 0 {
            return None;
        }
        let n = self.invested_cents - self.recovered_cents;
        let c = round_half_to_even_i64(n, self.qty);
        Some(Money::from_cents(c))
    }
}

/// 纯整数银行家舍入（round-half-to-even）的 n / d，支持负分子。
/// 用 div_euclidean/rem_euclidean 保证负数对称；半值（2*rem == d）时取偶数商。
fn round_half_to_even_i64(n: i64, d: u32) -> i64 {
    let d = d as i64;
    let floor = n.div_euclid(d); // 向下取整商（负数也正确）
    let rem = n.rem_euclid(d); // 非负余数 [0, d)
    match (2 * rem).cmp(&d) {
        std::cmp::Ordering::Less => floor,
        std::cmp::Ordering::Greater => floor + 1,
        std::cmp::Ordering::Equal => {
            // 恰好半：取偶数。floor 与 floor+1 二选一，谁偶取谁。
            if floor % 2 == 0 {
                floor
            } else {
                floor + 1
            }
        }
    }
}
