//! 股票模拟游戏核心引擎 (engine crate)。
//!
//! 纯逻辑核心：游戏规则、市场模拟、订单簿撮合、账务。无 I/O、无副作用、无全局可变状态。
//! 状态全部可序列化 (serde)，供前端经 WASM、后端、Tauri 复用同一份实现。
//!
//! 工程铁律（见 CLAUDE.md / docs/principles.md）：
//! - TDD：先写失败测试，再写实现。
//! - 防御式编程：可预期失败走 `Result`；不变量违反 panic + 上下文，绝不静默吞错。
//!
//! 详见 docs/architecture.md 与 docs/decisions/0002-engine-rust-wasm.md。

pub mod money;
pub use money::{Money, MoneyError};

pub mod config;
pub use config::{ConfigError, GameConfig};

pub mod orderbook;
pub use orderbook::{AccountId, MatchResult, Order, OrderBook, OrderError, OrderId, Side, Trade};

pub mod strategy;
pub use strategy::{
    HotParams, InstParams, Intent, MarketView, MomentumStrategy, PositionView, RetailParams, Rng,
    SelfView, StockView, Strategy, StrategyError, StrategyFactory, StrategyParams, TargetPolicy,
    ValueStrategy, ZiNoiseStrategy,
};

pub mod account;
pub use account::{Account, AccountError, AccountKind, Position, StockCode};

pub mod market;
pub use market::{Market, MarketError, VParams};

pub mod session;
pub use session::{
    AccountSnap, Event, FloatAllocation, GameSession, MarketSnap, NpcSetup, PositionSnap,
    RejectionReason, SessionError, SessionSetup, Snapshot, SplitMix64, StockSpec,
};
