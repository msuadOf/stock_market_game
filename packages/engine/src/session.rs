//! 编排层（ADR-0005 §5 GameSession）：把 money/config/orderbook/account/market/strategy
//! 串成每 tick 完整循环，产出快照 + 带序号增量事件流。种子化确定性 RNG。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-session-design.md。
//! 纯逻辑、无 I/O、无全局可变状态（联机预留：实例即隔离）。

mod account_book;
mod account_paged_map;
mod attention;
mod candles;
#[cfg(feature = "simulation-diagnostics")]
mod causal;
mod civil_clock;
mod company_assembly;
#[cfg(test)]
mod company_closing_tests;
mod company_groups;
mod continuous_cancellation;
mod decision_chain;
mod deferred_drop;
use crate::u64_decimal;
pub use deferred_drop::defer as defer_destruction;
use deferred_drop::defer_swap;
#[doc(hidden)]
pub use deferred_drop::host_deferred_count as deferred_destruction_count;
#[doc(hidden)]
pub use deferred_drop::host_deferred_degradation as deferred_destruction_degradation;
pub use deferred_drop::DeferredDropDegradation;
mod disclosures;
mod envelope_projection;
mod execution;
mod failure;
mod hash;
mod history_reads;
mod shared_ingress;
pub use shared_ingress::SharedSessionIngress;
#[cfg(feature = "verification-harness")]
mod verify_metrics;
#[cfg(feature = "verification-harness")]
pub use deferred_drop::{drain as verification_drain, DrainFailure};
#[cfg(feature = "verification-harness")]
pub use verify_metrics::records as verification_job_records;
#[cfg(feature = "verification-harness")]
pub use verify_metrics::snapshot as verification_metrics_snapshot;
#[cfg(feature = "verification-harness")]
pub use verify_metrics::{JobKind, JobRecord, TraceInvalid, VerifySnapshot};

/// 仅诊断构建记录 actor producer 的完整 lifetime；普通构建为零大小空操作。
/// P2②：私有字段防止绕过工厂构造（无登记构造 + Drop 递减会破坏 drain 前提：
/// 1→0 隐藏活 producer，0→MAX 静默回绕）。只有 [`verification_actor_guard`] 能构造。
#[doc(hidden)]
pub struct VerificationActorGuard(());
#[doc(hidden)]
#[inline(always)]
pub fn verification_actor_guard() -> VerificationActorGuard {
    #[cfg(feature = "verification-harness")]
    verify_metrics::METRICS
        .actor_producers
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    VerificationActorGuard(())
}
impl Drop for VerificationActorGuard {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(feature = "verification-harness")]
        verify_metrics::METRICS.producer_exit();
    }
}
mod initial_allocation;
mod institutional_behavior;
mod minimal_snapshot;
mod observation_clock;
mod persistence;
pub mod pipeline;
mod plan_chain_candidates;
mod plan_continuation_domain;
mod plan_execution;
mod player_candidates;
pub mod protocol;
mod self_views;
mod snapshot;
mod views;
pub use crate::stock::StockId;
pub use failure::StepFatal;
pub use hash::StateHash;
pub use history_reads::HistoricalStockData;
pub use initial_allocation::{
    InitialAllocation, InitialAllocationCategory, InitialAllocationKind, InitialStockAllocation,
};
#[cfg(test)]
mod continuous_cancellation_tests;
#[cfg(test)]
mod envelope_projection_hydration_tests;
#[cfg(test)]
mod envelope_projection_tests;
#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod hash_contract_tests;
#[cfg(test)]
mod plan_chain_candidates_tests;
#[cfg(test)]
mod plan_root_quota_tests;
#[cfg(test)]
mod plan_root_quota_wave_tests;
#[cfg(test)]
mod player_candidates_tests;
#[cfg(test)]
mod reconciliation_plan_phase_tests;
#[cfg(test)]
mod reconciliation_plan_tests;

use account_paged_map::AccountPagedMap;
use attention::NpcAttentionScheduler;
use candles::{
    generate_preset_daily_candles, stock_code_hash, DailyCandleHistory, SessionCandleBook,
};
use civil_clock::{default_civil_start_date, session_calendar_exchange};
use decision_chain::personal_state::BeliefParticipantState;
use persistence::{validate_save_slot, validate_saved_order_state};

pub use attention::NpcAttentionState;
pub(crate) mod notices;
pub use civil_clock::{CivilClock, CivilClockError, CivilClockSave, CivilDayEndReport, CivilPhase};
pub use company_groups::{GroupHolding, GroupStructure};
pub use decision_chain::{BeliefDebugSummary, DecisionChainDiagnostics};
// D3a-2c 件2/3：组线程组装/直调的承载类型（AccountBook 投影 + 时间映射）。
pub(crate) use account_book::AccountBook;
pub(crate) use observation_clock::intraday_observation_second;
// D3a-2c 件2/3：组线程直调 observe 的类型面（决策链深移植——方向 A；
// crate 内可见——runtime 组线程组装/直调）。
pub(crate) use decision_chain::{
    DecisionChainObservation, InstitutionDecisionRoot, PlanLifecycleAction, PlanPersonalState,
    PlanQuoteInputs, PlanRootDiagnostics, RootReadContext, buy_allocation_request,
    decide_plan_quote, exposed_stocks_shared, next_routable_buy_qty, next_routable_sell_qty,
    open_position_action,
};
// D3b 件 4：组线程复审宿主的共享类型面（trait+行为结构——两宿主实现
// 同一 trait，行为不得分叉）。
pub(crate) use institutional_behavior::InstitutionBehavior;
pub(crate) use institutional_behavior::assess_institution_behavior;
// 转发 decision_chain 的 re-export（组线程经 crate::session 使用）。
pub(crate) use decision_chain::{PlanLifecycleReview, PlanReviewHost, ratio_to_bp};
// D5 #2：组线程 attention evaluate 共用面（signal 概率+observe 门+wait
/// 采样——旧管线 evaluate_candidate_with_signal 同式，字段不得分叉）。
pub(crate) use attention::{
    attention_candidate_is_observation, market_attention_signal,
    maximum_observation_probability, sample_attention_wait,
};
pub use disclosures::{
    disclosure_phase_observer, DayEndDisclosureCtx, DayEndDisclosures, DisclosureDispatch,
    DisclosureError,
};
pub use minimal_snapshot::{SaveAccountSnap, SaveMarketSnap, SaveSnapshot};
pub use notices::NpcInformationCadence;
pub use persistence::{
    decode_save_slot, SaveDecodeLimits, SavedEnvelopeKey, SavedFeeComponents, SavedJournalRank,
    SavedLiveEnvelope, SavedReceiptLocalKey, SavedReceiptSource, SavedReceiptTransition,
    SavedRetailReceiptIdentity, SavedRuntimeState, MAX_SAVE_DECODE_BYTES, SIMULATION_POLICY_ID,
};
pub use plan_execution::{
    PlanExecutionDisposition, PlanExecutionError, PlanExecutionReport, PlanExecutionRequest,
};
pub use snapshot::{AccountSnap, MarketSnap, PositionSnap, Snapshot};

use crate::account::{Account, AccountError, AccountKind, StockCode};
#[cfg(test)]
use crate::behavior::BehaviorMarketObservation;
use crate::behavior::PositionDecision;
use crate::calendar::{CivilDate, CivilInstant, DayStatus, TradingCalendar};
use crate::company::CompanyId;
pub(crate) use crate::config::{buy_order_reservation, fee_delta};
use crate::config::{ConfigError, GameConfig};
use crate::error::ValidationFault;
use crate::experience::{ExperienceError, RetailExperienceState};
use crate::information::PublicationId;
use crate::market::{Market, MarketError};
use crate::money::{Money, MoneyError};
#[cfg(test)]
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, AccountRiskObservation,
    RiskPositionInput,
};
use crate::observation::{
    build_price_path_observation, completed_market_minute_count, CompletedDayClose,
    MarketMinuteClose, ObservationError, PricePathObservation, GAME_INTRADAY_MINUTES_PER_DAY,
};
use crate::orderbook::{AccountId, Order, OrderError, OrderId, Side};
use crate::stock::StockCatalog;
use crate::strategy::Rng;
use crate::strategy::{
    Intent, LimitPrice, MarketView, StockView, StrategyError, StrategyFactory, StrategyParams,
    StrategyProfile,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

/// 机构母单在没有新目标修订时，最多跨越一个标准交易日。
/// 日终所有剩余子单和计划都会失效，绝不跨日沿用旧观点。

/// SplitMix64：确定性 PRNG。种子化、可重放（同种子同序列）。
#[derive(Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }
    /// 流状态读面（D5 #2 组内 evaluate 续传写回——与 session rng_state
    /// 同源投影，字段不得分叉）。
    pub fn state(&self) -> u64 {
        self.state
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
pub enum RejectionReason {
    /// 买入资金不足（成交额+佣金 > 现金）。
    InsufficientCash,
    /// 卖出超卖（qty > 可卖持仓）。
    InsufficientShares,
    /// 下单价超涨跌停范围。
    LimitExceeded,
    /// 连续竞价限价申报超出“基准价 102%/98% 或上下十个最小价位孰宽”的价格笼子。
    PriceCageExceeded,
    /// 意图指向不存在的股票代码。
    UnknownStock,
    /// 集合竞价只接受限价委托，市价单无法确定保护价格。
    AuctionLimitOrderRequired,
    /// 当前处于集合竞价不可撤单时段（A 股 09:20 后）。
    AuctionOrderNotCancelable,
    /// 09:25–09:30 为开盘结果处理时段，不接受新申报。
    AuctionOrderEntryClosed,
    /// 委托数量不符合 A 股竞价交易单位或单笔上限。
    InvalidQuantity,
    /// 连续竞价撤单所指订单不存在。
    OrderNotFound,
    /// 委托已经全部成交，簿上没有可撤销的剩余数量。
    OrderAlreadyFilled,
    /// 只能撤销属于自己的委托。
    NotOrderOwner,
}

/// 当前交易阶段。`ticks_per_day` 包含集合竞价与连续竞价。
#[derive(
    Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS,
)]
pub enum TradingPhase {
    CallAuction,
    /// 开盘集合竞价已于 09:25 结束，09:25–09:30 不接受申报也不连续撮合。
    PreOpen,
    /// 14:57–15:00 收盘集合竞价：接受限价申报、交易主机不接受撤单，并在日终一次撮合。
    ClosingAuction,
    #[default]
    Continuous,
}

/// 增量事件（带单调 seq）。非错误类型：运行期失败（意图被拒/结算失败/V 失败）
/// 进事件流供前端呈现，不中断 tick 循环（铁律二：显式可见，不静默丢弃）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum Event {
    /// 成交：code 来自路由（orderbook.Trade 无 code 字段）——maker=卖腿/
    /// taker=买腿（ADR-0042 腿方向语义，2026-10-16）。
    Trade {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        code: StockId,
        price: Money,
        qty: u32,
        maker: AccountId,
        taker: AccountId,
    },
    /// 集合竞价每 tick 的虚拟撮合结果；没有交叉时价格为 None、量为 0。
    AuctionTick {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        tick: u64,
        /// `CallAuction`（开盘）或 `ClosingAuction`（收盘）；消费者不得按事件名猜测时段。
        phase: TradingPhase,
        code: StockId,
        indicative_price: Option<Money>,
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        matched_volume: u64,
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        imbalance: u64,
    },
    /// 集合竞价结束并一次性按唯一清算价撮合。
    AuctionCompleted {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        tick: u64,
        /// 完成这一笔集合竞价的交易阶段。
        phase: TradingPhase,
        code: StockId,
        clearing_price: Option<Money>,
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        matched_volume: u64,
    },
    /// 价格 tick：每 tick 末记录最新价。
    PriceTick {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        /// 权威游戏世界 tick；宿主压缩事件后仍可恢复交易分钟位置。
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        tick: u64,
        code: StockId,
        last_price: Money,
        /// Rust 聚合的权威当日日 K；宿主只同步，不再自行重算 OHLCV。
        daily_candle: DailyCandle,
        /// 当前买盘前五档（价高到低），数量仍以股为引擎单位。
        #[serde(with = "js_safe_depth")]
        #[ts(type = "Array<[Money, number]>")]
        bids: Vec<(Money, u64)>,
        /// 当前卖盘前五档（价低到高）。
        #[serde(with = "js_safe_depth")]
        #[ts(type = "Array<[Money, number]>")]
        asks: Vec<(Money, u64)>,
    },
    /// 日界：到 ticks_per_day 触发，day 自增。
    DayBoundary {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        day: u32,
        /// 刚刚收盘的每股日 K，供增量客户端提交历史而无需拉取 360 日全量快照。
        closed_daily_candles: BTreeMap<StockId, DailyCandle>,
    },
    /// 权威自然日已完成日结并前进；休市日同样产生，不能由交易事件替代。
    CivilDateAdvanced {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        settled_date: CivilDate,
        next_date: CivilDate,
        next_status: DayStatus,
    },
    /// 已成功写入不可变公开信息库的一条公司披露；不含总账或 NPC 私有信息。
    CompanyDisclosurePublished {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        publication_id: PublicationId,
        company: CompanyId,
        published_at: CivilInstant,
        kind: CompanyDisclosureKind,
    },
    /// 意图被拒（资金/持仓/涨跌停/未知股票）。
    IntentRejected {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        account: AccountId,
        code: StockId,
        reason: RejectionReason,
    },
    /// 结算失败（账户侧异常，透传 AccountError 文案）。
    SettlementError {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        account: AccountId,
        code: StockId,
        reason: String,
    },

    /// 连续竞价委托已撤销，冻结资金或股份随即释放。
    OrderCanceled {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        account: AccountId,
        code: StockId,
        id: OrderId,
        remaining_qty: u32,
    },
    /// 限价委托有未成交余量进入连续竞价订单簿。
    OrderAccepted {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        account: AccountId,
        code: StockId,
        id: OrderId,
        side: Side,
        price: Money,
        remaining_qty: u32,
    },
}

impl Event {
    /// 返回所有事件共有的单调序号，避免跨宿主重复维护易漏分支的匹配逻辑。
    pub fn seq(&self) -> u64 {
        match self {
            Self::Trade { seq, .. }
            | Self::AuctionTick { seq, .. }
            | Self::AuctionCompleted { seq, .. }
            | Self::PriceTick { seq, .. }
            | Self::DayBoundary { seq, .. }
            | Self::CivilDateAdvanced { seq, .. }
            | Self::CompanyDisclosurePublished { seq, .. }
            | Self::IntentRejected { seq, .. }
            | Self::SettlementError { seq, .. }
            | Self::OrderCanceled { seq, .. }
            | Self::OrderAccepted { seq, .. } => *seq,
        }
    }
}

/// 不可变公开信息库中一条披露的公开类别与报告版本标识。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum CompanyDisclosureKind {
    Report { report_revision: u32 },
    Announcement,
}

/// 单个交易日的 OHLCV。价格全部为分，time 是该公历日 UTC 零点的 Unix 秒日期标签，
/// 不是 Asia/Shanghai 的真实开盘时刻。虚拟前史和真实撮合日使用相同日期口径。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct DailyCandle {
    pub time: i64,
    pub open: Money,
    pub high: Money,
    pub low: Money,
    pub close: Money,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub volume: u64,
    /// 真实撮合产生的当日累计成交额与成交笔数。预置的合成历史没有逐笔来源，
    /// 因而显式为 `None`，不能伪装成可对账的真实统计。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_stats: Option<DailyTradeStats>,
}

/// 单个交易日由逐笔成交严格累计的统计。成交额以分为单位，并以十进制字符串
/// 跨 JSON 边界，避免超过 JavaScript 安全整数后丢失精度。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS)]
#[ts(export)]
pub struct DailyTradeStats {
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub turnover_cents: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub trade_count: u64,
}

/// 存档槽：保存权威市场、账户、集合竞价及连续竞价未成交委托。
/// 前端分时采样属于派生 UI 数据，不进入权威存档；日 K 由 engine 持久化。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SaveSlot {
    /// Escrow 并行 tick 的权威运行时状态。TypeScript 形状由 Web 严格存档
    /// parser 共同维护，避免把策略私有结构扩成通用宿主命令。
    #[ts(type = "import(\"../../save/schema/runtime-state\").SavedRuntimeState")]
    pub runtime_state: SavedRuntimeState,
    pub setup: SessionSetup,
    #[ts(type = "import(\"../../save/schema/company/groups\").GroupStructure[]")]
    pub groups: Vec<GroupStructure>,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub seed: u64,
    pub snapshot: SaveSnapshot,
    /// 日内存档恢复集合竞价所需的完整委托队列。
    /// 连续竞价未成交委托。
    pub resting_orders: BTreeMap<StockId, Vec<Order>>,
    /// 每股下一时间序。撤单和日界清簿不重置，不能从现存挂单推算。
    /// 十进制字符串保留完整 u64 游标，不受 JSON number 精度限制。
    #[serde(with = "book_sequence_map")]
    #[ts(type = "Record<string, string>")]
    pub book_next_sequences: BTreeMap<StockId, u64>,
    /// 已全部成交的委托身份。撤旧单时据此区分已成交与未知/已撤，跨 tick 保留。
    pub filled_orders: BTreeMap<StockId, Vec<FilledOrderSnap>>,
    /// 策略观察所需的短价格窗口。它会影响下一 tick 的决策，因此属于权威状态。
    pub price_history: BTreeMap<StockId, Vec<Money>>,
    /// 当前交易日的标准交易分钟收盘快照。它会影响后续策略，因此属于权威状态；
    /// 到日界后清空，跨日窗口读取已经完成的日 K。
    pub market_minute_closes: BTreeMap<StockId, Vec<MarketMinuteClose>>,
    /// 当前随机数生成器状态；用十进制字符串避免 JavaScript 丢失 u64 精度。
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub rng_state: u64,
    /// 每个 NPC 的权威注意力调度状态。独立随机流保证观察节奏可存档、可重放。
    pub npc_attention: BTreeMap<AccountId, NpcAttentionState>,
    /// 每个自然人散户由真实成交与观察形成的权威经历；机构、游资和玩家不得出现在此表。
    pub retail_experience: BTreeMap<AccountId, RetailExperienceState>,
    /// NPC 连续竞价普通限价单的可恢复主动撤单时间。
    pub npc_order_lifecycles: Vec<NpcOrderLifecycle>,
    /// 已被宿主确认入队、尚未在下一 tick 路由的玩家意图。
    pub pending_player: Vec<ReceiptBearingIntent>,
    /// 上一已提交版本生成、等待下一市场 tick 受理的 NPC 请求；日界处尚未生成。
    pub ingress_receipt_cursors: IngressReceiptCursors,
    /// 保持订单 id/到达序继续单调递增。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub next_order_id: u64,
    /// 自然日经营时钟权威状态；完整存档携带冻结的日历政策。
    pub civil_clock: CivilClockSave,
    /// ── 权威状态连续性（完整存档）：公司域与个体决策链权威状态。全部必填；缺失任一字段
    ///    的 JSON 不是当前 schema 的合法存档，走通用校验拒绝。──
    /// 经营编排（调度器/活跃冲击/各经营 RNG/账套——serde 全量持久化，分录与
    /// 余额在反序列化重放边界校验）。
    #[ts(type = "import(\"../../save/schema/company/operations\").CompanyOperations")]
    pub company_operations: crate::company::operations::CompanyOperations,
    /// 结账版本登记簿（不可变期间版本 + 重述底稿）。
    #[ts(skip)]
    pub closing_registry: crate::accounting::closing::ClosingEngine,
    /// 公开信息库（报告 + 公告；恢复走 from_parts 逐条重验）。
    #[ts(skip)]
    pub public_library: crate::information::PublicLibrary,
    /// 披露派发游标（published_through / announced_through）。
    #[ts(skip)]
    pub disclosures: DisclosureDispatch,
    /// 跨日个人交易计划簿（(账户,股票) 索引恢复时重建并校验）。
    pub plans: crate::plans::PlanBook,
    pub urgency_policy: crate::plans::UrgencyPolicy,
    /// 信念机构账户的个人信息集（只存公布 id 引用与获知时点）。
    #[ts(skip)]
    pub information_states: BTreeMap<AccountId, crate::information::NpcInformationState>,
    /// 信念机构账户的信念簿（含一次性抽定的个人假设）。
    #[ts(skip)]
    pub belief_books: BTreeMap<AccountId, crate::strategy::BeliefBook>,
    /// 信念机构账户的个人关注列表。
    pub watchlists: BTreeMap<AccountId, crate::experience::PersonalWatchlist>,
    pub price_memories: BTreeMap<AccountId, crate::experience::PersonalPriceMemory>,
    pub history_reads: BTreeMap<AccountId, crate::experience::PersonalHistoryReadLedger>,
}

impl SaveSlot {
    /// 边界便捷：按代码查 id（目录由 setup 构造）。未知代码返回 None。
    pub fn stock_id(&self, code: &str) -> Option<StockId> {
        self.setup
            .stock_catalog()
            .ok()
            .and_then(|catalog| catalog.id(code))
    }
}

/// 连续竞价中 NPC 主动挂出的普通限价单的可恢复生命周期。
///
/// 这不是交易所强制的报单有效期：A 股连续竞价限价单仍由既有订单簿和日终规则处理。
/// 它只记录模拟参与者在观察前主动撤回陈旧观点的时间；玩家委托、集合竞价委托和母单
/// 在途子单不进入此表。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct NpcOrderLifecycle {
    pub account: AccountId,
    pub code: StockId,
    pub order_id: OrderId,
    /// 委托进入连续竞价簿时的绝对标准交易分钟。
    pub placed_market_minute: u64,
    /// 到达该分钟后，由 NPC 经正常撤单生命周期撤回。
    pub expires_market_minute: u64,
}

mod canonical_u64_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let bytes = value.as_bytes();
        let nonzero_prefix = bytes
            .first()
            .is_some_and(|byte| (b'1'..=b'9').contains(byte));
        let canonical = bytes == b"0" || (nonzero_prefix && bytes.iter().all(u8::is_ascii_digit));
        if !canonical {
            return Err(serde::de::Error::custom(
                "u64 must use canonical decimal notation",
            ));
        }
        value.parse::<u64>().map_err(serde::de::Error::custom)
    }
}

mod book_sequence_map {
    use super::{u64_decimal, StockId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Cursor(#[serde(with = "u64_decimal")] u64);

    pub fn serialize<S>(value: &BTreeMap<StockId, u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(|(code, cursor)| (code, Cursor(*cursor)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<StockId, u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(BTreeMap::<StockId, Cursor>::deserialize(deserializer)?
            .into_iter()
            .map(|(code, cursor)| (code, cursor.0))
            .collect())
    }
}

mod account_ordinal_map {
    use super::{canonical_u64_decimal, AccountId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Cursor(#[serde(with = "canonical_u64_decimal")] u64);

    pub fn serialize<S>(value: &BTreeMap<AccountId, u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(|(id, cursor)| (id, Cursor(*cursor)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<AccountId, u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(BTreeMap::<AccountId, Cursor>::deserialize(deserializer)?
            .into_iter()
            .map(|(id, cursor)| (id, cursor.0))
            .collect())
    }
}

mod stock_ordinal_map {
    use super::{canonical_u64_decimal, StockId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Cursor(#[serde(with = "canonical_u64_decimal")] u64);

    pub fn serialize<S>(value: &BTreeMap<StockId, u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(|(code, cursor)| (code, Cursor(*cursor)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<StockId, u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(BTreeMap::<StockId, Cursor>::deserialize(deserializer)?
            .into_iter()
            .map(|(code, cursor)| (code, cursor.0))
            .collect())
    }
}

mod js_safe_depth {
    use crate::money::Money;
    use crate::orderbook::js_safe_u64;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &[(Money, u64)], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some((_, quantity)) = value
            .iter()
            .find(|(_, quantity)| *quantity > js_safe_u64::MAX)
        {
            return Err(serde::ser::Error::custom(format!(
                "depth quantity {quantity} exceeds JavaScript's safe integer range"
            )));
        }
        value.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<(Money, u64)>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Vec::<(Money, u64)>::deserialize(deserializer)?;
        if let Some((_, quantity)) = value
            .iter()
            .find(|(_, quantity)| *quantity > js_safe_u64::MAX)
        {
            return Err(serde::de::Error::custom(format!(
                "depth quantity {quantity} exceeds JavaScript's safe integer range"
            )));
        }
        Ok(value)
    }
}

/// 可序列化的集合竞价限价委托。order_id 只标识订单；
/// 该股票队列中的位置记录集合竞价的接受先后。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct AuctionOrderSnap {
    pub owner: AccountId,
    pub side: Side,
    pub limit: Money,
    pub qty: u32,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub order_id: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct FilledOrderSnap {
    pub id: OrderId,
    pub owner: AccountId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct ReceiptBearingIntent {
    pub owner: AccountId,
    pub intent: Intent,
    #[serde(with = "canonical_u64_decimal")]
    #[ts(type = "string")]
    pub account_ordinal: u64,
    #[serde(with = "canonical_u64_decimal")]
    #[ts(type = "string")]
    pub stock_ordinal: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct IngressReceiptCursors {
    #[serde(with = "account_ordinal_map")]
    #[ts(type = "Record<string, string>")]
    pub next_account_ordinal: BTreeMap<AccountId, u64>,
    #[serde(with = "stock_ordinal_map")]
    #[ts(type = "Record<string, string>")]
    pub next_stock_ordinal: BTreeMap<StockId, u64>,
}

impl IngressReceiptCursors {
    pub(super) fn receive(
        &mut self,
        owner: AccountId,
        intent: Intent,
    ) -> Result<ReceiptBearingIntent, SessionError> {
        let code = match &intent {
            Intent::PlaceLimit { code, .. }
            | Intent::PlaceMarket { code, .. }
            | Intent::Cancel { code, .. } => code.clone(),
        };
        let account_ordinal = self.next_account_ordinal.get(&owner).copied().unwrap_or(0);
        let account_next = account_ordinal.checked_add(1).ok_or_else(|| {
            // fault1003: 账户受理回执序号 u64 溢出。ctx[0] = AccountId。
            SessionError::ResourceLimit(ValidationFault::new(1003, vec![owner.0]))
        })?;
        let stock_ordinal = self.next_stock_ordinal.get(&code).copied().unwrap_or(0);
        let stock_next = stock_ordinal.checked_add(1).ok_or_else(|| {
            // fault1004: 股票受理回执序号 u64 溢出。ctx[0] = StockId。
            SessionError::ResourceLimit(ValidationFault::new(1004, vec![u64::from(code.0)]))
        })?;
        self.next_account_ordinal.insert(owner, account_next);
        self.next_stock_ordinal.insert(code, stock_next);
        Ok(ReceiptBearingIntent {
            owner,
            intent,
            account_ordinal,
            stock_ordinal,
        })
    }
}

type ContinuousOrdersByAccount = BTreeMap<AccountId, Vec<(StockId, Order)>>;
type AuctionOrdersByAccount = BTreeMap<AccountId, Vec<(StockId, AuctionOrderSnap)>>;

enum ReconcileScope {
    AllWorkingOrders,
    ReviewedStocks(BTreeSet<StockId>),
}

#[derive(Clone, Copy)]
struct WorkingOrderSlices<'a> {
    continuous: &'a [(StockId, Order)],
    auction: &'a [(StockId, AuctionOrderSnap)],
}

/// session 操作失败（致命：构造非法 / 未知玩家）。绝不静默吞错（铁律二）。
#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    ComputeInitialization(#[from] crate::compute::ComputeInitError),
    #[error(transparent)]
    Step(#[from] StepFatal),
    /// 构造参数非法（stocks 空 / ticks_per_day==0 等）。数值码+数值上下文（ADR 0041）。
    #[error("invalid setup: {0}")]
    InvalidSetup(crate::error::ValidationFault),
    /// 入队意图时玩家 id 不存在。
    #[error("unknown player: {0:?}")]
    UnknownPlayer(AccountId),
    /// 入队意图时账户存在，但它是引擎控制的 NPC，不接受玩家指令。
    #[error("account is not controlled by a player: {0:?}")]
    NotPlayer(AccountId),
    #[error("unknown account for history request: {0:?}")]
    UnknownHistoryAccount(AccountId),
    #[error("unknown stock for history request: {0:?}")]
    UnknownHistoryStock(StockId),
    #[error("history read could not be recorded: {0}")]
    InvalidHistoryRead(crate::error::ValidationFault),
    #[error("session resource limit exceeded: {0}")]
    ResourceLimit(crate::error::ValidationFault),
    /// 透传 market 错误。
    #[error(transparent)]
    Market(#[from] MarketError),
    /// 透传 account 错误。
    #[error(transparent)]
    Account(#[from] AccountError),
    /// 透传 money 错误。
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// 透传 NPC 策略参数错误。
    #[error(transparent)]
    Strategy(#[from] StrategyError),
    /// 透传散户经历状态更新错误。
    #[error(transparent)]
    Experience(#[from] ExperienceError),
    /// 透传游戏配置错误。
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// 存档 schema 或领域不变量不合法。数值码+数值上下文（ADR 0041）。
    #[error("invalid save: {0}")]
    InvalidSave(crate::error::ValidationFault),
    /// 透传交易日历错误（冻结日历与双时钟 开局日期门等）。
    #[error(transparent)]
    Calendar(#[from] crate::calendar::CalendarError),
    /// 透传自然日时钟错误（冻结日历与双时钟 日结验证/原子失败）。
    #[error(transparent)]
    CivilClock(#[from] CivilClockError),
    /// 透传公司经营错误（会话装配与执行接线 日终编排：装配校验与逐日经营推进）。
    #[error(transparent)]
    CompanyOperations(Box<crate::company::operations::OperationsError>),
    /// 透传披露派发错误（会话装配与执行接线 日终编排）。
    #[error(transparent)]
    Disclosure(#[from] DisclosureError),
    /// 透传结账错误（会话装配与执行接线 月/年末封账）。
    #[error("accounting closing failed: {0}")]
    Closing(#[source] crate::accounting::closing::ClosingError),
    /// 透传个体分析档案派生错误（会话装配与执行接线 信念机构装配）。
    #[error(transparent)]
    StrategyAnalysis(#[from] crate::strategy::AnalysisProfileError),
    /// 透传只读公开信息查询错误。
    #[error(transparent)]
    Information(#[from] Box<crate::information::InformationError>),
}

impl From<crate::company::operations::OperationsError> for SessionError {
    fn from(source: crate::company::operations::OperationsError) -> Self {
        Self::CompanyOperations(Box::new(source))
    }
}

/// A 股证券类别；类别决定涨跌幅与差异化申报数量上限。
#[derive(
    Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS,
)]
#[ts(export)]
pub enum SecurityCategory {
    /// 沪深主板普通股票：10% 涨跌幅。
    #[default]
    MainBoard,
    /// 风险警示主板股票：自 2026-07-06 起与普通主板同为 10% 涨跌幅。
    StMainBoard,
    /// 深交所创业板股票：20% 涨跌幅，且有差异化单笔申报上限。
    ChiNext,
}

/// A 股上市交易所。交易所与证券类别正交，决定集合竞价等市场级规则。
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS)]
#[ts(export)]
pub enum StockExchange {
    Shanghai,
    Shenzhen,
}

impl StockExchange {
    /// 从当前模型支持的六位沪深 A 股代码识别交易所，用于配置一致性校验。
    pub fn from_a_share_code(code: &StockCode) -> Result<Self, String> {
        let value = code.0.as_str();
        if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!(
                "stock code {} is not a supported six-digit A-share code",
                code.0
            ));
        }
        let prefix = &value[..3];
        if matches!(prefix, "600" | "601" | "603" | "605") {
            Ok(Self::Shanghai)
        } else if matches!(prefix, "000" | "001" | "002" | "003" | "300" | "301") {
            Ok(Self::Shenzhen)
        } else {
            Err(format!(
                "stock code {} is outside the supported Shanghai/Shenzhen A-share boards",
                code.0
            ))
        }
    }
}

impl SecurityCategory {
    pub const fn limit_pct(self) -> f64 {
        match self {
            Self::MainBoard => 0.10,
            Self::StMainBoard => 0.10,
            Self::ChiNext => 0.20,
        }
    }

    /// 单笔申报数量上限。
    ///
    /// 创业板限价 30 万股、市价 15 万股，见深交所创业板交易特别规定官方问答；
    /// 主板在当前统一竞价模型中沿用 100 万股上限。
    pub const fn max_order_qty(self, is_market: bool) -> u32 {
        match (self, is_market) {
            (Self::ChiNext, true) => 150_000,
            (Self::ChiNext, false) => 300_000,
            _ => 1_000_000,
        }
    }

    fn display_limit(self) -> &'static str {
        match self {
            Self::MainBoard => "10%",
            Self::StMainBoard => "10%",
            Self::ChiNext => "20%",
        }
    }
}

/// 单只股票初始规格（行情/涨跌停/tick/总股本/流通盘）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct StockSpec {
    pub code: StockCode,
    /// 上市交易所；配置与存档必须显式提供。
    pub exchange: StockExchange,
    pub initial_price: Money,
    /// 证券板块/风险警示类别；配置与存档必须显式提供。
    pub category: SecurityCategory,
    pub limit_pct: f64,
    pub tick: Money,
    /// 公司总股本。使用十进制字符串跨 JSON，避免未来大盘股超过 JavaScript 安全整数。
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub total_shares: u64,
    /// 流通盘股数：新游戏时分配给 NPC。0 表示不分配。
    pub float_shares: u32,
}

/// 类间流通盘分配方式。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum BetweenKindDistribution {
    /// 按种子化随机权重分配给存在的 NPC 类别。
    Random,
    /// 三类各占比例；缺失类别的比例由存在类别按权重归一分配。
    Percentage { retail: f64, inst: f64, hot: f64 },
}

/// 类内流通盘分配方式。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum WithinKindDistribution {
    /// 按种子化随机权重分配；散户随机分配给部分账户以形成稀疏持仓。
    Random,
    /// 在该类所有 NPC 账户间尽可能等分，股数余数按 AccountId 升序分配。
    EqualPercentage,
}

/// 流通盘类间与类内的独立分配设置。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct FloatAllocation {
    pub between_kinds: BetweenKindDistribution,
    pub within_kind: WithinKindDistribution,
}

impl FloatAllocation {
    pub const fn random() -> Self {
        Self {
            between_kinds: BetweenKindDistribution::Random,
            within_kind: WithinKindDistribution::Random,
        }
    }

    pub const fn class_percentages(
        retail: f64,
        inst: f64,
        hot: f64,
        within_kind: WithinKindDistribution,
    ) -> Self {
        Self {
            between_kinds: BetweenKindDistribution::Percentage { retail, inst, hot },
            within_kind,
        }
    }
}

fn split_by_weights(float: u32, weights: &[f64]) -> Vec<u32> {
    let total = weights.iter().sum::<f64>();
    let last_positive = weights.iter().rposition(|weight| *weight > 0.0);
    let mut remaining = float;
    weights
        .iter()
        .enumerate()
        .map(|(index, weight)| {
            if *weight == 0.0 {
                0
            } else if Some(index) == last_positive {
                remaining
            } else {
                let quantity = ((float as f64 * *weight / total).round() as u32).min(remaining);
                remaining -= quantity;
                quantity
            }
        })
        .collect()
}

fn split_equally(float: u32, account_ids: &[AccountId]) -> Vec<(AccountId, u32)> {
    if account_ids.is_empty() {
        return Vec::new();
    }
    let account_count = account_ids.len() as u64;
    let base_quantity = u64::from(float) / account_count;
    let remainder = u64::from(float) % account_count;
    account_ids
        .iter()
        .enumerate()
        .map(|(index, account_id)| {
            let remainder_share = u64::from((index as u64) < remainder);
            (*account_id, (base_quantity + remainder_share) as u32)
        })
        .collect()
}

/// NPC 账户配置。每个计数都创建对应数量的独立账户；现金从散户中位数及
/// 各类公开尺度分布逐户采样，不代表共享资金池或聚合账户。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct NpcSetup {
    pub retail_count: u32,
    pub inst_count: u32,
    pub hot_count: u32,
    /// 独立自然人散户初始现金的中位数；机构和游资分别按更高的账户尺度采样。
    pub retail_cash_median: Money,
}

/// session 初始化参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct SessionSetup {
    pub stocks: Vec<StockSpec>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "import(\"../../save/schema/company/operations\").CompanyOperationsConfig | null")]
    pub company_operations: Option<crate::company::operations::CompanyOperationsConfig>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "import(\"../../save/schema/company/groups\").GroupStructure[]")]
    pub groups: Vec<GroupStructure>,
    pub npcs: NpcSetup,
    pub config: GameConfig,
    pub strategy_params: StrategyParams,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub ticks_per_day: u64,
    /// 每个交易日 09:15–09:30 开盘窗口的 tick 数；前 2/3 为集合竞价申报，后 1/3 为 PreOpen。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub auction_ticks: u64,
    /// 每个交易日 14:57–15:00 收盘集合竞价申报窗口的 tick 数。0 仅用于未覆盖尾盘
    /// 集合竞价的短周期测试；正式 A 股默认局必须显式配置该窗口。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub closing_auction_ticks: u64,
    pub history_len: usize,
    pub t1_enabled: bool,
    /// 流通盘分配方式（新游戏时如何把 float_shares 分给 NPC）。
    pub float_allocation: FloatAllocation,
    /// 开局自然日。缺省为政策默认 2030-01-01；合法开局 2000-01-01..2099-12-31
    /// （1998–1999 仅供初始化前史查询）。休市起点保持原日，不挪到开市日。
    /// serde 缺省仅供宿主过渡期不发送该字段时使用；存档总是显式写出。
    #[serde(default = "default_civil_start_date")]
    pub start_date: crate::calendar::CivilDate,
    /// 模拟政策身份：本引擎当前行为契约（A 股交易语义、公司域、
    /// 决策链参数族）的稳定版本标识。新档必填；与存档一起固化，恢复时不
    /// 与任何“最新默认”比对或迁移——身份不匹配的档由宿主层拒绝。
    pub simulation_policy_id: String,
}

impl SessionSetup {
    /// 由规格构造股票目录（ADR 0041）：按代码升序分配 id，id 序即代码序。
    /// `GameSession::new` 与读档路径共用，保证两端 id 分配一致。
    pub fn stock_catalog(&self) -> Result<StockCatalog, SessionError> {
        StockCatalog::from_metas(
            self.stocks
                .iter()
                .map(|spec| crate::stock::StockMeta::new(spec.code.0.clone()))
                .collect(),
        )
        .map_err(|_| SessionError::InvalidSetup(ValidationFault::new(1001, vec![0])))
    }

    /// 全部股票 id（按代码序）。构造/装配路径的便捷视图。
    pub fn stock_ids(&self, catalog: &StockCatalog) -> Result<Vec<StockId>, SessionError> {
        self.stocks
            .iter()
            .enumerate()
            .map(|(spec_index, spec)| {
                catalog.id(&spec.code.0).ok_or_else(|| {
                    // 1002：规格代码未入目录；ctx[0]=规格下标。
                    SessionError::InvalidSetup(ValidationFault::new(1002, vec![spec_index as u64]))
                })
            })
            .collect()
    }
    /// 校验所有启动期外部输入。serde 可绕过各子类型构造器，因此 session 创建和恢复
    /// 都必须从这里进入，验证成功后才构造权威状态。
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.stocks.is_empty() {
            // fault1005: 股票规格列表为空。
            return Err(SessionError::InvalidSetup(ValidationFault::bare(1005)));
        }
        if self.simulation_policy_id != SIMULATION_POLICY_ID {
            // fault1006: 模拟政策身份不匹配。槽0/槽1 待 C 批数值化
            // （原文本为期望政策 id "{SIMULATION_POLICY_ID}" 与实际政策 id "{:?}"）。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1006,
                vec![0, 0],
            )));
        }
        // 冻结日历与双时钟 开局日期门：运行区间 2000-01-01..2099-12-31；1998–1999 仅供前史。
        TradingCalendar::current_default_calendar()?.validate_runtime_start(self.start_date)?;
        if self.ticks_per_day == 0 {
            // fault1007: ticks_per_day 必须为正。
            return Err(SessionError::InvalidSetup(ValidationFault::bare(1007)));
        }
        if self.auction_ticks >= self.ticks_per_day {
            // fault1008: auction_ticks 必须 < ticks_per_day。ctx = [auction_ticks, ticks_per_day]。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1008,
                vec![self.auction_ticks, self.ticks_per_day],
            )));
        }
        if self
            .auction_ticks
            .checked_add(self.closing_auction_ticks)
            .is_none_or(|total| total >= self.ticks_per_day)
        {
            // fault1009: auction_ticks + closing_auction_ticks 必须 < ticks_per_day。
            // ctx = [auction_ticks, closing_auction_ticks, ticks_per_day]。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1009,
                vec![
                    self.auction_ticks,
                    self.closing_auction_ticks,
                    self.ticks_per_day,
                ],
            )));
        }
        if self.history_len == 0 {
            // fault1010: history_len 必须为正。
            return Err(SessionError::InvalidSetup(ValidationFault::bare(1010)));
        }
        if self.npcs.retail_cash_median.cents() < 0 {
            // fault1011: 散户现金中位数必须非负（i64 分负值，幅值由配置侧自证）。
            return Err(SessionError::InvalidSetup(ValidationFault::bare(1011)));
        }
        self.config.validate()?;
        if !self.t1_enabled {
            // fault1012: 正式 A 股局必须启用 T+1 结算。
            return Err(SessionError::InvalidSetup(ValidationFault::bare(1012)));
        }
        if (self.config.stamp_tax_rate - 0.0005).abs() > f64::EPSILON {
            // fault1013: 卖出印花税率必须为 0.0005。ctx[0] = 实际税率 f64 的 IEEE-754 位模式
            // （C 批可改定点点表示）。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1013,
                vec![self.config.stamp_tax_rate.to_bits()],
            )));
        }
        if (self.config.default_limit - 0.10).abs() > f64::EPSILON
            || (self.config.st_limit - 0.10).abs() > f64::EPSILON
        {
            // fault1014: 主板/风险警示涨跌幅限制必须均为 10%。
            return Err(SessionError::InvalidSetup(ValidationFault::bare(1014)));
        }
        self.strategy_params.validate()?;
        let lot_size = self.config.lot_size;
        for (active, name, quantity) in [
            (
                self.npcs.retail_count > 0,
                "strategy_params.retail.order_size_mean",
                self.strategy_params.retail.order_size_mean,
            ),
            (
                self.npcs.inst_count > 0,
                "strategy_params.inst.order_size",
                self.strategy_params.inst.order_size,
            ),
            (
                self.npcs.hot_count > 0,
                "strategy_params.hot.order_size",
                self.strategy_params.hot.order_size,
            ),
        ] {
            if active && quantity % lot_size != 0 {
                // fault1015: 活跃类别下单均值必须是 A 股 lot_size 的整数倍。
                // 槽0 待 C 批数值化（原文本为参数名 "{name}"，取值
                // "strategy_params.retail.order_size_mean"/"strategy_params.inst.order_size"/
                // "strategy_params.hot.order_size"）；ctx[1] = 下单均值，ctx[2] = lot_size。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1015,
                    vec![0, u64::from(quantity), u64::from(lot_size)],
                )));
            }
        }
        let mut codes = BTreeSet::new();
        for stock in &self.stocks {
            if !codes.insert(stock.code.clone()) {
                // fault1016: 股票代码重复。槽0 待 C 批数值化（原文本为代码串 "{}"）。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1016,
                    vec![0],
                )));
            }
            if stock.tick != Money::from_cents(1) {
                // fault1017: 最小价位必须为 A 股 0.01 元。槽0 待 C 批数值化（原文本为代码串 "{}"）。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1017,
                    vec![0],
                )));
            }
            if stock.total_shares == 0 {
                // fault1018: 总股本必须为正。槽0 待 C 批数值化（原文本为代码串 "{}"）。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1018,
                    vec![0],
                )));
            }
            if u64::from(stock.float_shares) > stock.total_shares {
                // fault1019: 流通盘不得超过总股本。槽0 待 C 批数值化（原文本为代码串 "{}"）；
                // ctx[1] = float_shares，ctx[2] = total_shares。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1019,
                    vec![0, u64::from(stock.float_shares), stock.total_shares],
                )));
            }
            // fault1020: 代码不是受支持的六位沪深 A 股代码（前缀识别失败）。
            // 槽0 待 C 批数值化（原文本为内层 String 错误，含代码串）。
            let inferred_exchange = StockExchange::from_a_share_code(&stock.code)
                .map_err(|_| SessionError::InvalidSetup(ValidationFault::new(1020, vec![0])))?;
            if stock.exchange != inferred_exchange {
                // fault1021: 配置交易所与代码所属板块不一致。槽0 待 C 批数值化（原文本为代码串 "{}"）；
                // ctx[1] = 配置交易所（StockExchange 判别值 0=Shanghai/1=Shenzhen），
                // ctx[2] = 代码推导交易所。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1021,
                    vec![0, stock.exchange as u64, inferred_exchange as u64],
                )));
            }
            let is_chinext_code = matches!(&stock.code.0[..3], "300" | "301");
            if (stock.category == SecurityCategory::ChiNext) != is_chinext_code {
                // fault1022: 证券类别与代码板块冲突。槽0 待 C 批数值化（原文本为代码串 "{}"）；
                // ctx[1] = SecurityCategory 判别值（0=MainBoard/1=StMainBoard/2=ChiNext）。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1022,
                    vec![0, stock.category as u64],
                )));
            }
            if (stock.limit_pct - stock.category.limit_pct()).abs() > f64::EPSILON {
                // fault1023: 涨跌幅与类别要求不符。槽0 待 C 批数值化（原文本为代码串 "{}"）；
                // ctx[1] = SecurityCategory 判别值；类别对应涨跌幅（"10%"/"20%"）由 ctx[1] 唯一决定，
                // 原文本第三个参数无数值化必要。
                return Err(SessionError::InvalidSetup(ValidationFault::new(
                    1023,
                    vec![0, stock.category as u64],
                )));
            }
        }
        if let BetweenKindDistribution::Percentage { retail, inst, hot } =
            &self.float_allocation.between_kinds
        {
            for (value, name) in [(*retail, "retail"), (*inst, "inst"), (*hot, "hot")] {
                if !value.is_finite() || value < 0.0 {
                    // fault1024: 类间流通盘比例必须为有限非负 f64。槽0 待 C 批数值化
                    // （原文本为类别名 "{name}"，取值 "retail"/"inst"/"hot"）；
                    // ctx[1] = 比例值 f64 的 IEEE-754 位模式（C 批可改定点点表示）。
                    return Err(SessionError::InvalidSetup(ValidationFault::new(
                        1024,
                        vec![0, value.to_bits()],
                    )));
                }
            }
            if self.stocks.iter().any(|stock| stock.float_shares > 0)
                && (self.npcs.retail_count > 0
                    || self.npcs.inst_count > 0
                    || self.npcs.hot_count > 0)
            {
                let effective_weight = [
                    (self.npcs.retail_count, *retail),
                    (self.npcs.inst_count, *inst),
                    (self.npcs.hot_count, *hot),
                ]
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(_, weight)| weight)
                .sum::<f64>();
                if !effective_weight.is_finite() || effective_weight <= 0.0 {
                    // fault1025: 存在 NPC 类别时类间比例之和必须为有限正数。
                    return Err(SessionError::InvalidSetup(ValidationFault::bare(1025)));
                }
            }
        }
        Ok(())
    }
}

/// 编排层。持有全部状态；纯逻辑、无全局可变状态（实例即隔离）。
///
/// NPC 与玩家同构（均为 [`Account`]），区别只在 `strategy`（NPC=算法、玩家=None）。
/// 会话 RNG、按 tick 派生的决策 RNG 与可存档的个体注意力 RNG 都源于同一 seed，
/// 且用途彼此分离。种子固定随机决定；并发交易的先后仍由实际局部受理决定。
pub struct GameSession {
    compute: std::sync::Arc<crate::compute::ComputeContext>,
    ingress: Option<shared_ingress::IngressBinding>,
    /// 外部直改计数（X1 新鲜度校验）：任何绕过 tick 管线、在 commit 与下个
    /// plan_tick 之间改权威状态的行为必须自增（直改入口统一标记）。
    /// 不入 state 克隆链——shadow 克隆取值快照，commit 不回写。
    revision: std::sync::atomic::AtomicU64,
    poison: Option<StepFatal>,
    fresh_initial_allocation: bool,
    #[cfg(test)]
    injected_failure: Option<StepFatal>,
    #[cfg(test)]
    post_shadow_failure: Option<StepFatal>,
    /// 测试注入：runtime 步进成功后强制安装失败（Sol 二审修复集 3——
    /// 与 inject_post_shadow_failure 同型的安装面失败注入）。
    #[cfg(test)]
    runtime_install_failure: bool,
    /// D0 常驻 runtime 驱动器：step_via_runtime 惰性构造、跨 step 复用
    /// （world tick 与 session.tick 锁步）。不进 CommittableSessionState
    /// ——线程句柄不是可提交状态；shadow 置 None、commit_tick_shadow
    /// 只换 ingress/state ⇒ 常驻实例穿越日结不被覆盖。
    runtime_driver: Option<std::sync::Mutex<crate::runtime::drive::RuntimeDriver>>,
    state: CommittableSessionState,
}

/// tick shadow 与自然日日结共同提交的唯一状态集合。
struct CommittableSessionState {
    setup: SessionSetup,
    /// 运行时-only 费率快照（任务65 #1 前置条件守卫）：于 GameSession::new（含恢复重建）
    /// 时固定，不参与存档/business_state_hash。逐 tick 与 `setup.config` O(1) 比对，
    /// 任何静默热改 → capture/校验门禁 fail closed（见 envelope_projection 守卫）。
    validated_config: GameConfig,
    /// 股票目录（ADR 0041）：id ↔ 代码/显示名的唯一翻译层，构造后不可变。
    stocks: StockCatalog,
    rng: SplitMix64,
    seed: u64,
    markets: BTreeMap<StockId, Market>,
    accounts: AccountBook,
    history_reads: AccountPagedMap<crate::experience::PersonalHistoryReadLedger>,
    price_history: BTreeMap<StockId, VecDeque<Money>>,
    market_minute_closes: BTreeMap<StockId, Vec<MarketMinuteClose>>,
    candle_book: SessionCandleBook,
    pending_player: Vec<ReceiptBearingIntent>,
    ingress_receipt_cursors: IngressReceiptCursors,
    npc_attention: AccountPagedMap<NpcAttentionState>,
    retail_experience: AccountPagedMap<RetailExperienceState>,
    npc_order_lifecycles: Vec<NpcOrderLifecycle>,
    /// 上一 tick 中实际观察并判断目标仓位与经历调整的散户目标仓位样本。
    /// 这是诊断缓存，不进入存档、不会被策略读取，也不属于权威游戏状态。
    last_retail_decisions: Vec<RetailDecisionTrace>,
    last_retail_order_events: Vec<RetailOrderDiagnosticEvent>,
    #[cfg(feature = "simulation-diagnostics")]
    npc_decision_traces: crate::diagnostics::decision_trace::NpcDecisionTraceCollector,
    #[cfg(feature = "simulation-diagnostics")]
    causal: crate::diagnostics::causal::CausalCollector,
    attention_scheduler: NpcAttentionScheduler,
    // 公司域与完整决策链的权威状态；全部经完整存档保存恢复。
    /// 发行人注册表（公司规格与开局账套）：股票 ↔ 公司映射与开局账套。
    company_registry: std::sync::Arc<crate::company::CompanyRegistry>,
    /// 自然日经营编排（自然日经营演化；前史已推进到开局日）。
    operations: std::sync::Arc<crate::company::operations::CompanyOperations>,
    groups: Vec<GroupStructure>,
    /// 结账版本登记簿（结账与报表）。
    closing: std::sync::Arc<crate::accounting::closing::ClosingEngine>,
    /// 公开信息库（公开信息；前史已播种）。
    library: std::sync::Arc<crate::information::PublicLibrary>,
    /// 披露派发游标。
    disclosures: DisclosureDispatch,
    /// 跨日个人交易计划（个人计划生命周期；PlanBook 本身支持全账户）。
    plans: crate::plans::PlanBook,
    urgency_policy: crate::plans::UrgencyPolicy,
    /// 每个信念机构的独立个人认识、经历和关注事实。
    belief_participants: AccountPagedMap<BeliefParticipantState>,
    /// 在簿回执账本；跨存档由 `SavedRuntimeState` 的 `live_envelopes` 持久恢复。
    envelope_ledger: pipeline::EnvelopeLedger,
    /// 已由原子 Settlement 账户/经历投影消费的回执；随 tick shadow 提交的权威去重事实。
    retail_projection_seen: pipeline::RetailProjectionSeen,
    /// 下一个全局回执索引；跨存档由 `SavedRuntimeState` 的 `next_receipt_base` 持久恢复。
    next_receipt_base: u64,
    next_order_id: u64,
    tick: u64,
    day: u32,
    seq: u64,
    /// 冻结日历与双时钟 自然日经营时钟：与 tick/交易日计数分离的权威自然日推进。
    civil_clock: CivilClock,
}

/// commit_from 逐字段移交的字段身份（Luna 修复1 + Astra 修复2升级 + 增量五复检收窄）。
/// 仅 verification-harness 计量消费；析构语义零变化。完整性契约（如实范围，不称
/// 无条件保证）：
/// a) 防护面：任何 defer_swap 调用点的删除、字段集成员/数量变化（含加字段不
///    更新全集、FieldId variant 复用）必然让
///    verify_metrics_classifies_real_defer_swap_fields_as_commit_field 变红；
/// b) 盲区 1（已知，如实登记）：FieldId 是调用点自报标签，未与真实字段做类型级
///    绑定——错标（把 A 字段的 swap 标成 B 的 FieldId）叠加"删一换一"可绕过本绊线；
/// c) 盲区 2（已知，如实登记）：新增重字段若不经 defer_swap 而直接赋值（commit
///    路径同步析构），本绊线不拦截——它只观测走 defer_swap 的移交。
/// 同步清单：给 CommittableSessionState 增删字段时 ①增删枚举项 ②commit_from 的
/// defer_swap 传新项 ③同步更新两个 cfg 变体的 COMMIT_FIELD_EXPECTED 全集。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[repr(u8)]
pub(in crate::session) enum FieldId {
    Setup = 0,
    ValidatedConfig,
    Stocks,
    Rng,
    Markets,
    Accounts,
    HistoryReads,
    PriceHistory,
    MarketMinuteCloses,
    CandleBook,
    PendingPlayer,
    IngressReceiptCursors,
    NpcAttention,
    RetailExperience,
    // owner8 引擎闭包（批次④）删除 parent_orders/pending_plan_events 两个 state 字段，
    // 其 FieldId 变体随之移除（设计内更新，非弱化——见下方 COMMIT_FIELD_EXPECTED）。
    NpcOrderLifecycles,
    LastRetailDecisions,
    LastRetailOrderEvents,
    NpcDecisionTraces,
    Causal,
    AttentionScheduler,
    CompanyRegistry,
    Operations,
    Groups,
    Closing,
    Library,
    Disclosures,
    Plans,
    BeliefParticipants,
    EnvelopeLedger,
    RetailProjectionSeen,
    CivilClock,
    SpeculativePrepare,
}

/// commit_from 必须移交的字段全集（修复1·Astra 增量四复检 cfg 缺陷处置：按 cfg
/// 条件化，与 commit_from 的条件 swap 对齐，绊线在所有真实构建组合内有效）。
/// 默认 features 为 30 项；开启 simulation-diagnostics 时为 32 项（追加
/// NpcDecisionTraces/Causal）。owner8 批次④删除两字段后由 33/35 收窄为 31/33
///（Astra 终审口径校正：下文一律按现行 31/33 口径表述）。一致性要求（Astra
/// 增量六措辞校正；批四删除 PendingNpc 字段后 30/32 口径）：32 项变体去掉两个
/// 诊断项（NpcDecisionTraces/Causal）后，其余 30 项与默认变体逐项一致——诊断项插在默认变体第 20 项
/// （LastRetailOrderEvents）之后，其后的项在 33 项变体中下标后移，逐下标位置
/// 并不一致（人工同步维护，盲区 1 的自报标签性质同样适用于此处清单）。
#[cfg(not(feature = "simulation-diagnostics"))]
pub(in crate::session) const COMMIT_FIELD_EXPECTED: [FieldId; 30] = [
    FieldId::Setup,
    FieldId::ValidatedConfig,
    FieldId::Stocks,
    FieldId::Rng,
    FieldId::Markets,
    FieldId::Accounts,
    FieldId::HistoryReads,
    FieldId::PriceHistory,
    FieldId::MarketMinuteCloses,
    FieldId::CandleBook,
    FieldId::PendingPlayer,
    FieldId::IngressReceiptCursors,
    FieldId::NpcAttention,
    FieldId::RetailExperience,
    FieldId::NpcOrderLifecycles,
    FieldId::LastRetailDecisions,
    FieldId::LastRetailOrderEvents,
    FieldId::AttentionScheduler,
    FieldId::CompanyRegistry,
    FieldId::Operations,
    FieldId::Groups,
    FieldId::Closing,
    FieldId::Library,
    FieldId::Disclosures,
    FieldId::Plans,
    FieldId::BeliefParticipants,
    FieldId::EnvelopeLedger,
    FieldId::RetailProjectionSeen,
    FieldId::CivilClock,
    FieldId::SpeculativePrepare,
];

#[cfg(feature = "simulation-diagnostics")]
pub(in crate::session) const COMMIT_FIELD_EXPECTED: [FieldId; 32] = [
    FieldId::Setup,
    FieldId::ValidatedConfig,
    FieldId::Stocks,
    FieldId::Rng,
    FieldId::Markets,
    FieldId::Accounts,
    FieldId::HistoryReads,
    FieldId::PriceHistory,
    FieldId::MarketMinuteCloses,
    FieldId::CandleBook,
    FieldId::PendingPlayer,
    FieldId::IngressReceiptCursors,
    FieldId::NpcAttention,
    FieldId::RetailExperience,
    FieldId::NpcOrderLifecycles,
    FieldId::LastRetailDecisions,
    FieldId::LastRetailOrderEvents,
    FieldId::NpcDecisionTraces,
    FieldId::Causal,
    FieldId::AttentionScheduler,
    FieldId::CompanyRegistry,
    FieldId::Operations,
    FieldId::Groups,
    FieldId::Closing,
    FieldId::Library,
    FieldId::Disclosures,
    FieldId::Plans,
    FieldId::BeliefParticipants,
    FieldId::EnvelopeLedger,
    FieldId::RetailProjectionSeen,
    FieldId::CivilClock,
    FieldId::SpeculativePrepare,
];

/// FieldId 全变体数（含 simulation-diagnostics 两项），verify_metrics 计数数组长度。
pub(in crate::session) const FIELD_ID_COUNT: usize = 32;
const _: () = assert!(FieldId::SpeculativePrepare as usize == FIELD_ID_COUNT - 1);

/// 一个真实进入散户策略判断路径的目标仓位样本。
///
/// 仅用于离线联合验收；它保留判断输入产生的目标，不把“目标”误记成委托或成交。
#[derive(Clone, Debug, PartialEq)]
pub struct RetailDecisionTrace {
    pub account: AccountId,
    pub decision: PositionDecision,
    pub execution_urgency: crate::plans::urgency::risk::RiskUrgencyAssessment,
}

/// 真实散户订单生命周期的瞬时诊断事件。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum RetailOrderDiagnosticEvent {
    Submitted {
        account: AccountId,
        code: StockId,
        side: Side,
        order_id: OrderId,
        qty: u32,
    },
    Filled {
        account: AccountId,
        code: StockId,
        side: Side,
        order_id: OrderId,
        qty: u32,
    },
    Canceled {
        account: AccountId,
        code: StockId,
        order_id: OrderId,
        remaining_qty: u32,
    },
    /// 集合竞价原子提交失败后未写入连续订单簿的剩余委托；它不是“仍在簿中”。
    Aborted {
        account: AccountId,
        code: StockId,
        order_id: OrderId,
        remaining_qty: u32,
    },
    Rejected {
        account: AccountId,
        code: StockId,
        reason: RejectionReason,
    },
}

#[derive(Copy, Clone)]
struct OrderFillSettlement {
    account: AccountId,
    side: Side,
    order_id: OrderId,
    qty: u32,
    #[cfg(feature = "simulation-diagnostics")]
    gross: Money,
}

fn sample_npc_cash(
    kind: AccountKind,
    retail_cash_median: Money,
    seed: u64,
    account: AccountId,
) -> Result<Money, SessionError> {
    if retail_cash_median == Money::ZERO {
        return Ok(Money::ZERO);
    }
    let (kind_multiplier, log_std_dev) = match kind {
        AccountKind::Retail => (1.0, 1.0),
        AccountKind::Inst => (25_000.0, 0.35),
        AccountKind::Hot => (5_000.0, 0.50),
        AccountKind::Player => (1.0, 0.0),
    };
    let mut rng = SplitMix64::new(
        seed ^ account.0.wrapping_mul(0xD1B5_4A32_D192_ED03) ^ 0xC45A_5EED_5CA1_E001,
    );
    let u1 = rng.next_f64().max(f64::MIN_POSITIVE);
    let u2 = rng.next_f64();
    let normal = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    let wealth_factor = (normal * log_std_dev).exp().clamp(0.05, 20.0);
    let cents = retail_cash_median.cents() as f64 * kind_multiplier * wealth_factor;
    if !cents.is_finite() || cents > i64::MAX as f64 {
        // fault1026: NPC 初始现金采样超出 i64 分表示范围。
        // ctx[0] = AccountKind 判别值（0=Retail/1=Inst/2=Hot/3=Player），ctx[1] = AccountId。
        return Err(SessionError::InvalidSetup(ValidationFault::new(
            1026,
            vec![kind as u64, account.0],
        )));
    }
    Ok(Money::from_cents(cents.round() as i64))
}

/// ADR-0017 seller envelopes reserve shares only. Fees are charged from sale proceeds.
fn live_cash_reservation(
    config: &GameConfig,
    side: Side,
    limit: Money,
    qty: u32,
    filled_value: Money,
) -> Result<Money, MoneyError> {
    match side {
        Side::Buy => buy_order_reservation(config, limit, qty, filled_value),
        Side::Sell => Ok(Money::ZERO),
    }
}

const MAX_BEHAVIOR_DAILY_HISTORY: usize = 250;

#[cfg(test)]
fn retained_behavior_daily_closes(candles: &[DailyCandle]) -> Vec<CompletedDayClose> {
    candles
        .iter()
        .enumerate()
        .skip(candles.len().saturating_sub(MAX_BEHAVIOR_DAILY_HISTORY))
        .map(|(index, candle)| CompletedDayClose {
            trading_day: u32::try_from(index).expect("retained daily history length fits u32"),
            close: candle.close,
        })
        .collect()
}

fn retained_behavior_daily_closes_history(history: &DailyCandleHistory) -> Vec<CompletedDayClose> {
    let start = history.len().saturating_sub(MAX_BEHAVIOR_DAILY_HISTORY);
    history
        .recent(MAX_BEHAVIOR_DAILY_HISTORY)
        .into_iter()
        .enumerate()
        .map(|(offset, candle)| CompletedDayClose {
            trading_day: u32::try_from(start + offset)
                .expect("retained daily history length fits u32"),
            close: candle.close,
        })
        .collect()
}

impl GameSession {
    /// 外部直改计数当前值（X1 新鲜度校验）。
    pub(crate) fn external_revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 标记一次绕过 tick 管线的权威直改（X1 红线：直改必须先 mark）。
    pub(crate) fn mark_external_mutation(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// 测试夹具对 markets 的直改入口：标记外部直改后返回可变市场。
    #[cfg(test)]
    pub(crate) fn fixture_market_mut(
        &mut self,
        code: &crate::StockId,
    ) -> Option<&mut crate::Market> {
        self.mark_external_mutation();
        self.state.markets.get_mut(code)
    }

    /// 直改入口：等价于 state.accounts.get_mut 并标记外部直改
    /// （测试夹具与存档恢复共用；X1 红线——直改必须经此标记）。
    pub(crate) fn fixture_account_mut(
        &mut self,
        id: &crate::AccountId,
    ) -> Option<&mut crate::Account> {
        self.mark_external_mutation();
        self.state.accounts.get_mut(id)
    }
}

impl GameSession {
    pub(super) fn clone_for_tick_shadow(&self) -> Result<Self, StepFatal> {
        Ok(Self {
            compute: self.compute.clone(),
            ingress: self
                .ingress
                .as_ref()
                .map(shared_ingress::IngressBinding::fork)
                .transpose()
                .map_err(shared_ingress::ingress_fatal)?,
            poison: None,
            revision: std::sync::atomic::AtomicU64::new(
                self.revision.load(std::sync::atomic::Ordering::Relaxed),
            ),
            fresh_initial_allocation: self.fresh_initial_allocation,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
            #[cfg(test)]
            runtime_install_failure: false,
            runtime_driver: None,
            state: self.state.clone_for_shadow()?,
        })
    }

    pub(super) fn commit_tick_shadow(&mut self, shadow: Self) {
        self.ingress = shadow.ingress;
        self.state.commit_from(shadow.state);
    }
    /// 构造 session。校验参数 → 建 markets/accounts → 注入 NPC 策略。
    ///
    /// - 校验 `stocks` 非空、`ticks_per_day > 0`，否则 [`SessionError::InvalidSetup`]（铁律二：绝不静默）。
    /// - 每股构造一个 [`Market`]（`last_close = last_price = initial_price`），并初始化空价格历史队列。
    /// - 玩家 `AccountId(0)`：`Account::new`（strategy 默认 None）。
    /// - NPC 按 retail/inst/hot 计数逐个生成（id 递增），`StrategyFactory::build` 注入策略。
    pub fn new(setup: SessionSetup, seed: u64) -> Result<GameSession, SessionError> {
        Self::new_with_compute(
            setup,
            seed,
            crate::compute::ComputeContext::shared_default()?,
        )
    }

    /// 宿主明确提供资源；整个会话、candidate 与读视图共享此上下文。
    pub fn new_with_compute(
        setup: SessionSetup,
        seed: u64,
        compute: std::sync::Arc<crate::compute::ComputeContext>,
    ) -> Result<GameSession, SessionError> {
        // T3 批四·staged 决策槽防串扰：libtest 复用线程，上一用例残留的
        // staged 批不得泄漏进本会话的 produce（cfg(test) 适配面专属）。
        #[cfg(test)]
        crate::session::pipeline::staged_decision::clear();
        Self::new_configured(setup, seed, 10_000, compute)
    }

    pub fn compute_context(&self) -> &std::sync::Arc<crate::compute::ComputeContext> {
        &self.compute
    }

    /// 验证/测试专用：把克隆会话重绑到实验用的执行上下文（按线程数对比执行边界）。
    /// 生产路径的资源所有权不经过此入口；不影响业务状态与存档。
    #[cfg(any(test, feature = "verification-harness"))]
    pub fn rebind_compute_for_verification(
        &mut self,
        compute: std::sync::Arc<crate::compute::ComputeContext>,
    ) {
        self.compute = compute;
    }

    /// Creates a fresh session with a validated diagnostic-only company-event multiplier.
    /// The multiplier is not saved and normal construction always uses 10,000 bp.
    pub fn new_with_company_event_multiplier(
        setup: SessionSetup,
        seed: u64,
        event_multiplier_bp: u16,
    ) -> Result<GameSession, SessionError> {
        Self::new_configured(
            setup,
            seed,
            event_multiplier_bp,
            crate::compute::ComputeContext::shared_default()?,
        )
    }

    fn new_configured(
        setup: SessionSetup,
        seed: u64,
        event_multiplier_bp: u16,
        compute: std::sync::Arc<crate::compute::ComputeContext>,
    ) -> Result<GameSession, SessionError> {
        if !matches!(event_multiplier_bp, 5_000 | 10_000 | 20_000) {
            // fault1027: 公司事件倍率只允许 5000/10000/20000 bp。ctx[0] = 实际倍率（bp）。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1027,
                vec![u64::from(event_multiplier_bp)],
            )));
        }
        setup.validate()?;
        // 股票目录（ADR 0041）：按代码升序分配 id，id 序即代码序；
        // 重复代码在此显式报错，绝不静默去重。
        let stocks = setup.stock_catalog()?;
        let mut markets = BTreeMap::new();
        let mut price_history = BTreeMap::new();
        let mut market_minute_closes = BTreeMap::new();
        for (spec_index, s) in setup.stocks.iter().enumerate() {
            // 边界代码 → 数值 id；目录刚由同一批 specs 构造，查不到即内部不变量破损。
            // 1002：规格代码未入目录；ctx[0]=规格下标。
            let id = stocks.id(&s.code.0).ok_or_else(|| {
                SessionError::InvalidSetup(ValidationFault::new(1002, vec![spec_index as u64]))
            })?;
            markets.insert(id, Market::new(id, s.initial_price, s.limit_pct, s.tick)?);
            price_history.insert(id, VecDeque::new());
            market_minute_closes.insert(id, Vec::new());
        }
        let daily_candles = generate_preset_daily_candles(&setup, &stocks, seed)?;
        let mut accounts = AccountBook::default();
        accounts.insert(
            AccountId(0),
            Account::new(
                AccountId(0),
                AccountKind::Player,
                setup.config.starting_cash,
            ),
        );
        let mut history_reads = AccountPagedMap::default();
        history_reads.insert(
            AccountId(0),
            crate::experience::PersonalHistoryReadLedger::default(),
        );
        let rng = SplitMix64::new(seed);
        let civil_clock = CivilClock::new(
            setup.start_date,
            session_calendar_exchange(setup.stocks[0].exchange),
        )?;
        // 会话装配与执行接线 新局装配：公司注册表 + 前史经营 + 公开库 + 时钟/披露接线。
        let company_assembly::CompanyAssembly {
            registry,
            prehistory,
        } = company_assembly::assemble_companies(&setup, seed, event_multiplier_bp)?;
        let seeded_through = prehistory.last_published_instant();
        let crate::information::SeededPrehistory {
            ops,
            closing,
            library,
            ..
        } = prehistory;
        company_groups::validate_groups(&setup.groups, &ops).map_err(|_error| {
            // fault1028: 公司分组配置校验失败。槽0 待 C 批数值化（原文本为内层校验错误文本）。
            SessionError::InvalidSetup(ValidationFault::new(1028, vec![0]))
        })?;
        let mut civil_clock = civil_clock;
        // 公司 due 单一 authority 后，装配边界仍按本局冻结日历校验全部待办 horizon。
        for due in ops.scheduler().pending() {
            civil_clock.validate_due_horizon(due.due_date)?;
        }
        let disclosures = DisclosureDispatch::new(seeded_through);
        disclosures.install(&mut civil_clock);
        let mut sess = GameSession {
            compute,
            ingress: None,
            revision: std::sync::atomic::AtomicU64::new(0),
            poison: None,
            fresh_initial_allocation: true,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
            #[cfg(test)]
            runtime_install_failure: false,
            runtime_driver: None,
            state: CommittableSessionState {
                #[cfg(feature = "simulation-diagnostics")]
                causal: crate::diagnostics::causal::CausalCollector::default(),
                groups: setup.groups.clone(),
                validated_config: setup.config.clone(),
                setup,
                stocks,
                rng,
                seed,
                markets,
                accounts,
                history_reads,
                price_history,
                market_minute_closes,
                candle_book: SessionCandleBook::new(daily_candles, BTreeMap::new()),
                pending_player: Vec::new(),
                ingress_receipt_cursors: IngressReceiptCursors::default(),
                npc_attention: AccountPagedMap::default(),
                retail_experience: AccountPagedMap::default(),
                npc_order_lifecycles: Vec::new(),
                last_retail_decisions: Vec::new(),
                last_retail_order_events: Vec::new(),
                #[cfg(feature = "simulation-diagnostics")]
                npc_decision_traces:
                    crate::diagnostics::decision_trace::NpcDecisionTraceCollector::default(),
                attention_scheduler: NpcAttentionScheduler::default(),
                company_registry: std::sync::Arc::new(registry),
                operations: std::sync::Arc::new(ops),
                closing: std::sync::Arc::new(closing),
                library: std::sync::Arc::new(library),
                disclosures,
                plans: crate::plans::PlanBook::default(),
                urgency_policy: crate::plans::UrgencyPolicy::default(),
                belief_participants: AccountPagedMap::default(),
                envelope_ledger: pipeline::EnvelopeLedger::new(0, [])
                    .expect("an empty envelope ledger is valid"),
                retail_projection_seen: pipeline::RetailProjectionSeen::default(),
                next_receipt_base: 0,
                next_order_id: 1,
                tick: 0,
                day: 0,
                seq: 0,
                civil_clock,
            },
        };
        let mode = pipeline::phase::PhasePolicy::capture(&sess).matching_mode();
        for market in sess.state.markets.values_mut() {
            market
                .set_matching_mode(mode)
                .expect("新局空簿可以进入配置交易阶段");
        }
        sess.populate_npcs(AccountKind::Retail)?;
        sess.populate_npcs(AccountKind::Inst)?;
        sess.populate_npcs(AccountKind::Hot)?;
        sess.seed_float()?; // 分配流通盘给 NPC（筹码守恒、确定性、玩家不分配）
        sess.initialize_retail_experience()?;
        sess.reconcile_institutional_holdings()?;
        // 日界不提前生成日内请求，首个 tick 在隔离 shadow 上基于日结后的版本准备。
        // 因此休市日日结和交易日日结都能产生不含待处理请求的公共存档。
        Ok(sess)
    }

    fn account_equity(&self, id: AccountId) -> Result<Money, MoneyError> {
        let account = self
            .state
            .accounts
            .get(&id)
            .expect("equity may only be computed for an existing account");
        account
            .positions()
            .iter()
            .try_fold(account.cash(), |total, (code, position)| {
                let price = self
                    .state
                    .markets
                    .get(code)
                    .unwrap_or_else(|| panic!("account {} holds unknown stock {}", id.0, code.0))
                    .last_price();
                total.add(price.mul_shares(position.qty())?)
            })
    }

    fn current_market_minute(&self) -> u64 {
        let day_start = u64::from(self.state.day)
            .checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .expect("u32 session day times 240 fits u64");
        let day_tick = self.state.tick % self.state.setup.ticks_per_day;
        let continuous_ticks_per_day = self
            .state
            .setup
            .ticks_per_day
            .saturating_sub(self.state.setup.auction_ticks)
            .saturating_sub(self.state.setup.closing_auction_ticks);
        let completed = completed_market_minute_count(
            day_tick
                .saturating_sub(self.state.setup.auction_ticks)
                .min(continuous_ticks_per_day),
            continuous_ticks_per_day,
        )
        .expect("validated session timing maps to market minutes");
        day_start + u64::from(completed)
    }

    fn initialize_retail_experience(&mut self) -> Result<(), SessionError> {
        let market_minute = self.current_market_minute();
        let moment = crate::experience::ExperienceMoment {
            civil_date: self.state.civil_clock.current_date(),
            market_minute,
            trading_day: u64::from(self.state.day),
        };
        let retail_ids: Vec<_> = self
            .state
            .accounts
            .iter()
            .filter_map(|(id, account)| (account.kind() == AccountKind::Retail).then_some(*id))
            .collect();
        for id in retail_ids {
            let equity = self.account_equity(id)?;
            let mut experience = if equity.cents() > 0 {
                RetailExperienceState::new(equity)?
            } else {
                RetailExperienceState::without_equity_reference()
            };
            let holdings: Vec<_> = self.state.accounts[&id]
                .positions()
                .iter()
                .map(|(code, position)| {
                    let current = self.state.markets[code].last_price();
                    (
                        code.clone(),
                        position.cost_price().filter(|price| price.cents() > 0),
                        current,
                    )
                })
                .collect();
            for (code, reference, current) in holdings {
                experience.initialize_holding_dated(&code, reference, current, moment)?;
            }
            self.state.retail_experience.insert(id, experience);
        }
        Ok(())
    }

    fn reconcile_institutional_holdings(&mut self) -> Result<(), SessionError> {
        let account_ids: Vec<_> = self
            .state
            .belief_participants
            .iter()
            .filter_map(|(id, participant)| {
                matches!(
                    participant.belief().profile(),
                    StrategyProfile::Institution(_)
                )
                .then_some(*id)
            })
            .collect();
        let moment = crate::experience::ExperienceMoment {
            civil_date: self.state.civil_clock.current_date(),
            market_minute: self.current_market_minute(),
            trading_day: u64::from(self.state.day),
        };
        for account_id in account_ids {
            let held: Vec<_> = self.state.accounts[&account_id]
                .positions()
                .iter()
                .map(|(code, position)| {
                    (
                        code.clone(),
                        position.cost_price().filter(|price| price.cents() > 0),
                        self.state.markets[code].last_price(),
                    )
                })
                .collect();
            let held_codes: BTreeSet<_> = held.iter().map(|(code, _, _)| code.clone()).collect();
            let mut participant = self
                .state
                .belief_participants
                .remove(&account_id)
                .expect("collected institution experience account exists");
            let experience = participant.belief_mut().experience_mut();
            let stale_codes: BTreeSet<_> = experience
                .feedback
                .stocks
                .keys()
                .chain(experience.stocks.keys())
                .filter(|code| !held_codes.contains(*code))
                .cloned()
                .collect();
            for code in stale_codes {
                experience.clear_stale_institutional_holding(&code);
            }
            for (code, reference, current_price) in held {
                if experience.feedback.stocks.contains_key(&code) {
                    if experience.feedback.stocks[&code]
                        .institutional_fees_paid
                        .is_none()
                    {
                        // fault1029: 机构持仓缺乏费用史（institutional_fees_paid 缺失）。
                        // ctx[0] = AccountId，ctx[1] = StockId。
                        return Err(SessionError::InvalidSave(ValidationFault::new(
                            1029,
                            vec![account_id.0, u64::from(code.0)],
                        )));
                    }
                } else {
                    experience.initialize_institutional_holding_dated(
                        &code,
                        reference,
                        current_price,
                        moment,
                    )?;
                }
            }
            self.state
                .belief_participants
                .insert(account_id, participant);
        }
        Ok(())
    }

    #[cfg(test)]
    fn observe_retail_experience(&mut self, ids: &[AccountId]) -> Result<(), ExperienceError> {
        let market_minute = self.current_market_minute();
        let observations: Vec<_> = ids
            .iter()
            .filter(|id| self.state.retail_experience.contains_key(id))
            .map(|id| {
                let equity = self
                    .account_equity(*id)
                    .expect("validated account equity must remain representable");
                let positions: Vec<_> = self.state.accounts[id]
                    .positions()
                    .keys()
                    .map(|code| (code.clone(), self.state.markets[code].last_price()))
                    .collect();
                (*id, equity, positions)
            })
            .collect();
        for (id, equity, positions) in observations {
            let experience = self
                .state
                .retail_experience
                .get_mut(&id)
                .expect("filtered retail experience must exist");
            if equity.cents() > 0 {
                experience.observe_equity(equity)?;
            }
            let held: BTreeSet<_> = positions.iter().map(|(code, _)| code.clone()).collect();
            for (code, price) in positions {
                experience.observe_position(&code, price, market_minute)?;
            }
            experience.prune_watchlist(&held);
        }
        Ok(())
    }

    /// 分配流通盘给 NPC（按 setup.float_allocation）。float_shares==0 或无 NPC 则跳过。
    ///
    /// 筹码守恒：Σ NPC 持仓 == float_shares（最后一个 NPC/最后一类 拿余量）。玩家不分配（新进场）。
    /// 类间与类内各自按配置采用随机或等比例策略；玩家始终不分配。
    fn seed_float(&mut self) -> Result<(), SessionError> {
        let npc_ids: Vec<AccountId> = self
            .state
            .accounts
            .keys()
            .copied()
            .filter(|id| id.0 != 0)
            .collect();
        if npc_ids.is_empty() {
            return Ok(());
        }
        for spec in self.state.setup.stocks.clone() {
            if spec.float_shares == 0 {
                continue;
            }
            let code = self
                .state
                .stocks
                .id(&spec.code.0)
                .expect("流通盘分配的股票必须已入目录");
            let price = spec.initial_price;
            let allocation = self.state.setup.float_allocation.clone();
            let alloc = self.split_float(spec.float_shares, &npc_ids, &allocation);
            for (id, qty) in alloc {
                if qty > 0 {
                    if let Some(acc) = self.state.accounts.get_mut(&id) {
                        acc.grant_position(code, qty, price)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 按类间与类内配置分配流通盘，整数股数严格守恒。
    ///
    /// 缺类（该种类无 NPC）自动剔除并重归一化（其比例分摊给剩余种类）。最后一类拿整体余量
    /// → 全局精确守恒。f64 仅用于「比例归一」（股数分配，非金额）；最终量 u32。
    fn split_float(
        &mut self,
        float: u32,
        npc_ids: &[AccountId],
        allocation: &FloatAllocation,
    ) -> Vec<(AccountId, u32)> {
        let mut by_kind: [(AccountKind, Vec<AccountId>); 3] = [
            (AccountKind::Retail, Vec::new()),
            (AccountKind::Inst, Vec::new()),
            (AccountKind::Hot, Vec::new()),
        ];
        for id in npc_ids.iter().copied() {
            let kind = self.state.accounts[&id].kind();
            for entry in by_kind.iter_mut() {
                if entry.0 == kind {
                    entry.1.push(id);
                    break;
                }
            }
        }
        let nonempty: Vec<usize> = by_kind
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.1.is_empty())
            .map(|(i, _)| i)
            .collect();

        let kind_budgets = match &allocation.between_kinds {
            BetweenKindDistribution::Random => {
                let slots: Vec<AccountId> = nonempty
                    .iter()
                    .map(|index| AccountId(*index as u64))
                    .collect();
                self.split_random(float, &slots)
                    .into_iter()
                    .map(|(_, qty)| qty)
                    .collect()
            }
            BetweenKindDistribution::Percentage { retail, inst, hot } => {
                let weights = [*retail, *inst, *hot];
                split_by_weights(
                    float,
                    &nonempty
                        .iter()
                        .map(|index| weights[*index])
                        .collect::<Vec<_>>(),
                )
            }
        };

        let mut out: Vec<(AccountId, u32)> = Vec::new();
        for (index, &kind_index) in nonempty.iter().enumerate() {
            let (kind, ids) = &by_kind[kind_index];
            let kind_float = kind_budgets[index];
            let parts = match allocation.within_kind {
                WithinKindDistribution::EqualPercentage => split_equally(kind_float, ids),
                WithinKindDistribution::Random => {
                    let eligible_ids: Vec<AccountId> = if *kind == AccountKind::Retail
                        && ids.len() > 1
                    {
                        let mut selected: Vec<AccountId> = ids
                            .iter()
                            .copied()
                            .filter(|_| self.state.rng.next_f64() < 0.40)
                            .collect();
                        if selected.is_empty() {
                            selected.push(
                                ids[self.state.rng.next_range_u32(0, ids.len() as u32) as usize],
                            );
                        }
                        selected
                    } else {
                        ids.clone()
                    };
                    let tail_exponent = match *kind {
                        AccountKind::Retail => 1.5,
                        AccountKind::Inst => 2.0,
                        AccountKind::Hot => 1.7,
                        AccountKind::Player => 3.0,
                    };
                    self.split_random_with_tail(kind_float, &eligible_ids, tail_exponent)
                }
            };
            out.extend(parts);
        }
        out
    }

    /// 把 float 股随机分给 ids（权重随机归一，最后一个拿余量保 Σ==float）。
    ///
    /// f64 仅用于「权重归一」（股数分配，非金额）；最终量 u32。确定性（种子化 RNG）。
    fn split_random(&mut self, float: u32, ids: &[AccountId]) -> Vec<(AccountId, u32)> {
        self.split_random_with_tail(float, ids, 3.0)
    }

    fn split_random_with_tail(
        &mut self,
        float: u32,
        ids: &[AccountId],
        tail_exponent: f64,
    ) -> Vec<(AccountId, u32)> {
        let n = ids.len();
        if n == 0 || float == 0 {
            return ids.iter().map(|id| (*id, 0u32)).collect();
        }
        // Pareto 尾部让大量小持仓与少量大持仓共存；上限避免一个账户吞掉全部流通盘。
        let weights: Vec<f64> = (0..n)
            .map(|_| {
                (1.0 - self.state.rng.next_f64())
                    .max(1e-9)
                    .powf(-1.0 / tail_exponent)
                    .min(100.0)
            })
            .collect();
        let total_w: f64 = weights.iter().sum();
        let mut out: Vec<(AccountId, u32)> = Vec::with_capacity(n);
        let mut remaining = float;
        for i in 0..n {
            let q = if i == n - 1 {
                remaining // 最后一个 NPC 拿全部余量 → 精确守恒
            } else {
                let raw = (float as f64 * weights[i] / total_w).round() as u32;
                raw.min(remaining)
            };
            out.push((ids[i], q));
            remaining -= q;
        }
        out
    }

    /// 按 `kind` 生成 NPC 账户并注入策略。
    ///
    /// `next_id = accounts.keys().max() + 1`：保证 id 单调递增且不冲突。
    /// 策略参数非法时返回 [`SessionError::Strategy`]；不创建无策略 NPC。
    fn populate_npcs(&mut self, kind: AccountKind) -> Result<(), SessionError> {
        let count = match kind {
            AccountKind::Retail => self.state.setup.npcs.retail_count,
            AccountKind::Inst => self.state.setup.npcs.inst_count,
            AccountKind::Hot => self.state.setup.npcs.hot_count,
            AccountKind::Player => 0,
        };
        let first_id = self
            .state
            .accounts
            .keys()
            .next_back()
            .map_or(1, |id| id.0 + 1);
        let end_id = first_id.checked_add(u64::from(count)).ok_or_else(|| {
            // fault1030: NPC 账户 id 区间 u64 溢出。
            SessionError::InvalidSetup(ValidationFault::bare(1030))
        })?;
        for (ordinal, next_id) in (first_id..end_id).enumerate() {
            let id = AccountId(next_id);
            let initial_cash = sample_npc_cash(
                kind,
                self.state.setup.npcs.retail_cash_median,
                self.state.seed,
                id,
            )?;
            let mut acc = Account::new(id, kind, initial_cash);
            if let Some(s) = StrategyFactory::build_for_market_day_with_ordinal(
                kind,
                &self.state.setup.strategy_params,
                self.state.setup.ticks_per_day,
                u32::try_from(ordinal).map_err(|_| {
                    // fault1031: NPC 序号超出 u32。
                    SessionError::InvalidSetup(ValidationFault::bare(1031))
                })?,
                &mut self.state.rng,
            )? {
                let base_probability = json_canonical_f64(s.base_observation_probability())?;
                if !(base_probability.is_finite()
                    && 0.0 < base_probability
                    && base_probability <= 1.0)
                {
                    return Err(SessionError::Strategy(StrategyError::InvalidParam {
                        param: "base_observation_probability",
                        reason: format!("{base_probability} not in (0,1]"),
                    }));
                }
                let mut strategy_state = crate::strategy::StrategyState::from_strategy(s.as_ref())
                    .map_err(|_error| {
                        // fault1032: 生成的 NPC 策略状态非法。槽0 待 C 批数值化（原文本为内层错误文本）。
                        SessionError::InvalidSetup(ValidationFault::new(1032, vec![0]))
                    })?;
                strategy_state.set_base_observation_probability(base_probability);
                let s = strategy_state.into_strategy().map_err(|_error| {
                    // fault1033: 规范化 NPC 策略状态非法。槽0 待 C 批数值化（原文本为内层错误文本）。
                    SessionError::InvalidSetup(ValidationFault::new(1033, vec![0]))
                })?;
                acc.set_strategy(s);
                // 计划型机构的决策链状态：分析档案 + 信念簿 +
                // 个人信息集 + 关注列表。RNG 纪律（extraction_replay 教训）：
                // 全部使用 seed ^ FNV1a(账户派生标签) 的独立流，绝不用 self.state.rng。
                let profile = acc.strategy().expect("just set").profile();
                if kind == AccountKind::Retail
                    || (kind == AccountKind::Inst
                        && matches!(
                            profile,
                            StrategyProfile::Institution(
                                crate::strategy::InstitutionStyle::DeepValue
                                    | crate::strategy::InstitutionStyle::Growth
                                    | crate::strategy::InstitutionStyle::Balanced
                                    | crate::strategy::InstitutionStyle::Defensive
                                    | crate::strategy::InstitutionStyle::ActiveTrader
                            )
                        ))
                {
                    let mut analysis_rng = SplitMix64::new(decision_chain::derived_stream(
                        self.state.seed,
                        "analysis-profile",
                        id,
                    ));
                    let analysis =
                        crate::strategy::derive_analysis_profile(&profile, id, &mut analysis_rng)
                            .map_err(SessionError::StrategyAnalysis)?;
                    let mut belief_rng = SplitMix64::new(decision_chain::derived_stream(
                        self.state.seed,
                        "belief-assumptions",
                        id,
                    ));
                    let mut belief =
                        crate::strategy::BeliefBook::new(id, profile, analysis, &mut belief_rng);
                    if kind == AccountKind::Inst {
                        let style = acc
                            .strategy()
                            .expect("institution strategy exists")
                            .institution_style()
                            .expect("institution belief strategy has a style");
                        // 个体执行参数使用独立随机流，不改变已有估值假设的六次采样纪律。
                        let mut policy_rng = SplitMix64::new(decision_chain::derived_stream(
                            self.state.seed,
                            "institution-experience-policy",
                            id,
                        ));
                        belief.set_institution_policy(
                            crate::strategy::InstitutionExperiencePolicy::sample(
                                style,
                                &mut policy_rng,
                            ),
                        );
                    }
                    self.state.belief_participants.insert(
                        id,
                        BeliefParticipantState::new(
                            crate::experience::PersonalWatchlist::new(),
                            crate::experience::PersonalPriceMemory::default(),
                            crate::information::NpcInformationState::new(id),
                            belief,
                        ),
                    );
                }
                let attention_seed = self.state.seed
                    ^ next_id.wrapping_mul(0x6A09_E667_F3BC_C908)
                    ^ 0xA77E_7710_D15C_A11E;
                let mut attention_rng = SplitMix64::new(attention_seed);
                let first_candidate_tick = sample_attention_wait(
                    maximum_observation_probability(kind, base_probability),
                    &mut attention_rng,
                ) - 1;
                self.state.npc_attention.insert(
                    id,
                    NpcAttentionState {
                        information_cadence: NpcInformationCadence::for_profile(
                            &acc.strategy().expect("NPC strategy exists").profile(),
                            id,
                        ),
                        next_information_check: crate::CivilInstant::new(
                            self.state.civil_clock.current_date(),
                            0,
                        )
                        .expect("午夜有效"),
                        base_probability,
                        next_attention_candidate_tick: first_candidate_tick,
                        rng_state: attention_rng.state,
                    },
                );
                self.state
                    .attention_scheduler
                    .enqueue(first_candidate_tick, id);
            }
            self.state.accounts.insert(id, acc);
            self.state
                .history_reads
                .insert(id, crate::experience::PersonalHistoryReadLedger::default());
        }
        Ok(())
    }

    /// 股票数量。
    pub fn market_count(&self) -> usize {
        self.state.markets.len()
    }
    /// runtime 驱动器导出：确定性配置面副本（批 8d 纵切）。
    pub fn export_setup_for_runtime(&self) -> SessionSetup {
        self.state.setup.clone()
    }

    /// 账户数量（含玩家）。
    pub fn account_count(&self) -> usize {
        self.state.accounts.len()
    }
    /// 当前每个 NPC 的策略身份档案。诊断可用它归因成交参与度；玩家没有策略，故不在结果中。
    ///
    /// 档案只标识公开的策略种类/风格，不泄露账户现金、库存、成本或策略私有参数。
    pub fn account_strategy_profiles(&self) -> BTreeMap<AccountId, StrategyProfile> {
        self.state
            .accounts
            .iter()
            .filter_map(|(id, account)| {
                account.strategy().map(|strategy| (*id, strategy.profile()))
            })
            .collect()
    }
    /// 只读账户引用。
    pub fn account(&self, id: AccountId) -> Option<&Account> {
        self.state.accounts.get(&id)
    }
    /// 当前 tick（从 0 起，step 后自增）。
    pub fn tick(&self) -> u64 {
        self.state.tick
    }
    /// 当前交易日（0 起，日界自增）。
    pub fn day(&self) -> u32 {
        self.state.day
    }

    /// 当前交易阶段。`auction_ticks` 表示 09:15–09:30 的整段开盘时间；
    /// 前 2/3 接受集合竞价申报，后 1/3 为不接受申报的盘前窗口。
    pub fn phase(&self) -> TradingPhase {
        let day_tick = self.state.tick % self.state.setup.ticks_per_day;
        if day_tick < self.auction_entry_ticks() {
            TradingPhase::CallAuction
        } else if day_tick < self.state.setup.auction_ticks {
            TradingPhase::PreOpen
        } else if day_tick >= self.closing_auction_start_tick() {
            TradingPhase::ClosingAuction
        } else {
            TradingPhase::Continuous
        }
    }

    /// 把可配置的 15 分钟开盘窗口按 10:5 映射为申报期和盘前期。
    fn auction_entry_ticks(&self) -> u64 {
        self.state.setup.auction_ticks - self.state.setup.auction_ticks / 3
    }

    fn closing_auction_start_tick(&self) -> u64 {
        self.state.setup.ticks_per_day - self.state.setup.closing_auction_ticks
    }
    /// 最新事件 seq。
    pub fn seq(&self) -> u64 {
        self.state.seq
    }
    /// 自增并返回下一个事件 seq（单调）。
    fn next_seq(&mut self) -> u64 {
        self.state.seq += 1;
        self.state.seq
    }

    /// 当前自然日（冻结日历与双时钟：与交易日计数 `day` 分离；休市日推进只动这里）。
    pub fn civil_date(&self) -> crate::calendar::CivilDate {
        self.state.civil_clock.current_date()
    }

    /// Returns one host-safe page of reports visible at the current civil instant.
    pub fn query_public_reports(
        &self,
        query: &crate::company::PublicReportQuery,
    ) -> Result<crate::company::PublicReportPage, SessionError> {
        let as_of = crate::calendar::CivilInstant::new(self.civil_date(), 0)
            .map_err(crate::calendar::CalendarError::from)?;
        self.state
            .library
            .query_public_reports(query, as_of)
            .map_err(|error| SessionError::Information(Box::new(error)))
    }

    /// Returns one host-safe report visible at the current civil instant.
    pub fn public_report_by_id(
        &self,
        id: String,
    ) -> Result<crate::company::PublicReportSummary, SessionError> {
        let as_of = crate::calendar::CivilInstant::new(self.civil_date(), 0)
            .map_err(crate::calendar::CalendarError::from)?;
        self.state
            .library
            .public_report_by_id(id, as_of)
            .map_err(|error| SessionError::Information(Box::new(error)))
    }

    /// 自然日经营时钟只读访问（诊断/测试）。
    pub fn civil_clock(&self) -> &CivilClock {
        &self.state.civil_clock
    }

    /// 自然日经营时钟可变访问：注册到期业务、显式日期日结等高级用法。
    /// **绕过会话同步守卫**；宿主常规循环必须走 [`Self::end_civil_day`]。
    pub fn civil_clock_mut(&mut self) -> &mut CivilClock {
        &mut self.state.civil_clock
    }

    /// 自然日日结（冻结日历与双时钟）：当日经营终局窗口 →（月/年末封账）→ 18:00 披露 → 前进次日。
    ///
    /// 交易日必须先完成当日会话（`ticks_per_day` 个 step，`day` 已自增到位）；
    /// 休市日直接调用。先全量验证（会话同步 + 时钟规则）后原子应用，任何
    /// `Err` 不改变会话与时钟状态。step 的 tick 循环完全不变——日结是新接在
    /// 收盘之后的自然日权威推进，不是 tick 循环的一部分。
    ///
    /// 会话装配与执行接线（经营与信息披露顺序）：时钟日结（自然日推进 + 18:00 相位）→
    /// 经营终局（`CompanyOperations::advance_civil_day` 在单一 authority 调度器内
    /// 恰好一次派发当日公司到期）→ 月/年末封账（`close_accounting_periods`）→
    /// 披露派发（`DisclosureDispatch::run_day_end`，公告先于定期报告）。
    pub fn end_civil_day(&mut self) -> Result<CivilDayEndReport, SessionError> {
        self.with_prepared_civil_day(|_, report| Ok(report))
    }

    /// 自然日与 market tick 一样先在同版 candidate 完成全部 publication callback，
    /// 成功后一次安装；失败不得先触发 observer 再靠 checkpoint 回滚。
    pub(in crate::session) fn with_prepared_civil_day<P>(
        &mut self,
        project: impl FnOnce(&GameSession, CivilDayEndReport) -> Result<P, SessionError>,
    ) -> Result<P, SessionError> {
        self.require_healthy()?;
        let expected_sessions = self
            .state
            .civil_clock
            .completed_trading_sessions_expected()?;
        if self.state.day != expected_sessions {
            return Err(SessionError::CivilClock(
                CivilClockError::MarketSessionOutOfSync {
                    date: self.state.civil_clock.current_date(),
                    completed_sessions: self.state.day,
                    expected_sessions,
                },
            ));
        }
        let mut candidate = self.clone_for_tick_shadow().map_err(|_error| {
            // fault1034：自然日日终 candidate 捕获失败，保持数值错误契约。
            SessionError::InvalidSave(ValidationFault::new(1034, vec![0]))
        })?;
        let report = candidate.end_civil_day_after_session_check()?;
        let instant = report.disclosure_instant;
        let publication = project(&candidate, report)?;
        self.commit_tick_shadow(candidate);
        for observer in self.state.civil_clock.disclosure_observers() {
            observer(instant);
        }
        Ok(publication)
    }

    fn end_civil_day_after_session_check(&mut self) -> Result<CivilDayEndReport, SessionError> {
        // Session 合并公司 due、每日经营与其他 civil 事件的下一工作自然日；
        // 各 authority 仍只持有自己的队列/日历，不复制或镜像逐项公司待办。
        let next_company_day = self.state.operations.next_expected_date();
        let company_work_date = match self.state.operations.next_due_date() {
            Some(due_date) => due_date.min(next_company_day),
            None => next_company_day,
        };
        let next_work_date = company_work_date.min(self.state.civil_clock.next_civil_event_date());
        // CivilClock 对合并时点执行重复/回拨/跳日/上界校验；过期公司 due
        // 因此不能被当日推进静默跨过，且不产生新的运行时 String 错误载体。
        let mut report = self.state.civil_clock.end_day(next_work_date)?;
        // 公司经营日（单一 authority：OperatingScheduler 在内部恰好一次派发
        // 当日到期；跳日/次序守卫由 next_expected 序列与 (due_date, id) 排序承担）。
        // next_due 为 None 或未来日期不改变当日经营/封账/披露的执行。
        std::sync::Arc::make_mut(&mut self.state.operations)
            .advance_civil_day(report.settled_date)?;
        // 日结新增待办仍须落在冻结日历适用范围内（日结新增边界）。
        for due in self.state.operations.scheduler().pending() {
            self.state.civil_clock.validate_due_horizon(due.due_date)?;
        }
        std::sync::Arc::make_mut(&mut self.state.operations)
            .close_accounting_periods(
                report.settled_date,
                &self.state.groups,
                std::sync::Arc::make_mut(&mut self.state.closing),
            )
            .map_err(|source| match source {
                crate::company::operations::CompanyCloseError::Date(date) => {
                    SessionError::CivilClock(CivilClockError::CivilDate(date))
                }
                crate::company::operations::CompanyCloseError::Closing(closing) => {
                    SessionError::Closing(*closing)
                }
                crate::company::operations::CompanyCloseError::Information(information) => {
                    SessionError::Information(information)
                }
                crate::company::operations::CompanyCloseError::Fault(fault) => {
                    SessionError::InvalidSetup(fault)
                }
            })?;
        let disclosures = self
            .state
            .disclosures
            .run_day_end(DayEndDisclosureCtx {
                report: &report,
                ops: &self.state.operations,
                groups: &self.state.groups,
                closing: std::sync::Arc::make_mut(&mut self.state.closing),
                library: std::sync::Arc::make_mut(&mut self.state.library),
            })
            .map_err(SessionError::Disclosure)?;
        self.record_civil_day_events(&mut report, disclosures)?;
        self.deliver_public_information(report.disclosure_instant)?;
        self.prune_all_personal_memories();
        Ok(report)
    }

    fn next_civil_seq(&mut self) -> Result<u64, SessionError> {
        self.state.seq = self
            .state
            .seq
            .checked_add(1)
            .filter(|seq| *seq <= crate::orderbook::js_safe_u64::MAX)
            .ok_or_else(|| StepFatal::InvariantViolation {
                location: "GameSession::record_civil_day_events".to_owned(),
                description: "civil event seq 超出可发布整数范围".to_owned(),
            })?;
        Ok(self.state.seq)
    }

    fn record_civil_day_events(
        &mut self,
        report: &mut CivilDayEndReport,
        disclosures: DayEndDisclosures,
    ) -> Result<(), SessionError> {
        for publication_id in disclosures.announcements_published {
            let (company, published_at) = self
                .state
                .library
                .announcement(publication_id, report.disclosure_instant)
                .map(|announcement| (announcement.company.clone(), announcement.published_at))
                .map_err(|error| SessionError::Information(Box::new(error)))?;
            report.events.push(Event::CompanyDisclosurePublished {
                seq: self.next_civil_seq()?,
                publication_id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Announcement,
            });
        }
        for publication_id in disclosures.reports_published {
            let (company, published_at, report_revision) = self
                .state
                .library
                .report(publication_id, report.disclosure_instant)
                .map(|published| {
                    (
                        published.company.clone(),
                        published.published_at,
                        published.reports.version.sequence,
                    )
                })
                .map_err(|error| SessionError::Information(Box::new(error)))?;
            report.events.push(Event::CompanyDisclosurePublished {
                seq: self.next_civil_seq()?,
                publication_id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Report { report_revision },
            });
        }
        report.events.push(Event::CivilDateAdvanced {
            seq: self.next_civil_seq()?,
            settled_date: report.settled_date,
            next_date: report.next_date,
            next_status: report.next_status.clone(),
        });
        Ok(())
    }

    /// 读取上一 tick 的散户目标仓位诊断样本。
    ///
    /// 该切片在下一次 [`Self::step`] 开始时被替换；调用方不得把它当作存档、委托或成交。
    pub fn last_retail_decisions(&self) -> &[RetailDecisionTrace] {
        &self.state.last_retail_decisions
    }

    /// 读取上一 tick 的散户订单生命周期诊断事件；不属于存档或撮合状态。
    pub fn last_retail_order_events(&self) -> &[RetailOrderDiagnosticEvent] {
        &self.state.last_retail_order_events
    }

    pub fn npc_decision_diagnostics(
        &self,
        account: AccountId,
    ) -> crate::diagnostics::NpcDecisionDiagnostics {
        #[cfg(not(feature = "simulation-diagnostics"))]
        let _ = account;
        #[cfg(feature = "simulation-diagnostics")]
        {
            self.npc_decision_trace(account)
                .map(|records| crate::diagnostics::NpcDecisionDiagnostics::Supported { records })
                .unwrap_or(crate::diagnostics::NpcDecisionDiagnostics::Supported {
                    records: Vec::new(),
                })
        }
        #[cfg(not(feature = "simulation-diagnostics"))]
        crate::diagnostics::NpcDecisionDiagnostics::Unsupported
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub fn npc_decision_trace(
        &self,
        account: AccountId,
    ) -> Option<Vec<crate::diagnostics::NpcDecisionTraceRecord>> {
        self.state
            .npc_decision_traces
            .records(account)
            .map(|records| records.iter().cloned().collect())
    }

    fn record_retail_order_canceled(
        &mut self,
        account: AccountId,
        code: StockId,
        order_id: OrderId,
        remaining_qty: u32,
    ) {
        if self.state.retail_experience.contains_key(&account) {
            self.state
                .last_retail_order_events
                .push(RetailOrderDiagnosticEvent::Canceled {
                    account,
                    code,
                    order_id,
                    remaining_qty,
                });
        }
    }

    fn record_retail_intent_rejections(&mut self, account: AccountId, events: &[Event]) {
        if !self.state.retail_experience.contains_key(&account) {
            return;
        }
        for event in events {
            if let Event::IntentRejected {
                account: rejected_account,
                code,
                reason,
                ..
            } = event
            {
                if *rejected_account == account {
                    self.state.last_retail_order_events.push(
                        RetailOrderDiagnosticEvent::Rejected {
                            account,
                            code: code.clone(),
                            reason: reason.clone(),
                        },
                    );
                }
            }
        }
    }

    fn reserved_cash_for_account(&self, account: AccountId) -> Result<Money, MoneyError> {
        self.state
            .markets
            .values()
            .flat_map(|market| market.resting_orders_for(account))
            .try_fold(Money::ZERO, |total, order| {
                let required = live_cash_reservation(
                    &self.state.setup.config,
                    order.side,
                    order.price,
                    order.qty,
                    order.filled_value,
                )?;
                total.add(required)
            })
    }

    /// 股票目录（ADR 0041）只读访问：id ↔ 代码/显示名翻译的权威来源。
    pub fn stock_catalog(&self) -> &StockCatalog {
        &self.state.stocks
    }

    /// 边界便捷：按代码查 id。未知代码返回 None，由调用方显式处理。
    pub fn stock_id(&self, code: &str) -> Option<StockId> {
        self.state.stocks.id(code)
    }

    fn register_npc_order_lifecycle_at_quote(
        &mut self,
        account: AccountId,
        code: &StockId,
        order: &Order,
        last_price: Money,
        best_bid: Option<Money>,
        best_ask: Option<Money>,
    ) {
        if self.phase() != TradingPhase::Continuous
            || self
                .state
                .accounts
                .get(&account)
                .is_none_or(|candidate| candidate.kind() == AccountKind::Player)
            || self.is_active_parent_child(account, code, order.id)
        {
            return;
        }
        pipeline::quote_expiry::NpcOrderLifecycleBook::new(&mut self.state.npc_order_lifecycles)
            .ensure_order_absent(order.id);
        let placed_market_minute = self.current_market_minute();
        let lifetime_minutes = self.npc_quote_lifetime_minutes_at_quote(
            account, code, order, last_price, best_bid, best_ask,
        );
        let day_end = (u64::from(self.state.day) + 1)
            .checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .expect("session day plus one fits market-minute range");
        let expires_market_minute = placed_market_minute
            .checked_add(lifetime_minutes)
            .expect("NPC quote lifetime fits market-minute range")
            .min(day_end);
        pipeline::quote_expiry::NpcOrderLifecycleBook::new(&mut self.state.npc_order_lifecycles)
            .append_prepared(NpcOrderLifecycle {
                account,
                code: code.clone(),
                order_id: order.id,
                placed_market_minute,
                expires_market_minute,
            });
    }

    #[cfg(test)]
    fn npc_quote_lifetime_minutes(&self, account: AccountId, code: &StockId, order: &Order) -> u64 {
        let market = self
            .state
            .markets
            .get(code)
            .expect("NPC quote requires a known market");
        self.npc_quote_lifetime_minutes_at_quote(
            account,
            code,
            order,
            market.last_price(),
            market.best_bid(),
            market.best_ask(),
        )
    }

    /// 用接受时盘口生成分散的、日内有效的 NPC 撤单时间。这里的期限是行为模型，
    /// 不替代交易所的日内有效委托规则；数值被写入存档，恢复后不会再次抽样。
    fn npc_quote_lifetime_minutes_at_quote(
        &self,
        account: AccountId,
        code: &StockId,
        order: &Order,
        last_price: Money,
        best_bid: Option<Money>,
        best_ask: Option<Money>,
    ) -> u64 {
        let kind = self
            .state
            .accounts
            .get(&account)
            .expect("lifecycle registration only accepts an existing account")
            .kind();
        let base = match kind {
            AccountKind::Retail => 18_u64,
            AccountKind::Inst => 36_u64,
            AccountKind::Hot => 8_u64,
            AccountKind::Player => panic!("player orders must not receive NPC quote lifecycles"),
        };
        let code_str = self
            .state
            .stocks
            .code(*code)
            .expect("lifecycle registration only accepts a cataloged stock");
        let tick_cents = self
            .state
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code.0 == *code_str)
            .expect("lifecycle registration only accepts a configured stock")
            .tick
            .cents();
        let spread_ticks = match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => (ask.cents() - bid.cents()).max(0) / tick_cents,
            _ => 1,
        };
        let quote_distance_ticks = (order.price.cents() - last_price.cents()).unsigned_abs()
            / u64::try_from(tick_cents).expect("market tick is positive");
        let volatility_ticks = self
            .state
            .price_history
            .get(code)
            .map(|history| {
                let (low, high) = history
                    .iter()
                    .fold((i64::MAX, i64::MIN), |(low, high), price| {
                        (low.min(price.cents()), high.max(price.cents()))
                    });
                if low == i64::MAX {
                    0
                } else {
                    (high - low) / tick_cents
                }
            })
            .unwrap_or(0);
        // 抖动种子按代码字符串派生（保持既有数值路径不变，ADR 0041）。
        let jitter_code = StockCode(
            self.state
                .stocks
                .code(*code)
                .expect("lifecycle registration only accepts a cataloged stock")
                .to_string(),
        );
        let deterministic_jitter = self.state.seed
            ^ account.0.rotate_left(17)
            ^ order.id.0.rotate_left(31)
            ^ stock_code_hash(&jitter_code);
        let jitter = deterministic_jitter % (base / 2 + 1);
        base.saturating_add(u64::try_from(spread_ticks).unwrap_or(u64::MAX).min(8))
            .saturating_add(quote_distance_ticks.min(12))
            .saturating_add(jitter)
            .saturating_sub(
                u64::try_from(volatility_ticks)
                    .unwrap_or(u64::MAX)
                    .min(base / 2),
            )
            .clamp(1, 60)
    }

    fn is_active_parent_child(
        &self,
        account: AccountId,
        code: &StockId,
        order_id: OrderId,
    ) -> bool {
        self.state
            .plans
            .active_plan(account, code)
            .is_some_and(|plan| plan.active_child_order_id() == Some(order_id))
    }

    fn remove_npc_order_lifecycle(
        &mut self,
        account: AccountId,
        code: &StockId,
        order_id: OrderId,
    ) {
        pipeline::quote_expiry::NpcOrderLifecycleBook::new(&mut self.state.npc_order_lifecycles)
            .remove(account, code, order_id);
    }

    ///
    /// 玩家账户不存在 → [`SessionError::UnknownPlayer`]（致命错误显式返回，铁律二）。
    pub fn enqueue_player_intent(
        &mut self,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SessionError> {
        if let Some(binding) = &self.ingress {
            return binding.source.enqueue_player_intent(player_id, intent);
        }
        let account = self
            .state
            .accounts
            .get(&player_id)
            .ok_or(SessionError::UnknownPlayer(player_id))?;
        if account.kind() != AccountKind::Player {
            return Err(SessionError::NotPlayer(player_id));
        }
        let received = self
            .state
            .ingress_receipt_cursors
            .receive(player_id, intent)?;
        self.state.pending_player.push(received);
        Ok(())
    }

    /// 生成存档（精确到交易日）。
    /// 在 DayBoundary 后调用 → snapshot 含 end_of_day 后的状态（last_close 已更新）。
    fn save_snapshot_projection(&self) -> SaveSnapshot {
        SaveSnapshot {
            seq: self.state.seq,
            tick: self.state.tick,
            markets: self
                .state
                .markets
                .iter()
                .map(|(code, market)| {
                    (
                        code.clone(),
                        SaveMarketSnap {
                            last_price: market.last_price(),
                            last_close: market.last_close(),
                        },
                    )
                })
                .collect(),
            accounts: self
                .state
                .accounts
                .iter()
                .map(|(id, account)| {
                    (
                        *id,
                        SaveAccountSnap {
                            cash: account.cash(),
                            positions: account
                                .positions()
                                .iter()
                                .map(|(code, position)| {
                                    (
                                        code.clone(),
                                        PositionSnap {
                                            qty: position.qty(),
                                            t1_locked: position.t1_locked(),
                                            invested_cents: position.invested_cents(),
                                            recovered_cents: position.recovered_cents(),
                                        },
                                    )
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            daily_candles: self
                .state
                .candle_book
                .histories()
                .iter()
                .map(|(code, history)| (code.clone(), history.iter().cloned().collect()))
                .collect(),
            active_daily_candles: self.state.candle_book.active().clone(),
        }
    }

    fn save_projection(&self, runtime_state: SavedRuntimeState) -> SaveSlot {
        SaveSlot {
            runtime_state,
            setup: self.state.setup.clone(),
            seed: self.state.seed,
            snapshot: self.save_snapshot_projection(),
            resting_orders: self
                .state
                .markets
                .iter()
                .map(|(code, market)| (code.clone(), market.resting_orders()))
                .collect(),
            book_next_sequences: self
                .state
                .markets
                .iter()
                .map(|(code, market)| (code.clone(), market.book_next_sequence()))
                .collect(),
            filled_orders: self
                .state
                .markets
                .iter()
                .map(|(code, market)| {
                    (
                        code.clone(),
                        market
                            .filled_orders()
                            .into_iter()
                            .map(|(id, owner)| FilledOrderSnap { id, owner })
                            .collect(),
                    )
                })
                .collect(),
            price_history: self
                .state
                .price_history
                .iter()
                .map(|(code, prices)| (code.clone(), prices.iter().copied().collect()))
                .collect(),
            market_minute_closes: self.state.market_minute_closes.clone(),
            rng_state: self.state.rng.state,
            npc_attention: self.state.npc_attention.to_map(),
            retail_experience: self.state.retail_experience.to_map(),
            npc_order_lifecycles: self.state.npc_order_lifecycles.clone(),
            pending_player: self.state.pending_player.clone(),
            ingress_receipt_cursors: self.state.ingress_receipt_cursors.clone(),
            next_order_id: self.state.next_order_id,
            civil_clock: self.state.civil_clock.save(),
            // 权威状态连续性（完整存档）：公司域与个体决策链权威状态全量入档。
            company_operations: self.state.operations.as_ref().clone(),
            groups: self.state.groups.clone(),
            closing_registry: self.state.closing.as_ref().clone(),
            public_library: self.state.library.as_ref().clone(),
            disclosures: self.state.disclosures.clone(),
            plans: self.state.plans.clone(),
            urgency_policy: self.state.urgency_policy,
            information_states: self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (*id, participant.information().clone()))
                .collect(),
            belief_books: self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (*id, participant.belief().clone()))
                .collect(),
            watchlists: self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (*id, participant.watchlist().clone()))
                .collect(),
            price_memories: self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (*id, participant.price_memory().clone()))
                .collect(),
            history_reads: self.state.history_reads.to_map(),
        }
    }

    /// 从存档恢复权威账户、市场、日 K 与两种竞价阶段的未成交委托。
    ///
    /// 流程：
    /// 1. new(setup, seed) → 新建 session（含初始持仓分配）
    /// 2. 清空所有账户持仓 → 用快照精确覆盖（cash + positions invested/recovered/t1_locked）
    /// 3. 覆盖每只股票的 last_price/last_close
    /// 4. 重建连续竞价订单簿并校验深度；恢复 tick/day/seq 与集合竞价队列
    ///
    /// 不保留：前端派生的分时采样。策略价格窗口和 RNG 状态会被精确恢复。
    pub fn restore(save: &SaveSlot) -> Result<GameSession, SessionError> {
        Self::restore_with_compute(save, crate::compute::ComputeContext::shared_default()?)
    }

    /// 恢复路径与宿主计算资源显式绑定；restore 继续使用 direct-engine 默认上下文。
    pub fn restore_with_compute(
        save: &SaveSlot,
        compute: std::sync::Arc<crate::compute::ComputeContext>,
    ) -> Result<GameSession, SessionError> {
        // T3 批四·staged 决策槽防串扰（同 new_with_compute）。
        #[cfg(test)]
        crate::session::pipeline::staged_decision::clear();
        save.urgency_policy.validate().map_err(|_error| {
            // fault1037: 存档 urgency_policy 校验失败。槽0 待 C 批数值化（原文本为内层校验错误文本）。
            SessionError::InvalidSave(ValidationFault::new(1037, vec![0]))
        })?;
        validate_save_slot(save)?;
        let mut sess = GameSession::new_with_compute(save.setup.clone(), save.seed, compute)?;
        sess.fresh_initial_allocation = false;

        // 清空初始持仓分配 → 用快照精确覆盖
        for acc in sess.state.accounts.values_mut() {
            acc.restore_balances(acc.cash(), BTreeMap::new());
        }

        // 恢复账户状态（cash + positions 精确值）
        for (id, snap_acc) in &save.snapshot.accounts {
            let acc = sess
                .fixture_account_mut(id)
                .expect("validated save account set exactly matches setup");
            acc.restore_balances(
                snap_acc.cash,
                snap_acc
                    .positions
                    .iter()
                    .map(|(code, pos)| {
                        (
                            code.clone(),
                            crate::account::Position::from_restored_parts(
                                pos.qty,
                                pos.t1_locked,
                                pos.invested_cents,
                                pos.recovered_cents,
                            ),
                        )
                    })
                    .collect(),
            );
        }

        // 恢复市场状态（last_price/last_close）
        for (code, snap_mkt) in &save.snapshot.markets {
            let market = sess
                .state
                .markets
                .get_mut(code)
                .expect("validated save market set exactly matches setup");
            market.restore_prices(snap_mkt.last_price, snap_mkt.last_close);
        }

        sess.state.tick = save.snapshot.tick;
        let restored_mode = pipeline::phase::PhasePolicy::capture(&sess).matching_mode();
        for (code, market) in &mut sess.state.markets {
            market.set_matching_mode(restored_mode).map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(1038, vec![u64::from(code.0), 0]))
            })?;
        }
        // 保留原始时间排序键与历史游标；空簿也可能已有已撤/已成交委托。
        // 在私有订单簿内验证，不允许恢复过程产生新的成交。
        for (code, market) in &mut sess.state.markets {
            let orders = save
                .resting_orders
                .get(code)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            market
                .restore_resting_orders(orders, save.book_next_sequences[code])
                .map_err(|_error| {
                    // fault1038: 恢复连续竞价挂单失败。ctx[0] = StockId；
                    // 槽1 待 C 批数值化（原文本为内层错误文本）。
                    SessionError::InvalidSave(ValidationFault::new(
                        1038,
                        vec![u64::from(code.0), 0],
                    ))
                })?;
        }
        for (code, filled) in &save.filled_orders {
            sess.state
                .markets
                .get_mut(code)
                .expect("validated filled-order market must exist")
                .restore_filled_orders(filled.iter().map(|order| (order.id, order.owner)))
                .map_err(|_error| {
                    // fault1039: 恢复已成交委托记录失败。ctx[0] = StockId；
                    // 槽1 待 C 批数值化（原文本为内层错误文本）。
                    SessionError::InvalidSave(ValidationFault::new(
                        1039,
                        vec![u64::from(code.0), 0],
                    ))
                })?;
        }
        // 恢复进度
        sess.state.tick = save.snapshot.tick;
        sess.state.day =
            u32::try_from(save.snapshot.tick / save.setup.ticks_per_day).map_err(|_| {
                // fault1040: 存档交易日序号超出 u32。
                SessionError::InvalidSave(ValidationFault::bare(1040))
            })?;
        sess.state.seq = save.snapshot.seq;
        // 恢复自然日时钟；全量校验其自洽性，并使用随存档冻结的政策。
        sess.state.civil_clock = CivilClock::from_parts(
            save.setup.start_date,
            &save.civil_clock,
            session_calendar_exchange(save.setup.stocks[0].exchange),
        )?;
        validate_saved_order_state(&sess, save)?;
        sess.state.next_order_id = save.next_order_id;

        // 当前存档完整覆盖所有影响后续演进的确定性状态。
        sess.state.candle_book.replace_histories(
            save.snapshot
                .daily_candles
                .iter()
                .map(|(code, candles)| (code.clone(), candles.clone().into()))
                .collect(),
        );
        sess.state
            .candle_book
            .replace_active(save.snapshot.active_daily_candles.clone());
        sess.state.price_history = save
            .price_history
            .iter()
            .map(|(code, prices)| (code.clone(), prices.iter().copied().collect()))
            .collect();
        sess.state.market_minute_closes = save.market_minute_closes.clone();
        sess.state.rng.state = save.rng_state;
        let mut restored_attention = BTreeMap::new();
        for (id, saved_state) in &save.npc_attention {
            let reconstructed = sess
                .state
                .npc_attention
                .get(id)
                .expect("validated attention account must exist after reconstruction");
            let probability_scale = reconstructed
                .base_probability
                .abs()
                .max(saved_state.base_probability.abs());
            let differs_beyond_json_roundtrip =
                (reconstructed.base_probability - saved_state.base_probability).abs()
                    > probability_scale * f64::EPSILON * 4.0;
            if differs_beyond_json_roundtrip {
                // fault1041: 恢复的 NPC 注意力基础概率与确定性重建值不符（超出 JSON 往返容差）。
                // ctx[0] = AccountId；ctx[1] = 存档值 f64 位模式，ctx[2] = 重建值 f64 位模式
                // （f64 以 IEEE-754 位模式入槽，C 批可改定点点表示）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    1041,
                    vec![
                        id.0,
                        saved_state.base_probability.to_bits(),
                        reconstructed.base_probability.to_bits(),
                    ],
                )));
            }
            // 基础概率来自账户的确定性行为档案，不是随时间演进的状态。JSON 数字
            // 边界可能造成 f64 的末位变化；继续使用重建值可使存档前后保持完全重放。
            restored_attention.insert(
                *id,
                NpcAttentionState {
                    information_cadence: saved_state.information_cadence,
                    next_information_check: saved_state.next_information_check,
                    base_probability: reconstructed.base_probability,
                    next_attention_candidate_tick: saved_state.next_attention_candidate_tick,
                    rng_state: saved_state.rng_state,
                },
            );
        }
        sess.state.npc_attention = restored_attention.into();
        sess.state.attention_scheduler = sess
            .state
            .npc_attention
            .iter()
            .map(|(id, state)| (state.next_attention_candidate_tick, *id))
            .collect();
        sess.state.retail_experience = save.retail_experience.clone().into();
        sess.state.npc_order_lifecycles = save.npc_order_lifecycles.clone();
        sess.state.pending_player = save.pending_player.clone();
        sess.state.ingress_receipt_cursors = save.ingress_receipt_cursors.clone();
        // 诊断缓存不属于存档事实，恢复后由实际 tick 重新生成。
        sess.state.last_retail_decisions.clear();

        // 权威状态连续性（完整存档）：公司域与个体决策链权威状态直接从档恢复——不再前史
        // 重放、不再复位信念/计划/信息集、不再剥离 linked_plan_id。new() 重建
        // 的 prehistory/时钟接线是确定性产物，被下列赋值整体覆盖。
        let expected_issuers: BTreeSet<&crate::company::CompanyId> =
            sess.state.operations.companies.keys().collect();
        let saved_issuers: BTreeSet<&crate::company::CompanyId> =
            save.company_operations.companies.keys().collect();
        if expected_issuers != saved_issuers {
            // fault1043: 存档公司集合与 setup 重建的发行人集合不一致。
            // 槽0/槽1 待 C 批数值化（原文本为两个 CompanyId 集合的 Debug 文本——公司域字符串身份）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                1043,
                vec![0, 0],
            )));
        }
        for (id, company) in &save.company_operations.companies {
            if company.spec() != sess.state.operations.companies[id].spec() {
                // fault1044: 存档公司规格与 setup 重建不一致。
                // 槽0 待 C 批数值化（原文本为公司 id "{id:?}"——公司域字符串身份）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    1044,
                    vec![0],
                )));
            }
        }
        // 个体状态账户集合精确匹配确定性重建（populate_npcs 按 seed+ordinal
        // 重建信念机构集合）：缺失任一账户的个人状态 = 不完整存档。
        let expected_belief_accounts: BTreeSet<AccountId> =
            sess.state.belief_participants.keys().copied().collect();
        let saved_belief_accounts: BTreeSet<AccountId> =
            save.belief_books.keys().copied().collect();
        if expected_belief_accounts != saved_belief_accounts {
            // fault1045: 存档个人状态账户集合与确定性重建集合不一致。
            // 槽0/槽1 待 C 批数值化（原文本为两个 AccountId 集合的 Debug 文本——集合差异无数值槽）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                1045,
                vec![0, 0],
            )));
        }
        sess.state.operations = std::sync::Arc::new(save.company_operations.clone());
        sess.state.groups = save.groups.clone();
        sess.state.closing = std::sync::Arc::new(save.closing_registry.clone());
        sess.state.library = std::sync::Arc::new(save.public_library.clone());
        sess.state.disclosures = save.disclosures.clone();
        sess.state.plans = save.plans.clone();
        // 执行腿在途子单的剩余数量从真实订单簿/竞价队列重建（唯一 owner 的
        // restore 路径；旧 `parent_orders` 镜像已删除）。
        let active_plan_ids = sess.state.plans.active_plan_ids();
        for plan_id in active_plan_ids {
            let (account, code, direction, active_id) = {
                let plan = sess
                    .state
                    .plans
                    .plan(plan_id)
                    .expect("active plan ids resolve");
                match plan.active_child_order_id() {
                    Some(active_id) => (
                        plan.account(),
                        plan.code().clone(),
                        plan.direction(),
                        active_id,
                    ),
                    None => continue,
                }
            };
            let active_quantities = save
                .resting_orders
                .get(&code)
                .into_iter()
                .flatten()
                .filter(|order| {
                    order.id == active_id && order.owner == account && order.side == direction
                })
                .map(|order| order.qty)
                .collect::<Vec<_>>();
            let [remaining_qty] = active_quantities.as_slice() else {
                // fault1042: 计划在途子委托无法唯一重建（竞价/连续簿命中数 ≠ 1）。
                // ctx[0] = AccountId，ctx[1] = StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    1042,
                    vec![account.0, u64::from(code.0)],
                )));
            };
            // 以真实订单余量做 checked exposure 校验：恢复不得用簿上余量盖过
            // 计划份额目标（旧 fault2173 的语义迁移到唯一 owner）。
            let filled = sess
                .state
                .plans
                .plan(plan_id)
                .expect("active plan ids resolve")
                .filled_qty();
            let exposure = u64::from(filled)
                .checked_add(u64::from(*remaining_qty))
                .ok_or_else(|| {
                    // fault2185：执行腿在途风险敞口溢出或超目标；ctx[0]=AccountId，ctx[1]=StockId。
                    SessionError::InvalidSave(ValidationFault::new(
                        2185,
                        vec![account.0, u64::from(code.0)],
                    ))
                })?;
            if let crate::plans::PlanTarget::ShareCount(target) = sess
                .state
                .plans
                .plan(plan_id)
                .expect("active plan ids resolve")
                .target()
            {
                if exposure > u64::from(target) {
                    // fault2185：执行腿在途风险敞口溢出或超目标；ctx[0]=AccountId，ctx[1]=StockId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2185,
                        vec![account.0, u64::from(code.0)],
                    )));
                }
            }
            sess.state
                .plans
                .restore_child_remaining_qty(plan_id, Some(*remaining_qty))
                .map_err(|_error| {
                    // fault1042：执行腿与真实订单簿不一致。
                    SessionError::InvalidSave(ValidationFault::new(
                        1042,
                        vec![account.0, u64::from(code.0)],
                    ))
                })?;
        }
        sess.state.urgency_policy = save.urgency_policy;
        sess.state.belief_participants = save
            .belief_books
            .iter()
            .map(|(id, belief)| {
                (
                    *id,
                    BeliefParticipantState::new(
                        save.watchlists[id].clone(),
                        save.price_memories[id].clone(),
                        save.information_states[id].clone(),
                        belief.clone(),
                    ),
                )
            })
            .collect();
        sess.state.history_reads = save.history_reads.clone().into_iter().collect();
        sess.reconcile_institutional_holdings()?;
        // 与 new() 相同的进程内接线（观察者 hook 不入档，恢复后重装）。
        sess.state.disclosures.install(&mut sess.state.civil_clock);

        // 最后原子替换权威运行状态；账户、市场、订单、ID/seq 与 RNG 校验均已完成。
        // 计划/信念/信息域均已恢复，runtime 可以对完整 live-order 域做交叉校验。
        persistence::restore_runtime_state(&mut sess, &save.runtime_state)?;

        #[cfg(feature = "simulation-diagnostics")]
        sess.causal_record(crate::diagnostics::causal::CausalFactKind::ObservationRestart);
        Ok(sess)
    }
}

/// 将有限浮点数投影为 JSON 跨语言边界实际保存和读取的值。
///
/// `NpcAttentionState` 会进入存档；若运行时直接保留原始计算结果，JSON 数字格式的
/// 舍入可能让恢复后静态概率的最低有效位不同，从而破坏后续确定性重放。
fn json_canonical_f64(value: f64) -> Result<f64, SessionError> {
    let encoded = serde_json::to_string(&value).map_err(|_error| {
        // fault1046: f64 JSON 编码失败。槽0 待 C 批数值化（原文本为内层 serde 错误文本）。
        SessionError::InvalidSetup(ValidationFault::new(1046, vec![0]))
    })?;
    serde_json::from_str(&encoded).map_err(|_error| {
        // fault1047: f64 JSON 解码失败。槽0 待 C 批数值化（原文本为内层 serde 错误文本）。
        SessionError::InvalidSetup(ValidationFault::new(1047, vec![0]))
    })
}

/// A 股日内成交通常在开盘和尾盘更活跃。集合竞价分配 5% 的日量预期，连续竞价
/// 使用平滑 U 型累计曲线；该函数只定义观测基准，不生成任何成交量。
fn intraday_expected_volume_fraction(
    elapsed_ticks: u64,
    ticks_per_day: u64,
    auction_ticks: u64,
) -> f64 {
    const U_SHAPE_STRENGTH: f64 = 0.60;
    let auction_share = if auction_ticks > 0 { 0.05 } else { 0.0 };
    if elapsed_ticks >= ticks_per_day {
        return 1.0;
    }
    let auction_entry_ticks = auction_ticks * 2 / 3;
    if auction_entry_ticks > 0 && elapsed_ticks <= auction_entry_ticks {
        return auction_share * elapsed_ticks as f64 / auction_entry_ticks as f64;
    }
    if elapsed_ticks <= auction_ticks {
        return auction_share;
    }
    let continuous_ticks = ticks_per_day - auction_ticks;
    let progress = (elapsed_ticks - auction_ticks) as f64 / continuous_ticks as f64;
    let u_shaped = progress
        + U_SHAPE_STRENGTH * (std::f64::consts::TAU * progress).sin() / std::f64::consts::TAU;
    auction_share + (1.0 - auction_share) * u_shaped
}

#[cfg(test)]
mod candle_open_tests {
    use super::*;
    use crate::{HotParams, InstParams, RetailParams};

    fn gap_stock_setup() -> SessionSetup {
        SessionSetup {
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![StockSpec {
                code: StockCode("600102".to_owned()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 0,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 0,
                hot_count: 0,
                retail_cash_median: Money::ZERO,
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 1,
                    chase_prob: 0.0,
                },
                inst: InstParams {
                    margin: 0.01,
                    order_size: 1,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 1,
                },
            },
            ticks_per_day: 10,
            auction_ticks: 0,
            closing_auction_ticks: 0,
            history_len: 10,
            t1_enabled: true,
            float_allocation: FloatAllocation::random(),
            start_date: default_civil_start_date(),
            simulation_policy_id: SIMULATION_POLICY_ID.to_string(),
        }
    }

    #[test]
    fn first_auction_trade_replaces_provisional_previous_close_on_gap_up_days() {
        let code = StockId(1);
        let mut session = GameSession::new(gap_stock_setup(), 7).unwrap();
        let mut previous_close = Money::from_cents(1_000);

        for auction_open_cents in [1_100, 1_250, 1_400] {
            let auction_open = Money::from_cents(auction_open_cents);
            let close = Money::from_cents(auction_open_cents + 20);

            // 开盘前的无成交 tick 只能是占位，不能成为实体开盘价。
            session.update_active_daily_candle(&code, previous_close, 0);
            // 集合竞价后的第一笔真实成交定义今日开盘价。
            session.update_active_daily_candle(&code, auction_open, 100);
            session.update_active_daily_candle(&code, close, 50);

            let candle = session.state.candle_book.active().get(&code).unwrap();
            assert_eq!(candle.open, auction_open);
            assert_ne!(candle.open, previous_close);
            assert_eq!(candle.close, close);
            assert_eq!(candle.volume, 150);

            session.commit_active_daily_candles();
            session.state.day += 1;
            previous_close = close;
        }
    }

    #[test]
    fn first_auction_trade_replaces_provisional_previous_close_on_gap_down_days() {
        let code = StockId(1);
        let mut session = GameSession::new(gap_stock_setup(), 11).unwrap();
        let mut previous_close = Money::from_cents(1_000);

        for auction_open_cents in [900, 800, 700] {
            let auction_open = Money::from_cents(auction_open_cents);
            let close = Money::from_cents(auction_open_cents - 20);

            session.update_active_daily_candle(&code, previous_close, 0);
            session.update_active_daily_candle(&code, auction_open, 100);
            session.update_active_daily_candle(&code, close, 50);

            let candle = session.state.candle_book.active().get(&code).unwrap();
            assert!(candle.open < previous_close, "测试数据必须保持跳空低开");
            assert_eq!(candle.open, auction_open);
            assert_eq!(candle.high, auction_open);
            assert_eq!(candle.low, close);
            assert_eq!(candle.close, close);
            assert_eq!(candle.volume, 150);

            session.commit_active_daily_candles();
            session.state.day += 1;
            previous_close = close;
        }
    }
}

#[cfg(test)]
mod intraday_volume_curve_tests {
    use super::intraday_expected_volume_fraction;

    #[test]
    fn expected_volume_is_front_and_back_loaded_instead_of_linear() {
        let ticks_per_day = 15_300;
        let auction_ticks = 900;
        let open_quarter = intraday_expected_volume_fraction(4_500, ticks_per_day, auction_ticks);
        let midpoint = intraday_expected_volume_fraction(8_100, ticks_per_day, auction_ticks);
        let close_quarter = intraday_expected_volume_fraction(11_700, ticks_per_day, auction_ticks);

        assert!(open_quarter > 0.05 + 0.95 * 0.25);
        assert!((midpoint - 0.525).abs() < 1e-9);
        assert!(close_quarter < 0.05 + 0.95 * 0.75);
    }

    #[test]
    fn auction_has_an_explicit_share_and_curve_finishes_at_one() {
        assert!((intraday_expected_volume_fraction(600, 15_300, 900) - 0.05).abs() < 1e-9);
        assert!((intraday_expected_volume_fraction(15_300, 15_300, 900) - 1.0).abs() < 1e-9);
        assert!(intraday_expected_volume_fraction(1, 1_000, 0) < 0.01);
    }
}

#[cfg(test)]
impl GameSession {
    pub(crate) fn auction_order_projection_for_test(
        &self,
    ) -> BTreeMap<StockId, Vec<AuctionOrderSnap>> {
        self.state
            .markets
            .iter()
            .filter(|(_, market)| market.matching_mode() == crate::orderbook::MatchingMode::Auction)
            .filter_map(|(code, market)| {
                let orders = market
                    .resting_orders()
                    .into_iter()
                    .map(|order| AuctionOrderSnap {
                        owner: order.owner,
                        side: order.side,
                        limit: order.price,
                        qty: order.qty,
                        order_id: order.id.0,
                    })
                    .collect::<Vec<_>>();
                (!orders.is_empty()).then_some((*code, orders))
            })
            .collect()
    }

    /// cfg(test) fixture 辅助：直接改写 tick/setup 后，把每只股票的 book 模式
    /// 重新对齐当前 PhasePolicy，保持统一不变量（模式只由交易阶段决定）。
    pub(crate) fn fixture_sync_market_modes_for_test(&mut self) {
        let mode = pipeline::phase::PhasePolicy::capture(self).matching_mode();
        for market in self.state.markets.values_mut() {
            market
                .set_matching_mode(mode)
                .expect("测试跳时后的空/未交叉簿可以进入目标阶段模式");
        }
    }

    pub(crate) fn fixture_append_auction_orders(
        &mut self,
        code: StockId,
        orders: Vec<AuctionOrderSnap>,
    ) -> Result<Option<Vec<AuctionOrderSnap>>, crate::MarketError> {
        let previous = self.auction_order_projection_for_test().remove(&code);
        let market = self
            .state
            .markets
            .get_mut(&code)
            .expect("竞价fixture股票必须存在");
        market.set_matching_mode(crate::orderbook::MatchingMode::Auction)?;
        let mut restored = market.resting_orders();
        let mut next_seq = market.book_next_sequence();
        for order in orders {
            restored.push(Order {
                id: OrderId(order.order_id),
                side: order.side,
                price: order.limit,
                qty: order.qty,
                original_qty: order.qty,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: order.owner,
                seq: next_seq,
            });
            next_seq = next_seq.checked_add(1).expect("竞价fixture序号不能溢出");
        }
        market.restore_resting_orders(&restored, next_seq)?;
        Ok(previous)
    }

    /// 在真实book中放置下游测试所需的被动单，不推进市场 tick。
    pub(crate) fn seed_order_for_test(
        &mut self,
        account: AccountId,
        intent: Intent,
        events: &mut Vec<Event>,
    ) {
        self.seed_order_in_book_for_test(account, intent, events, false);
    }

    pub(crate) fn seed_auction_order_for_test(
        &mut self,
        account: AccountId,
        intent: Intent,
        events: &mut Vec<Event>,
    ) {
        self.seed_order_in_book_for_test(account, intent, events, true);
    }

    fn seed_order_in_book_for_test(
        &mut self,
        account: AccountId,
        intent: Intent,
        events: &mut Vec<Event>,
        auction: bool,
    ) {
        // Sol 整改注入通道（cfg(test) 分流）：runtime 驱动器在位时挂单
        /// 权威在组/desk——经 D2 意图注入路由进 runtime 世界（写旧
        /// session book 在 runtime 模式下不可见——裁决"book fixture
        /// 分别适配"）。Sol 二审修复集 5：首步前（驱动器不在位）的注入
        /// 走旧簿+构造期迁移（RuntimeDriver::adopt_session_resting_orders
        /// ——免全局分流破坏旧管线 fixture 面）。事件面：runtime 模式的
        /// 受理事件由下一 step 产出（seed 调用方的 events 缓冲不在此
        /// 伪造就地事件——如实空）。
        #[cfg(test)]
        if self.runtime_driver.is_some() {
            let mut intents = vec![crate::session::ReceiptBearingIntent {
                owner: account,
                intent: intent.clone(),
                account_ordinal: 0,
                stock_ordinal: 0,
            }];
            let _ = &mut intents;
            let upper: Vec<i64> = {
                let driver = self
                    .runtime_driver
                    .as_ref()
                    .and_then(|cell| cell.lock().ok())
                    .expect("runtime 驱动器在位（上方已判）");
                driver
                    .world_public_views()
                    .iter()
                    .map(|v| v.daily_upper_limit)
                    .collect()
            };
            let mut driver = self
                .runtime_driver
                .as_mut()
                .and_then(|cell| cell.lock().ok())
                .expect("runtime 驱动器在位（上方已判）");
            driver
                .inject_player_intents(&intents, &upper)
                .expect("runtime 意图注入（seed_order 分流）");
            intents.clear();
            let _ = events;
            return;
        }
        match intent {
            Intent::PlaceLimit {
                code,
                side,
                price: LimitPrice::Fixed(price),
                qty,
            } => {
                let id = OrderId(self.state.next_order_id);
                self.state.next_order_id = self
                    .state
                    .next_order_id
                    .checked_add(1)
                    .expect("fixture order IDs");
                let seq = self.next_seq();
                if auction {
                    self.fixture_append_auction_orders(
                        code,
                        vec![AuctionOrderSnap {
                            owner: account,
                            side,
                            limit: price,
                            qty,
                            order_id: id.0,
                        }],
                    )
                    .expect("fixture 竞价挂入不得重复身份");
                } else {
                    let (last_price, best_bid, best_ask) = {
                        let market = self.state.markets.get(&code).expect("fixture stock exists");
                        (market.last_price(), market.best_bid(), market.best_ask())
                    };
                    if self.state.accounts[&account].kind() != AccountKind::Player {
                        let working = self.state.markets[&code]
                            .resting_orders_for(account)
                            .into_iter()
                            .filter(|order| order.side == side)
                            .collect::<Vec<_>>();
                        for old in working {
                            self.state
                                .markets
                                .get_mut(&code)
                                .unwrap()
                                .cancel(old.id)
                                .unwrap();
                            self.remove_npc_order_lifecycle(account, &code, old.id);
                            events.push(Event::OrderCanceled {
                                seq: self.next_seq(),
                                account,
                                code: code.clone(),
                                id: old.id,
                                remaining_qty: old.qty,
                            });
                        }
                    }
                    let order = Order {
                        id,
                        side,
                        price,
                        qty,
                        original_qty: qty,
                        filled_qty: 0,
                        filled_value: Money::ZERO,
                        owner: account,
                        seq,
                    };
                    let result = self
                        .state
                        .markets
                        .get_mut(&code)
                        .expect("fixture stock exists")
                        .place(order)
                        .expect("fixture passive limit order is valid");
                    assert!(
                        result.trades.is_empty(),
                        "fixture orders must not match on insertion"
                    );
                    if let Some(resting) = result.resting {
                        self.register_npc_order_lifecycle_at_quote(
                            account, &code, &resting, last_price, best_bid, best_ask,
                        );
                    }
                }
                events.push(Event::OrderAccepted {
                    seq,
                    account,
                    code,
                    id,
                    side,
                    price,
                    remaining_qty: qty,
                });
            }
            Intent::PlaceLimit { price, .. } => {
                panic!("fixture order must have a fixed price, got {price:?}")
            }
            Intent::Cancel { code, id } => {
                if !auction {
                    let order = self
                        .state
                        .markets
                        .get_mut(&code)
                        .expect("fixture stock exists")
                        .cancel(id)
                        .expect("fixture cancellation succeeds");
                    self.remove_npc_order_lifecycle(account, &code, id);
                    self.record_parent_order_canceled(account, &code, id)
                        .expect("fixture plan cancellation succeeds");
                    events.push(Event::OrderCanceled {
                        seq: self.next_seq(),
                        account,
                        code,
                        id,
                        remaining_qty: order.qty,
                    });
                } else {
                    let order = self
                        .state
                        .markets
                        .get_mut(&code)
                        .expect("fixture stock exists")
                        .cancel(id)
                        .expect("fixture auction order exists");
                    events.push(Event::OrderCanceled {
                        seq: self.next_seq(),
                        account,
                        code,
                        id,
                        remaining_qty: order.qty,
                    });
                }
            }
            Intent::PlaceMarket { .. } => panic!("fixtures seed passive limit orders only"),
        }
        self.state.envelope_ledger = crate::session::pipeline::EnvelopeLedger::new(
            self.state.next_receipt_base,
            self.project_live_envelopes()
                .expect("fixture live envelopes are valid"),
        )
        .expect("fixture ledger is valid");
    }
}

#[cfg(test)]
mod npc_working_quote_tests {
    use super::*;
    use crate::{HotParams, InstParams, RetailParams};

    pub(super) fn quote_setup(auction_ticks: u64) -> SessionSetup {
        SessionSetup {
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![StockSpec {
                code: StockCode("600888".to_owned()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 0,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 1,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                },
                inst: InstParams {
                    margin: 0.05,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 100,
                },
            },
            ticks_per_day: if auction_ticks == 0 { 100 } else { 15_300 },
            auction_ticks,
            closing_auction_ticks: 0,
            history_len: 10,
            t1_enabled: true,
            float_allocation: FloatAllocation::random(),
            start_date: default_civil_start_date(),
            simulation_policy_id: SIMULATION_POLICY_ID.to_string(),
        }
    }

    #[test]
    fn restore_reconciles_edited_institution_holdings_without_fake_buys() {
        let session = GameSession::new(quote_setup(0), 42).unwrap();
        let mut save = session.save().unwrap();
        let account_id = *save.belief_books.keys().next().unwrap();
        let code = StockId(0);
        save.snapshot
            .accounts
            .get_mut(&account_id)
            .unwrap()
            .positions
            .insert(
                code.clone(),
                PositionSnap {
                    qty: 100,
                    t1_locked: 0,
                    invested_cents: 100_000,
                    recovered_cents: 0,
                },
            );

        let restored = GameSession::restore(&save).unwrap();
        let experience = restored.state.belief_participants[&account_id]
            .belief()
            .experience();
        assert_eq!(
            experience.feedback.stocks[&code].institutional_fees_paid,
            Some(Money::ZERO)
        );
        assert_eq!(experience.stocks[&code].last_buy_order_id, None);
        assert_eq!(experience.stocks[&code].last_buy_price, None);

        let mut edited_save = restored.save().unwrap();
        edited_save.next_order_id = 2;
        edited_save
            .snapshot
            .accounts
            .get_mut(&account_id)
            .unwrap()
            .positions
            .remove(&code);
        let book = edited_save.belief_books.get_mut(&account_id).unwrap();
        let experience = book.experience_mut();
        experience.stocks.get_mut(&code).unwrap().last_buy_price = Some(Money::from_cents(1_000));
        experience.stocks.get_mut(&code).unwrap().last_buy_order_id = Some(1);
        experience
            .stocks
            .get_mut(&code)
            .unwrap()
            .adverse_move_recorded = true;
        let reconciled = GameSession::restore(&edited_save).unwrap();
        let experience = reconciled.state.belief_participants[&account_id]
            .belief()
            .experience();
        assert!(!experience.feedback.stocks.contains_key(&code));
        assert_eq!(experience.stocks[&code].last_buy_price, None);
        assert_eq!(experience.stocks[&code].last_buy_order_id, None);
        assert_eq!(experience.stocks[&code].entry_reference_price, None);
        assert_eq!(experience.stocks[&code].peak_price_since_entry, None);
        assert!(!experience.stocks[&code].adverse_move_recorded);
    }

    #[test]
    fn opening_institution_experience_is_unchanged_by_save_restore() {
        let mut setup = quote_setup(0);
        setup.stocks[0].float_shares = 100;
        let session = GameSession::new(setup, 42).unwrap();
        assert!(session
            .state
            .belief_participants
            .values()
            .map(|participant| participant.belief())
            .any(|book| !book.experience().feedback.stocks.is_empty()));
        let opening_experiences: BTreeMap<_, _> = session
            .state
            .belief_participants
            .iter()
            .map(|(id, participant)| (id, participant.belief()))
            .map(|(id, book)| (*id, book.experience().clone()))
            .collect();
        let restored = GameSession::restore(&session.save().unwrap()).unwrap();
        for (id, opening) in opening_experiences {
            assert_eq!(
                restored.state.belief_participants[&id]
                    .belief()
                    .experience(),
                &opening
            );
        }
    }

    // 下列断言的 fault 产生于 persistence 分片（码段 2000-2199）。码值已按
    // persistence.rs 实际站点注释核对回填（S2 预测值 2132/2133/2134 整体偏移 +1）。
    const FAULT_FUTURE_EXPERIENCE_CLOCKS: u16 = 2131;
    const FAULT_INVALID_FAILURE_EXPERIENCE: u16 = 2132;
    const FAULT_INVALID_TRADE_EXPERIENCE: u16 = 2133;

    #[test]
    fn saved_institution_feedback_rejects_future_clocks_and_invalid_failure_references() {
        let session = GameSession::new(quote_setup(0), 42).unwrap();
        let save = session.save().unwrap();
        let account_id = *save.belief_books.keys().next().unwrap();
        let code = StockId(0);
        let current = crate::experience::ExperienceMoment {
            civil_date: save.civil_clock.current_date,
            market_minute: 0,
            trading_day: 0,
        };

        let mut invalid_reference = save.clone();
        let feedback = &mut invalid_reference
            .belief_books
            .get_mut(&account_id)
            .unwrap()
            .experience_mut()
            .feedback;
        feedback.latest_moment = Some(current);
        feedback
            .failure_events
            .push(crate::experience::FailureEventRecord {
                code: StockId(9),
                order_id: Some(0),
                moment: current,
            });
        assert!(matches!(
            persistence::validate_save_slot(&invalid_reference),
            // 数值断言替代原文本断言 "invalid failure experience"。
            Err(SessionError::InvalidSave(fault)) if fault.code == FAULT_INVALID_FAILURE_EXPERIENCE
        ));

        let mut future = save;
        let future_moment = crate::experience::ExperienceMoment {
            market_minute: 1,
            ..current
        };
        let feedback = &mut future
            .belief_books
            .get_mut(&account_id)
            .unwrap()
            .experience_mut()
            .feedback;
        feedback.latest_moment = Some(future_moment);
        feedback
            .failure_events
            .push(crate::experience::FailureEventRecord {
                code,
                order_id: None,
                moment: future_moment,
            });
        assert!(matches!(
            persistence::validate_save_slot(&future),
            // 数值断言替代原文本断言 "future experience clocks" || "invalid failure experience"
            // （首错序由哪个码先出现验证）。
            Err(SessionError::InvalidSave(fault)) if fault.code == FAULT_FUTURE_EXPERIENCE_CLOCKS
                || fault.code == FAULT_INVALID_FAILURE_EXPERIENCE
        ));
    }

    #[test]
    fn saved_institution_failure_requires_a_real_buy_identity() {
        let session = GameSession::new(quote_setup(0), 42).unwrap();
        let mut save = session.save().unwrap();
        let account = *save.belief_books.keys().next().unwrap();
        let moment = crate::experience::ExperienceMoment {
            civil_date: save.civil_clock.current_date,
            market_minute: 0,
            trading_day: 0,
        };
        let feedback = &mut save
            .belief_books
            .get_mut(&account)
            .unwrap()
            .experience_mut()
            .feedback;
        feedback.latest_moment = Some(moment);
        feedback
            .failure_events
            .push(crate::experience::FailureEventRecord {
                code: StockId(0),
                order_id: None,
                moment,
            });
        assert!(matches!(GameSession::restore(&save),
            // 数值断言替代原文本断言 "invalid failure experience"。
            Err(SessionError::InvalidSave(fault)) if fault.code == FAULT_INVALID_FAILURE_EXPERIENCE));
    }

    #[test]
    fn saved_institution_buy_memory_requires_paired_identity() {
        let session = GameSession::new(quote_setup(0), 42).unwrap();
        let save = session.save().unwrap();
        let account = *save.belief_books.keys().next().unwrap();
        for (price, order, adverse) in [
            (Some(Money::from_cents(1000)), None, false),
            (None, Some(1), false),
            (None, None, true),
        ] {
            let mut invalid = save.clone();
            invalid.next_order_id = 2;
            invalid
                .belief_books
                .get_mut(&account)
                .unwrap()
                .experience_mut()
                .stocks
                .insert(
                    StockId(0),
                    crate::experience::RetailStockExperience {
                        last_buy_price: price,
                        last_buy_order_id: order,
                        adverse_move_recorded: adverse,
                        ..Default::default()
                    },
                );
            assert!(
                matches!(GameSession::restore(&invalid),
                // 数值断言替代原文本断言 "invalid trade experience"。
                Err(SessionError::InvalidSave(fault)) if fault.code == FAULT_INVALID_TRADE_EXPERIENCE),
                "unpaired buy memory {price:?}/{order:?} or unsupported adverse flag must be rejected"
            );
        }
    }

    pub(super) fn retail_quote_setup() -> SessionSetup {
        let mut setup = quote_setup(0);
        setup.npcs.retail_count = 1;
        setup.npcs.inst_count = 0;
        setup
    }

    fn buy(code: &StockId, price: i64) -> Intent {
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(price)),
            qty: 100,
        }
    }

    pub(super) fn two_stock_quote_setup() -> SessionSetup {
        let mut setup = quote_setup(0);
        let second = StockCode("600889".to_owned());
        let mut spec = setup.stocks[0].clone();
        spec.code = second;
        setup.stocks.push(spec);
        setup
    }

    pub(super) fn force_attention_candidate(
        session: &mut GameSession,
        account: AccountId,
        tick: u64,
    ) {
        // Sol 整改注入通道（cfg(test) 分流）：runtime 驱动器在位时调度
        /// 权威在组（P1-10）——显式 ForceDue 指令（组验证执行；不偷改
        /// session 镜像）。旧管线（无驱动器）路径不变。Sol 二审修复集 5：
        /// 首步前（驱动器不在位）走旧镜像路径——构造器自会话 attention
        /// 恢复 next_due（镜像值即组权威起点）。
        #[cfg(test)]
        if session.runtime_driver.is_some() {
            let target = tick.max(session.state.tick + 1);
            session.force_due_via_runtime(&[account], target);
            return;
        }
        let attention = session.state.npc_attention.get_mut(&account).unwrap();
        attention.base_probability = 1.0;
        attention.next_attention_candidate_tick = tick;
        session.state.attention_scheduler.enqueue(tick, account);
    }

    // Sol 二审修复集 5 红证：首步前 seed 的挂单经**构造期迁移**进入
    // runtime 世界（RuntimeDriver::adopt_session_resting_orders——组 0
    // 下一 tick round 0 经 ExternalPlan 挂出）——首步的受理事件面可见。
    // 此前首构造不迁入旧挂单：seed 只写 session 旧簿，runtime 世界不可见。
    // 仅 runtime 翻转模式有意义（缺省走旧管线，seed 本就在旧簿生效）。
    #[test]
    fn sol5_seed_before_first_step_reaches_runtime_world() {
        if !crate::runtime::step_runtime_enabled() {
            return;
        }
        let mut session = GameSession::new(retail_quote_setup(), 311).unwrap();
        let code = StockId(0);
        let mut events = Vec::new();
        session.seed_order_for_test(AccountId(1), buy(&code, 900), &mut events);
        let step_events = session.step().expect("healthy step");
        assert!(
            step_events.iter().any(|event| matches!(
                event,
                Event::OrderAccepted {
                    account: AccountId(1),
                    ..
                }
            )),
            "首步前 seed 的挂单经构造期迁移进入 runtime 世界（首步即受理）"
        );
    }

    // Sol 三审修复 3：terminal 投影覆盖旧 Active 镜像——小型确定性
    // fixture（组簿 terminal 记录经 set_runtime_plans 交付点覆盖 session
    // 侧残留的旧 Active 镜像；不依赖巨量成交——全额成交即 Completed）。
    #[test]
    fn sol4th_terminal_projection_overwrites_stale_active_mirror() {
        let mut session = GameSession::new(quote_setup(0), 71).unwrap();
        let account = AccountId(1);
        let code = StockId(0);
        // session 侧残留旧 Active 镜像（迁移期注入面保护语义的对照起点）。
        session
            .state
            .plans
            .create(crate::plans::PlanOpen {
                account,
                code: code.clone(),
                direction: Side::Buy,
                target: crate::plans::PlanTarget::ShareCount(100),
                opinion: crate::plans::PlanOpinion {
                    signal_score_bp: 3_000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: crate::plans::Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .expect("旧镜像计划创建");
        assert!(
            session.plans_debug().iter().any(|(.., status)| status == "Active"),
            "前置：session 侧旧 Active 镜像在册"
        );
        // 组簿权威：同键计划的 terminal 记录（全额成交 ⇒ Completed）。
        let mut book = crate::plans::PlanBook::default();
        let pid = book
            .create(crate::plans::PlanOpen {
                account,
                code: code.clone(),
                direction: Side::Buy,
                target: crate::plans::PlanTarget::ShareCount(100),
                opinion: crate::plans::PlanOpinion {
                    signal_score_bp: 3_000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: crate::plans::Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .expect("组簿计划创建");
        book.install_execution(pid, 100, Money::from_cents(1_000), 240)
            .expect("执行腿登记");
        book.record_child_submitted(account, &code, Side::Buy, crate::OrderId(7), 100, 0)
            .expect("子单身份登记");
        book.record_child_fill(account, &code, Side::Buy, crate::OrderId(7), 100, 0)
            .expect("全额成交推进");
        assert!(
            matches!(
                book.plan(pid).unwrap().status(),
                crate::plans::PlanStatus::Completed
            ),
            "前置：组簿记录为 terminal（Completed）"
        );
        session.set_runtime_plans(book);
        let debug = session.plans_debug();
        assert!(
            debug.iter().any(|(.., status)| status == "Completed"),
            "terminal 记录经交付点投影可见：{debug:?}"
        );
        assert!(
            debug.iter().all(|(.., status)| status != "Active"),
            "旧 Active 镜像不残留（最后计划结束后被 terminal 覆盖）：{debug:?}"
        );
    }

    // Sol 五审修复 1/2 红证：同拍完整结算——step() 返回时本拍成交的
    // 持仓/T+1/现金/成本已在公开 session 面（ADR-0017；此前买腿持仓/现金
    // 迟一拍）。**不许多跑一拍让断言通过**（Sol 点名）——全部断言在本拍
    // 返回值上；随后仅验证下一拍不双计、失败拍不泄漏。
    #[test]
    fn sol6th_same_tick_settlement_full_projection() {
        if !crate::runtime::step_runtime_enabled() {
            return;
        }
        use crate::orderbook::Side;
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 1;
        let mut session = GameSession::new(setup, 71).unwrap();
        let seller = AccountId(1);
        let buyer = AccountId(0);
        let code = StockId(0);
        let price = session.state.markets[&code].last_price();
        session
            .fixture_account_mut(&seller)
            .unwrap()
            .grant_position(code.clone(), 100, price)
            .unwrap();
        session.seed_order_for_test(
            seller,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(price),
                qty: 100,
            },
            &mut Vec::new(),
        );
        let seller_cash = session.state.accounts[&seller].cash();
        let buyer_cash = session.state.accounts[&buyer].cash();
        let gross = price.mul_shares(100).unwrap();
        let config = &session.state.setup.config;
        let seller_net = gross
            .sub(config.commission(gross).unwrap())
            .unwrap()
            .sub(config.stamp_tax(gross).unwrap())
            .unwrap()
            .sub(config.transfer_fee(gross).unwrap())
            .unwrap();
        let buyer_total = crate::session::buy_order_reservation(
            config,
            price,
            100,
            Money::ZERO,
        )
        .unwrap();
        session
            .enqueue_player_intent(
                buyer,
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: LimitPrice::Fixed(price),
                    qty: 100,
                },
            )
            .unwrap();
        let events = session.step().expect("healthy step");
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::Trade {
                    maker: AccountId(1),
                    taker: AccountId(0),
                    qty: 100,
                    ..
                }
            )),
            "fixture 前提：本拍成交发生"
        );
        // 同拍全面断言（本拍返回值——不补拍）。
        let buyer_pos = session.state.accounts[&buyer].positions();
        assert_eq!(buyer_pos[&code].qty(), 100, "同拍：买方持仓入账");
        assert_eq!(
            buyer_pos[&code].t1_locked(),
            100,
            "同拍：T+1 冻结入账（买入当日不可卖）"
        );
        assert_eq!(
            session.state.accounts[&buyer].sellable_qty(&code),
            0,
            "同拍：可卖量=0（T+1）"
        );
        assert_eq!(
            buyer_pos[&code].invested_cents(),
            gross.cents(),
            "同拍：买方成本基（成交额）"
        );
        assert_eq!(
            session.state.accounts[&buyer].cash(),
            buyer_cash.sub(buyer_total).unwrap(),
            "同拍：买方现金扣全额预留（限价=成交价，全额成交）"
        );
        assert!(
            !session.state.accounts[&seller].positions().contains_key(&code),
            "同拍：卖方清仓条目删除"
        );
        assert_eq!(
            session.state.accounts[&seller].cash(),
            seller_cash.add(seller_net).unwrap(),
            "同拍：卖方现金净得入账"
        );
        // 下一拍不双计（整值回写对齐——值保持）。
        session.step().expect("下一拍（无成交）");
        assert_eq!(
            session.state.accounts[&buyer].positions()[&code].qty(),
            100,
            "下一拍不双计：持仓保持"
        );
        assert_eq!(
            session.state.accounts[&buyer].cash(),
            buyer_cash.sub(buyer_total).unwrap(),
            "下一拍不双计：现金保持"
        );
        // Abort 不泄漏部分见 drive_tests::sol6th_same_tick_abort_leaks_nothing
        // （驱动器直连持有——世界校验器注入口在驱动器面）。
    }

    #[test]
    fn accepted_retail_observation_persists_an_unheld_watchlist_stock() {
        let code = StockId(0);
        let retail = AccountId(1);
        let mut session = GameSession::new(retail_quote_setup(), 124).unwrap();
        force_attention_candidate(&mut session, retail, 0);

        session.step().expect("healthy step");

        let experience = &session.state.retail_experience[&retail];
        let stock = experience
            .stocks
            .get(&code)
            .expect("accepted observation must persist the reviewed unheld stock");
        assert_eq!(stock.entry_reference_price, None);
        assert_eq!(stock.last_buy_price, None);
        // T3（ADR-0042 §9.3/§9.7，A4 失败 2 双契约登记）：观察分钟一律 stamp
        // f(epoch)——本观察在 epoch 0 受理（D(0) 于消费 tick 入口生产同 tick
        // 消费），写入=day0 的 epoch-0 分钟（0）；step 返回后的
        // current_market_minute() 已推进到 f(1)，不再与观察 stamp 同拍。禁止
        // 加一/减 tick 修绿——按同一决策 epoch 的冻结观察基精确比较（v3）。
        assert_eq!(stock.last_observed_market_minute, 0);
        assert_ne!(
            stock.last_observed_market_minute,
            session.current_market_minute(),
            "发布节奏差：观察 stamp 停留在决策 epoch 的冻结分钟"
        );
    }

    #[test]
    fn market_view_keeps_tick_samples_separate_from_completed_market_minutes() {
        let code = StockId(0);
        let mut session = GameSession::new(quote_setup(0), 1_001).unwrap();
        session.state.price_history.insert(
            code.clone(),
            [Money::from_cents(1_000), Money::from_cents(1_050)]
                .into_iter()
                .collect(),
        );
        session.state.market_minute_closes.insert(
            code.clone(),
            vec![
                MarketMinuteClose {
                    absolute_trading_minute: 0,
                    close: Money::from_cents(1_000),
                },
                MarketMinuteClose {
                    absolute_trading_minute: 1,
                    close: Money::from_cents(1_000),
                },
            ],
        );

        let view = session.build_market_view();
        assert_eq!(
            view.stocks[&code].recent_prices,
            vec![Money::from_cents(1_000), Money::from_cents(1_050)]
        );
        assert_eq!(
            view.stocks[&code].recent_market_minute_prices,
            vec![Money::from_cents(1_000), Money::from_cents(1_000)]
        );
    }

    #[test]
    fn behavior_observation_remains_bounded_after_six_thousand_completed_days() {
        let code = StockId(0);
        let mut session = GameSession::new(quote_setup(0), 120).unwrap();
        let template = session.state.candle_book.histories()[&code][0].clone();
        session.state.candle_book.replace_history(
            code.clone(),
            (0..6_000)
                .map(|day| DailyCandle {
                    time: i64::from(day) * 86_400,
                    close: Money::from_cents(1_000 + i64::from(day % 100)),
                    ..template.clone()
                })
                .collect::<Vec<_>>()
                .into(),
        );
        session.state.market_minute_closes.insert(
            code.clone(),
            vec![MarketMinuteClose {
                absolute_trading_minute: 0,
                close: Money::from_cents(1_100),
            }],
        );

        let retained =
            retained_behavior_daily_closes_history(&session.state.candle_book.histories()[&code]);
        assert_eq!(retained.len(), 250);
        assert_eq!(retained.first().unwrap().trading_day, 5_750);
        assert_eq!(retained.last().unwrap().trading_day, 5_999);

        let short = retained_behavior_daily_closes(
            &session.state.candle_book.histories()[&code]
                .iter()
                .take(100)
                .cloned()
                .collect::<Vec<_>>(),
        );
        assert_eq!(short.len(), 100);
        assert_eq!(short.first().unwrap().trading_day, 0);
        assert_eq!(short.last().unwrap().trading_day, 99);

        let path = session.market_price_path_observations().unwrap();

        assert_eq!(path[&code].two_hundred_fifty_day.available_span, 250);
        assert!(path[&code].two_hundred_fifty_day.return_ratio.is_some());
    }

    #[test]
    fn closing_day_keeps_history_beyond_the_preset_window() {
        let code = StockId(0);
        let mut session = GameSession::new(quote_setup(0), 120).unwrap();
        let first = session.state.candle_book.histories()[&code][0].clone();
        let previous = session.state.candle_book.histories()[&code].len();
        let closed = DailyCandle {
            time: 0,
            ..session.state.candle_book.histories()[&code]
                .last()
                .unwrap()
                .clone()
        };
        session
            .state
            .candle_book
            .set_active(code.clone(), closed.clone());

        session.commit_active_daily_candles();

        assert_eq!(
            session.state.candle_book.histories()[&code].len(),
            previous + 1
        );
        assert_eq!(session.state.candle_book.histories()[&code][0], first);
        assert_eq!(
            session.state.candle_book.histories()[&code].last(),
            Some(&closed)
        );
        assert_eq!(session.snapshot().daily_candles[&code].len(), previous + 1);
    }

    #[test]
    fn realized_profit_can_make_cost_return_unavailable_without_breaking_observation() {
        let code = StockId(0);
        let account = AccountId(1);
        let mut session = GameSession::new(retail_quote_setup(), 121).unwrap();
        let config = session.state.setup.config.clone();
        let holder = session.fixture_account_mut(&account).unwrap();
        holder
            .grant_position(code.clone(), 1_000, Money::from_cents(1_000))
            .unwrap();
        holder
            .apply_sell(&config, code.clone(), Money::from_cents(2_000), 500)
            .unwrap();
        session.observe_retail_experience(&[account]).unwrap();

        let risks = session.account_risk_observations_for(&[account]);
        let position = &risks[&account].positions[&code];

        assert_eq!(position.unrealized_return, None);
        assert!(position.equity_weight.is_some());
    }

    #[test]
    fn initial_attention_candidates_are_individually_distributed() {
        let mut setup = quote_setup(0);
        setup.npcs.retail_count = 60;
        setup.npcs.inst_count = 0;
        let session = GameSession::new(setup, 0xA77E_7710).unwrap();
        let candidate_ticks: BTreeSet<u64> = session
            .state
            .npc_attention
            .values()
            .map(|state| state.next_attention_candidate_tick)
            .collect();

        assert!(candidate_ticks.len() > 1);
        assert!(candidate_ticks.iter().any(|tick| *tick > 0));
    }
}

impl CommittableSessionState {
    /// 按数值 id 查发行人公司（公司域仍以 StockCode 关联，翻译在会话侧完成；
    /// 公司域数值化后此助手消亡——见 string-free-inventory.md A 表）。
    fn issuer_of_stock(&self, id: StockId) -> Option<crate::company::CompanyId> {
        let code = self.stocks.code(id)?;
        self.company_registry
            .issuer_of(&crate::account::StockCode(code.to_string()))
    }
    fn clone_for_shadow(&self) -> Result<Self, StepFatal> {
        let Self {
            setup,
            validated_config,
            stocks,
            rng,
            seed,
            markets,
            accounts,
            history_reads,
            price_history,
            market_minute_closes,
            candle_book,
            pending_player,
            ingress_receipt_cursors,
            npc_attention,
            retail_experience,
            npc_order_lifecycles,
            last_retail_decisions,
            last_retail_order_events,
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces,
            #[cfg(feature = "simulation-diagnostics")]
            causal,
            attention_scheduler,
            company_registry,
            operations,
            groups,
            closing,
            library,
            disclosures,
            plans,
            urgency_policy,
            belief_participants,
            envelope_ledger,
            retail_projection_seen,
            next_receipt_base,
            next_order_id,
            tick,
            day,
            seq,
            civil_clock,
        } = self;
        let accounts =
            accounts
                .clone_for_shadow()
                .map_err(|error| StepFatal::InvariantViolation {
                    description: error.to_string(),
                    location: "GameSession::clone_for_tick_shadow".to_owned(),
                })?;
        Ok(Self {
            setup: setup.clone(),
            validated_config: validated_config.clone(),
            stocks: stocks.clone(),
            rng: rng.clone(),
            seed: *seed,
            markets: markets.clone(),
            accounts,
            history_reads: history_reads.clone(),
            price_history: price_history.clone(),
            market_minute_closes: market_minute_closes.clone(),
            candle_book: candle_book.clone(),
            pending_player: pending_player.clone(),
            ingress_receipt_cursors: ingress_receipt_cursors.clone(),
            npc_attention: npc_attention.clone(),
            retail_experience: retail_experience.clone(),
            npc_order_lifecycles: npc_order_lifecycles.clone(),
            last_retail_decisions: last_retail_decisions.clone(),
            last_retail_order_events: last_retail_order_events.clone(),
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces: npc_decision_traces.clone(),
            #[cfg(feature = "simulation-diagnostics")]
            causal: causal.clone(),
            attention_scheduler: attention_scheduler.clone(),
            company_registry: company_registry.clone(),
            groups: groups.clone(),
            operations: operations.clone(),
            closing: closing.clone(),
            library: library.clone(),
            disclosures: disclosures.clone(),
            plans: plans.clone(),
            urgency_policy: *urgency_policy,
            belief_participants: belief_participants.clone(),
            envelope_ledger: envelope_ledger.clone(),
            retail_projection_seen: retail_projection_seen.clone(),
            next_receipt_base: *next_receipt_base,
            next_order_id: *next_order_id,
            tick: *tick,
            day: *day,
            seq: *seq,
            civil_clock: civil_clock.clone(),
        })
    }

    fn commit_from(&mut self, shadow: Self) {
        #[cfg(feature = "verification-harness")]
        let _commit_scope = verify_metrics::Scope::new(
            &verify_metrics::METRICS.commit_from_calls,
            &verify_metrics::METRICS.commit_from_ns,
        );
        let Self {
            setup,
            validated_config,
            stocks,
            rng,
            seed,
            markets,
            accounts,
            history_reads,
            price_history,
            market_minute_closes,
            candle_book,
            pending_player,
            ingress_receipt_cursors,
            npc_attention,
            retail_experience,
            npc_order_lifecycles,
            last_retail_decisions,
            last_retail_order_events,
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces,
            #[cfg(feature = "simulation-diagnostics")]
            causal,
            attention_scheduler,
            company_registry,
            operations,
            groups,
            closing,
            library,
            disclosures,
            plans,
            belief_participants,
            envelope_ledger,
            retail_projection_seen,
            next_receipt_base,
            next_order_id,
            tick,
            day,
            seq,
            civil_clock,
            urgency_policy,
        } = shadow;
        // 专用析构线程（E7 复活路径）：旧权威值的析构整体移交池外线程，
        // actor 只做字段交接。四个分页容器原先走 rayon 池内 par drop，
        // 池外移交后也不再产生池交互。Copy 字段仍直接赋值。
        self.urgency_policy = urgency_policy;
        defer_swap(&mut self.setup, setup, FieldId::Setup);
        // 运行时费率快照随 shadow 原样回流（shadow 从权威克隆而来，二者恒相等；
        // 会话外的静默改写在下一 tick 门禁被指纹比对拒绝）。
        defer_swap(
            &mut self.validated_config,
            validated_config,
            FieldId::ValidatedConfig,
        );
        defer_swap(&mut self.stocks, stocks, FieldId::Stocks);
        defer_swap(&mut self.rng, rng, FieldId::Rng);
        self.seed = seed;
        defer_swap(&mut self.markets, markets, FieldId::Markets);
        defer_swap(&mut self.accounts, accounts, FieldId::Accounts);
        defer_swap(
            &mut self.history_reads,
            history_reads,
            FieldId::HistoryReads,
        );
        defer_swap(
            &mut self.price_history,
            price_history,
            FieldId::PriceHistory,
        );
        defer_swap(
            &mut self.market_minute_closes,
            market_minute_closes,
            FieldId::MarketMinuteCloses,
        );
        defer_swap(&mut self.candle_book, candle_book, FieldId::CandleBook);
        defer_swap(
            &mut self.pending_player,
            pending_player,
            FieldId::PendingPlayer,
        );
        defer_swap(
            &mut self.ingress_receipt_cursors,
            ingress_receipt_cursors,
            FieldId::IngressReceiptCursors,
        );
        defer_swap(
            &mut self.npc_attention,
            npc_attention,
            FieldId::NpcAttention,
        );
        defer_swap(
            &mut self.retail_experience,
            retail_experience,
            FieldId::RetailExperience,
        );
        defer_swap(
            &mut self.npc_order_lifecycles,
            npc_order_lifecycles,
            FieldId::NpcOrderLifecycles,
        );
        defer_swap(
            &mut self.last_retail_decisions,
            last_retail_decisions,
            FieldId::LastRetailDecisions,
        );
        defer_swap(
            &mut self.last_retail_order_events,
            last_retail_order_events,
            FieldId::LastRetailOrderEvents,
        );
        #[cfg(feature = "simulation-diagnostics")]
        {
            defer_swap(
                &mut self.npc_decision_traces,
                npc_decision_traces,
                FieldId::NpcDecisionTraces,
            );
            defer_swap(&mut self.causal, causal, FieldId::Causal);
        }
        defer_swap(
            &mut self.attention_scheduler,
            attention_scheduler,
            FieldId::AttentionScheduler,
        );
        defer_swap(
            &mut self.company_registry,
            company_registry,
            FieldId::CompanyRegistry,
        );
        defer_swap(&mut self.operations, operations, FieldId::Operations);
        defer_swap(&mut self.groups, groups, FieldId::Groups);
        defer_swap(&mut self.closing, closing, FieldId::Closing);
        defer_swap(&mut self.library, library, FieldId::Library);
        defer_swap(&mut self.disclosures, disclosures, FieldId::Disclosures);
        defer_swap(&mut self.plans, plans, FieldId::Plans);
        defer_swap(
            &mut self.belief_participants,
            belief_participants,
            FieldId::BeliefParticipants,
        );
        defer_swap(
            &mut self.envelope_ledger,
            envelope_ledger,
            FieldId::EnvelopeLedger,
        );
        defer_swap(
            &mut self.retail_projection_seen,
            retail_projection_seen,
            FieldId::RetailProjectionSeen,
        );
        self.next_receipt_base = next_receipt_base;
        self.next_order_id = next_order_id;
        self.tick = tick;
        self.day = day;
        self.seq = seq;
        defer_swap(&mut self.civil_clock, civil_clock, FieldId::CivilClock);
    }
}

#[cfg(test)]
mod committable_state_tests {
    use super::*;

    fn fatal(description: &str) -> StepFatal {
        StepFatal::InvariantViolation {
            description: description.to_owned(),
            location: "committable_state_tests".to_owned(),
        }
    }

    /// 观察真实成功 step 的移交值与实际 Job，不依赖其他实例的累计计数。
    /// 裁决 (d)（逐字段 defer_swap）形态：旧状态以完整字段 Job 集离开 commit 关键路径。
    /// 原 #23 断言「恰一个完整 CommittableSessionState Job（携带 (tick,cash)）」属被
    /// 否决的 replace 形态，随该形态失效（leader 裁决 2026-10-10 登记项）；本版以字段
    /// Job 等价覆盖其意图：字段集完整（≥30 Job）、旧状态真实（旧 AccountBook 携带步前
    /// 现金）、Arc lease 恰一次释放、drain 后无残留。
    /// 覆盖局限（Astra 登记）：Job 在测试线程手动执行（受控 backend），本测试自身
    /// 不能证明执行线程在池外——池外专用线程路径由 deferred_drop 测试组的
    /// defer_runs_on_dedicated_thread_and_counts（真实 backend）覆盖。
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn successful_step_transfers_one_complete_old_state_and_releases_it_once() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::Arc;
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 719).unwrap();
        let before_tick = session.state.tick;
        let before_cash = session.state.accounts[&AccountId(0)].cash();
        let owners = Arc::strong_count(&session.state.company_registry);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let evidence = observed.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(64);
        deferred_drop::controlled::with_backend(
            sender,
            move |value| {
                evidence.borrow_mut().push((
                    value.downcast_ref::<CommittableSessionState>().is_some(),
                    value
                        .downcast_ref::<AccountBook>()
                        .map(|old| old[&AccountId(0)].cash()),
                ));
            },
            || session.step().unwrap(),
        );
        let observed = observed.borrow();
        assert!(
            observed.len() >= 29,
            "旧状态必须以完整字段集离开关键路径（当前字段表 32 个 defer_swap；批四删除 PendingNpc），实际 {} 个 Job",
            observed.len()
        );
        assert!(
            observed.iter().all(|(whole, _)| !*whole),
            "裁决 (d)：移交单位是字段 Job，不得出现整状态 Job"
        );
        let old_cash_jobs: Vec<_> = observed.iter().filter_map(|&(_, cash)| cash).collect();
        assert_eq!(
            old_cash_jobs,
            vec![before_cash],
            "恰一个旧 AccountBook Job，且必须携带步前现金"
        );
        assert_eq!(session.state.tick, before_tick + 1);
        assert_eq!(
            Arc::strong_count(&session.state.company_registry),
            owners + 1,
            "未执行的 company_registry 字段 Job 必须仍持有真实旧状态 Arc"
        );
        let jobs: Vec<_> = receiver.try_iter().collect();
        assert_eq!(jobs.len(), observed.len(), "全部字段 Job 必须成功入队");
        for job in jobs {
            job();
        }
        assert_eq!(
            Arc::strong_count(&session.state.company_registry),
            owners,
            "旧状态的 Arc lease 必须恰一次释放"
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn failed_step_does_not_transfer_authority_to_drop_backend() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 720).unwrap();
        let before = session.business_state_hash().unwrap();
        let revision = session.external_revision();
        session.post_shadow_failure = Some(fatal("controlled post-shadow failure"));
        let attempts = Rc::new(RefCell::new(0));
        let evidence = attempts.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(64);
        let result = deferred_drop::controlled::with_backend(
            sender,
            move |_| *evidence.borrow_mut() += 1,
            || session.step(),
        );
        assert!(result.is_err());
        assert_eq!(*attempts.borrow(), 0);
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(session.external_revision(), revision);
        assert!(session.poison.is_some());
    }

    /// 64 条是 Job 数上限而不是字节上限；用 65 次连续 commit 实测受控 backlog 与 drain。
    /// 裁决 (d) 形态：每次 commit 移交一个完整字段 Job 集（K≥30），队列容量语义不变。
    /// 原 #23 的「每旧 State 恰一个 Job + State 本体字节记账」断言属被否决的 replace
    /// 形态，随该形态失效（leader 裁决 2026-10-10 登记项）；字节证据改为最重字段
    /// （npc_order_lifecycles，4096 条/次）容量记账 + 全字段 Job 计数。
    /// 覆盖局限（Astra 登记）：Job 在测试线程手动 drain，本测试自身不能证明执行
    /// 线程在池外——同上，由真实 backend 的专用线程测试覆盖。
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn complete_state_backlog_is_bounded_and_full_queue_releases_inline() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::Arc;
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 721).unwrap();
        session.state.npc_order_lifecycles = (0..4096)
            .map(|id| NpcOrderLifecycle {
                account: AccountId(1),
                code: StockId(0),
                order_id: OrderId(id),
                placed_market_minute: 0,
                expires_market_minute: 100,
            })
            .collect();
        let baseline_owners = Arc::strong_count(&session.state.company_registry);
        let observed_jobs = Rc::new(RefCell::new(0_usize));
        let counter = observed_jobs.clone();
        let lifecycle_bytes = Rc::new(RefCell::new(Vec::<usize>::new()));
        let lifecycles = lifecycle_bytes.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(64);
        deferred_drop::controlled::with_backend(
            sender,
            move |value| {
                *counter.borrow_mut() += 1;
                if let Some(old) = value.downcast_ref::<Vec<NpcOrderLifecycle>>() {
                    lifecycles
                        .borrow_mut()
                        .push(old.capacity() * std::mem::size_of::<NpcOrderLifecycle>());
                }
            },
            || {
                for _ in 0..65 {
                    let shadow = session.clone_for_tick_shadow().unwrap();
                    session.commit_tick_shadow(shadow);
                }
            },
        );
        let total = *observed_jobs.borrow();
        assert_eq!(total % 65, 0, "每次 commit 的字段 Job 数必须一致");
        let per_commit = total / 65;
        assert!(
            per_commit >= 29,
            "每次 commit 必须移交完整字段集（≥29 Job；字段表 32——批四删除 PendingNpc），实际 {per_commit}"
        );
        assert_eq!(
            lifecycle_bytes.borrow().len(),
            65,
            "65 次 commit 每次恰一个 lifecycle 字段 Job"
        );
        assert!(
            lifecycle_bytes
                .borrow()
                .iter()
                .all(|&bytes| bytes >= 4096 * std::mem::size_of::<NpcOrderLifecycle>()),
            "旧 lifecycle 容器必须完整移交（4096 条容量）"
        );
        let jobs: Vec<_> = receiver.try_iter().collect();
        assert_eq!(
            jobs.len(),
            64,
            "积压上界=队列容量：65×{per_commit} 个 Job 中只有 64 个留在队列"
        );
        let enqueued = jobs.len();
        let inline_released = total - enqueued;
        assert!(inline_released >= 1, "满队列后其余 Job 必须就地释放");
        let before_drain = Arc::strong_count(&session.state.company_registry);
        assert!(
            before_drain > baseline_owners && before_drain <= baseline_owners + 64,
            "未析构旧 Arc 有界（≤队列容量 64）且非空"
        );
        // Astra 小修 a)：65 次尝试的 lifecycle 容量总计是"移交总量"而非"队列保留量"
        // （受控队列只持有最先入队的 64 个字段 Job，其中含 ~2 份 lifecycle Job）。
        let lifecycle_transferred_bytes_total: usize = lifecycle_bytes.borrow().iter().sum();
        let drain_start = std::time::Instant::now();
        for job in jobs {
            job();
        }
        let drain_ns = drain_start.elapsed().as_nanos();
        assert_eq!(
            Arc::strong_count(&session.state.company_registry),
            baseline_owners,
            "drain 后旧状态必须完全析构，无残留"
        );
        println!(
            "{}",
            serde_json::json!({"commits":65,"jobs_per_commit":per_commit,"attempted_jobs":total,"enqueued":enqueued,"inline":inline_released,"pending_before_drain":enqueued,"pending_after_drain":0,"lifecycle_transferred_bytes_total":lifecycle_transferred_bytes_total,"transferred_bytes_scope":"65 次 lifecycle 尝试容量总计（非队列保留量：队列仅持最先入队的 64 个字段 Job，含 ~2 份 lifecycle）","drain_ns":drain_ns})
        );
    }

    /// 修复1（Luna：CommitField 生产路径可达性）+ 修复2升级（Astra：字段集合级
    /// 完整性，取代纯计数绊线）。走真实 commit_from→defer_swap 路径断言：
    /// ①JobKind 分类为 CommitField 且计数 == COMMIT_FIELD_EXPECTED 全集大小；
    /// ②per-FieldId 计数的**集合相等**：一次 commit 的观测字段多重集恰为期望全集
    /// 每项一次——任何单字段 defer_swap→直接赋值的回退（反例：把 markets 改回
    /// `self.markets = markets;`，重容器回 commit 路径同步析构）必然红：Markets
    /// 的 per-FieldId 增量为 0 ≠ 1；纯计数绊线可被"删一个+换一个"绕过，集合
    /// 相等不能（被删字段的计数缺失即红）。给结构体加字段而不更新
    /// COMMIT_FIELD_EXPECTED 同样红（新字段观测到、期望集没有）。红绿反例
    /// 证据见 logs/red-green-astra-counterexample-{red,green}.log。
    /// ③CommitState 增量必须为 0（裁决 (d)：逐字段形态不存在整状态移交）。
    /// 期望集随 cfg 条件化（默认 33 / simulation-diagnostics 35，与 commit_from
    /// 条件 swap 对齐——Astra 增量四复检修复1）。已知盲区与如实边界见 FieldId
    /// 注释 b/c：自报标签可被错标绕过、绕过 defer_swap 的直接赋值新字段不被拦。
    /// 运行现状：verification-harness+simulation-diagnostics 同开在本链树上因
    /// 既有 simdiag 编译债务（16 处，全部位于 simdiag 专属代码，见增量二登记）
    /// 无法构建，该组合的绊线执行验证待该债务修复后补（logs/
    /// simdiag-both-features-check.log 为本轮实测证据）。
    /// 运行口径：本测试读全局 METRICS 的 snapshot（ledger 派生 by_kind），
    /// 必须以独立过滤运行（cargo test -p engine --lib --features
    /// verification-harness verify_metrics_classifies）：同模块的
    /// producer_registration_pairs_and_underflow_exit_faults_explicitly 会按
    /// 预期语义对全局台账落 sticky fault（code=5，加固冻结 limits 登记项），
    /// 同进程并发/先后运行都会使 snapshot 永久 Err，与本测试的正确性无关。
    #[test]
    #[cfg(all(feature = "verification-harness", not(target_family = "wasm")))]
    fn verify_metrics_classifies_real_defer_swap_fields_as_commit_field() {
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 723).unwrap();
        let before = verify_metrics::snapshot().expect("计量台账一致");
        session.step().unwrap();
        let after = verify_metrics::snapshot().expect("计量台账一致");
        let classified = after.by_kind[JobKind::CommitField as usize].registered
            - before.by_kind[JobKind::CommitField as usize].registered;
        assert_eq!(
            classified,
            COMMIT_FIELD_EXPECTED.len() as u64,
            "修复1：真实 defer_swap 路径必须全部计量为 CommitField"
        );
        let whole_states = after.by_kind[JobKind::CommitState as usize].registered
            - before.by_kind[JobKind::CommitState as usize].registered;
        assert_eq!(whole_states, 0, "裁决 (d)：不得出现整状态 Job");
        for expected in COMMIT_FIELD_EXPECTED {
            let delta = after.by_field[expected as usize] - before.by_field[expected as usize];
            assert_eq!(
                delta, 1,
                "修复2升级：字段 {expected:?} 的移交计数必须恰为 1（集合级完整性绊线）"
            );
        }
    }

    #[test]
    fn shadow_keeps_the_complete_save_projection_and_resets_facade_hooks() {
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 717).unwrap();
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::Cancel {
                    code: StockId(0),
                    id: OrderId(101),
                },
            )
            .unwrap();
        let expected = serde_json::to_value(session.save().unwrap()).unwrap();
        session.poison = Some(fatal("authority poison"));
        session.injected_failure = Some(fatal("authority injection"));
        session.post_shadow_failure = Some(fatal("authority post-shadow"));

        let shadow = session.clone_for_tick_shadow().unwrap();

        assert_eq!(
            serde_json::to_value(shadow.save().unwrap()).unwrap(),
            expected
        );
        assert_eq!(shadow.poison, None);
        assert_eq!(shadow.injected_failure, None);
        assert_eq!(shadow.post_shadow_failure, None);
        assert_eq!(session.state.pending_player.len(), 1);
        assert_eq!(session.poison, Some(fatal("authority poison")));
    }

    #[test]
    fn state_commit_installs_candidate_facts_and_keeps_authority_hooks() {
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 718).unwrap();
        session.shared_ingress();
        session
            .revision
            .store(17, std::sync::atomic::Ordering::Relaxed);
        let original_allocation = session.fresh_initial_allocation;
        let mut shadow = session.clone_for_tick_shadow().unwrap();
        shadow
            .revision
            .store(99, std::sync::atomic::Ordering::Relaxed);
        shadow.fresh_initial_allocation = !original_allocation;
        shadow
            .enqueue_player_intent(
                AccountId(0),
                Intent::Cancel {
                    code: StockId(0),
                    id: OrderId(102),
                },
            )
            .unwrap();
        shadow.state.rng.next_u64();
        shadow
            .state
            .last_retail_order_events
            .push(RetailOrderDiagnosticEvent::Canceled {
                account: AccountId(1),
                code: StockId(0),
                order_id: OrderId(103),
                remaining_qty: 100,
            });
        let expected = serde_json::to_value(shadow.save().unwrap()).unwrap();
        let expected_rng = shadow.state.rng.state;
        session.poison = Some(fatal("preserved poison"));
        session.injected_failure = Some(fatal("preserved injection"));
        session.post_shadow_failure = Some(fatal("preserved post-shadow"));

        session.commit_tick_shadow(shadow);

        // ingress 的具体 fork/cutoff/receipt 不重不丢由 shared_ingress 的真实输入
        // 测试验证；这里只断言外层 authority 的 revision/生命周期/故障钩子未被替换。
        assert_eq!(session.external_revision(), 17);
        assert_eq!(session.fresh_initial_allocation, original_allocation);
        assert_eq!(session.poison, Some(fatal("preserved poison")));
        assert_eq!(session.injected_failure, Some(fatal("preserved injection")));
        assert_eq!(
            session.post_shadow_failure,
            Some(fatal("preserved post-shadow"))
        );
        assert_eq!(session.state.rng.state, expected_rng);
        assert_eq!(session.state.last_retail_order_events.len(), 1);
        session.poison = None;
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            expected
        );
    }

    /// 后台创建失败是性能退化，不是交易提交失败：真实 step 成功、authority 安装完整。
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn real_step_commits_successfully_when_drop_thread_cannot_spawn() {
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 720).unwrap();
        let before_tick = session.tick();
        let before_cash = session.state.accounts[&AccountId(0)].cash();
        let (result, degradation) =
            deferred_drop::controlled::with_failed_backend(Some(11), || session.step());
        assert!(result.is_ok(), "defer 退化不能把已提交 tick 变成失败");
        assert_eq!(session.tick(), before_tick + 1);
        assert_eq!(session.state.accounts[&AccountId(0)].cash(), before_cash);
        assert!(session.poison.is_none());
        assert_eq!(
            degradation,
            Some(DeferredDropDegradation::SpawnFailed { os_errno: Some(11) })
        );
    }
}
