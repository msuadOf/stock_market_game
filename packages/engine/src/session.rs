//! 编排层（ADR-0005 §5 GameSession）：把 money/config/orderbook/account/market/strategy
//! 串成每 tick 完整循环，产出快照 + 带序号增量事件流。种子化确定性 RNG。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-session-design.md。
//! 纯逻辑、无 I/O、无全局可变状态（联机预留：实例即隔离）。

use crate::account::{Account, AccountError, AccountKind, StockCode};
use crate::config::GameConfig;
use crate::market::{Market, MarketError, VParams};
use crate::money::{Money, MoneyError};
use crate::orderbook::{AccountId, Order, OrderId, Side};
use crate::strategy::{Intent, MarketView, PositionView, SelfView, StockView, StrategyParams};
use crate::strategy::Rng;
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

/// SplitMix64：确定性 PRNG。种子化、可重放（同种子同序列）。
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }
    /// 标准 SplitMix64 算法（常量固定，确定性依赖）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

impl Rng for SplitMix64 {
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() as u32 % (hi - lo))
    }
}

/// 意图被拒原因（预校验/涨跌停/未知股票）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum RejectionReason {
    /// 买入资金不足（成交额+佣金 > 现金）。
    InsufficientCash,
    /// 卖出超卖（qty > 可卖持仓）。
    InsufficientShares,
    /// 下单价超涨跌停范围。
    LimitExceeded,
    /// 意图指向不存在的股票代码。
    UnknownStock,
}

/// 增量事件（带单调 seq）。非错误类型：运行期失败（意图被拒/结算失败/V 失败）
/// 进事件流供前端呈现，不中断 tick 循环（铁律二：显式可见，不静默丢弃）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// 成交：code 来自路由（orderbook.Trade 无 code 字段），maker/taker 双方结算。
    Trade {
        seq: u64,
        code: StockCode,
        price: Money,
        qty: u32,
        maker: AccountId,
        taker: AccountId,
    },
    /// 价格 tick：每 tick 末记录最新价。
    PriceTick { seq: u64, code: StockCode, last_price: Money },
    /// 日界：到 ticks_per_day 触发，day 自增。
    DayBoundary { seq: u64, day: u32 },
    /// 意图被拒（资金/持仓/涨跌停/未知股票）。
    IntentRejected {
        seq: u64,
        account: AccountId,
        code: StockCode,
        reason: RejectionReason,
    },
    /// 结算失败（账户侧异常，透传 AccountError 文案）。
    SettlementError { seq: u64, account: AccountId, code: StockCode, reason: String },
    /// V 演化失败（market.evolve_v 异常）。
    VError { seq: u64, code: StockCode, reason: String },
}

/// 市场快照子结构（单股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketSnap {
    pub last_price: Money,
    pub last_close: Money,
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub fundamental_value: Money,
}

/// 持仓快照子结构（单只股票）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PositionSnap {
    pub qty: u32,
    pub t1_locked: u32,
    pub invested_cents: i64,
    pub recovered_cents: i64,
}

/// 账户快照子结构。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AccountSnap {
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionSnap>,
}

/// 完整状态快照（首次连/重连/存档）。含 V（前端展示层按需过滤）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub seq: u64,
    pub tick: u64,
    pub day: u32,
    pub markets: BTreeMap<StockCode, MarketSnap>,
    pub accounts: BTreeMap<AccountId, AccountSnap>,
}

/// session 操作失败（致命：构造非法 / 未知玩家）。绝不静默吞错（铁律二）。
#[derive(Debug, Error)]
pub enum SessionError {
    /// 构造参数非法（stocks 空 / ticks_per_day==0 等）。
    #[error("invalid setup: {0}")]
    InvalidSetup(String),
    /// 入队意图时玩家 id 不存在。
    #[error("unknown player: {0:?}")]
    UnknownPlayer(AccountId),
    /// 透传 market 错误。
    #[error(transparent)]
    Market(#[from] MarketError),
    /// 透传 account 错误。
    #[error(transparent)]
    Account(#[from] AccountError),
    /// 透传 money 错误。
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// 单只股票初始规格（行情/涨跌停/V/tick）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockSpec {
    pub code: StockCode,
    pub initial_price: Money,
    pub limit_pct: f64,
    pub v_initial: Money,
    pub tick: Money,
}

/// NPC 群体配置（三类计数 + 单户初始现金）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NpcSetup {
    pub retail_count: u32,
    pub inst_count: u32,
    pub hot_count: u32,
    pub cash_per_npc: Money,
}

/// session 初始化参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionSetup {
    pub stocks: Vec<StockSpec>,
    pub npcs: NpcSetup,
    pub config: GameConfig,
    pub v_params: VParams,
    pub strategy_params: StrategyParams,
    pub player_cash: Money,
    pub ticks_per_day: u64,
    pub history_len: usize,
    pub t1_enabled: bool,
}
