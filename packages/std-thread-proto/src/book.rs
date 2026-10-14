//! 本股订单簿撮合（worker 单线程私有，无锁）+ 统一账户竞技场。
//!
//! 语义（对齐大 A 基线，简化项登记于 design.md §5）：
//! - 价格优先、同价 FIFO（时间优先）：`BTreeMap<price, VecDeque>`。
//! - 涨跌停带：由 worker 解析价格时保证；book 内防御性断言价格 > 0、数量整手。
//! - 买单资金冻结：下单冻结「全额 + 佣金上限」，逐笔成交按实际扣费释放；
//!   佣金按**委托维度摊销**（min 佣金对委托总成交额取 max，不在每笔成交重复收），
//!   保证累计扣费 ≤ 冻结额——原型简化，与 engine 逐笔计费口径的差异已登记。
//! - 卖单 T+1 可卖占用：下单即从 sellable 划入 reserved_shares。
//! - 工作单对齐（对齐引擎「买卖两侧工作单与最新目标对齐」语义）：NPC 发出
//!   新意图前撤掉自己的全部挂单，从根上排除自成交。
//! - 挂单 TTL 到期撤销（FIFO 队列，index 校验存活）。
//!
//! 账户竞技场（跨股臂扩展）：本地名册槽 0..roster_len 之外可分配「外部交易部
//! 席位」槽。外部席位是家端 escrow 的资源实体（现金/股数已从家端账户转出，
//! 所有权随路由单流入本股交易部），撮合代码对两类槽位无差别操作。外部槽的
//! 每次账户变更都会产生 `FillReceipt`，由 worker 上交 coordinator 中转回
//! 家端应用；本地槽不产生回执（按股私有臂行为与原先完全一致）。

use crate::npc::{FeeTable, Npc};
use crate::ProtoError;
use engine::orderbook::Side;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// 外部账户身份：(home 股索引, 家端名册内 npc 索引)。
pub type ForeignId = (u8, u32);

/// 撮合回执：外部（跨股）订单的成交/终结事件。目标股 worker 产生 →
/// coordinator 中转 → 家端 worker 在回执相位应用到次级仓位/现金。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillReceipt {
    /// 外部买单成交：家端 cash -= charge、reserved_cash -= charge、
    /// 次级仓 qty/frozen_today += qty。
    BuyFill {
        home: u8,
        npc: u32,
        stock: u8,
        qty: u32,
        charge: i64,
    },
    /// 外部卖单成交：家端 cash += net、次级仓 qty/reserved_shares -= qty。
    SellFill {
        home: u8,
        npc: u32,
        stock: u8,
        qty: u32,
        net: i64,
    },
    /// 外部买单终结（全成/撤/到期）：家端 reserved_cash -= release。
    /// release = 席位剩余现金（escrow 总额 − 累计成交扣费），≥ 0。
    BuyClosed {
        home: u8,
        npc: u32,
        stock: u8,
        release: i64,
    },
    /// 外部卖单终结：家端次级仓 reserved_shares -= qty、sellable += qty。
    SellClosed { home: u8, npc: u32, stock: u8, qty: u32 },
}

impl FillReceipt {
    /// 回执目标家端股索引（coordinator 分发用）。
    pub fn home(&self) -> u8 {
        match *self {
            FillReceipt::BuyFill { home, .. }
            | FillReceipt::SellFill { home, .. }
            | FillReceipt::BuyClosed { home, .. }
            | FillReceipt::SellClosed { home, .. } => home,
        }
    }
}

/// 统一账户竞技场：本地名册 + 外部交易部席位共用一个账号下标空间，
/// 挂单的 `npc_idx` 一律指向竞技场槽位。
pub struct Arena {
    /// 全部账户（本地名册在前，外部席位在后）。
    pub accounts: Vec<Npc>,
    /// 槽位 → 外部身份（None = 本地名册槽）。与 accounts 等长。
    pub foreign: Vec<Option<ForeignId>>,
    /// 待上交的回执缓冲（仅外部槽产生）。worker 每相位取走。
    pub receipts: Vec<FillReceipt>,
    /// 本地名册长度（= 竞技场前缀长度）。
    pub roster_len: u32,
    /// 本竞技场服务的股票索引（回执的 stock 字段来源）。
    pub stock_idx: u8,
}

impl Arena {
    pub fn new(roster: Vec<Npc>, stock_idx: u8) -> Self {
        let roster_len = roster.len() as u32;
        let foreign = vec![None; roster.len()];
        Arena {
            accounts: roster,
            foreign,
            receipts: Vec::new(),
            roster_len,
            stock_idx,
        }
    }

    pub fn roster(&self) -> &[Npc] {
        &self.accounts[..self.roster_len as usize]
    }

    pub fn accounts(&self) -> &[Npc] {
        &self.accounts
    }

    pub fn accounts_mut(&mut self) -> &mut [Npc] {
        &mut self.accounts
    }

    pub fn into_roster(self) -> Vec<Npc> {
        debug_assert!(self.receipts.is_empty(), "回收竞技场时回执缓冲必须已清空");
        // 只上交本地名册：外部席位是 escrow 镜像（其权威账在家端回执里），
        // 不得混入期末审计的持仓统计。
        let mut accounts = self.accounts;
        accounts.truncate(self.roster_len as usize);
        accounts
    }

    /// 分配外部席位。买单传 `cash`（escrow 总额）；卖单传 `shares`
    /// （预留股数，落位为可卖）。返回槽位号。
    pub fn alloc_desk(&mut self, id: ForeignId, cash: i64, shares: u32) -> u32 {
        let slot = self.accounts.len() as u32;
        self.accounts.push(Npc::desk(cash, shares));
        self.foreign.push(Some(id));
        if crate::xdbg() {
            eprintln!("[desk alloc] slot={slot} id={id:?} cash={cash} shares={shares} stock={}", self.stock_idx);
        }
        slot
    }

    /// 回收外部席位（仅在对应挂单全部终结且回执已产生后由 worker 调用）。
    pub fn free_desk(&mut self, slot: u32) {
        if crate::xdbg() {
            eprintln!(
                "[desk free] slot={slot} id={:?} cash={} shares={} stock={}",
                self.foreign[slot as usize],
                self.accounts[slot as usize].cash,
                self.accounts[slot as usize].qty,
                self.stock_idx
            );
        }
        self.foreign[slot as usize] = None;
        // 槽位本身保留（下标稳定性），账户数据作废不再引用。
    }

    /// 复用已回收的席位槽（重置为新的 escrow 实体）。
    pub fn reuse_desk(&mut self, slot: u32, id: ForeignId, cash: i64, shares: u32) {
        if crate::xdbg() {
            eprintln!(
                "[desk reuse] slot={slot} id={id:?} cash={cash} shares={shares} stock={} 旧cash={} 旧qty={}",
                self.stock_idx,
                self.accounts[slot as usize].cash,
                self.accounts[slot as usize].qty
            );
        }
        self.accounts[slot as usize] = Npc::desk(cash, shares);
        self.foreign[slot as usize] = Some(id);
    }

    /// 外部买单成交回执（本地槽不产生）。
    fn note_buy_fill(&mut self, slot: u32, qty: u32, charge: i64) {
        if let Some((home, npc)) = self.foreign[slot as usize] {
            self.receipts.push(FillReceipt::BuyFill {
                home,
                npc,
                stock: self.stock_idx,
                qty,
                charge,
            });
        }
    }

    /// 外部卖单成交回执。
    fn note_sell_fill(&mut self, slot: u32, qty: u32, net: i64) {
        if let Some((home, npc)) = self.foreign[slot as usize] {
            self.receipts.push(FillReceipt::SellFill {
                home,
                npc,
                stock: self.stock_idx,
                qty,
                net,
            });
        }
    }

    /// 外部买单终结：席位剩余现金全额回流家端并清零。
    fn note_buy_closed(&mut self, slot: u32) {
        if let Some((home, npc)) = self.foreign[slot as usize] {
            let acct = &mut self.accounts[slot as usize];
            let release = acct.cash;
            debug_assert!(
                release >= 0,
                "外部买席位现金为负：slot={slot} cash={release}"
            );
            acct.cash = 0;
            self.receipts.push(FillReceipt::BuyClosed {
                home,
                npc,
                stock: self.stock_idx,
                release,
            });
        }
    }

    /// 外部卖单终结：剩余预留股回流家端可卖，并清零席位镜像。
    fn note_sell_closed(&mut self, slot: u32, qty: u32) {
        if qty == 0 {
            return;
        }
        if let Some((home, npc)) = self.foreign[slot as usize] {
            let acct = &mut self.accounts[slot as usize];
            debug_assert!(
                acct.qty >= qty,
                "外部卖席位股数不足：slot={slot} qty={} 释放={qty}",
                acct.qty
            );
            acct.qty -= qty;
            acct.sellable -= qty;
            self.receipts.push(FillReceipt::SellClosed {
                home,
                npc,
                stock: self.stock_idx,
                qty,
            });
        }
    }
}

/// 挂单/在途单统一形态（在途单不进簿，复用字段）。
#[derive(Clone, Debug)]
pub struct Order {
    pub id: u64,
    /// 竞技场槽位（本地名册索引或外部席位槽位）。
    pub npc_idx: u32,
    pub side: Side,
    /// 限价（分）。
    pub price: i64,
    /// 剩余数量（股）。
    pub qty: u32,
    /// 到期 tick。
    pub expire_tick: u64,
    /// 买单：剩余冻结资金（分）。
    pub reserved_cash: i64,
    /// 本委托累计成交额（分），费用摊销基准。
    pub filled_value: i64,
    /// 本委托已收佣金（分）。
    pub fee_charged: i64,
}

/// 下单结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// 全部成交。
    FullyFilled,
    /// 部分成交后剩余挂簿。
    Rested,
    /// 被拒绝（资金/可卖不足或数量非法），显式计数，不静默。
    Skipped(SkipReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    InsufficientCash,
    InsufficientSellable,
    NonLotQty,
}

/// 本股订单簿。
pub struct StockBook {
    bids: BTreeMap<i64, VecDeque<Order>>,
    asks: BTreeMap<i64, VecDeque<Order>>,
    /// id -> (side, price)，撤单/到期定位。
    index: HashMap<u64, (Side, i64)>,
    /// 竞技场槽位 -> 活跃挂单 id（工作单对齐用）。
    by_npc: HashMap<u32, Vec<u64>>,
    next_id: u64,
    /// (expire_tick, id) FIFO（ttl 恒定时单调）。
    expiry_queue: VecDeque<(u64, u64)>,
    /// 费用汇聚（守恒断言的对手方桶）。
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
}

impl StockBook {
    pub fn new() -> Self {
        StockBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            index: HashMap::new(),
            by_npc: HashMap::new(),
            next_id: 1,
            expiry_queue: VecDeque::new(),
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
        }
    }

    pub fn best_bid(&self) -> Option<i64> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<i64> {
        self.asks.keys().next().copied()
    }

    /// 前 5 档量失衡 [-1,1]（无深度返回 0）。
    pub fn top5_imbalance(&self) -> f64 {
        let bid: u64 = self.bids.values().rev().take(5).map(count_qty).sum();
        let ask: u64 = self.asks.values().take(5).map(count_qty).sum();
        let total = bid + ask;
        if total == 0 {
            0.0
        } else {
            (bid as f64 - ask as f64) / total as f64
        }
    }

    pub fn resting_orders(&self) -> u64 {
        self.resting_count
    }

    /// 某竞技场槽位是否还有活跃挂单（外部席位回收判定用）。
    pub fn has_active_orders(&self, npc_idx: u32) -> bool {
        self.by_npc.contains_key(&npc_idx)
    }

    /// 某竞技场槽位的活跃挂单 id（诊断用）。
    pub fn active_order_ids(&self, npc_idx: u32) -> Vec<u64> {
        self.by_npc.get(&npc_idx).cloned().unwrap_or_default()
    }

    /// 下单入口：先冻结（买资金/卖可卖），再对价撮合，剩余挂簿。
    pub fn place(
        &mut self,
        arena: &mut Arena,
        npc_idx: u32,
        side: Side,
        price: i64,
        qty: u32,
        tick: u64,
        ttl: u64,
        fees: &FeeTable,
    ) -> Result<Placement, ProtoError> {
        if price <= 0 || qty == 0 || qty % 100 != 0 {
            self.skipped_orders += 1;
            return Ok(Placement::Skipped(SkipReason::NonLotQty));
        }
        let mut inc = Order {
            id: 0,
            npc_idx,
            side,
            price,
            qty,
            expire_tick: tick.saturating_add(ttl),
            reserved_cash: 0,
            filled_value: 0,
            fee_charged: 0,
        };
        {
            let npc = &mut arena.accounts[npc_idx as usize];
            match side {
                Side::Buy => {
                    let value = mul_price_qty(price, qty)?;
                    let reserve = value
                        .checked_add(fees.commission(value)?)
                        .ok_or_else(|| overflow("reserve", value))?;
                    if npc.available_cash() < reserve {
                        self.skipped_orders += 1;
                        return Ok(Placement::Skipped(SkipReason::InsufficientCash));
                    }
                    npc.reserved_cash += reserve;
                    inc.reserved_cash = reserve;
                }
                Side::Sell => {
                    if npc.sellable < qty {
                        self.skipped_orders += 1;
                        return Ok(Placement::Skipped(SkipReason::InsufficientSellable));
                    }
                    npc.sellable -= qty;
                    npc.reserved_shares += qty;
                }
            }
        }
        let remaining = self.match_incoming(arena, &mut inc, fees)?;
        if remaining == 0 {
            // 外部**买**单全部成交：席位剩余 escrow（slack）回流家端。
            // （卖单席位剩余的是累计卖出净额，归 SellFill 回执，不得误发 BuyClosed。）
            if side == Side::Buy {
                arena.note_buy_closed(npc_idx);
            }
            return Ok(Placement::FullyFilled);
        }
        // 剩余挂簿。
        inc.id = self.next_id;
        self.next_id += 1;
        let id = inc.id;
        let entry = (side, price);
        let expire_tick = inc.expire_tick;
        let side_map = if side == Side::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };
        side_map.entry(price).or_default().push_back(inc);
        self.index.insert(id, entry);
        self.by_npc.entry(npc_idx).or_default().push(id);
        self.expiry_queue.push_back((expire_tick, id));
        self.resting_count += 1;
        Ok(Placement::Rested)
    }

    /// 对价撮合：incoming 依次吃对面最优档。返回剩余数量。
    fn match_incoming(
        &mut self,
        arena: &mut Arena,
        inc: &mut Order,
        fees: &FeeTable,
    ) -> Result<u32, ProtoError> {
        while inc.qty > 0 {
            // 读对面最优价（判断是否交叉）。
            let (crossed, level_price) = match inc.side {
                Side::Buy => match self.asks.keys().next() {
                    Some(&ask) => (ask <= inc.price, ask),
                    None => (false, 0),
                },
                Side::Sell => match self.bids.keys().next_back() {
                    Some(&bid) => (bid >= inc.price, bid),
                    None => (false, 0),
                },
            };
            if !crossed {
                break;
            }
            // 解构出不相交的可变字段借用：对面簿、索引、槽位索引、统计。
            let Self {
                bids,
                asks,
                index,
                by_npc,
                fee_sink,
                trade_count,
                volume_shares,
                last_price,
                min_trade_price,
                max_trade_price,
                resting_count,
                ..
            } = self;
            let opposite = if inc.side == Side::Buy {
                asks
            } else {
                bids
            };
            let level = opposite
                .get_mut(&level_price)
                .expect("上面已确认档位存在");
            while inc.qty > 0 {
                let q = match level.front() {
                    Some(maker) => inc.qty.min(maker.qty),
                    None => break,
                };
                let maker = level.front_mut().expect("front 已确认存在");
                let stats = FillStats {
                    fee_sink,
                    trade_count,
                    volume_shares,
                    last_price,
                    min_trade_price,
                    max_trade_price,
                };
                settle_fill(arena, maker, inc, q, fees, stats)?;
                inc.qty -= q;
                maker.qty -= q;
                if maker.qty == 0 {
                    let done = level.pop_front().expect("front 已确认存在");
                    index.remove(&done.id);
                    forget_by_npc(by_npc, done.npc_idx, done.id);
                    *resting_count -= 1;
                    // 买单完全成交：释放剩余冻结。
                    if done.reserved_cash > 0 {
                        arena.accounts[done.npc_idx as usize].reserved_cash -= done.reserved_cash;
                    }
                    // 外部挂单终结：买单回流剩余 escrow，卖单释放剩余预留股
                    // （各自只对本方向生效；本地槽不产生回执）。
                    if done.side == Side::Buy {
                        arena.note_buy_closed(done.npc_idx);
                    } else {
                        arena.note_sell_closed(done.npc_idx, done.qty);
                    }
                } else {
                    break; // maker 部分成交，incoming 已耗尽。
                }
            }
            if opposite
                .get(&level_price)
                .is_some_and(|lvl| lvl.is_empty())
            {
                opposite.remove(&level_price);
            }
        }
        Ok(inc.qty)
    }

    /// 工作单对齐：撤掉某竞技场槽位的全部挂单（两侧），释放资金/可卖。
    /// 跨股臂：外部槽位撤单同样产生终结回执（escrow 回流家端）。
    pub fn cancel_all_for(
        &mut self,
        arena: &mut Arena,
        npc_idx: u32,
    ) -> Result<u64, ProtoError> {
        let ids = match self.by_npc.remove(&npc_idx) {
            Some(ids) => ids,
            None => return Ok(0),
        };
        let mut cancelled = 0u64;
        for id in ids {
            let Some((side, price)) = self.index.remove(&id) else {
                continue; // 已在撮合中离场，残留登记跳过。
            };
            let side_map = if side == Side::Buy {
                &mut self.bids
            } else {
                &mut self.asks
            };
            let level = side_map
                .get_mut(&price)
                .expect("index 命中则档位必须存在");
            let pos = level
                .iter()
                .position(|o| o.id == id)
                .expect("index 命中则挂单必须在档内");
            let order = level.remove(pos).expect("position 已确认");
            self.resting_count -= 1;
            self.cancelled_orders += 1;
            cancelled += 1;
            match side {
                Side::Buy => {
                    arena.accounts[order.npc_idx as usize].reserved_cash -= order.reserved_cash;
                    arena.note_buy_closed(order.npc_idx);
                }
                Side::Sell => {
                    let npc = &mut arena.accounts[order.npc_idx as usize];
                    npc.reserved_shares -= order.qty;
                    npc.sellable += order.qty;
                    arena.note_sell_closed(order.npc_idx, order.qty);
                }
            }
            if level.is_empty() {
                side_map.remove(&price);
            }
        }
        Ok(cancelled)
    }

    /// 到期撤销：expire_tick ≤ tick 的挂单全部撤出并释放占用。
    pub fn expire_due(&mut self, arena: &mut Arena, tick: u64) -> Result<(), ProtoError> {
        while let Some(&(expire_tick, id)) = self.expiry_queue.front() {
            if expire_tick > tick {
                break;
            }
            self.expiry_queue.pop_front();
            let Some((side, price)) = self.index.remove(&id) else {
                continue; // 已成交/已撤，残留队列项跳过。
            };
            let side_map = if side == Side::Buy {
                &mut self.bids
            } else {
                &mut self.asks
            };
            let level = side_map
                .get_mut(&price)
                .expect("index 命中则档位必须存在");
            let pos = level
                .iter()
                .position(|o| o.id == id)
                .expect("index 命中则挂单必须在档内");
            let order = level.remove(pos).expect("position 已确认");
            self.resting_count -= 1;
            self.expired_orders += 1;
            match side {
                Side::Buy => {
                    arena.accounts[order.npc_idx as usize].reserved_cash -= order.reserved_cash;
                    arena.note_buy_closed(order.npc_idx);
                }
                Side::Sell => {
                    let npc = &mut arena.accounts[order.npc_idx as usize];
                    npc.reserved_shares -= order.qty;
                    npc.sellable += order.qty;
                    arena.note_sell_closed(order.npc_idx, order.qty);
                }
            }
            if level.is_empty() {
                side_map.remove(&price);
            }
        }
        Ok(())
    }
}

/// 撮合统计的分离借用包（与簿结构其余字段不相交）。
struct FillStats<'a> {
    fee_sink: &'a mut i64,
    trade_count: &'a mut u64,
    volume_shares: &'a mut u64,
    last_price: &'a mut Option<i64>,
    min_trade_price: &'a mut Option<i64>,
    max_trade_price: &'a mut Option<i64>,
}

/// 单笔成交结算（自由函数，避免 self 与 maker 的借用冲突）：
/// maker（挂单方）与 inc（在途方）互换现金/持仓。外部槽位的账户变更
/// 同步产生 FillReceipt（家端权威记账走回执；席位镜像仅供簿内校验）。
fn settle_fill(
    arena: &mut Arena,
    maker: &mut Order,
    inc: &mut Order,
    q: u32,
    fees: &FeeTable,
    stats: FillStats<'_>,
) -> Result<(), ProtoError> {
    let price = maker.price;
    let value = mul_price_qty(price, q)?;
    if crate::xdbg() {
        eprintln!(
            "[settle] stock={} maker(slot={},side={:?},id={},p={},qty_left={}) inc(slot={},side={:?},id={},p={},qty_left={}) q={} maker_acct_cash={} inc_acct_cash={}",
            arena.stock_idx,
            maker.npc_idx,
            maker.side,
            maker.id,
            maker.price,
            maker.qty,
            inc.npc_idx,
            inc.side,
            inc.id,
            inc.price,
            inc.qty,
            q,
            arena.accounts[maker.npc_idx as usize].cash,
            arena.accounts[inc.npc_idx as usize].cash
        );
    }
    if maker.npc_idx == inc.npc_idx {
        // 工作单对齐后理论不可达；显式失败暴露实现 bug（铁律二）。
        return Err(ProtoError::InvalidConfig(format!(
            "自成交被拒绝：npc={} price={} qty={}",
            maker.npc_idx, price, q
        )));
    }
    *stats.trade_count += 1;
    *stats.volume_shares += q as u64;
    *stats.last_price = Some(price);
    *stats.min_trade_price = Some(stats.min_trade_price.map_or(price, |p| p.min(price)));
    *stats.max_trade_price = Some(stats.max_trade_price.map_or(price, |p| p.max(price)));

    // 佣金增量（委托维度摊销）。
    let buy_fee_delta = if maker.side == Side::Sell {
        commission_step(fees, inc.filled_value, inc.fee_charged, value)?
    } else {
        commission_step(fees, maker.filled_value, maker.fee_charged, value)?
    };
    let sell_fee_delta = if maker.side == Side::Sell {
        commission_step(fees, maker.filled_value, maker.fee_charged, value)?
    } else {
        commission_step(fees, inc.filled_value, inc.fee_charged, value)?
    };
    let stamp = fees.stamp_tax(value)?;

    // 结算对手方解析：maker.side==Sell ⇒ maker 卖、inc 买；反之为 inc 卖、maker 买。
    // （修复记录：本段曾把买腿错算到卖方账户——两腿同账户时现金/股票守恒
    // 恰好仍成立（费用进 sink、股数自相抵消），按股私有臂的审计对此失明；
    // 跨股臂的外部席位（escrow 实体）使该错误以负现金形式显式暴露。）
    let (sell_order, buy_order) = if maker.side == Side::Sell {
        (maker.npc_idx, inc.npc_idx)
    } else {
        (inc.npc_idx, maker.npc_idx)
    };

    // 卖方结算。
    {
        let sell_npc = &mut arena.accounts[sell_order as usize];
        sell_npc.qty = sell_npc
            .qty
            .checked_sub(q)
            .ok_or_else(|| overflow("sell qty", q as i64))?;
        sell_npc.reserved_shares = sell_npc
            .reserved_shares
            .checked_sub(q)
            .ok_or_else(|| overflow("sell reserved", q as i64))?;
        let net = value
            .checked_sub(sell_fee_delta)
            .and_then(|v| v.checked_sub(stamp))
            .ok_or_else(|| overflow("sell net", value))?;
        sell_npc.cash += net;
        if sell_npc.cash < 0 {
            return Err(ProtoError::NegativeCash {
                code: format!(
                    "npc-{} sell@{} q={} net={} desk_cash={} desk_res={} inc(id={},fv={},fc={},rc={}) maker(id={},fv={},fc={},rc={})",
                    sell_order,
                    price,
                    q,
                    net,
                    sell_npc.cash,
                    sell_npc.reserved_cash,
                    inc.id,
                    inc.filled_value,
                    inc.fee_charged,
                    inc.reserved_cash,
                    maker.id,
                    maker.filled_value,
                    maker.fee_charged,
                    maker.reserved_cash
                ),
                npc: sell_order,
                cash: sell_npc.cash,
            });
        }
    }
    arena.note_sell_fill(sell_order, q, value - sell_fee_delta - stamp);
    // 买方结算。
    {
        let buy_npc = &mut arena.accounts[buy_order as usize];
        buy_npc.qty += q; // 总持仓增加（T+1：当日冻结）
        buy_npc.frozen_today += q;
        let charge = value
            .checked_add(buy_fee_delta)
            .ok_or_else(|| overflow("buy charge", value))?;
        buy_npc.cash -= charge;
        if buy_npc.cash < 0 {
            return Err(ProtoError::NegativeCash {
                code: format!(
                    "npc-{} buy@{} q={} charge={} desk_cash={} desk_res={} inc(id={},fv={},fc={},rc={}) maker(id={},fv={},fc={},rc={})",
                    buy_order,
                    price,
                    q,
                    charge,
                    buy_npc.cash,
                    buy_npc.reserved_cash,
                    inc.id,
                    inc.filled_value,
                    inc.fee_charged,
                    inc.reserved_cash,
                    maker.id,
                    maker.filled_value,
                    maker.fee_charged,
                    maker.reserved_cash
                ),
                npc: buy_order,
                cash: buy_npc.cash,
            });
        }
    }
    arena.note_buy_fill(buy_order, q, value + buy_fee_delta);
    // 摊销状态与冻结释放回写到对应 order。
    if maker.side == Side::Sell {
        maker.filled_value += value;
        maker.fee_charged += sell_fee_delta;
        inc.filled_value += value;
        inc.fee_charged += buy_fee_delta;
        let release = inc.reserved_cash.min(value + buy_fee_delta);
        inc.reserved_cash -= release;
        arena.accounts[inc.npc_idx as usize].reserved_cash -= release;
    } else {
        inc.filled_value += value;
        inc.fee_charged += sell_fee_delta;
        maker.filled_value += value;
        maker.fee_charged += buy_fee_delta;
        let release = maker.reserved_cash.min(value + buy_fee_delta);
        maker.reserved_cash -= release;
        arena.accounts[maker.npc_idx as usize].reserved_cash -= release;
    }
    *stats.fee_sink += buy_fee_delta + sell_fee_delta + stamp;
    Ok(())
}

fn forget_by_npc(by_npc: &mut HashMap<u32, Vec<u64>>, npc_idx: u32, id: u64) {
    if let Some(ids) = by_npc.get_mut(&npc_idx) {
        ids.retain(|&x| x != id);
        if ids.is_empty() {
            by_npc.remove(&npc_idx);
        }
    }
}

/// 佣金摊销步进：新总额 − 已收（max(min, rate·x) 对 x 单调 ⇒ 步进 ≥ 0）。
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

fn count_qty(level: &VecDeque<Order>) -> u64 {
    level.iter().map(|o| o.qty as u64).sum()
}
