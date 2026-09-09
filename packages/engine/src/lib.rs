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
    decide_data, HotParams, InstParams, Intent, MarketView, MomentumStrategy, PositionView,
    RetailParams, RetailStyle, Rng, SelfView, StockView, Strategy, StrategyData, StrategyDecision,
    StrategyError, StrategyFactory, StrategyParams, TargetPolicy, ValueStrategy, ZiNoiseStrategy,
};

pub mod account;
pub use account::{Account, AccountError, AccountKind, Position, StockCode};

pub mod behavior;
pub use behavior::{
    decide_retail_position, BehaviorMarketObservation, DecisionReason, PositionAction,
    PositionDecision,
};

pub mod market;
pub use market::{Market, MarketError, VParams};

pub mod compute;
pub use compute::{create_backend, ComputeBackend, ComputeError, ComputeMode, CpuBackend};

pub mod session;
pub use session::{
    AccountSnap, AuctionOrderSnap, DailyCandle, DailyTradeStats, Event, FloatAllocation,
    GameSession, MarketSnap, NpcAttentionState, NpcSetup, PositionSnap, RejectionReason, SaveSlot,
    SecurityCategory, SessionError, SessionSetup, Snapshot, SplitMix64, StockExchange, StockSpec,
    TradingPhase, MAX_OPEN_ORDERS, MAX_OPEN_ORDERS_PER_ACCOUNT, MAX_PENDING_PLAYER_INTENTS,
};

pub mod diagnostics;
pub use diagnostics::{
    run_price_volume_baseline, BaselineError, DistributionSummary, ExtremeSeedCase,
    PriceVolumeBaselineReport, PriceVolumeRunReport, StockEnsembleReport, StockPriceVolumeReport,
};

pub mod observation;
pub use observation::{
    build_account_risk_observation, build_equal_weight_market_observation,
    build_market_minute_closes, build_price_path_observation, completed_market_minute_count,
    AccountRiskObservation, CompletedDayClose, EqualWeightMarketObservation, HorizonReturn,
    MarketMinuteClose, MarketTickPrice, ObservationError, PositionRiskObservation,
    PricePathObservation, PriorRangeObservation, RiskPositionInput, GAME_INTRADAY_MINUTES_PER_DAY,
};
