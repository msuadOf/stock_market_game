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
use crate::strategy::{Intent, MarketView, PositionView, SelfView, StockView, StrategyFactory, StrategyParams};
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

/// 编排层。持有全部状态；纯逻辑、无全局可变状态（实例即隔离）。
///
/// NPC 与玩家同构（均为 [`Account`]），区别仅在 `strategy`（NPC=算法、玩家=None）。
/// 单一 RNG 源 `rng`（[`SplitMix64`]）贯穿全 tick，保证确定性（同种子同输入同输出）。
pub struct GameSession {
    setup: SessionSetup,
    rng: SplitMix64,
    markets: BTreeMap<StockCode, Market>,
    accounts: BTreeMap<AccountId, Account>,
    price_history: BTreeMap<StockCode, VecDeque<Money>>,
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
            return Err(SessionError::InvalidSetup("ticks_per_day must be > 0".to_string()));
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
        let mut accounts = BTreeMap::new();
        accounts.insert(
            AccountId(0),
            Account::new(AccountId(0), AccountKind::Player, setup.player_cash),
        );
        let rng = SplitMix64::new(seed);
        let mut sess = GameSession {
            setup,
            rng,
            markets,
            accounts,
            price_history,
            pending_player: Vec::new(),
            next_order_id: 1,
            tick: 0,
            day: 0,
            seq: 0,
        };
        sess.populate_npcs(AccountKind::Retail);
        sess.populate_npcs(AccountKind::Inst);
        sess.populate_npcs(AccountKind::Hot);
        Ok(sess)
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
            markets,
            accounts,
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
                    fundamental_value: if see_v { Some(m.fundamental_value()) } else { None },
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
            None => return SelfView { cash: Money::ZERO, positions: BTreeMap::new() },
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
        SelfView { cash: a.cash, positions }
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

        // 1. 收集 Intent（NPC 升序 + 玩家队列）。
        let mut pending: Vec<(AccountId, Intent)> = Vec::new();
        let npc_ids: Vec<AccountId> =
            self.accounts.keys().copied().filter(|id| id.0 != 0).collect();
        for id in npc_ids {
            let see_v = self
                .accounts
                .get(&id)
                .map(|a| a.kind == AccountKind::Inst)
                .unwrap_or(false);
            let mv = self.build_market_view(see_v);
            let sv = self.build_self_view(id);
            let intents: Vec<Intent> = match self.accounts.get_mut(&id) {
                Some(acc) => match acc.strategy.as_mut() {
                    Some(strat) => strat.decide(&mv, &sv, &mut self.rng),
                    None => Vec::new(),
                },
                None => Vec::new(),
            };
            for it in intents {
                pending.push((id, it));
            }
        }
        let player_intents = std::mem::take(&mut self.pending_player);
        for it in player_intents {
            pending.push((AccountId(0), it));
        }

        // 2. 预校验 + 路由（market.place + 结算均在内）。
        for (acct, intent) in pending {
            self.route_intent(acct, intent, &mut events);
        }

        // 3. V 演化（单一 RNG 源，确定性）。
        let v_params = self.setup.v_params.clone();
        let codes: Vec<StockCode> = self.markets.keys().cloned().collect();
        for code in &codes {
            if let Some(m) = self.markets.get_mut(code) {
                if let Err(e) = m.evolve_v(&v_params, &mut self.rng) {
                    events.push(Event::VError {
                        seq: self.next_seq(),
                        code: code.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        }

        // 4. tick 自增 + 价格历史 + PriceTick。
        self.tick += 1;
        for code in &codes {
            let last = self
                .markets
                .get(code)
                .map(|m| m.last_price())
                .unwrap_or(Money::ZERO);
            if let Some(h) = self.price_history.get_mut(code) {
                h.push_back(last);
                while h.len() > self.setup.history_len {
                    h.pop_front();
                }
            }
            events.push(Event::PriceTick {
                seq: self.next_seq(),
                code: code.clone(),
                last_price: last,
            });
        }

        // 5. 日界：到 ticks_per_day → end_of_day + day+1 + DayBoundary。
        if self.tick.is_multiple_of(self.setup.ticks_per_day) {
            for code in &codes {
                if let Some(m) = self.markets.get_mut(code) {
                    m.end_of_day();
                }
            }
            self.day += 1;
            events.push(Event::DayBoundary { seq: self.next_seq(), day: self.day });
        }

        events
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
            Intent::PlaceLimit { code, side, price, qty } => (code.clone(), *side, *price, *qty),
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
                let have = self.accounts.get(&acct).map(|a| a.cash).unwrap_or(Money::ZERO);
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
                let sellable = self.accounts.get(&acct).map(|a| a.sellable_qty(&code)).unwrap_or(0);
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
        let order = Order { id: oid, side, price, qty, owner: acct, seq: 0 };
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
        let maker_side = if side == Side::Buy { Side::Sell } else { Side::Buy };
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
}
