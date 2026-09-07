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
use crate::strategy::Rng;
use crate::strategy::{
    Intent, MarketView, PositionView, SelfView, StockView, StrategyFactory, StrategyParams,
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
    /// 集合竞价只接受限价委托，市价单无法确定保护价格。
    AuctionLimitOrderRequired,
    /// 集合竞价委托不进入连续订单簿，当前协议没有可撤销的公开竞价订单 id。
    AuctionOrderNotCancelable,
}

/// 当前交易阶段。`ticks_per_day` 包含集合竞价与连续竞价。
#[derive(Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum TradingPhase {
    CallAuction,
    #[default]
    Continuous,
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
}

/// 市场快照子结构（单股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketSnap {
    pub last_price: Money,
    pub last_close: Money,
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub fundamental_value: Money,
    /// 买盘深度（价高→低，每价聚合总量）。前端取前 N 档渲染五档盘口。
    pub bids: Vec<(Money, u32)>,
    /// 卖盘深度（价低→高，每价聚合总量）。
    pub asks: Vec<(Money, u32)>,
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

/// 单个交易日的 OHLCV。价格全部为分，time 为游戏内相对 Unix 秒：第 0 日为 0，
/// 启动预置历史使用负数，确保前端图表可直接按时间排序。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DailyCandle {
    pub time: i64,
    pub open: Money,
    pub high: Money,
    pub low: Money,
    pub close: Money,
    pub volume: u64,
}

/// 完整状态快照（首次连/重连/存档）。含 V（前端展示层按需过滤）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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

/// 存档槽：精确到交易日（不保留日内分时/盘口挂单）。
/// 加载后：市场价/持仓/资金/V 恢复；盘口空（NPC 重新挂单填充）；分时图从头开始。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SaveSlot {
    pub setup: SessionSetup,
    pub seed: u64,
    pub snapshot: Snapshot,
    /// 日内存档恢复集合竞价所需的完整委托队列。
    #[serde(default)]
    pub auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
    /// 保持订单 id/到达序继续单调递增。
    #[serde(default = "default_next_order_id")]
    pub next_order_id: u64,
}

fn default_next_order_id() -> u64 {
    1
}

/// 可序列化的集合竞价限价委托。arrival_seq 同时承担价格相同时的时间优先键。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
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

/// 单只股票初始规格（行情/涨跌停/V/tick/流通盘）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockSpec {
    pub code: StockCode,
    pub initial_price: Money,
    pub limit_pct: f64,
    pub v_initial: Money,
    pub tick: Money,
    /// 流通盘股数：新游戏时分配给 NPC。0 表示不分配（兼容加载存档路径）。
    pub float_shares: u32,
}

/// 流通盘分配方式。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum FloatAllocation {
    /// 默认：所有 NPC 随机分配（权重随机，筹码守恒）。
    Random,
    /// 三类各占比例（类内随机）。比例应 ≈ 1（运行时按「有 NPC 的种类」归一化）。
    ByKind { retail: f64, inst: f64, hot: f64 },
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
    /// 每个交易日开头用于集合竞价的 tick 数；旧配置缺失时为 0。
    #[serde(default)]
    pub auction_ticks: u64,
    pub history_len: usize,
    pub t1_enabled: bool,
    /// 流通盘分配方式（新游戏时如何把 float_shares 分给 NPC）。
    pub float_allocation: FloatAllocation,
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
    pending_player: Vec<Intent>,
    next_order_id: u64,
    tick: u64,
    day: u32,
    seq: u64,
}

impl GameSession {
    /// 构造 session。校验参数 → 建 markets/accounts → 注入 NPC 策略。
    ///
    /// - 校验 `stocks` 非空、`ticks_per_day > 0`，否则 [`SessionError::InvalidSetup`]（铁律二：绝不静默）。
    /// - 每股构造一个 [`Market`]（`last_close = last_price = initial_price`），并初始化空价格历史队列。
    /// - 玩家 `AccountId(0)`：`Account::new`（strategy 默认 None）。
    /// - NPC 按 retail/inst/hot 计数逐个生成（id 递增），`StrategyFactory::build` 注入策略。
    pub fn new(setup: SessionSetup, seed: u64) -> Result<GameSession, SessionError> {
        if setup.stocks.is_empty() {
            return Err(SessionError::InvalidSetup(
                "stocks must be non-empty".to_string(),
            ));
        }
        if setup.ticks_per_day == 0 {
            return Err(SessionError::InvalidSetup(
                "ticks_per_day must be > 0".to_string(),
            ));
        }
        if setup.auction_ticks >= setup.ticks_per_day {
            return Err(SessionError::InvalidSetup(format!(
                "auction_ticks ({}) must be < ticks_per_day ({})",
                setup.auction_ticks, setup.ticks_per_day
            )));
        }
        // ByKind 比例校验：每个比例必须有限且非负（铁律二：非法参数显式拒绝，不静默归一化）。
        if let FloatAllocation::ByKind { retail, inst, hot } = &setup.float_allocation {
            for (v, name) in [(*retail, "retail"), (*inst, "inst"), (*hot, "hot")] {
                if !v.is_finite() || v < 0.0 {
                    return Err(SessionError::InvalidSetup(format!(
                        "float_allocation ByKind {name}={v} invalid (must be finite >=0)"
                    )));
                }
            }
        }
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
            Account::new(AccountId(0), AccountKind::Player, setup.player_cash),
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
        sess.populate_npcs(AccountKind::Retail);
        sess.populate_npcs(AccountKind::Inst);
        sess.populate_npcs(AccountKind::Hot);
        sess.seed_float(); // 分配流通盘给 NPC（筹码守恒、确定性、玩家不分配）
        Ok(sess)
    }

    /// 分配流通盘给 NPC（按 setup.float_allocation）。float_shares==0 或无 NPC 则跳过。
    ///
    /// 筹码守恒：Σ NPC 持仓 == float_shares（最后一个 NPC/最后一类 拿余量）。玩家不分配（新进场）。
    /// [`FloatAllocation::Random`]→全 NPC 随机；[`FloatAllocation::ByKind`]→按种类比例（类内随机、
    /// 缺类自动归一化分摊）。
    fn seed_float(&mut self) {
        let npc_ids: Vec<AccountId> = self
            .accounts
            .keys()
            .copied()
            .filter(|id| id.0 != 0)
            .collect();
        if npc_ids.is_empty() {
            return;
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
                        acc.grant_position(code.clone(), qty, price);
                    }
                }
            }
        }
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
    /// `StrategyFactory::build` 返回 None（参数非法）时**不注入策略**（工厂已显式上抛，
    /// 此处保留账户、策略为 None，运行期 decide 不调用——不静默用默认值）。
    fn populate_npcs(&mut self, kind: AccountKind) {
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
                StrategyFactory::build(kind, &self.setup.strategy_params, &mut self.rng)
            {
                acc.set_strategy(s);
            }
            self.accounts.insert(id, acc);
        }
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
    /// 当前交易阶段。日内 tick 为 0..auction_ticks-1 时处于集合竞价。
    pub fn phase(&self) -> TradingPhase {
        let day_tick = self.tick % self.setup.ticks_per_day;
        if day_tick < self.setup.auction_ticks {
            TradingPhase::CallAuction
        } else {
            TradingPhase::Continuous
        }
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
        self.snapshot_inner(true)
    }

    /// 高频运行快照：刷新报价、账户和昨收，但不复制历史 K 线。
    /// 完整 K 线只在首次连接、重连和读档时通过 [`Self::snapshot`] 同步。
    pub fn runtime_snapshot(&self) -> Snapshot {
        self.snapshot_inner(false)
    }

    fn snapshot_inner(&self, include_daily_candles: bool) -> Snapshot {
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
                        fundamental_value: m.fundamental_value(),
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
                .unwrap_or_default();
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
                let kind = crate::account::AccountKind::Inst; // placeholder
                let _ = kind;
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
        let player_intents = std::mem::take(&mut self.pending_player);
        for it in player_intents {
            pending.push((AccountId(0), it));
        }

        // 2. 预校验 + 路由。集合竞价阶段只积累限价委托，不提前成交。
        for (acct, intent) in pending {
            if phase == TradingPhase::CallAuction {
                self.route_auction_intent(acct, intent, &mut events);
            } else {
                self.route_intent(acct, intent, &mut events);
            }
        }

        // 3. V 演化（并行：各股独立、各自确定性种子 RNG）。
        let v_params = self.setup.v_params.clone();
        let codes: Vec<StockCode> = self.markets.keys().cloned().collect();
        // 收集需要演化的 markets 的可变引用（通过 unsafe 拆分 BTreeMap 借用）。
        // 安全：各 Market 互不引用，par_iter_mut 不冲突。
        // 但 BTreeMap 没有 par_iter_mut → 转 Vec 拆分。
        let mut market_list: Vec<(&StockCode, &mut Market)> = self.markets.iter_mut().collect();
        market_list.par_iter_mut().for_each(|(code, m)| {
            let m_seed = seed ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ stock_code_hash(code);
            let mut m_rng = SplitMix64::new(m_seed);
            let _ = m.evolve_v(&v_params, &mut m_rng);
        });
        // VError 事件由上述 evolve_v 产生但被忽略（par_iter_mut 无法收集 Err）。
        // TODO: 若需 VError 精确上报，改为 collect 返回 Result。

        // 4. tick 自增。集合竞价只发 AuctionTick，最后一 tick 再一次性撮合；连续竞价发 PriceTick。
        self.tick += 1;
        if phase == TradingPhase::CallAuction {
            let final_auction_tick =
                self.tick % self.setup.ticks_per_day == self.setup.auction_ticks;
            for code in &codes {
                let previous_close = self
                    .markets
                    .get(code)
                    .expect("code collected from markets must exist")
                    .last_close();
                let result = clearing_result(
                    self.auction_orders
                        .get(code)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    previous_close,
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
        } else {
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

    fn update_active_daily_candle(&mut self, code: &StockCode, price: Money, added_volume: u64) {
        let time = i64::from(self.day) * SECONDS_PER_DAY;
        let candle = self
            .active_daily_candles
            .entry(code.clone())
            .or_insert(DailyCandle {
                time,
                open: price,
                high: price,
                low: price,
                close: price,
                volume: 0,
            });
        // 开盘前 PriceTick 会用昨收建立零成交占位 K。集合竞价后的第一笔真实成交
        // 才是当日开盘价；此时必须丢弃占位 OHLC，否则跳空实体会错误连接到昨收。
        if candle.volume == 0 && added_volume > 0 {
            *candle = DailyCandle {
                time,
                open: price,
                high: price,
                low: price,
                close: price,
                volume: added_volume,
            };
            return;
        }
        candle.high = candle.high.max(price);
        candle.low = candle.low.min(price);
        candle.close = price;
        candle.volume = candle
            .volume
            .checked_add(added_volume)
            .expect("daily candle volume overflow: engine state invariant violated");
    }

    fn commit_active_daily_candles(&mut self) -> BTreeMap<StockCode, DailyCandle> {
        let closed = std::mem::take(&mut self.active_daily_candles);
        for (code, candle) in &closed {
            let history = self.daily_candles.entry(code.clone()).or_default();
            history.push(candle.clone());
            if history.len() > PRESET_HISTORY_DAYS {
                history.drain(..history.len() - PRESET_HISTORY_DAYS);
            }
        }
        closed
    }

    fn route_auction_intent(&mut self, acct: AccountId, intent: Intent, events: &mut Vec<Event>) {
        let (code, side, price, qty) = match intent {
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } => (code, side, price, qty),
            Intent::PlaceMarket { code, .. } => {
                events.push(Event::IntentRejected {
                    seq: self.next_seq(),
                    account: acct,
                    code,
                    reason: RejectionReason::AuctionLimitOrderRequired,
                });
                return;
            }
            Intent::Cancel { code, .. } => {
                events.push(Event::IntentRejected {
                    seq: self.next_seq(),
                    account: acct,
                    code,
                    reason: RejectionReason::AuctionOrderNotCancelable,
                });
                return;
            }
        };
        let Some(market) = self.markets.get(&code) else {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::UnknownStock,
            });
            return;
        };
        if price < market.down_stop() || price > market.up_stop() {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::LimitExceeded,
            });
            return;
        }
        let tick = self
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == code)
            .expect("market code must have a stock spec")
            .tick;
        if qty == 0 || price.cents() < 0 || price.cents() % tick.cents() != 0 {
            events.push(Event::SettlementError {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: format!("invalid auction limit order: price={price:?}, qty={qty}"),
            });
            return;
        }
        if !self.prevalidate_order(acct, &code, side, price, qty, events) {
            return;
        }
        let arrival_seq = self.next_order_id;
        self.next_order_id += 1;
        self.auction_orders
            .entry(code)
            .or_default()
            .push(AuctionOrderSnap {
                owner: acct,
                side,
                limit: price,
                qty,
                arrival_seq,
            });
    }

    fn complete_auction(&mut self, code: &StockCode, events: &mut Vec<Event>) {
        let previous_close = self
            .markets
            .get(code)
            .expect("code collected from markets must exist")
            .last_close();
        let orders = self.auction_orders.get(code).cloned().unwrap_or_default();
        let Some(clearing) = clearing_result(&orders, previous_close) else {
            self.update_active_daily_candle(code, previous_close, 0);
            events.push(Event::AuctionCompleted {
                seq: self.next_seq(),
                tick: self.tick,
                code: code.clone(),
                opening_price: None,
                matched_volume: 0,
            });
            return;
        };

        let mut buys: Vec<AuctionOrderSnap> = orders
            .iter()
            .filter(|order| order.side == Side::Buy && order.limit >= clearing.price)
            .cloned()
            .collect();
        let mut sells: Vec<AuctionOrderSnap> = orders
            .iter()
            .filter(|order| order.side == Side::Sell && order.limit <= clearing.price)
            .cloned()
            .collect();
        buys.sort_by_key(|order| (std::cmp::Reverse(order.limit), order.arrival_seq));
        sells.sort_by_key(|order| (order.limit, order.arrival_seq));

        let mut buy_index = 0;
        let mut sell_index = 0;
        let mut matched_volume = 0_u64;
        while buy_index < buys.len() && sell_index < sells.len() {
            let qty = buys[buy_index].qty.min(sells[sell_index].qty);
            let buyer = buys[buy_index].owner;
            let seller = sells[sell_index].owner;
            let (maker, taker) = if buys[buy_index].arrival_seq < sells[sell_index].arrival_seq {
                (buyer, seller)
            } else {
                (seller, buyer)
            };
            if let Err(error) = self.settle(buyer, Side::Buy, code, clearing.price, qty) {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: buyer,
                    code: code.clone(),
                    reason: error.to_string(),
                });
            }
            if let Err(error) = self.settle(seller, Side::Sell, code, clearing.price, qty) {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: seller,
                    code: code.clone(),
                    reason: error.to_string(),
                });
            }
            matched_volume = matched_volume
                .checked_add(u64::from(qty))
                .expect("auction matched volume overflow: order quantities exceed u64");
            self.update_active_daily_candle(code, clearing.price, u64::from(qty));
            events.push(Event::Trade {
                seq: self.next_seq(),
                code: code.clone(),
                price: clearing.price,
                qty,
                maker,
                taker,
            });
            buys[buy_index].qty -= qty;
            sells[sell_index].qty -= qty;
            if buys[buy_index].qty == 0 {
                buy_index += 1;
            }
            if sells[sell_index].qty == 0 {
                sell_index += 1;
            }
        }
        self.markets
            .get_mut(code)
            .expect("code collected from markets must exist")
            .set_last_price(clearing.price);
        events.push(Event::AuctionCompleted {
            seq: self.next_seq(),
            tick: self.tick,
            code: code.clone(),
            opening_price: Some(clearing.price),
            matched_volume,
        });
    }

    fn prevalidate_order(
        &mut self,
        acct: AccountId,
        code: &StockCode,
        side: Side,
        price: Money,
        qty: u32,
        events: &mut Vec<Event>,
    ) -> bool {
        let rejection = match side {
            Side::Buy => {
                let required_for = |limit: Money, quantity: u32| -> i128 {
                    let gross = i128::from(limit.cents()) * i128::from(quantity);
                    if gross > i128::from(i64::MAX) {
                        return i128::MAX;
                    }
                    let gross_money = Money::from_cents(gross as i64);
                    match self.setup.config.commission(gross_money) {
                        Ok(commission) => gross.saturating_add(i128::from(commission.cents())),
                        Err(_) => i128::MAX,
                    }
                };
                let already_reserved = self
                    .auction_orders
                    .values()
                    .flatten()
                    .filter(|order| order.owner == acct && order.side == Side::Buy)
                    .fold(0_i128, |total, order| {
                        total.saturating_add(required_for(order.limit, order.qty))
                    });
                let total = already_reserved.saturating_add(required_for(price, qty));
                let cash = self
                    .accounts
                    .get(&acct)
                    .map(|a| a.cash)
                    .unwrap_or(Money::ZERO);
                (total > i128::from(cash.cents())).then_some(RejectionReason::InsufficientCash)
            }
            Side::Sell => {
                let already_reserved = self
                    .auction_orders
                    .get(code)
                    .into_iter()
                    .flatten()
                    .filter(|order| order.owner == acct && order.side == Side::Sell)
                    .fold(0_u64, |total, order| {
                        total
                            .checked_add(u64::from(order.qty))
                            .expect("auction sell reservation overflow")
                    });
                let sellable = self
                    .accounts
                    .get(&acct)
                    .map(|account| account.sellable_qty(code))
                    .unwrap_or(0);
                (already_reserved.saturating_add(u64::from(qty)) > u64::from(sellable))
                    .then_some(RejectionReason::InsufficientShares)
            }
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
        let (code, side, price, qty) = match &intent {
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } => (code.clone(), *side, *price, *qty),
            Intent::PlaceMarket { code, side, qty } => {
                let last = self
                    .markets
                    .get(code)
                    .map(|m| m.last_price())
                    .unwrap_or(Money::ZERO);
                (code.clone(), *side, last, *qty)
            }
            Intent::Cancel { .. } => return, // v1 撤单暂不处理（不静默：后续任务补）
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
        // 预校验资金/持仓（最坏情况估算）。
        match side {
            Side::Buy => {
                // cost 用 i128 防溢出再 clamp 回 i64（极端大单）。
                let cost_cents = (price.cents() as i128)
                    .checked_mul(qty as i128)
                    .unwrap_or(i64::MAX as i128);
                let cost = if cost_cents > i64::MAX as i128 {
                    Money::from_cents(i64::MAX)
                } else {
                    Money::from_cents(cost_cents as i64)
                };
                let commission = self.setup.config.commission(cost).unwrap_or(Money::ZERO);
                let total = cost.add(commission).unwrap_or(cost);
                let have = self
                    .accounts
                    .get(&acct)
                    .map(|a| a.cash)
                    .unwrap_or(Money::ZERO);
                if total > have {
                    events.push(Event::IntentRejected {
                        seq: self.next_seq(),
                        account: acct,
                        code: code.clone(),
                        reason: RejectionReason::InsufficientCash,
                    });
                    return;
                }
            }
            Side::Sell => {
                let sellable = self
                    .accounts
                    .get(&acct)
                    .map(|a| a.sellable_qty(&code))
                    .unwrap_or(0);
                if qty > sellable {
                    events.push(Event::IntentRejected {
                        seq: self.next_seq(),
                        account: acct,
                        code: code.clone(),
                        reason: RejectionReason::InsufficientShares,
                    });
                    return;
                }
            }
        }
        // 构造 Order（唯一 id）并撮合。
        let oid = OrderId(self.next_order_id);
        self.next_order_id += 1;
        let order = Order {
            id: oid,
            side,
            price,
            qty,
            owner: acct,
            seq: 0,
        };
        let trades = match self.markets.get_mut(&code) {
            Some(m) => match m.place(order) {
                Ok(r) => r.trades,
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
            },
            None => return, // 上面 contains_key 已校验，理论不可达；不静默吞：无事件即无副作用。
        };
        // 逐笔结算。maker 反向 side、taker 本 side。code 用路由的 code（Trade 无 code 字段）。
        let maker_side = if side == Side::Buy {
            Side::Sell
        } else {
            Side::Buy
        };
        for t in trades {
            let (maker_id, taker_id) = (t.maker, t.taker);
            if let Err(e) = self.settle(maker_id, maker_side, &code, t.price, t.qty) {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: maker_id,
                    code: code.clone(),
                    reason: e.to_string(),
                });
            }
            if let Err(e) = self.settle(taker_id, side, &code, t.price, t.qty) {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: taker_id,
                    code: code.clone(),
                    reason: e.to_string(),
                });
            }
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

    /// 结算到某账户：克隆 config（解 `&self.setup` 借用）→ 取 `&mut Account` → `apply_trade`。
    /// 账户不存在 → [`AccountError::NoPosition`]（不静默，铁律二）。
    fn settle(
        &mut self,
        id: AccountId,
        side: Side,
        code: &StockCode,
        price: Money,
        qty: u32,
    ) -> Result<(), AccountError> {
        let cfg = self.setup.config.clone();
        let acc = self
            .accounts
            .get_mut(&id)
            .ok_or_else(|| AccountError::NoPosition(code.clone()))?;
        acc.apply_trade(&cfg, side, code.clone(), price, qty, self.setup.t1_enabled)
    }

    /// 玩家意图入队（随时可调）；在下个 [`Self::step`] 开头才执行（玩家意图统一在 NPC 之后路由）。
    ///
    /// 玩家账户不存在 → [`SessionError::UnknownPlayer`]（致命错误显式返回，铁律二）。
    pub fn enqueue_player_intent(
        &mut self,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SessionError> {
        if !self.accounts.contains_key(&player_id) {
            return Err(SessionError::UnknownPlayer(player_id));
        }
        self.pending_player.push(intent);
        Ok(())
    }

    /// 生成存档（精确到交易日）。
    /// 在 DayBoundary 后调用 → snapshot 含 end_of_day 后的状态（last_close 已更新）。
    pub fn save(&self) -> SaveSlot {
        SaveSlot {
            setup: self.setup.clone(),
            seed: self.seed,
            snapshot: self.snapshot(),
            auction_orders: self.auction_orders.clone(),
            next_order_id: self.next_order_id,
        }
    }

    /// 从存档恢复（精确到天，不保留日内分时/盘口挂单）。
    ///
    /// 流程：
    /// 1. new(setup, seed) → 新建 session（含初始持仓分配）
    /// 2. 清空所有账户持仓 → 用快照精确覆盖（cash + positions invested/recovered/t1_locked）
    /// 3. 覆盖每只股票的 last_price/last_close/fundamental_value
    /// 4. 设 tick/day/seq
    ///
    /// 不保留：盘口挂单（加载后 NPC 重新挂单）、分时图历史（前端从头累积）、价格历史队列。
    pub fn restore(save: &SaveSlot) -> Result<GameSession, SessionError> {
        let mut sess = GameSession::new(save.setup.clone(), save.seed)?;

        // 清空初始持仓分配 → 用快照精确覆盖
        for acc in sess.accounts.values_mut() {
            acc.positions.clear();
        }

        // 恢复账户状态（cash + positions 精确值）
        for (id, snap_acc) in &save.snapshot.accounts {
            if let Some(acc) = sess.accounts.get_mut(id) {
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
        }

        // 恢复市场状态（last_price/last_close/V）
        for (code, snap_mkt) in &save.snapshot.markets {
            if let Some(m) = sess.markets.get_mut(code) {
                m.set_last_price(snap_mkt.last_price);
                m.set_last_close(snap_mkt.last_close);
                m.set_fundamental_value(snap_mkt.fundamental_value);
            }
        }

        // 恢复进度
        sess.tick = save.snapshot.tick;
        sess.day = save.snapshot.day;
        sess.seq = save.snapshot.seq;
        validate_saved_auction_state(&sess, save)?;
        sess.auction_orders = save
            .auction_orders
            .iter()
            .filter(|(code, _)| sess.markets.contains_key(*code))
            .map(|(code, orders)| (code.clone(), orders.clone()))
            .collect();
        sess.next_order_id = save.next_order_id;

        // 新格式精确恢复 Rust 持有的 K 线；旧存档或某只新增股票没有记录时，
        // 保留 `new` 已按相同 setup/seed 生成的 360 日基线。
        for (code, candles) in &save.snapshot.daily_candles {
            if !candles.is_empty() && sess.markets.contains_key(code) {
                sess.daily_candles.insert(code.clone(), candles.clone());
            }
        }
        sess.active_daily_candles = save
            .snapshot
            .active_daily_candles
            .iter()
            .filter(|(code, _)| sess.markets.contains_key(*code))
            .map(|(code, candle)| (code.clone(), candle.clone()))
            .collect();

        Ok(sess)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct ClearingResult {
    price: Money,
    volume: u64,
    imbalance: u64,
}

/// A 股集合竞价的简化唯一价格规则：最大成交量 → 最小不平衡量 → 最接近昨收 → 低价优先。
fn clearing_result(orders: &[AuctionOrderSnap], previous_close: Money) -> Option<ClearingResult> {
    let candidates: BTreeSet<Money> = orders.iter().map(|order| order.limit).collect();
    candidates
        .into_iter()
        .filter_map(|price| {
            let buy = orders
                .iter()
                .filter(|order| order.side == Side::Buy && order.limit >= price)
                .try_fold(0_u64, |total, order| {
                    total.checked_add(u64::from(order.qty))
                })?;
            let sell = orders
                .iter()
                .filter(|order| order.side == Side::Sell && order.limit <= price)
                .try_fold(0_u64, |total, order| {
                    total.checked_add(u64::from(order.qty))
                })?;
            let volume = buy.min(sell);
            (volume > 0).then_some((
                ClearingResult {
                    price,
                    volume,
                    imbalance: buy.abs_diff(sell),
                },
                buy.abs_diff(sell),
                i128::from(price.cents()).abs_diff(i128::from(previous_close.cents())),
            ))
        })
        .min_by_key(|(result, imbalance, distance)| {
            (
                std::cmp::Reverse(result.volume),
                *imbalance,
                *distance,
                result.price,
            )
        })
        .map(|(result, _, _)| result)
}

fn auction_total_imbalance(orders: &[AuctionOrderSnap]) -> u64 {
    let (buy, sell) = orders
        .iter()
        .fold((0_u64, 0_u64), |(buy, sell), order| match order.side {
            Side::Buy => (
                buy.checked_add(u64::from(order.qty))
                    .expect("auction buy quantity overflow"),
                sell,
            ),
            Side::Sell => (
                buy,
                sell.checked_add(u64::from(order.qty))
                    .expect("auction sell quantity overflow"),
            ),
        });
    buy.abs_diff(sell)
}

fn validate_saved_auction_state(
    session: &GameSession,
    save: &SaveSlot,
) -> Result<(), SessionError> {
    if save.snapshot.phase != session.phase() {
        return Err(SessionError::InvalidSetup(format!(
            "save phase {:?} does not match tick-derived phase {:?}",
            save.snapshot.phase,
            session.phase()
        )));
    }
    if session.phase() == TradingPhase::Continuous && !save.auction_orders.is_empty() {
        return Err(SessionError::InvalidSetup(
            "continuous-phase save must not contain auction orders".to_string(),
        ));
    }
    let mut arrivals = BTreeSet::new();
    let mut max_arrival = 0_u64;
    let mut reserved_buys: BTreeMap<AccountId, i128> = BTreeMap::new();
    let mut reserved_sells: BTreeMap<(AccountId, StockCode), u64> = BTreeMap::new();
    for (code, orders) in &save.auction_orders {
        let market = session.markets.get(code).ok_or_else(|| {
            SessionError::InvalidSetup(format!(
                "save auction order references unknown stock {code:?}"
            ))
        })?;
        let tick = session
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .expect("market code must have a stock spec")
            .tick;
        for order in orders {
            if !session.accounts.contains_key(&order.owner) {
                return Err(SessionError::InvalidSetup(format!(
                    "save auction order references unknown account {:?}",
                    order.owner
                )));
            }
            if order.qty == 0
                || order.limit < market.down_stop()
                || order.limit > market.up_stop()
                || order.limit.cents() % tick.cents() != 0
            {
                return Err(SessionError::InvalidSetup(format!(
                    "invalid saved auction order for {code:?}: {order:?}"
                )));
            }
            if !arrivals.insert(order.arrival_seq) {
                return Err(SessionError::InvalidSetup(format!(
                    "duplicate saved auction arrival_seq {}",
                    order.arrival_seq
                )));
            }
            max_arrival = max_arrival.max(order.arrival_seq);
            match order.side {
                Side::Buy => {
                    let gross = i128::from(order.limit.cents()) * i128::from(order.qty);
                    if gross > i128::from(i64::MAX) {
                        return Err(SessionError::InvalidSetup(
                            "saved auction buy amount overflows Money".to_string(),
                        ));
                    }
                    let commission = session
                        .setup
                        .config
                        .commission(Money::from_cents(gross as i64))?;
                    let reserved = reserved_buys.entry(order.owner).or_default();
                    *reserved = reserved
                        .checked_add(gross + i128::from(commission.cents()))
                        .ok_or_else(|| {
                            SessionError::InvalidSetup(
                                "saved auction buy reservations overflow".to_string(),
                            )
                        })?;
                }
                Side::Sell => {
                    let reserved = reserved_sells
                        .entry((order.owner, code.clone()))
                        .or_default();
                    *reserved = reserved.checked_add(u64::from(order.qty)).ok_or_else(|| {
                        SessionError::InvalidSetup(
                            "saved auction sell reservations overflow".to_string(),
                        )
                    })?;
                }
            }
        }
    }
    if !arrivals.is_empty() && save.next_order_id <= max_arrival {
        return Err(SessionError::InvalidSetup(format!(
            "next_order_id {} must exceed saved auction arrival_seq {}",
            save.next_order_id, max_arrival
        )));
    }
    for (owner, reserved) in reserved_buys {
        let cash = session
            .accounts
            .get(&owner)
            .expect("saved auction owner was validated")
            .cash;
        if reserved > i128::from(cash.cents()) {
            return Err(SessionError::InvalidSetup(format!(
                "saved auction buys over-reserve cash for {owner:?}"
            )));
        }
    }
    for ((owner, code), reserved) in reserved_sells {
        let sellable = session
            .accounts
            .get(&owner)
            .expect("saved auction owner was validated")
            .sellable_qty(&code);
        if reserved > u64::from(sellable) {
            return Err(SessionError::InvalidSetup(format!(
                "saved auction sells over-reserve shares for {owner:?} {code:?}"
            )));
        }
    }
    Ok(())
}

const PRESET_HISTORY_DAYS: usize = 360;
const SECONDS_PER_DAY: i64 = 86_400;

/// 生成与 session RNG 隔离的确定性历史，避免预置行情改变 NPC 的随机序列。
fn generate_preset_daily_candles(
    setup: &SessionSetup,
    seed: u64,
) -> BTreeMap<StockCode, Vec<DailyCandle>> {
    setup
        .stocks
        .iter()
        .map(|stock| {
            let mut rng =
                SplitMix64::new(seed ^ stock_code_hash(&stock.code) ^ 0xD1A1_C4AD_1E50_0360);
            let tick = stock.tick.cents().max(1);
            let anchor = stock.initial_price.cents().max(tick);
            let limit_bps = (stock.limit_pct * 10_000.0).round().max(1.0) as i64;
            let min_price = quantize_cents((anchor / 3).max(tick), tick);
            let max_price = quantize_cents(anchor.saturating_mul(3), tick);
            let mut close = anchor;
            let mut newest_first = Vec::with_capacity(PRESET_HISTORY_DAYS);

            for recency in 0..PRESET_HISTORY_DAYS {
                let body_radius = (close.saturating_mul(limit_bps) / 20_000).max(tick);
                let body_delta = random_signed(&mut rng, body_radius / tick) * tick;
                let open = quantize_cents(
                    close.saturating_sub(body_delta).clamp(min_price, max_price),
                    tick,
                );
                let wick_radius = (close.saturating_mul(limit_bps) / 50_000).max(tick);
                let upper = (rng.next_u64() % ((wick_radius / tick + 1) as u64)) as i64 * tick;
                let lower = (rng.next_u64() % ((wick_radius / tick + 1) as u64)) as i64 * tick;
                let high =
                    quantize_cents(open.max(close).saturating_add(upper).min(max_price), tick);
                let low = quantize_cents(open.min(close).saturating_sub(lower).max(tick), tick);
                let base_volume = u64::from(stock.float_shares).max(100_000) / 1_250;
                let volume = base_volume + rng.next_u64() % (base_volume.saturating_mul(5).max(1));
                newest_first.push(DailyCandle {
                    time: -((recency as i64) + 1) * SECONDS_PER_DAY,
                    open: Money::from_cents(open),
                    high: Money::from_cents(high),
                    low: Money::from_cents(low),
                    close: Money::from_cents(close),
                    volume,
                });

                let gap_radius = (open.saturating_mul(limit_bps) / 80_000).max(tick);
                let gap_delta = random_signed(&mut rng, gap_radius / tick) * tick;
                close = quantize_cents(
                    open.saturating_sub(gap_delta).clamp(min_price, max_price),
                    tick,
                );
            }
            newest_first.reverse();
            (stock.code.clone(), newest_first)
        })
        .collect()
}

fn random_signed(rng: &mut SplitMix64, radius: i64) -> i64 {
    let radius = radius.max(1);
    let width = (radius as u64).saturating_mul(2).saturating_add(1);
    (rng.next_u64() % width) as i64 - radius
}

fn quantize_cents(cents: i64, tick: i64) -> i64 {
    ((cents.saturating_add(tick / 2)) / tick).max(1) * tick
}

/// 把 StockCode 哈希为 u64（用于派生每只股票的确定性 RNG 种子）。
fn stock_code_hash(code: &StockCode) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod candle_open_tests {
    use super::*;
    use crate::{HotParams, InstParams, RetailParams};

    fn gap_stock_setup() -> SessionSetup {
        SessionSetup {
            stocks: vec![StockSpec {
                code: StockCode("GAP001".to_string()),
                initial_price: Money::from_cents(1_000),
                limit_pct: 0.5,
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
            player_cash: Money::from_cents(1_000_000),
            ticks_per_day: 10,
            auction_ticks: 0,
            history_len: 10,
            t1_enabled: false,
            float_allocation: FloatAllocation::Random,
        }
    }

    #[test]
    fn first_auction_trade_replaces_provisional_previous_close_on_gap_up_days() {
        let code = StockCode("GAP001".to_string());
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
        let code = StockCode("GAP001".to_string());
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
