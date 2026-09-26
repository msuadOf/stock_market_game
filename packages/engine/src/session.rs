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
mod company_operations;
mod continuous_cancellation;
mod decision_chain;
mod disclosures;
mod envelope_projection;
mod execution;
mod failure;
mod hash;
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
mod player_candidates_tests;
#[cfg(test)]
mod reconciliation_plan_phase_tests;
#[cfg(test)]
mod reconciliation_plan_tests;

use account_book::AccountBook;
use account_paged_map::AccountPagedMap;
use candles::{generate_preset_daily_candles, stock_code_hash, DailyCandleHistory};
use civil_clock::{default_civil_start_date, session_calendar_exchange};
use persistence::{validate_save_slot, validate_saved_order_state};

pub use attention::NpcAttentionState;
pub use civil_clock::{
    CivilClock, CivilClockError, CivilClockSave, CivilDayEndReport, CivilPhase, DueBusiness,
    DueBusinessId, DueKind,
};
pub use company_operations::{CompanyOperationsClockWiring, CompanyOperationsSeamError};
pub use decision_chain::{BeliefDebugSummary, DecisionChainDiagnostics};
pub use disclosures::{
    disclosure_phase_observer, DayEndDisclosureCtx, DayEndDisclosures, DisclosureDispatch,
    DisclosureError,
};
pub use execution::ParentOrderPlan;
pub use persistence::{
    decode_save_slot, EnvelopeAuditV2, EnvelopeKeyV2, FeeComponentsV2, JournalRankV2,
    LiveEnvelopeV2, ReceiptLocalKeyV2, ReceiptSourceV2, ReceiptTransitionV2, ResourceV2,
    RetailReceiptIdentityV2, SaveDecodeLimits, SaveRuntimeV2, MAX_SAVE_DECODE_BYTES,
    SAVE_SCHEMA_VERSION_V2, SIMULATION_POLICY_ID_V2,
};
pub use plan_execution::{
    PendingPlanEvent, PlanExecutionDisposition, PlanExecutionError, PlanExecutionReport,
    PlanExecutionRequest,
};
pub use snapshot::{AccountSnap, MarketSnap, PositionSnap, Snapshot};

use attention::{maximum_observation_probability, sample_attention_wait};

use crate::account::{Account, AccountError, AccountKind, Position, SettlementTotals, StockCode};
#[cfg(test)]
use crate::behavior::BehaviorMarketObservation;
use crate::behavior::PositionDecision;
use crate::calendar::{CivilDate, CivilInstant, DayStatus, TradingCalendar};
use crate::company::CompanyId;
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
    Intent, MarketView, StockView, StrategyError, StrategyFactory, StrategyParams, StrategyProfile,
};
#[cfg(test)]
use crate::strategy::{PositionView, SelfView};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
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
        account: AccountId,
        code: StockCode,
        reason: RejectionReason,
    },
    /// 结算失败（账户侧异常，透传 AccountError 文案）。
    SettlementError {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
        account: AccountId,
        code: StockCode,
        reason: String,
    },

    /// 连续竞价委托已撤销，冻结资金或股份随即释放。
    OrderCanceled {
        #[serde(with = "crate::orderbook::js_safe_u64")]
        #[ts(type = "number")]
        seq: u64,
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

/// 单个交易日的 OHLCV。价格全部为分，time 为游戏内相对 Unix 秒：第 0 日为 0，
/// 启动预置历史使用负数，确保前端图表可直接按时间排序。
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
    /// 存档契约版本。v1 及缺失版本均显式拒绝，不提供迁移器。
    pub schema_version: u32,
    /// escrow 并行 tick 新增的权威运行时状态。TypeScript 形状由 Web 严格存档
    /// parser 共同维护，避免把策略私有结构扩成通用宿主命令。
    #[ts(type = "import(\"../../save/schema/runtime-v2\").SaveRuntimeV2")]
    pub runtime_v2: SaveRuntimeV2,
    pub setup: SessionSetup,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub seed: u64,
    pub snapshot: Snapshot,
    /// 日内存档恢复集合竞价所需的完整委托队列。
    pub auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    /// 连续竞价未成交委托。
    pub resting_orders: BTreeMap<StockCode, Vec<Order>>,
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
    pub parent_orders: BTreeMap<AccountId, BTreeMap<StockCode, ParentOrderPlan>>,
    /// NPC 连续竞价普通限价单的可恢复主动撤单时间。
    pub npc_order_lifecycles: Vec<NpcOrderLifecycle>,
    /// 已被宿主确认入队、尚未在下一 tick 路由的玩家意图。
    pub pending_player: Vec<(AccountId, Intent)>,
    /// 上一已提交版本生成、等待下一市场 tick 受理的 NPC 请求。
    pub pending_npc: Option<PendingNpcBatch>,
    /// 保持订单 id/到达序继续单调递增。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub next_order_id: u64,
    /// K1 自然日经营时钟权威状态（任务 27 起随档携带冻结日历政策）。
    pub civil_clock: CivilClockSave,
    /// ── K7（任务 27）：公司域与个体决策链权威状态。全部必填；缺失任一字段
    ///    的 JSON 不是当前 schema 的合法存档，走通用校验拒绝。──
    /// 经营编排（调度器/活跃冲击/各经营 RNG/账套——serde 全量持久化，分录与
    /// 余额在反序列化重放边界校验）。
    #[ts(skip)]
    pub company_operations: crate::company::operations::CompanyOperations,
    /// 结账版本登记簿（不可变期间版本 + 重述底稿）。
    #[ts(skip)]
    pub closing_registry: crate::accounting::closing::ClosingEngine,
    /// 公开信息库（报告 + 公告；恢复走 from_parts 逐条重验）。
    #[ts(skip)]
    pub public_library: crate::information::PublicLibrary,
    /// 经营 ↔ 时钟到期镜像（已镜像调度事件 id 集合）。
    #[ts(skip)]
    pub ops_wiring: CompanyOperationsClockWiring,
    /// 披露派发游标（published_through / announced_through）。
    #[ts(skip)]
    pub disclosures: DisclosureDispatch,
    /// 跨日个人交易计划簿（(账户,股票) 索引恢复时重建并校验）。
    pub plans: crate::plans::PlanBook,
    /// 信念机构账户的个人信息集（只存公布 id 引用与获知时点）。
    #[ts(skip)]
    pub information_states: BTreeMap<AccountId, crate::information::NpcInformationState>,
    /// 信念机构账户的信念簿（含一次性抽定的个人假设）。
    #[ts(skip)]
    pub belief_books: BTreeMap<AccountId, crate::strategy::BeliefBook>,
    /// 信念机构账户的个人关注列表。
    pub watchlists: BTreeMap<AccountId, crate::experience::PersonalWatchlist>,
    pub price_memories: BTreeMap<AccountId, crate::experience::PersonalPriceMemory>,
    /// 计划执行待应用事实队列（存档边界只保留「计划簿中仍存活」的条目；
    /// 未知/已终止计划的迟到条目按存档契约丢弃——issues.md 任务 27 §3）。
    #[ts(skip)]
    pub pending_plan_events: Vec<plan_execution::PendingPlanEvent>,
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
    pub intents: Vec<(AccountId, Intent)>,
    /// Reconciliation cancellation -> replacement admission, by intent position.
    pub dependencies: Vec<(usize, usize)>,
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
            let (before_owner, before_intent) = &self.intents[before];
            let (after_owner, after_intent) = &self.intents[after];
            let Intent::Cancel {
                code: before_code, ..
            } = before_intent
            else {
                return Err(format!("{prefix} predecessor is not a cancellation"));
            };
            let after_code = match after_intent {
                Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } => code,
                Intent::Cancel { .. } => {
                    return Err(format!("{prefix} successor is not a placement"));
                }
            };
            if before_owner != after_owner || before_code != after_code {
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
    /// 透传交易日历错误（K1 开局日期门等）。
    #[error(transparent)]
    Calendar(#[from] crate::calendar::CalendarError),
    /// 透传自然日时钟错误（K1 日结验证/原子失败）。
    #[error(transparent)]
    CivilClock(#[from] CivilClockError),
    /// 透传公司经营接线错误（任务 26 日终编排）。
    #[error(transparent)]
    CompanyOperations(#[from] company_operations::CompanyOperationsSeamError),
    /// 透传披露派发错误（任务 26 日终编排）。
    #[error(transparent)]
    Disclosure(#[from] DisclosureError),
    /// 透传结账错误（任务 26 月/年末封账）。
    #[error("accounting closing failed: {0}")]
    Closing(#[source] crate::accounting::closing::ClosingError),
    /// 透传个体分析档案派生错误（任务 26 信念机构装配）。
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

/// 流通盘分配方式。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum FloatAllocation {
    /// 默认：所有 NPC 随机分配（权重随机，筹码守恒）。
    Random,
    /// 三类各占比例（类内随机）。比例应 ≈ 1（运行时按「有 NPC 的种类」归一化）。
    ByKind { retail: f64, inst: f64, hot: f64 },
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
    /// K1 开局自然日。缺省为政策默认 2030-01-01；合法开局 2000-01-01..2099-12-31
    /// （1998–1999 仅供初始化前史查询）。休市起点保持原日，不挪到开市日。
    /// serde 缺省仅供宿主过渡期不发送该字段时使用；存档总是显式写出。
    #[serde(default = "default_civil_start_date")]
    pub start_date: crate::calendar::CivilDate,
    /// 模拟政策身份（K7 行 174）：本引擎当前行为契约（A 股交易语义、公司域、
    /// 决策链参数族）的稳定版本标识。新档必填；与存档一起固化，恢复时不
    /// 与任何“最新默认”比对或迁移——身份不匹配的档由宿主层拒绝。
    pub simulation_policy_id: String,
}

impl SessionSetup {
    /// 校验所有启动期外部输入。serde 可绕过各子类型构造器，因此 session 创建和恢复
    /// 都必须从这里进入，验证成功后才构造权威状态。
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.stocks.is_empty() {
            return Err(SessionError::InvalidSetup(
                "stocks must be non-empty".to_string(),
            ));
        }
        if self.simulation_policy_id != SIMULATION_POLICY_ID_V2 {
            return Err(SessionError::InvalidSetup(format!(
                "simulation_policy_id must be the current policy {SIMULATION_POLICY_ID_V2:?}, got {:?}",
                self.simulation_policy_id
            )));
        }
        // K1 开局日期门：运行区间 2000-01-01..2099-12-31；1998–1999 仅供前史。
        TradingCalendar::default_v1()?.validate_runtime_start(self.start_date)?;
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
        if let FloatAllocation::ByKind { retail, inst, hot } = &self.float_allocation {
            for (value, name) in [(*retail, "retail"), (*inst, "inst"), (*hot, "hot")] {
                if !value.is_finite() || value < 0.0 {
                    return Err(SessionError::InvalidSetup(format!(
                        "float_allocation ByKind {name}={value} invalid (must be finite >=0)"
                    )));
                }
            }
            if self.stocks.iter().any(|stock| stock.float_shares > 0) {
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
                        "float_allocation ByKind weights for existing NPC kinds must have a finite sum > 0"
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
    poison: Option<StepFatal>,
    #[cfg(test)]
    injected_failure: Option<StepFatal>,
    #[cfg(test)]
    post_shadow_failure: Option<StepFatal>,
    setup: SessionSetup,
    rng: SplitMix64,
    seed: u64,
    markets: BTreeMap<StockCode, Market>,
    accounts: AccountBook,
    price_history: BTreeMap<StockCode, VecDeque<Money>>,
    market_minute_closes: BTreeMap<StockCode, Vec<MarketMinuteClose>>,
    daily_candles: BTreeMap<StockCode, DailyCandleHistory>,
    active_daily_candles: BTreeMap<StockCode, DailyCandle>,
    auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    pending_player: Vec<(AccountId, Intent)>,
    pending_npc: Option<PendingNpcBatch>,
    npc_attention: AccountPagedMap<NpcAttentionState>,
    retail_experience: AccountPagedMap<RetailExperienceState>,
    parent_orders: BTreeMap<AccountId, BTreeMap<StockCode, ParentOrderPlan>>,
    pending_plan_events: Vec<plan_execution::PendingPlanEvent>,
    npc_order_lifecycles: Vec<NpcOrderLifecycle>,
    /// 上一 tick 中被实际观察并执行 B02/B03 判断的散户目标仓位样本。
    /// 这是诊断缓存，不进入存档、不会被策略读取，也不属于权威游戏状态。
    last_retail_decisions: Vec<RetailDecisionTrace>,
    last_retail_order_events: Vec<RetailOrderDiagnosticEvent>,
    #[cfg(feature = "simulation-diagnostics")]
    npc_decision_traces: crate::diagnostics::decision_trace::NpcDecisionTraceCollector,
    #[cfg(feature = "simulation-diagnostics")]
    causal: crate::diagnostics::causal::CausalCollector,
    attention_queue: BinaryHeap<Reverse<(u64, AccountId)>>,
    // ── 公司域 + 决策链状态（任务 26；持久化契约归任务 27，当前为会话期状态：
    //    恢复时前史确定性重建 + 经营按自然日重放，个人信念/计划/信息集复位，
    //    已在 issues.md 登记）──
    /// 发行人注册表（任务 7）：股票 ↔ 公司映射与开局账套。
    company_registry: std::sync::Arc<crate::company::CompanyRegistry>,
    /// 自然日经营编排（任务 14；前史已推进到开局日）。
    operations: std::sync::Arc<crate::company::operations::CompanyOperations>,
    /// 结账版本登记簿（任务 13）。
    closing: crate::accounting::closing::ClosingEngine,
    /// 公开信息库（任务 15；前史已播种）。
    library: crate::information::PublicLibrary,
    /// 经营 ↔ 时钟到期镜像。
    ops_wiring: CompanyOperationsClockWiring,
    /// 披露派发游标。
    disclosures: DisclosureDispatch,
    /// 跨日个人交易计划（K6；PlanBook 本身支持全账户）。
    plans: crate::plans::PlanBook,
    /// 信念机构账户的个人信息集（K4 任务 16）。
    information: BTreeMap<AccountId, crate::information::NpcInformationState>,
    /// 信念机构账户的信念簿（K5 任务 18）。
    belief_books: AccountPagedMap<crate::strategy::BeliefBook>,
    /// 信念机构账户的关注列表（任务 25）。
    watchlists: AccountPagedMap<crate::experience::PersonalWatchlist>,
    price_memories: AccountPagedMap<crate::experience::PersonalPriceMemory>,
    /// Empty until the later P3-P6 receipt migration creates live envelopes.
    envelope_ledger: pipeline::EnvelopeLedger,
    /// Receipts already consumed by the atomic P6 account/experience projection.
    /// This is authoritative replay protection and must travel with the tick shadow.
    retail_projection_seen: pipeline::RetailProjectionSeen,
    /// The next globally allocated receipt index; currently hydrated as zero because the
    /// compatibility bridge has not yet produced persisted receipts.
    next_receipt_base: u64,
    next_order_id: u64,
    tick: u64,
    day: u32,
    seq: u64,
    /// K1 自然日经营时钟：与 tick/交易日计数分离的权威自然日推进。
    civil_clock: CivilClock,
}

/// 一个真实进入散户策略判断路径的目标仓位样本。
///
/// 仅用于离线联合验收；它保留判断输入产生的目标，不把“目标”误记成委托或成交。
#[derive(Clone, Debug, PartialEq)]
pub struct RetailDecisionTrace {
    pub account: AccountId,
    pub decision: PositionDecision,
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
    filled_value_before: Money,
    gross: Money,
    qty: u32,
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
        return Err(SessionError::InvalidSetup(format!(
            "initial cash overflow for {kind:?} account {}",
            account.0
        )));
    }
    Ok(Money::from_cents(cents.round() as i64))
}

pub(crate) fn fee_delta(
    before: Money,
    after: Money,
    calculate: impl Fn(Money) -> Result<Money, MoneyError>,
) -> Result<Money, MoneyError> {
    let before_fee = if before == Money::ZERO {
        Money::ZERO
    } else {
        calculate(before)?
    };
    calculate(after)?.sub(before_fee)
}

pub(crate) fn buy_order_reservation(
    config: &GameConfig,
    limit: Money,
    qty: u32,
    filled_value: Money,
) -> Result<Money, MoneyError> {
    let remaining_gross = limit.mul_shares(qty)?;
    let final_gross = filled_value.add(remaining_gross)?;
    let commission = fee_delta(filled_value, final_gross, |amount| {
        config.commission(amount)
    })?;
    let transfer_fee = fee_delta(filled_value, final_gross, |amount| {
        config.transfer_fee(amount)
    })?;
    remaining_gross.add(commission)?.add(transfer_fee)
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
    /// Frozen input for `run_chain_for_account`. A plan root reads account and
    /// issuer facts, current market prices, the public library and the live plan
    /// index. Order queues, histories and mutable personal state belong to the
    /// tick candidate and must not be copied into each root wave.
    pub(super) fn clone_for_plan_roots(&self) -> Self {
        Self {
            poison: None,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
            setup: self.setup.clone(),
            rng: self.rng.clone(),
            seed: self.seed,
            markets: BTreeMap::new(),
            accounts: self.accounts.clone(),
            price_history: BTreeMap::new(),
            market_minute_closes: BTreeMap::new(),
            daily_candles: BTreeMap::new(),
            active_daily_candles: BTreeMap::new(),
            auction_orders: BTreeMap::new(),
            pending_player: Vec::new(),
            pending_npc: None,
            npc_attention: AccountPagedMap::default(),
            retail_experience: AccountPagedMap::default(),
            parent_orders: BTreeMap::new(),
            pending_plan_events: Vec::new(),
            npc_order_lifecycles: Vec::new(),
            last_retail_decisions: Vec::new(),
            last_retail_order_events: Vec::new(),
            #[cfg(feature = "simulation-diagnostics")]
            npc_decision_traces: Default::default(),
            #[cfg(feature = "simulation-diagnostics")]
            causal: Default::default(),
            attention_queue: BinaryHeap::new(),
            company_registry: self.company_registry.clone(),
            operations: self.operations.clone(),
            closing: self.closing.clone(),
            library: self.library.clone(),
            ops_wiring: self.ops_wiring.clone(),
            disclosures: self.disclosures.clone(),
            plans: self.plans.clone(),
            information: BTreeMap::new(),
            belief_books: AccountPagedMap::default(),
            watchlists: AccountPagedMap::default(),
            price_memories: AccountPagedMap::default(),
            envelope_ledger: pipeline::EnvelopeLedger::new(self.next_receipt_base, [])
                .expect("empty plan observation ledger is valid"),
            retail_projection_seen: Default::default(),
            next_receipt_base: self.next_receipt_base,
            next_order_id: self.next_order_id,
            tick: self.tick,
            day: self.day,
            seq: self.seq,
            civil_clock: self.civil_clock.clone(),
        }
    }

    pub(super) fn clone_for_tick_shadow(&self) -> Result<Self, StepFatal> {
        let Self {
            poison: _,
            #[cfg(test)]
                injected_failure: _,
            #[cfg(test)]
                post_shadow_failure: _,
            setup,
            rng,
            seed,
            markets,
            accounts,
            price_history,
            market_minute_closes,
            daily_candles,
            active_daily_candles,
            auction_orders,
            pending_player,
            pending_npc,
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
            attention_queue,
            company_registry,
            operations,
            closing,
            library,
            ops_wiring,
            disclosures,
            plans,
            information,
            belief_books,
            watchlists,
            price_memories,
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
            poison: None,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
            setup: setup.clone(),
            rng: rng.clone(),
            seed: *seed,
            markets: markets.clone(),
            accounts,
            price_history: price_history.clone(),
            market_minute_closes: market_minute_closes.clone(),
            daily_candles: daily_candles.clone(),
            active_daily_candles: active_daily_candles.clone(),
            auction_orders: auction_orders.clone(),
            pending_player: pending_player.clone(),
            pending_npc: pending_npc.clone(),
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
            attention_queue: attention_queue.clone(),
            company_registry: company_registry.clone(),
            operations: operations.clone(),
            closing: closing.clone(),
            library: library.clone(),
            ops_wiring: ops_wiring.clone(),
            disclosures: disclosures.clone(),
            plans: plans.clone(),
            information: information.clone(),
            belief_books: belief_books.clone(),
            watchlists: watchlists.clone(),
            price_memories: price_memories.clone(),
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

    pub(super) fn commit_tick_shadow(&mut self, shadow: Self) {
        let Self {
            poison: _,
            #[cfg(test)]
                injected_failure: _,
            #[cfg(test)]
                post_shadow_failure: _,
            setup,
            rng,
            seed,
            markets,
            accounts,
            price_history,
            market_minute_closes,
            daily_candles,
            active_daily_candles,
            auction_orders,
            pending_player,
            pending_npc,
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
            attention_queue,
            company_registry,
            operations,
            closing,
            library,
            ops_wiring,
            disclosures,
            plans,
            information,
            belief_books,
            watchlists,
            price_memories,
            envelope_ledger,
            retail_projection_seen,
            next_receipt_base,
            next_order_id,
            tick,
            day,
            seq,
            civil_clock,
        } = shadow;
        self.setup = setup;
        self.rng = rng;
        self.seed = seed;
        self.markets = markets;
        self.accounts.replace_and_drop_parallel(accounts);
        self.price_history = price_history;
        self.market_minute_closes = market_minute_closes;
        self.daily_candles = daily_candles;
        self.active_daily_candles = active_daily_candles;
        self.auction_orders = auction_orders;
        self.pending_player = pending_player;
        self.pending_npc = pending_npc;
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
        self.attention_queue = attention_queue;
        self.company_registry = company_registry;
        self.operations = operations;
        self.closing = closing;
        self.library = library;
        self.ops_wiring = ops_wiring;
        self.disclosures = disclosures;
        self.plans = plans;
        self.information = information;
        self.belief_books = belief_books;
        self.watchlists = watchlists;
        self.price_memories = price_memories;
        self.envelope_ledger = envelope_ledger;
        self.retail_projection_seen = retail_projection_seen;
        self.next_receipt_base = next_receipt_base;
        self.next_order_id = next_order_id;
        self.tick = tick;
        self.day = day;
        self.seq = seq;
        self.civil_clock = civil_clock;
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
        let daily_candles = generate_preset_daily_candles(&setup, seed);
        let mut accounts = AccountBook::default();
        accounts.insert(
            AccountId(0),
            Account::new(
                AccountId(0),
                AccountKind::Player,
                setup.config.starting_cash,
            ),
        );
        let rng = SplitMix64::new(seed);
        let civil_clock = CivilClock::new(
            setup.start_date,
            session_calendar_exchange(setup.stocks[0].exchange),
        )?;
        // 任务 26 新局装配：公司注册表 + 前史经营 + 公开库 + 时钟/披露接线。
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
        let mut civil_clock = civil_clock;
        let mut ops_wiring = CompanyOperationsClockWiring::new();
        ops_wiring
            .install(&mut civil_clock, &ops)
            .map_err(SessionError::CompanyOperations)?;
        let disclosures = DisclosureDispatch::new(seeded_through);
        disclosures.install(&mut civil_clock);
        let mut sess = GameSession {
            poison: None,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            post_shadow_failure: None,
            #[cfg(feature = "simulation-diagnostics")]
            causal: crate::diagnostics::causal::CausalCollector::default(),
            setup,
            rng,
            seed,
            markets,
            accounts,
            price_history,
            market_minute_closes,
            daily_candles,
            active_daily_candles: BTreeMap::new(),
            auction_orders: BTreeMap::new(),
            pending_player: Vec::new(),
            pending_npc: None,
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
            attention_queue: BinaryHeap::new(),
            company_registry: std::sync::Arc::new(registry),
            operations: std::sync::Arc::new(ops),
            closing,
            library,
            ops_wiring,
            disclosures,
            plans: crate::plans::PlanBook::default(),
            information: BTreeMap::new(),
            belief_books: AccountPagedMap::default(),
            watchlists: AccountPagedMap::default(),
            price_memories: AccountPagedMap::default(),
            envelope_ledger: pipeline::EnvelopeLedger::new(0, [])
                .expect("an empty envelope ledger is valid"),
            retail_projection_seen: pipeline::RetailProjectionSeen::default(),
            next_receipt_base: 0,
            next_order_id: 1,
            tick: 0,
            day: 0,
            seq: 0,
            civil_clock,
        };
        sess.populate_npcs(AccountKind::Retail)?;
        sess.populate_npcs(AccountKind::Inst)?;
        sess.populate_npcs(AccountKind::Hot)?;
        sess.seed_float()?; // 分配流通盘给 NPC（筹码守恒、确定性、玩家不分配）
        sess.initialize_retail_experience()?;
        pipeline::queue_npc_for_next_tick(&mut sess)?;
        Ok(sess)
    }

    fn account_equity(&self, id: AccountId) -> Result<Money, MoneyError> {
        let account = self
            .accounts
            .get(&id)
            .expect("equity may only be computed for an existing account");
        account
            .positions
            .iter()
            .try_fold(account.cash, |total, (code, position)| {
                let price = self
                    .markets
                    .get(code)
                    .unwrap_or_else(|| panic!("account {} holds unknown stock {}", id.0, code.0))
                    .last_price();
                total.add(price.mul_shares(position.qty)?)
            })
    }

    fn current_market_minute(&self) -> u64 {
        let day_start = u64::from(self.day)
            .checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .expect("u32 session day times 240 fits u64");
        let day_tick = self.tick % self.setup.ticks_per_day;
        let continuous_ticks_per_day = self
            .setup
            .ticks_per_day
            .saturating_sub(self.setup.auction_ticks)
            .saturating_sub(self.setup.closing_auction_ticks);
        let completed = completed_market_minute_count(
            day_tick
                .saturating_sub(self.setup.auction_ticks)
                .min(continuous_ticks_per_day),
            continuous_ticks_per_day,
        )
        .expect("validated session timing maps to market minutes");
        day_start + u64::from(completed)
    }

    fn initialize_retail_experience(&mut self) -> Result<(), SessionError> {
        let market_minute = self.current_market_minute();
        let retail_ids: Vec<_> = self
            .accounts
            .iter()
            .filter_map(|(id, account)| (account.kind == AccountKind::Retail).then_some(*id))
            .collect();
        for id in retail_ids {
            let equity = self.account_equity(id)?;
            let mut experience = if equity.cents() > 0 {
                RetailExperienceState::new(equity)?
            } else {
                RetailExperienceState::without_equity_reference()
            };
            let holdings: Vec<_> = self.accounts[&id]
                .positions
                .iter()
                .map(|(code, position)| {
                    let current = self.markets[code].last_price();
                    (
                        code.clone(),
                        position.cost_price().filter(|price| price.cents() > 0),
                        current,
                    )
                })
                .collect();
            for (code, reference, current) in holdings {
                experience.initialize_holding(&code, reference, current, market_minute)?;
            }
            self.retail_experience.insert(id, experience);
        }
        Ok(())
    }

    #[cfg(test)]
    fn observe_retail_experience(&mut self, ids: &[AccountId]) -> Result<(), ExperienceError> {
        let market_minute = self.current_market_minute();
        let observations: Vec<_> = ids
            .iter()
            .filter(|id| self.retail_experience.contains_key(id))
            .map(|id| {
                let equity = self
                    .account_equity(*id)
                    .expect("validated account equity must remain representable");
                let positions: Vec<_> = self.accounts[id]
                    .positions
                    .keys()
                    .map(|code| (code.clone(), self.markets[code].last_price()))
                    .collect();
                (*id, equity, positions)
            })
            .collect();
        for (id, equity, positions) in observations {
            let experience = self
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
    /// [`FloatAllocation::Random`]→全 NPC 随机；[`FloatAllocation::ByKind`]→按种类比例（类内随机、
    /// 缺类自动归一化分摊）。
    fn seed_float(&mut self) -> Result<(), SessionError> {
        let npc_ids: Vec<AccountId> = self
            .accounts
            .keys()
            .copied()
            .filter(|id| id.0 != 0)
            .collect();
        if npc_ids.is_empty() {
            return Ok(());
        }
        for spec in self.setup.stocks.clone() {
            if spec.float_shares == 0 {
                continue;
            }
            let code = spec.code.clone();
            let price = spec.initial_price;
            let alloc: Vec<(AccountId, u32)> = match &self.setup.float_allocation {
                FloatAllocation::Random => self.split_random(spec.float_shares, &npc_ids),
                FloatAllocation::ByKind { retail, inst, hot } => {
                    self.split_by_kind(spec.float_shares, *retail, *inst, *hot)
                }
            };
            for (id, qty) in alloc {
                if qty > 0 {
                    if let Some(acc) = self.accounts.get_mut(&id) {
                        acc.grant_position(code.clone(), qty, price)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 按种类比例分：归一化到「有 NPC 的种类」→ 类内 [`Self::split_random`]。Σ==float。
    ///
    /// 缺类（该种类无 NPC）自动剔除并重归一化（其比例分摊给剩余种类）。最后一类拿整体余量
    /// → 全局精确守恒。f64 仅用于「比例归一」（股数分配，非金额）；最终量 u32。
    fn split_by_kind(
        &mut self,
        float: u32,
        r_retail: f64,
        r_inst: f64,
        r_hot: f64,
    ) -> Vec<(AccountId, u32)> {
        // 收集每类 NPC（按 id 升序，BTreeMap keys 天然有序 → 确定性）。固定 [3] 数组免动态分配。
        let mut by_kind: [(AccountKind, f64, Vec<AccountId>); 3] = [
            (AccountKind::Retail, r_retail, Vec::new()),
            (AccountKind::Inst, r_inst, Vec::new()),
            (AccountKind::Hot, r_hot, Vec::new()),
        ];
        for id in self.accounts.keys().copied().filter(|id| id.0 != 0) {
            let kind = self
                .accounts
                .get(&id)
                .map(|a| a.kind)
                .unwrap_or(AccountKind::Retail);
            for entry in by_kind.iter_mut() {
                if entry.0 == kind {
                    entry.2.push(id);
                    break;
                }
            }
        }
        // 归一化：只算有 NPC 的种类（缺类不计入 norm → 其比例自动分摊给剩余种类）。
        let norm: f64 = by_kind
            .iter()
            .filter(|entry| !entry.2.is_empty())
            .map(|entry| entry.1)
            .sum();
        let nonempty: Vec<usize> = by_kind
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.2.is_empty())
            .map(|(i, _)| i)
            .collect();
        let mut out: Vec<(AccountId, u32)> = Vec::new();
        let mut remaining = float;
        for (idx, &i) in nonempty.iter().enumerate() {
            let (kind, ratio, ids) = &by_kind[i];
            let kind_float = if idx == nonempty.len() - 1 {
                remaining // 最后一类拿整体余量 → 全局守恒
            } else if norm > 0.0 {
                ((float as f64 * *ratio / norm).round() as u32).min(remaining)
            } else {
                0
            };
            let eligible_ids: Vec<AccountId> = if *kind == AccountKind::Retail && ids.len() > 1 {
                let mut selected: Vec<AccountId> = ids
                    .iter()
                    .copied()
                    .filter(|_| self.rng.next_f64() < 0.40)
                    .collect();
                if selected.is_empty() {
                    selected.push(ids[self.rng.next_range_u32(0, ids.len() as u32) as usize]);
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
            let parts = self.split_random_with_tail(kind_float, &eligible_ids, tail_exponent);
            for (id, q) in parts {
                out.push((id, q));
                remaining = remaining.saturating_sub(q);
            }
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
                (1.0 - self.rng.next_f64())
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
            AccountKind::Retail => self.setup.npcs.retail_count,
            AccountKind::Inst => self.setup.npcs.inst_count,
            AccountKind::Hot => self.setup.npcs.hot_count,
            AccountKind::Player => 0,
        };
        let first_id = self.accounts.keys().next_back().map_or(1, |id| id.0 + 1);
        let end_id = first_id.checked_add(u64::from(count)).ok_or_else(|| {
            SessionError::InvalidSetup("NPC account id range overflow".to_string())
        })?;
        for (ordinal, next_id) in (first_id..end_id).enumerate() {
            let id = AccountId(next_id);
            let initial_cash =
                sample_npc_cash(kind, self.setup.npcs.retail_cash_median, self.seed, id)?;
            let mut acc = Account::new(id, kind, initial_cash);
            if let Some(s) = StrategyFactory::build_for_market_day_with_ordinal(
                kind,
                &self.setup.strategy_params,
                self.setup.ticks_per_day,
                u32::try_from(ordinal).map_err(|_| {
                    SessionError::InvalidSetup("NPC ordinal exceeds u32".to_string())
                })?,
                &mut self.rng,
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
                // 全部使用 seed ^ FNV1a(账户派生标签) 的独立流，绝不用 self.rng。
                let profile = acc.strategy.as_ref().expect("just set").profile();
                if kind == AccountKind::Inst
                    && matches!(
                        profile,
                        StrategyProfile::Institution(
                            crate::strategy::InstitutionStyle::DeepValue
                                | crate::strategy::InstitutionStyle::Growth
                                | crate::strategy::InstitutionStyle::Balanced
                                | crate::strategy::InstitutionStyle::Defensive
                                | crate::strategy::InstitutionStyle::ActiveTrader
                        )
                    )
                {
                    let mut analysis_rng = SplitMix64::new(decision_chain::derived_stream(
                        self.seed,
                        "analysis-profile",
                        id,
                    ));
                    let analysis =
                        crate::strategy::derive_analysis_profile(&profile, id, &mut analysis_rng)
                            .map_err(SessionError::StrategyAnalysis)?;
                    let mut belief_rng = SplitMix64::new(decision_chain::derived_stream(
                        self.seed,
                        "belief-assumptions",
                        id,
                    ));
                    let belief =
                        crate::strategy::BeliefBook::new(id, profile, analysis, &mut belief_rng);
                    self.belief_books.insert(id, belief);
                    self.information
                        .insert(id, crate::information::NpcInformationState::new(id));
                    self.watchlists
                        .insert(id, crate::experience::PersonalWatchlist::new());
                    self.price_memories
                        .insert(id, crate::experience::PersonalPriceMemory::default());
                }
                let attention_seed =
                    self.seed ^ next_id.wrapping_mul(0x6A09_E667_F3BC_C908) ^ 0xA77E_7710_D15C_A11E;
                let mut attention_rng = SplitMix64::new(attention_seed);
                let first_candidate_tick = sample_attention_wait(
                    maximum_observation_probability(kind, base_probability),
                    &mut attention_rng,
                ) - 1;
                self.npc_attention.insert(
                    id,
                    NpcAttentionState {
                        base_probability,
                        next_attention_candidate_tick: first_candidate_tick,
                        rng_state: attention_rng.state,
                    },
                );
                self.attention_queue
                    .push(Reverse((first_candidate_tick, id)));
            }
            self.accounts.insert(id, acc);
        }
        Ok(())
    }

    /// 股票数量。
    pub fn market_count(&self) -> usize {
        self.markets.len()
    }
    /// 账户数量（含玩家）。
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }
    /// 当前每个 NPC 的策略身份档案。诊断可用它归因成交参与度；玩家没有策略，故不在结果中。
    ///
    /// 档案只标识公开的策略种类/风格，不泄露账户现金、库存、成本或策略私有参数。
    pub fn account_strategy_profiles(&self) -> BTreeMap<AccountId, StrategyProfile> {
        self.accounts
            .iter()
            .filter_map(|(id, account)| {
                account
                    .strategy
                    .as_ref()
                    .map(|strategy| (*id, strategy.profile()))
            })
            .collect()
    }
    /// 只读账户引用。
    pub fn account(&self, id: AccountId) -> Option<&Account> {
        self.accounts.get(&id)
    }
    /// 当前 tick（从 0 起，step 后自增）。
    pub fn tick(&self) -> u64 {
        self.tick
    }
    /// 当前交易日（0 起，日界自增）。
    pub fn day(&self) -> u32 {
        self.day
    }

    /// 当前交易阶段。`auction_ticks` 表示 09:15–09:30 的整段开盘时间；
    /// 前 2/3 接受集合竞价申报，后 1/3 为不接受申报的盘前窗口。
    pub fn phase(&self) -> TradingPhase {
        let day_tick = self.tick % self.setup.ticks_per_day;
        if day_tick < self.auction_entry_ticks() {
            TradingPhase::CallAuction
        } else if day_tick < self.setup.auction_ticks {
            TradingPhase::PreOpen
        } else if day_tick >= self.closing_auction_start_tick() {
            TradingPhase::ClosingAuction
        } else {
            TradingPhase::Continuous
        }
    }

    /// 把可配置的 15 分钟开盘窗口按 10:5 映射为申报期和盘前期。
    fn auction_entry_ticks(&self) -> u64 {
        self.setup.auction_ticks - self.setup.auction_ticks / 3
    }

    fn closing_auction_start_tick(&self) -> u64 {
        self.setup.ticks_per_day - self.setup.closing_auction_ticks
    }
    /// 最新事件 seq。
    pub fn seq(&self) -> u64 {
        self.seq
    }
    /// 自增并返回下一个事件 seq（单调）。
    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// 当前自然日（K1：与交易日计数 `day` 分离；休市日推进只动这里）。
    pub fn civil_date(&self) -> crate::calendar::CivilDate {
        self.civil_clock.current_date()
    }

    /// Returns one host-safe page of reports visible at the current civil instant.
    pub fn query_public_reports(
        &self,
        query: &crate::company::PublicReportQuery,
    ) -> Result<crate::company::PublicReportPage, SessionError> {
        let as_of = crate::calendar::CivilInstant::new(self.civil_date(), 0)
            .map_err(crate::calendar::CalendarError::from)?;
        self.library
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
        self.library
            .public_report_by_id(id, as_of)
            .map_err(|error| SessionError::Information(Box::new(error)))
    }

    /// 自然日经营时钟只读访问（诊断/测试）。
    pub fn civil_clock(&self) -> &CivilClock {
        &self.civil_clock
    }

    /// 自然日经营时钟可变访问：注册到期业务、显式日期日结等高级用法。
    /// **绕过会话同步守卫**；宿主常规循环必须走 [`Self::end_civil_day`]。
    pub fn civil_clock_mut(&mut self) -> &mut CivilClock {
        &mut self.civil_clock
    }

    /// 自然日日结（K1）：当日经营终局窗口 →（月/年末封账）→ 18:00 披露 → 前进次日。
    ///
    /// 交易日必须先完成当日会话（`ticks_per_day` 个 step，`day` 已自增到位）；
    /// 休市日直接调用。先全量验证（会话同步 + 时钟规则）后原子应用，任何
    /// `Err` 不改变会话与时钟状态。step 的 tick 循环完全不变——日结是新接在
    /// 收盘之后的自然日权威推进，不是 tick 循环的一部分。
    ///
    /// 任务 26 接线（K4 顺序）：时钟日结（到期派发 + 18:00 相位）→ 经营终局
    /// （`CompanyOperationsClockWiring::run_day_end` 推进当日经营并增量再同步）
    /// → 月/年末封账（`close_accounting_periods`）→ 披露派发
    /// （`DisclosureDispatch::run_day_end`，公告先于定期报告）。
    pub fn end_civil_day(&mut self) -> Result<CivilDayEndReport, SessionError> {
        self.require_healthy()?;
        let expected_sessions = self.civil_clock.completed_trading_sessions_expected()?;
        if self.day != expected_sessions {
            return Err(SessionError::CivilClock(
                CivilClockError::MarketSessionOutOfSync {
                    date: self.civil_clock.current_date(),
                    completed_sessions: self.day,
                    expected_sessions,
                },
            ));
        }
        let observers = self.civil_clock.disclosure_observers().to_vec();
        let before = self.save()?;
        match self.end_civil_day_after_session_check() {
            Ok(report) => Ok(report),
            Err(error) => {
                let mut restored = Self::restore(&before).map_err(|rollback| {
                    SessionError::InvalidSave(format!(
                        "civil day-end rollback failed after {error}: {rollback}"
                    ))
                })?;
                restored.civil_clock.replace_disclosure_observers(observers);
                *self = restored;
                Err(error)
            }
        }
    }

    fn end_civil_day_after_session_check(&mut self) -> Result<CivilDayEndReport, SessionError> {
        let mut report = self.civil_clock.end_day(self.civil_clock.current_date())?;
        self.ops_wiring.run_day_end(
            &report,
            &mut self.civil_clock,
            std::sync::Arc::make_mut(&mut self.operations),
        )?;
        self.close_accounting_periods(report.settled_date)?;
        let disclosures = self
            .disclosures
            .run_day_end(DayEndDisclosureCtx {
                report: &report,
                ops: &self.operations,
                closing: &mut self.closing,
                library: &mut self.library,
            })
            .map_err(SessionError::Disclosure)?;
        self.ops_wiring.prune_dispatched(&self.operations);
        self.record_civil_day_events(&mut report, disclosures)?;
        for observer in self.civil_clock.disclosure_observers() {
            observer(report.disclosure_instant);
        }
        Ok(report)
    }

    fn record_civil_day_events(
        &mut self,
        report: &mut CivilDayEndReport,
        disclosures: DayEndDisclosures,
    ) -> Result<(), SessionError> {
        for publication_id in disclosures.announcements_published {
            let (company, published_at) = self
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
            let (company, published_at, report_revision) = self
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
                seq: self.next_seq(),
                publication_id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Report { report_revision },
            });
        }
        report.events.push(Event::CivilDateAdvanced {
            seq: self.next_seq(),
            settled_date: report.settled_date,
            next_date: report.next_date,
            next_status: report.next_status.clone(),
        });
        Ok(())
    }

    /// 月/年末封账（K4 日终顺序的第二步）：settled 是当月最后一天时对该月
    /// 封月；12 月末走 `close_year`（其内部含 12 月封月 + 年报版本）。封账后
    /// 该期间拒绝后续入账（底座守卫），晚于封账日的分录天然落在开放期间。
    fn close_accounting_periods(
        &mut self,
        settled: crate::calendar::CivilDate,
    ) -> Result<(), SessionError> {
        let is_month_end = settled
            .next()
            .map(|next| next.month() != settled.month())
            .unwrap_or(false);
        if !is_month_end {
            return Ok(());
        }
        let year_end = settled.month() == 12;
        let period = crate::accounting::AccountingPeriod::from_ymd(settled.year(), settled.month())
            .map_err(|error| {
                SessionError::InvalidSetup(format!("closing period invalid: {error}"))
            })?;
        let targets: Vec<(
            crate::company::CompanyId,
            crate::accounting::consolidation::MemberId,
            crate::accounting::reports::IndustryPresentation,
        )> = self
            .operations
            .companies
            .iter()
            .map(|(id, company)| {
                (
                    id.clone(),
                    crate::accounting::consolidation::MemberId(id.0.clone()),
                    crate::information::industry_presentation(company.spec().kind),
                )
            })
            .collect();
        for (company_id, member, industry) in targets {
            let Some(company) =
                std::sync::Arc::make_mut(&mut self.operations).company_mut(&company_id)
            else {
                continue;
            };
            let books = company.books_mut();
            let result = if year_end {
                self.closing
                    .close_year(books, &member, industry, settled.year())
                    .map(|_| ())
            } else {
                self.closing
                    .close_month(books, &member, industry, period)
                    .map(|_| ())
            };
            result.map_err(SessionError::Closing)?;
        }
        Ok(())
    }

    /// 读取上一 tick 的散户目标仓位诊断样本。
    ///
    /// 该切片在下一次 [`Self::step`] 开始时被替换；调用方不得把它当作存档、委托或成交。
    pub fn last_retail_decisions(&self) -> &[RetailDecisionTrace] {
        &self.last_retail_decisions
    }

    /// 读取上一 tick 的散户订单生命周期诊断事件；不属于存档或撮合状态。
    pub fn last_retail_order_events(&self) -> &[RetailOrderDiagnosticEvent] {
        &self.last_retail_order_events
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
        self.npc_decision_traces
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
        if self.retail_experience.contains_key(&account) {
            self.last_retail_order_events
                .push(RetailOrderDiagnosticEvent::Canceled {
                    account,
                    code,
                    order_id,
                    remaining_qty,
                });
        }
    }

    fn record_retail_intent_rejections(&mut self, account: AccountId, events: &[Event]) {
        if !self.retail_experience.contains_key(&account) {
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
                    self.last_retail_order_events
                        .push(RetailOrderDiagnosticEvent::Rejected {
                            account,
                            code: code.clone(),
                            reason: reason.clone(),
                        });
                }
            }
        }
    }

    fn reserved_cash_for_account(&self, account: AccountId) -> Result<Money, MoneyError> {
        let auction_reserved = self
            .auction_orders
            .values()
            .flatten()
            .filter(|order| order.owner == account)
            .try_fold(Money::ZERO, |total, order| {
                let required = live_cash_reservation(
                    &self.setup.config,
                    order.side,
                    order.limit,
                    order.qty,
                    Money::ZERO,
                )?;
                total.add(required)
            })?;
        self.markets
            .values()
            .flat_map(|market| market.resting_orders_for(account))
            .try_fold(auction_reserved, |total, order| {
                let required = live_cash_reservation(
                    &self.setup.config,
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
                .accounts
                .get(&account)
                .is_none_or(|candidate| candidate.kind == AccountKind::Player)
            || self.is_active_parent_child(account, code, order.id)
        {
            return;
        }
        if self
            .npc_order_lifecycles
            .iter()
            .any(|lifecycle| lifecycle.order_id == order.id)
        {
            panic!(
                "NPC quote lifecycle already exists for order {}",
                order.id.0
            );
        }
        let placed_market_minute = self.current_market_minute();
        let lifetime_minutes = self.npc_quote_lifetime_minutes_at_quote(
            account, code, order, last_price, best_bid, best_ask,
        );
        let day_end = (u64::from(self.day) + 1)
            .checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .expect("session day plus one fits market-minute range");
        let expires_market_minute = placed_market_minute
            .checked_add(lifetime_minutes)
            .expect("NPC quote lifetime fits market-minute range")
            .min(day_end);
        self.npc_order_lifecycles.push(NpcOrderLifecycle {
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
            .accounts
            .get(&account)
            .expect("lifecycle registration only accepts an existing account")
            .kind;
        let base = match kind {
            AccountKind::Retail => 18_u64,
            AccountKind::Inst => 36_u64,
            AccountKind::Hot => 8_u64,
            AccountKind::Player => panic!("player orders must not receive NPC quote lifecycles"),
        };
        let tick_cents = self
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
        let deterministic_jitter = self.seed
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
        self.parent_orders
            .get(&account)
            .and_then(|plans| plans.get(code))
            .is_some_and(|plan| plan.active_child_order_id == Some(order_id))
    }

    fn remove_npc_order_lifecycle(
        &mut self,
        account: AccountId,
        code: &StockCode,
        order_id: OrderId,
    ) {
        self.npc_order_lifecycles.retain(|lifecycle| {
            !(lifecycle.account == account
                && lifecycle.code == *code
                && lifecycle.order_id == order_id)
        });
    }

    ///
    /// 玩家账户不存在 → [`SessionError::UnknownPlayer`]（致命错误显式返回，铁律二）。
    pub fn enqueue_player_intent(
        &mut self,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SessionError> {
        let account = self
            .accounts
            .get(&player_id)
            .ok_or(SessionError::UnknownPlayer(player_id))?;
        if account.kind != AccountKind::Player {
            return Err(SessionError::NotPlayer(player_id));
        }
        self.pending_player.push((player_id, intent));
        Ok(())
    }

    /// 生成存档（精确到交易日）。
    /// 在 DayBoundary 后调用 → snapshot 含 end_of_day 后的状态（last_close 已更新）。
    fn save_projection(&self, runtime_v2: SaveRuntimeV2) -> SaveSlot {
        SaveSlot {
            schema_version: SAVE_SCHEMA_VERSION_V2,
            runtime_v2,
            setup: self.setup.clone(),
            seed: self.seed,
            snapshot: self.snapshot_inner(true, true),
            auction_orders: self.auction_orders.clone(),
            resting_orders: self
                .markets
                .iter()
                .map(|(code, market)| (code.clone(), market.resting_orders()))
                .collect(),
            filled_orders: self
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
                .price_history
                .iter()
                .map(|(code, prices)| (code.clone(), prices.iter().copied().collect()))
                .collect(),
            market_minute_closes: self.market_minute_closes.clone(),
            rng_state: self.rng.state,
            npc_attention: self.npc_attention.to_map(),
            retail_experience: self.retail_experience.to_map(),
            parent_orders: self.parent_orders.clone(),
            npc_order_lifecycles: self.npc_order_lifecycles.clone(),
            pending_player: self.pending_player.clone(),
            pending_npc: self.pending_npc.clone(),
            next_order_id: self.next_order_id,
            civil_clock: self.civil_clock.save(),
            // K7（任务 27）：公司域与个体决策链权威状态全量入档。
            company_operations: self.operations.as_ref().clone(),
            closing_registry: self.closing.clone(),
            public_library: self.library.clone(),
            ops_wiring: self.ops_wiring.clone(),
            disclosures: self.disclosures.clone(),
            plans: self.plans.clone(),
            information_states: self.information.clone(),
            belief_books: self.belief_books.to_map(),
            watchlists: self.watchlists.to_map(),
            price_memories: self.price_memories.to_map(),
            // 存档契约只保留「计划簿中仍存活」的待应用事实；未知/已终止计划
            // 的迟到条目在此显式丢弃（永不适用；见 issues.md 任务 27 §3）。
            pending_plan_events: self
                .pending_plan_events
                .iter()
                .copied()
                .filter(|event| {
                    self.plans
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
        validate_save_slot(save)?;
        let mut sess = GameSession::new(save.setup.clone(), save.seed)?;

        // 清空初始持仓分配 → 用快照精确覆盖
        for acc in sess.accounts.values_mut() {
            acc.positions.clear();
        }

        // 恢复账户状态（cash + positions 精确值）
        for (id, snap_acc) in &save.snapshot.accounts {
            let acc = sess
                .accounts
                .get_mut(id)
                .expect("validated save account set exactly matches setup");
            acc.cash = snap_acc.cash;
            for (code, pos) in &snap_acc.positions {
                acc.positions.insert(
                    code.clone(),
                    crate::account::Position {
                        qty: pos.qty,
                        t1_locked: pos.t1_locked,
                        invested_cents: pos.invested_cents,
                        recovered_cents: pos.recovered_cents,
                    },
                );
            }
        }

        // 恢复市场状态（last_price/last_close）
        for (code, snap_mkt) in &save.snapshot.markets {
            let market = sess
                .markets
                .get_mut(code)
                .expect("validated save market set exactly matches setup");
            market.set_last_price(snap_mkt.last_price);
            market.set_last_close(snap_mkt.last_close);
        }

        // 按原到达序重建订单簿。有效静态订单簿不应自行成交；若发生，说明存档互相交叉。
        for (code, saved_orders) in &save.resting_orders {
            let market = sess
                .markets
                .get_mut(code)
                .expect("validated save resting-order market must exist");
            let mut ordered = saved_orders.clone();
            ordered.sort_by_key(|order| order.seq);
            for order in ordered {
                let result = market.place(order).map_err(|error| {
                    SessionError::InvalidSave(format!(
                        "cannot restore resting orders for {}: {error}",
                        code.0
                    ))
                })?;
                if !result.trades.is_empty() || result.resting.is_none() {
                    return Err(SessionError::InvalidSave(format!(
                        "resting orders for {} cross during restore",
                        code.0
                    )));
                }
            }
        }
        for (code, filled) in &save.filled_orders {
            sess.markets
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
        for (code, market) in &sess.markets {
            let saved_market = save
                .snapshot
                .markets
                .get(code)
                .expect("validated save market must exist");
            if market.bid_depth() != saved_market.bids || market.ask_depth() != saved_market.asks {
                return Err(SessionError::InvalidSave(format!(
                    "resting orders for {} do not match saved depth",
                    code.0
                )));
            }
            if saved_market.best_bid != saved_market.bids.first().map(|(price, _)| *price)
                || saved_market.best_ask != saved_market.asks.first().map(|(price, _)| *price)
            {
                return Err(SessionError::InvalidSave(format!(
                    "market {} best price does not match depth",
                    code.0
                )));
            }
        }

        // 恢复进度
        sess.tick = save.snapshot.tick;
        sess.day = save.snapshot.day;
        sess.seq = save.snapshot.seq;
        // 恢复自然日时钟（自洽全量校验；政策 v1 重建，任务 27 起随档冻结）。
        sess.civil_clock = CivilClock::from_parts(
            save.setup.start_date,
            &save.civil_clock,
            session_calendar_exchange(save.setup.stocks[0].exchange),
        )?;
        validate_saved_order_state(&sess, save)?;
        sess.auction_orders = save.auction_orders.clone();
        sess.next_order_id = save.next_order_id;

        // 当前存档完整覆盖所有影响后续演进的确定性状态。
        sess.daily_candles = save
            .snapshot
            .daily_candles
            .iter()
            .map(|(code, candles)| (code.clone(), candles.clone().into()))
            .collect();
        sess.active_daily_candles = save.snapshot.active_daily_candles.clone();
        sess.price_history = save
            .price_history
            .iter()
            .map(|(code, prices)| (code.clone(), prices.iter().copied().collect()))
            .collect();
        sess.market_minute_closes = save.market_minute_closes.clone();
        sess.rng.state = save.rng_state;
        let mut restored_attention = BTreeMap::new();
        for (id, saved_state) in &save.npc_attention {
            let reconstructed = sess
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
                    base_probability: reconstructed.base_probability,
                    next_attention_candidate_tick: saved_state.next_attention_candidate_tick,
                    rng_state: saved_state.rng_state,
                },
            );
        }
        sess.npc_attention = restored_attention.into();
        sess.attention_queue = sess
            .npc_attention
            .iter()
            .map(|(id, state)| Reverse((state.next_attention_candidate_tick, *id)))
            .collect();
        sess.retail_experience = save.retail_experience.clone().into();
        sess.parent_orders = save.parent_orders.clone();
        sess.npc_order_lifecycles = save.npc_order_lifecycles.clone();
        sess.pending_player = save.pending_player.clone();
        sess.pending_npc = save.pending_npc.clone();
        // new() prepares its own first NPC batch; the saved batch replaces it, so its
        // diagnostic samples must not leak into the restored session.
        sess.last_retail_decisions.clear();

        // K7（任务 27）：公司域与个体决策链权威状态直接从档恢复——不再前史
        // 重放、不再复位信念/计划/信息集、不再剥离 linked_plan_id。new() 重建
        // 的 prehistory/时钟接线是确定性产物，被下列赋值整体覆盖。
        let expected_issuers: BTreeSet<&crate::company::CompanyId> = save
            .setup
            .stocks
            .iter()
            .filter_map(|stock| sess.company_registry.issuer_of(&stock.code))
            .collect();
        let saved_issuers: BTreeSet<&crate::company::CompanyId> =
            save.company_operations.companies.keys().collect();
        if expected_issuers != saved_issuers {
            return Err(SessionError::InvalidSave(
                "saved company set does not exactly match the issuers rebuilt from setup"
                    .to_string(),
            ));
        }
        // 个体状态账户集合精确匹配确定性重建（populate_npcs 按 seed+ordinal
        // 重建信念机构集合）：缺失任一账户的个人状态 = 不完整存档。
        let expected_belief_accounts: BTreeSet<AccountId> =
            sess.belief_books.keys().copied().collect();
        let saved_belief_accounts: BTreeSet<AccountId> =
            save.belief_books.keys().copied().collect();
        if expected_belief_accounts != saved_belief_accounts {
            return Err(SessionError::InvalidSave(format!(
                "saved personal-state account set {saved_belief_accounts:?} does not match the \
                 reconstructed belief accounts {expected_belief_accounts:?}"
            )));
        }
        sess.operations = std::sync::Arc::new(save.company_operations.clone());
        sess.closing = save.closing_registry.clone();
        sess.library = save.public_library.clone();
        sess.ops_wiring = save.ops_wiring.clone();
        sess.disclosures = save.disclosures.clone();
        sess.plans = save.plans.clone();
        sess.information = save.information_states.clone();
        sess.belief_books = save.belief_books.clone().into();
        sess.watchlists = save.watchlists.clone().into();
        sess.price_memories = save.price_memories.clone().into();
        sess.pending_plan_events = save.pending_plan_events.clone();
        // 与 new() 相同的进程内接线（观察者 hook 不入档，恢复后重装）。
        sess.disclosures.install(&mut sess.civil_clock);

        // 最后原子替换 v2-only authority。到此账户、市场、订单、ID/seq、RNG 与
        // 决策链旧字段均已恢复，runtime 可以对完整 live-order 域做交叉校验。
        persistence::restore_runtime_v2(&mut sess, &save.runtime_v2)?;

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
                    tick_cents: 1,
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
            float_allocation: FloatAllocation::Random,
            start_date: default_civil_start_date(),
            simulation_policy_id: SIMULATION_POLICY_ID_V2.to_string(),
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

            let candle = session.active_daily_candles.get(&code).unwrap();
            assert_eq!(candle.open, auction_open);
            assert_ne!(candle.open, previous_close);
            assert_eq!(candle.close, close);
            assert_eq!(candle.volume, 150);

            session.commit_active_daily_candles();
            session.day += 1;
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

            let candle = session.active_daily_candles.get(&code).unwrap();
            assert!(candle.open < previous_close, "测试数据必须保持跳空低开");
            assert_eq!(candle.open, auction_open);
            assert_eq!(candle.high, auction_open);
            assert_eq!(candle.low, close);
            assert_eq!(candle.close, close);
            assert_eq!(candle.volume, 150);

            session.commit_active_daily_candles();
            session.day += 1;
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
                price,
                qty,
            } => {
                let id = OrderId(self.next_order_id);
                self.next_order_id = self
                    .next_order_id
                    .checked_add(1)
                    .expect("fixture order IDs");
                let seq = self.next_seq();
                if auction {
                    self.auction_orders
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
                        let market = self.markets.get(&code).expect("fixture stock exists");
                        (market.last_price(), market.best_bid(), market.best_ask())
                    };
                    if self.accounts[&account].kind != AccountKind::Player {
                        let working = self.markets[&code]
                            .resting_orders_for(account)
                            .into_iter()
                            .filter(|order| order.side == side)
                            .collect::<Vec<_>>();
                        for old in working {
                            self.markets.get_mut(&code).unwrap().cancel(old.id).unwrap();
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
            Intent::Cancel { code, id } => {
                if !auction {
                    let order = self
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
        self.envelope_ledger = crate::session::pipeline::EnvelopeLedger::new(
            self.next_receipt_base,
            self.project_live_envelopes()
                .expect("fixture live envelopes are valid"),
        )
        .expect("fixture ledger is valid");
    }
}

#[cfg(test)]
mod npc_working_quote_tests {
    use super::*;
    use crate::{HotParams, InstParams, PlanId, RetailParams};

    fn defer_npc_attention(session: &mut GameSession, account: AccountId) {
        let next_tick = session.tick.checked_add(1).unwrap();
        session
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = next_tick;
        session.attention_queue.clear();
        session.attention_queue.push(Reverse((next_tick, account)));
    }

    pub(super) fn quote_setup(auction_ticks: u64) -> SessionSetup {
        let code = StockCode("600888".to_string());
        SessionSetup {
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
                    tick_cents: 1,
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
            float_allocation: FloatAllocation::Random,
            start_date: default_civil_start_date(),
            simulation_policy_id: SIMULATION_POLICY_ID_V2.to_string(),
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
            price: Money::from_cents(price),
            qty: 100,
        }
    }

    pub(super) fn two_stock_quote_setup() -> SessionSetup {
        let mut setup = quote_setup(0);
        let second = StockCode("600889".to_string());
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
        let attention = session.npc_attention.get_mut(&account).unwrap();
        attention.base_probability = 1.0;
        attention.next_attention_candidate_tick = tick;
        session.attention_queue.push(Reverse((tick, account)));
    }

    #[test]
    fn accepted_retail_observation_persists_an_unheld_watchlist_stock() {
        let code = StockCode("600888".to_string());
        let retail = AccountId(1);
        let mut session = GameSession::new(retail_quote_setup(), 124).unwrap();
        force_attention_candidate(&mut session, retail, 0);

        session.step().expect("healthy step");

        let experience = &session.retail_experience[&retail];
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
            .parent_orders
            .entry(institution)
            .or_default()
            .insert(
                code.clone(),
                ParentOrderPlan {
                    code: code.clone(),
                    side: Side::Buy,
                    target_qty: 400,
                    filled_qty: 350,
                    child_qty: 100,
                    active_child_order_id: None,
                    active_child_remaining_qty: None,
                    linked_plan_id: None,
                    limit_price: Money::from_cents(1_000),
                    expires_market_minute: PARENT_ORDER_HORIZON_MINUTES,
                },
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
            session.parent_orders[&institution][&code].remaining_qty(),
            50
        );
    }

    #[test]
    fn closing_auction_does_not_duplicate_a_parent_child_still_resting_in_continuous_book() {
        let code = StockCode("600888".to_string());
        let institution = AccountId(1);
        let mut setup = quote_setup(0);
        setup.ticks_per_day = 100;
        setup.closing_auction_ticks = 10;
        let mut session = GameSession::new(setup, 993).unwrap();
        session.tick = 90;
        session
            .parent_orders
            .entry(institution)
            .or_default()
            .insert(
                code.clone(),
                ParentOrderPlan {
                    code: code.clone(),
                    side: Side::Buy,
                    target_qty: 400,
                    filled_qty: 0,
                    child_qty: 100,
                    active_child_order_id: Some(OrderId(1)),
                    active_child_remaining_qty: Some(100),
                    linked_plan_id: None,
                    limit_price: Money::from_cents(1_000),
                    expires_market_minute: PARENT_ORDER_HORIZON_MINUTES * 2,
                },
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
        session.next_order_id = 2;
        session
            .parent_orders
            .entry(institution)
            .or_default()
            .insert(
                code.clone(),
                ParentOrderPlan {
                    code: code.clone(),
                    side: Side::Buy,
                    target_qty: 400,
                    filled_qty: 0,
                    child_qty: 100,
                    active_child_order_id: Some(OrderId(1)),
                    active_child_remaining_qty: Some(100),
                    linked_plan_id: None,
                    limit_price: Money::from_cents(1_000),
                    expires_market_minute: PARENT_ORDER_HORIZON_MINUTES * 2,
                },
            );

        session
            .hydrate_or_validate_envelope_ledger()
            .expect("direct order-book fixture must synchronize current save authority");

        let restored = GameSession::restore(&session.save().expect("healthy save")).unwrap();
        assert_eq!(restored.parent_orders, session.parent_orders);
    }

    #[test]
    fn market_view_keeps_tick_samples_separate_from_completed_market_minutes() {
        let code = StockCode("600888".to_string());
        let mut session = GameSession::new(quote_setup(0), 1_001).unwrap();
        session.price_history.insert(
            code.clone(),
            [Money::from_cents(1_000), Money::from_cents(1_050)]
                .into_iter()
                .collect(),
        );
        session.market_minute_closes.insert(
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
        let template = session.daily_candles[&code][0].clone();
        session.daily_candles.insert(
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
        session.market_minute_closes.insert(
            code.clone(),
            vec![MarketMinuteClose {
                absolute_trading_minute: 0,
                close: Money::from_cents(1_100),
            }],
        );

        let retained = retained_behavior_daily_closes_history(&session.daily_candles[&code]);
        assert_eq!(retained.len(), 250);
        assert_eq!(retained.first().unwrap().trading_day, 5_750);
        assert_eq!(retained.last().unwrap().trading_day, 5_999);

        let short = retained_behavior_daily_closes(
            &session.daily_candles[&code]
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
        let first = session.daily_candles[&code][0].clone();
        let previous = session.daily_candles[&code].len();
        let closed = DailyCandle {
            time: 0,
            ..session.daily_candles[&code].last().unwrap().clone()
        };
        session
            .active_daily_candles
            .insert(code.clone(), closed.clone());

        session.commit_active_daily_candles();

        assert_eq!(session.daily_candles[&code].len(), previous + 1);
        assert_eq!(session.daily_candles[&code][0], first);
        assert_eq!(session.daily_candles[&code].last(), Some(&closed));
        assert_eq!(session.snapshot().daily_candles[&code].len(), previous + 1);
    }

    #[test]
    fn realized_profit_can_make_cost_return_unavailable_without_breaking_observation() {
        let code = StockCode("600888".to_string());
        let account = AccountId(1);
        let mut session = GameSession::new(retail_quote_setup(), 121).unwrap();
        let config = session.setup.config.clone();
        let holder = session.accounts.get_mut(&account).unwrap();
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
            .npc_attention
            .values()
            .map(|state| state.next_attention_candidate_tick)
            .collect();

        assert!(candidate_ticks.len() > 1);
        assert!(candidate_ticks.iter().any(|tick| *tick > 0));
    }
}
