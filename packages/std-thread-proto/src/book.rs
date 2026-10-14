//! 本股订单簿撮合（worker 单线程私有，无锁）。
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

use crate::npc::{FeeTable, Npc};
use crate::ProtoError;
use engine::orderbook::Side;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// 挂单/在途单统一形态（在途单不进簿，复用字段）。
#[derive(Clone, Debug)]
pub struct Order {
    pub id: u64,
    /// 本股名册索引。
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
    /// npc_idx -> 活跃挂单 id（工作单对齐用）。
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

    /// 下单入口：先冻结（买资金/卖可卖），再对价撮合，剩余挂簿。
    pub fn place(
        &mut self,
        npcs: &mut [Npc],
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
            let npc = &mut npcs[npc_idx as usize];
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
        let remaining = self.match_incoming(npcs, &mut inc, fees)?;
        if remaining == 0 {
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
        npcs: &mut [Npc],
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
            // 解构出不相交的可变字段借用：对面簿、索引、npc 索引、统计。
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
                settle_fill(npcs, maker, inc, q, fees, stats)?;
                inc.qty -= q;
                maker.qty -= q;
                if maker.qty == 0 {
                    let done = level.pop_front().expect("front 已确认存在");
                    index.remove(&done.id);
                    forget_by_npc(by_npc, done.npc_idx, done.id);
                    *resting_count -= 1;
                    // 买单完全成交：释放剩余冻结。
                    if done.reserved_cash > 0 {
                        npcs[done.npc_idx as usize].reserved_cash -= done.reserved_cash;
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

    /// 工作单对齐：撤掉某 NPC 的全部挂单（两侧），释放资金/可卖。
    pub fn cancel_all_for(&mut self, npcs: &mut [Npc], npc_idx: u32) -> Result<u64, ProtoError> {
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
            let npc = &mut npcs[order.npc_idx as usize];
            match side {
                Side::Buy => npc.reserved_cash -= order.reserved_cash,
                Side::Sell => {
                    npc.reserved_shares -= order.qty;
                    npc.sellable += order.qty;
                }
            }
            if level.is_empty() {
                side_map.remove(&price);
            }
        }
        Ok(cancelled)
    }

    /// 到期撤销：expire_tick ≤ tick 的挂单全部撤出并释放占用。
    pub fn expire_due(&mut self, npcs: &mut [Npc], tick: u64) -> Result<(), ProtoError> {
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
            let npc = &mut npcs[order.npc_idx as usize];
            match side {
                Side::Buy => npc.reserved_cash -= order.reserved_cash,
                Side::Sell => {
                    npc.reserved_shares -= order.qty;
                    npc.sellable += order.qty;
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
/// maker（挂单方）与 inc（在途方）互换现金/持仓。
fn settle_fill(
    npcs: &mut [Npc],
    maker: &mut Order,
    inc: &mut Order,
    q: u32,
    fees: &FeeTable,
    stats: FillStats<'_>,
) -> Result<(), ProtoError> {
    let price = maker.price;
    let value = mul_price_qty(price, q)?;
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

    let (sell_order, buy_order_is_maker) = if maker.side == Side::Sell {
        ((maker.npc_idx), true)
    } else {
        ((inc.npc_idx), false)
    };
    let buy_order = if buy_order_is_maker { maker.npc_idx } else { inc.npc_idx };

    // 卖方结算。
    {
        let sell_npc = &mut npcs[sell_order as usize];
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
                code: format!("npc-{}", sell_order),
                npc: sell_order,
                cash: sell_npc.cash,
            });
        }
    }
    // 买方结算。
    {
        let buy_npc = &mut npcs[buy_order as usize];
        buy_npc.qty += q; // 总持仓增加（T+1：当日冻结）
        buy_npc.frozen_today += q;
        let charge = value
            .checked_add(buy_fee_delta)
            .ok_or_else(|| overflow("buy charge", value))?;
        buy_npc.cash -= charge;
        if buy_npc.cash < 0 {
            return Err(ProtoError::NegativeCash {
                code: format!("npc-{}", buy_order),
                npc: buy_order,
                cash: buy_npc.cash,
            });
        }
    }
    // 摊销状态与冻结释放回写到对应 order。
    if maker.side == Side::Sell {
        maker.filled_value += value;
        maker.fee_charged += sell_fee_delta;
        inc.filled_value += value;
        inc.fee_charged += buy_fee_delta;
        let release = inc.reserved_cash.min(value + buy_fee_delta);
        inc.reserved_cash -= release;
        npcs[inc.npc_idx as usize].reserved_cash -= release;
    } else {
        inc.filled_value += value;
        inc.fee_charged += sell_fee_delta;
        maker.filled_value += value;
        maker.fee_charged += buy_fee_delta;
        let release = maker.reserved_cash.min(value + buy_fee_delta);
        maker.reserved_cash -= release;
        npcs[maker.npc_idx as usize].reserved_cash -= release;
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
