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

// Native workers allocate and release tick candidates on different threads.
// The allocator choice is process-wide but does not own game state. Other
// platforms keep their supported allocator. See ADR-0020 for measurements.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[global_allocator]
static NATIVE_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(test)]
#[macro_use]
#[path = "../test-support/simple_company.rs"]
mod simple_company_test_fixture;

pub mod indicators;
pub mod money;
pub use money::{Money, MoneyError};

pub mod intraday_average;
pub use intraday_average::{calculate_intraday_average, calculate_intraday_average_curve, parse_turnover_cents, IntradayAverage, IntradayAverageError, IntradayAverageInput};

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
pub use compute::{create_backend, ComputeBackend, ComputeError, ComputeMode, CpuBackend};

pub mod verification_evidence;

pub mod session;
pub use session::{
    CompanyReportCorrection, CompletedReportCorrection, ReportCorrectionEpoch, ReportCorrectionStatus,
    PersonalTradeConfirmation, SharedSessionIngress,
    decode_save_slot, AccountSnap, AuctionOrderSnap, BetweenKindDistribution, DailyCandle,
    DailyTradeStats, Event, FloatAllocation, GameSession, IngressReceiptCursors, MarketSnap,
    NpcAttentionState, NpcSetup, ParentOrderPlan, PendingNpcBatch, PendingPlanEvent, PositionSnap,
    ReceiptBearingIntent, RejectionReason, SaveDecodeLimits, SaveParentOrderPlan, SaveSlot,
    SavedEnvelopeKey, SavedFeeComponents, SavedJournalRank, SavedLiveEnvelope,
    SavedReceiptLocalKey, SavedReceiptSource, SavedReceiptTransition, SavedRetailReceiptIdentity,
    SavedRuntimeState, SecurityCategory, SessionError, SessionSetup, Snapshot, SplitMix64,
    StockExchange, StockSpec, TradingPhase, WithinKindDistribution, MAX_SAVE_DECODE_BYTES,
    SIMULATION_POLICY_ID,
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
    OpinionSource, PauseReason, PlanBook, PlanError, PlanEvent, PlanId, PlanOpen, PlanOpinion,
    PlanPolicy, PlanRevision, PlanStatus, PlanTarget, ResumeReason, ReviewConditions,
    RevisionReason, RevisionRecord, TerminationReason, TradingPlan, Urgency,
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
