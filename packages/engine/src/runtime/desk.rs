//! 交易部线程（一股一线程，独占撮合）——批 3 引擎化。
//!
//! 业务语义照搬现引擎（脊树），执行形态改为交易部线程私有：
//! - 簿：双边 `BTreeMap<价, VecDeque<DeskOrder>>` 价格档 × 到达序 FIFO——价格时间
//!   优先（taker 逐档吃对面最优，同价先到先成交），对齐 `orderbook.rs place_inner`
//!   与统一簿 RestingKey 语义；成交价恒取 maker 价。
//! - 校验：相位门禁（`pipeline/phase.rs` PhasePolicy 语义）→ 单笔上限（主板/
//!   创业板差异，`session.rs SecurityCategory::max_order_qty`）→ 买入整手 →
//!   价格合法（正数 + tick 整数倍）→ 涨跌停带（闭区间，`market.rs place_inner`）→
//!   价格笼子（仅连续竞价 + 开关，参考价对手一档→本方一档→最新价，±2% 与十个
//!   最小价位取宽，`market.rs continuous_limit_*`）。
//! - 费用：`runtime/fees.rs`（照搬 config.rs/transition.rs，银行家舍入同口径）。
//! - 集合竞价：申报只积累不撮合；开盘竞价在相位离开申报窗的首个 tick 一次清算
//!   （`orderbook/auction_clearing.rs select_clearing` 沪深选价规则直接复用），
//!   余单保留原档转连续；收盘竞价在 Drain 时一次清算后清簿。
//! - T+1：卖出可卖校验在组端（持仓知识），交易部无账户视图（F §7 行 6/7）。
//! - 失败原子性：撤销日志覆盖簿/序号/计数/费用桶/行情（快照条目）——Abort 逆序
//!   回放 + 清出口缓冲；转正 = 下一 tick 首批 Ops（隐式）。

use super::fees::{buyer_charge, seller_charge, FeeQuote};
use super::protocol::{
    DeskBatch, DeskIn, DeskMsg, DeskReport, RoutedPrice, RuntimeError, UpMsg, WorkerId,
};
use super::watchdog::{worker_recv, WatchdogConfig};
use super::{DeskReceipt, Entity, FillFact, OrderRef, Owner, ReceiptKind, StockIdx};
use crate::config::GameConfig;
use crate::money::Money;
use crate::observation::completed_market_minute_count;
use crate::orderbook::auction_clearing::{select_clearing, ClearingOrder};
use crate::orderbook::Side;
use crate::StockExchange;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// 构造期迁入的在途挂单种子（adopt：会话旧簿单直入 desk 初始簿——
/// 先于本 tick 新单（到簿序对齐旧管线；此前经 ExternalPlan 消息链晚于
/// 外部新单，maker/taker 身分颠倒）。
#[derive(Clone, Copy, Debug)]
pub struct DeskRestingSeed {
    pub owner: Owner,
    pub side: Side,
    pub price: i64,
    pub qty: u32,
    /// 买腿 purse（卖恒 0）——adopt 单的 live 资源面。
    pub purse: i64,
    /// D5 restore 链最后一层：受理事实已随此前世界交付（save 前的事件
    /// 历史含 Accepted——restore 重放即重复）。true=直入不记 Accepted
    /// 生命周期事实；false=live 构造期 adopt 形态（受理事实照记）。
    pub accepted_delivered: bool,
}

/// D5 restore 链最后一层：日内行情聚合种子（save 点的 desk 行情权威
/// 投影——活动 K/日内高低调/分钟收盘/昨收。此前 restore 后 desk 行情
/// 从构造零点起，续拍把当日聚合覆盖回零起点，save 对拍显红）。
#[derive(Clone, Debug)]
pub struct DeskMarketSeed {
    /// 最新价（活动 K close）。
    pub last_price: i64,
    /// 昨收（日 K 历史末条 close；首日=初始价）。
    pub last_close: i64,
    /// 日内最高。
    pub day_high: i64,
    /// 日内最低。
    pub day_low: i64,
    /// 日累计成交量（股）。
    pub volume_today: u64,
    /// 日累计成交额（分）。
    pub turnover_today: i64,
    /// 日累计成交笔数。
    pub trade_count: u64,
    /// 活动 K（o/h/l/c/v）。
    pub candle: Option<(i64, i64, i64, i64, u64)>,
    /// 分钟收盘（绝对交易分钟, 分）。
    pub minute_closes: Vec<(u64, i64)>,
}

/// 每股交易部构造参数（世界构造期确定，运行期不变）。
#[derive(Clone, Debug)]
pub struct DeskSetup {
    /// 股索引。
    pub stock: StockIdx,
    /// 上市交易所（集合竞价选价规则差异：沪同价取中间价 / 深距离规则）。
    pub exchange: StockExchange,
    /// 初始价（分；`last_price`/`last_close` 初值，首日无昨收以开盘价为准）。
    pub initial_price: i64,
    /// 涨跌幅（基点：主板 1000 / 创业板 2000）。
    pub limit_bps: u32,
    /// 限价单笔申报上限（主板 1_000_000 / 创业板 300_000）。
    pub max_order_qty_limit: u32,
    /// 市价单笔申报上限（主板 1_000_000 / 创业板 150_000）。
    pub max_order_qty_market: u32,
    /// 每交易日 tick 数。
    pub ticks_per_day: u64,
    /// 开盘集合竞价窗口 tick 数（前 2/3 申报、后 1/3 盘前；0=全天连续，测试形态）。
    pub auction_ticks: u64,
    /// 收盘集合竞价窗口 tick 数。
    pub closing_auction_ticks: u64,
    /// 挂单 TTL（tick）。
    pub order_ttl_ticks: u64,
    /// tick 内价格历史容量。
    pub history_len: usize,
    /// D0：起始已转正 tick（恢复/接管对齐——Ops 的 tick 连续性按绝对
    /// tick 推导）。
    pub initial_tick: u64,
    /// D5 adopt：构造期直入簿的会话旧簿在途单。
    pub initial_orders: Vec<DeskRestingSeed>,
    /// D5 restore 等价：种子 tick 价格历史。
    pub initial_recent_prices: Vec<i64>,
    /// D3a-2：种子日 K（会话预置历史投影——(open, high, low, close,
    /// volume)，不过滤；desk 为链观察的日 K 权威承载）。
    pub initial_daily_bars: Vec<(i64, i64, i64, i64, u64)>,
    /// D3a-2：种子历史之外的累计 traded 日数（种子内的 traded 由
    /// initial_daily_bars 自行统计——避免双计）。
    pub initial_traded_days: u64,
    /// D5 restore 链最后一层：日内行情聚合种子（save 点投影；None=
    /// live 构造/无会话来源，desk 从构造零点起）。
    pub initial_market: Option<DeskMarketSeed>,
    /// D5 attention：收盘竞价公开截点种子（restore 链）。
    pub initial_public_cutoff: u64,
}

/// 交易相位（照搬 `session.rs TradingPhase` 的 tick 推导）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskPhase {
    /// 开盘集合竞价申报期（09:15–09:25：前 2/3 窗口）。
    CallAuction,
    /// 盘前（09:25–09:30：不接受申报）。
    PreOpen,
    /// 连续竞价。
    Continuous,
    /// 收盘集合竞价（14:57–15:00：限价申报、不可撤、日终一次撮合）。
    ClosingAuction,
}

impl DeskSetup {
    /// tick 对应交易相位（照搬 `GameSession::phase`）。
    pub fn phase_at(&self, tick: u64) -> DeskPhase {
        let day_tick = tick % self.ticks_per_day;
        let entry = self.auction_ticks - self.auction_ticks / 3;
        if day_tick < entry {
            DeskPhase::CallAuction
        } else if day_tick < self.auction_ticks {
            DeskPhase::PreOpen
        } else if day_tick >= self.ticks_per_day - self.closing_auction_ticks {
            DeskPhase::ClosingAuction
        } else {
            DeskPhase::Continuous
        }
    }

    /// 相位是否允许撤单（照搬 PhasePolicy::allows_cancel：开盘竞价前 1/3 可撤、
    /// 连续可撤、盘前与收盘竞价不可撤）。
    pub fn allows_cancel(&self, tick: u64) -> bool {
        match self.phase_at(tick) {
            DeskPhase::CallAuction => tick % self.ticks_per_day < self.auction_ticks / 3,
            DeskPhase::Continuous => true,
            DeskPhase::PreOpen | DeskPhase::ClosingAuction => false,
        }
    }

    /// 本 tick 是否为连续竞价价格笼子生效窗口。
    pub fn cage_applies(&self, tick: u64) -> bool {
        self.phase_at(tick) == DeskPhase::Continuous
    }

    /// 连续竞价每分钟的 tick 换算输入。
    fn continuous_ticks_per_day(&self) -> u64 {
        self.ticks_per_day
            .checked_sub(self.auction_ticks)
            .and_then(|t| t.checked_sub(self.closing_auction_ticks))
            .unwrap_or(0)
    }
}

/// 挂单：资源实体随单驻留交易部（买单 purse=剩余可花；卖单 qty=剩余股数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskOrder {
    /// 复合挂单标识。
    pub order_ref: OrderRef,
    /// 所有者。
    pub owner: Owner,
    /// 方向。
    pub side: Side,
    /// 限价（分；符号已解析）。
    pub price: i64,
    /// 剩余数量。
    pub qty: u32,
    /// 买腿剩余可花（卖恒 0）。
    pub purse: i64,
    /// 累计成交额（分）。
    pub filled_value: i64,
    /// 卖腿累计实收费用（分项）。
    pub charged_commission: i64,
    /// 卖腿累计实收印花税。
    pub charged_stamp_tax: i64,
    /// 卖腿累计实收过户费。
    pub charged_transfer_fee: i64,
    /// 到期 tick。
    pub expire_tick: u64,
}

/// 活动日 K（字段对齐 `session.rs DailyCandle` 的行情面）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeskCandle {
    /// 开盘价。
    pub open: i64,
    /// 最高价。
    pub high: i64,
    /// 最低价。
    pub low: i64,
    /// 收盘价（最新价）。
    pub close: i64,
    /// 成交量（股）。
    pub volume: u64,
}

/// 行情面（desk 私有常驻，不每 tick 重建）。
#[derive(Clone, Debug)]
pub struct DeskMarket {
    /// 最新成交价（成交驱动；初值=initial_price）。
    pub last_price: i64,
    /// 昨收（涨跌停基准）。
    pub last_close: i64,
    /// 日内最高（含昨收锚定：无成交时=last_close）。
    pub day_high: i64,
    /// 日内最低。
    pub day_low: i64,
    /// 日累计成交量（股）。
    pub volume_today: u64,
    /// 日累计成交额（分）——活动 K trade_stats 口径。
    pub turnover_today: i64,
    /// 日累计成交笔数。
    pub trade_count: u64,
    /// 活动日 K。
    pub candle: Option<DeskCandle>,
    /// 分钟收盘（绝对交易分钟，无成交分钟沿用当时最新价——对齐
    /// `observation.rs MarketMinuteClose` 语义）。
    pub minute_closes: Vec<(u64, i64)>,
    /// tick 收盘价历史（容量 history_len）。
    pub recent_prices: VecDeque<i64>,
    /// 已完成交易日 K 历史（日界时活动 K 入列）。
    pub candles: Vec<DeskCandle>,
    /// 历史累计 traded 日数（volume>0；D3a-2 技术面 valid_sample_count）。
    pub traded_days_total: u64,
}

/// 行情快照（撤销日志条目）。
#[derive(Clone, Debug)]
struct MarketSnapshot {
    last_price: i64,
    last_close: i64,
    day_high: i64,
    day_low: i64,
    volume_today: u64,
    trade_count: u64,
    candle: Option<DeskCandle>,
    minute_len: usize,
    recent_len: usize,
    candles_len: usize,
    turnover_today: i64,
}

/// 撤销日志条目（逆序回放恢复；覆盖面=全部可变状态）。
#[derive(Clone, Debug)]
enum DeskUndo {
    /// 新挂入订单（回滚=移除）。
    Inserted(DeskOrder),
    /// 移除订单（回滚=放回档尾原序由 FIFO 保证——移除只发生在档首，放回档首）。
    Removed(DeskOrder),
    /// 订单字段被改（回滚=按 order_ref 原位恢复字段值）。
    Mutated(DeskOrder),
    /// 本地序号前进。
    Serial,
    /// resting 计数变动。
    RestingCount(i64),
    /// 累计跳过计数变动。
    Skipped(u64),
    /// 到期/撤单计数变动。
    Expired(u64),
    Cancelled(u64),
    /// 费用桶累计变动。
    FeeSink(i64),
    /// 行情快照（tick 起始整面）。
    Market(MarketSnapshot),
}

/// 交易部期末上交状态。
#[derive(Clone, Debug)]
pub struct DeskFinalState {
    /// 股索引。
    pub stock: StockIdx,
    /// 期末挂单（Drain 后为空）。
    pub resting: Vec<DeskOrder>,
    /// 累计受理拒绝数。
    pub skipped_orders: u64,
    /// 累计到期撤销数。
    pub expired_orders: u64,
    /// 累计撤单数。
    pub cancelled_orders: u64,
    /// 费用桶（守恒审计：双边实收费用累计）。
    pub fee_sink: i64,
    /// 期末行情。
    pub market: DeskMarket,
    /// 当前态哈希。
    pub state_hash: u64,
    /// 提交态哈希。
    pub committed_hash: u64,
    /// Abort 回滚完成时刻的态哈希（回滚等价判据：应等于 `committed_hash`；
    /// 之后的 Drain 关停属于合法的后续状态变化，不污染本判据。None=无 Abort）。
    pub rollback_hash: Option<u64>,
}

/// 交易部私有状态。
pub struct DeskState {
    setup: DeskSetup,
    config: Arc<GameConfig>,
    bids: BTreeMap<i64, VecDeque<DeskOrder>>,
    asks: BTreeMap<i64, VecDeque<DeskOrder>>,
    next_serial: u64,
    /// D5 attention 公开可见性门禁：收盘竞价公开截点（连续→收盘竞价
    /// 切换时点的 next_serial——老单（serial<截点）公开、竞价新单隐藏；
    /// A 股收盘集合竞价时段不揭示新申报的真实制度面）。
    public_cutoff_serial: u64,
    /// 上拍相位（公开截点的切换检测）。
    last_phase: Option<DeskPhase>,
    undo: Vec<DeskUndo>,
    out: Vec<Vec<Entity>>,
    fills_out: Vec<Vec<FillFact>>,
    receipts_out: Vec<Vec<DeskReceipt>>,
    flushed: Vec<bool>,
    cur_tick: u64,
    cur_round: u32,
    committed_tick: u64,
    committed_hash: u64,
    pub market: DeskMarket,
    fee_sink: i64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub resting_count: u64,
    /// 本 tick 是否已执行开盘清算（防御：一次）。
    opening_cleared: bool,
    /// 本 tick 双腿成交事实缓冲（出口随 DeskReport；Abort 清空）。
    trade_facts: Vec<super::protocol::DeskTradeFact>,
    /// 本 tick 生命周期事实缓冲（受理/撤单/到期）。
    lifecycle_facts: Vec<super::protocol::DeskLifecycleFact>,
    /// 本 tick 竞价事实缓冲。
    auction_facts: Vec<super::protocol::DeskAuctionFact>,
    /// 自成交快速索引：owner → 卖侧自单价集合（价→计数）。taker 到达先查
    /// 索引（无交叉自单则零簿扫描走原快速路径）。
    self_asks: std::collections::HashMap<Owner, std::collections::BTreeMap<i64, u32>>,
    /// owner → 买侧自单价集合。
    self_bids: std::collections::HashMap<Owner, std::collections::BTreeMap<i64, u32>>,
    /// 最近一次 Abort 回滚完成时刻的态哈希（回滚等价判据：应等于 committed_hash；
    /// 之后的 Drain 关停属于合法的后续状态变化，不污染本判据）。
    rollback_hash: Option<u64>,
}

impl DeskState {
    /// 构造交易部状态。
    pub fn new(setup: DeskSetup, n_groups: usize, config: Arc<GameConfig>) -> Self {
        let initial_tick = setup.initial_tick;
        let market = DeskMarket {
            last_price: setup.initial_price,
            last_close: setup.initial_price,
            day_high: setup.initial_price,
            day_low: setup.initial_price,
            volume_today: 0,
            turnover_today: 0,
            trade_count: 0,
            candle: None,
            minute_closes: Vec::new(),
            recent_prices: VecDeque::new(),
            candles: setup.initial_daily_bars.iter().map(|&(o, h, l, c, v)| DeskCandle {
                open: o,
                high: h,
                low: l,
                close: c,
                volume: v,
            }).collect(),
            traded_days_total: setup.initial_traded_days
                + setup
                    .initial_daily_bars
                    .iter()
                    .filter(|&&(_, _, _, _, v)| v > 0)
                    .count() as u64,
        };
        DeskState {
            setup,
            config,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            next_serial: 1,
            public_cutoff_serial: 0,
            last_phase: None,
            undo: Vec::new(),
            out: vec![Vec::new(); n_groups],
            fills_out: vec![Vec::new(); n_groups],
            receipts_out: vec![Vec::new(); n_groups],
            flushed: vec![false; n_groups],
            cur_tick: initial_tick,
            cur_round: 0,
            committed_tick: initial_tick,
            committed_hash: 0,
            market,
            fee_sink: 0,
            skipped_orders: 0,
            expired_orders: 0,
            cancelled_orders: 0,
            resting_count: 0,
            opening_cleared: false,
            trade_facts: Vec::new(),
            lifecycle_facts: Vec::new(),
            auction_facts: Vec::new(),
            self_asks: std::collections::HashMap::new(),
            self_bids: std::collections::HashMap::new(),
            rollback_hash: None,
        }
    }

    /// 当前态哈希（FNV-1a；仅域内容，跨 tick 可比）。
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = fnv(h, self.setup.stock as u64);
        h = fnv(h, self.next_serial);
        h = fnv(h, self.resting_count);
        h = fnv(h, self.market.turnover_today as u64);
        h = fnv(h, self.fee_sink as u64);
        let m = &self.market;
        for v in [
            m.last_price as u64,
            m.last_close as u64,
            m.day_high as u64,
            m.day_low as u64,
            m.volume_today,
            m.trade_count,
            m.minute_closes.len() as u64,
            m.recent_prices.len() as u64,
        ] {
            h = fnv(h, v);
        }
        if let Some(c) = m.candle {
            for v in [
                c.open as u64,
                c.high as u64,
                c.low as u64,
                c.close as u64,
                c.volume,
            ] {
                h = fnv(h, v);
            }
        }
        for (minute, close) in &m.minute_closes {
            h = fnv(h, *minute);
            h = fnv(h, *close as u64);
        }
        for &p in &m.recent_prices {
            h = fnv(h, p as u64);
        }
        for (price, level) in self.bids.iter().chain(self.asks.iter()) {
            for o in level {
                h = fnv(h, *price as u64);
                h = fnv(h, o.order_ref.1);
                h = fnv(h, o.owner.0 as u64);
                h = fnv(h, o.owner.1 as u64);
                h = fnv(
                    h,
                    match o.side {
                        Side::Buy => 1u64,
                        Side::Sell => 2u64,
                    },
                );
                h = fnv(h, o.qty as u64);
                h = fnv(h, o.purse as u64);
                h = fnv(h, o.filled_value as u64);
            }
        }
        h
    }

    // ── 撤销日志 ──

    fn snapshot_market(&mut self) {
        let m = &self.market;
        self.undo.push(DeskUndo::Market(MarketSnapshot {
            last_price: m.last_price,
            last_close: m.last_close,
            day_high: m.day_high,
            day_low: m.day_low,
            volume_today: m.volume_today,
            trade_count: m.trade_count,
            candle: m.candle,
            minute_len: m.minute_closes.len(),
            recent_len: m.recent_prices.len(),
            candles_len: m.candles.len(),
            turnover_today: m.turnover_today,
        }));
    }

    /// 逆序回放撤销日志 + 清出口缓冲（Abort/panic 回滚）。
    fn rollback(&mut self) {
        let undo = std::mem::take(&mut self.undo);
        for entry in undo.into_iter().rev() {
            match entry {
                DeskUndo::Inserted(o) => {
                    self.remove_by_ref(&o.order_ref);
                }
                DeskUndo::Removed(o) => {
                    let idx = if o.side == Side::Buy {
                        &mut self.self_bids
                    } else {
                        &mut self.self_asks
                    };
                    *idx.entry(o.owner).or_default().entry(o.price).or_insert(0) += 1;
                    let level = if o.side == Side::Buy {
                        &mut self.bids
                    } else {
                        &mut self.asks
                    }
                    .entry(o.price)
                    .or_default();
                    level.push_front(o);
                }
                DeskUndo::Mutated(o) => {
                    self.restore_fields(&o);
                }
                DeskUndo::Serial => self.next_serial -= 1,
                DeskUndo::RestingCount(d) => {
                    self.resting_count = (self.resting_count as i64 - d) as u64;
                }
                DeskUndo::Skipped(d) => self.skipped_orders -= d,
                DeskUndo::Expired(d) => self.expired_orders -= d,
                DeskUndo::Cancelled(d) => self.cancelled_orders -= d,
                DeskUndo::FeeSink(d) => self.fee_sink -= d,
                DeskUndo::Market(s) => {
                    let m = &mut self.market;
                    m.last_price = s.last_price;
                    m.last_close = s.last_close;
                    m.day_high = s.day_high;
                    m.day_low = s.day_low;
                    m.volume_today = s.volume_today;
                    m.trade_count = s.trade_count;
                    m.candle = s.candle;
                    m.minute_closes.truncate(s.minute_len);
                    m.recent_prices.truncate(s.recent_len);
                    m.candles.truncate(s.candles_len);
                    m.turnover_today = s.turnover_today;
                }
            }
        }
        for buf in &mut self.out {
            buf.clear();
        }
        for buf in &mut self.fills_out {
            buf.clear();
        }
        for buf in &mut self.receipts_out {
            buf.clear();
        }
        for f in &mut self.flushed {
            *f = false;
        }
        self.trade_facts.clear();
        self.lifecycle_facts.clear();
        self.auction_facts.clear();
        self.cur_tick = self.committed_tick;
        self.cur_round = 0;
        self.opening_cleared = false;
    }

    fn remove_by_ref(&mut self, order_ref: &OrderRef) {
        let mut removed: Option<DeskOrder> = None;
        for book in [&mut self.bids, &mut self.asks] {
            let mut hit_price: Option<i64> = None;
            for (price, level) in book.iter_mut() {
                let mut drain_at: Option<usize> = None;
                for (i, o) in level.iter().enumerate() {
                    if o.order_ref == *order_ref {
                        drain_at = Some(i);
                        break;
                    }
                }
                if let Some(i) = drain_at {
                    removed = level.remove(i);
                    hit_price = Some(*price);
                    break;
                }
            }
            if let Some(price) = hit_price {
                // Sol 二审修复集 1：回放删除同步清空价档（与 remove_order/
                // apply_maker_fill 同口径）。空档残留会让拍卖清算的
                // keys().next()/next_back() 踩空档提前零成交——
                // best_bid/best_ask 的跳空防御只护连续撮合路径。
                if book.get(&price).is_some_and(|l| l.is_empty()) {
                    book.remove(&price);
                }
                break;
            }
        }
        // P1-3（Sol）：回放移除同步自成交索引（此前漏——自单残留
        // 阻断后续第三方配对）。
        if let Some(o) = removed {
            let idx = if o.side == Side::Buy {
                &mut self.self_bids
            } else {
                &mut self.self_asks
            };
            if let Some(prices) = idx.get_mut(&o.owner) {
                if let Some(cnt) = prices.get_mut(&o.price) {
                    *cnt = cnt.saturating_sub(1);
                    if *cnt == 0 {
                        prices.remove(&o.price);
                    }
                }
                if prices.is_empty() {
                    idx.remove(&o.owner);
                }
            }
        }
    }

    fn restore_fields(&mut self, snapshot: &DeskOrder) {
        for book in [&mut self.bids, &mut self.asks] {
            for level in book.values_mut() {
                for o in level.iter_mut() {
                    if o.order_ref == snapshot.order_ref {
                        *o = snapshot.clone();
                    }
                }
            }
        }
    }

    // ── 价格边界（照搬 market.rs，Money 同口径取整）──

    fn money(cents: i64) -> Money {
        Money::from_cents(cents)
    }

    fn price_bound(
        &self,
        reference: Money,
        ratio_bps: u32,
        upward: bool,
    ) -> Result<Money, RuntimeError> {
        let tick = Money::from_cents(1);
        let denominator = i128::from(10_000u32) * i128::from(tick.cents());
        let numerator = i128::from(reference.cents()) * i128::from(ratio_bps);
        let rounded_ticks = (numerator + denominator / 2) / denominator;
        let rounded_cents = i64::try_from(rounded_ticks * i128::from(tick.cents()))
            .map_err(|_| over("涨跌停价溢出"))?;
        let mut bound = Money::from_cents(rounded_cents.max(tick.cents()));
        if upward && bound <= reference {
            bound = reference.add(tick).map_err(|_| over("涨跌停价溢出"))?;
        } else if !upward && bound >= reference {
            bound = reference
                .sub(tick)
                .map_err(|_| over("涨跌停价溢出"))?
                .max(tick);
        }
        Ok(bound)
    }

    /// 涨停价（昨收 × (1+limit)，正数四舍五入至最小价位，至少一档）。
    pub fn up_stop(&self) -> Result<i64, RuntimeError> {
        Ok(self
            .price_bound(
                Self::money(self.market.last_close),
                10_000 + self.setup.limit_bps,
                true,
            )?
            .cents())
    }

    /// 跌停价。
    pub fn down_stop(&self) -> Result<i64, RuntimeError> {
        Ok(self
            .price_bound(
                Self::money(self.market.last_close),
                10_000 - self.setup.limit_bps,
                false,
            )?
            .cents())
    }

    fn best_bid(&self) -> Option<i64> {
        // 防御：跳过空档（键残留而队列空的中间态——remove 路径清理
        // 不完全时的兜底显式化，不静默成交错价）。
        self.bids
            .iter()
            .rev()
            .find(|(_, level)| !level.is_empty())
            .map(|(price, _)| *price)
    }

    fn best_ask(&self) -> Option<i64> {
        self.asks
            .iter()
            .find(|(_, level)| !level.is_empty())
            .map(|(price, _)| *price)
    }

    /// 连续竞价限价申报基准价（上交所规则 3.3.14：对手一档→本方一档→最新价）。
    fn continuous_reference(&self, side: Side) -> i64 {
        match side {
            Side::Buy => self.best_ask().or_else(|| self.best_bid()),
            Side::Sell => self.best_bid().or_else(|| self.best_ask()),
        }
        .unwrap_or(self.market.last_price)
    }

    /// 价格笼子边界（买上限/卖下限；±2% 与十个最小价位取宽）。
    fn cage_bound(&self, side: Side) -> Result<i64, RuntimeError> {
        let reference = Self::money(self.continuous_reference(side));
        let ten_ticks = 10i64;
        match side {
            Side::Buy => {
                let pct = self.price_bound(reference, 10_200, true)?.cents();
                Ok(pct.max(reference.cents() + ten_ticks))
            }
            Side::Sell => {
                let pct = self.price_bound(reference, 9_800, false)?.cents();
                Ok(pct.min(reference.cents() - ten_ticks).max(1))
            }
        }
    }

    /// 受理价上限/下限：日涨跌停 ∧ 价格笼子（笼子仅连续竞价 + 配置开启时叠加）。
    fn price_bound_for(
        &self,
        side: Side,
        tick: u64,
        cage: bool,
    ) -> Result<(i64, i64), RuntimeError> {
        let up = self.up_stop()?;
        let down = self.down_stop()?;
        if !cage || !self.setup.cage_applies(tick) {
            return Ok((down, up));
        }
        let cage = self.cage_bound(side)?;
        Ok(match side {
            Side::Buy => (down, up.min(cage)),
            Side::Sell => (down.max(cage), up),
        })
    }

    /// 解析价格符号（照搬 market.rs resolve_limit_price + 市价=涨跌停保护）。
    /// 返回 (受理价, 余量是否挂簿)——市价余量不进簿。
    fn resolve_price(
        &self,
        rp: RoutedPrice,
        side: Side,
        tick: u64,
    ) -> Result<(i64, bool), RuntimeError> {
        let cage = self.config.price_cage_enabled;
        let (down, up) = self.price_bound_for(side, tick, cage)?;
        let (price, rest_remainder) = match rp {
            RoutedPrice::Fixed(p) => (p, true),
            RoutedPrice::Highest => match side {
                Side::Buy => (up, true),
                Side::Sell => (up, true),
            },
            RoutedPrice::Lowest => (down, true),
            RoutedPrice::Market => match side {
                Side::Buy => (up, false),
                Side::Sell => (down, false),
            },
        };
        Ok((price, rest_remainder))
    }

    // ── 受理与撮合 ──

    fn apply_ops(&mut self, tick: u64, ops: Vec<DeskIn>) -> Result<(), RuntimeError> {
        for op in ops {
            match op {
                DeskIn::Place {
                    owner,
                    side,
                    price,
                    qty,
                    purse,
                } => {
                    self.place(tick, owner, side, price, qty, purse)?;
                }
                DeskIn::CancelAll { owner } => self.cancel_all_for(tick, owner),
                DeskIn::Cancel { owner, order } => self.cancel_one(tick, owner, order)?,
            }
        }
        Ok(())
    }

    /// 受理下单：相位门禁 → 数量上限 → 整手 → 价格合法 → 带/笼 → 撮合/积累 → 挂簿。
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        tick: u64,
        owner: Owner,
        side: Side,
        rp: RoutedPrice,
        qty: u32,
        purse: i64,
    ) -> Result<(), RuntimeError> {
        let phase = self.setup.phase_at(tick);
        let is_market = matches!(rp, RoutedPrice::Market);
        // 相位门禁：盘前拒收；竞价阶段只收限价。
        if phase == DeskPhase::PreOpen {
            self.reject(tick, owner, side, qty, purse, ReceiptKind::PhaseRejected);
            return Ok(());
        }
        if phase == DeskPhase::CallAuction || phase == DeskPhase::ClosingAuction {
            if is_market {
                self.reject(
                    tick,
                    owner,
                    side,
                    qty,
                    purse,
                    ReceiptKind::AuctionLimitOrderRequired,
                );
                return Ok(());
            }
        }
        // 数量：非零 + 单笔上限（类别差异：主板/创业板 × 限价/市价）。
        let cap = if is_market {
            self.setup.max_order_qty_market
        } else {
            self.setup.max_order_qty_limit
        };
        if qty == 0 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "交易部 {} 收到 qty=0 下单（组端校验缺失）",
                    self.setup.stock
                ),
            });
        }
        if qty > cap {
            self.reject(tick, owner, side, qty, purse, ReceiptKind::ExceedsOrderCap);
            return Ok(());
        }
        // 买入整手（卖出零股一次性由组端校验——持仓知识在本组）。
        if side == Side::Buy && qty % self.config.lot_size != 0 {
            self.reject(tick, owner, side, qty, purse, ReceiptKind::NotRoundLot);
            return Ok(());
        }
        // 价格：正数 + tick 整数倍（最小价位 0.01 元）。
        let (price, rest_remainder) = self.resolve_price(rp, side, tick)?;
        if price <= 0 || price % 1 != 0 {
            self.reject(tick, owner, side, qty, purse, ReceiptKind::PriceOutOfBand);
            return Ok(());
        }
        // 涨跌停带（闭区间）与价格笼子。
        let cage = self.config.price_cage_enabled;
        let (down, up) = self.price_bound_for(side, tick, cage)?;
        if price < down || price > up {
            self.reject(tick, owner, side, qty, purse, ReceiptKind::PriceOutOfBand);
            return Ok(());
        }
        if purse < 0 || (side == Side::Sell && purse != 0) {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "交易部 {} purse 非法：side={side:?}, purse={purse}",
                    self.setup.stock
                ),
            });
        }
        let order_ref = (self.setup.stock, self.next_serial);
        let order = DeskOrder {
            order_ref,
            owner,
            side,
            price,
            qty,
            purse,
            filled_value: 0,
            charged_commission: 0,
            charged_stamp_tax: 0,
            charged_transfer_fee: 0,
            expire_tick: tick.saturating_add(self.setup.order_ttl_ticks),
        };
        match phase {
            DeskPhase::CallAuction | DeskPhase::ClosingAuction => {
                // 集合竞价：只积累不撮合。
                self.rest(order);
            }
            DeskPhase::PreOpen | DeskPhase::Continuous => {
                let mut order = order;
                self.match_incoming(tick, &mut order)?;
                if order.qty == 0 {
                    // 全成：purse slack（最坏情况预留 − 实际扣费）退款 + Closed。
                    self.close_order_entities(&order);
                } else if rest_remainder {
                    self.rest(order);
                } else {
                    // 市价余量不进簿：原路退回。
                    self.close_order_entities(&order);
                    self.cancelled_orders += 1;
                    self.undo.push(DeskUndo::Cancelled(1));
                }
            }
        }
        Ok(())
    }

    /// 记录受理事实（Accepted）。
    fn note_accepted(&mut self, order: &DeskOrder) {
        self.lifecycle_facts
            .push(super::protocol::DeskLifecycleFact {
                stock: self.setup.stock,
                owner: order.owner,
                order_serial: order.order_ref.1,
                remaining: order.qty,
                side: order.side,
                price: order.price,
                kind: super::protocol::LifecycleKind::Accepted,
            });
    }

    /// 记录终结事实（Cancelled/Expired）。
    fn note_terminated(&mut self, order: &DeskOrder, kind: super::protocol::LifecycleKind) {
        self.lifecycle_facts
            .push(super::protocol::DeskLifecycleFact {
                stock: self.setup.stock,
                owner: order.owner,
                order_serial: order.order_ref.1,
                remaining: order.qty,
                side: order.side,
                price: order.price,
                kind,
            });
    }

    /// 挂簿（记撤销日志+Accepted 生命周期事实）。
    fn rest(&mut self, order: DeskOrder) {
        let fact = self.rest_inner(order);
        self.lifecycle_facts.push(fact);
    }

    /// 挂簿（记撤销日志，不记 Accepted 事实——restore 形态种子直入：
    /// 受理事件已随此前世界交付，重放即重复）。
    fn rest_silent(&mut self, order: DeskOrder) {
        self.rest_inner(order);
    }

    /// 挂簿共用段（返回本单的 Accepted 生命周期事实——调用方决定是否记录）。
    fn rest_inner(&mut self, order: DeskOrder) -> super::protocol::DeskLifecycleFact {
        self.next_serial += 1;
        self.undo.push(DeskUndo::Serial);
        let idx = if order.side == Side::Buy {
            &mut self.self_bids
        } else {
            &mut self.self_asks
        };
        *idx.entry(order.owner)
            .or_default()
            .entry(order.price)
            .or_insert(0) += 1;
        let fact = super::protocol::DeskLifecycleFact {
            stock: self.setup.stock,
            owner: order.owner,
            order_serial: order.order_ref.1,
            remaining: order.qty,
            side: order.side,
            price: order.price,
            kind: super::protocol::LifecycleKind::Accepted,
        };
        let book = if order.side == Side::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };
        book.entry(order.price)
            .or_default()
            .push_back(order.clone());
        self.undo.push(DeskUndo::Inserted(order));
        self.resting_count += 1;
        self.undo.push(DeskUndo::RestingCount(1));
        fact
    }

    /// 连续竞价撮合：逐档吃对面最优（价格时间优先），成交价取 maker 价。
    fn match_incoming(&mut self, _tick: u64, order: &mut DeskOrder) -> Result<(), RuntimeError> {
        loop {
            if order.qty == 0 {
                break;
            }
            let opposite_best = match order.side {
                Side::Buy => self.best_ask(),
                Side::Sell => self.best_bid(),
            };
            let Some(best) = opposite_best else { break };
            let crossed = match order.side {
                Side::Buy => order.price >= best,
                Side::Sell => order.price <= best,
            };
            if !crossed {
                break;
            }
            // A 股自成交限制（索引快速路径）：taker 先查自己在对手侧
            // 有无 ≤/≥ 本档价的自单（O(log) 索引查，无自单零簿扫描）。
            let has_self_at_or_better = match order.side {
                Side::Buy => self
                    .self_asks
                    .get(&order.owner)
                    .and_then(|m| m.range(..=best).next_back())
                    .is_some(),
                Side::Sell => self
                    .self_bids
                    .get(&order.owner)
                    .and_then(|m| m.range(best..).next())
                    .is_some(),
            };
            if has_self_at_or_better {
                // 档首是自单（FIFO 档首即最早自单）——保守终止撮合。
                // 语义与批 2 版一致：仅当交叉档存在自单竞争时终止。
                break;
            }
            // 取档首 maker（同价先到先成交）。
            let maker = {
                let book = if order.side == Side::Buy {
                    &self.asks
                } else {
                    &self.bids
                };
                book.get(&best)
                    .and_then(|level| level.front().cloned())
                    .expect("crossed => 档非空")
            };
            let fill_qty = order.qty.min(maker.qty);
            self.settle_fill(order, maker, fill_qty)?;
        }
        Ok(())
    }

    /// 单笔成交结算：实体互换 + 费用 + FillFact 双边 + 行情更新（全部记撤销日志）。
    fn settle_fill(
        &mut self,
        taker: &mut DeskOrder,
        maker: DeskOrder,
        qty: u32,
    ) -> Result<(), RuntimeError> {
        let price = maker.price;
        let value = Money::from_cents(price)
            .mul_shares(qty)
            .map_err(|_| over("成交额溢出"))?
            .cents();
        // 快照（撤销日志）：taker 与 maker 变更前状态。
        self.undo.push(DeskUndo::Mutated(taker.clone()));
        self.undo.push(DeskUndo::Mutated(maker.clone()));
        // 分别按腿结算（taker/maker 身份不影响费用，只按 side）。
        let (buy_owner, buy_local, buy_filled_before, buy_purse_before, buy_ref) =
            if taker.side == Side::Buy {
                (
                    taker.owner,
                    taker.owner.1,
                    taker.filled_value,
                    taker.purse,
                    taker.order_ref,
                )
            } else {
                (
                    maker.owner,
                    maker.owner.1,
                    maker.filled_value,
                    maker.purse,
                    maker.order_ref,
                )
            };
        let (sell_owner, sell_local, sell_filled_before, sell_charged_before, sell_ref) =
            if taker.side == Side::Sell {
                (
                    taker.owner,
                    taker.owner.1,
                    taker.filled_value,
                    FeeQuote {
                        commission: Money::from_cents(taker.charged_commission),
                        stamp_tax: Money::from_cents(taker.charged_stamp_tax),
                        transfer_fee: Money::from_cents(taker.charged_transfer_fee),
                    },
                    taker.order_ref,
                )
            } else {
                (
                    maker.owner,
                    maker.owner.1,
                    maker.filled_value,
                    FeeQuote {
                        commission: Money::from_cents(maker.charged_commission),
                        stamp_tax: Money::from_cents(maker.charged_stamp_tax),
                        transfer_fee: Money::from_cents(maker.charged_transfer_fee),
                    },
                    maker.order_ref,
                )
            };
        // 买腿：charge = 成交额 + 边际佣金 + 边际过户费；purse 扣减；得股票实体。
        let charge = buyer_charge(
            &self.config,
            Money::from_cents(buy_filled_before),
            Money::from_cents(value),
        )
        .map_err(|_| over("买腿费用计算失败"))?
        .cents();
        let buy_commission_delta = marginal(
            &self.config,
            Money::from_cents(buy_filled_before),
            Money::from_cents(buy_filled_before + value),
            |c, a| c.commission(a),
        )
        .map_err(|_| over("佣金边际失败"))?
        .cents();
        let buy_transfer_delta = marginal(
            &self.config,
            Money::from_cents(buy_filled_before),
            Money::from_cents(buy_filled_before + value),
            |c, a| c.transfer_fee(a),
        )
        .map_err(|_| over("过户费边际失败"))?
        .cents();
        // purse 防御：不足=组端预留口径 bug（purse 应含费用上限），显式失败不静默负数。
        // D5 adopt 例外：purse=0 的单=构造期迁入的旧簿在途单（无 purse
        /// 预留——组端现金实扣走外来成交落账面，Sol 六/七审外来路径），
        // purse 记账维持 0（不透支簿面）。
        let adopt_foreign_buy = buy_purse_before == 0;
        let buy_purse_after = if adopt_foreign_buy {
            0
        } else {
            let after = buy_purse_before - charge;
            if after < 0 {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "交易部 {} 买腿 purse 不足：purse={buy_purse_before}, charge={charge}（组端预留口径 bug）",
                        self.setup.stock
                    ),
                });
            }
            after
        };
        // 卖腿：名义费用 → 实收封顶分配（佣金→印花→过户费）；净得实体。
        let sell_filled_after = sell_filled_before + value;
        let nominal_after = FeeQuote::nominal(
            &self.config,
            Side::Sell,
            Money::from_cents(sell_filled_after),
        )
        .map_err(|_| over("卖腿名义费用失败"))?;
        let charged = seller_charge(nominal_after, sell_charged_before, Money::from_cents(value))
            .map_err(|_| over("卖腿费用分配失败"))?;
        // 应用到 taker/maker 副本（写回簿/订单）。
        if taker.side == Side::Buy {
            taker.filled_value += value;
            taker.purse = buy_purse_after;
            taker.qty -= qty;
        } else {
            taker.filled_value = sell_filled_after;
            taker.charged_commission += charged.commission.cents();
            taker.charged_stamp_tax += charged.stamp_tax.cents();
            taker.charged_transfer_fee += charged.transfer_fee.cents();
            taker.qty -= qty;
        }
        let maker_after = {
            let mut m = maker.clone();
            if m.side == Side::Buy {
                m.filled_value += value;
                m.purse = buy_purse_after;
                m.qty -= qty;
            } else {
                m.filled_value = sell_filled_after;
                m.charged_commission += charged.commission.cents();
                m.charged_stamp_tax += charged.stamp_tax.cents();
                m.charged_transfer_fee += charged.transfer_fee.cents();
                m.qty -= qty;
            }
            m
        };
        // maker 写回：档首扣减或移除。
        self.apply_maker_fill(&maker, maker_after)?;
        // 费用桶（守恒）：双边实收**费用**合计（charge 含成交额，须扣除）。
        let fees_total = (charge - value)
            + charged.commission.cents()
            + charged.stamp_tax.cents()
            + charged.transfer_fee.cents();
        self.fee_sink += fees_total;
        self.undo.push(DeskUndo::FeeSink(fees_total));
        // 双腿成交事实（事件域输入：maker+taker 身份与成交序）。
        let taker_side = taker.side;
        let (maker_id, taker_id) = if taker_side == Side::Buy {
            (maker.owner, taker.owner)
        } else {
            (taker.owner, maker.owner)
        };
        // 双腿实收费用（Sol 五审同拍投影源）：买腿=charge−value（佣金+
        // 过户边际）；卖腿=封顶分配合计。**费用跟归一化身份走**（Sol
        // 六审修复 1：maker 字段恒为卖方、taker 恒为买方——此前按原始
        // taker_side 交换，主动卖时双方费用互换、本拍现金错）。
        let buy_fees = charge - value;
        let sell_fees = charged.commission.cents()
            + charged.stamp_tax.cents()
            + charged.transfer_fee.cents();
        // 双腿订单身份（归一化同序——Sol 六审修复 2：同拍执行腿投影
        // 按真实订单推进，外部单不得误挂计划子单。Sol 七审修复 1：两
        // 方向统一 (sell_ref, buy_ref)——owner 已固定 (seller, buyer)，
        // 序号按原始方向交换会使主动卖时身份核跳过合法计划成交）。
        let (maker_order, taker_order) = (sell_ref, buy_ref);
        self.trade_facts.push(super::protocol::DeskTradeFact {
            stock: self.setup.stock,
            price,
            qty,
            maker: maker_id,
            taker: taker_id,
            taker_side,
            maker_fees_cents: sell_fees,
            taker_fees_cents: buy_fees,
            maker_order_serial: maker_order.1,
            taker_order_serial: taker_order.1,
        });
        // 实体互换：买方得股票、卖方得净额。
        let bg = buy_owner.0 as usize;
        self.out[bg].push(Entity::BoughtShares {
            local: buy_local,
            qty,
        });
        let sg = sell_owner.0 as usize;
        self.out[sg].push(Entity::CashProceeds {
            local: sell_local,
            amount: charged.deliver_cash.cents(),
        });
        // FillFact 双边（信息回流：计划续行/经验/回执聚合）。
        self.fills_out[bg].push(FillFact {
            order: buy_ref,
            owner: buy_owner,
            side: Side::Buy,
            price,
            qty,
            commission: buy_commission_delta,
            stamp_tax: 0,
            transfer_fee: buy_transfer_delta,
        });
        self.fills_out[sg].push(FillFact {
            order: sell_ref,
            owner: sell_owner,
            side: Side::Sell,
            price,
            qty,
            commission: charged.commission.cents(),
            stamp_tax: charged.stamp_tax.cents(),
            transfer_fee: charged.transfer_fee.cents(),
        });
        // 行情更新：末笔成交价、日内高低、量、笔数、活动日 K。
        let m = &mut self.market;
        m.last_price = price;
        m.day_high = m.day_high.max(price);
        m.day_low = m.day_low.min(price);
        m.volume_today += qty as u64;
        m.turnover_today += value;
        m.trade_count += 1;
        let candle = m.candle.get_or_insert(DeskCandle {
            open: price,
            high: price,
            low: price,
            close: price,
            volume: 0,
        });
        candle.high = candle.high.max(price);
        candle.low = candle.low.min(price);
        candle.close = price;
        candle.volume += qty as u64;
        Ok(())
    }

    /// maker 写回：清零移除（记 Removed），否则字段恢复（档首原位）。
    fn apply_maker_fill(
        &mut self,
        maker: &DeskOrder,
        after: DeskOrder,
    ) -> Result<(), RuntimeError> {
        let book = if maker.side == Side::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };
        let level = book
            .get_mut(&maker.price)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: format!("maker 档缺失：price={}", maker.price),
            })?;
        let front = level
            .front_mut()
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: format!("maker 档空：price={}", maker.price),
            })?;
        if front.order_ref != maker.order_ref {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("maker 非档首：{:?}", maker.order_ref),
            });
        }
        if after.qty == 0 {
            let removed = level.pop_front().expect("front 已核验");
            // 自成交索引同步（全成移除路径——cancel/expire 之外的第三路径）。
            {
                let idx = if removed.side == Side::Buy {
                    &mut self.self_bids
                } else {
                    &mut self.self_asks
                };
                if let Some(prices) = idx.get_mut(&removed.owner) {
                    if let Some(cnt) = prices.get_mut(&removed.price) {
                        *cnt -= 1;
                        if *cnt == 0 {
                            prices.remove(&removed.price);
                        }
                    }
                    if prices.is_empty() {
                        idx.remove(&removed.owner);
                    }
                }
            }
            self.undo.push(DeskUndo::Removed(removed));
            self.resting_count -= 1;
            self.undo.push(DeskUndo::RestingCount(-1));
            let book = if maker.side == Side::Buy {
                &mut self.bids
            } else {
                &mut self.asks
            };
            if book.get(&maker.price).is_some_and(|l| l.is_empty()) {
                book.remove(&maker.price);
            }
            // maker 全成闭合：买腿 slack 退款（purse 最坏情况 − 实际扣费）+ Closed 恒发。
            let g = after.owner.0 as usize;
            if after.side == Side::Buy && after.purse > 0 {
                self.out[g].push(Entity::CashRefund {
                    local: after.owner.1,
                    amount: after.purse,
                });
            }
            self.out[g].push(Entity::Closed {
                local: after.owner.1,
                order_serial: after.order_ref.1,
            });
        } else {
            *front = after;
        }
        Ok(())
    }

    /// 终结挂单实体出口：买单 purse 余额退款 / 卖单余股回流（零额不推）+
    /// Closed 恒发（零额终结也必须发，否则在途登记泄漏）。
    fn close_order_entities(&mut self, order: &DeskOrder) {
        let g = order.owner.0 as usize;
        match order.side {
            Side::Buy => {
                if order.purse > 0 {
                    self.out[g].push(Entity::CashRefund {
                        local: order.owner.1,
                        amount: order.purse,
                    });
                }
            }
            Side::Sell => {
                if order.qty > 0 {
                    self.out[g].push(Entity::RefundedShares {
                        local: order.owner.1,
                        qty: order.qty,
                    });
                }
            }
        }
        self.out[g].push(Entity::Closed {
            local: order.owner.1,
            order_serial: order.order_ref.1,
        });
    }

    /// 移除挂单（记 Removed）。
    fn remove_order(&mut self, order: &DeskOrder) {
        // 自成交索引同步移除。
        {
            let idx = if order.side == Side::Buy {
                &mut self.self_bids
            } else {
                &mut self.self_asks
            };
            if let Some(prices) = idx.get_mut(&order.owner) {
                if let Some(cnt) = prices.get_mut(&order.price) {
                    *cnt -= 1;
                    if *cnt == 0 {
                        prices.remove(&order.price);
                    }
                }
                if prices.is_empty() {
                    idx.remove(&order.owner);
                }
            }
        }
        let book = if order.side == Side::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };
        if let Some(level) = book.get_mut(&order.price) {
            if let Some(pos) = level.iter().position(|o| o.order_ref == order.order_ref) {
                let removed = level.remove(pos).expect("position 已核验");
                self.undo.push(DeskUndo::Removed(removed));
                self.resting_count -= 1;
                self.undo.push(DeskUndo::RestingCount(-1));
                if level.is_empty() {
                    book.remove(&order.price);
                }
            }
        }
    }

    /// 对齐撤单：撤该 owner 全部挂单（实体原路回流）。撤单窗口门禁在组端不知相位，
    /// 由交易部统一裁决。
    fn cancel_all_for(&mut self, tick: u64, owner: Owner) {
        let allowed = self.setup.allows_cancel(tick);
        let targets: Vec<DeskOrder> = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|l| l.iter())
            .filter(|o| o.owner == owner)
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }
        if !allowed {
            for o in targets {
                self.receipts_out[owner.0 as usize].push(DeskReceipt {
                    order: o.order_ref,
                    owner,
                    kind: ReceiptKind::CancelWindowClosed,
                });
            }
            return;
        }
        for o in targets {
            self.note_terminated(&o, super::protocol::LifecycleKind::Cancelled);
            self.remove_order(&o);
            self.close_order_entities(&o);
            self.cancelled_orders += 1;
            self.undo.push(DeskUndo::Cancelled(1));
        }
    }

    /// 定向撤单：命中撤余量；未命中=已全成（显式业务回执）。
    fn cancel_one(&mut self, tick: u64, owner: Owner, order: OrderRef) -> Result<(), RuntimeError> {
        if order.0 != self.setup.stock {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("交易部 {} 收到他股撤单：order={order:?}", self.setup.stock),
            });
        }
        let found: Option<DeskOrder> = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|l| l.iter())
            .find(|o| o.order_ref == order)
            .cloned();
        let kind = match &found {
            Some(o) if o.owner != owner => Some(ReceiptKind::OrderNotFound),
            Some(_) if !self.setup.allows_cancel(tick) => Some(ReceiptKind::CancelWindowClosed),
            _ => None,
        };
        if let Some(kind) = kind {
            self.receipts_out[owner.0 as usize].push(DeskReceipt { order, owner, kind });
            return Ok(());
        }
        match found {
            Some(o) => {
                self.note_terminated(&o, super::protocol::LifecycleKind::Cancelled);
                self.remove_order(&o);
                self.close_order_entities(&o);
                self.cancelled_orders += 1;
                self.undo.push(DeskUndo::Cancelled(1));
                Ok(())
            }
            None => {
                self.receipts_out[owner.0 as usize].push(DeskReceipt {
                    order,
                    owner,
                    kind: ReceiptKind::AlreadyFilled,
                });
                Ok(())
            }
        }
    }

    /// 受理拒绝：实体原路退回 + 回执（被拒单消耗序号）。
    fn reject(
        &mut self,
        _tick: u64,
        owner: Owner,
        side: Side,
        qty: u32,
        purse: i64,
        kind: ReceiptKind,
    ) {
        let g = owner.0 as usize;
        match side {
            Side::Buy => {
                self.out[g].push(Entity::CashRefund {
                    local: owner.1,
                    amount: purse,
                });
            }
            Side::Sell => {
                self.out[g].push(Entity::RefundedShares {
                    local: owner.1,
                    qty,
                });
            }
        }
        // 被拒单与回执共用同一 desk 序号（Closed 携带——组端按订单身份
        // 关联释放，Sol 二审修复集 3）。
        self.out[g].push(Entity::Closed {
            local: owner.1,
            order_serial: self.next_serial,
        });
        let order = (self.setup.stock, self.next_serial);
        self.next_serial += 1;
        self.undo.push(DeskUndo::Serial);
        self.receipts_out[g].push(DeskReceipt { order, owner, kind });
        self.skipped_orders += 1;
        self.undo.push(DeskUndo::Skipped(1));
    }

    /// 到期撤销：tick 起点先于本 tick 新意图（对齐 ExpiryShadow 相位时点）。
    fn expire_due(&mut self, tick: u64) {
        let due: Vec<DeskOrder> = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|l| l.iter())
            .filter(|o| tick >= o.expire_tick)
            .cloned()
            .collect();
        for o in due {
            self.note_terminated(&o, super::protocol::LifecycleKind::Expired);
            self.remove_order(&o);
            self.close_order_entities(&o);
            self.expired_orders += 1;
            self.undo.push(DeskUndo::Expired(1));
        }
    }

    /// 开盘集合竞价一次清算（相位离开申报窗的首个 tick 触发）：
    /// 沪深选价（`select_clearing` 复用）→ 按清算价 FIFO 配对 → 余单保留原档转连续。
    fn execute_opening_clearing(&mut self) -> Result<Option<(i64, u64)>, RuntimeError> {
        self.opening_cleared = true;
        let orders: Vec<ClearingOrder> = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|l| l.iter())
            .map(|o| ClearingOrder {
                side: o.side,
                limit: Money::from_cents(o.price),
                qty: o.qty as u64,
            })
            .collect();
        let selection = select_clearing(
            &orders,
            Money::from_cents(self.market.last_close),
            self.setup.exchange,
            Money::from_cents(1),
        )
        .map_err(|e| RuntimeError::ProtocolInvariant {
            detail: format!("开盘清算选价失败：{e:?}"),
        })?;
        // D4（t4 事件面对等）：完结 tick 先产末次指示价事实——
        // AuctionTick 先于 AuctionCompleted（旧管线在竞价窗每 tick
        // 产指示价，完结 tick 含末次；空簿=无交叉，量价如实为零/None）。
        self.auction_facts.push(super::protocol::DeskAuctionFact {
            stock: self.setup.stock,
            tick: self.cur_tick,
            phase: crate::TradingPhase::CallAuction,
            indicative_price: selection.as_ref().map(|s| s.price.cents()),
            matched_volume: selection.as_ref().map(|s| s.volume).unwrap_or(0),
            completed: false,
            imbalance: selection.as_ref().map(|s| s.imbalance).unwrap_or(0),
        });
        let Some(sel) = selection else {
            // 空簿清算也产 Completed 事实（清算发生、量恰为零）+ 标记
            // 活动 K（close=最新价、量=0——与旧管线"无成交以收盘价标记"
            // 对齐；此前空簿既无事件也无 K 标记）。
            self.auction_facts.push(super::protocol::DeskAuctionFact {
                stock: self.setup.stock,
                tick: self.cur_tick,
                phase: crate::TradingPhase::CallAuction,
                indicative_price: None,
                matched_volume: 0,
                completed: true,
                imbalance: 0,
            });
            if self.market.candle.is_none() {
                let last = self.market.last_price;
                self.market.candle = Some(super::desk::DeskCandle {
                    open: last,
                    high: last,
                    low: last,
                    close: last,
                    volume: 0,
                });
            }
            return Ok(Some((self.market.last_price, 0)));
        };
        let price = sel.price.cents();
        // P1-2（Sol）：公开成交量=实际成交量（内部选择量只是上界——
        // 自单剔除后可配对量可低于选择量）。
        let actual = self.execute_clearing_at(price, sel.volume)?;
        self.auction_facts.push(super::protocol::DeskAuctionFact {
            stock: self.setup.stock,
            tick: self.cur_tick,
            phase: crate::TradingPhase::CallAuction,
            indicative_price: Some(price),
            matched_volume: actual,
            completed: true,
            imbalance: sel.imbalance,
        });
        Ok(Some((price, actual)))
    }

    /// 按清算价配对成交（买卖双方都从最优价 FIFO 取，eligible 按清算价过滤）。
    fn execute_clearing_at(&mut self, price: i64, volume: u64) -> Result<u64, RuntimeError> {
        let mut matched = 0u64;
        while matched < volume {
            let buy = {
                let best = self.bids.keys().next_back().copied();
                best.and_then(|p| {
                    if p >= price {
                        self.bids.get(&p).and_then(|l| l.front().cloned())
                    } else {
                        None
                    }
                })
            };
            let sell = {
                let best = self.asks.keys().next().copied();
                best.and_then(|p| {
                    if p <= price {
                        self.asks.get(&p).and_then(|l| l.front().cloned())
                    } else {
                        None
                    }
                })
            };
            let (Some(buy), Some(sell)) = (buy, sell) else {
                // 自成交排除后的可配对量可能低于选择量（同主单剔除）——
                // 允许提前结束（matched < volume 合法：选择量是上界）。
                break;
            };
            // A 股自成交限制（拍卖清算，索引判同主）：同 owner 配对
            // 只撤卖侧自单、不成交（买侧续找第三方流动性——语义见
            // 下方 P1-1 注释）——索引 O(1) 比较，无簿扫描。
            if buy.owner == sell.owner {
                // P1-1（Sol）：同主配对闭合资源。修正常次序问题：
                // 只撤卖侧自单（让买侧继续找下一非自卖单——成对双撤
                // 会把买侧可配对的第三方流动性一并消耗）；卖侧皆自单时
                // 买侧无对手，循环自然终止且资源全部闭合（Sol 二审：
                // 该终止与"双撤"只是资源闭合结论相同，行为面是
                // 只撤卖侧——措辞以此为准）。
                self.note_terminated(&sell, super::protocol::LifecycleKind::Cancelled);
                self.remove_order(&sell);
                self.close_order_entities(&sell);
                continue;
            }
            let qty = buy.qty.min(sell.qty);
            if qty == 0 || matched + qty as u64 > volume {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("清算数量超界：matched={matched}, qty={qty}, volume={volume}"),
                });
            }
            // 以买方为 taker、卖方为 maker 结算（费用按清算价双边计）。
            let mut taker = buy;
            let maker = sell;
            self.settle_fill(&mut taker, maker, qty)?;
            // taker 全成即闭合；部分成交写回簿（apply_maker_fill 只写 maker——
            // taker 也是簿内单，需对称写回）。
            self.write_back_taker(&taker)?;
            matched += qty as u64;
        }
        Ok(matched)
    }

    /// taker（簿内单）成交后写回：全成移除（slack 退回），否则原位更新。
    fn write_back_taker(&mut self, order: &DeskOrder) -> Result<(), RuntimeError> {
        if order.qty == 0 {
            self.remove_order(order);
            self.close_order_entities(order);
        } else {
            self.restore_fields(order);
        }
        Ok(())
    }

    /// 日界重置：昨收=最新价、日内高低/量清零、活动 K 入历史（F §6.2 步骤 5
    /// 的下一交易日视图；撤销日志的 Market 快照覆盖本重置——Abort 可回退）。
    fn roll_day(&mut self) {
        let m = &mut self.market;
        m.last_close = m.last_price;
        m.day_high = m.last_price;
        m.day_low = m.last_price;
        m.volume_today = 0;
        m.turnover_today = 0;
        m.trade_count = 0;
        if let Some(candle) = m.candle.take() {
            if candle.volume > 0 {
                m.traded_days_total = m.traded_days_total.saturating_add(1);
            }
            m.candles.push(candle);
        }
        m.minute_closes.clear();
    }

    /// 新 tick 起点：隐式转正 → （日界重置）→ 行情快照 → 开盘清算 → 到期撤销。
    fn begin_tick(&mut self, tick: u64) -> Result<(), RuntimeError> {
        // 下一 tick 首批 Ops(N) = tick N−1 隐式转正。
        self.undo.clear();
        let day_before = self.committed_tick / self.setup.ticks_per_day.max(1);
        let day_after = tick / self.setup.ticks_per_day.max(1);
        self.committed_tick = self.cur_tick;
        self.cur_tick = tick;
        self.cur_round = 0;
        if day_after > day_before {
            self.roll_day();
        }
        self.snapshot_market();
        // 开盘清算：上一 tick 还在申报窗、本 tick 已离开 → 09:25 一次撮合。
        let prev_phase = self.setup.phase_at(tick.saturating_sub(1));
        let phase = self.setup.phase_at(tick);
        if prev_phase == DeskPhase::CallAuction
            && phase != DeskPhase::CallAuction
            && !self.opening_cleared
        {
            self.execute_opening_clearing()?;
        }
        // D5 attention 公开可见性门禁：连续→收盘竞价切换时点记公开截点
        ///（老单公开、竞价新单隐藏——旧管线 set_matching_mode 的 cutoff
        /// 语义等价，desk 权威域承载）。
        if phase == DeskPhase::ClosingAuction
            && self.last_phase.is_some_and(|p| p != DeskPhase::ClosingAuction)
        {
            self.public_cutoff_serial = self.next_serial;
        }
        self.last_phase = Some(phase);
        // 到期撤销先于本 tick 新意图。
        self.expire_due(tick);
        // 批 8b 哈希惰性化：热路径 O(1) 指纹；全量留 Final/回滚冷路径。
        self.committed_hash = (0x9E37_79B9_7F4A_7C15u64 ^ tick).wrapping_mul(0x100000001b3);
        Ok(())
    }

    /// 竞价窗收束：产出指示价预演事实（AuctionTick——每轮虚拟撮合）。
    fn note_auction_indicative(&mut self, tick: u64) {
        let phase = self.setup.phase_at(tick);
        if phase != DeskPhase::CallAuction && phase != DeskPhase::ClosingAuction {
            return;
        }
        let orders: Vec<ClearingOrder> = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|l| l.iter())
            .map(|o| ClearingOrder {
                side: o.side,
                limit: Money::from_cents(o.price),
                qty: o.qty as u64,
            })
            .collect();
        let Ok(sel) = select_clearing(
            &orders,
            Money::from_cents(self.market.last_close),
            self.setup.exchange,
            Money::from_cents(1),
        ) else {
            return;
        };
        self.auction_facts.push(super::protocol::DeskAuctionFact {
            stock: self.setup.stock,
            tick,
            phase: match phase {
                DeskPhase::CallAuction => crate::TradingPhase::CallAuction,
                _ => crate::TradingPhase::ClosingAuction,
            },
            indicative_price: sel.map(|s| s.price.cents()),
            matched_volume: sel.map(|s| s.volume).unwrap_or(0),
            completed: false,
            imbalance: sel.map(|s| s.imbalance).unwrap_or(0),
        });
    }

    /// 本收束点公共态（协调者安装进下一 tick 广播）。
    fn build_public(&self, tick: u64) -> super::protocol::StockPublic {
        let (down, up_buy) = self
            .price_bound_for(Side::Buy, tick, self.config.price_cage_enabled)
            .unwrap_or((1, self.market.last_price));
        // D5 attention 公开可见性门禁（A 股制度面）：开盘集合竞价申报窗
        /// 公开簿=空（不揭示买卖盘）；收盘竞价只揭示截点前老单；盘前/
        /// 连续全公开（竞价清算后的揭示=簿全量）。desk 广播是组内观察
        /// （signal/imbalance）与路由（best/depth）的输入——公开面与
        /// 真实簿分离。
        let phase = self.setup.phase_at(tick);
        let public_orders: Vec<&DeskOrder> = match phase {
            DeskPhase::CallAuction => Vec::new(),
            DeskPhase::ClosingAuction => self
                .bids
                .values()
                .chain(self.asks.values())
                .flat_map(|level| level.iter())
                .filter(|order| order.order_ref.1 < self.public_cutoff_serial)
                .collect(),
            DeskPhase::PreOpen | DeskPhase::Continuous => self
                .bids
                .values()
                .chain(self.asks.values())
                .flat_map(|level| level.iter())
                .collect(),
        };
        let mut pub_bids: BTreeMap<i64, u64> = BTreeMap::new();
        let mut pub_asks: BTreeMap<i64, u64> = BTreeMap::new();
        for order in &public_orders {
            let level = match order.side {
                Side::Buy => &mut pub_bids,
                Side::Sell => &mut pub_asks,
            };
            *level.entry(order.price).or_default() += u64::from(order.qty);
        }
        let pub_best_bid = pub_bids.keys().next_back().copied();
        let pub_best_ask = pub_asks.keys().next().copied();
        let imbalance_public = {
            let bid: u64 = pub_bids.values().rev().take(5).sum();
            let ask: u64 = pub_asks.values().take(5).sum();
            let total = bid + ask;
            if total == 0 { 0.0 } else { (bid as f64 - ask as f64) / total as f64 }
        };
        super::protocol::StockPublic {
            last_price: self.market.last_price,
            best_bid: pub_best_bid,
            best_ask: pub_best_ask,
            max_buy_price: up_buy,
            daily_upper_limit: self.up_stop().unwrap_or(self.market.last_close),
            min_sell_price: down,
            recent_prices: self.market.recent_prices.iter().copied().collect(),
            minute_closes: self.market.minute_closes.clone(),
            // D3a-2 观察基：已完成日 K 尾部 250（不过滤）+绝对计数。
            daily_bars: {
                let mut bars: Vec<(i64, i64, i64, i64, u64)> = self
                    .market
                    .candles
                    .iter()
                    .map(|c| (c.open, c.high, c.low, c.close, c.volume))
                    .collect();
                let len = bars.len();
                if len > 250 {
                    bars.drain(..len - 250);
                }
                bars
            },
            daily_active: self
                .market
                .candle
                .as_ref()
                .map(|c| (c.open, c.high, c.low, c.close, c.volume)),
            daily_total_completed: self.market.candles.len() as u64,
            daily_total_traded: self.market.traded_days_total,
            volume_today: self.market.volume_today,
            relative_volume: 1.0,
            order_book_imbalance: imbalance_public,
            public_cutoff_serial: self.public_cutoff_serial,
            depth_bids: pub_bids
                .iter()
                .rev()
                .take(5)
                .map(|(price, qty)| (*price, *qty))
                .collect(),
            depth_asks: pub_asks
                .iter()
                .take(5)
                .map(|(price, qty)| (*price, *qty))
                .collect(),
        }
    }

    /// 前五档买卖失衡（照搬原型 `top5_imbalance`）。
    fn top5_imbalance(&self) -> f64 {
        let bid: u64 = self.bids.values().rev().take(5).map(count_qty).sum();
        let ask: u64 = self.asks.values().take(5).map(count_qty).sum();
        let total = bid + ask;
        if total == 0 {
            0.0
        } else {
            (bid as f64 - ask as f64) / total as f64
        }
    }

    /// 本轮收束：行情收尾（tick 收盘价历史、分钟收盘、零成交 K 占位）→
    /// 出口缓冲 flush（先于 DeskReport，因果基石）。
    fn finish_round(&mut self, tick: u64) -> Result<(), RuntimeError> {
        // 零成交也建立占位 K（对齐 record_trade_or_mark 的 or_insert 语义）。
        let last = self.market.last_price;
        let candle = self.market.candle.get_or_insert(DeskCandle {
            open: last,
            high: last,
            low: last,
            close: last,
            volume: 0,
        });
        candle.close = last;
        // 竞价窗：指示价预演上行（每轮 AuctionTick 事实）。
        self.note_auction_indicative(tick);
        // tick 收盘价历史（容量裁剪）。
        self.market.recent_prices.push_back(last);
        if self.market.recent_prices.len() > self.setup.history_len {
            self.market.recent_prices.pop_front();
        }
        // 分钟收盘：连续竞价相位按完成分钟回填（无成交分钟沿用当时最新价）。
        // 收盘竞价段同填（存档口径 persistence.rs 把收盘段的连续窗算满
        //——min(elapsed, 连续窗) 钳制；此前收盘段不填，tick7 save 的
        // fault2018（缺末段分钟）显红）。
        let phase_now = self.setup.phase_at(tick);
        if phase_now == DeskPhase::Continuous || phase_now == DeskPhase::ClosingAuction {
            let continuous = self.setup.continuous_ticks_per_day();
            if continuous > 0 {
                let day = tick / self.setup.ticks_per_day;
                // 存档校验口径（persistence.rs）：completed = day×连续窗 +
                // (tick%tpd − auction).min(连续窗)——无 +1（完成 tick 语义）。
                let elapsed = (tick % self.setup.ticks_per_day)
                    .saturating_sub(self.setup.auction_ticks)
                    .min(continuous);
                let completed =
                    completed_market_minute_count(elapsed, continuous).map_err(|e| {
                        RuntimeError::ProtocolInvariant {
                            detail: format!("分钟映射失败：{e:?}"),
                        }
                    })?;
                // P1-6（Sol）：键=绝对分钟（day×240+日内序），容量口径=
                // 日内 completed（跨日 roll_day 已清空 ⇒ 第二日从 0 重数，
                // 键重新带当日偏移——修复"绝对键当日内长度"的双计）。
                let day_base = day * 240;
                let target_local = completed as u64;
                while (self.market.minute_closes.len() as u64) < target_local {
                    let idx = self.market.minute_closes.len() as u64;
                    self.market.minute_closes.push((day_base + idx, last));
                }
            }
        }
        Ok(())
    }

    /// 本轮收束 flush：实体/成交事实/回执聚合批发组线程。
    fn flush_batches(
        &mut self,
        group_txs: &[Sender<super::protocol::GroupIn>],
        tick: u64,
        round: u32,
    ) -> Result<(u64, u64, u64), RuntimeError> {
        let mut entities_total = 0u64;
        let mut fills_total = 0u64;
        let mut receipts_total = 0u64;
        let stock = self.setup.stock;
        let out = std::mem::take(&mut self.out);
        for (g, entities) in out.into_iter().enumerate() {
            let fills = std::mem::take(&mut self.fills_out[g]);
            let receipts = std::mem::take(&mut self.receipts_out[g]);
            if entities.is_empty() && fills.is_empty() && receipts.is_empty() {
                continue;
            }
            entities_total += entities.len() as u64;
            fills_total += fills.len() as u64;
            receipts_total += receipts.len() as u64;
            let batch = DeskBatch {
                stock,
                tick,
                round,
                entities,
                fills,
                receipts,
            };
            group_txs[g]
                .send(super::protocol::GroupIn::Batch(batch))
                .map_err(|_| RuntimeError::SendFailed {
                    detail: format!("交易部 {stock} → 组 {g} 实体批次发送失败"),
                })?;
        }
        // take 走的 out 需重建空缓冲（后续轮次/Drain 继续累积）。
        self.out = vec![Vec::new(); group_txs.len()];
        Ok((entities_total, fills_total, receipts_total))
    }

    /// 强撤全部挂单（Drain 日终清簿）。收盘竞价先一次清算再清余单。
    fn drain_all(&mut self) -> Result<Option<(i64, u64)>, RuntimeError> {
        if self.setup.phase_at(self.cur_tick) == DeskPhase::ClosingAuction && !self.opening_cleared
        {
            self.opening_cleared = true;
            let orders: Vec<ClearingOrder> = self
                .bids
                .values()
                .chain(self.asks.values())
                .flat_map(|l| l.iter())
                .map(|o| ClearingOrder {
                    side: o.side,
                    limit: Money::from_cents(o.price),
                    qty: o.qty as u64,
                })
                .collect();
            let selection = select_clearing(
                &orders,
                Money::from_cents(self.market.last_close),
                self.setup.exchange,
                Money::from_cents(1),
            )
            .map_err(|e| RuntimeError::ProtocolInvariant {
                detail: format!("收盘清算选价失败：{e:?}"),
            })?;
            if let Some(sel) = selection {
                let price = sel.price.cents();
                let actual = self.execute_clearing_at(price, sel.volume)?;
                return Ok(Some((price, actual)));
            }
            return Ok(None);
        }
        let targets: Vec<DeskOrder> = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|l| l.iter())
            .cloned()
            .collect();
        for o in targets {
            self.note_terminated(&o, super::protocol::LifecycleKind::Cancelled);
            self.remove_order(&o);
            self.close_order_entities(&o);
        }
        Ok(None)
    }

    /// K 线投影（回写 candle_book 用）。
    fn candle_projection(
        &self,
    ) -> (
        Vec<(i64, i64, i64, i64, i64, u64, i64, u64)>,
        Option<(i64, i64, i64, i64, i64, u64, i64, u64)>,
    ) {
        let hist = self
            .market
            .candles
            .iter()
            .map(|c| (0i64, c.open, c.high, c.low, c.close, c.volume, 0, 0))
            .collect();
        let active = self.market.candle.as_ref().map(|c| {
            (
                0i64,
                c.open,
                c.high,
                c.low,
                c.close,
                c.volume,
                self.market.turnover_today,
                self.market.trade_count,
            )
        });
        (hist, active)
    }

    /// 期末上交。
    fn final_state(&self) -> DeskFinalState {
        DeskFinalState {
            stock: self.setup.stock,
            resting: self
                .bids
                .values()
                .chain(self.asks.values())
                .flat_map(|l| l.iter())
                .cloned()
                .collect(),
            skipped_orders: self.skipped_orders,
            expired_orders: self.expired_orders,
            cancelled_orders: self.cancelled_orders,
            fee_sink: self.fee_sink,
            market: self.market.clone(),
            state_hash: self.state_hash(),
            committed_hash: self.committed_hash,
            rollback_hash: self.rollback_hash,
        }
    }
}

/// 边际费用 helper（与 fees.rs fee_delta 同口径）。
fn marginal(
    config: &GameConfig,
    before: Money,
    after: Money,
    calc: fn(&GameConfig, Money) -> Result<Money, crate::money::MoneyError>,
) -> Result<Money, crate::money::MoneyError> {
    let before_fee = if before == Money::ZERO {
        Money::ZERO
    } else {
        calc(config, before)?
    };
    calc(config, after)?.sub(before_fee)
}

fn over(msg: &str) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: msg.to_owned(),
    }
}

/// 价格边界计算（照搬 `market.rs price_bound`：基准 × 比率后正数四舍五入至
/// 最小价位 0.01 元，不足一档至少移动一档）。交易部与世界初始视图共用。
pub fn bound_price(reference: Money, ratio_bps: u32, upward: bool) -> Result<Money, String> {
    let tick = Money::from_cents(1);
    let denominator = i128::from(10_000u32) * i128::from(tick.cents());
    let numerator = i128::from(reference.cents()) * i128::from(ratio_bps);
    let rounded_ticks = (numerator + denominator / 2) / denominator;
    let rounded_cents =
        i64::try_from(rounded_ticks * i128::from(tick.cents())).map_err(|e| format!("{e}"))?;
    let mut bound = Money::from_cents(rounded_cents.max(tick.cents()));
    if upward && bound <= reference {
        bound = reference.add(tick).map_err(|e| format!("{e}"))?;
    } else if !upward && bound >= reference {
        bound = reference.sub(tick).map_err(|e| format!("{e}"))?.max(tick);
    }
    Ok(bound)
}

fn count_qty(level: &VecDeque<DeskOrder>) -> u64 {
    level.iter().map(|o| o.qty as u64).sum()
}

fn fnv(state: u64, value: u64) -> u64 {
    let mut h = state;
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 交易部主循环（批 3 引擎化）。
pub fn desk_main(
    mut setup: DeskSetup,
    n_groups: usize,
    config: Arc<GameConfig>,
    rx: Receiver<DeskMsg>,
    group_txs: Vec<Sender<super::protocol::GroupIn>>,
    up_tx: Sender<UpMsg>,
    abort: Arc<AtomicBool>,
    watchdog: WatchdogConfig,
    capture: Arc<std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>>,
) -> Result<(), RuntimeError> {
    /// D4：消息处理期安装捕获绑定（无捕获零开销；上报在绑定释放后）。
    fn with_capture<R>(
        capture: &Arc<std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>>,
        work: impl FnOnce() -> R,
    ) -> R {
        let binding = capture
            .lock()
            .expect("capture 槽锁被 poison")
            .clone();
        crate::verification_evidence::with_phase_timing_binding(binding, work)
    }
    let who = WorkerId::Desk(setup.stock);
    let initial_orders = std::mem::take(&mut setup.initial_orders);
    let initial_recent_prices = std::mem::take(&mut setup.initial_recent_prices);
    let initial_market = setup.initial_market.take();
    let initial_public_cutoff = setup.initial_public_cutoff;
    let initial_tick = setup.initial_tick;
    let mut state = DeskState::new(setup, n_groups, config);
    // D5 restore 等价：种子 tick 价格历史（容量裁剪）。
    for price in initial_recent_prices {
        state.market.recent_prices.push_back(price.max(1));
        if state.market.recent_prices.len() > state.setup.history_len {
            state.market.recent_prices.pop_front();
        }
    }
    // D5 adopt：旧簿在途单直入初始簿（不撮合；受理事实按种子标记——
    /// live 构造期 adopt 照记 Accepted（对齐旧管线 enqueue 后首拍交付
    /// 面），restore 形态不重放（save 前事件历史已含，重发即重复）；
    /// serial 序=构造期前段，与消息链首单同起 0）。
    for seed in initial_orders {
        let order_ref = (state.setup.stock, state.next_serial);
        let order = DeskOrder {
            order_ref,
            owner: seed.owner,
            side: seed.side,
            price: seed.price,
            qty: seed.qty,
            purse: seed.purse,
            filled_value: 0,
            charged_commission: 0,
            charged_stamp_tax: 0,
            charged_transfer_fee: 0,
            expire_tick: initial_tick.saturating_add(state.setup.order_ttl_ticks),
        };
        if seed.accepted_delivered {
            state.rest_silent(order);
        } else {
            state.rest(order);
        }
    }
    // D5 attention：公开截点种子（restore 链——desk 权威自此起步）。
    state.public_cutoff_serial = initial_public_cutoff;
    // D5 restore 链最后一层：日内行情聚合种子装载（save 点权威——
    /// 续拍在 save 聚合之上演进，不再从构造零点覆盖）。
    if let Some(seed) = initial_market {
        let m = &mut state.market;
        m.last_price = seed.last_price;
        m.last_close = seed.last_close;
        m.day_high = seed.day_high;
        m.day_low = seed.day_low;
        m.volume_today = seed.volume_today;
        m.turnover_today = seed.turnover_today;
        m.trade_count = seed.trade_count;
        m.candle = seed
            .candle
            .map(|(open, high, low, close, volume)| DeskCandle {
                open,
                high,
                low,
                close,
                volume,
            });
        m.minute_closes = seed.minute_closes;
    }
    let mut result = Ok(());
    loop {
        let msg = match worker_recv(&rx, &watchdog, &abort, who) {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                abort.store(true, Ordering::SeqCst);
                let _ = up_tx.send(UpMsg::Failed {
                    who,
                    error: e.clone(),
                });
                result = Err(e);
                break;
            }
        };
        if matches!(msg, DeskMsg::EchoCandles) {
            let candles = state.candle_projection();
            if up_tx
                .send(UpMsg::DeskCandles {
                    stock: state.setup.stock,
                    history: candles.0,
                    active: candles.1,
                })
                .is_err()
            {
                abort.store(true, Ordering::SeqCst);
                result = Err(RuntimeError::SendFailed {
                    detail: format!("交易部 EchoCandles 发送失败"),
                });
                break;
            }
            continue;
        }
        match &msg {
            DeskMsg::Shutdown => break,
            _ => {}
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_capture(&capture, || process_msg(&mut state, &group_txs, &up_tx, msg))
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                abort.store(true, Ordering::SeqCst);
                let _ = up_tx.send(UpMsg::Failed {
                    who,
                    error: e.clone(),
                });
                result = Err(e);
                break;
            }
            Err(payload) => {
                state.rollback();
                state.rollback_hash = Some(state.state_hash());
                abort.store(true, Ordering::SeqCst);
                let _ = up_tx.send(UpMsg::TaskDied { who, payload });
                break;
            }
        }
    }
    let final_state = state.final_state();
    let send = up_tx.send(UpMsg::DeskFinal(Box::new(final_state)));
    if result.is_ok() && send.is_err() {
        result = Err(RuntimeError::SendFailed {
            detail: format!("交易部 {} DeskFinal 发送失败", who),
        });
    }
    result
}

/// 单消息处理（panic 运输保护范围）。
fn process_msg(
    state: &mut DeskState,
    group_txs: &[Sender<super::protocol::GroupIn>],
    up_tx: &Sender<UpMsg>,
    msg: DeskMsg,
) -> Result<(), RuntimeError> {
    match msg {
        DeskMsg::EchoResting => {
            // D5 envelope 投影：在途单快照（bids+asks 全量——费用
            // 累计/余量直取 desk 权威）。
            let mut orders = Vec::new();
            for (side, book) in [(Side::Buy, &state.bids), (Side::Sell, &state.asks)] {
                for (_price, level) in book.iter() {
                    for order in level.iter() {
                        orders.push(super::protocol::RestingFact {
                            owner: order.owner,
                            side,
                            price: order.price,
                            qty: order.qty,
                            purse: order.purse,
                            filled_value: order.filled_value,
                            charged_commission: order.charged_commission,
                            charged_stamp_tax: order.charged_stamp_tax,
                            charged_transfer_fee: order.charged_transfer_fee,
                            order_serial: order.order_ref.1,
                        });
                    }
                }
            }
            if up_tx
                .send(UpMsg::DeskResting {
                    stock: state.setup.stock,
                    orders,
                })
                .is_err()
            {
                return Err(RuntimeError::SendFailed {
                    detail: "DeskResting 上行失败".to_owned(),
                });
            }
        }
        DeskMsg::Start { tick } => {
            // D4：tick 起点（先于同 tick Ops，FIFO）——每 tick 必推进
            // begin_tick：到期/清算/快照不再依赖 Ops 到达。
            if tick > state.cur_tick {
                if tick != state.cur_tick + 1 {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} Start tick 跳变：tick={tick}, 上一执行 tick={}",
                            state.setup.stock, state.cur_tick
                        ),
                    });
                }
                crate::verification_evidence::enter_phase(
                    crate::session::pipeline::TickPhase::ExpiryShadow,
                );
                state.begin_tick(tick)?;
                crate::verification_evidence::enter_phase(
                    crate::session::pipeline::TickPhase::StockProcessing,
                );
            }
            // tick == cur_tick：Ops 已自行 begin（防御路径），无操作。
        }
        DeskMsg::Ops {
            tick,
            round,
            group,
            ops,
            final_flush,
        } => {
            if tick > state.cur_tick {
                if tick != state.cur_tick + 1 {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} Ops tick 跳变：tick={tick}, 上一执行 tick={}",
                            state.setup.stock, state.cur_tick
                        ),
                    });
                }
                // D4 相位：到期撤销段（begin_tick 内 expire_due/清算/快照）。
                crate::verification_evidence::enter_phase(
                    crate::session::pipeline::TickPhase::ExpiryShadow,
                );
                state.begin_tick(tick)?;
            } else if tick < state.cur_tick {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "交易部 {} 收到过期 tick 消息：{tick} < {}",
                        state.setup.stock, state.cur_tick
                    ),
                });
            }
            if round != state.cur_round {
                if round == state.cur_round + 1 {
                    state.cur_round = round;
                    for f in &mut state.flushed {
                        *f = false;
                    }
                } else {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} Ops 轮次错序：round={round}, 当前={}",
                            state.setup.stock, state.cur_round
                        ),
                    });
                }
            }
            let g = group as usize;
            if g >= state.flushed.len() {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("交易部 {} 收到越界组索引：{group}", state.setup.stock),
                });
            }
            // D4 相位：撮合处理段（ops 应用+清算收尾）。
            crate::verification_evidence::enter_phase(
                crate::session::pipeline::TickPhase::StockProcessing,
            );
            state.apply_ops(tick, ops)?;
            if final_flush {
                if state.flushed[g] {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} 组 {group} 第 ({tick},{round}) 轮重复 final_flush",
                            state.setup.stock
                        ),
                    });
                }
                state.flushed[g] = true;
                if state.flushed.iter().all(|&f| f) {
                    for f in &mut state.flushed {
                        *f = false;
                    }
                    // 行情收尾 → 实体批次 flush（先于 DeskReport，因果基石）→ 上报。
                    state.finish_round(tick)?;
                    let receipts_detail: Vec<DeskReceipt> = state
                        .receipts_out
                        .iter()
                        .flat_map(|b| b.iter().copied())
                        .collect();
                    let (entities_out, fills_out, receipts_out) =
                        state.flush_batches(group_txs, tick, round)?;
                    let report = DeskReport {
                        stock: state.setup.stock,
                        tick,
                        round,
                        resting_count: state.resting_count,
                        entities_out,
                        receipts_out,
                        committed_hash: state.committed_hash,
                        fills_out,
                        last_price: state.market.last_price,
                        day_high: state.market.day_high,
                        day_low: state.market.day_low,
                        volume_today: state.market.volume_today,
                        trade_count: state.market.trade_count,
                        fee_sink: state.fee_sink,
                        view: state.build_public(tick),
                        trades: std::mem::take(&mut state.trade_facts),
                        lifecycle: std::mem::take(&mut state.lifecycle_facts),
                        receipts_out_list: receipts_detail,
                        auction: std::mem::take(&mut state.auction_facts),
                    };
                    up_tx.send(UpMsg::DeskReport(report)).map_err(|_| {
                        RuntimeError::SendFailed {
                            detail: format!("交易部 {} DeskReport 发送失败", state.setup.stock),
                        }
                    })?;
                }
            }
        }
        DeskMsg::Abort { tick } => {
            if tick != state.cur_tick {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "交易部 {} Abort tick 错配：{tick} != {}",
                        state.setup.stock, state.cur_tick
                    ),
                });
            }
            state.rollback();
            state.rollback_hash = Some(state.state_hash());
        }
        DeskMsg::Drain => {
            // 隐式转正上一 tick（日终已收敛）：清簿进独立撤销日志，
            // Abort 只恢复清簿前簿态（回到上一交易日收敛态）。
            state.undo.clear();
            state.committed_tick = state.cur_tick;
            state.drain_all()?;
            state.finish_round(state.cur_tick)?;
            state.flush_batches(group_txs, state.cur_tick, state.cur_round)?;
            up_tx
                .send(UpMsg::DeskDrained {
                    stock: state.setup.stock,
                    fee_sink: state.fee_sink,
                    last_price: state.market.last_price,
                    drained_lifecycle: std::mem::take(&mut state.lifecycle_facts),
                })
                .map_err(|_| RuntimeError::SendFailed {
                    detail: format!("交易部 {} DeskDrained 发送失败", state.setup.stock),
                })?;
        }
        // Shutdown 由主循环拦截，此处防御保留（不可达）。
        DeskMsg::Shutdown => {}
        // EchoCandles 由主循环拦截，此处防御保留（不可达）。
        DeskMsg::EchoCandles => {}
    }
    Ok(())
}
