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
mod company_corrections;
mod company_groups;
#[cfg(test)]
mod company_mechanism_switch_tests;
#[cfg(test)]
mod company_simple_session_tests;
mod corporate_actions;
#[cfg(test)]
mod dividend_tax_mode_tests;
#[cfg(test)]
mod issuer_repurchase_session_tests;
#[cfg(test)]
mod rights_offering_session_tests;
mod exchange_calendar;
#[cfg(test)]
mod exchange_calendar_tests;
#[cfg(test)]
mod simple_preferences_session_tests;
pub use company_corrections::{
    CompanyReportCorrection, CompletedReportCorrection, ReportCorrectionEpoch,
    ReportCorrectionError, ReportCorrectionStatus,
};
mod company_operations;
mod continuous_cancellation;
mod decision_chain;
mod disclosures;
mod envelope_projection;
mod execution;
mod failure;
mod hash;
mod history_reads;
mod intraday_disclosures;
mod trade_confirmation_query;
pub use trade_confirmation_query::{
    PersonalTradeHistoryPage, PersonalTradeHistoryRequest, TradeHistoryReceiptCursor,
};
mod live_minute_history;
mod retained_history;
pub use live_minute_history::{
    CurrentMinuteHistoryPhase, CurrentMinuteHistoryRequest, CurrentMinuteHistoryResponse,
};
pub use retained_history::{
    HistorySessionStatus, HistoryTradingPhase, MarketHistoryAvailability, MarketHistoryEntry,
    MarketHistoryPage, MarketHistoryRequest, MinuteBar, MinuteHistorySession, RetainedHistoryDay,
};
mod shared_ingress;
pub use shared_ingress::SharedSessionIngress;
mod initial_allocation;
mod institutional_behavior;
mod memberships;
mod minimal_snapshot;
pub use memberships::{
    AdmissionFunding, MarketMembership, MarketMembershipState, MembershipError, OpaqueSubjectId,
};
#[cfg(test)]
mod memberships_tests;
mod observation_clock;
mod persistence;
pub mod pipeline;
mod plan_chain_candidates;
mod plan_execution;
mod player_candidates;
pub mod protocol;
mod self_views;
mod snapshot;
mod views;
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
mod financial_hash_contract_tests;
#[cfg(test)]
mod hash_contract_tests;
#[cfg(test)]
mod plan_chain_candidates_tests;
#[cfg(test)]
mod player_candidates_tests;
#[cfg(test)]
mod reconciliation_plan_phase_tests;
#[cfg(test)]
mod reconciliation_plan_tests;

use account_book::AccountBook;
use account_paged_map::AccountPagedMap;
use attention::NpcAttentionScheduler;
use candles::{
    generate_preset_daily_candles, stock_code_hash, DailyCandleHistory, SessionCandleBook,
};
use civil_clock::{default_civil_start_date, session_calendar_exchange};
use decision_chain::personal_state::BeliefParticipantState;
use persistence::{validate_save_slot, validate_saved_order_state};

pub use attention::NpcAttentionState;
mod notices;
pub use civil_clock::{
    CivilClock, CivilClockError, CivilClockSave, CivilDayEndReport, CivilPhase, DueBusiness,
    DueBusinessId, DueKind,
};
pub use company_groups::{GroupHolding, GroupStructure};
pub use company_operations::{CompanyOperationsClockWiring, CompanyOperationsSeamError};
pub use corporate_actions::{
    AppliedExReferenceGroup, ExternalDividendReceipt, SessionCorporateActions,
    SessionCorporateActionsError,
};
pub use decision_chain::{BeliefDebugSummary, DecisionChainDiagnostics};
pub use disclosures::{
    disclosure_phase_observer, DayEndDisclosureCtx, DayEndDisclosures, DisclosureDispatch,
    DisclosureError,
};
pub use execution::ParentOrderPlan;
pub use minimal_snapshot::{SaveAccountSnap, SaveMarketSnap, SaveSnapshot};
pub use notices::NpcInformationCadence;
pub use persistence::{
    decode_save_slot, SaveDecodeLimits, SavedEnvelopeKey, SavedFeeComponents, SavedJournalRank,
    SavedLiveEnvelope, SavedReceiptLocalKey, SavedReceiptSource, SavedReceiptTransition,
    SavedRetailReceiptIdentity, SavedRuntimeState, MAX_SAVE_DECODE_BYTES, SIMULATION_POLICY_ID,
};
pub use plan_execution::{
    PendingPlanEvent, PlanExecutionDisposition, PlanExecutionError, PlanExecutionReport,
    PlanExecutionRequest,
};
pub use snapshot::{AccountSnap, MarketSnap, PositionSnap, Snapshot};

use attention::{maximum_observation_probability, sample_attention_wait};

use crate::account::{Account, AccountError, AccountKind, StockCode};
#[cfg(test)]
use crate::behavior::BehaviorMarketObservation;
use crate::behavior::PositionDecision;
use crate::calendar::{CivilDate, CivilInstant, DayStatus, TradingCalendar};
use crate::company::CompanyId;
pub(crate) use crate::config::{buy_order_reservation, fee_delta};
use crate::config::{ConfigError, GameConfig};
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
use crate::strategy::Rng;
use crate::strategy::{
    Intent, LimitPrice, MarketView, StockView, StrategyError, StrategyFactory, StrategyParams,
    StrategyProfile,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

/// 机构母单在没有新目标修订时，最多跨越一个标准交易日。
/// 日终所有剩余子单和计划都会失效，绝不跨日沿用旧观点。
pub const PARENT_ORDER_HORIZON_MINUTES: u64 = GAME_INTRADAY_MINUTES_PER_DAY as u64;

/// SplitMix64：确定性 PRNG。种子化、可重放（同种子同序列）。
#[derive(Clone)]
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
    /// 本证券所属交易所当前自然日休市。
    ExchangeClosed,
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
    PublicTrade {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        code: StockCode,
        price: Money,
        qty: u32,
    },
    PrivateEventOmitted {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
    },
    /// 成交：code 来自路由（orderbook.Trade 无 code 字段），maker/taker 双方结算。
    Trade {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        code: StockCode,
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
        code: StockCode,
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
        code: StockCode,
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
        code: StockCode,
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
        closed_daily_candles: BTreeMap<StockCode, DailyCandle>,
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
        #[serde(with = "memberships::account_id_decimal")]
        #[ts(type = "string")]
        account: AccountId,
        code: StockCode,
        reason: RejectionReason,
    },
    /// 结算失败（账户侧异常，透传 AccountError 文案）。
    SettlementError {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        #[serde(with = "memberships::account_id_decimal")]
        #[ts(type = "string")]
        account: AccountId,
        code: StockCode,
        reason: String,
    },

    /// 连续竞价委托已撤销，冻结资金或股份随即释放。
    OrderCanceled {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        #[serde(with = "memberships::account_id_decimal")]
        #[ts(type = "string")]
        account: AccountId,
        code: StockCode,
        id: OrderId,
        remaining_qty: u32,
    },
    /// 限价委托有未成交余量进入连续竞价订单簿。
    OrderAccepted {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        #[serde(with = "memberships::account_id_decimal")]
        #[ts(type = "string")]
        account: AccountId,
        code: StockCode,
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
            | Self::PublicTrade { seq, .. }
            | Self::PrivateEventOmitted { seq, .. }
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
    #[serde(with = "crate::orderbook::canonical_u128_decimal")]
    #[ts(type = "string")]
    pub turnover_cents: u128,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub trade_count: u64,
}

/// 当前契约的状态槽。公共持久档仅接受完整日结；低层内存 checkpoint 另保留活动状态。
/// 真实分钟量价、日 K 和交割事实永久保留；前端展示采样不冒充真实成交历史。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SaveSlot {
    pub retained_market_history: Vec<RetainedHistoryDay>,
    pub market_memberships: MarketMembershipState,
    #[ts(skip)]
    pub report_correction_operations: BTreeMap<String, CompletedReportCorrection>,
    /// Escrow 并行 tick 的权威运行时状态。TypeScript 形状由 Web 严格存档
    /// parser 共同维护，避免把策略私有结构扩成通用宿主命令。
    #[ts(type = "import(\"../../save/schema/runtime-state\").SavedRuntimeState")]
    pub runtime_state: SavedRuntimeState,
    pub setup: SessionSetup,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub seed: u64,
    pub snapshot: SaveSnapshot,
    /// 日内存档恢复集合竞价所需的完整委托队列。
    pub auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    /// 连续竞价未成交委托。
    pub resting_orders: BTreeMap<StockCode, Vec<Order>>,
    /// 每股下一时间序。撤单和日界清簿不重置，不能从现存挂单推算。
    /// 十进制字符串保留完整 u64 游标，不受 JSON number 精度限制。
    #[serde(with = "book_sequence_map")]
    #[ts(type = "Record<string, string>")]
    pub book_next_sequences: BTreeMap<StockCode, u64>,
    /// 已全部成交的委托身份。撤旧单时据此区分已成交与未知/已撤，跨 tick 保留。
    pub filled_orders: BTreeMap<StockCode, Vec<FilledOrderSnap>>,
    /// 策略观察所需的短价格窗口。它会影响下一 tick 的决策，因此属于权威状态。
    pub price_history: BTreeMap<StockCode, Vec<Money>>,
    /// 当前交易日的标准交易分钟收盘快照。它会影响后续策略，因此属于权威状态；
    /// 到日界后清空，跨日窗口读取已经完成的日 K。
    pub market_minute_closes: BTreeMap<StockCode, Vec<MarketMinuteClose>>,
    /// 当前随机数生成器状态；用十进制字符串避免 JavaScript 丢失 u64 精度。
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub rng_state: u64,
    /// 每个 NPC 的权威注意力调度状态。独立随机流保证观察节奏可存档、可重放。
    pub npc_attention: BTreeMap<AccountId, NpcAttentionState>,
    /// 每个自然人散户由真实成交与观察形成的权威经历；机构、游资和玩家不得出现在此表。
    pub retail_experience: BTreeMap<AccountId, RetailExperienceState>,
    /// 机构策略已经形成、但尚未完全成交的母单执行计划。
    /// 目标和实际成交分开保存，读档后不会把未成交目标误作持仓。
    pub parent_orders: BTreeMap<AccountId, BTreeMap<StockCode, SaveParentOrderPlan>>,
    /// NPC 连续竞价普通限价单的可恢复主动撤单时间。
    pub npc_order_lifecycles: Vec<NpcOrderLifecycle>,
    /// 已被宿主确认入队、尚未在下一 tick 路由的玩家意图。
    pub pending_player: Vec<ReceiptBearingIntent>,
    /// 上一已提交版本生成、等待下一市场 tick 受理的 NPC 请求；日界处尚未生成。
    pub pending_npc: Option<PendingNpcBatch>,
    pub ingress_receipt_cursors: IngressReceiptCursors,
    /// 保持订单 id/到达序继续单调递增。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub next_order_id: u64,
    /// 自然日经营时钟权威状态；完整存档携带冻结的日历政策。
    pub civil_clock: CivilClockSave,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").SessionCorporateActions")]
    pub corporate_actions: SessionCorporateActions,
    /// ── 权威状态连续性（完整存档）：公司域与个体决策链权威状态。全部必填；缺失任一字段
    ///    的 JSON 不是当前 schema 的合法存档，走通用校验拒绝。──
    /// 新局选定的公司系统、财务事实与独立随机状态。
    #[ts(type = "import(\"../../save/schema/company/system\").CompanySystem")]
    pub company_system: crate::company::CompanySystem,
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
    /// 计划执行待应用事实队列（存档边界只保留「计划簿中仍存活」的条目；
    /// 未知/已终止计划的迟到条目按完整存档契约显式丢弃。
    #[ts(skip)]
    pub pending_plan_events: Vec<plan_execution::PendingPlanEvent>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SaveParentOrderPlan {
    pub code: StockCode,
    pub side: Side,
    pub target_qty: u32,
    pub filled_qty: u32,
    pub child_qty: u32,
    pub active_child_order_id: Option<OrderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub linked_plan_id: Option<crate::plans::PlanId>,
    pub limit_price: Money,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub expires_market_minute: u64,
}

impl From<&ParentOrderPlan> for SaveParentOrderPlan {
    fn from(plan: &ParentOrderPlan) -> Self {
        Self {
            code: plan.code().clone(),
            side: plan.side(),
            target_qty: plan.target_qty(),
            filled_qty: plan.filled_qty(),
            child_qty: plan.child_qty(),
            active_child_order_id: plan.active_child_order_id(),
            linked_plan_id: plan.linked_plan_id(),
            limit_price: plan.limit_price(),
            expires_market_minute: plan.expires_market_minute(),
        }
    }
}

impl From<SaveParentOrderPlan> for ParentOrderPlan {
    fn from(plan: SaveParentOrderPlan) -> Self {
        Self::from_saved_facts(
            plan.code,
            plan.side,
            plan.target_qty,
            plan.filled_qty,
            plan.child_qty,
            plan.active_child_order_id,
            None,
            plan.linked_plan_id,
            plan.limit_price,
            plan.expires_market_minute,
        )
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
    pub code: StockCode,
    pub order_id: OrderId,
    /// 委托进入连续竞价簿时的绝对标准交易分钟。
    pub placed_market_minute: u64,
    /// 到达该分钟后，由 NPC 经正常撤单生命周期撤回。
    pub expires_market_minute: u64,
}

pub(crate) mod u64_decimal {
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
        String::deserialize(deserializer)?
            .parse::<u64>()
            .map_err(serde::de::Error::custom)
    }
}

use crate::orderbook::canonical_u64_decimal;

mod book_sequence_map {
    use super::{u64_decimal, StockCode};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Cursor(#[serde(with = "u64_decimal")] u64);

    pub fn serialize<S>(value: &BTreeMap<StockCode, u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(|(code, cursor)| (code, Cursor(*cursor)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<StockCode, u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(BTreeMap::<StockCode, Cursor>::deserialize(deserializer)?
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
    use super::{canonical_u64_decimal, StockCode};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Cursor(#[serde(with = "canonical_u64_decimal")] u64);

    pub fn serialize<S>(value: &BTreeMap<StockCode, u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(|(code, cursor)| (code, Cursor(*cursor)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<StockCode, u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(BTreeMap::<StockCode, Cursor>::deserialize(deserializer)?
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
pub struct PendingNpcBatch {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub observed_tick: u64,
    pub observed_accounts: Vec<AccountId>,
    pub intents: Vec<ReceiptBearingIntent>,
    /// Reconciliation cancellation -> replacement admission, by intent position.
    pub dependencies: Vec<(usize, usize)>,
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
    pub next_stock_ordinal: BTreeMap<StockCode, u64>,
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
            SessionError::ResourceLimit(format!("account {owner:?} ingress ordinal overflow"))
        })?;
        let stock_ordinal = self.next_stock_ordinal.get(&code).copied().unwrap_or(0);
        let stock_next = stock_ordinal.checked_add(1).ok_or_else(|| {
            SessionError::ResourceLimit(format!("stock {code:?} ingress ordinal overflow"))
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

impl PendingNpcBatch {
    pub(super) fn validate_dependencies(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for &(before, after) in &self.dependencies {
            let prefix = format!("pending NPC dependency {before}->{after}");
            if before >= after || after >= self.intents.len() {
                return Err(format!(
                    "{prefix} must reference earlier and later queued intents"
                ));
            }
            if !seen.insert((before, after)) {
                return Err(format!("{prefix} is duplicated"));
            }
            let before = &self.intents[before];
            let after = &self.intents[after];
            let Intent::Cancel {
                code: before_code, ..
            } = &before.intent
            else {
                return Err(format!("{prefix} predecessor is not a cancellation"));
            };
            let after_code = match &after.intent {
                Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } => code,
                Intent::Cancel { .. } => {
                    return Err(format!("{prefix} successor is not a placement"));
                }
            };
            if before.owner != after.owner || before_code != after_code {
                return Err(format!(
                    "{prefix} must belong to the same account and stock"
                ));
            }
        }
        Ok(())
    }
}

type ContinuousOrdersByAccount = BTreeMap<AccountId, Vec<(StockCode, Order)>>;
type AuctionOrdersByAccount = BTreeMap<AccountId, Vec<(StockCode, AuctionOrderSnap)>>;

enum ReconcileScope {
    AllWorkingOrders,
    ReviewedStocks(BTreeSet<StockCode>),
}

#[derive(Clone, Copy)]
struct WorkingOrderSlices<'a> {
    continuous: &'a [(StockCode, Order)],
    auction: &'a [(StockCode, AuctionOrderSnap)],
}

/// session 操作失败（致命：构造非法 / 未知玩家）。绝不静默吞错（铁律二）。
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("报表更正失败：{0}")]
    ReportCorrection(#[source] ReportCorrectionError),
    #[error("报表更正内部不变量失败：{0}")]
    CorrectionInvariant(#[source] Box<crate::company::CompanyCorrectionError>),
    #[error(transparent)]
    Step(#[from] StepFatal),
    /// 构造参数非法（stocks 空 / ticks_per_day==0 等）。
    #[error("invalid setup: {0}")]
    InvalidSetup(String),
    /// 入队意图时玩家 id 不存在。
    #[error("unknown player: {0:?}")]
    UnknownPlayer(AccountId),
    /// 入队意图时账户存在，但它是引擎控制的 NPC，不接受玩家指令。
    #[error("account is not controlled by a player: {0:?}")]
    NotPlayer(AccountId),
    #[error("unknown account for history request: {0:?}")]
    UnknownHistoryAccount(AccountId),
    #[error("unknown stock for history request: {0:?}")]
    UnknownHistoryStock(StockCode),
    #[error("intraday average could not be calculated: {0}")]
    InvalidIntradayAverage(String),
    #[error("本人交割历史查询无效：{0}")]
    InvalidTradeHistoryQuery(String),
    #[error("history read could not be recorded: {0}")]
    InvalidHistoryRead(String),
    #[error("session resource limit exceeded: {0}")]
    ResourceLimit(String),
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
    /// 存档 schema 或领域不变量不合法。
    #[error("invalid save: {0}")]
    InvalidSave(String),
    /// 透传交易日历错误（冻结日历与双时钟 开局日期门等）。
    #[error(transparent)]
    Calendar(#[from] crate::calendar::CalendarError),
    /// 透传自然日时钟错误（冻结日历与双时钟 日结验证/原子失败）。
    #[error(transparent)]
    CivilClock(#[from] CivilClockError),
    /// 透传公司经营接线错误（会话装配与执行接线 日终编排）。
    #[error(transparent)]
    CompanyOperations(#[from] company_operations::CompanyOperationsSeamError),
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
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
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SessionSetup {
    pub stocks: Vec<StockSpec>,
    #[ts(type = "import(\"../../save/schema/company/system\").CompanySystemConfig")]
    pub company_system: crate::company::config::CompanySystemConfig,
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
    pub report_frequency: crate::information::ReportFrequency,
    /// 开局自然日。缺省为政策默认 2030-01-01；合法开局 2000-01-01..2099-12-31
    /// （1998–1999 仅供初始化前史查询）。休市起点保持原日，不挪到开市日。
    /// serde 缺省仅供宿主过渡期不发送该字段时使用；存档总是显式写出。
    #[serde(default = "default_civil_start_date")]
    pub start_date: crate::calendar::CivilDate,
    /// 模拟政策身份：本引擎当前行为契约（A 股交易语义、公司域、
    /// 决策链参数族）的稳定版本标识。新档必填；与存档一起固化，恢复时不
    /// 与任何“最新默认”比对或迁移——身份不匹配的档由宿主层拒绝。
    pub simulation_policy_id: String,
    /// 新局现金分红税务模式（2026-10-06 产品决策）：默认大 A 个人差别化
    /// （装配期自动为个人身份账户配置 `IndividualPublicMarket` 税账），
    /// 可选不扣税。严格持久化字段：新档必填、无 serde 默认，缺失该字段的
    /// 旧档被显式拒绝；恢复后自动配置与计税语义不变。
    pub dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode,
    /// 新局「配股／增发」机制开关（2026-10-07 产品决策，ADR-0039）：
    /// 关闭（UI 默认）时配股／增发机制不触发，显式 API 调用被显式拒绝
    /// （错误指明本局未启用）。严格持久化字段：新档必填、无 serde 默认，
    /// 缺失该字段的旧档被显式拒绝；恢复后开关语义不变。
    pub rights_offering_enabled: bool,
    /// 新局「发行人回购」机制开关（2026-10-07 产品决策，ADR-0038）：
    /// 独立于配股／增发开关，默认关闭。关闭时回购机制不触发，显式 API
    /// 调用被显式拒绝（错误指明本局未启用）。严格持久化字段：新档必填、
    /// 无 serde 默认，缺失该字段的旧档被显式拒绝；恢复后开关语义不变。
    pub issuer_repurchase_enabled: bool,
}

impl SessionSetup {
    /// 校验所有启动期外部输入。serde 可绕过各子类型构造器，因此 session 创建和恢复
    /// 都必须从这里进入，验证成功后才构造权威状态。
    pub fn validate(&self) -> Result<(), SessionError> {
        let issuers =
            crate::company::identity::IssuerRegistry::new(company_assembly::issuer_specs(self)?)
                .map_err(|error| SessionError::InvalidSetup(format!("发行人身份非法：{error}")))?;
        match &self.company_system {
            crate::company::config::CompanySystemConfig::Simple(config) => config
                .validate_for_issuers(&issuers)
                .map_err(|error| SessionError::InvalidSetup(format!("Simple 参数非法：{error}")))?,
            crate::company::config::CompanySystemConfig::Simulation => {
                return Err(SessionError::InvalidSetup(
                    "Simulation 将在独立分支实现，当前不能创建".into(),
                ))
            }
        }
        if let crate::information::ReportFrequency::Monthly { schedule } = self.report_frequency {
            schedule
                .validate()
                .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        }
        if self.stocks.is_empty() {
            return Err(SessionError::InvalidSetup(
                "stocks must be non-empty".to_string(),
            ));
        }
        if self.simulation_policy_id != SIMULATION_POLICY_ID {
            return Err(SessionError::InvalidSetup(format!(
                "simulation_policy_id 必须为当前政策 {SIMULATION_POLICY_ID:?}，实际为 {:?}",
                self.simulation_policy_id
            )));
        }
        // 冻结日历与双时钟 开局日期门：运行区间 2000-01-01..2099-12-31；1998–1999 仅供前史。
        TradingCalendar::current_default_calendar()?.validate_runtime_start(self.start_date)?;
        if self.ticks_per_day == 0 {
            return Err(SessionError::InvalidSetup(
                "ticks_per_day must be > 0".to_string(),
            ));
        }
        if self.auction_ticks >= self.ticks_per_day {
            return Err(SessionError::InvalidSetup(format!(
                "auction_ticks ({}) must be < ticks_per_day ({})",
                self.auction_ticks, self.ticks_per_day
            )));
        }
        if self
            .auction_ticks
            .checked_add(self.closing_auction_ticks)
            .is_none_or(|total| total >= self.ticks_per_day)
        {
            return Err(SessionError::InvalidSetup(format!(
                "auction_ticks ({}) + closing_auction_ticks ({}) must be < ticks_per_day ({})",
                self.auction_ticks, self.closing_auction_ticks, self.ticks_per_day
            )));
        }
        if self.history_len == 0 {
            return Err(SessionError::InvalidSetup(
                "history_len must be > 0".to_string(),
            ));
        }
        if self.npcs.retail_cash_median.cents() < 0 {
            return Err(SessionError::InvalidSetup(
                "retail_cash_median must be >= 0".to_string(),
            ));
        }
        self.config.validate()?;
        if !self.t1_enabled {
            return Err(SessionError::InvalidSetup(
                "formal A-share sessions require T+1 settlement".to_string(),
            ));
        }
        if (self.config.stamp_tax_rate - 0.0005).abs() > f64::EPSILON {
            return Err(SessionError::InvalidSetup(format!(
                "formal A-share sessions require sell stamp_tax_rate=0.0005, got {}",
                self.config.stamp_tax_rate
            )));
        }
        if (self.config.default_limit - 0.10).abs() > f64::EPSILON
            || (self.config.st_limit - 0.10).abs() > f64::EPSILON
        {
            return Err(SessionError::InvalidSetup(
                "formal A-share sessions require default_limit=10% and st_limit=10%".to_string(),
            ));
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
                return Err(SessionError::InvalidSetup(format!(
                    "{name}={quantity} must be a multiple of A-share lot_size={lot_size}"
                )));
            }
        }
        let mut codes = BTreeSet::new();
        for stock in &self.stocks {
            if !codes.insert(stock.code.clone()) {
                return Err(SessionError::InvalidSetup(format!(
                    "duplicate stock code: {}",
                    stock.code.0
                )));
            }
            if stock.tick != Money::from_cents(1) {
                return Err(SessionError::InvalidSetup(format!(
                    "stock {} must use the A-share 0.01 yuan tick",
                    stock.code.0
                )));
            }
            if stock.total_shares == 0 {
                return Err(SessionError::InvalidSetup(format!(
                    "stock {} total_shares must be > 0",
                    stock.code.0
                )));
            }
            if u64::from(stock.float_shares) > stock.total_shares {
                return Err(SessionError::InvalidSetup(format!(
                    "stock {} float_shares={} must not exceed total_shares={}",
                    stock.code.0, stock.float_shares, stock.total_shares
                )));
            }
            let inferred_exchange = StockExchange::from_a_share_code(&stock.code)
                .map_err(SessionError::InvalidSetup)?;
            if stock.exchange != inferred_exchange {
                return Err(SessionError::InvalidSetup(format!(
                    "stock {} is assigned to {:?}, but its code belongs to {:?}",
                    stock.code.0, stock.exchange, inferred_exchange
                )));
            }
            let is_chinext_code = matches!(&stock.code.0[..3], "300" | "301");
            if (stock.category == SecurityCategory::ChiNext) != is_chinext_code {
                return Err(SessionError::InvalidSetup(format!(
                    "stock {} category {:?} conflicts with its board code",
                    stock.code.0, stock.category
                )));
            }
            if (stock.limit_pct - stock.category.limit_pct()).abs() > f64::EPSILON {
                return Err(SessionError::InvalidSetup(format!(
                    "stock {} category {:?} requires a {} price limit",
                    stock.code.0,
                    stock.category,
                    stock.category.display_limit()
                )));
            }
        }
        if let BetweenKindDistribution::Percentage { retail, inst, hot } =
            &self.float_allocation.between_kinds
        {
            for (value, name) in [(*retail, "retail"), (*inst, "inst"), (*hot, "hot")] {
                if !value.is_finite() || value < 0.0 {
                    return Err(SessionError::InvalidSetup(format!(
                        "float_allocation between_kinds Percentage {name}={value} invalid (must be finite >=0)"
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
                    return Err(SessionError::InvalidSetup(
                        "float_allocation between_kinds Percentage weights for existing NPC kinds must have a finite sum > 0"
                            .to_string(),
                    ));
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
    report_correction_epoch: std::sync::Arc<()>,
    ingress: Option<shared_ingress::IngressBinding>,
    poison: Option<StepFatal>,
    fresh_initial_allocation: bool,
    #[cfg(test)]
    injected_failure: Option<StepFatal>,
    #[cfg(test)]
    post_shadow_failure: Option<StepFatal>,
    state: CommittableSessionState,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalTradeConfirmation {
    #[serde(with = "canonical_u64_decimal")]
    #[ts(type = "string")]
    pub receipt_id: u64,
    pub civil_date: CivilDate,
    pub code: StockCode,
    pub side: Side,
    pub price: Money,
    pub quantity_shares: u32,
    pub gross: Money,
    pub actual_fees: pipeline::FeeComponents,
}

/// tick shadow 与自然日日结共同提交的唯一状态集合。
struct CommittableSessionState {
    retained_market_history: retained_history::RetainedMarketHistory,
    memberships: MarketMembershipState,
    pending_report_corrections: Vec<CompanyReportCorrection>,
    report_correction_operations: BTreeMap<String, CompletedReportCorrection>,
    setup: SessionSetup,
    rng: SplitMix64,
    seed: u64,
    markets: BTreeMap<StockCode, Market>,
    accounts: AccountBook,
    history_reads: AccountPagedMap<crate::experience::PersonalHistoryReadLedger>,
    price_history: BTreeMap<StockCode, VecDeque<Money>>,
    market_minute_closes: BTreeMap<StockCode, Vec<MarketMinuteClose>>,
    candle_book: SessionCandleBook,
    auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    pending_player: Vec<ReceiptBearingIntent>,
    pending_npc: Option<PendingNpcBatch>,
    ingress_receipt_cursors: IngressReceiptCursors,
    npc_attention: AccountPagedMap<NpcAttentionState>,
    retail_experience: AccountPagedMap<RetailExperienceState>,
    parent_orders: BTreeMap<AccountId, BTreeMap<StockCode, ParentOrderPlan>>,
    pending_plan_events: Vec<plan_execution::PendingPlanEvent>,
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
    company_system: std::sync::Arc<crate::company::CompanySystem>,
    corporate_actions: SessionCorporateActions,
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
    personal_trade_confirmations:
        BTreeMap<AccountId, crate::experience::AppendOnlyHistory<PersonalTradeConfirmation>>,
    /// 下一个全局回执索引；跨存档由 `SavedRuntimeState` 的 `next_receipt_base` 持久恢复。
    next_receipt_base: u64,
    next_order_id: u64,
    tick: u64,
    day: u32,
    seq: u64,
    /// 冻结日历与双时钟 自然日经营时钟：与 tick/交易日计数分离的权威自然日推进。
    civil_clock: CivilClock,
}

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
        code: StockCode,
        side: Side,
        order_id: OrderId,
        qty: u32,
    },
    Filled {
        account: AccountId,
        code: StockCode,
        side: Side,
        order_id: OrderId,
        qty: u32,
    },
    Canceled {
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        remaining_qty: u32,
    },
    /// 集合竞价原子提交失败后未写入连续订单簿的剩余委托；它不是“仍在簿中”。
    Aborted {
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        remaining_qty: u32,
    },
    Rejected {
        account: AccountId,
        code: StockCode,
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
        // 玩家与回购账户不从该采样入口生成现金（各自由显式配置/合成入账）。
        AccountKind::Player | AccountKind::IssuerRepurchase => (1.0, 0.0),
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
        return Err(SessionError::InvalidSetup(format!(
            "initial cash overflow for {kind:?} account {}",
            account.0
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
    pub(super) fn clone_for_tick_shadow(&self) -> Result<Self, StepFatal> {
        Ok(Self {
            report_correction_epoch: self.report_correction_epoch.clone(),
            ingress: self
                .ingress
                .as_ref()
                .map(shared_ingress::IngressBinding::fork)
                .transpose()
                .map_err(shared_ingress::ingress_fatal)?,
            poison: None,
            fresh_initial_allocation: self.fresh_initial_allocation,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
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
        Self::new_with_company_event_multiplier(setup, seed, 10_000)
    }

    /// Creates a fresh session with a validated diagnostic-only company-event multiplier.
    /// The multiplier is not saved and normal construction always uses 10,000 bp.
    pub fn new_with_company_event_multiplier(
        setup: SessionSetup,
        seed: u64,
        event_multiplier_bp: u16,
    ) -> Result<GameSession, SessionError> {
        if !matches!(event_multiplier_bp, 5_000 | 10_000 | 20_000) {
            return Err(SessionError::InvalidSetup(format!(
                "company event multiplier must be 5000, 10000, or 20000 bp, got {event_multiplier_bp}"
            )));
        }
        setup.validate()?;
        let mut markets = BTreeMap::new();
        let mut price_history = BTreeMap::new();
        let mut market_minute_closes = BTreeMap::new();
        for s in &setup.stocks {
            markets.insert(
                s.code.clone(),
                Market::new(s.code.clone(), s.initial_price, s.limit_pct, s.tick)?,
            );
            price_history.insert(s.code.clone(), VecDeque::new());
            market_minute_closes.insert(s.code.clone(), Vec::new());
        }
        let daily_candles = generate_preset_daily_candles(&setup, seed)?;
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
        let civil_clock = CivilClock::new_for_stocks(
            setup.start_date,
            setup.stocks.iter().map(|stock| {
                (
                    stock.code.clone(),
                    session_calendar_exchange(stock.exchange),
                )
            }),
        )?;
        // 会话装配与执行接线 新局装配：公司注册表 + 前史经营 + 公开库 + 时钟/披露接线。
        let company_assembly::CompanyAssembly { system, library } =
            company_assembly::assemble_companies(&setup, seed)?;
        let seeded_through = library.latest_published_instant();
        let mut civil_clock = civil_clock;
        let disclosures = DisclosureDispatch::new(seeded_through);
        disclosures.install(&mut civil_clock);
        let issuer_repurchase_enabled_at_creation = setup.issuer_repurchase_enabled;
        let issuer_repurchase_npc_count_at_creation = u64::from(setup.npcs.retail_count)
            + u64::from(setup.npcs.inst_count)
            + u64::from(setup.npcs.hot_count);
        let mut sess = GameSession {
            report_correction_epoch: std::sync::Arc::new(()),
            ingress: None,
            poison: None,
            fresh_initial_allocation: true,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
            state: CommittableSessionState {
                memberships: MarketMembershipState::local_owner(setup.config.starting_cash),
                pending_report_corrections: Vec::new(),
                report_correction_operations: BTreeMap::new(),
                #[cfg(feature = "simulation-diagnostics")]
                causal: crate::diagnostics::causal::CausalCollector::default(),
                setup,
                rng,
                seed,
                markets,
                accounts,
                history_reads,
                price_history,
                market_minute_closes,
                candle_book: SessionCandleBook::new(daily_candles, BTreeMap::new()),
                retained_market_history: retained_history::RetainedMarketHistory::default(),
                auction_orders: BTreeMap::new(),
                pending_player: Vec::new(),
                pending_npc: None,
                ingress_receipt_cursors: IngressReceiptCursors::default(),
                npc_attention: AccountPagedMap::default(),
                retail_experience: AccountPagedMap::default(),
                parent_orders: BTreeMap::new(),
                pending_plan_events: Vec::new(),
                npc_order_lifecycles: Vec::new(),
                last_retail_decisions: Vec::new(),
                last_retail_order_events: Vec::new(),
                #[cfg(feature = "simulation-diagnostics")]
                npc_decision_traces:
                    crate::diagnostics::decision_trace::NpcDecisionTraceCollector::default(),
                attention_scheduler: NpcAttentionScheduler::default(),
                company_system: std::sync::Arc::new(system),
                corporate_actions: SessionCorporateActions::default(),
                library: std::sync::Arc::new(library),
                disclosures,
                plans: crate::plans::PlanBook::default(),
                urgency_policy: crate::plans::UrgencyPolicy::default(),
                belief_participants: AccountPagedMap::default(),
                envelope_ledger: pipeline::EnvelopeLedger::new(0, [])
                    .expect("an empty envelope ledger is valid"),
                retail_projection_seen: pipeline::RetailProjectionSeen::default(),
                personal_trade_confirmations: BTreeMap::new(),
                next_receipt_base: 0,
                next_order_id: 1,
                tick: 0,
                day: 0,
                seq: 0,
                civil_clock,
            },
        };
        sess.populate_npcs(AccountKind::Retail)?;
        sess.populate_npcs(AccountKind::Inst)?;
        sess.populate_npcs(AccountKind::Hot)?;
        if issuer_repurchase_enabled_at_creation {
            // 发行人回购专用账户（ADR-0038）：确定性创建于 NPC 序列之后，零现金
            // 起步；获批回购计划时按额度合成入账。名册身份为 IssuerTreasury。
            let npc_count = issuer_repurchase_npc_count_at_creation;
            let repurchase_id = AccountId(npc_count.saturating_add(1));
            sess.accounts_insert_with_history_reads(
                repurchase_id,
                Account::new(repurchase_id, AccountKind::IssuerRepurchase, Money::ZERO),
            );
        }
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
                        return Err(SessionError::InvalidSave(format!(
                            "institution account {} held stock {} has unknown fee history",
                            account_id.0, code.0
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
            let code = spec.code.clone();
            let price = spec.initial_price;
            let allocation = self.state.setup.float_allocation.clone();
            let alloc = self.split_float(spec.float_shares, &npc_ids, &allocation);
            for (id, qty) in alloc {
                if qty > 0 {
                    if let Some(acc) = self.state.accounts.get_mut(&id) {
                        acc.grant_position(code.clone(), qty, price)?;
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
                        AccountKind::Player | AccountKind::IssuerRepurchase => 3.0,
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

    /// 插入账户并同步建立历史读取台账（存档契约要求两集合恰好一致）。
    fn accounts_insert_with_history_reads(&mut self, id: AccountId, account: Account) {
        self.state.accounts.insert(id, account);
        self.state.history_reads.insert(
            id,
            crate::experience::PersonalHistoryReadLedger::default(),
        );
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
            AccountKind::Player | AccountKind::IssuerRepurchase => 0,
        };
        let first_id = self
            .state
            .accounts
            .keys()
            .next_back()
            .map_or(1, |id| id.0 + 1);
        let end_id = first_id.checked_add(u64::from(count)).ok_or_else(|| {
            SessionError::InvalidSetup("NPC account id range overflow".to_string())
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
                    SessionError::InvalidSetup("NPC ordinal exceeds u32".to_string())
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
                    .map_err(|error| {
                        SessionError::InvalidSetup(format!(
                            "generated NPC strategy state is invalid: {error}"
                        ))
                    })?;
                strategy_state.set_base_observation_probability(base_probability);
                let s = strategy_state.into_strategy().map_err(|error| {
                    SessionError::InvalidSetup(format!(
                        "canonical NPC strategy state is invalid: {error}"
                    ))
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

    /// 配置某只证券的完整股东登记事实；账户、外部股东及股份来源必须由调用方明确提供。
    /// 默认税务模式（大 A 个人差别化）下，登记成功后立即为每个「个人」身份的账户持有人
    /// （玩家与自然人散户 NPC）走既有 `configure_cash_dividend_tax_book` 自动配置
    /// `IndividualPublicMarket` 税账；机构/游资保持 `TreatmentNotConfigured`
    /// （企业/机构税未实现）。名册与税账在同一候选副本上落账，任一步失败都不留半配置状态。
    pub fn configure_share_registry(
        &mut self,
        registry: crate::company::share_registry::ShareRegistry,
    ) -> Result<(), SessionCorporateActionsError> {
        let positions = self
            .state
            .accounts
            .iter()
            .map(|(id, account)| {
                (
                    *id,
                    account
                        .positions()
                        .iter()
                        .map(|(code, position)| (code.clone(), u64::from(position.qty())))
                        .collect(),
                )
            })
            .collect();
        let mut candidate = self.state.corporate_actions.clone();
        candidate.configure_registry(
            registry.clone(),
            &positions,
            self.state.company_system.issuers(),
        )?;
        if self.state.setup.dividend_tax_mode
            == crate::company::cash_dividend_tax::CashDividendTaxMode::IndividualPublicMarket
        {
            let stock = registry.stock().clone();
            let personal_accounts: Vec<AccountId> = registry
                .holdings()
                .iter()
                .filter_map(|holding| {
                    let crate::company::share_registry::HolderId::Account(account) =
                        &holding.holder
                    else {
                        return None;
                    };
                    Some(*account)
                })
                .filter(|account| {
                    matches!(
                        corporate_actions::taxpayer_identity_of_kind(
                            self.state
                                .accounts
                                .get(account)
                                .expect("configure_registry 已验证名册持有人账户存在")
                                .kind()
                        ),
                        corporate_actions::TaxpayerIdentity::Personal
                    )
                })
                .collect();
            for account in personal_accounts {
                candidate.configure_cash_dividend_tax_book(
                    account,
                    stock.clone(),
                    crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
                )?;
            }
        }
        self.state.corporate_actions = candidate;
        Ok(())
    }

    /// 查询单账户的现金分红税务状态：会话税务模式、按账户种类映射的纳税人身份
    /// 与每个已配置完整名册证券上的税账状态。只读汇总既有事实，不产生新事实；
    /// 未知账户显式报错，不静默返回空视图。
    pub fn account_dividend_tax_status(
        &self,
        account: AccountId,
    ) -> Result<corporate_actions::AccountDividendTaxStatusView, SessionError> {
        let identity = corporate_actions::taxpayer_identity_of_kind(
            self.state
                .accounts
                .get(&account)
                .map(Account::kind)
                .ok_or_else(|| {
                    SessionError::InvalidSetup(format!("查询股息税状态的账户 {account:?} 不存在"))
                })?,
        );
        let stocks =
            self.state
                .corporate_actions
                .registries
                .iter()
                .map(
                    |registry| corporate_actions::AccountStockDividendTaxStatus {
                        stock: registry.stock().clone(),
                        status: if self.state.corporate_actions.dividend_tax_books.iter().any(
                            |book| book.account() == account && book.stock() == registry.stock(),
                        ) {
                            corporate_actions::DividendTaxStatus::IndividualPublicMarket
                        } else {
                            corporate_actions::DividendTaxStatus::TreatmentNotConfigured
                        },
                    },
                )
                .collect();
        Ok(corporate_actions::AccountDividendTaxStatusView {
            mode: self.state.setup.dividend_tax_mode,
            identity,
            stocks,
        })
    }

    /// 显式配置账户在指定证券下的现金分红税务身份；调用方不得由账户类型或策略风格推断。
    /// 仅限会话装配期调用：名册已有历史日结回执或已登记分红时会拒绝，
    /// 因为事后配置无法重建 FIFO 税事实并会令后续日终永久失败。
    pub fn configure_cash_dividend_tax_book(
        &mut self,
        account: AccountId,
        stock: crate::account::StockCode,
        profile: crate::company::cash_dividend_tax::DividendTaxProfile,
    ) -> Result<(), SessionCorporateActionsError> {
        self.state
            .corporate_actions
            .configure_cash_dividend_tax_book(account, stock, profile)
    }

    /// 查询各账户证券的个人现金分红税未划收税额与资金不足原因；
    /// 只读汇总税账既有事实，不产生新事实，UI 呈现由后续批次接线。
    pub fn dividend_tax_outstanding_views(
        &self,
    ) -> Result<Vec<corporate_actions::DividendTaxOutstandingView>, SessionCorporateActionsError>
    {
        self.state
            .corporate_actions
            .dividend_tax_outstanding_views()
    }

    /// 显式绑定公司注册资本及其来源证据；不会从股本或账户持仓推断法定事实。
    pub fn define_dividend_legal_facts(
        &mut self,
        company: &crate::company::CompanyId,
        registered_capital: crate::accounting::AccountingAmount,
        source_evidence: String,
    ) -> Result<(), SessionCorporateActionsError> {
        let mut candidate = self.state.company_system.as_ref().clone();
        candidate
            .define_dividend_legal_facts(company, registered_capital, source_evidence)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        self.state.company_system = std::sync::Arc::new(candidate);
        Ok(())
    }

    /// 当前会话共同股东登记与分红结算状态；空登记表表示尚未配置完整股份来源事实。
    pub fn corporate_actions(&self) -> &SessionCorporateActions {
        &self.state.corporate_actions
    }

    /// 以 SimpleFinanceState 的已结算财务与显式注册资本事实受理现金分红决议。
    pub fn approve_cash_dividend(
        &mut self,
        declaration: crate::company::DividendDeclaration,
        plan: crate::company::cash_dividend::CashDividendPlan,
    ) -> Result<(), SessionCorporateActionsError> {
        if declaration.plan_id != plan.plan_id
            || declaration.approved_on != plan.approved_on
            || plan.approved_on > self.civil_date()
            || plan.announced_on < self.civil_date()
        {
            return Err(SessionCorporateActionsError::Invalid(
                "分红决议与计划的身份、批准日期、授权总额或公告日期不一致".into(),
            ));
        }
        let registry = self
            .state
            .corporate_actions
            .registries
            .iter()
            .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid("现金分红需要已显式配置的完整股东名册".into())
            })?;
        if registry.settled_on() > self.civil_date() {
            return Err(SessionCorporateActionsError::Invalid(
                "股东名册日期晚于当前会话日期".into(),
            ));
        }
        let eligible_shares = registry
            .holdings()
            .iter()
            .filter(|holding| {
                !matches!(
                    &holding.holder,
                    crate::company::share_registry::HolderId::IssuerTreasury
                )
            })
            .try_fold(0_u64, |total, holding| {
                holding
                    .lots
                    .iter()
                    .try_fold(total, |sum, lot| sum.checked_add(lot.qty))
            })
            .ok_or_else(|| SessionCorporateActionsError::Invalid("可分红股数溢出".into()))?;
        let calculated_gross_cents = i128::from(plan.gross_per_share.cents())
            .checked_mul(i128::from(eligible_shares))
            .and_then(|cents| i64::try_from(cents).ok())
            .ok_or_else(|| SessionCorporateActionsError::Invalid("现金分红总额溢出".into()))?;
        if declaration
            .total_gross
            .to_money()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
            != Money::from_cents(calculated_gross_cents)
        {
            return Err(SessionCorporateActionsError::Invalid(
                "决议总额必须等于完整名册中非库存股的税前每股金额乘以股数".into(),
            ));
        }
        if declaration
            .total_gross
            .to_money()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
            > plan.distributable_amount
        {
            return Err(SessionCorporateActionsError::Invalid(
                "决议总额超过计划明确授权的可分配金额".into(),
            ));
        }
        if plan.formula
            == crate::company::ex_reference_price::CashDividendFormula::ExchangeApprovedAdjustment
        {
            return Err(SessionCorporateActionsError::Invalid(
                "尚不支持交易所批准的特殊除息调整公式".into(),
            ));
        }
        let mut candidate_system = self.state.company_system.as_ref().clone();
        let issuer = candidate_system
            .issuers()
            .get(&plan.issuer)
            .ok_or_else(|| SessionCorporateActionsError::Invalid("分红计划发行人不存在".into()))?;
        if issuer.listed_stock.as_ref() != Some(&plan.stock) {
            return Err(SessionCorporateActionsError::Invalid(
                "分红计划证券与发行人不匹配".into(),
            ));
        }
        let approved = candidate_system
            .distributable_profit(&plan.issuer)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        let authorized = approved
            .available_for_distribution
            .to_money()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if declaration
            .total_gross
            .to_money()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
            > authorized
        {
            return Err(SessionCorporateActionsError::Invalid(
                "决议总额超过已核定的 Simple 可分配利润".into(),
            ));
        }
        plan.validate_calendar(self.state.civil_clock.calendar())
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        candidate_system
            .declare_dividend(&plan.issuer, declaration)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        let book = crate::company::cash_dividend::CashDividendBook::new(plan)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if self
            .state
            .corporate_actions
            .dividends
            .iter()
            .any(|existing| existing.plan().plan_id == book.plan().plan_id)
        {
            return Err(SessionCorporateActionsError::Invalid(
                "分红计划 id 已存在".into(),
            ));
        }
        self.state.company_system = std::sync::Arc::new(candidate_system);
        self.state.corporate_actions.dividends.push(book);
        self.state
            .corporate_actions
            .dividends
            .sort_by(|left, right| left.plan().plan_id.cmp(&right.plan().plan_id));
        Ok(())
    }

    /// 以 SimpleFinanceState 的显式注册资本面值事实受理送转方案（显式计划入口）。
    ///
    /// 面值口径遵循现实语义：每股面值恒定，送转入账后注册资本按 面值×新增股数
    /// 演进（见 `SimpleFinanceState::record_stock_distribution_credit`）。首次送转
    /// 声明的面值由「已绑定注册资本法定事实 ÷ 名册当前已发行股数」整除推导并就此
    /// 固定；同一发行人后续送转必须沿用同一面值，不得用增大后的发行股数反推缩小
    /// 面值。送股（股票股利）额外受可分配利润上限约束（Simple 账面只做面值展示
    /// 登记，不做借贷过账，不产生投资者现金）。
    pub fn approve_stock_distribution(
        &mut self,
        plan: crate::company::stock_distribution::StockDistributionEventPlan,
    ) -> Result<(), SessionCorporateActionsError> {
        plan.validate()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if plan.approved_on > self.civil_date() || plan.announced_on < self.civil_date() {
            return Err(SessionCorporateActionsError::Invalid(
                "送转方案的批准日期或公告日期与当前会话日期不一致".into(),
            ));
        }
        // 同一发行人同证券同除权日的第二起送转事件在受理时直接拒绝：同日多起送转
        // 的合并除权口径（比例相加还是复合）未在官方材料核实，不能推迟到 R+1 首
        // tick 的除权准备才以致命错误暴露。
        if self
            .state
            .corporate_actions
            .stock_distributions
            .iter()
            .any(|existing| {
                let existing = existing.plan();
                existing.issuer == plan.issuer
                    && existing.stock == plan.stock
                    && existing.ex_rights_on == plan.ex_rights_on
            })
        {
            return Err(SessionCorporateActionsError::Invalid(
                "同一发行人同证券同除权日已存在送转事件；同日多起送转的合并除权口径未核实，受理时显式拒绝"
                    .into(),
            ));
        }
        let registry = self
            .state
            .corporate_actions
            .registries
            .iter()
            .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid("送转需要已显式配置的完整股东名册".into())
            })?;
        if registry.settled_on() > self.civil_date() {
            return Err(SessionCorporateActionsError::Invalid(
                "股东名册日期晚于当前会话日期".into(),
            ));
        }
        plan.validate_calendar(self.state.civil_clock.calendar())
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        let mut candidate_system = self.state.company_system.as_ref().clone();
        let issuer = candidate_system
            .issuers()
            .get(&plan.issuer)
            .ok_or_else(|| SessionCorporateActionsError::Invalid("送转计划发行人不存在".into()))?;
        if issuer.listed_stock.as_ref() != Some(&plan.stock) {
            return Err(SessionCorporateActionsError::Invalid(
                "送转计划证券与发行人不匹配".into(),
            ));
        }
        let legal_facts = candidate_system
            .dividend_legal_facts(&plan.issuer)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid(
                    "送转面值推导需要先显式绑定公司注册资本法定事实".into(),
                )
            })?;
        let prior_facts = candidate_system
            .stock_distribution_facts(&plan.issuer)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        let par_cents = match prior_facts.first() {
            Some(first) => {
                // 已有送转声明：面值在首次声明时固定，后续送转必须沿用同一面值。
                if prior_facts
                    .iter()
                    .any(|fact| fact.par_value_per_share != first.par_value_per_share)
                {
                    return Err(SessionCorporateActionsError::Invalid(
                        "既有送转声明的每股面值不一致，无法确定后续送转沿用面值".into(),
                    ));
                }
                i128::from(first.par_value_per_share.cents())
            }
            None => {
                // 首次送转声明：由注册资本法定事实与当前发行股数整除推导面值。
                let issued_shares = registry.issued_shares();
                let registered_capital_cents = legal_facts
                    .registered_capital
                    .to_money()
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .cents();
                let capital_cents_i128 = i128::from(registered_capital_cents);
                let shares_i128 = i128::from(issued_shares);
                if capital_cents_i128 % shares_i128 != 0 {
                    return Err(SessionCorporateActionsError::Invalid(
                        "注册资本与发行股数不能整除为每股面值；需先提供可整除的法定事实".into(),
                    ));
                }
                capital_cents_i128 / shares_i128
            }
        };
        let par_value_per_share = Money::from_cents(
            i64::try_from(par_cents)
                .map_err(|_| SessionCorporateActionsError::Invalid("每股面值溢出".into()))?,
        );
        if par_value_per_share <= Money::ZERO {
            return Err(SessionCorporateActionsError::Invalid(
                "推导出的每股面值必须为正数".into(),
            ));
        }
        let capital_increase_cents = par_cents
            .checked_mul(i128::from(plan.approved_total_new_shares))
            .ok_or_else(|| SessionCorporateActionsError::Invalid("送转股本增加金额溢出".into()))?;
        let declaration = crate::company::stock_distribution::StockDistributionDeclaration {
            event_id: plan.event_id.clone(),
            approval_reference: plan.approval_reference.clone(),
            kind: plan.kind.clone(),
            approved_on: plan.approved_on,
            new_shares: plan.approved_total_new_shares,
            par_value_per_share,
            capital_increase: crate::accounting::AccountingAmount::from_cents(
                capital_increase_cents,
            ),
            registered_capital_at_approval: legal_facts.registered_capital,
        };
        candidate_system
            .declare_stock_distribution(&plan.issuer, declaration)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if self
            .state
            .corporate_actions
            .stock_distributions
            .iter()
            .any(|existing| existing.plan().event_id == plan.event_id)
        {
            return Err(SessionCorporateActionsError::Invalid(
                "送转事件 id 已存在".into(),
            ));
        }
        let book = crate::company::stock_distribution::StockDistributionBook::new(plan)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        self.state.company_system = std::sync::Arc::new(candidate_system);
        self.state.corporate_actions.stock_distributions.push(book);
        self.state
            .corporate_actions
            .stock_distributions
            .sort_by(|left, right| left.plan().event_id.cmp(&right.plan().event_id));
        Ok(())
    }

    /// 偏好自动提案（ADR-0037）：在 simple 结算周期完成后的同一日结候选事务内，
    /// 按每公司显式配置的偏好评估并构造方案，提交给与显式 API 完全相同的
    /// [`Self::approve_cash_dividend`] / [`Self::approve_stock_distribution`] 入口
    /// （同一状态机、同一制度校验；偏好触发不减少任何校验）。
    ///
    /// 提案被制度拒绝时把原因如实记入 Simple 偏好台账（同周期同因幂等，不产生
    /// 重试风暴），不令日结失败；结构前置不满足（未上市、无完整名册）时不评估，
    /// 不产生方案也不记录拒绝。评估仅在结算周期末日发生一次，全部为轻量整数
    /// 比较与既有事实扫描，无每 tick 开销。
    fn process_simple_preference_proposals(
        &mut self,
        settled_date: crate::calendar::CivilDate,
    ) -> Result<(), SessionCorporateActionsError> {
        use crate::company::simple::preferences::{
            self as preference_api, SimplePreferenceProposalKind,
        };
        let approve_on = self.civil_date();
        debug_assert_eq!(
            approve_on,
            settled_date
                .next()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?,
            "偏好评估时点的会话自然日必须为结算周期末日的次日"
        );
        let cycle_months = match self.state.company_system.config() {
            crate::company::config::CompanySystemConfig::Simple(config) => {
                config.settlement_cycle.months()
            }
            crate::company::config::CompanySystemConfig::Simulation => {
                return Err(SessionCorporateActionsError::Invalid(
                    "Simulation 模型尚不能创建，不应进入偏好评估".into(),
                ));
            }
        };
        let companies: Vec<crate::company::CompanyId> = self
            .state
            .company_system
            .issuers()
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        for company in companies {
            let preference = self
                .state
                .company_system
                .simple_preferences(&company)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if preference.cash_dividend.is_none() && preference.stock_distribution.is_none() {
                continue;
            }
            let Some(stock) = self
                .state
                .company_system
                .issuers()
                .get(&company)
                .and_then(|issuer| issuer.listed_stock.clone())
            else {
                // 结构前置：未上市发行人无公司行为通道，不评估（非制度拒绝）。
                continue;
            };
            let Some(registry_index) = self
                .state
                .corporate_actions
                .registries
                .iter()
                .position(|registry| registry.stock() == &stock)
            else {
                // 结构前置：无完整股东名册时无法构造或登记任何方案，不评估。
                continue;
            };
            let exchange = self
                .state
                .civil_clock
                .stock_exchange(&stock)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!(
                        "偏好评估缺少 {stock:?} 的交易所日历映射"
                    ))
                })?;
            let eligible_shares = self.state.corporate_actions.registries[registry_index]
                .holdings()
                .iter()
                .filter(|holding| {
                    !matches!(
                        &holding.holder,
                        crate::company::share_registry::HolderId::IssuerTreasury
                    )
                })
                .try_fold(0_u64, |total, holding| {
                    holding
                        .lots
                        .iter()
                        .try_fold(total, |sum, lot| sum.checked_add(lot.qty))
                })
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("偏好评估可分红股数溢出".into())
                })?;
            // 每公司每类别每周期只取一次事实向量并在幂等判定、频率归并间复用
            // （行为不变；避免同一周期对 facts 的重复取用与 Vec 克隆）。
            let dividend_facts = match &preference.cash_dividend {
                Some(_) => Some(
                    self.state
                        .company_system
                        .dividend_plan_facts(&company)
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?,
                ),
                None => None,
            };
            let stock_facts = match &preference.stock_distribution {
                Some(_) => Some(
                    self.state
                        .company_system
                        .stock_distribution_facts(&company)
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?,
                ),
                None => None,
            };
            // 行情敞口上下文：提案锚收盘、涨跌幅限制与最小价位（跌停链迭代
            // 与引擎 price_bound 同一取整口径），以及按本周期日程推导的
            // 除息/除权日上既有的同日方案（显式 + 自动）合并事实。日程推导失败
            // 时返回 None，由评估函数自行推导日程并以同一失败原因如实拒绝。
            let market_anchor = self
                .state
                .markets
                .get(&stock)
                .map(|market| (market.last_close(), market.limit_bps(), market.tick()));
            let same_ex_date =
                self.same_ex_date_preference_context(&stock, exchange, approve_on)?;
            let (same_ex_gross, same_ex_ratio_micros, same_ex_stock_event) =
                same_ex_date.unwrap_or((Money::ZERO, None, false));
            if let Some(cash_preference) = preference.cash_dividend.clone() {
                let facts = dividend_facts
                    .as_ref()
                    .expect("配置了现金分红偏好时必须已取分红事实");
                let plan_id = preference_api::cash_dividend_plan_id(&company, settled_date);
                let already_proposed = facts.iter().any(|fact| fact.plan_id == plan_id);
                // 已有现金分红提案（接受或被拒）所属结算周期末日：接受的提案按
                // 「批准日的前一自然日」归并周期（自动提案批准日 = 周期末日次日），
                // 被拒提案直接取台账评估日。
                let last_cash_proposal_on = self.last_simple_proposal_period_end(
                    &company,
                    SimplePreferenceProposalKind::CashDividend,
                    facts.iter().map(|fact| fact.approved_on),
                )?;
                let distributable = self.state.company_system.distributable_profit(&company);
                let legal_capital = self
                    .state
                    .company_system
                    .dividend_legal_facts(&company)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .map(|facts| facts.registered_capital);
                let exposure = market_anchor.map(|(last_close, limit_bps, tick)| {
                    preference_api::SimpleCashDividendExposure {
                        last_close,
                        limit_bps,
                        tick,
                        same_ex_date_gross_per_share: same_ex_gross,
                        same_ex_date_ratio_micros: same_ex_ratio_micros,
                    }
                });
                let rejection = match distributable {
                    Ok(distributable) => {
                        let input = preference_api::SimpleCashDividendEvaluation {
                            preference: &cash_preference,
                            company: &company,
                            stock: &stock,
                            exchange,
                            period_end: settled_date,
                            approve_on,
                            calendar: self.state.civil_clock.calendar(),
                            distributable,
                            eligible_shares,
                            exposure,
                            registered_capital: legal_capital,
                            last_proposal_period_end: last_cash_proposal_on,
                            cycle_months,
                            suppress_duplicate: already_proposed,
                        };
                        match preference_api::evaluate_cash_dividend_preference(&input) {
                            preference_api::SimpleCashDividendOutcome::Skip => None,
                            preference_api::SimpleCashDividendOutcome::Proposal {
                                declaration,
                                plan,
                            } => self
                                .approve_cash_dividend(declaration, plan)
                                .err()
                                .map(|error| error.to_string()),
                            preference_api::SimpleCashDividendOutcome::Rejected { detail } => {
                                Some(detail)
                            }
                        }
                    }
                    Err(error) => Some(error.to_string()),
                };
                if let Some(detail) = rejection {
                    std::sync::Arc::make_mut(&mut self.state.company_system)
                        .record_preference_rejection(
                            &company,
                            SimplePreferenceProposalKind::CashDividend,
                            settled_date,
                            detail,
                        )
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }
            if let Some(stock_preference) = preference.stock_distribution.clone() {
                let facts = stock_facts
                    .as_ref()
                    .expect("配置了送转偏好时必须已取送转事实");
                let event_id = preference_api::stock_distribution_event_id(&company, settled_date);
                let already_proposed = facts.iter().any(|fact| fact.event_id == event_id);
                let cumulative_shares = facts
                    .iter()
                    .try_fold(0_u64, |total, fact| total.checked_add(fact.new_shares))
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("偏好评估送转累计股数溢出".into())
                    })?;
                let last_stock_proposal_on = self.last_simple_proposal_period_end(
                    &company,
                    SimplePreferenceProposalKind::StockDistribution,
                    facts.iter().map(|fact| fact.approved_on),
                )?;
                let initial_issued_shares = self
                    .state
                    .setup
                    .stocks
                    .iter()
                    .find(|spec| spec.code == stock)
                    .map(|spec| spec.total_shares)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(format!(
                            "偏好评估缺少 {stock:?} 的开局发行股数"
                        ))
                    })?;
                // 行情敞口上下文：同除权日现金红利按**当前**账簿重扫合计——现金
                // 分红偏好先评估，若本轮刚批准自动现金方案，其 gross 已入账簿
                // 并被计入（显式 + 自动合并口径）。
                let same_ex_cash_gross = self
                    .same_ex_date_preference_context(&stock, exchange, approve_on)?
                    .map(|(gross, _ratio, _event)| gross)
                    .unwrap_or(Money::ZERO);
                let exposure = market_anchor.map(|(last_close, limit_bps, tick)| {
                    preference_api::SimpleStockDistributionExposure {
                        last_close,
                        limit_bps,
                        tick,
                        same_ex_date_gross_per_share: same_ex_cash_gross,
                        same_ex_date_stock_event: same_ex_stock_event,
                    }
                });
                // 现金分红批准会核减可分配利润，送转评估必须读取最新账面。
                let distributable = self.state.company_system.distributable_profit(&company);
                let rejection = match distributable {
                    Ok(distributable) => {
                        let input = preference_api::SimpleStockDistributionEvaluation {
                            preference: &stock_preference,
                            company: &company,
                            stock: &stock,
                            exchange,
                            period_end: settled_date,
                            approve_on,
                            calendar: self.state.civil_clock.calendar(),
                            distributable,
                            eligible_shares,
                            initial_issued_shares,
                            cumulative_distributed_shares: cumulative_shares,
                            exposure,
                            last_proposal_period_end: last_stock_proposal_on,
                            cycle_months,
                            suppress_duplicate: already_proposed,
                        };
                        match preference_api::evaluate_stock_distribution_preference(&input) {
                            preference_api::SimpleStockDistributionOutcome::Skip => None,
                            preference_api::SimpleStockDistributionOutcome::Proposal { plan } => {
                                self.approve_stock_distribution(plan)
                                    .err()
                                    .map(|error| error.to_string())
                            }
                            preference_api::SimpleStockDistributionOutcome::Rejected { detail } => {
                                Some(detail)
                            }
                        }
                    }
                    Err(error) => Some(error.to_string()),
                };
                if let Some(detail) = rejection {
                    std::sync::Arc::make_mut(&mut self.state.company_system)
                        .record_preference_rejection(
                            &company,
                            SimplePreferenceProposalKind::StockDistribution,
                            settled_date,
                            detail,
                        )
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }
        }
        Ok(())
    }

    /// 发行人回购专用账户 id（开关开启时创建于 NPC 序列之后）；未启用为 None。
    pub fn issuer_repurchase_account_id(&self) -> Option<AccountId> {
        self.state
            .setup
            .issuer_repurchase_enabled
            .then(|| {
                let npc_count = u64::from(self.state.setup.npcs.retail_count)
                    + u64::from(self.state.setup.npcs.inst_count)
                    + u64::from(self.state.setup.npcs.hot_count);
                AccountId(npc_count.saturating_add(1))
            })
            .filter(|id| self.state.accounts.contains_key(id))
    }

    /// 汇总发行人回购账户当日买入成交（真实回执；卖出恒不可能——回购只买不卖）。
    fn collect_issuer_repurchase_fills(
        &self,
        day: crate::calendar::CivilDate,
    ) -> Result<Vec<crate::company::issuer_repurchase::RepurchaseFillRecord>, StepFatal> {
        let Some(account) = self.issuer_repurchase_account_id() else {
            return Ok(Vec::new());
        };
        let mut by_stock = std::collections::BTreeMap::<StockCode, (u64, i64, i64)>::new();
        if let Some(confirmations) = self.state.personal_trade_confirmations.get(&account) {
            for confirmation in confirmations.iter() {
                if confirmation.civil_date != day
                    || confirmation.side != crate::orderbook::Side::Buy
                {
                    continue;
                }
                let entry = by_stock
                    .entry(confirmation.code.clone())
                    .or_insert((0, 0, 0));
                entry.0 += u64::from(confirmation.quantity_shares);
                entry.1 += confirmation.gross.cents();
                entry.2 += confirmation
                    .actual_fees
                    .total()
                    .map(|fees| fees.cents())
                    .map_err(|fatal| {
                        StepFatal::InvariantViolation {
                            description: format!("回购成交费用汇总失败：{fatal}"),
                            location: "GameSession::collect_issuer_repurchase_fills".into(),
                        }
                    })?;
            }
        }
        Ok(by_stock
            .into_iter()
            .filter(|(_, (shares, _, _))| *shares > 0)
            .map(|(stock, (shares, gross, fees))| {
                crate::company::issuer_repurchase::RepurchaseFillRecord {
                    stock,
                    day,
                    shares,
                    gross: Money::from_cents(gross),
                    fees: Money::from_cents(fees),
                }
            })
            .collect())
    }

    /// 受理配股／增发方案（显式计划入口；新局开关 `rights_offering_enabled`）。
    ///
    /// 面值口径与送转一致：首次声明由「已绑定注册资本法定事实 ÷ 名册当前发行股数」
    /// 整除推导并固定；发行价不得低于面值（《公司法》第 148 条）。发行人侧只记
    /// Simple 账面声明事实（ADR-0035/0039），真实现金只在缴款期日终从投资者划扣。
    pub fn approve_rights_offering(
        &mut self,
        plan: crate::company::rights_offering::RightsOfferingEventPlan,
    ) -> Result<(), SessionError> {
        if !self.state.setup.rights_offering_enabled {
            return Err(SessionError::InvalidSetup(
                "本局未启用配股／增发机制（新局开关 rights_offering_enabled=false），显式拒绝"
                    .into(),
            ));
        }
        plan.validate()
            .map_err(|error| SessionError::InvalidSetup(format!("配股方案非法：{error}")))?;
        if let crate::company::rights_offering::RightsSubscriptionStrategy::StrategyBased =
            plan.npc_subscription_strategy
        {
            return Err(SessionError::InvalidSetup(
                "配股 NPC 认购策略 StrategyBased 未实现：当前仅支持默认足额认购（FullByDefault）"
                    .into(),
            ));
        }
        if plan.approved_on > self.civil_date() || plan.announced_on < self.civil_date() {
            return Err(SessionError::InvalidSetup(
                "配股方案的批准日期或公告日期与当前会话日期不一致".into(),
            ));
        }
        plan.validate_calendar(self.state.civil_clock.calendar())
            .map_err(|error| SessionError::InvalidSetup(format!("配股日程非法：{error}")))?;
        if self
            .state
            .corporate_actions
            .rights_offerings
            .iter()
            .any(|existing| existing.plan().event_id == plan.event_id)
        {
            return Err(SessionError::InvalidSetup("配股事件 id 已存在".into()));
        }
        let registry = self
            .state
            .corporate_actions
            .registries
            .iter()
            .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
            .ok_or_else(|| {
                SessionError::InvalidSetup("配股需要已显式配置的完整股东名册".into())
            })?;
        if registry.settled_on() > self.civil_date() {
            return Err(SessionError::InvalidSetup(
                "股东名册日期晚于当前会话日期".into(),
            ));
        }
        let mut candidate_system = self.state.company_system.as_ref().clone();
        let issuer = candidate_system
            .issuers()
            .get(&plan.issuer)
            .ok_or_else(|| SessionError::InvalidSetup("配股计划发行人不存在".into()))?;
        if issuer.listed_stock.as_ref() != Some(&plan.stock) {
            return Err(SessionError::InvalidSetup("配股计划证券与发行人不匹配".into()));
        }
        let legal_facts = candidate_system
            .dividend_legal_facts(&plan.issuer)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?
            .ok_or_else(|| {
                SessionError::InvalidSetup(
                    "配股面值推导需要先显式绑定公司注册资本法定事实".into(),
                )
            })?;
        // 面值口径：优先沿用送转/配股已绑定面值，否则整除推导并就此固定。
        let par_cents = if let Some(first) = candidate_system
            .stock_distribution_facts(&plan.issuer)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?
            .first()
        {
            i128::from(first.par_value_per_share.cents())
        } else if let Some(first) = candidate_system
            .rights_offering_facts(&plan.issuer)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?
            .first()
        {
            i128::from(first.par_value_per_share.cents())
        } else {
            let registered_capital_cents = i128::from(
                legal_facts
                    .registered_capital
                    .to_money()
                    .map_err(|error| SessionError::InvalidSetup(error.to_string()))?
                    .cents(),
            );
            let shares = i128::from(registry.issued_shares());
            if registered_capital_cents % shares != 0 {
                return Err(SessionError::InvalidSetup(
                    "注册资本与发行股数不能整除为每股面值；需先提供可整除的法定事实".into(),
                ));
            }
            registered_capital_cents / shares
        };
        let par_value_per_share = Money::from_cents(
            i64::try_from(par_cents)
                .map_err(|_| SessionError::InvalidSetup("每股面值溢出".into()))?,
        );
        if par_value_per_share <= Money::ZERO || plan.price_per_share < par_value_per_share {
            return Err(SessionError::InvalidSetup(
                "配股发行价不得低于推导出的每股面值（面值必须为正）".into(),
            ));
        }
        candidate_system
            .declare_rights_offering(
                &plan.issuer,
                crate::company::rights_offering::RightsOfferingDeclaration {
                    event_id: plan.event_id.clone(),
                    approval_reference: plan.approval_reference.clone(),
                    approved_on: plan.approved_on,
                    price_per_share: plan.price_per_share,
                    par_value_per_share,
                    registered_capital_at_approval: legal_facts.registered_capital,
                },
            )
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        let book = crate::company::rights_offering::RightsOfferingBook::new(plan)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        self.state.company_system = std::sync::Arc::new(candidate_system);
        self.state.corporate_actions.rights_offerings.push(book);
        self.state
            .corporate_actions
            .rights_offerings
            .sort_by(|left, right| left.plan().event_id.cmp(&right.plan().event_id));
        Ok(())
    }

    /// 玩家／宿主显式认购配股（缴款期内当日日终划扣；现金不足时部分放弃并如实记录）。
    pub fn subscribe_rights_offering(
        &mut self,
        event_id: &str,
        account: AccountId,
        shares: u64,
    ) -> Result<(), SessionError> {
        if !self.state.setup.rights_offering_enabled {
            return Err(SessionError::InvalidSetup(
                "本局未启用配股／增发机制（新局开关 rights_offering_enabled=false），显式拒绝"
                    .into(),
            ));
        }
        if shares == 0 {
            return Err(SessionError::InvalidSetup("认购股数必须为正数".into()));
        }
        let book = self
            .state
            .corporate_actions
            .rights_offerings
            .iter()
            .find(|book| book.plan().event_id == event_id)
            .ok_or_else(|| SessionError::InvalidSetup(format!("未知配股事件 {event_id}")))?;
        let plan = book.plan().clone();
        let today = self.civil_date();
        if !plan.payment_window_contains(today) {
            return Err(SessionError::InvalidSetup(
                "认购只能在缴款期窗口内提交".into(),
            ));
        }
        if book.status()
            != &crate::company::rights_offering::RightsOfferingStatus::Entitled
        {
            return Err(SessionError::InvalidSetup(
                "配股尚未完成权证派发或已关窗".into(),
            ));
        }
        let account_state = self
            .state
            .accounts
            .get(&account)
            .ok_or(SessionError::UnknownPlayer(account))?;
        let cost = Money::from_cents(
            i64::try_from(
                u128::from(shares) * u128::from(plan.price_per_share.cents().unsigned_abs()),
            )
            .map_err(|_| SessionError::InvalidSetup("认购金额溢出".into()))?,
        );
        if account_state.cash() < cost {
            return Err(SessionError::InvalidSetup(format!(
                "认购金额 {cost:?} 超过账户真实现金（不足显式拒绝，不补认购资金）"
            )));
        }
        // 权利额度预检（日终复核）：具名权利或公开配售额度。
        let entitlement = book
            .entitlement()
            .ok_or_else(|| SessionError::InvalidSetup("配股缺少权证回执".into()))?;
        let holder = crate::company::share_registry::HolderId::Account(account);
        if let Some(holder_rights) = entitlement
            .entitlements
            .iter()
            .find(|entry| entry.holder == holder)
        {
            if shares > holder_rights.rights_shares {
                return Err(SessionError::InvalidSetup(
                    "认购股数超过持有权利（不超权利认购）".into(),
                ));
            }
        } else {
            if entitlement.open_subscription_shares == 0 {
                return Err(SessionError::InvalidSetup(
                    "该账户无配股权利且方案无公开配售额度".into(),
                ));
            }
            // 公开配售：按「剩余公开额度 − 已排队未结算量」受理，超额显式拒绝
            // （不静默、不截断、不入队）——否则日终 record_subscription 的额度
            // 校验会令整个日终失败且回滚后队列仍在，一次超额提交即永久卡死。
            let settled_open_used: u64 = self
                .state
                .corporate_actions
                .rights_offerings
                .iter()
                .find(|existing| existing.plan().event_id == event_id)
                .map(|existing| {
                    existing
                        .subscriptions()
                        .iter()
                        .filter(|record| {
                            !entitlement
                                .entitlements
                                .iter()
                                .any(|entry| entry.holder == record.holder)
                        })
                        .map(|record| record.requested_shares)
                        .try_fold(0_u64, |sum, requested| sum.checked_add(requested))
                })
                .unwrap_or(Some(0))
                .ok_or_else(|| SessionError::InvalidSetup("公开配售已入账认购合计溢出".into()))?;
            let queued_open_used: u64 = self
                .state
                .corporate_actions
                .rights_subscription_queue
                .iter()
                .filter(|queued| {
                    queued.event_id == event_id
                        && !entitlement.entitlements.iter().any(|entry| {
                            entry.holder
                                == crate::company::share_registry::HolderId::Account(queued.account)
                        })
                })
                .map(|queued| queued.requested_shares)
                .try_fold(0_u64, |sum, requested| sum.checked_add(requested))
                .ok_or_else(|| SessionError::InvalidSetup("公开配售排队认购合计溢出".into()))?;
            let remaining = entitlement
                .open_subscription_shares
                .checked_sub(settled_open_used)
                .and_then(|value| value.checked_sub(queued_open_used))
                .ok_or_else(|| {
                    SessionError::InvalidSetup("公开配售额度与已受理认购不一致".into())
                })?;
            if shares > remaining {
                return Err(SessionError::InvalidSetup(format!(
                    "公开配售剩余额度 {remaining} 股，本次申请 {shares} 股超出剩余额度；超额部分显式拒绝，不截断不入队，请调减后重试"
                )));
            }
        }
        if self
            .state
            .corporate_actions
            .rights_offerings
            .iter()
            .any(|book| {
                book.subscriptions()
                    .iter()
                    .any(|record| record.holder == holder)
                    && book.plan().event_id == event_id
            })
            || self
                .state
                .corporate_actions
                .rights_subscription_queue
                .iter()
                .any(|queued| {
                    queued.event_id == event_id && queued.account == account
                })
        {
            return Err(SessionError::InvalidSetup(
                "同一持有人只能提交一条净认购记录".into(),
            ));
        }
        self.state
            .corporate_actions
            .rights_subscription_queue
            .push(crate::session::corporate_actions::QueuedRightsSubscription {
                event_id: event_id.to_owned(),
                account,
                requested_shares: shares,
                submitted_on: today,
            });
        Ok(())
    }

    /// 受理发行人回购方案（独立开关 `issuer_repurchase_enabled`；ADR-0038）。
    ///
    /// 批准即按计划额度向发行人回购专用账户合成入账结算资金（凭空生成、专款
    /// 语义）；其后由执行器在窗口内以真实委托进入既有订单簿（不伪造成交、
    /// 不绕过涨跌停/笼子/撮合规则），卖方投资者真实收到资金。
    pub fn approve_issuer_repurchase(
        &mut self,
        plan: crate::company::issuer_repurchase::IssuerRepurchasePlan,
    ) -> Result<(), SessionError> {
        if !self.state.setup.issuer_repurchase_enabled {
            return Err(SessionError::InvalidSetup(
                "本局未启用发行人回购机制（新局开关 issuer_repurchase_enabled=false），显式拒绝"
                    .into(),
            ));
        }
        plan.validate()
            .map_err(|error| SessionError::InvalidSetup(format!("回购方案非法：{error}")))?;
        if plan.approved_on > self.civil_date() || plan.announced_on < self.civil_date() {
            return Err(SessionError::InvalidSetup(
                "回购方案的批准日期或公告日期与当前会话日期不一致".into(),
            ));
        }
        plan.validate_calendar(self.state.civil_clock.calendar())
            .map_err(|error| SessionError::InvalidSetup(format!("回购日程非法：{error}")))?;
        if self
            .state
            .corporate_actions
            .issuer_repurchases
            .iter()
            .any(|existing| existing.plan().event_id == plan.event_id)
        {
            return Err(SessionError::InvalidSetup("回购方案 id 已存在".into()));
        }
        // 同一证券同时只能有一个未完成回购方案：日终成交回执按证券聚合、不区分
        // 方案，第二个未完成方案会把同一笔真实成交记入多本账簿，完成勾稽必然
        // 失真（完成或取消后才可批准同证券新方案）。
        if self
            .state
            .corporate_actions
            .issuer_repurchases
            .iter()
            .any(|existing| {
                existing.plan().stock == plan.stock
                    && !matches!(
                        existing.status(),
                        crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed
                            | crate::company::issuer_repurchase::IssuerRepurchaseStatus::Cancelled
                    )
            })
        {
            return Err(SessionError::InvalidSetup(
                "该证券已有未完成的回购方案；同一证券同时只能有一个未完成方案，完成或取消后才能批准新方案".into(),
            ));
        }
        let repurchase_account = self.issuer_repurchase_account_id().ok_or_else(|| {
            SessionError::InvalidSetup("回购专用账户缺失（开关开启时应确定性创建）".into())
        })?;
        let mut candidate_system = self.state.company_system.as_ref().clone();
        let issuer = candidate_system
            .issuers()
            .get(&plan.issuer)
            .ok_or_else(|| SessionError::InvalidSetup("回购计划发行人不存在".into()))?;
        if issuer.listed_stock.as_ref() != Some(&plan.stock) {
            return Err(SessionError::InvalidSetup("回购计划证券与发行人不匹配".into()));
        }
        let registry_index = self
            .state
            .corporate_actions
            .registries
            .iter()
            .position(|registry| registry.stock() == &plan.stock)
            .ok_or_else(|| {
                SessionError::InvalidSetup("回购需要已显式配置的完整股东名册".into())
            })?;
        // 非减资用途：合计持有不得超过已发行股份 10%（63 号第 17 条）；
        // 减资注销用途不受 10% 限制（注销不长期持有）。
        if !matches!(
            plan.purpose,
            crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital
        ) && plan.max_shares * 10 > self.state.corporate_actions.registries[registry_index].issued_shares()
        {
            return Err(SessionError::InvalidSetup(
                "非减资用途回购数量上限超过已发行股份 10%（证监会回购规则第 17 条）".into(),
            ));
        }
        // 建立回购专户事实（bind-once）：已存在则幂等沿用；在候选副本上执行，
        // 后续任何失败都不留部分状态。
        let account_reference = format!("issuer-repurchase-account-{}", repurchase_account.0);
        let mut candidate_registry = self.state.corporate_actions.registries[registry_index].clone();
        candidate_registry
            .set_issuer_repurchase_account(crate::company::share_registry::IssuerRepurchaseAccountFacts {
                account_reference,
                source_evidence: plan.event_id.clone(),
                established_on: plan.approved_on,
            })
            .map_err(|error| {
                SessionError::InvalidSetup(format!("回购专户事实非法：{error}"))
            })?;
        candidate_system
            .declare_issuer_repurchase(
                &plan.issuer,
                crate::company::issuer_repurchase::IssuerRepurchaseFinanceFact {
                    event_id: plan.event_id.clone(),
                    approval_reference: plan.approval_reference.clone(),
                    approved_on: plan.approved_on,
                    synthetic_funding: crate::accounting::AccountingAmount::from_money(
                        plan.total_budget,
                    ),
                    purpose: plan.purpose.clone(),
                    spent: None,
                    withdrawn_remainder: None,
                    completed_on: None,
                    cancelled_shares: 0,
                    cancelled_on: None,
                    capital_reduction: None,
                },
            )
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        // 合成资金凭空入账（ADR-0038：获批计划额度；投资者资金池注入的唯一来源侧）。
        let synthetic_budget = plan.total_budget;
        let book = crate::company::issuer_repurchase::IssuerRepurchaseBook::new(plan)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        self.state.corporate_actions.registries[registry_index] = candidate_registry;
        self.state.company_system = std::sync::Arc::new(candidate_system);
        self.state.corporate_actions.issuer_repurchases.push(book);
        self.state
            .corporate_actions
            .issuer_repurchases
            .sort_by(|left, right| left.plan().event_id.cmp(&right.plan().event_id));
        self.state
            .accounts
            .get_mut(&repurchase_account)
            .ok_or_else(|| {
                SessionError::InvalidSetup("回购专用账户缺失（开关开启时应确定性创建）".into())
            })?
            .credit_cash(synthetic_budget)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        Ok(())
    }

    /// 执行回购注销（减资用途）：核减专户股份、总股本与注册资本；不除权
    /// （无官方除权条文，市场实践不除权——登记口径见 docs/trading-rules.md）。
    pub fn execute_issuer_repurchase_cancellation(
        &mut self,
        event_id: &str,
        shares: u64,
        on: crate::calendar::CivilDate,
    ) -> Result<(), SessionError> {
        let repurchase_account = self
            .issuer_repurchase_account_id()
            .ok_or_else(|| SessionError::InvalidSetup("本局未启用发行人回购机制".into()))?;
        let index = self
            .state
            .corporate_actions
            .issuer_repurchases
            .iter()
            .position(|book| book.plan().event_id == event_id)
            .ok_or_else(|| SessionError::InvalidSetup(format!("未知回购方案 {event_id}")))?;
        let stock = self.state.corporate_actions.issuer_repurchases[index]
            .plan()
            .stock
            .clone();
        let issuer = self.state.corporate_actions.issuer_repurchases[index]
            .plan()
            .issuer
            .clone();
        // 先按候选事务执行名册与账户核减，任何失败整体回滚（借用 clone 兜底）。
        let registry_checkpoint = self
            .state
            .corporate_actions
            .registries
            .iter()
            .find(|registry| registry.stock() == &stock)
            .cloned()
            .ok_or_else(|| SessionError::InvalidSetup("回购注销缺少股东名册".into()))?;
        let issued_before = registry_checkpoint.issued_shares();
        let mut candidate_registry = registry_checkpoint.clone();
        candidate_registry
            .close_day(crate::company::share_registry::ShareDayRequest {
                event_id: format!("issuer-repurchase-cancellation:{event_id}"),
                day: on,
                scope: crate::company::share_registry::MovementScope::IssuerRepurchaseCancellation {
                    basis: self.state.corporate_actions.issuer_repurchases[index]
                        .plan()
                        .approval_reference
                        .clone(),
                },
                changes: vec![crate::company::share_registry::DayNetChange {
                    holder: crate::company::share_registry::HolderId::IssuerTreasury,
                    change: -i128::try_from(shares).map_err(|_| {
                        SessionError::InvalidSetup("回购注销股数超出范围".to_string())
                    })?,
                    acquisition: None,
                }],
            })
            .map_err(|error| SessionError::InvalidSetup(format!("回购注销名册核减失败：{error}")))?;
        let mut candidate_accounts = self.state.accounts.clone();
        candidate_accounts
            .get_mut(&repurchase_account)
            .ok_or_else(|| {
                SessionError::InvalidSetup("回购专用账户缺失（开关开启时应确定性创建）".into())
            })?
            .write_down_position_shares(stock.clone(), shares)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        let mut candidate_system = self.state.company_system.as_ref().clone();
        candidate_system
            .record_issuer_repurchase_cancellation(
                &issuer,
                event_id,
                on,
                shares,
                issued_before,
            )
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        // 全部成功后一次性安装候选状态并推进账簿状态机。
        let registry_index = self
            .state
            .corporate_actions
            .registries
            .iter()
            .position(|registry| registry.stock() == &stock)
            .expect("registry presence was checked above");
        self.state.corporate_actions.registries[registry_index] = candidate_registry;
        self.state.accounts = candidate_accounts;
        self.state.company_system = std::sync::Arc::new(candidate_system);
        self.state.corporate_actions.issuer_repurchases[index]
            .record_cancellation(on, shares)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))?;
        Ok(())
    }

    /// 回购执行器：窗口内每个交易日的连续竞价阶段以真实委托进入既有订单簿。
    ///
    /// 委托约束（63 号第 30 条）：申报价格不得为当日涨幅限制价格——取
    /// min(方案价格上限, 涨停价−1 个最小价位)；不得在集合竞价时段申报（仅
    /// Continuous 阶段执行）。数量按剩余额度（预留费用余量）、数量上限与
    /// 单笔上限取整手。每日至多一单（`last_order_day` 幂等去重）。
    pub(super) fn place_issuer_repurchase_orders(&mut self) -> Result<(), StepFatal> {
        if !self.state.setup.issuer_repurchase_enabled {
            return Ok(());
        }
        if self.phase() != TradingPhase::Continuous {
            return Ok(());
        }
        let Some(repurchase_account) = self.issuer_repurchase_account_id() else {
            return Ok(());
        };
        let day = self.civil_date();
        let lot_size = u64::from(self.state.setup.config.lot_size.max(1));
        let mut orders = Vec::new();
        for book in &self.state.corporate_actions.issuer_repurchases {
            let plan = book.plan();
            if !matches!(
                book.status(),
                crate::company::issuer_repurchase::IssuerRepurchaseStatus::Announced
                    | crate::company::issuer_repurchase::IssuerRepurchaseStatus::Executing
            ) || !plan.window_contains(day)
                || book.last_order_day() == Some(day)
            {
                continue;
            }
            let Ok(remaining) = book.remaining_budget() else {
                continue;
            };
            let share_room = plan
                .max_shares
                .saturating_sub(book.total_filled_shares());
            if share_room == 0 {
                continue;
            }
            let market = match self.state.markets.get(&plan.stock) {
                Some(market) => market,
                None => continue,
            };
            // 委托价上界取三者最小：方案价格上限、涨停价−1 个最小价位（63 号
            // 第 30 条（一）：申报价格不得为当日涨幅限制的价格）、连续竞价买入
            // 价格笼子上界（不绕过既有申报规则）。
            let up_stop = match market.up_stop() {
                Ok(up_stop) => up_stop,
                Err(_) => continue,
            };
            // 最小价位取自该证券的显式配置（不猜测市场内部状态）。
            let tick = self
                .state
                .setup
                .stocks
                .iter()
                .find(|spec| spec.code == plan.stock)
                .map(|spec| spec.tick)
                .unwrap_or(Money::from_cents(1));
            // 价格笼子上界：max(参考价×102%, 参考价+10 个最小价位)（既有交易规则）。
            let reference = market.continuous_limit_reference(crate::orderbook::Side::Buy);
            let cage_upper = reference
                .apply_rate(1.02)
                .unwrap_or(reference)
                .max(
                    reference
                        .add(tick.mul_shares(10).unwrap_or(tick))
                        .unwrap_or(reference),
                );
            let cap = plan
                .price_cap_per_share
                .min(up_stop.sub(tick).unwrap_or(plan.price_cap_per_share))
                .min(cage_upper);
            if cap <= Money::ZERO {
                continue;
            }
            // 预留 0.5% 费用余量后按剩余额度取整手。
            let usable = (remaining.cents().max(0) as u128) * 995 / 1000;
            let mut qty = (usable / (u128::from(cap.cents().unsigned_abs()) * u128::from(lot_size)))
                * u128::from(lot_size);
            qty = qty.min(u128::from(share_room));
            // 主板单笔上限 1,000,000 股（既有申报规则的保守钳制；实际由受理校验）。
            qty = qty.min(1_000_000);
            if qty < u128::from(lot_size) {
                continue;
            }
            orders.push((
                plan.stock.clone(),
                cap,
                u32::try_from(qty).map_err(|_| StepFatal::InvariantViolation {
                    description: "回购委托数量超出 u32 申报域".into(),
                    location: "GameSession::place_issuer_repurchase_orders".into(),
                })?,
            ));
        }
        for (stock, price, qty) in orders {
            let event_ids: Vec<String> = self
                .state
                .corporate_actions
                .issuer_repurchases
                .iter()
                .filter(|book| {
                    let plan = book.plan();
                    plan.stock == stock
                        && plan.window_contains(day)
                        && book.last_order_day() != Some(day)
                        && matches!(
                            book.status(),
                            crate::company::issuer_repurchase::IssuerRepurchaseStatus::Announced
                                | crate::company::issuer_repurchase::IssuerRepurchaseStatus::Executing
                        )
                })
                .map(|book| book.plan().event_id.clone())
                .collect();
            let intent = crate::strategy::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::orderbook::Side::Buy,
                price: crate::strategy::LimitPrice::Fixed(price),
                qty,
            };
            self.validate_player_calendar(&intent).map_err(|error| {
                StepFatal::InvariantViolation {
                    description: format!("回购委托日历校验失败：{error}"),
                    location: "GameSession::place_issuer_repurchase_orders".into(),
                }
            })?;
            let received = self
                .state
                .ingress_receipt_cursors
                .receive(repurchase_account, intent)
                .map_err(|error| StepFatal::InvariantViolation {
                    description: format!("回购委托回执登记失败：{error}"),
                    location: "GameSession::place_issuer_repurchase_orders".into(),
                })?;
            self.state.pending_player.push(received);
            for event_id in event_ids {
                if let Some(book) = self
                    .state
                    .corporate_actions
                    .issuer_repurchases
                    .iter_mut()
                    .find(|book| book.plan().event_id == event_id)
                {
                    book.set_last_order_day(day);
                }
            }
        }
        Ok(())
    }

    /// 按本周期自动提案日程推导的除息/除权日上，既有同日方案（显式 + 自动）
    /// 的合并事实：(同日现金分红合计税前每股红利, 同日送转事件比例, 是否已
    /// 存在同日送转事件)。日程推导失败返回 `None`（评估函数会以同一失败原因
    /// 如实拒绝，不静默）。
    fn same_ex_date_preference_context(
        &self,
        stock: &crate::account::StockCode,
        exchange: crate::calendar::CalendarExchange,
        approve_on: crate::calendar::CivilDate,
    ) -> Result<Option<(Money, Option<u64>, bool)>, SessionCorporateActionsError> {
        use crate::company::simple::preferences as preference_api;
        let Some((_registered_on, ex_on)) = preference_api::preference_ex_dates(
            self.state.civil_clock.calendar(),
            exchange,
            approve_on,
        )
        .ok() else {
            return Ok(None);
        };
        let mut cash_gross = Money::ZERO;
        let mut stock_ratio_micros = None;
        let mut stock_event_exists = false;
        for book in &self.state.corporate_actions.dividends {
            let plan = book.plan();
            if plan.stock == *stock && plan.ex_dividend_on == ex_on {
                cash_gross = cash_gross
                    .add(plan.gross_per_share)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
        }
        for book in &self.state.corporate_actions.stock_distributions {
            let plan = book.plan();
            if plan.stock == *stock && plan.ex_rights_on == ex_on {
                stock_event_exists = true;
                stock_ratio_micros = Some(plan.shares_per_existing_share_micros);
            }
        }
        Ok(Some((cash_gross, stock_ratio_micros, stock_event_exists)))
    }

    /// 该公司该类别最近一次提案（接受或被拒）所属结算周期末日。
    /// 接受的提案以「批准日的前一自然日」归并（自动提案批准日 = 周期末日次日；
    /// 显式方案按其批准日的前一日就近归并），被拒提案取台账评估日。
    /// 批准日序列由调用方传入（facts 每周期已取一次，避免重复取用与克隆）。
    fn last_simple_proposal_period_end(
        &self,
        company: &crate::company::CompanyId,
        kind: crate::company::simple::preferences::SimplePreferenceProposalKind,
        approved_on_dates: impl Iterator<Item = crate::calendar::CivilDate>,
    ) -> Result<Option<crate::calendar::CivilDate>, SessionCorporateActionsError> {
        let map_err = |error: crate::company::CompanySystemError| {
            SessionCorporateActionsError::Invalid(error.to_string())
        };
        let mut latest = self
            .state
            .company_system
            .last_preference_rejection_on(company, kind)
            .map_err(map_err)?;
        for approved_on in approved_on_dates {
            let proposal_day = approved_on
                .prev()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if latest.is_none_or(|known| proposal_day > known) {
                latest = Some(proposal_day);
            }
        }
        Ok(latest)
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

    pub fn personal_trade_confirmations(
        &self,
        account: AccountId,
    ) -> Vec<PersonalTradeConfirmation> {
        self.state
            .personal_trade_confirmations
            .get(&account)
            .map(|history| history.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn personal_trade_confirmations_page(
        &self,
        account: AccountId,
        before_receipt: Option<u64>,
    ) -> Vec<PersonalTradeConfirmation> {
        self.state
            .personal_trade_confirmations
            .get(&account)
            .map(|history| {
                history
                    .iter_rev()
                    .filter(|confirmation| {
                        before_receipt.is_none_or(|cursor| confirmation.receipt_id < cursor)
                    })
                    .take(100)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn query_intraday_average(
        &self,
        code: &StockCode,
    ) -> Result<Option<crate::IntradayAverage>, SessionError> {
        if !self.state.markets.contains_key(code) {
            return Err(SessionError::UnknownHistoryStock(code.clone()));
        }
        let candle = self.state.candle_book.active().get(code);
        crate::calculate_intraday_average(
            candle.and_then(|value| value.trade_stats.as_ref()),
            candle.map_or(0, |value| value.volume),
        )
        .map_err(|error| SessionError::InvalidIntradayAverage(error.to_string()))
    }

    /// Returns one host-safe page of reports visible at the current civil instant.
    pub fn query_public_reports(
        &self,
        query: &crate::company::PublicReportQuery,
    ) -> Result<crate::company::PublicReportPage, SessionError> {
        let as_of = self.observation_civil_instant();
        self.state
            .library
            .query_public_reports(query, as_of)
            .map_err(|error| SessionError::Information(Box::new(error)))
    }

    pub fn query_public_report_availability(
        &self,
        query: &crate::company::PublicReportAvailabilityQuery,
    ) -> Result<crate::company::PublicReportAvailability, SessionError> {
        self.state
            .library
            .query_report_availability(
                &self.state.company_system,
                query,
                self.state.setup.report_frequency,
                self.observation_civil_instant(),
            )
            .map_err(|error| SessionError::Information(Box::new(error)))
    }

    /// Returns one host-safe report visible at the current civil instant.
    pub fn public_report_by_id(
        &self,
        id: String,
    ) -> Result<crate::company::PublicReportSummary, SessionError> {
        let as_of = self.observation_civil_instant();
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

    /// 自然日日结（冻结日历与双时钟）：当日经营终局窗口 → 更正过账 →（月/年末封账）→ 18:00 披露 → 前进次日。
    ///
    /// 交易日必须先完成当日会话（`ticks_per_day` 个 step，`day` 已自增到位）；
    /// 休市日直接调用。先全量验证（会话同步 + 时钟规则）后原子应用，任何
    /// `Err` 不改变会话与时钟状态。step 的 tick 循环完全不变——日结是新接在
    /// 收盘之后的自然日权威推进，不是 tick 循环的一部分。
    ///
    /// 日终 candidate 依次推进 CivilClock、选中 CompanySystem、公开披露与个人获知。
    /// 公司系统失败或公开材料校验失败时，候选状态整体回滚。
    pub fn end_civil_day(&mut self) -> Result<CivilDayEndReport, SessionError> {
        self.end_civil_day_inner(true)
    }

    pub(in crate::session) fn end_civil_day_without_ingress_publication(
        &mut self,
    ) -> Result<CivilDayEndReport, SessionError> {
        self.end_civil_day_inner(false)
    }

    fn end_civil_day_inner(&mut self, publish: bool) -> Result<CivilDayEndReport, SessionError> {
        self.require_healthy()?;
        let publication = self.ingress_calendar_publication()?;
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
        let checkpoint = self.clone_for_tick_shadow().map_err(|error| {
            SessionError::InvalidSave(format!("cannot create civil day-end checkpoint: {error}"))
        })?;
        let result = self.end_civil_day_after_session_check().and_then(|report| {
            if publish {
                self.publish_ingress_calendar(publication)?;
            }
            Ok(report)
        });
        match result {
            Ok(report) => {
                if publish {
                    for observer in self.state.civil_clock.disclosure_observers() {
                        observer(report.disclosure_instant);
                    }
                }
                Ok(report)
            }
            Err(error) => {
                *self = checkpoint;
                Err(error)
            }
        }
    }

    fn end_civil_day_after_session_check(&mut self) -> Result<CivilDayEndReport, SessionError> {
        let mut report = self
            .state
            .civil_clock
            .end_day(self.state.civil_clock.current_date())?;
        let account_positions = self
            .state
            .accounts
            .iter()
            .map(|(id, account)| {
                (
                    *id,
                    account
                        .positions()
                        .iter()
                        .map(|(code, position)| (code.clone(), u64::from(position.qty())))
                        .collect(),
                )
            })
            .collect();
        self.state
            .corporate_actions
            .close_registries_through(
                report.settled_date,
                &account_positions,
                &self.state.personal_trade_confirmations,
                self.issuer_repurchase_account_id(),
            )
            .map_err(|error| SessionError::InvalidSave(format!("股东名册日终推进失败：{error}")))?;
        let correction_publications = self.apply_report_corrections_at_day_end(&report)?;
        let issuers = self.state.company_system.issuers().clone();
        self.state
            .corporate_actions
            .process_dividends_on_day_end(
                report.settled_date,
                self.state.civil_clock.calendar(),
                &issuers,
                std::sync::Arc::make_mut(&mut self.state.company_system),
                &mut self.state.accounts,
            )
            .map_err(|error| SessionError::InvalidSave(format!("现金分红日终结算失败：{error}")))?;
        self.state
            .corporate_actions
            .process_stock_distributions_on_day_end(
                report.settled_date,
                self.state.civil_clock.calendar(),
                std::sync::Arc::make_mut(&mut self.state.company_system),
                &mut self.state.accounts,
            )
            .map_err(|error| SessionError::InvalidSave(format!("送转日终结算失败：{error}")))?;
        self.state
            .corporate_actions
            .process_rights_offerings_on_day_end(
                report.settled_date,
                self.state.civil_clock.calendar(),
                std::sync::Arc::make_mut(&mut self.state.company_system),
                &mut self.state.accounts,
            )
            .map_err(|error| SessionError::InvalidSave(format!("配股日终结算失败：{error}")))?;
        let issuer_fills = self.collect_issuer_repurchase_fills(report.settled_date)?;
        let repurchase_account_id = self.issuer_repurchase_account_id();
        let repurchase_lot_size = self.state.setup.config.lot_size;
        let repurchase_account_for_validate = repurchase_account_id;
        self.state
            .corporate_actions
            .process_issuer_repurchases_on_day_end(
                report.settled_date,
                &issuer_fills,
                &mut self.state.accounts,
                repurchase_account_id,
                repurchase_lot_size,
                Some(std::sync::Arc::make_mut(&mut self.state.company_system)),
            )
            .map_err(|error| SessionError::InvalidSave(format!("发行人回购日终结算失败：{error}")))?;
        let account_positions = self
            .state
            .accounts
            .iter()
            .map(|(id, account)| {
                (
                    *id,
                    account
                        .positions()
                        .iter()
                        .map(|(code, position)| (code.clone(), u64::from(position.qty())))
                        .collect(),
                )
            })
            .collect();
        self.state
            .corporate_actions
            .validate(
                &account_positions,
                &self.state.company_system,
                report.settled_date,
                repurchase_account_for_validate,
            )
            .map_err(|error| {
                SessionError::InvalidSave(format!("日终公司行为与Simple账务勾稽失败：{error}"))
            })?;
        std::sync::Arc::make_mut(&mut self.state.company_system)
            .advance_day(report.settled_date)
            .map_err(|error| SessionError::InvalidSave(format!("公司日终推进失败：{error}")))?;
        if self
            .state
            .company_system
            .settlement_completed_on(report.settled_date)
            .map_err(|error| SessionError::InvalidSave(format!("结算周期判定失败：{error}")))?
        {
            self.process_simple_preference_proposals(report.settled_date)
                .map_err(|error| {
                    SessionError::InvalidSave(format!("公司行为偏好提案失败：{error}"))
                })?;
        }
        let mut disclosures = self
            .state
            .disclosures
            .run_simple_day_end(disclosures::SimpleDayEndDisclosureCtx {
                report_frequency: self.state.setup.report_frequency,
                report: &report,
                system: &self.state.company_system,
                seed: self.state.seed,
                dividends: &self.state.corporate_actions.dividends,
                rights_offerings: &self.state.corporate_actions.rights_offerings,
                issuer_repurchases: &self.state.corporate_actions.issuer_repurchases,
                library: std::sync::Arc::make_mut(&mut self.state.library),
            })
            .map_err(SessionError::Disclosure)?;
        disclosures
            .reports_published
            .extend(correction_publications);
        let disclosure_limit = if matches!(
            self.state.setup.report_frequency,
            crate::information::ReportFrequency::Monthly { .. }
        ) {
            CivilInstant::new(report.settled_date, 86399)
                .map_err(crate::calendar::CalendarError::from)?
        } else {
            report.disclosure_instant
        };
        let mut information_instants =
            std::collections::BTreeSet::from([report.disclosure_instant]);
        for id in &disclosures.reports_published {
            information_instants.insert(
                self.state
                    .library
                    .report(*id, disclosure_limit)
                    .map_err(|error| SessionError::Information(Box::new(error)))?
                    .published_at,
            );
        }
        for instant in information_instants {
            self.deliver_public_information(instant)?;
        }
        self.record_civil_day_events(&mut report, disclosures)?;
        self.prune_all_personal_memories();
        self.finish_retained_history(report.settled_date)?;
        Ok(report)
    }

    fn record_civil_day_events(
        &mut self,
        report: &mut CivilDayEndReport,
        disclosures: DayEndDisclosures,
    ) -> Result<(), SessionError> {
        self.record_company_disclosure_events(report, disclosures)?;
        report.events.push(Event::CivilDateAdvanced {
            seq: self.next_seq(),
            settled_date: report.settled_date,
            next_date: report.next_date,
            next_status: report.next_status.clone(),
        });
        Ok(())
    }

    fn record_company_disclosure_events(
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
                seq: self.next_seq(),
                publication_id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Announcement,
            });
        }
        for publication_id in disclosures.reports_published {
            let limit = CivilInstant::new(report.settled_date, 86399)
                .map_err(crate::calendar::CalendarError::from)?;
            let (company, published_at, report_revision) = self
                .state
                .library
                .report(publication_id, limit)
                .map(|published| {
                    (
                        published.company.clone(),
                        published.published_at,
                        published.reports.version.sequence,
                    )
                })
                .map_err(|error| SessionError::Information(Box::new(error)))?;
            report.events.push(Event::CompanyDisclosurePublished {
                seq: self.next_seq(),
                publication_id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Report { report_revision },
            });
        }
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
        code: StockCode,
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
        let auction_reserved = self
            .state
            .auction_orders
            .values()
            .flatten()
            .filter(|order| order.owner == account)
            .try_fold(Money::ZERO, |total, order| {
                let required = live_cash_reservation(
                    &self.state.setup.config,
                    order.side,
                    order.limit,
                    order.qty,
                    Money::ZERO,
                )?;
                total.add(required)
            })?;
        self.state
            .markets
            .values()
            .flat_map(|market| market.resting_orders_for(account))
            .try_fold(auction_reserved, |total, order| {
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

    fn register_npc_order_lifecycle_at_quote(
        &mut self,
        account: AccountId,
        code: &StockCode,
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
    fn npc_quote_lifetime_minutes(
        &self,
        account: AccountId,
        code: &StockCode,
        order: &Order,
    ) -> u64 {
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
        code: &StockCode,
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
            AccountKind::Player | AccountKind::IssuerRepurchase => {
                panic!("player/issuer orders must not receive NPC quote lifecycles")
            }
        };
        let tick_cents = self
            .state
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
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
        let deterministic_jitter = self.state.seed
            ^ account.0.rotate_left(17)
            ^ order.id.0.rotate_left(31)
            ^ stock_code_hash(code);
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
        code: &StockCode,
        order_id: OrderId,
    ) -> bool {
        self.state
            .parent_orders
            .get(&account)
            .and_then(|plans| plans.get(code))
            .is_some_and(|plan| plan.active_child_order_id() == Some(order_id))
    }

    fn remove_npc_order_lifecycle(
        &mut self,
        account: AccountId,
        code: &StockCode,
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
        self.validate_player_calendar(&intent)?;
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
                            cash_ex_reference_pending_trade: market
                                .cash_ex_reference_pending_trade(),
                            day_market_activity: market.day_market_activity(),
                            last_cash_ex_reference: market.last_cash_ex_reference(),
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
            retained_market_history: self.state.retained_market_history.saved_days(),
            market_memberships: self.state.memberships.clone(),
            report_correction_operations: self.state.report_correction_operations.clone(),
            runtime_state,
            setup: self.state.setup.clone(),
            seed: self.state.seed,
            snapshot: self.save_snapshot_projection(),
            auction_orders: self.state.auction_orders.clone(),
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
            parent_orders: self
                .state
                .parent_orders
                .iter()
                .map(|(account, plans)| {
                    (
                        *account,
                        plans
                            .iter()
                            .map(|(code, plan)| (code.clone(), SaveParentOrderPlan::from(plan)))
                            .collect(),
                    )
                })
                .collect(),
            npc_order_lifecycles: self.state.npc_order_lifecycles.clone(),
            pending_player: self.state.pending_player.clone(),
            pending_npc: self.state.pending_npc.clone(),
            ingress_receipt_cursors: self.state.ingress_receipt_cursors.clone(),
            next_order_id: self.state.next_order_id,
            civil_clock: self.state.civil_clock.save(),
            corporate_actions: self.state.corporate_actions.clone(),
            // 权威状态连续性（完整存档）：公司域与个体决策链权威状态全量入档。
            company_system: self.state.company_system.export_state(),
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
            // 存档契约只保留「计划簿中仍存活」的待应用事实；未知/已终止计划
            // 的迟到条目在此按完整存档契约显式丢弃（永不适用）。
            pending_plan_events: self
                .state
                .pending_plan_events
                .iter()
                .copied()
                .filter(|event| {
                    self.state
                        .plans
                        .plan(event.plan_id())
                        .is_ok_and(|plan| !plan.is_terminal())
                })
                .collect(),
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
        save.urgency_policy
            .validate()
            .map_err(|error| SessionError::InvalidSave(format!("urgency_policy: {error}")))?;
        validate_save_slot(save)?;
        retained_history::validate_saved_history(save)?;
        let mut sess = GameSession::new(save.setup.clone(), save.seed)?;
        sess.state.retained_market_history =
            retained_history::RetainedMarketHistory::restore_days(&save.retained_market_history);
        sess.fresh_initial_allocation = false;
        sess.state.memberships = save.market_memberships.clone();
        for member in save.market_memberships.members.values() {
            if !sess.state.accounts.contains_key(&member.account_id) {
                sess.state.accounts.insert(
                    member.account_id,
                    Account::new(member.account_id, AccountKind::Player, Money::ZERO),
                );
            }
        }

        // 清空初始持仓分配 → 用快照精确覆盖
        for acc in sess.state.accounts.values_mut() {
            acc.restore_balances(acc.cash(), BTreeMap::new());
        }

        // 恢复账户状态（cash + positions 精确值）
        for (id, snap_acc) in &save.snapshot.accounts {
            let acc = sess
                .state
                .accounts
                .get_mut(id)
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
            let configured_tick = save
                .setup
                .stocks
                .iter()
                .find(|spec| &spec.code == code)
                .expect("validated save market set exactly matches setup")
                .tick;
            Market::validate_restored_facts(
                code,
                save.civil_clock.current_date,
                snap_mkt.last_price,
                snap_mkt.last_close,
                snap_mkt.cash_ex_reference_pending_trade,
                snap_mkt.last_cash_ex_reference,
                configured_tick,
            )
            .map_err(|error| {
                SessionError::InvalidSave(format!(
                    "market {:?} restore facts invalid: {error}",
                    code
                ))
            })?;
            if !snap_mkt.day_market_activity
                && (save
                    .resting_orders
                    .get(code)
                    .is_some_and(|orders| !orders.is_empty())
                    || save
                        .auction_orders
                        .get(code)
                        .is_some_and(|orders| !orders.is_empty()))
            {
                return Err(SessionError::InvalidSave(format!(
                    "market {:?} has active orders without day market activity",
                    code
                )));
            }
            let market = sess
                .state
                .markets
                .get_mut(code)
                .expect("validated save market set exactly matches setup");
            market.restore_prices(
                snap_mkt.last_price,
                snap_mkt.last_close,
                snap_mkt.cash_ex_reference_pending_trade,
                snap_mkt.day_market_activity,
                snap_mkt.last_cash_ex_reference,
            );
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
                .map_err(|error| {
                    SessionError::InvalidSave(format!(
                        "cannot restore resting orders for {}: {error}",
                        code.0
                    ))
                })?;
        }
        for (code, filled) in &save.filled_orders {
            sess.state
                .markets
                .get_mut(code)
                .expect("validated filled-order market must exist")
                .restore_filled_orders(filled.iter().map(|order| (order.id, order.owner)))
                .map_err(|error| {
                    SessionError::InvalidSave(format!(
                        "cannot restore filled orders for {}: {error}",
                        code.0
                    ))
                })?;
        }
        // 恢复进度
        sess.state.tick = save.snapshot.tick;
        sess.state.day = u32::try_from(save.snapshot.tick / save.setup.ticks_per_day)
            .map_err(|_| SessionError::InvalidSave("saved trading day exceeds u32".to_owned()))?;
        sess.state.seq = save.snapshot.seq;
        // 恢复自然日时钟；全量校验其自洽性，并使用随存档冻结的政策。
        sess.state.civil_clock = CivilClock::from_parts_for_stocks(
            save.setup.start_date,
            &save.civil_clock,
            save.setup.stocks.iter().map(|stock| {
                (
                    stock.code.clone(),
                    session_calendar_exchange(stock.exchange),
                )
            }),
        )?;
        validate_saved_order_state(&sess, save)?;
        sess.state.auction_orders = save.auction_orders.clone();
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
                return Err(SessionError::InvalidSave(format!(
                    "NPC {} attention probability {} does not match its deterministic profile {}",
                    id.0, saved_state.base_probability, reconstructed.base_probability
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
        sess.state.parent_orders = save
            .parent_orders
            .iter()
            .map(|(account, plans)| {
                (
                    *account,
                    plans
                        .iter()
                        .map(|(code, plan)| (code.clone(), ParentOrderPlan::from(plan.clone())))
                        .collect(),
                )
            })
            .collect();
        for (account, plans) in &mut sess.state.parent_orders {
            for (code, plan) in plans {
                let Some(active_id) = plan.active_child_order_id() else {
                    plan.restore_active_child_remaining_qty(None);
                    continue;
                };
                let active_quantities = save
                    .auction_orders
                    .get(code)
                    .into_iter()
                    .flatten()
                    .filter(|order| {
                        order.order_id == active_id.0
                            && order.owner == *account
                            && order.side == plan.side()
                    })
                    .map(|order| order.qty)
                    .chain(
                        save.resting_orders
                            .get(code)
                            .into_iter()
                            .flatten()
                            .filter(|order| {
                                order.id == active_id
                                    && order.owner == *account
                                    && order.side == plan.side()
                            })
                            .map(|order| order.qty),
                    )
                    .collect::<Vec<_>>();
                let [remaining_qty] = active_quantities.as_slice() else {
                    return Err(SessionError::InvalidSave(format!(
                        "parent-order account {} stock {} active child cannot be uniquely rebuilt",
                        account.0, code.0
                    )));
                };
                plan.restore_active_child_remaining_qty(Some(*remaining_qty));
            }
        }
        sess.state.npc_order_lifecycles = save.npc_order_lifecycles.clone();
        sess.state.pending_player = save.pending_player.clone();
        sess.state.pending_npc = save.pending_npc.clone();
        sess.state.ingress_receipt_cursors = save.ingress_receipt_cursors.clone();
        // 诊断缓存不属于存档事实，恢复后由实际 tick 重新生成。
        sess.state.last_retail_decisions.clear();

        // 权威状态连续性（完整存档）：公司域与个体决策链权威状态直接从档恢复——不再前史
        // 重放、不再复位信念/计划/信息集、不再剥离 linked_plan_id。new() 重建
        // 的 prehistory/时钟接线是确定性产物，被下列赋值整体覆盖。
        if sess.state.company_system.issuers() != save.company_system.issuers()
            && !persistence::issuer_increase_matches_non_trading_issuance(
                save,
                sess.state.company_system.issuers(),
            )?
        {
            return Err(SessionError::InvalidSave(
                "saved company set does not exactly match the issuers rebuilt from setup"
                    .to_string(),
            ));
        }
        // 个体状态账户集合精确匹配确定性重建（populate_npcs 按 seed+ordinal
        // 重建信念机构集合）：缺失任一账户的个人状态 = 不完整存档。
        let expected_belief_accounts: BTreeSet<AccountId> =
            sess.state.belief_participants.keys().copied().collect();
        let saved_belief_accounts: BTreeSet<AccountId> =
            save.belief_books.keys().copied().collect();
        if expected_belief_accounts != saved_belief_accounts {
            return Err(SessionError::InvalidSave(format!(
                "saved personal-state account set {saved_belief_accounts:?} does not match the \
                 reconstructed belief accounts {expected_belief_accounts:?}"
            )));
        }
        sess.state.company_system = std::sync::Arc::new(save.company_system.export_state());
        sess.state.corporate_actions = save.corporate_actions.clone();
        sess.state.report_correction_operations = save.report_correction_operations.clone();
        sess.state.library = std::sync::Arc::new(save.public_library.clone());
        sess.state.disclosures = save.disclosures.clone();
        sess.state.plans = save.plans.clone();
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
        sess.state.pending_plan_events = save.pending_plan_events.clone();
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
    let encoded = serde_json::to_string(&value).map_err(|error| {
        SessionError::InvalidSetup(format!("cannot encode finite f64: {error}"))
    })?;
    serde_json::from_str(&encoded)
        .map_err(|error| SessionError::InvalidSetup(format!("cannot decode finite f64: {error}")))
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
            company_system: simple_company_fixture!(crate; ["600999"]),
            stocks: vec![StockSpec {
                code: StockCode("600999".to_string()),
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
            report_frequency: crate::information::ReportFrequency::Quarterly,
            float_allocation: FloatAllocation::random(),
            start_date: default_civil_start_date(),
            simulation_policy_id: SIMULATION_POLICY_ID.to_string(),
            dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
            rights_offering_enabled: false,
            issuer_repurchase_enabled: false,
        }
    }

    #[test]
    fn first_auction_trade_replaces_provisional_previous_close_on_gap_up_days() {
        let code = StockCode("600999".to_string());
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
        let code = StockCode("600999".to_string());
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
    /// Seed a passive order for tests of downstream stages without advancing a market tick.
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
                    self.state
                        .auction_orders
                        .entry(code.clone())
                        .or_default()
                        .push(AuctionOrderSnap {
                            owner: account,
                            side,
                            limit: price,
                            qty,
                            order_id: id.0,
                        });
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
                    self.record_parent_order_canceled(account, &code, id);
                    events.push(Event::OrderCanceled {
                        seq: self.next_seq(),
                        account,
                        code,
                        id,
                        remaining_qty: order.qty,
                    });
                } else {
                    let orders = self
                        .state
                        .auction_orders
                        .get_mut(&code)
                        .expect("fixture stock exists");
                    let index = orders
                        .iter()
                        .position(|order| order.order_id == id.0)
                        .expect("fixture auction order exists");
                    let order = orders.remove(index);
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
        let code = StockCode("600888".to_string());
        SessionSetup {
            company_system: simple_company_fixture!(crate; ["600888"]),
            stocks: vec![StockSpec {
                code: code.clone(),
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
            report_frequency: crate::information::ReportFrequency::Quarterly,
            float_allocation: FloatAllocation::random(),
            start_date: default_civil_start_date(),
            simulation_policy_id: SIMULATION_POLICY_ID.to_string(),
            dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
            rights_offering_enabled: false,
            issuer_repurchase_enabled: false,
        }
    }

    #[test]
    fn restore_reconciles_edited_institution_holdings_without_fake_buys() {
        let session = GameSession::new(quote_setup(0), 42).unwrap();
        let mut save = session.save().unwrap();
        let account_id = *save.belief_books.keys().next().unwrap();
        let code = save.setup.stocks[0].code.clone();
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

    #[test]
    fn saved_institution_feedback_rejects_future_clocks_and_invalid_failure_references() {
        let session = GameSession::new(quote_setup(0), 42).unwrap();
        let save = session.save().unwrap();
        let account_id = *save.belief_books.keys().next().unwrap();
        let code = save.setup.stocks[0].code.clone();
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
                code: StockCode("999999".to_owned()),
                order_id: Some(0),
                moment: current,
            });
        assert!(matches!(
            persistence::validate_save_slot(&invalid_reference),
            Err(SessionError::InvalidSave(message)) if message.contains("invalid failure experience")
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
            Err(SessionError::InvalidSave(message)) if message.contains("future experience clocks")
                || message.contains("invalid failure experience")
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
                code: save.setup.stocks[0].code.clone(),
                order_id: None,
                moment,
            });
        assert!(matches!(GameSession::restore(&save),
            Err(SessionError::InvalidSave(message)) if message.contains("invalid failure experience")));
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
                    save.setup.stocks[0].code.clone(),
                    crate::experience::RetailStockExperience {
                        last_buy_price: price,
                        last_buy_order_id: order,
                        adverse_move_recorded: adverse,
                        ..Default::default()
                    },
                );
            assert!(
                matches!(GameSession::restore(&invalid),
                Err(SessionError::InvalidSave(message)) if message.contains("invalid trade experience")),
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

    fn buy(code: &StockCode, price: i64) -> Intent {
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(price)),
            qty: 100,
        }
    }

    pub(super) fn two_stock_quote_setup() -> SessionSetup {
        let mut setup = quote_setup(0);
        let second = StockCode("600889".to_string());
        let mut spec = setup.stocks[0].clone();
        spec.code = second;
        setup.stocks.push(spec);
        setup.company_system = simple_company_fixture!(crate; ["600888", "600889"]);
        setup
    }

    pub(super) fn force_attention_candidate(
        session: &mut GameSession,
        account: AccountId,
        tick: u64,
    ) {
        let attention = session.state.npc_attention.get_mut(&account).unwrap();
        attention.base_probability = 1.0;
        attention.next_attention_candidate_tick = tick;
        session.state.attention_scheduler.enqueue(tick, account);
    }

    #[test]
    fn accepted_retail_observation_persists_an_unheld_watchlist_stock() {
        let code = StockCode("600888".to_string());
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
        assert_eq!(
            stock.last_observed_market_minute,
            session.current_market_minute()
        );
    }

    #[test]
    fn parent_buy_does_not_overbuy_when_an_odd_lot_partial_fill_leaves_a_sub_lot_target() {
        let code = StockCode("600888".to_string());
        let institution = AccountId(1);
        let mut session = GameSession::new(quote_setup(0), 992).unwrap();
        session
            .state
            .parent_orders
            .entry(institution)
            .or_default()
            .insert(
                code.clone(),
                ParentOrderPlan::from_saved_facts(
                    code.clone(),
                    Side::Buy,
                    400,
                    350,
                    100,
                    None,
                    None,
                    None,
                    Money::from_cents(1_000),
                    PARENT_ORDER_HORIZON_MINUTES,
                ),
            );

        let mut desired = Vec::new();
        session.append_parent_child_or_working_intent(
            institution,
            &code,
            0,
            WorkingOrderSlices {
                continuous: &[],
                auction: &[],
            },
            &mut desired,
        );

        assert!(desired.is_empty(), "不足一手的残余不得触发超额或非法买单");
        assert_eq!(
            session.state.parent_orders[&institution][&code].remaining_qty(),
            50
        );
    }

    #[test]
    fn institution_symbolic_intent_bypasses_parent_order_and_supersedes_old_target() {
        let code = StockCode("600888".to_string());
        let institution = AccountId(1);
        let mut session = GameSession::new(quote_setup(0), 994).unwrap();
        let empty_working = WorkingOrderSlices {
            continuous: &[],
            auction: &[],
        };
        let fixed = buy(&code, 1_000);
        let initial =
            session.materialize_parent_order_intents(institution, vec![fixed], 0, empty_working);
        assert!(matches!(
            initial.as_slice(),
            [Intent::PlaceLimit {
                price: LimitPrice::Fixed(_),
                ..
            }]
        ));
        assert!(session.state.parent_orders[&institution].contains_key(&code));

        let symbolic = Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Highest,
            qty: 100,
        };
        let desired =
            session.materialize_parent_order_intents(institution, vec![symbolic], 0, empty_working);
        assert!(matches!(
            desired.as_slice(),
            [Intent::PlaceLimit {
                price: LimitPrice::Highest,
                ..
            }]
        ));
        assert!(!session.state.parent_orders.contains_key(&institution));
    }

    #[test]
    fn mixed_fixed_and_symbolic_intents_survive_with_or_without_an_old_parent() {
        let code = StockCode("600888".to_string());
        let institution = AccountId(1);
        let empty_working = WorkingOrderSlices {
            continuous: &[],
            auction: &[],
        };
        let mut baseline = None;
        for with_old_parent in [false, true] {
            let mut session = GameSession::new(quote_setup(0), 995).unwrap();
            if with_old_parent {
                session.materialize_parent_order_intents(
                    institution,
                    vec![Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: LimitPrice::Fixed(Money::from_cents(950)),
                        qty: 400,
                    }],
                    0,
                    empty_working,
                );
            }
            let desired = session.materialize_parent_order_intents(
                institution,
                vec![
                    buy(&code, 1_000),
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: LimitPrice::Highest,
                        qty: 100,
                    },
                ],
                0,
                empty_working,
            );
            assert!(matches!(
                desired.as_slice(),
                [
                    Intent::PlaceLimit {
                        price: LimitPrice::Highest,
                        ..
                    },
                    Intent::PlaceLimit {
                        price: LimitPrice::Fixed(price),
                        ..
                    }
                ] if *price == Money::from_cents(1_000)
            ));
            let parent = &session.state.parent_orders[&institution][&code];
            assert_eq!(parent.target_qty(), 100);
            assert_eq!(parent.limit_price(), Money::from_cents(1_000));
            let result = (
                serde_json::to_value(&desired).unwrap(),
                serde_json::to_value(parent).unwrap(),
            );
            if let Some(expected) = &baseline {
                assert_eq!(&result, expected);
            } else {
                baseline = Some(result);
            }
        }
    }

    #[test]
    fn closing_auction_does_not_duplicate_a_parent_child_still_resting_in_continuous_book() {
        let code = StockCode("600888".to_string());
        let institution = AccountId(1);
        let mut setup = quote_setup(0);
        setup.ticks_per_day = 100;
        setup.closing_auction_ticks = 10;
        let mut session = GameSession::new(setup, 993).unwrap();
        session.state.tick = 90;
        session
            .state
            .parent_orders
            .entry(institution)
            .or_default()
            .insert(
                code.clone(),
                ParentOrderPlan::from_saved_facts(
                    code.clone(),
                    Side::Buy,
                    400,
                    0,
                    100,
                    Some(OrderId(1)),
                    Some(100),
                    None,
                    Money::from_cents(1_000),
                    PARENT_ORDER_HORIZON_MINUTES * 2,
                ),
            );
        let continuous = [(
            code.clone(),
            Order {
                id: OrderId(1),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: institution,
                seq: 0,
            },
        )];
        let mut desired = Vec::new();
        session.append_parent_child_or_working_intent(
            institution,
            &code,
            0,
            WorkingOrderSlices {
                continuous: &continuous,
                auction: &[],
            },
            &mut desired,
        );

        assert!(desired.is_empty(), "收盘集合竞价不得复制连续簿中的母单子单");
    }

    #[test]
    fn closing_auction_restores_a_parent_child_that_remains_in_continuous_book() {
        let code = StockCode("600888".to_string());
        let institution = AccountId(1);
        let mut setup = quote_setup(0);
        setup.ticks_per_day = 2;
        setup.closing_auction_ticks = 1;
        let mut session = GameSession::new(setup, 994).unwrap();
        session.step().expect("healthy step");
        assert_eq!(session.phase(), TradingPhase::ClosingAuction);
        session
            .state
            .markets
            .get_mut(&code)
            .unwrap()
            .place(Order {
                id: OrderId(1),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: institution,
                seq: 0,
            })
            .unwrap();
        session.state.next_order_id = 2;
        session
            .state
            .parent_orders
            .entry(institution)
            .or_default()
            .insert(
                code.clone(),
                ParentOrderPlan::from_saved_facts(
                    code.clone(),
                    Side::Buy,
                    400,
                    0,
                    100,
                    Some(OrderId(1)),
                    Some(100),
                    None,
                    Money::from_cents(1_000),
                    PARENT_ORDER_HORIZON_MINUTES * 2,
                ),
            );

        session
            .hydrate_or_validate_envelope_ledger()
            .expect("direct order-book fixture must synchronize current save authority");

        let restored = GameSession::restore(&session.save().expect("healthy save")).unwrap();
        assert_eq!(restored.state.parent_orders, session.state.parent_orders);
    }

    #[test]
    fn market_view_keeps_tick_samples_separate_from_completed_market_minutes() {
        let code = StockCode("600888".to_string());
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
        let code = StockCode("600888".to_string());
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
        let code = StockCode("600888".to_string());
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
        let code = StockCode("600888".to_string());
        let account = AccountId(1);
        let mut session = GameSession::new(retail_quote_setup(), 121).unwrap();
        let config = session.state.setup.config.clone();
        let holder = session.state.accounts.get_mut(&account).unwrap();
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
    fn clone_for_shadow(&self) -> Result<Self, StepFatal> {
        let Self {
            retained_market_history,
            memberships,
            pending_report_corrections,
            report_correction_operations,
            setup,
            rng,
            seed,
            markets,
            accounts,
            history_reads,
            price_history,
            market_minute_closes,
            candle_book,
            auction_orders,
            pending_player,
            pending_npc,
            ingress_receipt_cursors,
            npc_attention,
            retail_experience,
            parent_orders,
            pending_plan_events,
            npc_order_lifecycles,
            last_retail_decisions,
            last_retail_order_events,
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces,
            #[cfg(feature = "simulation-diagnostics")]
            causal,
            attention_scheduler,
            company_system,
            corporate_actions,
            library,
            disclosures,
            plans,
            urgency_policy,
            belief_participants,
            envelope_ledger,
            retail_projection_seen,
            personal_trade_confirmations,
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
            retained_market_history: retained_market_history.clone(),
            memberships: memberships.clone(),
            pending_report_corrections: pending_report_corrections.clone(),
            report_correction_operations: report_correction_operations.clone(),
            setup: setup.clone(),
            rng: rng.clone(),
            seed: *seed,
            markets: markets.clone(),
            accounts,
            history_reads: history_reads.clone(),
            price_history: price_history.clone(),
            market_minute_closes: market_minute_closes.clone(),
            candle_book: candle_book.clone(),
            auction_orders: auction_orders.clone(),
            pending_player: pending_player.clone(),
            pending_npc: pending_npc.clone(),
            ingress_receipt_cursors: ingress_receipt_cursors.clone(),
            npc_attention: npc_attention.clone(),
            retail_experience: retail_experience.clone(),
            parent_orders: parent_orders.clone(),
            pending_plan_events: pending_plan_events.clone(),
            npc_order_lifecycles: npc_order_lifecycles.clone(),
            last_retail_decisions: last_retail_decisions.clone(),
            last_retail_order_events: last_retail_order_events.clone(),
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces: npc_decision_traces.clone(),
            #[cfg(feature = "simulation-diagnostics")]
            causal: causal.clone(),
            attention_scheduler: attention_scheduler.clone(),
            company_system: company_system.clone(),
            corporate_actions: corporate_actions.clone(),
            library: library.clone(),
            disclosures: disclosures.clone(),
            plans: plans.clone(),
            urgency_policy: *urgency_policy,
            belief_participants: belief_participants.clone(),
            envelope_ledger: envelope_ledger.clone(),
            retail_projection_seen: retail_projection_seen.clone(),
            personal_trade_confirmations: personal_trade_confirmations.clone(),
            next_receipt_base: *next_receipt_base,
            next_order_id: *next_order_id,
            tick: *tick,
            day: *day,
            seq: *seq,
            civil_clock: civil_clock.clone(),
        })
    }

    fn commit_from(&mut self, shadow: Self) {
        let Self {
            retained_market_history,
            memberships,
            pending_report_corrections,
            report_correction_operations,
            setup,
            rng,
            seed,
            markets,
            accounts,
            history_reads,
            price_history,
            market_minute_closes,
            candle_book,
            auction_orders,
            pending_player,
            pending_npc,
            ingress_receipt_cursors,
            npc_attention,
            retail_experience,
            parent_orders,
            pending_plan_events,
            npc_order_lifecycles,
            last_retail_decisions,
            last_retail_order_events,
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces,
            #[cfg(feature = "simulation-diagnostics")]
            causal,
            attention_scheduler,
            company_system,
            corporate_actions,
            library,
            disclosures,
            plans,
            belief_participants,
            envelope_ledger,
            retail_projection_seen,
            personal_trade_confirmations,
            next_receipt_base,
            next_order_id,
            tick,
            day,
            seq,
            civil_clock,
            urgency_policy,
        } = shadow;
        self.retained_market_history = retained_market_history;
        self.memberships = memberships;
        self.pending_report_corrections = pending_report_corrections;
        self.report_correction_operations = report_correction_operations;
        self.urgency_policy = urgency_policy;
        self.setup = setup;
        self.rng = rng;
        self.seed = seed;
        self.markets = markets;
        self.accounts.replace_and_drop_parallel(accounts);
        self.history_reads.replace_and_drop_parallel(history_reads);
        self.price_history = price_history;
        self.market_minute_closes = market_minute_closes;
        self.candle_book = candle_book;
        self.auction_orders = auction_orders;
        self.pending_player = pending_player;
        self.pending_npc = pending_npc;
        self.ingress_receipt_cursors = ingress_receipt_cursors;
        self.npc_attention.replace_and_drop_parallel(npc_attention);
        self.retail_experience
            .replace_and_drop_parallel(retail_experience);
        self.parent_orders = parent_orders;
        self.pending_plan_events = pending_plan_events;
        self.npc_order_lifecycles = npc_order_lifecycles;
        self.last_retail_decisions = last_retail_decisions;
        self.last_retail_order_events = last_retail_order_events;
        #[cfg(feature = "simulation-diagnostics")]
        {
            self.npc_decision_traces = npc_decision_traces;
            self.causal = causal;
        }
        self.attention_scheduler = attention_scheduler;
        self.company_system = company_system;
        self.corporate_actions = corporate_actions;
        self.library = library;
        self.disclosures = disclosures;
        self.plans = plans;
        self.belief_participants = belief_participants;
        self.envelope_ledger = envelope_ledger;
        self.retail_projection_seen = retail_projection_seen;
        self.personal_trade_confirmations = personal_trade_confirmations;
        self.next_receipt_base = next_receipt_base;
        self.next_order_id = next_order_id;
        self.tick = tick;
        self.day = day;
        self.seq = seq;
        self.civil_clock = civil_clock;
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

    #[test]
    fn shadow_keeps_the_complete_save_projection_and_resets_facade_hooks() {
        let mut session =
            GameSession::new(npc_working_quote_tests::retail_quote_setup(), 717).unwrap();
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::Cancel {
                    code: StockCode("600888".to_owned()),
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
        let mut shadow = session.clone_for_tick_shadow().unwrap();
        shadow
            .enqueue_player_intent(
                AccountId(0),
                Intent::Cancel {
                    code: StockCode("600888".to_owned()),
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
                code: StockCode("600888".to_owned()),
                order_id: OrderId(103),
                remaining_qty: 100,
            });
        let expected = serde_json::to_value(shadow.save().unwrap()).unwrap();
        let expected_rng = shadow.state.rng.state;
        session.poison = Some(fatal("preserved poison"));
        session.injected_failure = Some(fatal("preserved injection"));
        session.post_shadow_failure = Some(fatal("preserved post-shadow"));

        session.commit_tick_shadow(shadow);

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
}
