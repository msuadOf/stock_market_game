//! 所有权转移臂（第三原型，用户模型原话要旨）：
//!
//! NPC 分组持线程（分组持一组一线程），组线程持有组内全部现金/持仓。
//! 决策时从持有账户把相关的钱/股**实际扣出**，打包成资源实体（CashPurse /
//! ShareLot）经 channel 直发目标股交易部线程——所有权随实体转移，不是
//! escrow 预留。交易部一股一线程，收到实体后独占撮合（零锁零共享账本）：
//! 成交时实体互换（买方得股票实体、卖方得现金净额实体），结果批次回组线程；
//! 失败/撤单/到期时实体原路退回。tick 收束 = 全部交易部收齐全部组的
//! 提交标记后上报 coordinator。
//!
//! 与跨股中转臂（xworker）的核心区别：xworker 是家端预留（reserved_cash）
//! + 席位镜像 + 回执记账；本臂 NPC 账户在挂单期间**不含**该笔资金/股票
//! （决策可见余额 = npc.cash，严格所有权口径——在途退款的差额由 skip
//! 计数显式暴露，不静默掩盖）。
//!
//! 时序关键不变量（正确性基石）：交易部在 DeskDone 之前 flush 全部实体
//! 批次，coordinator 收齐全部 DeskDone 之后才广播下一 Start ⇒ 组线程收件
//! 箱中 tick N 的全部实体先于 Start(N+1) 入队（mpsc 发送序 = 入队序，
//! happens-before 链保证），T+1 解冻放在 Start 处理开头是安全的。

use crate::npc::{FeeTable, Npc, XPos};
use crate::setup::ProtoConfig;
use crate::worker::{MarketSnapshot, RoutedPrice, TickDone};
use crate::ProtoError;
use engine::account::StockCode;
use engine::money::Money;
use engine::session::SplitMix64;
use engine::strategy::{decide_data, Intent, MarketView, PositionView, SelfView, StockView};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// 资源所有者身份：(组索引, 组内名册下标)。实体回流时原样回带，
/// 组线程零查找落账。
pub type Owner = (u8, u32);

/// 组线程 → 交易部线程的操作。同一批次（及同一 channel 的先后批次）内
/// FIFO：同 NPC 的 CancelAll 必先于其 Place（对齐语义）。
#[derive(Clone, Copy, Debug)]
pub enum DeskIn {
    /// 对齐撤单：撤该 owner 在本交易部的全部挂单（实体原路回流）。
    CancelAll { owner: Owner },
    /// 下单：买单 `purse` = 现金实体（value + 佣金上限，已从 NPC 账户
    /// 实际扣除）；卖单 `purse` = 0、qty 股已从持仓实际扣除。
    /// 价格符号在目标股交易部受理时解析（组线程看不到目标股活簿）。
    Place {
        owner: Owner,
        side: engine::orderbook::Side,
        price: RoutedPrice,
        qty: u32,
        purse: i64,
    },
}

/// 交易部 → 组线程的资源实体（成交所得 / 买单余额 / 新股票 / 卖单余股 /
/// 终结标记）。所在批次携带股索引（DeskBatch.stock）。
#[derive(Clone, Copy, Debug)]
pub enum Entity {
    /// 卖出成交净额（value − 佣金 − 印花税）：现金实体回流所有者。
    CashProceeds { local: u32, amount: i64 },
    /// 买单终结（全成/撤/到期/受理拒绝）：purse 余额回流。
    CashRefund { local: u32, amount: i64 },
    /// 买入成交：股票实体回流（组端 T+1 冻结入账）。
    BoughtShares { local: u32, qty: u32 },
    /// 卖单终结：未成交股数回流（组端恢复可卖）。
    RefundedShares { local: u32, qty: u32 },
    /// 挂单终结标记：组端在途登记递减（零额终结也必须发，否则计数泄漏）。
    Closed { local: u32 },
}

/// 交易部 → 组线程的一个实体批次（本 tick 内该交易部产生的全部实体）。
pub struct DeskBatch {
    /// 发方交易部的股索引（组端 primary/secondary 判定与在途登记用）。
    pub stock: u8,
    pub entities: Vec<Entity>,
}

/// 组线程收件消息（coordinator 的 Start 与交易部的实体批次共用一个
/// mpsc；如上所述因果序保证批次先于下一 Start 被处理）。
pub enum GroupIn {
    Start(Arc<MarketSnapshot>),
    Batch(DeskBatch),
    Shutdown,
}

/// 交易部收件消息。
pub enum DeskMsg {
    /// 一组本 tick 的一批操作。`final_flush` = 该组本 tick 最后一批
    /// （可能为空——作为组的提交完成标记）；交易部收齐全部组的
    /// final_flush 即本 tick 撮合输入完毕。
    Ops {
        tick: u64,
        group: u8,
        ops: Vec<DeskIn>,
        final_flush: bool,
    },
    /// 收尾：强撤全部挂单，实体回流后上报（期末审计前清空在途实体）。
    Drain,
}

/// worker → coordinator 的上行消息（组与交易部共用一个 mpsc）。
/// 手写 Debug：负载类型不透明，只报告变体名（coordinator 错误信息用）。
impl std::fmt::Debug for OwnUp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OwnUp::GroupStats { group, .. } => write!(f, "GroupStats(group={group})"),
            OwnUp::DeskDone(_) => write!(f, "DeskDone(..)"),
            OwnUp::DeskDrained => write!(f, "DeskDrained"),
            OwnUp::GroupFinal(_) => write!(f, "GroupFinal(..)"),
            OwnUp::DeskFinal(_) => write!(f, "DeskFinal(..)"),
            OwnUp::Failed(who, e) => write!(f, "Failed({who}, {e})"),
        }
    }
}

pub enum OwnUp {
    GroupStats {
        group: u8,
        decide_calls: u64,
        intents: u64,
    },
    DeskDone(TickDone),
    DeskDrained,
    GroupFinal(Box<GroupFinal>),
    DeskFinal(Box<DeskFinal>),
    /// 任一 worker 显式失败上抛（铁律二：不静默；尽力发送，通道已断则
    /// 由收件超时 + abort 兜底）。
    Failed(u8, String),
}

/// 组线程期末状态。
pub struct GroupFinal {
    pub group: u8,
    pub roster: Vec<Npc>,
    /// 组端拒绝数（资金/可卖不足——严格所有权口径下的在途退款差额）。
    pub skipped_orders: u64,
    pub cancel_intents: u64,
    /// 目标 ≠ home 的下单数（跨股流量）。
    pub cross_places: u64,
    /// 目标 = home 的下单数。
    pub home_places: u64,
    /// 对齐撤单消息数。
    pub align_cancels: u64,
    /// 组→交易部批次消息总数（含 final 空标记）。
    pub flushes_sent: u64,
}

/// 交易部期末状态。
pub struct DeskFinal {
    pub stock_idx: usize,
    pub fee_sink: i64,
    pub trade_count: u64,
    pub volume_shares: u64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub min_trade_price: Option<i64>,
    pub max_trade_price: Option<i64>,
    pub entities: EntCounters,
    /// 交易部→组批次消息总数。
    pub batches_sent: u64,
}

/// 实体计数（报告用）。
#[derive(Default, Clone, Copy, Debug)]
pub struct EntCounters {
    pub cash_proceeds: u64,
    pub cash_refund: u64,
    pub bought_shares: u64,
    pub refunded_shares: u64,
    pub closed: u64,
}

/// 组/交易部共享的静态表（构造一次，Arc 分发）。
pub struct OwnShared {
    pub cfg: Arc<ProtoConfig>,
    pub game_cfg: Arc<engine::config::GameConfig>,
    pub codes: Vec<StockCode>,
    pub code_idx: HashMap<String, u8>,
    /// 每股 (lower, upper) 涨跌停带。
    pub bands: Vec<(i64, i64)>,
    pub initial_prices: Vec<i64>,
}

/// 分段 flush 粒度（NPC 数）：决策与撮合流水线重叠深度。环境变量
/// X_OWN_FLUSH 可调（需要正整数，非法值显式 panic 不静默）。
fn flush_chunk() -> usize {
    static V: OnceLock<usize> = OnceLock::new();
    *V.get_or_init(|| match std::env::var("X_OWN_FLUSH") {
        Ok(s) => s
            .parse()
            .unwrap_or_else(|_| panic!("X_OWN_FLUSH 需要正整数，收到 {s}")),
        Err(_) => 512,
    })
}

/// 收件循环超时（错误路径防挂死：任一 worker 失败后全体经 abort 退出，
/// 正常路径远快于该值）。
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

// ═══════════════════════════════════════════════════════════════════
// 交易部（一股一线程，实体独占撮合）
// ═══════════════════════════════════════════════════════════════════

/// 挂单（资源实体随单驻留交易部：买单 purse = 剩余可花现金；
/// 卖单 qty = 剩余股数）。
struct DeskOrder {
    id: u64,
    owner: Owner,
    side: engine::orderbook::Side,
    price: i64,
    qty: u32,
    expire_tick: u64,
    purse: i64,
    filled_value: i64,
    fee_charged: i64,
}

/// 实体出口（拆借用包：out 缓冲 + 计数一起传给自由函数）。
struct DeskSink<'a> {
    out: &'a mut [Vec<Entity>],
    cnt: &'a mut EntCounters,
}

impl DeskSink<'_> {
    fn push(&mut self, group: u8, e: Entity) {
        match &e {
            Entity::CashProceeds { .. } => self.cnt.cash_proceeds += 1,
            Entity::CashRefund { .. } => self.cnt.cash_refund += 1,
            Entity::BoughtShares { .. } => self.cnt.bought_shares += 1,
            Entity::RefundedShares { .. } => self.cnt.refunded_shares += 1,
            Entity::Closed { .. } => self.cnt.closed += 1,
        }
        self.out[group as usize].push(e);
    }
}

/// 撮合统计的分离借用包（与簿结构其余字段不相交；同 book.rs 手法）。
struct DeskFillStats<'a> {
    fee_sink: &'a mut i64,
    trade_count: &'a mut u64,
    volume_shares: &'a mut u64,
    last_price: &'a mut Option<i64>,
    min_trade_price: &'a mut Option<i64>,
    max_trade_price: &'a mut Option<i64>,
}

/// 交易部簿：双边 BTreeMap 价格档 × FIFO，挂单携带资源实体。
pub struct Desk {
    stock_idx: u8,
    bids: BTreeMap<i64, VecDeque<DeskOrder>>,
    asks: BTreeMap<i64, VecDeque<DeskOrder>>,
    index: HashMap<u64, (engine::orderbook::Side, i64)>,
    by_owner: HashMap<Owner, Vec<u64>>,
    next_id: u64,
    expiry_queue: VecDeque<(u64, u64)>,
    n_groups: usize,
    /// 实体流出缓冲（每组一个，tick 收束时整体 flush）。
    out: Vec<Vec<Entity>>,
    pub fee_sink: i64,
    pub trade_count: u64,
    pub volume_shares: u64,
    pub last_price: Option<i64>,
    pub min_trade_price: Option<i64>,
    pub max_trade_price: Option<i64>,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub resting_count: u64,
    pub ent: EntCounters,
    pub batches_sent: u64,
}

impl Desk {
    fn new(stock_idx: u8, n_groups: usize) -> Self {
        Desk {
            stock_idx,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            index: HashMap::new(),
            by_owner: HashMap::new(),
            next_id: 1,
            expiry_queue: VecDeque::new(),
            n_groups,
            out: (0..n_groups).map(|_| Vec::new()).collect(),
            fee_sink: 0,
            trade_count: 0,
            volume_shares: 0,
            last_price: None,
            min_trade_price: None,
            max_trade_price: None,
            skipped_orders: 0,
            expired_orders: 0,
            cancelled_orders: 0,
            resting_count: 0,
            ent: EntCounters::default(),
            batches_sent: 0,
        }
    }

    fn best_bid(&self) -> Option<i64> {
        self.bids.keys().next_back().copied()
    }

    fn best_ask(&self) -> Option<i64> {
        self.asks.keys().next().copied()
    }

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

    /// 下单：解析价格 → 防御校验 → 对价撮合 → 剩余挂簿。
    /// 资金/可卖已在组端实体化扣除，交易部无需冻结步骤。
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        owner: Owner,
        side: engine::orderbook::Side,
        rp: RoutedPrice,
        qty: u32,
        purse: i64,
        tick: u64,
        ttl: u64,
        fees: &FeeTable,
        lower: i64,
        upper: i64,
    ) -> Result<(), ProtoError> {
        let price = resolve_rp(&rp, side, self.best_bid(), self.best_ask(), upper, lower);
        if price <= 0 || qty == 0 || qty % 100 != 0 || price < lower || price > upper {
            // 带外/非整手显式拒绝：实体原路退回（不静默吞资源）。
            self.skipped_orders += 1;
            let mut sink = DeskSink {
                out: &mut self.out,
                cnt: &mut self.ent,
            };
            reject_entities(&mut sink, owner, side, qty, purse);
            return Ok(());
        }
        if self.by_owner.contains_key(&owner) {
            if crate::xdbg() {
                eprintln!(
                    "[desk {} guard] owner={owner:?} 已有挂单 {:?}，tick 上下文 place(price={price}, qty={qty}, purse={purse})",
                    self.stock_idx,
                    self.by_owner.get(&owner),
                    purse = purse
                );
            }
            return Err(ProtoError::InvalidConfig(format!(
                "owner={owner:?} 在股票 {} 有活跃挂单又收到新单（对齐撤单缺失，实现 bug）",
                self.stock_idx
            )));
        }
        let mut inc = DeskOrder {
            id: 0,
            owner,
            side,
            price,
            qty,
            expire_tick: tick.saturating_add(ttl),
            purse,
            filled_value: 0,
            fee_charged: 0,
        };
        let remaining = self.match_incoming(&mut inc, fees)?;
        if remaining == 0 {
            // 全部成交：买单 slack（reserve − 累计扣费）回流；卖单无余股。
            let mut sink = DeskSink {
                out: &mut self.out,
                cnt: &mut self.ent,
            };
            close_entities(&mut sink, &inc);
            return Ok(());
        }
        inc.id = self.next_id;
        self.next_id += 1;
        let id = inc.id;
        let entry = (side, price);
        let expire_tick = inc.expire_tick;
        let side_map = if side == engine::orderbook::Side::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };
        side_map.entry(price).or_default().push_back(inc);
        self.index.insert(id, entry);
        self.by_owner.entry(owner).or_default().push(id);
        self.expiry_queue.push_back((expire_tick, id));
        self.resting_count += 1;
        Ok(())
    }

    /// 对价撮合：incoming 依次吃对面最优档，实体互换。返回剩余数量。
    fn match_incoming(&mut self, inc: &mut DeskOrder, fees: &FeeTable) -> Result<u32, ProtoError> {
        while inc.qty > 0 {
            let (crossed, level_price) = match inc.side {
                engine::orderbook::Side::Buy => match self.asks.keys().next() {
                    Some(&ask) => (ask <= inc.price, ask),
                    None => (false, 0),
                },
                engine::orderbook::Side::Sell => match self.bids.keys().next_back() {
                    Some(&bid) => (bid >= inc.price, bid),
                    None => (false, 0),
                },
            };
            if !crossed {
                break;
            }
            let Desk {
                bids,
                asks,
                index,
                by_owner,
                fee_sink,
                trade_count,
                volume_shares,
                last_price,
                min_trade_price,
                max_trade_price,
                resting_count,
                out,
                ent,
                ..
            } = self;
            let opposite = if inc.side == engine::orderbook::Side::Buy {
                asks
            } else {
                bids
            };
            let level = opposite.get_mut(&level_price).expect("上面已确认档位存在");
            while inc.qty > 0 {
                let q = match level.front() {
                    Some(maker) => inc.qty.min(maker.qty),
                    None => break,
                };
                let maker = level.front_mut().expect("front 已确认存在");
                let mut sink = DeskSink { out, cnt: ent };
                let mut stats = DeskFillStats {
                    fee_sink,
                    trade_count,
                    volume_shares,
                    last_price,
                    min_trade_price,
                    max_trade_price,
                };
                settle_desk(&mut sink, &mut stats, maker, inc, q, fees)?;
                inc.qty -= q;
                maker.qty -= q;
                if maker.qty == 0 {
                    let done = level.pop_front().expect("front 已确认存在");
                    index.remove(&done.id);
                    forget_by_owner(by_owner, done.owner, done.id);
                    *resting_count -= 1;
                    close_entities(&mut sink, &done);
                } else {
                    break; // maker 部分成交，incoming 已耗尽。
                }
            }
            if opposite.get(&level_price).is_some_and(|lvl| lvl.is_empty()) {
                opposite.remove(&level_price);
            }
        }
        Ok(inc.qty)
    }

    /// 对齐撤单：撤该 owner 全部挂单，实体原路回流。
    fn cancel_all_for(&mut self, owner: Owner) -> Result<u64, ProtoError> {
        let ids = match self.by_owner.remove(&owner) {
            Some(ids) => ids,
            None => return Ok(0),
        };
        let mut cancelled = 0u64;
        for id in ids {
            let Some((side, price)) = self.index.remove(&id) else {
                continue; // 已在撮合中离场，残留登记跳过。
            };
            let side_map = if side == engine::orderbook::Side::Buy {
                &mut self.bids
            } else {
                &mut self.asks
            };
            let level = side_map.get_mut(&price).expect("index 命中则档位必须存在");
            let pos = level
                .iter()
                .position(|o| o.id == id)
                .expect("index 命中则挂单必须在档内");
            let order = level.remove(pos).expect("position 已确认");
            self.resting_count -= 1;
            self.cancelled_orders += 1;
            cancelled += 1;
            let mut sink = DeskSink {
                out: &mut self.out,
                cnt: &mut self.ent,
            };
            close_entities(&mut sink, &order);
            if level.is_empty() {
                side_map.remove(&price);
            }
        }
        Ok(cancelled)
    }

    /// 到期撤销：expire_tick ≤ tick 的挂单全部撤出并回流实体。
    fn expire_due(&mut self, tick: u64) -> Result<(), ProtoError> {
        while let Some(&(expire_tick, id)) = self.expiry_queue.front() {
            if expire_tick > tick {
                break;
            }
            self.expiry_queue.pop_front();
            let Some((side, price)) = self.index.remove(&id) else {
                continue; // 已成交/已撤，残留队列项跳过。
            };
            let side_map = if side == engine::orderbook::Side::Buy {
                &mut self.bids
            } else {
                &mut self.asks
            };
            let level = side_map.get_mut(&price).expect("index 命中则档位必须存在");
            let pos = level
                .iter()
                .position(|o| o.id == id)
                .expect("index 命中则挂单必须在档内");
            let order = level.remove(pos).expect("position 已确认");
            self.resting_count -= 1;
            self.expired_orders += 1;
            forget_by_owner(&mut self.by_owner, order.owner, order.id);
            let mut sink = DeskSink {
                out: &mut self.out,
                cnt: &mut self.ent,
            };
            close_entities(&mut sink, &order);
            if level.is_empty() {
                side_map.remove(&price);
            }
        }
        Ok(())
    }

    /// 强撤全部挂单（Drain 收尾）。
    fn cancel_all(&mut self) -> Result<u64, ProtoError> {
        let owners: Vec<Owner> = self.by_owner.keys().copied().collect();
        let mut n = 0;
        for owner in owners {
            n += self.cancel_all_for(owner)?;
        }
        Ok(n)
    }

    /// 取走实体缓冲（tick 收束 / Drain 时 flush 给组线程）。
    fn take_out(&mut self) -> Vec<Vec<Entity>> {
        self.batches_sent += self.out.iter().filter(|v| !v.is_empty()).count() as u64;
        std::mem::replace(
            &mut self.out,
            (0..self.n_groups).map(|_| Vec::new()).collect(),
        )
    }
}

/// 受理拒绝的实体退回（带外价/非整手防御分支）。
fn reject_entities(
    sink: &mut DeskSink<'_>,
    owner: Owner,
    side: engine::orderbook::Side,
    qty: u32,
    purse: i64,
) {
    match side {
        engine::orderbook::Side::Buy => sink.push(
            owner.0,
            Entity::CashRefund {
                local: owner.1,
                amount: purse,
            },
        ),
        engine::orderbook::Side::Sell => sink.push(
            owner.0,
            Entity::RefundedShares {
                local: owner.1,
                qty,
            },
        ),
    }
    sink.push(owner.0, Entity::Closed { local: owner.1 });
}

/// 挂单终结实体：买单 purse 余额（slack）回流；卖单余股回流；
/// 终结标记恒发（零额也发，组端在途登记才能闭合）。
fn close_entities(sink: &mut DeskSink<'_>, order: &DeskOrder) {
    match order.side {
        engine::orderbook::Side::Buy => {
            if order.purse > 0 {
                sink.push(
                    order.owner.0,
                    Entity::CashRefund {
                        local: order.owner.1,
                        amount: order.purse,
                    },
                );
            }
        }
        engine::orderbook::Side::Sell => {
            if order.qty > 0 {
                sink.push(
                    order.owner.0,
                    Entity::RefundedShares {
                        local: order.owner.1,
                        qty: order.qty,
                    },
                );
            }
        }
    }
    sink.push(
        order.owner.0,
        Entity::Closed {
            local: order.owner.1,
        },
    );
}

/// 单笔成交结算（实体互换）：maker（挂单方）与 inc（在途方）互换
/// 现金/股票实体。卖方得现金净额实体，买方 purser 扣减、得股票实体；
/// 费用进 fee sink（守恒对手方桶）。
fn settle_desk(
    sink: &mut DeskSink<'_>,
    stats: &mut DeskFillStats<'_>,
    maker: &mut DeskOrder,
    inc: &mut DeskOrder,
    q: u32,
    fees: &FeeTable,
) -> Result<(), ProtoError> {
    let price = maker.price;
    let value = mul_price_qty(price, q)?;
    if maker.owner == inc.owner {
        // 工作单对齐后理论不可达；显式失败暴露实现 bug（铁律二）。
        return Err(ProtoError::InvalidConfig(format!(
            "自成交被拒绝：owner={:?} price={} qty={}",
            maker.owner, price, q
        )));
    }
    *stats.trade_count += 1;
    *stats.volume_shares += q as u64;
    *stats.last_price = Some(price);
    *stats.min_trade_price = Some(stats.min_trade_price.map_or(price, |p| p.min(price)));
    *stats.max_trade_price = Some(stats.max_trade_price.map_or(price, |p| p.max(price)));

    // 佣金增量（委托维度摊销；买腿跟买方、卖腿跟卖方——对齐 book.rs 修正后的口径）。
    let buy_fee_delta = if maker.side == engine::orderbook::Side::Sell {
        commission_step(fees, inc.filled_value, inc.fee_charged, value)?
    } else {
        commission_step(fees, maker.filled_value, maker.fee_charged, value)?
    };
    let sell_fee_delta = if maker.side == engine::orderbook::Side::Sell {
        commission_step(fees, maker.filled_value, maker.fee_charged, value)?
    } else {
        commission_step(fees, inc.filled_value, inc.fee_charged, value)?
    };
    let stamp = fees.stamp_tax(value)?;

    // 结算对手方解析：maker.side==Sell ⇒ maker 卖、inc 买；反之 inc 卖、maker 买。
    let (sell_order, buy_order) = if maker.side == engine::orderbook::Side::Sell {
        (&mut *maker, &mut *inc)
    } else {
        (&mut *inc, &mut *maker)
    };

    // 卖方：现金净额实体回流其组（股票实体扣减由调用方统一做：
    // match_incoming 的 inc.qty/maker.qty -= q——order.qty 即在途股票 lot）。
    let net = value
        .checked_sub(sell_fee_delta)
        .and_then(|v| v.checked_sub(stamp))
        .ok_or_else(|| overflow("sell net", value))?;
    let sell_group = sell_order.owner.0;
    sink.push(
        sell_group,
        Entity::CashProceeds {
            local: sell_order.owner.1,
            amount: net,
        },
    );
    // 买方：purse 扣 charge（value + 佣金增量），股票实体回流其组（T+1 冻结）。
    let charge = value
        .checked_add(buy_fee_delta)
        .ok_or_else(|| overflow("buy charge", value))?;
    buy_order.purse = buy_order.purse.checked_sub(charge).ok_or_else(|| {
        ProtoError::InvalidConfig(format!(
            "买单 purse 不足：owner={:?} charge={charge} purse={}（组端 purse 预留口径不一致）",
            buy_order.owner, buy_order.purse
        ))
    })?;
    sink.push(
        buy_order.owner.0,
        Entity::BoughtShares {
            local: buy_order.owner.1,
            qty: q,
        },
    );
    *stats.fee_sink += buy_fee_delta + sell_fee_delta + stamp;

    // 摊销状态回写到对应 order。
    if maker.side == engine::orderbook::Side::Sell {
        maker.filled_value += value;
        maker.fee_charged += sell_fee_delta;
        inc.filled_value += value;
        inc.fee_charged += buy_fee_delta;
    } else {
        inc.filled_value += value;
        inc.fee_charged += sell_fee_delta;
        maker.filled_value += value;
        maker.fee_charged += buy_fee_delta;
    }
    Ok(())
}

fn forget_by_owner(by_owner: &mut HashMap<Owner, Vec<u64>>, owner: Owner, id: u64) {
    if let Some(ids) = by_owner.get_mut(&owner) {
        ids.retain(|&x| x != id);
        if ids.is_empty() {
            by_owner.remove(&owner);
        }
    }
}

/// RoutedPrice 在交易部受理时解析（与 worker.rs 同语义；独立实现避免
/// 改动现有臂的 StockBook 签名）。
fn resolve_rp(
    rp: &RoutedPrice,
    side: engine::orderbook::Side,
    best_bid: Option<i64>,
    best_ask: Option<i64>,
    upper: i64,
    lower: i64,
) -> i64 {
    match rp {
        RoutedPrice::Fixed(p) => *p,
        RoutedPrice::Highest => match side {
            engine::orderbook::Side::Buy => best_ask.unwrap_or(upper).min(upper),
            engine::orderbook::Side::Sell => upper,
        },
        RoutedPrice::Lowest => match side {
            engine::orderbook::Side::Sell => best_bid.unwrap_or(lower).max(lower),
            engine::orderbook::Side::Buy => lower,
        },
        RoutedPrice::Market => match side {
            engine::orderbook::Side::Buy => upper,
            engine::orderbook::Side::Sell => lower,
        },
    }
}

/// 佣金摊销步进（与 book.rs 同法）：新总额 − 已收。
fn commission_step(
    fees: &FeeTable,
    filled_value: i64,
    fee_charged: i64,
    value: i64,
) -> Result<i64, ProtoError> {
    let total = filled_value
        .checked_add(value)
        .ok_or_else(|| overflow("filled_value", value))?;
    Ok(fees.commission(total)? - fee_charged)
}

fn mul_price_qty(price: i64, qty: u32) -> Result<i64, ProtoError> {
    price
        .checked_mul(qty as i64)
        .ok_or_else(|| ProtoError::Overflow {
            op: "price*qty",
            operand: format!("{price}*{qty}"),
        })
}

fn overflow(op: &'static str, v: i64) -> ProtoError {
    ProtoError::Overflow {
        op,
        operand: v.to_string(),
    }
}

fn count_qty(level: &VecDeque<DeskOrder>) -> u64 {
    level.iter().map(|o| o.qty as u64).sum()
}

// ═══════════════════════════════════════════════════════════════════
// 交易部线程主循环
// ═══════════════════════════════════════════════════════════════════

/// 交易部主函数：一股一线程，独占本股簿与全部在途实体。
pub fn desk_main(
    shared: Arc<OwnShared>,
    stock_idx: usize,
    n_groups: u8,
    rx: Receiver<DeskMsg>,
    group_txs: Vec<Sender<GroupIn>>,
    up_tx: Sender<OwnUp>,
    abort: Arc<AtomicBool>,
) -> Result<(), ProtoError> {
    let stock = stock_idx as u8;
    let fees = FeeTable::from_config(&shared.game_cfg);
    let (lower, upper) = shared.bands[stock_idx];
    let mut desk = Desk::new(stock, n_groups as usize);
    let mut cur_tick = 0u64;
    // 已收到 final_flush 的组计数（每组每 tick 恰发一个 final；计满
    // n_groups 即本 tick 撮合输入完毕）。不用位掩码：组数可超 64。
    let mut groups_done: u32 = 0;
    let mut vol_before = 0u64;
    let mut trades_before = 0u64;
    let mut last_price = shared.initial_prices[stock_idx];
    let mut result: Result<(), ProtoError> = Ok(());

    let report_failure = |result: &mut Result<(), ProtoError>, e: ProtoError| {
        abort.store(true, Ordering::SeqCst);
        let _ = up_tx.send(OwnUp::Failed(stock, e.to_string()));
        *result = Err(e);
    };

    loop {
        let msg = match rx.recv_timeout(RECV_TIMEOUT) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => {
                if abort.load(Ordering::SeqCst) {
                    break;
                }
                report_failure(
                    &mut result,
                    ProtoError::ChannelClosed(format!(
                        "交易部 {stock} 收件超时（{}s 无消息且无中止标记，实现 bug 或上游挂死）",
                        RECV_TIMEOUT.as_secs()
                    )),
                );
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            DeskMsg::Ops {
                tick,
                group,
                ops,
                final_flush,
            } => {
                if tick > cur_tick {
                    // 新 tick 首批先到：到期撤销先于本 tick 新意图（对齐
                    // worker.rs 相位序：expire → intents）。
                    cur_tick = tick;
                    vol_before = desk.volume_shares;
                    trades_before = desk.trade_count;
                    if let Err(e) = desk.expire_due(tick) {
                        report_failure(&mut result, e);
                        break;
                    }
                } else if tick < cur_tick {
                    report_failure(
                        &mut result,
                        ProtoError::InvalidConfig(format!(
                            "交易部 {stock} 收到过期 tick 消息：{tick} < {cur_tick}"
                        )),
                    );
                    break;
                }
                let mut failed: Option<ProtoError> = None;
                for op in ops {
                    let step = match op {
                        DeskIn::CancelAll { owner } => desk.cancel_all_for(owner).map(|_| ()),
                        DeskIn::Place {
                            owner,
                            side,
                            price,
                            qty,
                            purse,
                        } => desk.place(
                            owner,
                            side,
                            price,
                            qty,
                            purse,
                            tick,
                            shared.cfg.order_ttl_ticks,
                            &fees,
                            lower,
                            upper,
                        ),
                    };
                    if let Err(e) = step {
                        failed = Some(e);
                        break;
                    }
                }
                if let Some(e) = failed {
                    report_failure(&mut result, e);
                    break;
                }
                if final_flush {
                    groups_done += 1;
                    if groups_done == n_groups as u32 {
                        groups_done = 0;
                        // tick 收束：实体批次 flush → 组线程（先于 DeskDone，
                        // 因果保证组端在下一 Start 前收齐）。
                        let mut send_failed = false;
                        for (g, entities) in desk.take_out().into_iter().enumerate() {
                            if entities.is_empty() {
                                continue;
                            }
                            if group_txs[g]
                                .send(GroupIn::Batch(DeskBatch { stock, entities }))
                                .is_err()
                            {
                                send_failed = true;
                                break;
                            }
                        }
                        if send_failed {
                            report_failure(
                                &mut result,
                                ProtoError::ChannelClosed(format!(
                                    "交易部 {stock} 实体批次发送失败"
                                )),
                            );
                            break;
                        }
                        let done = TickDone {
                            stock_idx,
                            last_price: desk.last_price.unwrap_or(last_price),
                            best_bid: desk.best_bid(),
                            best_ask: desk.best_ask(),
                            volume_delta: desk.volume_shares - vol_before,
                            trade_count_delta: desk.trade_count - trades_before,
                            decide_calls: 0,
                            intents: 0,
                            resting: desk.resting_count,
                            imbalance: desk.top5_imbalance(),
                        };
                        last_price = done.last_price;
                        if up_tx.send(OwnUp::DeskDone(done)).is_err() {
                            report_failure(
                                &mut result,
                                ProtoError::ChannelClosed(format!(
                                    "交易部 {stock} DeskDone 发送失败"
                                )),
                            );
                            break;
                        }
                    }
                }
            }
            DeskMsg::Drain => {
                if let Err(e) = desk.cancel_all() {
                    report_failure(&mut result, e);
                    break;
                }
                let mut send_failed = false;
                for (g, entities) in desk.take_out().into_iter().enumerate() {
                    if entities.is_empty() {
                        continue;
                    }
                    if group_txs[g]
                        .send(GroupIn::Batch(DeskBatch { stock, entities }))
                        .is_err()
                    {
                        send_failed = true;
                        break;
                    }
                }
                if send_failed {
                    report_failure(
                        &mut result,
                        ProtoError::ChannelClosed(format!("交易部 {stock} Drain 实体批次发送失败")),
                    );
                    break;
                }
                if up_tx.send(OwnUp::DeskDrained).is_err() {
                    report_failure(
                        &mut result,
                        ProtoError::ChannelClosed(format!("交易部 {stock} DeskDrained 发送失败")),
                    );
                    break;
                }
            }
        }
    }
    let send = up_tx.send(OwnUp::DeskFinal(Box::new(DeskFinal {
        stock_idx,
        fee_sink: desk.fee_sink,
        trade_count: desk.trade_count,
        volume_shares: desk.volume_shares,
        skipped_orders: desk.skipped_orders,
        expired_orders: desk.expired_orders,
        cancelled_orders: desk.cancelled_orders,
        min_trade_price: desk.min_trade_price,
        max_trade_price: desk.max_trade_price,
        entities: desk.ent,
        batches_sent: desk.batches_sent,
    })));
    if result.is_ok() && send.is_err() {
        result = Err(ProtoError::ChannelClosed(format!(
            "交易部 {stock} Final 发送失败"
        )));
    }
    result
}

// ═══════════════════════════════════════════════════════════════════
// NPC 组线程（分组持一组一线程，持有组内全部现金/持仓）
// ═══════════════════════════════════════════════════════════════════

/// 组线程主函数：内联决策 → 实体扣除 → 分批发交易部；收实体批次落账。
#[allow(clippy::too_many_arguments)]
pub fn group_main(
    shared: Arc<OwnShared>,
    group_idx: u8,
    n_groups: u8,
    roster: Vec<Npc>,
    home_of: Vec<u8>,
    masks: Vec<u8>,
    desk_txs: Vec<Sender<DeskMsg>>,
    rx: Receiver<GroupIn>,
    up_tx: Sender<OwnUp>,
    abort: Arc<AtomicBool>,
) -> Result<(), ProtoError> {
    let fees = FeeTable::from_config(&shared.game_cfg);
    let n_stocks = shared.cfg.stocks.len();
    let chunk = flush_chunk();
    let mut roster = roster;
    // 在途挂单登记：npc → 每股在途单计数（对齐撤单只发有在途的股）。
    let mut foreign_active = vec![[0u16; 8]; roster.len()];
    // 每 home 股的单持仓 SelfView 模板（组内 NPC 混多个 home 股）。
    let mut singles: Vec<SelfView> = (0..n_stocks)
        .map(|s| {
            let mut positions = BTreeMap::new();
            positions.insert(
                shared.codes[s].clone(),
                PositionView {
                    qty: 0,
                    sellable_qty: 0,
                    cost_price: None,
                },
            );
            SelfView {
                cash: Money::ZERO,
                positions,
            }
        })
        .collect();
    let mut own_multi = SelfView {
        cash: Money::ZERO,
        positions: BTreeMap::new(),
    };
    // 组内出现的持股掩码集合（静态）。
    let mut views_masks: Vec<u8> = masks.iter().copied().collect();
    views_masks.sort_unstable();
    views_masks.dedup();

    let mut result: Result<(), ProtoError> = Ok(());
    let mut skipped_orders = 0u64;
    let mut cancel_intents = 0u64;
    let mut cross_places = 0u64;
    let mut home_places = 0u64;
    let mut align_cancels = 0u64;
    let mut flushes_sent = 0u64;

    let report_failure = |result: &mut Result<(), ProtoError>, e: ProtoError| {
        abort.store(true, Ordering::SeqCst);
        let _ = up_tx.send(OwnUp::Failed(group_idx, e.to_string()));
        *result = Err(e);
    };

    loop {
        let msg = match rx.recv_timeout(RECV_TIMEOUT) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => {
                if abort.load(Ordering::SeqCst) {
                    break;
                }
                report_failure(
                    &mut result,
                    ProtoError::ChannelClosed(format!(
                        "组 {group_idx} 收件超时（{}s 无消息且无中止标记，实现 bug 或上游挂死）",
                        RECV_TIMEOUT.as_secs()
                    )),
                );
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            GroupIn::Batch(batch) => {
                if let Err(e) = apply_entities(
                    &mut roster,
                    &mut foreign_active,
                    &home_of,
                    &shared.initial_prices,
                    batch,
                ) {
                    report_failure(&mut result, e);
                    break;
                }
            }
            GroupIn::Start(snapshot) => {
                // T+1 解冻：上一 tick 的全部实体批次已先于本 Start 入队
                // （交易部 flush 先于 DeskDone，coordinator 收齐才广播）。
                for npc in &mut roster {
                    npc.unfreeze_t1_all();
                }
                let tick = snapshot.tick;
                let views = build_views_own(&shared, &snapshot, &views_masks);
                let step = decide_and_ship(
                    &shared,
                    &fees,
                    group_idx,
                    n_groups,
                    &mut roster,
                    &home_of,
                    &masks,
                    &mut foreign_active,
                    &mut singles,
                    &mut own_multi,
                    &views,
                    &desk_txs,
                    tick,
                    chunk,
                    &mut skipped_orders,
                    &mut cancel_intents,
                    &mut cross_places,
                    &mut home_places,
                    &mut align_cancels,
                    &mut flushes_sent,
                );
                match step {
                    Ok((decide_calls, intents)) => {
                        if up_tx
                            .send(OwnUp::GroupStats {
                                group: group_idx,
                                decide_calls,
                                intents,
                            })
                            .is_err()
                        {
                            report_failure(
                                &mut result,
                                ProtoError::ChannelClosed(format!(
                                    "组 {group_idx} GroupStats 发送失败"
                                )),
                            );
                            break;
                        }
                    }
                    Err(e) => {
                        report_failure(&mut result, e);
                        break;
                    }
                }
            }
            GroupIn::Shutdown => break,
        }
    }
    let send = up_tx.send(OwnUp::GroupFinal(Box::new(GroupFinal {
        group: group_idx,
        roster,
        skipped_orders,
        cancel_intents,
        cross_places,
        home_places,
        align_cancels,
        flushes_sent,
    })));
    if result.is_ok() && send.is_err() {
        result = Err(ProtoError::ChannelClosed(format!(
            "组 {group_idx} Final 发送失败"
        )));
    }
    result
}

/// 实体批次落账（组端权威账户）：现金回流、新股票 T+1 冻结、余股恢复
/// 可卖、终结标记递减在途登记。
fn apply_entities(
    roster: &mut [Npc],
    foreign_active: &mut [[u16; 8]],
    home_of: &[u8],
    initial_prices: &[i64],
    batch: DeskBatch,
) -> Result<(), ProtoError> {
    let s = batch.stock as usize;
    for e in batch.entities {
        match e {
            Entity::CashProceeds { local, amount } | Entity::CashRefund { local, amount } => {
                let npc = &mut roster[local as usize];
                npc.cash = npc
                    .cash
                    .checked_add(amount)
                    .ok_or_else(|| overflow("cash entity", amount))?;
            }
            Entity::BoughtShares { local, qty } => {
                let npc = &mut roster[local as usize];
                if s == home_of[local as usize] as usize {
                    npc.qty = npc
                        .qty
                        .checked_add(qty)
                        .ok_or_else(|| overflow("qty", qty as i64))?;
                    npc.frozen_today = npc
                        .frozen_today
                        .checked_add(qty)
                        .ok_or_else(|| overflow("frozen", qty as i64))?;
                } else if let Some(pos) = npc
                    .secondaries
                    .iter_mut()
                    .find(|p| p.stock_idx as usize == s)
                {
                    pos.qty = pos
                        .qty
                        .checked_add(qty)
                        .ok_or_else(|| overflow("qty", qty as i64))?;
                    pos.frozen_today = pos
                        .frozen_today
                        .checked_add(qty)
                        .ok_or_else(|| overflow("frozen", qty as i64))?;
                } else {
                    // 跨股新仓（运行期掩码静态 ⇒ 理论必命中；建分支防御保留）。
                    npc.secondaries.push(XPos {
                        stock_idx: s as u8,
                        qty,
                        sellable: 0,
                        reserved_shares: 0,
                        frozen_today: qty,
                        cost_price_cents: initial_prices[s],
                    });
                }
            }
            Entity::RefundedShares { local, qty } => {
                let npc = &mut roster[local as usize];
                if s == home_of[local as usize] as usize {
                    npc.qty = npc
                        .qty
                        .checked_add(qty)
                        .ok_or_else(|| overflow("qty", qty as i64))?;
                    npc.sellable = npc
                        .sellable
                        .checked_add(qty)
                        .ok_or_else(|| overflow("sellable", qty as i64))?;
                } else {
                    let Some(pos) = npc
                        .secondaries
                        .iter_mut()
                        .find(|p| p.stock_idx as usize == s)
                    else {
                        return Err(ProtoError::InvalidConfig(format!(
                            "RefundedShares 无对应次级仓：local={local} stock={s}"
                        )));
                    };
                    pos.qty = pos
                        .qty
                        .checked_add(qty)
                        .ok_or_else(|| overflow("qty", qty as i64))?;
                    pos.sellable = pos
                        .sellable
                        .checked_add(qty)
                        .ok_or_else(|| overflow("sellable", qty as i64))?;
                }
            }
            Entity::Closed { local } => {
                let slot = &mut foreign_active[local as usize][s];
                if *slot == 0 {
                    return Err(ProtoError::InvalidConfig(format!(
                        "Closed 实体无对应在途单：local={local} stock={s}"
                    )));
                }
                *slot -= 1;
            }
        }
    }
    Ok(())
}

/// 决策 + 实体扣除 + 分批直发交易部（组线程内联单线程）。
/// 每 `chunk` 个 NPC flush 一次（决策与撮合流水线重叠）；同 NPC 的
/// 对齐撤单与其 Place 恒在同批（FIFO 保序）。
#[allow(clippy::too_many_arguments)]
fn decide_and_ship(
    shared: &Arc<OwnShared>,
    fees: &FeeTable,
    group_idx: u8,
    _n_groups: u8,
    roster: &mut [Npc],
    home_of: &[u8],
    masks: &[u8],
    foreign_active: &mut [[u16; 8]],
    singles: &mut [SelfView],
    own_multi: &mut SelfView,
    views: &[Option<Arc<MarketView>>],
    desk_txs: &[Sender<DeskMsg>],
    tick: u64,
    chunk: usize,
    skipped_orders: &mut u64,
    cancel_intents: &mut u64,
    cross_places: &mut u64,
    home_places: &mut u64,
    align_cancels: &mut u64,
    flushes_sent: &mut u64,
) -> Result<(u64, u64), ProtoError> {
    let n_stocks = shared.cfg.stocks.len();
    let seed = shared.cfg.seed;
    let game_cfg = &shared.game_cfg;
    let mut batches: Vec<Vec<DeskIn>> = (0..n_stocks).map(|_| Vec::new()).collect();
    let mut aligned = vec![false; roster.len()];
    let mut decide_calls = 0u64;
    let mut intents_total = 0u64;

    let mut flush = |batches: &mut Vec<Vec<DeskIn>>, final_flush: bool| -> Result<(), ProtoError> {
        for (s, ops) in batches.iter_mut().enumerate() {
            if ops.is_empty() && !final_flush {
                continue;
            }
            desk_txs[s]
                .send(DeskMsg::Ops {
                    tick,
                    group: group_idx,
                    ops: std::mem::take(ops),
                    final_flush,
                })
                .map_err(|_| {
                    ProtoError::ChannelClosed(format!("组 {group_idx} → 交易部 {s} 批次发送失败"))
                })?;
            *flushes_sent += 1;
        }
        Ok(())
    };

    for idx in 0..roster.len() {
        let npc = &roster[idx];
        decide_calls += 1;
        let home = home_of[idx];
        let mask = masks[idx];
        let market = views[mask as usize]
            .as_ref()
            .expect("持股掩码的视图必须预建（掩码静态）");
        // SelfView：单持仓走 home 模板快路径（就地刷新，零分配），
        // 多持仓逐 NPC 重建（clear 复用容器）。
        if npc.secondaries.is_empty() {
            let own = &mut singles[home as usize];
            let pos = own
                .positions
                .get_mut(&shared.codes[home as usize])
                .expect("模板已插入 home 股键");
            pos.qty = npc.qty;
            pos.sellable_qty = npc.sellable;
            pos.cost_price = Some(Money::from_cents(npc.cost_price_cents));
            own.cash = Money::from_cents(npc.cash); // 严格所有权：在途实体已扣。
        } else {
            own_multi.positions.clear();
            own_multi.positions.insert(
                shared.codes[home as usize].clone(),
                PositionView {
                    qty: npc.qty,
                    sellable_qty: npc.sellable,
                    cost_price: Some(Money::from_cents(npc.cost_price_cents)),
                },
            );
            for p in &npc.secondaries {
                own_multi.positions.insert(
                    shared.codes[p.stock_idx as usize].clone(),
                    PositionView {
                        qty: p.qty,
                        sellable_qty: p.sellable,
                        cost_price: Some(Money::from_cents(p.cost_price_cents)),
                    },
                );
            }
            own_multi.cash = Money::from_cents(npc.cash);
        }
        let own: &SelfView = if npc.secondaries.is_empty() {
            &singles[home as usize]
        } else {
            own_multi
        };
        let mut rng = SplitMix64::new(seed ^ tick ^ npc.id as u64);
        let intents = decide_data(&npc.strategy, market, own, &mut rng, game_cfg);
        if intents.is_empty() {
            // 无意图也要参与分段 flush 计数（chunk 以 NPC 数计）。
            if (idx + 1) % chunk == 0 || idx + 1 == roster.len() {
                flush(&mut batches, idx + 1 == roster.len())?;
            }
            continue;
        }
        intents_total += intents.len() as u64;
        for intent in intents {
            if !aligned[idx] {
                // 工作单对齐（引擎语义）：新意图先撤两侧挂单——对每个
                // 有在途单的交易部发 CancelAll（home 股同样经消息撤，
                // 簿在交易部手里）。
                aligned[idx] = true;
                for (d, &count) in foreign_active[idx].iter().enumerate().take(n_stocks) {
                    if count > 0 {
                        batches[d].push(DeskIn::CancelAll {
                            owner: (group_idx, idx as u32),
                        });
                        *align_cancels += 1;
                    }
                }
            }
            let code = match &intent {
                Intent::PlaceLimit { code, .. }
                | Intent::PlaceMarket { code, .. }
                | Intent::Cancel { code, .. } => code,
            };
            let target = *shared.code_idx.get(code.0.as_str()).ok_or_else(|| {
                ProtoError::InvalidConfig(format!("意图指向未知股票：{:?}", code.0))
            })?;
            let t = target as usize;
            let (side, rp, qty) = match &intent {
                Intent::PlaceLimit {
                    side, price, qty, ..
                } => (*side, RoutedPrice::from_limit(price, false), *qty),
                Intent::PlaceMarket { side, qty, .. } => (*side, RoutedPrice::Market, *qty),
                // 策略层不产 Cancel（会话层职责）；出现即显式计数，不静默。
                Intent::Cancel { .. } => {
                    *cancel_intents += 1;
                    continue;
                }
            };
            match side {
                engine::orderbook::Side::Buy => {
                    // purse 上限价：Fixed 用固定价，符号价/市价用目标股涨停
                    // （目标价活簿在交易部，组端按最坏情况实体化）。
                    let upper_t = shared.bands[t].1;
                    let p = match rp {
                        RoutedPrice::Fixed(p) => p,
                        _ => upper_t,
                    };
                    let value = p
                        .checked_mul(qty as i64)
                        .ok_or_else(|| ProtoError::Overflow {
                            op: "purse price*qty",
                            operand: format!("{p}*{qty}"),
                        })?;
                    let purse = value.checked_add(fees.commission(value)?).ok_or_else(|| {
                        ProtoError::Overflow {
                            op: "purse+fee",
                            operand: format!("{value}"),
                        }
                    })?;
                    // 严格所有权：现金实体从持有账户实际扣出。挂单在途期间
                    // 该笔资金不可用；对齐撤单的在途退款未到账同样不可用
                    // （与 escrow 臂的同步释放差异 → skip 计数显式暴露）。
                    let npc = &mut roster[idx];
                    if npc.cash < purse {
                        *skipped_orders += 1;
                        continue;
                    }
                    npc.cash -= purse;
                    foreign_active[idx][t] += 1;
                    if t == home as usize {
                        *home_places += 1;
                    } else {
                        *cross_places += 1;
                    }
                    batches[t].push(DeskIn::Place {
                        owner: (group_idx, idx as u32),
                        side,
                        price: rp,
                        qty,
                        purse,
                    });
                }
                engine::orderbook::Side::Sell => {
                    let npc = &mut roster[idx];
                    if t == home as usize {
                        if npc.sellable < qty {
                            *skipped_orders += 1;
                            continue;
                        }
                        npc.sellable -= qty;
                        npc.qty -= qty; // 股票实体实际搬出。
                    } else {
                        let Some(pos) = npc
                            .secondaries
                            .iter_mut()
                            .find(|p| p.stock_idx as usize == t)
                        else {
                            *skipped_orders += 1;
                            continue;
                        };
                        if pos.sellable < qty {
                            *skipped_orders += 1;
                            continue;
                        }
                        pos.sellable -= qty;
                        pos.qty -= qty;
                    }
                    foreign_active[idx][t] += 1;
                    if t == home as usize {
                        *home_places += 1;
                    } else {
                        *cross_places += 1;
                    }
                    batches[t].push(DeskIn::Place {
                        owner: (group_idx, idx as u32),
                        side,
                        price: rp,
                        qty,
                        purse: 0,
                    });
                }
            }
        }
        if (idx + 1) % chunk == 0 || idx + 1 == roster.len() {
            flush(&mut batches, idx + 1 == roster.len())?;
        }
    }
    // 防御：roster 为空时上面循环不执行，final 标记必须仍发出。
    if roster.is_empty() {
        flush(&mut batches, true)?;
    }
    Ok((decide_calls, intents_total))
}

/// 每 mask 的多股视图（快照口径：外股盘口 = 上一 tick 收束值，与跨股臂
/// 一致——组线程看不到任何活簿，决策信息集两臂等价）。
fn build_views_own(
    shared: &OwnShared,
    snap: &MarketSnapshot,
    views_masks: &[u8],
) -> Vec<Option<Arc<MarketView>>> {
    let n = shared.cfg.stocks.len();
    let base: Vec<StockView> = (0..n)
        .map(|i| {
            let st = &snap.stocks[i];
            let (lower, upper) = shared.bands[i];
            StockView {
                is_trading: true,
                best_bid: st.best_bid.map(Money::from_cents),
                best_ask: st.best_ask.map(Money::from_cents),
                last_price: Money::from_cents(st.last_price.max(0)),
                max_buy_price: Money::from_cents(upper),
                daily_upper_limit: Money::from_cents(upper),
                min_sell_price: Money::from_cents(lower),
                recent_prices: st
                    .recent_prices
                    .iter()
                    .map(|&c| Money::from_cents(c))
                    .collect(),
                recent_market_minute_prices: st
                    .minute_prices
                    .iter()
                    .map(|&c| Money::from_cents(c))
                    .collect(),
                relative_volume: st.rel_volume,
                order_book_imbalance: st.imbalance,
            }
        })
        .collect();
    let mut out: Vec<Option<Arc<MarketView>>> = (0..32).map(|_| None).collect();
    for &mask in views_masks {
        let mut map = BTreeMap::new();
        for i in 0..n {
            if mask & (1 << i) != 0 {
                map.insert(shared.codes[i].clone(), base[i].clone());
            }
        }
        out[mask as usize] = Some(Arc::new(MarketView {
            stocks: map,
            tick: snap.tick,
            market_minute: snap.market_minute,
        }));
    }
    out
}

/// 允许测试直接检查组端结构（预留，与 run.rs 的 audit_npc_t1 对应）。
pub fn own_roster_t1(npc: &Npc) -> bool {
    npc.x_t1_ok()
}
