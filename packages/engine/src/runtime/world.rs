//! 世界拓扑（批 2 骨架，非 wasm）：G 个组线程 + N 个交易部线程 + 协调者
//! （协调者逻辑在调用线程上运行，不单独建线程）。
//!
//! 通道拓扑（F §4.2，全程 std::sync::mpsc）：
//! - 组↔交易部全互联直发（G×N 下行 Ops + N×G 上行实体批次）——数据面不经协调者；
//! - 每组一条协调者下行（Start/Continue/Abort/Shutdown）；
//! - 每交易部一条协调者下行（Abort/Drain/Shutdown）；
//! - 单一上行收束 channel（全部 worker 共用）。
//!
//! 代码本体零线程数管理概念：线程数 = G+N(+调用线程)，由世界构造参数决定。

use super::coordinator::{
    broadcast_continue, broadcast_start, converge_round, no_op_validator, GlobalState,
    RoundOutcome, TickEvidence, Validator,
};
use super::desk::{desk_main, DeskFinalState};
use super::group::{group_main, GroupAccount, GroupFinalState, GroupScript};
use super::protocol::{DeskMsg, GroupIn, RuntimeError, UpMsg, WorkerId};
use super::watchdog::{recv_deadline, Phase, WatchdogConfig};
use super::{GroupIdx, StockIdx};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// 世界构造参数。
pub struct WorldSpec {
    /// 组数 G。
    pub n_groups: usize,
    /// 股数 N（交易部数）。
    pub n_stocks: usize,
    /// 每组账户数。
    pub accounts_per_group: usize,
    /// 每账户初始现金（i64 分）。
    pub initial_cash: i64,
    /// 初始持仓 `[group][local][stock]`（缺省零）。
    pub initial_shares: Vec<Vec<Vec<u32>>>,
    /// 组决策脚本工厂。
    pub script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
    /// 组端分段 flush 粒度（op 数）。
    pub chunk: usize,
    /// 看门狗阈值。
    pub watchdog: WatchdogConfig,
    /// 同 tick 续行轮数上限（异常形状显式登记）。
    pub rounds_cap: u32,
}

/// 每个已收敛 tick 的记录（测试与审计面）。
#[derive(Clone, Debug)]
pub struct TickRecord {
    /// tick 编号。
    pub tick: u64,
    /// 实际轮数。
    pub rounds: u32,
    /// 各组提交态哈希。
    pub group_hashes: Vec<(GroupIdx, u64)>,
    /// 各交易部提交态哈希。
    pub desk_hashes: Vec<(StockIdx, u64)>,
}

/// 关停报告（期末对账面）。
#[derive(Clone, Debug)]
pub struct ShutdownReport {
    /// 组期末状态（下标=组）。
    pub group_finals: Vec<GroupFinalState>,
    /// 交易部期末状态（下标=股）。
    pub desk_finals: Vec<DeskFinalState>,
    /// worker 线程返回值（Err=线程主循环显式失败，已通过 Failed 上抛过）。
    pub worker_results: Vec<(WorkerId, Result<(), RuntimeError>)>,
}

/// 世界（协调者视角持有全部通道与线程句柄）。
pub struct World {
    /// 全局域权威（已收敛态）。
    pub global: GlobalState,
    /// 已收敛 tick 记录。
    pub report_log: Vec<TickRecord>,
    group_txs: Vec<Sender<GroupIn>>,
    desk_txs: Vec<Sender<DeskMsg>>,
    up_rx: Receiver<UpMsg>,
    handles: Vec<(WorkerId, std::thread::JoinHandle<Result<(), RuntimeError>>)>,
    abort: Arc<AtomicBool>,
    watchdog: WatchdogConfig,
    validator: Validator,
    rounds_cap: u32,
    n_groups: usize,
    n_stocks: usize,
    /// 已死亡 worker（Failed 路径收齐的 Final；world 不可再跑 tick）。
    dead_finals: Option<ShutdownReport>,
    dead_reason: Option<String>,
}

impl World {
    /// 构造世界并启动全部 worker 线程。
    pub fn spawn(spec: WorldSpec) -> Result<Self, RuntimeError> {
        if spec.n_groups == 0 || spec.n_groups > GroupIdx::MAX as usize {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组数需 1~{}，收到 {}", GroupIdx::MAX, spec.n_groups),
            });
        }
        if spec.n_stocks == 0 || spec.n_stocks > StockIdx::MAX as usize {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("股数需 1~{}，收到 {}", StockIdx::MAX, spec.n_stocks),
            });
        }
        if spec.chunk == 0 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: "chunk 需为正（分段 flush 粒度）".to_owned(),
            });
        }
        let WorldSpec {
            n_groups,
            n_stocks,
            accounts_per_group,
            initial_cash,
            initial_shares,
            script_for,
            chunk,
            watchdog,
            rounds_cap,
        } = spec;
        let abort = Arc::new(AtomicBool::new(false));
        let (up_tx, up_rx) = channel::<UpMsg>();
        let mut group_rxs = Vec::with_capacity(n_groups);
        let mut group_txs = Vec::with_capacity(n_groups);
        for _ in 0..n_groups {
            let (tx, rx) = channel::<GroupIn>();
            group_txs.push(tx);
            group_rxs.push(rx);
        }
        let mut desk_rxs = Vec::with_capacity(n_stocks);
        let mut desk_txs = Vec::with_capacity(n_stocks);
        for _ in 0..n_stocks {
            let (tx, rx) = channel::<DeskMsg>();
            desk_txs.push(tx);
            desk_rxs.push(rx);
        }
        let mut handles = Vec::with_capacity(n_groups + n_stocks);
        for g in 0..n_groups as GroupIdx {
            let mut accounts = Vec::with_capacity(accounts_per_group);
            for l in 0..accounts_per_group {
                let mut acct = GroupAccount::new(initial_cash, n_stocks);
                if let Some(shares) = initial_shares
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    acct.shares.copy_from_slice(shares);
                }
                accounts.push(acct);
            }
            let state = super::group::GroupState::new(g, accounts, chunk, script_for(g));
            let rx = group_rxs.remove(0);
            let handle = std::thread::Builder::new()
                .name(format!("rt-group-{g}"))
                .spawn({
                    let desk_txs = desk_txs.clone();
                    let up_tx = up_tx.clone();
                    let abort = Arc::clone(&abort);
                    let watchdog = watchdog;
                    move || group_main(state, desk_txs, rx, up_tx, abort, watchdog)
                })
                .map_err(|e| RuntimeError::SendFailed {
                    detail: format!("spawn rt-group-{g} 失败：{e}"),
                })?;
            handles.push((WorkerId::Group(g), handle));
        }
        for s in 0..n_stocks as StockIdx {
            let rx = desk_rxs.remove(0);
            let handle = std::thread::Builder::new()
                .name(format!("rt-desk-{s}"))
                .spawn({
                    let group_txs = group_txs.clone();
                    let up_tx = up_tx.clone();
                    let abort = Arc::clone(&abort);
                    let watchdog = watchdog;
                    move || desk_main(s, n_groups, rx, group_txs, up_tx, abort, watchdog)
                })
                .map_err(|e| RuntimeError::SendFailed {
                    detail: format!("spawn rt-desk-{s} 失败：{e}"),
                })?;
            handles.push((WorkerId::Desk(s), handle));
        }
        // 世界不持上行发送端：全部 worker 退出后 recv 才会断连（显式失败面）。
        drop(up_tx);
        Ok(World {
            global: GlobalState::new(),
            report_log: Vec::new(),
            group_txs,
            desk_txs,
            up_rx,
            handles,
            abort,
            watchdog,
            validator: no_op_validator(),
            rounds_cap,
            n_groups,
            n_stocks,
            dead_finals: None,
            dead_reason: None,
        })
    }

    /// 注入 PreCommit 校验器（测试/批 3+ 接线点）。
    pub fn set_validator(&mut self, v: Validator) {
        self.validator = v;
    }

    /// 运行 n 个 tick（协调者逻辑在本调用线程执行）。
    /// panic 运输：worker panic ⇒ 本函数以原 payload `resume_unwind`（不吞不改写）。
    pub fn run_ticks(&mut self, n: u64) -> Result<u64, RuntimeError> {
        if let Some(reason) = &self.dead_reason {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("world 已死亡，不可继续跑 tick：{reason}"),
            });
        }
        for _ in 0..n {
            let tick = self.global.committed_tick + 1;
            broadcast_start(&self.group_txs, tick)?;
            let mut rounds = 0u32;
            loop {
                let evidence = self.converge_or_die(tick, rounds)?;
                if let Err(detail) = (self.validator)(&evidence) {
                    self.abort_tick(tick);
                    return Err(RuntimeError::TickValidation {
                        tick,
                        round: rounds,
                        detail,
                    });
                }
                if evidence.continuation_any {
                    let next = rounds + 1;
                    if next > self.rounds_cap {
                        self.abort_tick(tick);
                        return Err(RuntimeError::RoundsExceeded {
                            tick,
                            rounds_cap: self.rounds_cap,
                        });
                    }
                    broadcast_continue(&self.group_txs, tick, next)?;
                    rounds = next;
                } else {
                    self.install_tick(tick, rounds, evidence);
                    break;
                }
            }
        }
        Ok(self.global.committed_tick)
    }

    /// 收束一轮；TaskDied ⇒ 撤退 + 收齐 Final + join + `resume_unwind` 原 payload
    /// （不吞、不改写）。收集失败不丢原 payload：stderr 登记后仍 resume。
    fn converge_or_die(&mut self, tick: u64, round: u32) -> Result<TickEvidence, RuntimeError> {
        let outcome = converge_round(
            &self.up_rx,
            self.n_groups,
            self.n_stocks,
            tick,
            round,
            &self.watchdog,
        );
        match outcome {
            RoundOutcome::Converged(ev) => Ok(ev),
            RoundOutcome::TaskDied { who, payload } => {
                // 全体撤退：Abort（幸存者回滚）+ Shutdown（含已死者，发送失败忽略）。
                self.abort.store(true, Ordering::SeqCst);
                for tx in &self.group_txs {
                    let _ = tx.send(GroupIn::Abort { tick });
                    let _ = tx.send(GroupIn::Shutdown);
                }
                for tx in &self.desk_txs {
                    let _ = tx.send(DeskMsg::Abort { tick });
                    let _ = tx.send(DeskMsg::Shutdown);
                }
                match self.collect_finals() {
                    Ok(report) => {
                        let joined = self.join_workers(report).expect("join 不可失败");
                        self.dead_finals = Some(joined);
                        self.dead_reason = Some(format!("worker {who} panic（payload 已 resume）"));
                    }
                    Err(e) => {
                        // 撤退对账失败是第二重失败：显式登记到 stderr，但原 panic
                        // payload 优先（不吞不改写）。
                        eprintln!("panic 撤退路径 Final 收集失败（worker {who}）：{e}");
                    }
                }
                std::panic::resume_unwind(payload);
            }
            RoundOutcome::Failed(RuntimeError::WorkerFailed { who, error }) => {
                // worker 显式失败：撤退（幸存者回滚存活）+ 收齐 Final + 标记死亡。
                self.abort_tick(tick);
                for tx in &self.group_txs {
                    let _ = tx.send(GroupIn::Shutdown);
                }
                for tx in &self.desk_txs {
                    let _ = tx.send(DeskMsg::Shutdown);
                }
                let report =
                    self.collect_finals()
                        .map_err(|e| RuntimeError::ProtocolInvariant {
                            detail: format!("失败撤退路径 Final 收集失败：{e}"),
                        })?;
                self.dead_finals = Some(report);
                self.dead_reason = Some(format!("worker {who} 显式失败"));
                Err(RuntimeError::WorkerFailed { who, error })
            }
            RoundOutcome::Failed(e) => {
                self.abort_tick(tick);
                Err(e)
            }
        }
    }

    /// 广播 Abort（tick 判死；幸存 worker 回滚并存活，可重跑或关停）。
    fn abort_tick(&mut self, tick: u64) {
        self.abort.store(true, Ordering::SeqCst);
        for tx in &self.group_txs {
            let _ = tx.send(GroupIn::Abort { tick });
        }
        for tx in &self.desk_txs {
            let _ = tx.send(DeskMsg::Abort { tick });
        }
    }

    /// 单点安装（唯一安装点语义保持：成功路径才触碰全局域权威）。
    fn install_tick(&mut self, tick: u64, rounds: u32, evidence: TickEvidence) {
        // tick 成功转正：清除可能残留的全局中止标记（上一次 Abort 已回滚收敛）。
        self.abort.store(false, Ordering::SeqCst);
        self.global.committed_tick = tick;
        self.global.day = (tick / 240) as u32;
        self.global.events_installed += evidence.entities_out;
        self.report_log.push(TickRecord {
            tick,
            rounds,
            group_hashes: evidence.group_hashes,
            desk_hashes: evidence.desk_hashes,
        });
    }

    /// 关停序列（对齐原型 orun.rs:373-453）：
    /// Drain 交易部（强撤+实体回流）→ 收 DeskDrained → Shutdown 组（实体批次
    /// 先于 Shutdown 入队——FIFO 保证期末名册含全部回流）→ 收 GroupFinal →
    /// Shutdown 交易部 → 收 DeskFinal → join。
    pub fn shutdown(mut self) -> Result<ShutdownReport, RuntimeError> {
        if let Some(report) = self.dead_finals.take() {
            // 已在失败撤退路径收齐 Final：只补 join。
            self.join_workers(report)
        } else {
            for tx in &self.desk_txs {
                tx.send(DeskMsg::Drain)
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: "Drain 广播失败".to_owned(),
                    })?;
            }
            let mut drained = 0usize;
            while drained < self.n_stocks {
                match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                    UpMsg::DeskDrained { .. } => drained += 1,
                    UpMsg::Failed { who, error } => {
                        return Err(RuntimeError::WorkerFailed {
                            who,
                            error: Box::new(error),
                        });
                    }
                    other => {
                        return Err(RuntimeError::Disconnected {
                            who: None,
                            stage: format!("Drain 期望 DeskDrained，收到 {other:?}（实现 bug）"),
                        });
                    }
                }
            }
            for tx in &self.group_txs {
                tx.send(GroupIn::Shutdown)
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: "组 Shutdown 广播失败".to_owned(),
                    })?;
            }
            // 交易部在组 Final 前发 Shutdown：组的 desk_txs 克隆随组退出释放，
            // 交易部收件循环随后退出并上交 Final。
            for tx in &self.desk_txs {
                tx.send(DeskMsg::Shutdown)
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: "交易部 Shutdown 广播失败".to_owned(),
                    })?;
            }
            let report = self.collect_finals()?;
            self.join_workers(report)
        }
    }

    /// 收齐 G+N Final（worker 退出路径必发）。缺口（worker 未发即消亡）由
    /// 看门狗/断连显式暴露，不静默。
    fn collect_finals(&mut self) -> Result<ShutdownReport, RuntimeError> {
        let mut group_finals: Vec<Option<GroupFinalState>> =
            (0..self.n_groups).map(|_| None).collect();
        let mut desk_finals: Vec<Option<DeskFinalState>> =
            (0..self.n_stocks).map(|_| None).collect();
        let mut left = self.n_groups + self.n_stocks;
        while left > 0 {
            let msg = recv_deadline(&self.up_rx, Phase::FinalCollection, &self.watchdog)?;
            match msg {
                UpMsg::GroupFinal(f) => {
                    let idx = f.group as usize;
                    group_finals[idx] = Some(*f);
                    left -= 1;
                }
                UpMsg::DeskFinal(f) => {
                    let idx = f.stock as usize;
                    desk_finals[idx] = Some(*f);
                    left -= 1;
                }
                UpMsg::Failed { who, error } => {
                    // Failed 与 Final 都会到（worker 先 Failed 后 Final）；记录不中断。
                    let _ = (who, error);
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!(
                            "Final 收集期望 GroupFinal/DeskFinal，收到 {other:?}（实现 bug）"
                        ),
                    });
                }
            }
        }
        let missing_groups: Vec<usize> = group_finals
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_none())
            .map(|(i, _)| i)
            .collect();
        if !missing_groups.is_empty() {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "GroupFinal 缺口：组 {missing_groups:?} 未上交（worker 未发 Final 即消亡）"
                ),
            });
        }
        let missing_desks: Vec<usize> = desk_finals
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_none())
            .map(|(i, _)| i)
            .collect();
        if !missing_desks.is_empty() {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "DeskFinal 缺口：交易部 {missing_desks:?} 未上交（worker 未发 Final 即消亡）"
                ),
            });
        }
        Ok(ShutdownReport {
            group_finals: group_finals
                .into_iter()
                .map(|f| f.expect("已核验非空"))
                .collect(),
            desk_finals: desk_finals
                .into_iter()
                .map(|f| f.expect("已核验非空"))
                .collect(),
            worker_results: Vec::new(),
        })
    }

    /// join 全部 worker（消费句柄）。
    fn join_workers(&mut self, mut report: ShutdownReport) -> Result<ShutdownReport, RuntimeError> {
        for (who, handle) in std::mem::take(&mut self.handles) {
            match handle.join() {
                Ok(r) => report.worker_results.push((who, r)),
                Err(_) => {
                    report.worker_results.push((
                        who,
                        Err(RuntimeError::ProtocolInvariant {
                            detail: "worker 线程 panic 退出（payload 应已由 TaskDied 运输）"
                                .to_owned(),
                        }),
                    ));
                }
            }
        }
        Ok(report)
    }

    /// 切断协调者下行通道（断连显式失败测试面）：worker 见 Disconnected 退出。
    pub fn sever(&mut self) {
        self.group_txs.clear();
        self.desk_txs.clear();
    }
}
