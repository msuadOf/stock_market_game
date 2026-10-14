//! NPC 组线程骨架：持有组内账户（现金/持仓/在途登记），决策脚本注入（批 4 换
//! `decide_data`），实体扣除 → 分段 flush → 收实体落账。
//!
//! 失败原子性（F §5.2 组账户行）：
//! - **可逆日志**：本 tick 内每次落账（实体应用）与每次决策期扣除、每次
//!   `foreign_active` 增减都记逆操作；Abort 逆序回放恢复起始值。
//! - **转正点**：收到下一 `Start`（隐式：上一 tick 已成功）——落账 staging 后丢弃
//!   全部本 tick 日志。`Continue` 只触发落账（进可逆日志），**不是**转正点。
//! - **Abort**：逆序回放 + 清 staging + 回到 `committed_tick`（失败后可继续重跑）。

use super::decision::{decide_account, NpcProfile};
use super::fees::buy_purse_limit;
use super::protocol::TickSnapshot;
use super::protocol::{
    AdminRight, DayEndDirective, DeskBatch, DeskIn, GroupIn, GroupReport, RoutedPrice,
    RuntimeError, UpMsg, WorkerId,
};
use super::watchdog::{worker_recv, WatchdogConfig};
use super::{Entity, GroupIdx, LocalIdx, StockIdx};
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// 组内账户：现金聚合额 + 每股持仓聚合额 + T+1 冻结 + 每股在途挂单登记。
/// 不变量（期末审计断言）：`shares == sellable + frozen_today`（sellable 派生）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupAccount {
    /// 现金聚合额（i64 分）。
    pub cash: i64,
    /// 每股持仓聚合额（下标=StockIdx）。
    pub shares: Vec<u32>,
    /// 每股当日买入冻结（T+1：日界解冻，非每 tick——批 5 DayEnd 接线）。
    pub frozen_today: Vec<u32>,
    /// 每股在途挂单计数（下标=StockIdx；严格所有权口径的诊断面）。
    pub foreign_active: Vec<u16>,
}

impl GroupAccount {
    /// 构造：给定现金与股数（持仓/冻结/在途清零）。
    pub fn new(cash: i64, n_stocks: usize) -> Self {
        GroupAccount {
            cash,
            shares: vec![0; n_stocks],
            frozen_today: vec![0; n_stocks],
            foreign_active: vec![0; n_stocks],
        }
    }

    /// 可卖（T+1 口径）：持仓 − 当日冻结。
    pub fn sellable(&self, stock: StockIdx) -> u32 {
        self.shares[stock as usize] - self.frozen_today[stock as usize]
    }

    /// 日界解冻：当日买入进入可卖（DayEnd 相位调用；批 5 接线）。
    pub fn unfreeze_t1(&mut self) {
        for f in &mut self.frozen_today {
            *f = 0;
        }
    }
}

/// 组端可逆日志条目：记录“已施加的增量”，回滚时逆序施加 `-delta`。
/// 覆盖：决策期扣除（purse/股票）、实体落账、`foreign_active` 增减。
#[derive(Clone, Copy, Debug)]
enum GroupUndo {
    /// 现金变动（扣除为负、落账为正）。
    Cash { local: LocalIdx, delta: i64 },
    /// 持仓变动（卖出扣除为负、回流为正）。
    Shares {
        local: LocalIdx,
        stock: StockIdx,
        delta: i32,
    },
    /// 在途登记变动（下单 +1、Closed 落账 −1）。
    ForeignActive {
        local: LocalIdx,
        stock: StockIdx,
        delta: i16,
    },
    /// T+1 冻结变动（BoughtShares 落账 +qty）。
    Frozen {
        local: LocalIdx,
        stock: StockIdx,
        delta: i32,
    },
}

/// 决策脚本注入的下单规格（组端换算为 `DeskIn`， purse=对价，费用上限随批 3 补）。
#[derive(Clone, Copy, Debug)]
pub struct PlaceSpec {
    /// 组内名册下标。
    pub local: LocalIdx,
    /// 目标股。
    pub stock: StockIdx,
    /// 方向。
    pub side: Side,
    /// 限价（i64 分；骨架只支持 Fixed）。
    pub price: i64,
    /// 数量。
    pub qty: u32,
}

/// 决策脚本注入的对齐撤单规格。
#[derive(Clone, Copy, Debug)]
pub struct CancelSpec {
    /// 组内名册下标。
    pub local: LocalIdx,
    /// 目标股。
    pub stock: StockIdx,
}

/// 一轮决策计划（骨架档：脚本产出；批 4 换 decide_data + 计划续行）。
#[derive(Clone, Debug, Default)]
pub struct TickPlan {
    /// 本轮下单（同 NPC 的 cancel 恒先于 place——FIFO 对齐语义由脚本顺序保证）。
    pub places: Vec<PlaceSpec>,
    /// 本轮对齐撤单。
    pub cancels: Vec<CancelSpec>,
    /// 本组是否还有续行需求（驱动协调者 Continue）。
    pub continuation: bool,
    /// 注入 panic（panic 运输测试）：处理期以 `panic_any` 抛出。
    pub panic_payload: Option<String>,
    /// 注入停顿（看门狗测试）。
    pub stall: Option<Duration>,
}

/// 组决策脚本：`(tick, round) -> TickPlan`。
pub type GroupScript = Box<dyn Fn(u64, u32) -> TickPlan + Send>;

/// 组线程观察日志（协议时序断言用：实体批次先于下一 Start）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observed {
    /// 收到实体批次（携带产出 tick）。
    Batch(u64),
    /// 收到 Start（携带 tick）。
    Start(u64),
    /// 收到 Continue（携带 tick/round）。
    Continue(u64, u32),
    /// 收到 Abort（携带 tick）。
    Abort(u64),
    /// 收到 DayEnd（携带交易日）。
    DayEnd(u32),
    /// 收到 Shutdown。
    Shutdown,
}

/// 组线程期末上交状态。
#[derive(Clone, Debug)]
pub struct GroupFinalState {
    /// 组索引。
    pub group: GroupIdx,
    /// 期末账户（含 Drain 回流落账）。
    pub accounts: Vec<GroupAccount>,
    /// 累计跳过单量。
    pub skipped_orders: u64,
    /// 累计到期决策次数。
    pub decide_calls_total: u64,
    /// 累计 Cancel 意图跳过数。
    pub cancel_intents_total: u64,
    /// 当前态哈希（Abort 回滚后应等于 `committed_hash`）。
    pub state_hash: u64,
    /// 提交态哈希（最后一次 Start 转正后）。
    pub committed_hash: u64,
    /// 消息观察日志。
    pub observed: Vec<Observed>,
}

/// 组线程私有状态。
pub struct GroupState {
    /// 组索引。
    pub group: GroupIdx,
    /// 组内账户（下标=LocalIdx）。
    pub accounts: Vec<GroupAccount>,
    /// 可逆日志（本 tick 内累积；Start 转正时清空）。
    undo: Vec<GroupUndo>,
    /// 实体批次 staging（tick N 内到达，Start/Continue 时落账）。
    staging: Vec<DeskBatch>,
    /// 当前正在执行的 tick（Abort 后回退到 committed_tick）。
    cur_tick: u64,
    /// 当前轮次。
    cur_round: u32,
    /// 已转正 tick。
    committed_tick: u64,
    /// 提交态哈希（转正后计算）。
    committed_hash: u64,
    /// 累计跳过单量。
    pub skipped_orders: u64,
    /// 累计到期 NPC 决策次数（attention due）。
    pub decide_calls_total: u64,
    /// 累计被登记跳过的 Cancel 意图数。
    pub cancel_intents_total: u64,
    /// 分段 flush 粒度（op 数）。
    chunk: usize,
    /// 一手股数（买入整手/卖出零股一次性校验）。
    lot: u32,
    /// 费用/整手配置（purse 最坏情况口径）。
    config: Arc<GameConfig>,
    /// 决策装配（Some=内联 decide_data 路径；None=测试脚本路径）。
    decision: Option<(Vec<NpcProfile>, u32)>,
    /// 每交易日 tick 数（T+1 日界解冻判定）。
    ticks_per_day: u64,
    /// 最近一次广播快照（决策观察基；Continue 轮复用）。
    snapshot: Option<std::sync::Arc<TickSnapshot>>,
    /// 消息观察日志。
    pub observed: Vec<Observed>,
    /// 决策脚本。
    script: GroupScript,
}

impl GroupState {
    /// 构造组状态。
    pub fn new(
        group: GroupIdx,
        accounts: Vec<GroupAccount>,
        chunk: usize,
        lot: u32,
        config: Arc<GameConfig>,
        ticks_per_day: u64,
        decision: Option<(Vec<NpcProfile>, u32)>,
        script: GroupScript,
    ) -> Self {
        GroupState {
            group,
            accounts,
            undo: Vec::new(),
            staging: Vec::new(),
            cur_tick: 0,
            cur_round: 0,
            committed_tick: 0,
            committed_hash: 0,
            skipped_orders: 0,
            decide_calls_total: 0,
            cancel_intents_total: 0,
            chunk,
            lot,
            config,
            ticks_per_day,
            decision,
            snapshot: None,
            observed: Vec::new(),
            script,
        }
    }

    /// 当前态哈希（FNV-1a，按名册与股序确定序遍历）。仅域内容（账户），不含
    /// 协议位置字段（tick/轮）——跨 tick 可比（Abort 回滚等价判据）。
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = fnv(h, self.group as u64);
        for a in &self.accounts {
            h = fnv(h, a.cash as u64);
            for &s in &a.shares {
                h = fnv(h, s as u64);
            }
            for &f in &a.frozen_today {
                h = fnv(h, f as u64);
            }
            for &f in &a.foreign_active {
                h = fnv(h, f as u64);
            }
        }
        h
    }

    /// 逆序回放可逆日志（Abort/panic 回滚）：恢复到本 tick 起始（上一收敛态）。
    fn rollback(&mut self) {
        for entry in self.undo.drain(..).rev() {
            match entry {
                GroupUndo::Cash { local, delta } => {
                    self.accounts[local as usize].cash -= delta;
                }
                GroupUndo::Shares {
                    local,
                    stock,
                    delta,
                } => {
                    let slot = &mut self.accounts[local as usize].shares[stock as usize];
                    *slot = (*slot as i64 - delta as i64) as u32;
                }
                GroupUndo::ForeignActive {
                    local,
                    stock,
                    delta,
                } => {
                    let slot = &mut self.accounts[local as usize].foreign_active[stock as usize];
                    *slot = (*slot as i32 - delta as i32) as u16;
                }
                GroupUndo::Frozen {
                    local,
                    stock,
                    delta,
                } => {
                    let slot = &mut self.accounts[local as usize].frozen_today[stock as usize];
                    *slot = (*slot as i64 - delta as i64) as u32;
                }
            }
        }
        self.staging.clear();
        self.cur_tick = self.committed_tick;
        self.cur_round = 0;
    }

    fn acct_mut(&mut self, local: LocalIdx) -> Result<&mut GroupAccount, RuntimeError> {
        let group = self.group;
        self.accounts
            .get_mut(local as usize)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} 收到越界 local={local}"),
            })
    }

    /// 落账 staging 中属于 `(expected_tick, expected_round)` 的批次（记逆操作）。
    fn apply_staging(
        &mut self,
        expected_tick: u64,
        expected_round: u32,
    ) -> Result<(), RuntimeError> {
        let batches = std::mem::take(&mut self.staging);
        for batch in batches {
            batch
                .verify_tick_round(expected_tick, expected_round)
                .map_err(|m| RuntimeError::ProtocolInvariant {
                    detail: format!("组 {} staging 批次防御核对失败：{m}", self.group),
                })?;
            let stock = batch.stock as usize;
            if stock >= self.accounts.first().map(|a| a.shares.len()).unwrap_or(0) {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("组 {} staging 批次股索引越界：{stock}", self.group),
                });
            }
            for entity in batch.entities {
                self.apply_entity(stock as StockIdx, entity)?;
            }
        }
        Ok(())
    }

    /// 落账单条实体（记逆操作）。
    fn apply_entity(&mut self, stock: StockIdx, e: Entity) -> Result<(), RuntimeError> {
        let group = self.group;
        match e {
            Entity::CashProceeds { local, amount } | Entity::CashRefund { local, amount } => {
                let acct = self.acct_mut(local)?;
                acct.cash = acct.cash.checked_add(amount).ok_or_else(|| {
                    RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} 现金落账溢出：local={local}, amount={amount}"),
                    }
                })?;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: amount,
                });
            }
            Entity::BoughtShares { local, qty } => {
                // T+1：当日买入冻结，日界解冻（下一交易日可卖）。
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.shares[stock as usize];
                *slot = slot
                    .checked_add(qty)
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {group} 持仓落账溢出：local={local}, stock={stock}, qty={qty}"
                        ),
                    })?;
                acct.frozen_today[stock as usize] += qty;
                self.undo.push(GroupUndo::Shares {
                    local,
                    stock,
                    delta: qty as i32,
                });
                self.undo.push(GroupUndo::Frozen {
                    local,
                    stock,
                    delta: qty as i32,
                });
            }
            Entity::RefundedShares { local, qty } => {
                // 卖单余股回流：恢复可卖（不新增冻结）。
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.shares[stock as usize];
                *slot = slot
                    .checked_add(qty)
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {group} 持仓落账溢出：local={local}, stock={stock}, qty={qty}"
                        ),
                    })?;
                self.undo.push(GroupUndo::Shares {
                    local,
                    stock,
                    delta: qty as i32,
                });
            }
            Entity::Closed { local } => {
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.foreign_active[stock as usize];
                if *slot == 0 {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {} Closed 落账使在途登记为负：local={local}, stock={stock}",
                            self.group
                        ),
                    });
                }
                *slot -= 1;
                self.undo.push(GroupUndo::ForeignActive {
                    local,
                    stock,
                    delta: -1,
                });
            }
        }
        Ok(())
    }

    /// 处理 `Start{tick}`：收到 `Start(N)` 即证明 tick N−1 已收敛——落账 staging →
    /// 转正（清可逆日志、`committed_tick = N−1`）→ 本轮决策。
    fn handle_start(
        &mut self,
        tick: u64,
        desk_txs: &[Sender<super::protocol::DeskMsg>],
        up_tx: &Sender<UpMsg>,
    ) -> Result<(), RuntimeError> {
        if tick != self.cur_tick + 1 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} Start 错序：tick={tick}, 上一执行 tick={}",
                    self.group, self.cur_tick
                ),
            });
        }
        // 先落账上一 tick 最后一轮的实体批次（tick/round=上一执行位）。
        self.apply_staging(self.cur_tick, self.cur_round)?;
        // 转正点：丢弃上一 tick 可逆日志；committed 推进到 N−1（旧 cur_tick）。
        self.undo.clear();
        self.committed_tick = self.cur_tick;
        self.cur_tick = tick;
        self.cur_round = 0;
        // T+1 日界解冻移至 DayEnd 相位（F §6.2，批 5）：Start 不再承担。
        self.committed_hash = self.state_hash();
        self.run_round(tick, 0, desk_txs, up_tx)
    }

    /// 日结相位（F §6.2 步骤 2/3）：Drain 回流落账 → T+1 解冻 → AdminBatch
    /// 权益落账——全部进可逆日志（日结校验失败 ⇒ Abort 整体冲回上一交易日
    /// 收敛态）；成功则由下一 `Start` 隐式转正（可逆日志退役=窗口关闭）。
    fn handle_day_end(&mut self, directive: &DayEndDirective) -> Result<(), RuntimeError> {
        if directive.tick != self.cur_tick {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} DayEnd tick 错配：{} != {}",
                    self.group, directive.tick, self.cur_tick
                ),
            });
        }
        // ⓪ 隐式转正上一 tick（日终 tick 已收敛——DayEnd 只在收束后发生）：
        // 隔离日结可逆日志，Abort 只冲回日结变更（回到上一交易日收敛态）。
        self.undo.clear();
        self.committed_tick = self.cur_tick;
        // ① Drain 回流落账（清空在途）。
        self.apply_staging(self.cur_tick, self.cur_round)?;
        // ② T+1 日界解冻（当日买入进入可卖）。
        for local in 0..self.accounts.len() as LocalIdx {
            let frozen = self.accounts[local as usize].frozen_today.clone();
            for (stock, &f) in frozen.iter().enumerate() {
                if f > 0 {
                    self.accounts[local as usize].frozen_today[stock] = 0;
                    self.undo.push(GroupUndo::Frozen {
                        local,
                        stock: stock as StockIdx,
                        delta: -(f as i32),
                    });
                }
            }
        }
        // ③ AdminBatch 权益落账（可逆）。
        for right in &directive.rights {
            self.apply_right(*right)?;
        }
        Ok(())
    }

    /// 单条权益落账（记逆操作）。
    fn apply_right(&mut self, right: AdminRight) -> Result<(), RuntimeError> {
        let group = self.group;
        match right {
            AdminRight::CashDividend {
                local,
                stock,
                amount,
            } => {
                let acct = self.acct_mut(local)?;
                acct.cash = acct.cash.checked_add(amount).ok_or_else(|| {
                    RuntimeError::ProtocolInvariant {
                        detail: format!("股利入账溢出：local={local}, stock={stock}"),
                    }
                })?;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: amount,
                });
            }
            AdminRight::ShareSplit {
                local,
                stock,
                bonus_qty,
            } => {
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.shares[stock as usize];
                *slot =
                    slot.checked_add(bonus_qty)
                        .ok_or_else(|| RuntimeError::ProtocolInvariant {
                            detail: format!("送转股溢出：local={local}, stock={stock}"),
                        })?;
                self.undo.push(GroupUndo::Shares {
                    local,
                    stock,
                    delta: bonus_qty as i32,
                });
            }
            AdminRight::WithholdingTax {
                local,
                stock,
                amount,
            } => {
                let acct = self.acct_mut(local)?;
                if acct.cash < amount {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {group} 扣税现金不足：local={local}, stock={stock}, cash={}, tax={amount}",
                            acct.cash
                        ),
                    });
                }
                acct.cash -= amount;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: -amount,
                });
            }
        }
        Ok(())
    }

    /// 处理 `Continue{tick, round}`：落账本轮实体（进可逆日志，不转正）→ 续行决策。
    fn handle_continue(
        &mut self,
        tick: u64,
        round: u32,
        desk_txs: &[Sender<super::protocol::DeskMsg>],
        up_tx: &Sender<UpMsg>,
    ) -> Result<(), RuntimeError> {
        if tick != self.cur_tick || round != self.cur_round + 1 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} Continue 错序：tick={tick}, round={round}, 当前=({}, {})",
                    self.group, self.cur_tick, self.cur_round
                ),
            });
        }
        // 上一轮实体批次落账（tick/round=上一执行位）。
        self.apply_staging(self.cur_tick, self.cur_round)?;
        self.cur_round = round;
        self.run_round(tick, round, desk_txs, up_tx)
    }

    /// 执行一轮：脚本计划 → 实体扣除（记逆操作）→ 分段 flush → GroupReport。
    fn run_round(
        &mut self,
        tick: u64,
        round: u32,
        desk_txs: &[Sender<super::protocol::DeskMsg>],
        up_tx: &Sender<UpMsg>,
    ) -> Result<(), RuntimeError> {
        // 决策来源：内联 decide_data（批 4）或测试脚本。
        enum Pending {
            Op(super::decision::PendingOp),
            CancelAll { local: LocalIdx, stock: StockIdx },
        }
        let mut pendings: Vec<Pending> = Vec::new();
        let mut continuation = false;
        let mut due_npcs = 0u64;
        let mut decide_calls = 0u64;
        let mut cancel_intents_skipped = 0u64;
        let mut panic_payload: Option<String> = None;
        let mut stall: Option<Duration> = None;
        if let Some((profiles, gap)) = &mut self.decision {
            let snapshot =
                self.snapshot
                    .clone()
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {} 决策缺快照（Start 未达即 Continue？）", self.group),
                    })?;
            let group_mix = (self.group as u64).wrapping_mul(0x9E3779B9);
            for local in 0..profiles.len() as LocalIdx {
                let account = &self.accounts[local as usize];
                let (ops, decided, cancels) = decide_account(
                    &mut profiles[local as usize],
                    group_mix,
                    tick,
                    local,
                    *gap,
                    &snapshot,
                    account,
                    &self.config,
                );
                if decided {
                    due_npcs += 1;
                    decide_calls += 1;
                    self.decide_calls_total += 1;
                }
                cancel_intents_skipped += cancels as u64;
                self.cancel_intents_total += cancels as u64;
                for op in ops {
                    pendings.push(Pending::Op(op));
                }
            }
        } else {
            let plan = (self.script)(tick, round);
            panic_payload = plan.panic_payload;
            stall = plan.stall;
            continuation = plan.continuation;
            for c in &plan.cancels {
                pendings.push(Pending::CancelAll {
                    local: c.local,
                    stock: c.stock,
                });
            }
            for p in &plan.places {
                pendings.push(Pending::Op(super::decision::PendingOp {
                    local: p.local,
                    stock: p.stock,
                    side: p.side,
                    routed: RoutedPrice::Fixed(p.price),
                    purse_reference: p.price,
                    qty: p.qty,
                }));
            }
        }
        if let Some(payload) = panic_payload {
            std::panic::panic_any(payload);
        }
        if let Some(stall) = stall {
            std::thread::sleep(stall);
        }
        let group = self.group;
        let n_stocks = self.accounts.first().map(|a| a.shares.len()).unwrap_or(0);
        let mut buffers: Vec<Vec<DeskIn>> = vec![Vec::new(); n_stocks];
        let mut buffered = 0usize;
        let mut ops_sent = 0u64;
        // 对齐撤单先于下单（同批次 FIFO；脚本顺序保证）。
        for c in pendings.iter().filter_map(|p| match p {
            Pending::CancelAll { local, stock } => Some((*local, *stock)),
            _ => None,
        }) {
            let active = self.accounts[c.0 as usize].foreign_active[c.1 as usize];
            if active > 0 {
                buffers[c.1 as usize].push(DeskIn::CancelAll {
                    owner: (group, c.0),
                });
                buffered += 1;
            }
        }
        for p in pendings.iter().filter_map(|p| match p {
            Pending::Op(op) => Some(*op),
            _ => None,
        }) {
            let owner = (group, p.local);
            match p.side {
                Side::Buy => {
                    // purse = 最坏情况对价 + 佣金上限 + 过户费上限（F §1.5；
                    // `fees.rs buy_purse_limit`，与现引擎预留同口径。符号价/市价
                    // 的最坏情况 = 日涨停（`purse_reference`，F §1.5）。
                    let purse = buy_purse_limit(
                        &self.config,
                        Money::from_cents(p.purse_reference.max(1)),
                        p.qty,
                        Money::ZERO,
                    )
                    .map_err(|e| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} purse 计算失败：{e:?}"),
                    })?
                    .cents();
                    let acct = self.acct_mut(p.local)?;
                    if acct.cash < purse {
                        // 严格所有权：在途退款未到账 ⇒ 显式 skip 计数（不静默、不部分执行）。
                        self.skipped_orders += 1;
                        continue;
                    }
                    acct.cash -= purse;
                    acct.foreign_active[p.stock as usize] += 1;
                    self.undo.push(GroupUndo::Cash {
                        local: p.local,
                        delta: -purse,
                    });
                    self.undo.push(GroupUndo::ForeignActive {
                        local: p.local,
                        stock: p.stock,
                        delta: 1,
                    });
                    buffers[p.stock as usize].push(DeskIn::Place {
                        owner,
                        side: p.side,
                        price: p.routed,
                        qty: p.qty,
                        purse,
                    });
                    buffered += 1;
                }
                Side::Sell => {
                    let lot = self.lot;
                    let acct = self.acct_mut(p.local)?;
                    let sellable = acct.sellable(p.stock);
                    if p.qty > sellable {
                        // 含 T+1：当日买入不可卖。
                        self.skipped_orders += 1;
                        continue;
                    }
                    // 零股须一次性卖出：非整手数量只允许恰好等于全部可卖零头。
                    if p.qty % lot != 0 && p.qty != sellable {
                        self.skipped_orders += 1;
                        continue;
                    }
                    let slot = &mut acct.shares[p.stock as usize];
                    *slot -= p.qty;
                    acct.foreign_active[p.stock as usize] += 1;
                    self.undo.push(GroupUndo::Shares {
                        local: p.local,
                        stock: p.stock,
                        delta: -(p.qty as i32),
                    });
                    self.undo.push(GroupUndo::ForeignActive {
                        local: p.local,
                        stock: p.stock,
                        delta: 1,
                    });
                    buffers[p.stock as usize].push(DeskIn::Place {
                        owner,
                        side: p.side,
                        price: p.routed,
                        qty: p.qty,
                        purse: 0,
                    });
                    buffered += 1;
                }
            }
            if buffered >= self.chunk {
                ops_sent += flush_buffers(&mut buffers, desk_txs, tick, round, group, false)?;
                buffered = 0;
            }
        }
        // 名册末尾：每股发 final_flush 空批作为本组本轮提交标记（可空）。
        ops_sent += flush_buffers(&mut buffers, desk_txs, tick, round, group, true)?;
        let report = GroupReport {
            group,
            tick,
            round,
            ops_sent,
            skipped: self.skipped_orders,
            continuation,
            committed_hash: self.committed_hash,
            due_npcs,
            decide_calls,
            cancel_intents_skipped,
        };
        up_tx
            .send(UpMsg::GroupReport(report))
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("组 {group} GroupReport 发送失败"),
            })
    }

    /// 期末上交。
    fn final_state(&self) -> GroupFinalState {
        GroupFinalState {
            group: self.group,
            accounts: self.accounts.clone(),
            skipped_orders: self.skipped_orders,
            decide_calls_total: self.decide_calls_total,
            cancel_intents_total: self.cancel_intents_total,
            state_hash: self.state_hash(),
            committed_hash: self.committed_hash,
            observed: self.observed.clone(),
        }
    }
}

/// 分段 flush：非空缓冲直发目标交易部；`final` 时每股必发（空批=提交标记）。
fn flush_buffers(
    buffers: &mut [Vec<DeskIn>],
    desk_txs: &[Sender<super::protocol::DeskMsg>],
    tick: u64,
    round: u32,
    group: GroupIdx,
    final_flush: bool,
) -> Result<u64, RuntimeError> {
    let mut sent = 0u64;
    for (stock, buf) in buffers.iter_mut().enumerate() {
        if buf.is_empty() && !final_flush {
            continue;
        }
        let ops = std::mem::take(buf);
        sent += ops.len() as u64;
        desk_txs[stock]
            .send(super::protocol::DeskMsg::Ops {
                tick,
                round,
                group,
                ops,
                final_flush,
            })
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("组 {group} → 交易部 {stock} Ops 发送失败"),
            })?;
    }
    Ok(sent)
}

/// FNV-1a 混合（数值直传）。
fn fnv(state: u64, value: u64) -> u64 {
    let mut h = state;
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 组线程主循环（骨架）。panic 运输：处理体 `catch_unwind` 捕获 → 回滚自身
/// overlay → `TaskDied`（原 payload）→ Final 上交 → 退出。
pub fn group_main(
    state: GroupState,
    desk_txs: Vec<Sender<super::protocol::DeskMsg>>,
    rx: Receiver<GroupIn>,
    up_tx: Sender<UpMsg>,
    abort: Arc<AtomicBool>,
    watchdog: WatchdogConfig,
) -> Result<(), RuntimeError> {
    let who = WorkerId::Group(state.group);
    let mut state = state;
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
        match msg {
            GroupIn::Batch(batch) => {
                state.observed.push(Observed::Batch(batch.tick));
                state.staging.push(batch);
            }
            GroupIn::Start { tick, snapshot } => {
                state.observed.push(Observed::Start(tick));
                if snapshot.tick != tick {
                    abort.store(true, Ordering::SeqCst);
                    let e = RuntimeError::ProtocolInvariant {
                        detail: format!("组 {} Start 快照 tick 不一致", state.group),
                    };
                    let _ = up_tx.send(UpMsg::Failed {
                        who,
                        error: e.clone(),
                    });
                    result = Err(e);
                    break;
                }
                state.snapshot = Some(snapshot);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.handle_start(tick, &desk_txs, &up_tx)
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
                        // 回滚自身 overlay（可逆日志逆序回放），上交已收敛态后退出。
                        state.rollback();
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::TaskDied { who, payload });
                        break;
                    }
                }
            }
            GroupIn::Continue { tick, round } => {
                state.observed.push(Observed::Continue(tick, round));
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.handle_continue(tick, round, &desk_txs, &up_tx)
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
            GroupIn::Abort { tick } => {
                state.observed.push(Observed::Abort(tick));
                state.rollback();
            }
            GroupIn::DayEnd(directive) => {
                state.observed.push(Observed::DayEnd(directive.day));
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.handle_day_end(&directive)
                }));
                match outcome {
                    Ok(Ok(())) => {
                        let done = UpMsg::GroupDayDone {
                            group: state.group,
                            day: directive.day,
                            accounts: state.accounts.clone(),
                            state_hash: state.state_hash(),
                        };
                        if up_tx.send(done).is_err() {
                            abort.store(true, Ordering::SeqCst);
                            result = Err(RuntimeError::SendFailed {
                                detail: format!("组 {who} GroupDayDone 发送失败"),
                            });
                            break;
                        }
                    }
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
            GroupIn::Shutdown => {
                state.observed.push(Observed::Shutdown);
                // Drain 实体批次已先于 Shutdown 入队（FIFO）：期末名册须含全部回流。
                let (cur_tick, cur_round) = (state.cur_tick, state.cur_round);
                if let Err(e) = state.apply_staging(cur_tick, cur_round) {
                    abort.store(true, Ordering::SeqCst);
                    let _ = up_tx.send(UpMsg::Failed {
                        who,
                        error: e.clone(),
                    });
                    result = Err(e);
                    break;
                }
                break;
            }
        }
    }
    let final_state = state.final_state();
    let send = up_tx.send(UpMsg::GroupFinal(Box::new(final_state)));
    if result.is_ok() {
        if let Err(_e) = send {
            result = Err(RuntimeError::SendFailed {
                detail: format!("组 {} GroupFinal 发送失败", who),
            });
        }
    }
    result
}
