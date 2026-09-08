//! 编排层（ADR-0005 §5 GameSession）：把 money/config/orderbook/account/market/strategy
//! 串成每 tick 完整循环，产出快照 + 带序号增量事件流。种子化确定性 RNG。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-session-design.md。
//! 纯逻辑、无 I/O、无全局可变状态（联机预留：实例即隔离）。

mod auction;
mod candles;
mod persistence;

use auction::{auction_total_imbalance, clearing_result};
use candles::{generate_preset_daily_candles, stock_code_hash};
use persistence::{migrate_save_slot, validate_save_slot, validate_saved_order_state};

use crate::account::{Account, AccountError, AccountKind, Position, SettlementTotals, StockCode};
use crate::config::{ConfigError, GameConfig};
use crate::market::{Market, MarketError, VParams};
use crate::money::{Money, MoneyError};
use crate::orderbook::{AccountId, Order, OrderError, OrderId, Side};
use crate::strategy::Rng;
use crate::strategy::{
    Intent, MarketView, PositionView, SelfView, StockView, StrategyError, StrategyFactory,
    StrategyParams,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
    #[default]
    Continuous,
}

/// 增量事件（带单调 seq）。非错误类型：运行期失败（意图被拒/结算失败/V 失败）
/// 进事件流供前端呈现，不中断 tick 循环（铁律二：显式可见，不静默丢弃）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
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
    /// 集合竞价每 tick 的虚拟撮合结果；没有交叉时价格为 None、量为 0。
    AuctionTick {
        seq: u64,
        tick: u64,
        code: StockCode,
        indicative_price: Option<Money>,
        matched_volume: u64,
        imbalance: u64,
    },
    /// 集合竞价结束并一次性按唯一开盘价撮合。
    AuctionCompleted {
        seq: u64,
        tick: u64,
        code: StockCode,
        opening_price: Option<Money>,
        matched_volume: u64,
    },
    /// 价格 tick：每 tick 末记录最新价。
    PriceTick {
        seq: u64,
        /// 权威游戏世界 tick；宿主压缩事件后仍可恢复交易分钟位置。
        tick: u64,
        code: StockCode,
        last_price: Money,
        /// Rust 聚合的权威当日日 K；宿主只同步，不再自行重算 OHLCV。
        daily_candle: DailyCandle,
        /// 当前买盘前五档（价高到低），数量仍以股为引擎单位。
        bids: Vec<(Money, u32)>,
        /// 当前卖盘前五档（价低到高）。
        asks: Vec<(Money, u32)>,
    },
    /// 日界：到 ticks_per_day 触发，day 自增。
    DayBoundary {
        seq: u64,
        day: u32,
        /// 刚刚收盘的每股日 K，供增量客户端提交历史而无需拉取 360 日全量快照。
        closed_daily_candles: BTreeMap<StockCode, DailyCandle>,
    },
    /// 意图被拒（资金/持仓/涨跌停/未知股票）。
    IntentRejected {
        seq: u64,
        account: AccountId,
        code: StockCode,
        reason: RejectionReason,
    },
    /// 结算失败（账户侧异常，透传 AccountError 文案）。
    SettlementError {
        seq: u64,
        account: AccountId,
        code: StockCode,
        reason: String,
    },
    /// V 演化失败（market.evolve_v 异常）。
    VError {
        seq: u64,
        code: StockCode,
        reason: String,
    },
    /// 连续竞价委托已撤销，冻结资金或股份随即释放。
    OrderCanceled {
        seq: u64,
        account: AccountId,
        code: StockCode,
        id: OrderId,
        remaining_qty: u32,
    },
    /// 限价委托有未成交余量进入连续竞价订单簿。
    OrderAccepted {
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
            | Self::IntentRejected { seq, .. }
            | Self::SettlementError { seq, .. }
            | Self::VError { seq, .. }
            | Self::OrderCanceled { seq, .. }
            | Self::OrderAccepted { seq, .. } => *seq,
        }
    }
}

/// 市场快照子结构（单股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct MarketSnap {
    pub last_price: Money,
    pub last_close: Money,
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    /// 仅存档等可信内部边界携带。面向玩家的运行快照必须为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fundamental_value: Option<Money>,
    /// 买盘深度（价高→低，每价聚合总量）。前端取前 N 档渲染五档盘口。
    pub bids: Vec<(Money, u32)>,
    /// 卖盘深度（价低→高，每价聚合总量）。
    pub asks: Vec<(Money, u32)>,
}

/// 持仓快照子结构（单只股票）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct PositionSnap {
    pub qty: u32,
    pub t1_locked: u32,
    pub invested_cents: i64,
    pub recovered_cents: i64,
}

/// 账户快照子结构。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct AccountSnap {
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionSnap>,
    /// 已被当日未成交买单占用的资金（含剩余成交额对应的佣金与过户费）。
    #[serde(default)]
    pub reserved_buy_cash: Money,
    /// 已被当日未成交卖单占用的股数，按股票汇总。
    #[serde(default)]
    pub reserved_sell_qty: BTreeMap<StockCode, u32>,
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
    pub volume: u64,
}

/// 完整玩家状态快照（首次连接/重连）。隐藏基本面 V 不跨玩家边界泄露。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct Snapshot {
    pub seq: u64,
    pub tick: u64,
    pub day: u32,
    #[serde(default)]
    pub phase: TradingPhase,
    pub markets: BTreeMap<StockCode, MarketSnap>,
    pub accounts: BTreeMap<AccountId, AccountSnap>,
    /// Rust 引擎持有的已完成日 K；首次连接、重连与存档恢复均由快照同步。
    #[serde(default)]
    pub daily_candles: BTreeMap<StockCode, Vec<DailyCandle>>,
    /// 当前交易日正在形成的日 K。旧存档缺少该字段时按空处理。
    #[serde(default)]
    pub active_daily_candles: BTreeMap<StockCode, DailyCandle>,
}

/// 存档槽：保存权威市场、账户、集合竞价及连续竞价未成交委托。
/// 前端分时采样属于派生 UI 数据，不进入权威存档；日 K 由 engine 持久化。
#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct SaveSlot {
    /// 存档 schema 版本。缺失表示最初的 v1 格式；当前写出 v2。
    #[serde(default = "default_legacy_save_schema_version")]
    pub schema_version: u32,
    pub setup: SessionSetup,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub seed: u64,
    pub snapshot: Snapshot,
    /// 日内存档恢复集合竞价所需的完整委托队列。
    #[serde(default)]
    pub auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    /// 连续竞价未成交委托；旧存档缺失时按空处理。
    #[serde(default)]
    pub resting_orders: BTreeMap<StockCode, Vec<Order>>,
    /// 策略观察所需的短价格窗口。它会影响下一 tick 的决策，因此属于权威状态。
    #[serde(default)]
    pub price_history: BTreeMap<StockCode, Vec<Money>>,
    /// 当前随机数生成器状态；用十进制字符串避免 JavaScript 丢失 u64 精度。
    /// 旧存档缺失时保留按 setup/seed 完成初始化后的状态。
    #[serde(default, with = "optional_u64_decimal")]
    #[ts(type = "string | null")]
    pub rng_state: Option<u64>,
    /// 保持订单 id/到达序继续单调递增。
    #[serde(default = "default_next_order_id")]
    pub next_order_id: u64,
}

#[derive(serde::Deserialize)]
struct SaveSlotWire {
    #[serde(default = "default_legacy_save_schema_version")]
    schema_version: u32,
    setup: serde_json::Value,
    #[serde(with = "u64_decimal")]
    seed: u64,
    snapshot: Snapshot,
    #[serde(default)]
    auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    #[serde(default)]
    resting_orders: BTreeMap<StockCode, Vec<Order>>,
    #[serde(default)]
    price_history: BTreeMap<StockCode, Vec<Money>>,
    #[serde(default, with = "optional_u64_decimal")]
    rng_state: Option<u64>,
    #[serde(default = "default_next_order_id")]
    next_order_id: u64,
}

impl<'de> serde::Deserialize<'de> for SaveSlot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut wire = <SaveSlotWire as serde::Deserialize>::deserialize(deserializer)?;
        if wire.schema_version == LEGACY_SAVE_SCHEMA_VERSION {
            normalize_v1_stock_fields(&mut wire.setup).map_err(serde::de::Error::custom)?;
        }
        let setup = serde_json::from_value(wire.setup).map_err(serde::de::Error::custom)?;
        Ok(Self {
            schema_version: wire.schema_version,
            setup,
            seed: wire.seed,
            snapshot: wire.snapshot,
            auction_orders: wire.auction_orders,
            resting_orders: wire.resting_orders,
            price_history: wire.price_history,
            rng_state: wire.rng_state,
            next_order_id: wire.next_order_id,
        })
    }
}

mod u64_decimal {
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
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Decimal(String),
            LegacyNumber(u64),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Decimal(value) => value.parse::<u64>().map_err(serde::de::Error::custom),
            Repr::LegacyNumber(value) => Ok(value),
        }
    }
}

mod optional_u64_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Decimal(String),
            LegacyNumber(u64),
        }

        Option::<Repr>::deserialize(deserializer)?
            .map(|value| match value {
                Repr::Decimal(value) => value.parse::<u64>().map_err(serde::de::Error::custom),
                Repr::LegacyNumber(value) => Ok(value),
            })
            .transpose()
    }
}

pub const SAVE_SCHEMA_VERSION: u32 = 2;
const LEGACY_SAVE_SCHEMA_VERSION: u32 = 1;

fn default_legacy_save_schema_version() -> u32 {
    LEGACY_SAVE_SCHEMA_VERSION
}

fn default_next_order_id() -> u64 {
    1
}

fn normalize_v1_stock_fields(setup: &mut serde_json::Value) -> Result<(), String> {
    let stocks = setup
        .get_mut("stocks")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| "legacy save setup.stocks must be an array".to_string())?;
    for (index, stock) in stocks.iter_mut().enumerate() {
        let stock = stock
            .as_object_mut()
            .ok_or_else(|| format!("legacy save setup.stocks[{index}] must be an object"))?;
        let code = stock
            .get("code")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("legacy save setup.stocks[{index}].code must be a string"))?
            .to_string();
        if !stock.contains_key("exchange") {
            let exchange = StockExchange::from_a_share_code(&StockCode(code.clone()))?;
            stock.insert(
                "exchange".to_string(),
                serde_json::to_value(exchange)
                    .map_err(|error| format!("cannot serialize inferred exchange: {error}"))?,
            );
        }
        if !stock.contains_key("category") {
            let category = if code.starts_with("300") || code.starts_with("301") {
                SecurityCategory::ChiNext
            } else if stock
                .get("limit_pct")
                .and_then(serde_json::Value::as_f64)
                .is_some_and(|limit| (limit - 0.05).abs() <= f64::EPSILON)
            {
                SecurityCategory::StMainBoard
            } else {
                SecurityCategory::MainBoard
            };
            stock.insert(
                "category".to_string(),
                serde_json::to_value(category)
                    .map_err(|error| format!("cannot serialize inferred category: {error}"))?,
            );
        }
    }
    Ok(())
}

/// 可序列化的集合竞价限价委托。arrival_seq 同时承担价格相同时的时间优先键。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct AuctionOrderSnap {
    pub owner: AccountId,
    pub side: Side,
    pub limit: Money,
    pub qty: u32,
    pub arrival_seq: u64,
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
    /// 入队意图时账户存在，但它是引擎控制的 NPC，不接受玩家指令。
    #[error("account is not controlled by a player: {0:?}")]
    NotPlayer(AccountId),
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
    /// 透传游戏配置错误。
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// 存档 schema 或领域不变量不合法。
    #[error("invalid save: {0}")]
    InvalidSave(String),
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
    /// 从当前模型支持的六位沪深 A 股代码推断交易所，仅供旧存档迁移和一致性校验。
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

/// 单只股票初始规格（行情/涨跌停/V/tick/流通盘）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct StockSpec {
    pub code: StockCode,
    /// 上市交易所；新配置必须显式提供，旧存档缺失时仅按受支持的标准代码迁移。
    pub exchange: StockExchange,
    pub initial_price: Money,
    /// 证券板块/风险警示类别；新配置必须显式提供，旧存档由 v1 wire 迁移。
    pub category: SecurityCategory,
    pub limit_pct: f64,
    pub v_initial: Money,
    pub tick: Money,
    /// 流通盘股数：新游戏时分配给 NPC。0 表示不分配（兼容加载存档路径）。
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

/// NPC 群体配置（三类计数 + 单户初始现金）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct NpcSetup {
    pub retail_count: u32,
    pub inst_count: u32,
    pub hot_count: u32,
    pub cash_per_npc: Money,
}

/// session 初始化参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct SessionSetup {
    pub stocks: Vec<StockSpec>,
    pub npcs: NpcSetup,
    pub config: GameConfig,
    pub v_params: VParams,
    /// 每只股票的隐藏基本价值长期均值；旧配置缺失时回退到该股 `v_initial`。
    #[serde(default)]
    pub fundamental_value_means: BTreeMap<StockCode, Money>,
    pub strategy_params: StrategyParams,
    pub ticks_per_day: u64,
    /// 每个交易日 09:15–09:30 开盘窗口的 tick 数；前 2/3 为集合竞价申报，后 1/3 为 PreOpen。
    /// 旧配置缺失时为 0。
    #[serde(default)]
    pub auction_ticks: u64,
    pub history_len: usize,
    pub t1_enabled: bool,
    /// 流通盘分配方式（新游戏时如何把 float_shares 分给 NPC）。
    pub float_allocation: FloatAllocation,
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
        if self.history_len == 0 {
            return Err(SessionError::InvalidSetup(
                "history_len must be > 0".to_string(),
            ));
        }
        if self.npcs.cash_per_npc.cents() < 0 {
            return Err(SessionError::InvalidSetup(
                "cash_per_npc must be >= 0".to_string(),
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
        if self.v_params.long_run_mean.cents() <= 0
            || !self.v_params.mean_reversion.is_finite()
            || self.v_params.mean_reversion < 0.0
            || !self.v_params.volatility.is_finite()
            || self.v_params.volatility < 0.0
        {
            return Err(SessionError::InvalidSetup(
                "v_params require a positive mean and finite non-negative rates".to_string(),
            ));
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
        for (code, mean) in &self.fundamental_value_means {
            if !codes.contains(code) || mean.cents() <= 0 {
                return Err(SessionError::InvalidSetup(format!(
                    "invalid fundamental value mean for stock {}",
                    code.0
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
        }
        Ok(())
    }
}

/// 编排层。持有全部状态；纯逻辑、无全局可变状态（实例即隔离）。
///
/// NPC 与玩家同构（均为 [`Account`]），区别只在 `strategy`（NPC=算法、玩家=None）。
/// 单一 RNG 源 `rng`（[`SplitMix64`]）贯穿全 tick，保证确定性（同种子同输入同输出）。
pub struct GameSession {
    setup: SessionSetup,
    rng: SplitMix64,
    seed: u64,
    markets: BTreeMap<StockCode, Market>,
    accounts: BTreeMap<AccountId, Account>,
    price_history: BTreeMap<StockCode, VecDeque<Money>>,
    daily_candles: BTreeMap<StockCode, Vec<DailyCandle>>,
    active_daily_candles: BTreeMap<StockCode, DailyCandle>,
    auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    pending_player: Vec<(AccountId, Intent)>,
    next_order_id: u64,
    tick: u64,
    day: u32,
    seq: u64,
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

#[derive(Copy, Clone)]
struct OrderValidationInput<'a> {
    account: AccountId,
    code: &'a StockCode,
    side: Side,
    price: Money,
    qty: u32,
    is_market: bool,
}

fn fee_delta(
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

fn buy_order_reservation(
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

impl GameSession {
    /// 构造 session。校验参数 → 建 markets/accounts → 注入 NPC 策略。
    ///
    /// - 校验 `stocks` 非空、`ticks_per_day > 0`，否则 [`SessionError::InvalidSetup`]（铁律二：绝不静默）。
    /// - 每股构造一个 [`Market`]（`last_close = last_price = initial_price`），并初始化空价格历史队列。
    /// - 玩家 `AccountId(0)`：`Account::new`（strategy 默认 None）。
    /// - NPC 按 retail/inst/hot 计数逐个生成（id 递增），`StrategyFactory::build` 注入策略。
    pub fn new(setup: SessionSetup, seed: u64) -> Result<GameSession, SessionError> {
        setup.validate()?;
        let mut markets = BTreeMap::new();
        let mut price_history = BTreeMap::new();
        for s in &setup.stocks {
            markets.insert(
                s.code.clone(),
                Market::new(
                    s.code.clone(),
                    s.initial_price,
                    s.limit_pct,
                    s.v_initial,
                    s.tick,
                )?,
            );
            price_history.insert(s.code.clone(), VecDeque::new());
        }
        let daily_candles = generate_preset_daily_candles(&setup, seed);
        let mut accounts = BTreeMap::new();
        accounts.insert(
            AccountId(0),
            Account::new(
                AccountId(0),
                AccountKind::Player,
                setup.config.starting_cash,
            ),
        );
        let rng = SplitMix64::new(seed);
        let mut sess = GameSession {
            setup,
            rng,
            seed,
            markets,
            accounts,
            price_history,
            daily_candles,
            active_daily_candles: BTreeMap::new(),
            auction_orders: BTreeMap::new(),
            pending_player: Vec::new(),
            next_order_id: 1,
            tick: 0,
            day: 0,
            seq: 0,
        };
        sess.populate_npcs(AccountKind::Retail)?;
        sess.populate_npcs(AccountKind::Inst)?;
        sess.populate_npcs(AccountKind::Hot)?;
        sess.seed_float()?; // 分配流通盘给 NPC（筹码守恒、确定性、玩家不分配）
        Ok(sess)
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
            let (_, ratio, ids) = &by_kind[i];
            let kind_float = if idx == nonempty.len() - 1 {
                remaining // 最后一类拿整体余量 → 全局守恒
            } else if norm > 0.0 {
                ((float as f64 * *ratio / norm).round() as u32).min(remaining)
            } else {
                0
            };
            let parts = self.split_random(kind_float, ids);
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
        let n = ids.len();
        if n == 0 || float == 0 {
            return ids.iter().map(|id| (*id, 0u32)).collect();
        }
        // 随机权重（+ epsilon 防零权重导致某 NPC 恒为 0）。
        let weights: Vec<f64> = (0..n).map(|_| self.rng.next_f64() + 1e-9).collect();
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
        for _ in 0..count {
            let next_id = self.accounts.keys().map(|a| a.0).max().unwrap_or(0) + 1;
            let id = AccountId(next_id);
            let mut acc = Account::new(id, kind, self.setup.npcs.cash_per_npc);
            if let Some(s) =
                StrategyFactory::build(kind, &self.setup.strategy_params, &mut self.rng)?
            {
                acc.set_strategy(s);
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
        } else {
            TradingPhase::Continuous
        }
    }

    /// 把可配置的 15 分钟开盘窗口按 10:5 映射为申报期和盘前期。
    fn auction_entry_ticks(&self) -> u64 {
        self.setup.auction_ticks - self.setup.auction_ticks / 3
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

    /// 完整状态快照（首次连/重连/存档）。
    ///
    /// 遍历 markets/accounts 取只读值快照：market 的 last_price/last_close/best_bid/
    /// best_ask/fundamental_value；account 的 cash + positions（qty/t1_locked/
    /// invested_cents/recovered_cents）。snapshot 自身只读、不影响 session 状态。
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_inner(true, false)
    }

    /// 高频运行快照：刷新报价、账户和昨收，但不复制历史 K 线。
    /// 完整 K 线只在首次连接、重连和读档时通过 [`Self::snapshot`] 同步。
    pub fn runtime_snapshot(&self) -> Snapshot {
        self.snapshot_inner(false, false)
    }

    fn snapshot_inner(
        &self,
        include_daily_candles: bool,
        include_fundamental_value: bool,
    ) -> Snapshot {
        let mut reserved_sell_qty: BTreeMap<AccountId, BTreeMap<StockCode, u32>> = BTreeMap::new();
        let mut reserved_buy_cash: BTreeMap<AccountId, Money> = BTreeMap::new();
        let mut record_reserved_sell = |owner: AccountId, code: &StockCode, qty: u32| {
            let reserved = reserved_sell_qty
                .entry(owner)
                .or_default()
                .entry(code.clone())
                .or_default();
            *reserved = reserved
                .checked_add(qty)
                .expect("validated live sell reservations cannot exceed u32 holdings");
        };
        let mut record_reserved_buy =
            |owner: AccountId, price: Money, qty: u32, filled_value: Money| {
                let required = buy_order_reservation(&self.setup.config, price, qty, filled_value)
                    .expect("validated live buy reservation must fit Money");
                let reserved = reserved_buy_cash.entry(owner).or_default();
                *reserved = reserved
                    .add(required)
                    .expect("validated live buy reservations cannot exceed account cash");
            };
        for (code, market) in &self.markets {
            for order in market.resting_orders() {
                match order.side {
                    Side::Buy => {
                        record_reserved_buy(order.owner, order.price, order.qty, order.filled_value)
                    }
                    Side::Sell => record_reserved_sell(order.owner, code, order.qty),
                }
            }
        }
        for (code, orders) in &self.auction_orders {
            for order in orders {
                match order.side {
                    Side::Buy => {
                        record_reserved_buy(order.owner, order.limit, order.qty, Money::ZERO)
                    }
                    Side::Sell => record_reserved_sell(order.owner, code, order.qty),
                }
            }
        }
        let markets = self
            .markets
            .iter()
            .map(|(code, m)| {
                (
                    code.clone(),
                    MarketSnap {
                        last_price: m.last_price(),
                        last_close: m.last_close(),
                        best_bid: m.best_bid(),
                        best_ask: m.best_ask(),
                        fundamental_value: include_fundamental_value.then(|| m.fundamental_value()),
                        bids: m.bid_depth(),
                        asks: m.ask_depth(),
                    },
                )
            })
            .collect();
        let accounts = self
            .accounts
            .iter()
            .map(|(id, a)| {
                (
                    *id,
                    AccountSnap {
                        cash: a.cash,
                        positions: a
                            .positions
                            .iter()
                            .map(|(c, p)| {
                                (
                                    c.clone(),
                                    PositionSnap {
                                        qty: p.qty,
                                        t1_locked: p.t1_locked,
                                        invested_cents: p.invested_cents,
                                        recovered_cents: p.recovered_cents,
                                    },
                                )
                            })
                            .collect(),
                        reserved_buy_cash: reserved_buy_cash.remove(id).unwrap_or(Money::ZERO),
                        reserved_sell_qty: reserved_sell_qty.remove(id).unwrap_or_default(),
                    },
                )
            })
            .collect();
        Snapshot {
            seq: self.seq,
            tick: self.tick,
            day: self.day,
            phase: self.phase(),
            markets,
            accounts,
            daily_candles: if include_daily_candles {
                self.daily_candles.clone()
            } else {
                BTreeMap::new()
            },
            active_daily_candles: if include_daily_candles {
                self.active_daily_candles.clone()
            } else {
                BTreeMap::new()
            },
        }
    }

    /// 构建市场视图。`see_v=true` 填隐藏公允价 V（机构），否则 None（散户/游资/玩家）。
    ///
    /// 遍历所有 markets，每股取 best_bid/best_ask/last_price，并把对应 `price_history`
    /// 队列拷为 `recent_prices`。产 owned [`MarketView`]（不持 `&self` 借用），便于随后
    /// 安全地 `self.accounts.get_mut`。
    fn build_market_view(&self, see_v: bool) -> MarketView {
        let mut stocks = BTreeMap::new();
        for (code, m) in &self.markets {
            let hist: Vec<Money> = self
                .price_history
                .get(code)
                .map(|d| d.iter().copied().collect())
                .expect("every market must have a price-history queue");
            stocks.insert(
                code.clone(),
                StockView {
                    best_bid: m.best_bid(),
                    best_ask: m.best_ask(),
                    last_price: m.last_price(),
                    fundamental_value: if see_v {
                        Some(m.fundamental_value())
                    } else {
                        None
                    },
                    recent_prices: hist,
                },
            );
        }
        MarketView { stocks }
    }

    /// 构建账户自身视图：现金 + 每只持仓的 [`PositionView`]。
    ///
    /// `sellable` 取 `Position::sellable()`（持仓 − T+1 锁定）；`cost_price` 取派生成本价。
    /// 同样产 owned [`SelfView`]（账户不存在时返回空视图，调用方仅对已知 id 取）。
    fn build_self_view(&self, id: AccountId) -> SelfView {
        let a = match self.accounts.get(&id) {
            Some(a) => a,
            None => {
                return SelfView {
                    cash: Money::ZERO,
                    positions: BTreeMap::new(),
                }
            }
        };
        let positions = a
            .positions
            .iter()
            .map(|(c, p)| {
                (
                    c.clone(),
                    PositionView {
                        qty: p.qty,
                        sellable_qty: p.sellable(),
                        cost_price: p.cost_price(),
                    },
                )
            })
            .collect();
        SelfView {
            cash: a.cash,
            positions,
        }
    }

    /// 推进一个 tick：决策 → 预校验路由 → 结算 → V 演化 → 价格历史 → 日界。
    ///
    /// 返回带单调 seq 的增量事件 [`Vec<Event>`]；**单项失败进事件流（[`Event::IntentRejected`]/
    /// [`Event::SettlementError`]/[`Event::VError`]），绝不中断循环、绝不静默丢弃意图**（铁律二）。
    ///
    /// 顺序：
    /// 1. 收集 Intent：NPC 按 [`AccountId`] 升序遍历（BTreeMap keys 天然有序），每 NPC 建
    ///    视图（机构 `see_v=true`）→ `strategy.decide`；再追加玩家队列 `pending_player`（取走清空）。
    /// 2. 逐 [`Self::route_intent`]：预校验资金/持仓/涨跌停/未知股票 → 撮合 → 结算每笔成交。
    /// 3. V 演化：每股 `Market::evolve_v`（单一 RNG 源），失败 → [`Event::VError`]。
    /// 4. `tick += 1`；每股 push 价格历史（trim 到 `history_len`）+ 产 [`Event::PriceTick`]。
    /// 5. `tick % ticks_per_day == 0` → 每股 `Market::end_of_day`、`day += 1`、产 [`Event::DayBoundary`]。
    pub fn step(&mut self) -> Vec<Event> {
        let mut events: Vec<Event> = Vec::new();
        let phase = self.phase();

        // 1. 收集 Intent：NPC 并行 decide（rayon）+ 玩家队列串行追加。
        let npc_ids: Vec<AccountId> = self
            .accounts
            .keys()
            .copied()
            .filter(|id| id.0 != 0)
            .collect();

        // 构建只读视图（不借 &mut self，可在并行闭包中用）。
        // 机构看 V、其它不看。
        let market_view_with_v = self.build_market_view(true);
        let market_view_no_v = self.build_market_view(false);

        // 临时取出 NPC 策略（Box<dyn Strategy + Send + Sync>），并行 decide 后放回。
        let mut strategies: Vec<(AccountId, Box<dyn crate::strategy::Strategy>)> = Vec::new();
        for &id in &npc_ids {
            if let Some(acc) = self.accounts.get_mut(&id) {
                if let Some(s) = acc.strategy.take() {
                    strategies.push((id, s));
                }
            }
        }

        // 并行 decide：每个 NPC 独立种子 RNG（seed ^ tick ^ npc_id → 确定性）。
        let tick = self.tick;
        let seed = self.seed;
        let results: Vec<(AccountId, Vec<Intent>)> = strategies
            .par_iter_mut()
            .map(|(id, strat)| {
                let see_v = self
                    .accounts
                    .get(id)
                    .map(|a| a.kind == AccountKind::Inst)
                    .unwrap_or(false);
                let mv = if see_v {
                    &market_view_with_v
                } else {
                    &market_view_no_v
                };
                let sv = self.build_self_view(*id);
                // 每NPC确定性 RNG：seed ^ (tick * 0x9E3779B97F4A7C15) ^ (id * 0x6A09E667F3BCC908)
                let npc_seed = seed
                    ^ tick.wrapping_mul(0x9E3779B97F4A7C15)
                    ^ (id.0).wrapping_mul(0x6A09E667F3BCC908);
                let mut npc_rng = SplitMix64::new(npc_seed);
                (*id, strat.decide(mv, &sv, &mut npc_rng))
            })
            .collect();

        // 放回策略。
        for (id, strat) in strategies {
            if let Some(acc) = self.accounts.get_mut(&id) {
                acc.strategy = Some(strat);
            }
        }

        // 按AccountId升序排列意图（确定性：同种子同输出）。
        let mut pending: Vec<(AccountId, Intent)> = Vec::new();
        let mut sorted = results;
        sorted.sort_by_key(|(id, _)| *id);
        for (id, intents) in sorted {
            for it in intents {
                pending.push((id, it));
            }
        }
        pending.extend(std::mem::take(&mut self.pending_player));

        // 2. 预校验 + 路由。集合竞价阶段只积累限价委托，不提前成交。
        for (acct, intent) in pending {
            match phase {
                TradingPhase::CallAuction => {
                    self.route_auction_intent(acct, intent, &mut events);
                }
                TradingPhase::PreOpen => {
                    let (code, reason) = match intent {
                        Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } => {
                            (code, RejectionReason::AuctionOrderEntryClosed)
                        }
                        Intent::Cancel { code, .. } => {
                            (code, RejectionReason::AuctionOrderNotCancelable)
                        }
                    };
                    events.push(Event::IntentRejected {
                        seq: self.next_seq(),
                        account: acct,
                        code,
                        reason,
                    });
                }
                TradingPhase::Continuous => self.route_intent(acct, intent, &mut events),
            }
        }

        // 3. V 演化（并行：各股独立、各自确定性种子 RNG）。
        let base_v_params = self.setup.v_params.clone();
        let long_run_means: BTreeMap<StockCode, Money> = self
            .setup
            .stocks
            .iter()
            .map(|stock| {
                (
                    stock.code.clone(),
                    self.setup
                        .fundamental_value_means
                        .get(&stock.code)
                        .copied()
                        .unwrap_or(stock.v_initial),
                )
            })
            .collect();
        let codes: Vec<StockCode> = self.markets.keys().cloned().collect();
        // 收集需要演化的 markets 的可变引用（通过 unsafe 拆分 BTreeMap 借用）。
        // 安全：各 Market 互不引用，par_iter_mut 不冲突。
        // 但 BTreeMap 没有 par_iter_mut → 转 Vec 拆分。
        let mut market_list: Vec<(&StockCode, &mut Market)> = self.markets.iter_mut().collect();
        let v_results: Vec<(StockCode, Result<(), String>)> = market_list
            .par_iter_mut()
            .map(|(code, market)| {
                let mut params = base_v_params.clone();
                params.long_run_mean = *long_run_means
                    .get(*code)
                    .expect("every market must have a stock-specific long-run mean");
                let m_seed = seed ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ stock_code_hash(code);
                let mut m_rng = SplitMix64::new(m_seed);
                (
                    (*code).clone(),
                    market
                        .evolve_v(&params, &mut m_rng)
                        .map_err(|error| error.to_string()),
                )
            })
            .collect();
        for (code, result) in v_results {
            if let Err(reason) = result {
                events.push(Event::VError {
                    seq: self.next_seq(),
                    code,
                    reason,
                });
            }
        }

        // 4. tick 自增。集合竞价申报期发 AuctionTick 并在 09:25 一次撮合；
        // 09:25–09:30 为 PreOpen 静默窗口；连续竞价发 PriceTick。
        self.tick += 1;
        if phase == TradingPhase::CallAuction {
            let final_auction_tick =
                self.tick % self.setup.ticks_per_day == self.auction_entry_ticks();
            for code in &codes {
                let previous_close = self
                    .markets
                    .get(code)
                    .expect("code collected from markets must exist")
                    .last_close();
                let stock = self
                    .setup
                    .stocks
                    .iter()
                    .find(|stock| stock.code == *code)
                    .expect("code collected from markets must have a stock spec");
                let result = clearing_result(
                    self.auction_orders
                        .get(code)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    previous_close,
                    stock.exchange,
                    stock.tick,
                );
                events.push(Event::AuctionTick {
                    seq: self.next_seq(),
                    tick: self.tick,
                    code: code.clone(),
                    indicative_price: result.map(|r| r.price),
                    matched_volume: result.map_or(0, |r| r.volume),
                    imbalance: result.map_or_else(
                        || {
                            auction_total_imbalance(
                                self.auction_orders
                                    .get(code)
                                    .map(Vec::as_slice)
                                    .unwrap_or(&[]),
                            )
                        },
                        |r| r.imbalance,
                    ),
                });
            }
            if final_auction_tick {
                for code in &codes {
                    self.complete_auction(code, &mut events);
                }
                self.auction_orders.clear();
            }
        } else if phase == TradingPhase::Continuous {
            for code in &codes {
                let (last, bids, asks) = self
                    .markets
                    .get(code)
                    .map(|m| {
                        (
                            m.last_price(),
                            m.bid_depth_limited(5),
                            m.ask_depth_limited(5),
                        )
                    })
                    .unwrap_or((Money::ZERO, Vec::new(), Vec::new()));
                if let Some(h) = self.price_history.get_mut(code) {
                    h.push_back(last);
                    while h.len() > self.setup.history_len {
                        h.pop_front();
                    }
                }
                self.update_active_daily_candle(code, last, 0);
                let daily_candle = self
                    .active_daily_candles
                    .get(code)
                    .expect("active daily candle must exist after price update")
                    .clone();
                events.push(Event::PriceTick {
                    seq: self.next_seq(),
                    tick: self.tick,
                    code: code.clone(),
                    last_price: last,
                    daily_candle,
                    bids,
                    asks,
                });
            }
        }

        // 5. 日界：到 ticks_per_day → end_of_day + day+1 + DayBoundary。
        if self.setup.ticks_per_day > 0 && self.tick.is_multiple_of(self.setup.ticks_per_day) {
            for code in &codes {
                if let Some(m) = self.markets.get_mut(code) {
                    m.end_of_day();
                }
            }
            if self.setup.t1_enabled {
                for account in self.accounts.values_mut() {
                    account.unlock_t1_positions();
                }
            }
            let closed_daily_candles = self.commit_active_daily_candles();
            self.day += 1;
            events.push(Event::DayBoundary {
                seq: self.next_seq(),
                day: self.day,
                closed_daily_candles,
            });
        }

        events
    }

    fn prevalidate_order(
        &mut self,
        order: OrderValidationInput<'_>,
        events: &mut Vec<Event>,
    ) -> bool {
        let OrderValidationInput {
            account: acct,
            code,
            side,
            price,
            qty,
            is_market,
        } = order;
        let lot_size = self.setup.config.lot_size;
        let stock = self
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .expect("prevalidation only runs for configured markets");
        let max_order_qty = stock.category.max_order_qty(is_market);

        let available_sell = if side == Side::Sell {
            let reserved_sell_qty = self
                .markets
                .get(code)
                .expect("prevalidation only runs for configured markets")
                .resting_orders_for(acct)
                .into_iter()
                .filter(|order| order.side == Side::Sell)
                .try_fold(0_u64, |total, order| {
                    total.checked_add(u64::from(order.qty))
                });
            let auction_sell_qty = self
                .auction_orders
                .get(code)
                .into_iter()
                .flatten()
                .filter(|order| order.owner == acct && order.side == Side::Sell)
                .try_fold(0_u64, |total, order| {
                    total.checked_add(u64::from(order.qty))
                });
            let total_sellable = self
                .accounts
                .get(&acct)
                .map(|account| account.sellable_qty(code))
                .unwrap_or(0);
            let available = reserved_sell_qty
                .zip(auction_sell_qty)
                .and_then(|(continuous, auction)| continuous.checked_add(auction))
                .and_then(|reserved| u64::from(total_sellable).checked_sub(reserved));
            match available {
                Some(available) => available,
                None => {
                    events.push(Event::SettlementError {
                        seq: self.next_seq(),
                        account: acct,
                        code: code.clone(),
                        reason: "卖出挂单预留量溢出或超过可卖持仓；拒绝继续处理委托".to_string(),
                    });
                    return false;
                }
            }
        } else {
            0
        };

        if side == Side::Sell && u64::from(qty) > available_sell {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code: code.clone(),
                reason: RejectionReason::InsufficientShares,
            });
            return false;
        }
        let quantity_valid = qty > 0
            && qty <= max_order_qty
            && match side {
                Side::Buy => qty.is_multiple_of(lot_size),
                Side::Sell => {
                    qty.is_multiple_of(lot_size)
                        || u64::from(qty % lot_size) == available_sell % u64::from(lot_size)
                }
            };
        if !quantity_valid {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code: code.clone(),
                reason: RejectionReason::InvalidQuantity,
            });
            return false;
        }

        if self.phase() == TradingPhase::Continuous && !is_market {
            let bound = match self
                .markets
                .get(code)
                .expect("prevalidation only runs for configured markets")
                .continuous_limit_bound(side)
            {
                Ok(bound) => bound,
                Err(error) => {
                    events.push(Event::SettlementError {
                        seq: self.next_seq(),
                        account: acct,
                        code: code.clone(),
                        reason: error.to_string(),
                    });
                    return false;
                }
            };
            let outside_cage = match side {
                Side::Buy => price > bound,
                Side::Sell => price < bound,
            };
            if outside_cage {
                events.push(Event::IntentRejected {
                    seq: self.next_seq(),
                    account: acct,
                    code: code.clone(),
                    reason: RejectionReason::PriceCageExceeded,
                });
                return false;
            }
        }

        let rejection = match side {
            Side::Buy => {
                let auction_reserved = self
                    .auction_orders
                    .values()
                    .flatten()
                    .filter(|order| order.owner == acct && order.side == Side::Buy)
                    .try_fold(Money::ZERO, |total, order| {
                        total.add(buy_order_reservation(
                            &self.setup.config,
                            order.limit,
                            order.qty,
                            Money::ZERO,
                        )?)
                    });
                let continuous_reserved = self
                    .markets
                    .values()
                    .flat_map(|market| market.resting_orders_for(acct))
                    .filter(|order| order.side == Side::Buy)
                    .try_fold(Money::ZERO, |total, order| {
                        total.add(buy_order_reservation(
                            &self.setup.config,
                            order.price,
                            order.qty,
                            order.filled_value,
                        )?)
                    });
                let total = auction_reserved
                    .and_then(|reserved| reserved.add(continuous_reserved?))
                    .and_then(|reserved| {
                        reserved.add(buy_order_reservation(
                            &self.setup.config,
                            price,
                            qty,
                            Money::ZERO,
                        )?)
                    });
                let cash = self
                    .accounts
                    .get(&acct)
                    .map(|a| a.cash)
                    .expect("intent routing only accepts existing accounts");
                match total {
                    Ok(total) => (total > cash).then_some(RejectionReason::InsufficientCash),
                    Err(error) => {
                        events.push(Event::SettlementError {
                            seq: self.next_seq(),
                            account: acct,
                            code: code.clone(),
                            reason: error.to_string(),
                        });
                        return false;
                    }
                }
            }
            Side::Sell => None,
        };
        if let Some(reason) = rejection {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code: code.clone(),
                reason,
            });
            false
        } else {
            true
        }
    }

    /// 预校验 + 路由单个 Intent。不可行 → [`Event::IntentRejected`]（不静默丢弃，铁律二）；
    /// 可行 → 构造唯一 id 的 [`Order`] → `Market::place` → 逐笔成交结算 → 产 [`Event::Trade`]。
    ///
    /// 借用分阶段（合法）：先 `self.markets.get_mut(&code).place(order)` 拿 owned `Vec<Trade>`，
    /// 再 `self.settle`（借 `&mut accounts`）——两阶段不重叠。
    ///
    /// `code` 来自本 Intent（orderbook 的 [`Trade`] **无 code 字段**，需路由侧注入）。
    /// taker = 本订单方（`side`），maker = 反向（被动挂单方）。
    fn route_intent(&mut self, acct: AccountId, intent: Intent, events: &mut Vec<Event>) {
        if let Intent::Cancel { code, id } = &intent {
            self.cancel_continuous_order(acct, code.clone(), *id, events);
            return;
        }

        let (code, side, price, qty, is_market) = match &intent {
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } => (code.clone(), *side, *price, *qty, false),
            Intent::PlaceMarket { code, side, qty } => {
                let protective_price = self.markets.get(code).map(|market| match side {
                    Side::Buy => market.up_stop(),
                    Side::Sell => market.down_stop(),
                });
                let protective_price = match protective_price.transpose() {
                    Ok(Some(price)) => price,
                    Ok(None) => Money::ZERO,
                    Err(error) => {
                        events.push(Event::SettlementError {
                            seq: self.next_seq(),
                            account: acct,
                            code: code.clone(),
                            reason: error.to_string(),
                        });
                        return;
                    }
                };
                (code.clone(), *side, protective_price, *qty, true)
            }
            Intent::Cancel { .. } => unreachable!("cancel handled above"),
        };
        // 未知股票：显式拒绝，不静默忽略。
        if !self.markets.contains_key(&code) {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::UnknownStock,
            });
            return;
        }
        if !self.prevalidate_order(
            OrderValidationInput {
                account: acct,
                code: &code,
                side,
                price,
                qty,
                is_market,
            },
            events,
        ) {
            return;
        }

        // 构造 Order（唯一 id）并撮合。
        let oid = OrderId(self.next_order_id);
        self.next_order_id += 1;
        let order = Order {
            id: oid,
            side,
            price,
            qty,
            original_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: acct,
            seq: 0,
        };
        // 先在市场副本上撮合；只有双边账务全部结算成功才提交订单簿和最新价。
        let mut candidate_market = self
            .markets
            .get(&code)
            .expect("market existence checked above")
            .clone();
        let match_result = match candidate_market.place(order) {
            Ok(result) => result,
            Err(MarketError::LimitExceeded { .. }) => {
                events.push(Event::IntentRejected {
                    seq: self.next_seq(),
                    account: acct,
                    code: code.clone(),
                    reason: RejectionReason::LimitExceeded,
                });
                return;
            }
            Err(e) => {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: acct,
                    code: code.clone(),
                    reason: e.to_string(),
                });
                return;
            }
        };
        let resting = match_result.resting;
        if is_market && resting.is_some() {
            candidate_market
                .cancel(oid)
                .expect("market order remainder was just inserted into candidate book");
        }

        let trades = match_result.trades;
        // 逐笔结算。maker 反向 side、taker 本 side。code 用路由的 code（Trade 无 code 字段）。
        let mut account_backups: BTreeMap<AccountId, (Money, BTreeMap<StockCode, Position>)> =
            BTreeMap::new();
        for trade in &trades {
            for participant in [trade.maker, trade.taker] {
                if let Some(account) = self.accounts.get(&participant) {
                    account_backups
                        .entry(participant)
                        .or_insert_with(|| (account.cash, account.positions.clone()));
                }
            }
        }

        let mut order_fills = Vec::with_capacity(trades.len() * 2);
        for trade in &trades {
            let gross = match trade.price.mul_shares(trade.qty) {
                Ok(gross) => gross,
                Err(error) => {
                    events.push(Event::SettlementError {
                        seq: self.next_seq(),
                        account: acct,
                        code,
                        reason: error.to_string(),
                    });
                    return;
                }
            };
            let (buyer, buyer_order_id, buyer_before, seller, seller_order_id, seller_before) =
                if side == Side::Buy {
                    (
                        trade.taker,
                        trade.taker_order_id,
                        trade.taker_filled_value_before,
                        trade.maker,
                        trade.maker_order_id,
                        trade.maker_filled_value_before,
                    )
                } else {
                    (
                        trade.maker,
                        trade.maker_order_id,
                        trade.maker_filled_value_before,
                        trade.taker,
                        trade.taker_order_id,
                        trade.taker_filled_value_before,
                    )
                };
            order_fills.push(OrderFillSettlement {
                account: buyer,
                side: Side::Buy,
                order_id: buyer_order_id,
                filled_value_before: buyer_before,
                gross,
                qty: trade.qty,
            });
            order_fills.push(OrderFillSettlement {
                account: seller,
                side: Side::Sell,
                order_id: seller_order_id,
                filled_value_before: seller_before,
                gross,
                qty: trade.qty,
            });
        }
        let mut settlement_failure = self.settle_order_fills(&code, &order_fills);
        if settlement_failure.is_none() {
            if let Err((account, reason)) =
                self.validate_live_reservations_with(&code, &candidate_market)
            {
                settlement_failure = Some((account, AccountError::ReservationInvariant { reason }));
            }
        }
        if let Some((failed_account, error)) = settlement_failure {
            for (id, (cash, positions)) in account_backups {
                if let Some(account) = self.accounts.get_mut(&id) {
                    account.cash = cash;
                    account.positions = positions;
                }
            }
            events.push(Event::SettlementError {
                seq: self.next_seq(),
                account: failed_account,
                code,
                reason: error.to_string(),
            });
            return;
        }

        self.markets.insert(code.clone(), candidate_market);
        if !is_market {
            if let Some(resting_order) = resting {
                events.push(Event::OrderAccepted {
                    seq: self.next_seq(),
                    account: acct,
                    code: code.clone(),
                    id: resting_order.id,
                    side: resting_order.side,
                    price: resting_order.price,
                    remaining_qty: resting_order.qty,
                });
            }
        }
        for t in trades {
            let (maker_id, taker_id) = (t.maker, t.taker);
            self.update_active_daily_candle(&code, t.price, u64::from(t.qty));
            events.push(Event::Trade {
                seq: self.next_seq(),
                code: code.clone(),
                price: t.price,
                qty: t.qty,
                maker: maker_id,
                taker: taker_id,
            });
        }
    }

    fn cancel_continuous_order(
        &mut self,
        acct: AccountId,
        code: StockCode,
        id: OrderId,
        events: &mut Vec<Event>,
    ) {
        let Some(market) = self.markets.get(&code) else {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::UnknownStock,
            });
            return;
        };
        let mut candidate_market = market.clone();
        match candidate_market.cancel(id) {
            Ok(order) if order.owner == acct => {
                self.markets.insert(code.clone(), candidate_market);
                events.push(Event::OrderCanceled {
                    seq: self.next_seq(),
                    account: acct,
                    code,
                    id,
                    remaining_qty: order.qty,
                });
            }
            Ok(_) => events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::NotOrderOwner,
            }),
            Err(MarketError::OrderBook(OrderError::OrderNotFound(_))) => {
                events.push(Event::IntentRejected {
                    seq: self.next_seq(),
                    account: acct,
                    code,
                    reason: RejectionReason::OrderNotFound,
                });
            }
            Err(error) => events.push(Event::SettlementError {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: error.to_string(),
            }),
        }
    }

    /// 按委托累计成交额结算：同一委托跨多次 fill 只补收累计费用的差额。
    fn settle_order_fills(
        &mut self,
        code: &StockCode,
        fills: &[OrderFillSettlement],
    ) -> Option<(AccountId, AccountError)> {
        let cfg = self.setup.config.clone();
        let mut by_order: BTreeMap<OrderId, OrderFillSettlement> = BTreeMap::new();
        for fill in fills {
            match by_order.entry(fill.order_id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(*fill);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let aggregate = entry.get_mut();
                    if aggregate.account != fill.account || aggregate.side != fill.side {
                        return Some((
                            fill.account,
                            AccountError::ReservationInvariant {
                                reason: format!(
                                    "order {:?} changed owner or side while settling",
                                    fill.order_id
                                ),
                            },
                        ));
                    }
                    aggregate.gross = match aggregate.gross.add(fill.gross) {
                        Ok(gross) => gross,
                        Err(error) => return Some((fill.account, error.into())),
                    };
                    aggregate.qty = match aggregate.qty.checked_add(fill.qty) {
                        Some(qty) => qty,
                        None => {
                            return Some((
                                fill.account,
                                MoneyError::Overflow {
                                    op: "order_fill_qty_add",
                                    operand: format!("{} + {}", aggregate.qty, fill.qty),
                                }
                                .into(),
                            ));
                        }
                    };
                }
            }
        }

        let mut buy_totals: BTreeMap<AccountId, SettlementTotals> = BTreeMap::new();
        let mut sell_totals: BTreeMap<AccountId, SettlementTotals> = BTreeMap::new();
        for fill in by_order.values() {
            let after = match fill.filled_value_before.add(fill.gross) {
                Ok(after) => after,
                Err(error) => return Some((fill.account, error.into())),
            };
            let commission = match fee_delta(fill.filled_value_before, after, |amount| {
                cfg.commission(amount)
            }) {
                Ok(value) => value,
                Err(error) => return Some((fill.account, error.into())),
            };
            let transfer_fee = match fee_delta(fill.filled_value_before, after, |amount| {
                cfg.transfer_fee(amount)
            }) {
                Ok(value) => value,
                Err(error) => return Some((fill.account, error.into())),
            };
            let stamp_tax = if fill.side == Side::Sell {
                match fee_delta(fill.filled_value_before, after, |amount| {
                    cfg.stamp_tax(amount)
                }) {
                    Ok(value) => value,
                    Err(error) => return Some((fill.account, error.into())),
                }
            } else {
                Money::ZERO
            };
            let totals = match fill.side {
                Side::Buy => buy_totals.entry(fill.account).or_default(),
                Side::Sell => sell_totals.entry(fill.account).or_default(),
            };
            totals.gross = match totals.gross.add(fill.gross) {
                Ok(value) => value,
                Err(error) => return Some((fill.account, error.into())),
            };
            totals.commission = match totals.commission.add(commission) {
                Ok(value) => value,
                Err(error) => return Some((fill.account, error.into())),
            };
            totals.stamp_tax = match totals.stamp_tax.add(stamp_tax) {
                Ok(value) => value,
                Err(error) => return Some((fill.account, error.into())),
            };
            totals.transfer_fee = match totals.transfer_fee.add(transfer_fee) {
                Ok(value) => value,
                Err(error) => return Some((fill.account, error.into())),
            };
            totals.qty = match totals.qty.checked_add(fill.qty) {
                Some(qty) => qty,
                None => {
                    return Some((
                        fill.account,
                        MoneyError::Overflow {
                            op: "account_settlement_qty_add",
                            operand: format!("{} + {}", totals.qty, fill.qty),
                        }
                        .into(),
                    ));
                }
            };
        }

        for (id, totals) in buy_totals {
            let Some(account) = self.accounts.get_mut(&id) else {
                return Some((id, AccountError::NoPosition(code.clone())));
            };
            if let Err(error) =
                account.apply_settlement(Side::Buy, code.clone(), totals, self.setup.t1_enabled)
            {
                return Some((id, error));
            }
        }
        for (id, totals) in sell_totals {
            let Some(account) = self.accounts.get_mut(&id) else {
                return Some((id, AccountError::NoPosition(code.clone())));
            };
            if let Err(error) =
                account.apply_settlement(Side::Sell, code.clone(), totals, self.setup.t1_enabled)
            {
                return Some((id, error));
            }
        }
        None
    }

    fn validate_live_reservations_with(
        &self,
        replacement_code: &StockCode,
        replacement_market: &Market,
    ) -> Result<(), (AccountId, String)> {
        let mut buy_totals: BTreeMap<AccountId, Money> = BTreeMap::new();
        let mut sell_totals: BTreeMap<(AccountId, StockCode), u64> = BTreeMap::new();
        let mut record = |code: &StockCode, order: &Order| -> Result<(), (AccountId, String)> {
            match order.side {
                Side::Buy => {
                    let required = buy_order_reservation(
                        &self.setup.config,
                        order.price,
                        order.qty,
                        order.filled_value,
                    )
                    .map_err(|error| (order.owner, error.to_string()))?;
                    let current = buy_totals.entry(order.owner).or_insert(Money::ZERO);
                    *current = current
                        .add(required)
                        .map_err(|error| (order.owner, error.to_string()))?;
                }
                Side::Sell => {
                    let current = sell_totals.entry((order.owner, code.clone())).or_default();
                    *current = current.checked_add(u64::from(order.qty)).ok_or_else(|| {
                        (
                            order.owner,
                            "sell reservation quantity overflow".to_string(),
                        )
                    })?;
                }
            }
            Ok(())
        };
        for (code, market) in &self.markets {
            let orders = if code == replacement_code {
                replacement_market.resting_orders()
            } else {
                market.resting_orders()
            };
            for order in &orders {
                record(code, order)?;
            }
        }
        for (code, orders) in &self.auction_orders {
            if code == replacement_code {
                continue;
            }
            for order in orders {
                let resting = Order {
                    id: OrderId(order.arrival_seq),
                    side: order.side,
                    price: order.limit,
                    qty: order.qty,
                    original_qty: order.qty,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                    owner: order.owner,
                    seq: order.arrival_seq,
                };
                record(code, &resting)?;
            }
        }
        for (owner, reserved) in buy_totals {
            let cash = self
                .accounts
                .get(&owner)
                .map(|account| account.cash)
                .ok_or_else(|| (owner, "buy reservation owner is missing".to_string()))?;
            if reserved > cash {
                return Err((
                    owner,
                    "remaining buy orders exceed available cash".to_string(),
                ));
            }
        }
        for ((owner, code), reserved) in sell_totals {
            let sellable = self
                .accounts
                .get(&owner)
                .map(|account| account.sellable_qty(&code))
                .ok_or_else(|| (owner, "sell reservation owner is missing".to_string()))?;
            if reserved > u64::from(sellable) {
                return Err((
                    owner,
                    "remaining sell orders exceed sellable shares".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// 玩家意图入队（随时可调）；在下个 [`Self::step`] 开头才执行（玩家意图统一在 NPC 之后路由）。
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
    pub fn save(&self) -> SaveSlot {
        SaveSlot {
            schema_version: SAVE_SCHEMA_VERSION,
            setup: self.setup.clone(),
            seed: self.seed,
            snapshot: self.snapshot_inner(true, true),
            auction_orders: self.auction_orders.clone(),
            resting_orders: self
                .markets
                .iter()
                .map(|(code, market)| (code.clone(), market.resting_orders()))
                .collect(),
            price_history: self
                .price_history
                .iter()
                .map(|(code, prices)| (code.clone(), prices.iter().copied().collect()))
                .collect(),
            rng_state: Some(self.rng.state),
            next_order_id: self.next_order_id,
        }
    }

    /// 从存档恢复权威账户、市场、日 K 与两种竞价阶段的未成交委托。
    ///
    /// 流程：
    /// 1. new(setup, seed) → 新建 session（含初始持仓分配）
    /// 2. 清空所有账户持仓 → 用快照精确覆盖（cash + positions invested/recovered/t1_locked）
    /// 3. 覆盖每只股票的 last_price/last_close/fundamental_value
    /// 4. 重建连续竞价订单簿并校验深度；恢复 tick/day/seq 与集合竞价队列
    ///
    /// 不保留：前端派生的分时采样。策略价格窗口和 RNG 状态会被精确恢复。
    pub fn restore(save: &SaveSlot) -> Result<GameSession, SessionError> {
        let migrated_save = migrate_save_slot(save)?;
        let save = &migrated_save;
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

        // 恢复市场状态（last_price/last_close/V）
        for (code, snap_mkt) in &save.snapshot.markets {
            let market = sess
                .markets
                .get_mut(code)
                .expect("validated save market set exactly matches setup");
            market.set_last_price(snap_mkt.last_price);
            market.set_last_close(snap_mkt.last_close);
            market.set_fundamental_value(
                snap_mkt
                    .fundamental_value
                    .expect("validated save contains every fundamental value"),
            );
        }

        // 按原到达序重建订单簿。有效静态订单簿不应自行成交；若发生，说明存档互相交叉。
        for (code, saved_orders) in &save.resting_orders {
            let market = sess
                .markets
                .get_mut(code)
                .expect("validated save resting-order market must exist");
            let mut ordered = saved_orders.clone();
            ordered.sort_by_key(|order| order.seq);
            for mut order in ordered {
                if order.original_qty == 0
                    && order.filled_qty == 0
                    && order.filled_value == Money::ZERO
                {
                    order.original_qty = order.qty;
                }
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
        }

        // 恢复进度
        sess.tick = save.snapshot.tick;
        sess.day = save.snapshot.day;
        sess.seq = save.snapshot.seq;
        validate_saved_order_state(&sess, save)?;
        sess.auction_orders = save.auction_orders.clone();
        sess.next_order_id = save.next_order_id;

        // 新格式精确恢复 Rust 持有的 K 线；旧存档或某只新增股票没有记录时，
        // 保留 `new` 已按相同 setup/seed 生成的 360 日基线。
        for (code, candles) in &save.snapshot.daily_candles {
            if !candles.is_empty() {
                sess.daily_candles.insert(code.clone(), candles.clone());
            }
        }
        sess.active_daily_candles = save.snapshot.active_daily_candles.clone();
        if !save.price_history.is_empty() {
            sess.price_history = save
                .price_history
                .iter()
                .map(|(code, prices)| (code.clone(), prices.iter().copied().collect()))
                .collect();
        }
        if let Some(state) = save.rng_state {
            sess.rng.state = state;
        }

        Ok(sess)
    }
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
                v_initial: Money::from_cents(1_000),
                tick: Money::from_cents(1),
                float_shares: 0,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 0,
                hot_count: 0,
                cash_per_npc: Money::ZERO,
            },
            config: GameConfig::proposed_defaults(),
            v_params: VParams {
                long_run_mean: Money::from_cents(1_000),
                mean_reversion: 0.0,
                volatility: 0.0,
            },
            fundamental_value_means: Default::default(),
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
            history_len: 10,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
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
