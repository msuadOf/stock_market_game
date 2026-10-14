//! 交易部线程骨架（一股一线程）：`DeskIn` 收件箱 + `DeskBatch` 处理循环 +
//! `DeskReport` 收束 + 实体退回协议。
//!
//! 骨架边界（诚实登记）：**无撮合**（价格档 FIFO 撮合、费用、涨跌停带、价格笼子、
//! 相位门禁随批 3 引擎化接入）——本骨架只做受理登记、对齐/定向撤单的实体退回、
//! 日终清簿。价格符号仅支持 `Fixed`，其余符号显式失败。
//!
//! 失败原子性（F §5.2 交易部簿行）：撤销日志（COW 粒度）覆盖骨架全部可变状态：
//! 挂单簿、本地序号、resting 计数；Abort 逆序回放 + 清实体出口缓冲。转正 =
//! 下一 tick 首批 `Ops`（tick 字段前进即隐式转正；同 tick 续行轮 Ops 不触发）。

use super::protocol::{
    DeskBatch, DeskIn, DeskMsg, DeskReport, RoutedPrice, RuntimeError, UpMsg, WorkerId,
};
use super::watchdog::{worker_recv, WatchdogConfig};
use super::{DeskReceipt, Entity, OrderRef, Owner, ReceiptKind, StockIdx};
use crate::orderbook::Side;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// 骨架挂单：资源实体随单驻留交易部（买单 purse=剩余可花；卖单 qty=剩余股数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkeletonOrder {
    /// 复合挂单标识 `(stock, 本地单调序号)`。
    pub order_ref: OrderRef,
    /// 所有者。
    pub owner: Owner,
    /// 方向。
    pub side: Side,
    /// 受理价（i64 分；符号已在受理时解析为 Fixed）。
    pub price: i64,
    /// 剩余数量。
    pub qty: u32,
    /// 买单剩余可花现金（卖单恒 0）。
    pub purse: i64,
}

/// 交易部撤销日志条目（逆序回放恢复）。
#[derive(Clone, Debug)]
enum DeskUndo {
    /// 新挂入了订单（回滚=移除）。
    Rested(SkeletonOrder),
    /// 移除了订单（回滚=放回）。
    Removed(SkeletonOrder),
    /// 本地序号前进（回滚=回退）。
    Serial,
    /// resting 计数变动。
    RestingCount(i64),
    /// 累计跳过计数变动。
    Skipped(u64),
}

/// 交易部期末上交状态。
#[derive(Clone, Debug)]
pub struct DeskFinalState {
    /// 股索引。
    pub stock: StockIdx,
    /// 期末挂单（Drain 后为空）。
    pub resting: Vec<SkeletonOrder>,
    /// 累计跳过（受理拒绝）数。
    pub skipped_orders: u64,
    /// 当前态哈希（Abort 回滚后应等于 `committed_hash`）。
    pub state_hash: u64,
    /// 提交态哈希。
    pub committed_hash: u64,
}

/// 交易部私有状态。
pub struct DeskState {
    /// 股索引。
    pub stock: StockIdx,
    /// 挂单簿（骨架档：无价格档结构；批 3 换双边 BTreeMap 价格档 × FIFO）。
    resting: BTreeMap<OrderRef, SkeletonOrder>,
    /// 本地单调序号（下一个）。
    next_serial: u64,
    /// 撤销日志。
    undo: Vec<DeskUndo>,
    /// 实体出口缓冲（每组一个，本轮收束整体 flush）。
    out: Vec<Vec<Entity>>,
    /// 回执出口缓冲（每组一个）。
    receipts_out: Vec<Vec<DeskReceipt>>,
    /// 本轮已 final_flush 的组集合。
    flushed: Vec<bool>,
    /// 当前 tick / 轮。
    cur_tick: u64,
    cur_round: u32,
    /// 已转正 tick。
    committed_tick: u64,
    /// 提交态哈希。
    committed_hash: u64,
    /// 累计受理拒绝数。
    pub skipped_orders: u64,
    /// 挂单数。
    pub resting_count: u64,
}

impl DeskState {
    /// 构造交易部状态。
    pub fn new(stock: StockIdx, n_groups: usize) -> Self {
        DeskState {
            stock,
            resting: BTreeMap::new(),
            next_serial: 1,
            undo: Vec::new(),
            out: vec![Vec::new(); n_groups],
            receipts_out: vec![Vec::new(); n_groups],
            flushed: vec![false; n_groups],
            cur_tick: 0,
            cur_round: 0,
            committed_tick: 0,
            committed_hash: 0,
            skipped_orders: 0,
            resting_count: 0,
        }
    }

    /// 当前态哈希（FNV-1a，BTreeMap 确定序）。仅域内容（簿/序号/计数），不含
    /// 协议位置字段——跨 tick 可比（Abort 回滚等价判据）。
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = fnv(h, self.stock as u64);
        h = fnv(h, self.next_serial);
        h = fnv(h, self.resting_count);
        for (order_ref, o) in &self.resting {
            h = fnv(h, order_ref.0 as u64);
            h = fnv(h, order_ref.1);
            h = fnv(h, o.owner.0 as u64);
            h = fnv(h, o.owner.1 as u64);
            h = fnv(
                h,
                match o.side {
                    Side::Buy => 1u64,
                    Side::Sell => 2u64,
                },
            );
            h = fnv(h, o.price as u64);
            h = fnv(h, o.qty as u64);
            h = fnv(h, o.purse as u64);
        }
        h
    }

    /// 逆序回放撤销日志 + 清出口缓冲（Abort）。
    fn rollback(&mut self) {
        for entry in self.undo.drain(..).rev() {
            match entry {
                DeskUndo::Rested(o) => {
                    self.resting.remove(&o.order_ref);
                }
                DeskUndo::Removed(o) => {
                    self.resting.insert(o.order_ref, o);
                }
                DeskUndo::Serial => {
                    self.next_serial -= 1;
                }
                DeskUndo::RestingCount(d) => {
                    self.resting_count = (self.resting_count as i64 - d) as u64;
                }
                DeskUndo::Skipped(d) => {
                    self.skipped_orders -= d;
                }
            }
        }
        for buf in &mut self.out {
            buf.clear();
        }
        for buf in &mut self.receipts_out {
            buf.clear();
        }
        for f in &mut self.flushed {
            *f = false;
        }
        self.cur_tick = self.committed_tick;
        self.cur_round = 0;
    }

    /// 处理一批 Ops（已由主循环完成 tick/轮防御核对与隐式转正）。
    fn apply_ops(&mut self, ops: Vec<DeskIn>) -> Result<(), RuntimeError> {
        for op in ops {
            match op {
                DeskIn::Place {
                    owner,
                    side,
                    price,
                    qty,
                    purse,
                } => {
                    self.place(owner, side, price, qty, purse)?;
                }
                DeskIn::CancelAll { owner } => {
                    self.cancel_all_for(owner);
                }
                DeskIn::Cancel { owner, order } => {
                    self.cancel_one(owner, order)?;
                }
            }
        }
        Ok(())
    }

    /// 受理下单（骨架档）：解析价格符号 → 基础校验 → 挂簿。
    fn place(
        &mut self,
        owner: Owner,
        side: Side,
        rp: RoutedPrice,
        qty: u32,
        purse: i64,
    ) -> Result<(), RuntimeError> {
        // 符号价（Highest/Lowest/Market）需要活簿与涨跌停带——批 3 接入；
        // 骨架收到符号价=显式失败（不静默按 0 处理）。
        let price = match rp {
            RoutedPrice::Fixed(p) => p,
            other => {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "交易部 {} 骨架不支持符号价 {other:?}（批 3 接入）",
                        self.stock
                    ),
                });
            }
        };
        if price <= 0 || qty == 0 || purse < 0 || (side == Side::Sell && purse != 0) {
            // 带外/非法申报：实体原路退回 + 回执（不静默吞资源）。
            self.reject(owner, side, qty, purse, ReceiptKind::PriceOutOfBand);
            return Ok(());
        }
        if side == Side::Buy && qty % 100 != 0 {
            self.reject(owner, side, qty, purse, ReceiptKind::NotRoundLot);
            return Ok(());
        }
        let order_ref = (self.stock, self.next_serial);
        let order = SkeletonOrder {
            order_ref,
            owner,
            side,
            price,
            qty,
            purse,
        };
        self.next_serial += 1;
        self.undo.push(DeskUndo::Serial);
        self.resting.insert(order_ref, order);
        self.undo.push(DeskUndo::Rested(SkeletonOrder {
            order_ref,
            owner,
            side,
            price,
            qty,
            purse,
        }));
        self.resting_count += 1;
        self.undo.push(DeskUndo::RestingCount(1));
        Ok(())
    }

    /// 对齐撤单：撤该 owner 全部挂单，实体原路回流。
    fn cancel_all_for(&mut self, owner: Owner) {
        let targets: Vec<OrderRef> = self
            .resting
            .values()
            .filter(|o| o.owner == owner)
            .map(|o| o.order_ref)
            .collect();
        for order_ref in targets {
            self.close(order_ref);
        }
    }

    /// 定向撤单：命中则撤余量；未命中=已全成（骨架无撮合 ⇒ 语义近似为已终结），
    /// 显式回执。
    fn cancel_one(&mut self, owner: Owner, order: OrderRef) -> Result<(), RuntimeError> {
        if order.0 != self.stock {
            // 复合标识股前缀指向他股：路由错误=结构性 bug，显式失败。
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("交易部 {} 收到他股撤单：order={order:?}", self.stock),
            });
        }
        match self.resting.get(&order) {
            Some(o) if o.owner == owner => {
                self.close(order);
            }
            Some(_) => {
                // 他人挂单：不变态，显式回执（防御）。
                self.receipts_out[owner.0 as usize].push(DeskReceipt {
                    order,
                    owner,
                    kind: ReceiptKind::AlreadyFilled,
                });
            }
            None => {
                self.receipts_out[owner.0 as usize].push(DeskReceipt {
                    order,
                    owner,
                    kind: ReceiptKind::AlreadyFilled,
                });
            }
        }
        Ok(())
    }

    /// 终结挂单：移簿（记撤销日志）+ 实体出口（买单退款/卖单余股 + Closed 恒发）。
    fn close(&mut self, order_ref: OrderRef) {
        if let Some(order) = self.resting.remove(&order_ref) {
            self.undo.push(DeskUndo::Removed(order.clone()));
            self.resting_count -= 1;
            self.undo.push(DeskUndo::RestingCount(-1));
            let g = order.owner.0 as usize;
            match order.side {
                Side::Buy => {
                    self.out[g].push(Entity::CashRefund {
                        local: order.owner.1,
                        amount: order.purse,
                    });
                }
                Side::Sell => {
                    self.out[g].push(Entity::RefundedShares {
                        local: order.owner.1,
                        qty: order.qty,
                    });
                }
            }
            self.out[g].push(Entity::Closed {
                local: order.owner.1,
            });
        }
    }

    /// 受理拒绝：实体原路退回 + 回执。被拒新单也消耗一个本地序号（OrderRef
    /// 全局唯一口径包含被拒单；恢复按各股已消耗序号重建）。
    fn reject(&mut self, owner: Owner, side: Side, qty: u32, purse: i64, kind: ReceiptKind) {
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
        self.out[g].push(Entity::Closed { local: owner.1 });
        let order = (self.stock, self.next_serial);
        self.next_serial += 1;
        self.undo.push(DeskUndo::Serial);
        self.receipts_out[g].push(DeskReceipt { order, owner, kind });
        self.skipped_orders += 1;
        self.undo.push(DeskUndo::Skipped(1));
    }

    /// 本轮收束：出口缓冲 flush（先于 DeskReport，因果序是正确性基石）。
    fn flush_batches(
        &mut self,
        group_txs: &[Sender<super::protocol::GroupIn>],
        tick: u64,
        round: u32,
    ) -> Result<(u64, u64), RuntimeError> {
        let mut entities_total = 0u64;
        let mut receipts_total = 0u64;
        let stock = self.stock;
        let out = std::mem::take(&mut self.out);
        for (g, buf) in out.into_iter().enumerate() {
            let receipts = std::mem::take(&mut self.receipts_out[g]);
            if buf.is_empty() && receipts.is_empty() {
                continue;
            }
            entities_total += buf.len() as u64;
            receipts_total += receipts.len() as u64;
            let batch = DeskBatch {
                stock,
                tick,
                round,
                entities: buf,
                fills: Vec::new(),
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
        Ok((entities_total, receipts_total))
    }

    /// 强撤全部挂单（Drain：日终清簿语义，实体回流）。
    fn drain_all(&mut self) {
        let targets: Vec<OrderRef> = self.resting.keys().copied().collect();
        for order_ref in targets {
            self.close(order_ref);
        }
    }

    /// 期末上交。
    fn final_state(&self) -> DeskFinalState {
        DeskFinalState {
            stock: self.stock,
            resting: self.resting.values().cloned().collect(),
            skipped_orders: self.skipped_orders,
            state_hash: self.state_hash(),
            committed_hash: self.committed_hash,
        }
    }
}

/// FNV-1a 混合。
fn fnv(state: u64, value: u64) -> u64 {
    let mut h = state;
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 交易部主循环（骨架）。
pub fn desk_main(
    stock: StockIdx,
    n_groups: usize,
    rx: Receiver<DeskMsg>,
    group_txs: Vec<Sender<super::protocol::GroupIn>>,
    up_tx: Sender<UpMsg>,
    abort: Arc<AtomicBool>,
    watchdog: WatchdogConfig,
) -> Result<(), RuntimeError> {
    let who = WorkerId::Desk(stock);
    let mut state = DeskState::new(stock, n_groups);
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
        match &msg {
            DeskMsg::Shutdown => break,
            _ => {}
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_msg(&mut state, &group_txs, &up_tx, msg)
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
                abort.store(true, Ordering::SeqCst);
                let _ = up_tx.send(UpMsg::TaskDied { who, payload });
                break;
            }
        }
    }
    let final_state = state.final_state();
    let send = up_tx.send(UpMsg::DeskFinal(Box::new(final_state)));
    if result.is_ok() {
        if send.is_err() {
            result = Err(RuntimeError::SendFailed {
                detail: format!("交易部 {stock} DeskFinal 发送失败"),
            });
        }
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
        DeskMsg::Ops {
            tick,
            round,
            group,
            ops,
            final_flush,
        } => {
            if tick > state.cur_tick {
                // 下一 tick 首批 Ops(N) = tick N−1 隐式转正（丢弃撤销日志；同 tick
                // 续行轮 Ops 不触发）。跳变（>1）= 协议错误。
                if tick != state.cur_tick + 1 {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} Ops tick 跳变：tick={tick}, 上一执行 tick={}",
                            state.stock, state.cur_tick
                        ),
                    });
                }
                state.undo.clear();
                state.committed_tick = state.cur_tick;
                state.cur_tick = tick;
                state.cur_round = 0;
                state.committed_hash = state.state_hash();
            } else if tick < state.cur_tick {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "交易部 {} 收到过期 tick 消息：{tick} < {}",
                        state.stock, state.cur_tick
                    ),
                });
            }
            if round != state.cur_round {
                if round == state.cur_round + 1 {
                    state.cur_round = round;
                    // 新一轮：重置本轮 final_flush 集合。
                    for f in &mut state.flushed {
                        *f = false;
                    }
                } else {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} Ops 轮次错序：round={round}, 当前={}",
                            state.stock, state.cur_round
                        ),
                    });
                }
            }
            let g = group as usize;
            if g >= state.flushed.len() {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("交易部 {} 收到越界组索引：{group}", state.stock),
                });
            }
            state.apply_ops(ops)?;
            if final_flush {
                if state.flushed[g] {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "交易部 {} 组 {group} 第 ({tick},{round}) 轮重复 final_flush",
                            state.stock
                        ),
                    });
                }
                state.flushed[g] = true;
                if state.flushed.iter().all(|&f| f) {
                    for f in &mut state.flushed {
                        *f = false;
                    }
                    // 本轮撮合输入完毕：实体批次 flush **先于** DeskReport（因果基石）。
                    let (entities_out, receipts_out) =
                        state.flush_batches(group_txs, tick, round)?;
                    let report = DeskReport {
                        stock: state.stock,
                        tick,
                        round,
                        resting_count: state.resting_count,
                        entities_out,
                        receipts_out,
                        committed_hash: state.committed_hash,
                    };
                    up_tx.send(UpMsg::DeskReport(report)).map_err(|_| {
                        RuntimeError::SendFailed {
                            detail: format!("交易部 {} DeskReport 发送失败", state.stock),
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
                        state.stock, state.cur_tick
                    ),
                });
            }
            state.rollback();
        }
        DeskMsg::Drain => {
            state.drain_all();
            state.flush_batches(group_txs, state.cur_tick, state.cur_round)?;
            up_tx
                .send(UpMsg::DeskDrained { stock: state.stock })
                .map_err(|_| RuntimeError::SendFailed {
                    detail: format!("交易部 {} DeskDrained 发送失败", state.stock),
                })?;
        }
        // Shutdown 由主循环拦截，此处防御保留（不可达）。
        DeskMsg::Shutdown => {}
    }
    Ok(())
}
