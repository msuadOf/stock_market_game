//! 股票模拟游戏核心引擎 (engine crate)。
//!
//! 纯逻辑核心：游戏规则、市场模拟、订单簿撮合、账务。无 I/O、无副作用、无全局可变状态。
//! 状态全部可序列化 (serde)，供前端经 WASM、后端、Tauri 复用同一份实现。
//!
//! 工程铁律（见 AGENTS.md / docs/principles.md）：
//! - TDD：先写失败测试，再写实现。
//! - 防御式编程：可预期失败走 `Result`；不变量违反 panic + 上下文，绝不静默吞错。
//!
//! 详见 docs/architecture.md 与 docs/decisions/0002-engine-rust-wasm.md。

#![cfg_attr(target_arch = "wasm32", feature(stdarch_wasm_atomic_wait))]

// Native workers allocate and release tick candidates on different threads.
// The allocator choice is process-wide but does not own game state. Other
// platforms keep their supported allocator. See ADR-0020 for measurements.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[global_allocator]
static NATIVE_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

pub mod shared_history;
pub(crate) mod u64_decimal;

pub mod indicators;
pub mod money;
pub use money::{Money, MoneyError};

/// 中立费用域（B2：三分项共享概念，旧管线仅重导出）。
pub mod fees;
pub use fees::FeeComponents;

pub mod intraday_average;
pub use intraday_average::{
    calculate_intraday_average, calculate_intraday_average_curve,
    calculate_intraday_average_from_input, parse_turnover_cents, IntradayAverage,
    IntradayAverageError, IntradayAverageInput,
};

pub mod config;
pub use config::{ConfigError, GameConfig};

pub mod orderbook;
pub use orderbook::{AccountId, MatchResult, Order, OrderBook, OrderError, OrderId, Side, Trade};

pub mod strategy;
pub use strategy::{
    decide_data, BeliefInstitutionStrategy, HotParams, InstParams, Intent, LimitPrice, MarketView,
    MomentumStrategy, PositionView, RetailParams, RetailStyle, Rng, SelfView, StockView, Strategy,
    StrategyData, StrategyDecision, StrategyError, StrategyFactory, StrategyFamily, StrategyParams,
    StrategyProfile, TargetPolicy, ZiNoiseStrategy,
};

pub mod error;
pub use error::{ymd_context, ValidationFault};

pub mod stock;
pub use stock::{StockCatalog, StockCatalogError, StockId, StockMeta};

pub mod account;
pub use account::{Account, AccountError, AccountKind, Position, StockCode};

pub mod behavior;
pub use behavior::{
    decide_retail_position, decide_retail_position_with_experience, BehaviorMarketObservation,
    DecisionReason, PositionAction, PositionDecision,
};

pub mod experience;
pub use experience::{
    ExperienceError, RetailExperienceState, RetailStockExperience, MAX_UNHELD_WATCHLIST_STOCKS,
    POST_EXIT_COOLDOWN_MINUTES,
};

pub mod market;
pub use market::{Market, MarketError};

pub mod compute;
pub use compute::{
    create_backend, ComputeBackend, ComputeContext, ComputeError, ComputeInitError, ComputeMode,
    ComputeWork, CpuBackend,
};

/// 时钟门面：native=std re-export，wasm=env.engine_clock_now（设计 §6.1.3-2/§8.3）。
pub mod clock;
/// 请求槽协议核心（wasm 生产面 + 全部单元测试；设计 §12.1/§13/§14.1）。
///
/// native生产路径仅用std re-export；协议核心只在wasm或测试参与编译，
/// 避免把未使用的协议模块引入native codegen输入与CGU分区。
/// 旧阶段曾把跨CGU调用形态变化归因于本模块，后续已撤回该实测归因；
/// 本门控不声称已证明该因果关系或native热函数逐指令等价。
#[cfg(any(target_family = "wasm", test))]
mod facade_slot;
/// 线程门面：native=std re-export，wasm=槽协议接线（设计 §0/§1.1/§12-14）。
pub mod thread_facade;
#[cfg(any(target_arch = "wasm32", test))]
mod wasm_tombstone;

pub mod verification_evidence;

pub mod session;
pub use session::{
    decode_save_slot, AccountSnap, AuctionOrderSnap, BetweenKindDistribution, DailyCandle,
    DailyTradeStats, Event, FloatAllocation, GameSession, IngressReceiptCursors, MarketSnap,
    NpcAttentionState, NpcSetup, PositionSnap, ReceiptBearingIntent, RejectionReason,
    SaveDecodeLimits, SaveSlot, SavedEnvelopeKey, SavedFeeComponents, SavedJournalRank,
    SavedLiveEnvelope, SavedReceiptLocalKey, SavedReceiptSource, SavedReceiptTransition,
    SavedRetailReceiptIdentity, SavedRuntimeState, SecurityCategory, SessionError, SessionSetup,
    SharedSessionIngress, Snapshot, SplitMix64, StockExchange, StockSpec, TradingPhase,
    WithinKindDistribution, MAX_SAVE_DECODE_BYTES, SIMULATION_POLICY_ID,
};

pub mod diagnostics;
pub use diagnostics::NpcDecisionDiagnostics;
#[cfg(feature = "simulation-diagnostics")]
pub use diagnostics::{
    run_combined_diagnostics, CombinedCausalRunReport, CombinedDiagnosticsReport,
    CombinedRunSource, NpcDecisionTraceRecord, MAX_NPC_DECISION_TRACE_RECORDS,
};
pub use diagnostics::{
    run_price_volume_baseline, BaselineError, DistributionSummary, ExtremeSeedCase,
    ParticipantExecutionRunReport, PriceVolumeBaselineReport, PriceVolumeRunReport,
    StockEnsembleReport, StockPriceVolumeReport,
};

pub mod observation;
pub use observation::{
    build_account_risk_observation, build_equal_weight_market_observation,
    build_market_minute_closes, build_price_path_observation, completed_market_minute_count,
    AccountRiskObservation, CompletedDayClose, EqualWeightMarketObservation, HorizonReturn,
    MarketMinuteClose, MarketTickPrice, ObservationError, PositionRiskObservation,
    PricePathObservation, PriorRangeObservation, RiskPositionInput, GAME_INTRADAY_MINUTES_PER_DAY,
};

pub mod plans;
pub use plans::{
    OpinionSource, PauseReason, PlanBook, PlanEntry, PlanError, PlanEvent, PlanId, PlanOpen,
    PlanOpinion, PlanOrigin, PlanPolicy, PlanRevision, PlanStatus, PlanTarget, ResumeReason,
    ReviewConditions, RevisionReason, RevisionRecord, TerminationReason, TradingPlan, Urgency,
};

pub mod calendar;
pub use calendar::{
    CivilDate, CivilDateError, CivilInstant, DayStatus, TradingCalendar, TradingDayOrdinal,
    Weekday, YearCoverageLabel,
};

pub mod accounting;
pub use accounting::{
    AccountChart, AccountingAmount, AccountingError, AccountingPeriod, Books, BusinessEventId,
    CashFlowClass, Journal, JournalEntry, Ledger, LedgerAccountId, PeriodStatus,
};

pub mod company;

pub mod information;

/// std-thread 所有权转移模型运行时原语（身份/实体/事实/批次协议类型，批 1 类型先行）。
pub mod runtime;

pub use runtime::mirror_origin::{RuntimeMirrorDayEndStage, RuntimeMirrorOrigin};
pub use runtime::sync_policy::RuntimeSyncPolicy;
