//! 协调者（tick 脊 + 全局域权威，骨架档）。
//!
//! 职责（F §4）：串行准备 tick → 广播 `Start` → 收束 G+N 报告（数据面组↔交易部
//! 直发，不经协调者）→ 同 tick 多轮（`Continue`，任一组上报续行即续）→
//! PreCommit 校验（注入点）→ **单点安装**或广播 `Abort`。
//!
//! 失败原子性（F §5.2 协调者行）：全局域工作在私有 candidate 上推进（本函数局部
//! 变量），成功才安装进 `GlobalState`；失败=消费式丢弃，权威从未被失败路径触碰。
//!
//! 全局域骨架档：时钟（tick）、日历（day）、事件计数。公司域候选（权益/披露）随
//! 批 5 日结接线接入。

use super::protocol::{RuntimeError, TickSnapshot, UpMsg, WorkerId};
use super::watchdog::{recv_deadline, Phase, WatchdogConfig};
use super::{GroupIdx, StockIdx};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// 协调者全局域（已收敛态；失败路径只触碰 candidate，不触碰本结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalState {
    /// 已转正 tick。
    pub committed_tick: u64,
    /// 交易日（骨架：每 240 tick 一日）。
    pub day: u32,
    /// 已安装事件数（骨架档计数；批 5 换 EventFact 批）。
    pub events_installed: u64,
}

impl GlobalState {
    /// 初始全局域。
    pub fn new() -> Self {
        GlobalState {
            committed_tick: 0,
            day: 0,
            events_installed: 0,
        }
    }

    /// 全局域哈希（FNV-1a）。
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for v in [self.committed_tick, self.day as u64, self.events_installed] {
            h = fnv(h, v);
        }
        h
    }
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

fn fnv(state: u64, value: u64) -> u64 {
    let mut h = state;
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 一轮收束证据（PreCommit 校验注入点的输入）。
#[derive(Clone, Debug)]
pub struct TickEvidence {
    /// tick 编号。
    pub tick: u64,
    /// 轮次。
    pub round: u32,
    /// 各组提交态哈希。
    pub group_hashes: Vec<(GroupIdx, u64)>,
    /// 各交易部提交态哈希。
    pub desk_hashes: Vec<(StockIdx, u64)>,
    /// 任一组是否还有续行需求。
    pub continuation_any: bool,
    /// 本轮实体流出总数（守恒审计输入，批 3+ 扩展）。
    pub entities_out: u64,
    /// 各交易部公共态（安装进下一 tick 广播快照；决策观察基）。
    pub desk_views: Vec<(StockIdx, super::protocol::StockPublic)>,
    /// 各交易部本收束点挂单数（观察面）。
    pub desk_resting: Vec<(StockIdx, u64)>,
    /// 本 tick 双腿成交事实（事件映射输入）。
    pub desk_trades: Vec<super::protocol::DeskTradeFact>,
    /// 本 tick 生命周期事实（受理/撤单/到期）。
    pub desk_lifecycle: Vec<super::protocol::DeskLifecycleFact>,
    /// 本轮回执（拒收/不可撤）。
    pub desk_receipts: Vec<super::DeskReceipt>,
    /// 组端业务拒绝回执（资金/可卖/零股/竞价×市价——与 desk 回执
    /// 同入事件流，Sol 复审修复 2）。
    pub group_rejections: Vec<super::DeskReceipt>,
    /// 本轮竞价事实。
    pub desk_auction: Vec<super::protocol::DeskAuctionFact>,
    /// P2-2：各组 belief 稀疏分片句柄（随 GroupReport 上行——读面
    /// 发布源；Abort 时随 evidence 丢弃，candidate 不外泄）。
    pub group_belief: Vec<(
        super::GroupIdx,
        std::sync::Arc<Vec<(super::LocalIdx, super::world::BeliefParticipantSnapshot)>>,
    )>,
}

/// PreCommit 校验器（骨架注入点；批 3+ 换 PreCommitValidation）。
pub type Validator = Box<dyn FnMut(&TickEvidence) -> Result<(), String> + Send>;

/// 恒通过的缺省校验器。
pub fn no_op_validator() -> Validator {
    Box::new(|_| Ok(()))
}

/// 一轮收束结果：TaskDied 单列——调用方（world）负责撤退 + 收齐 Final + join +
/// `resume_unwind` 原 payload（不吞、不改写）。
pub enum RoundOutcome {
    /// 本轮收束完成。
    Converged(TickEvidence),
    /// 显式失败（看门狗/断连/worker Failed/协议不变量）。
    Failed(RuntimeError),
    /// worker panic：原 payload 待运输。
    TaskDied {
        /// panic 的 worker。
        who: WorkerId,
        /// 原 panic payload。
        payload: Box<dyn std::any::Any + Send>,
    },
}

/// 收束一轮：收齐 G 个 `GroupReport` + N 个 `DeskReport`。
pub fn converge_round(
    up_rx: &Receiver<UpMsg>,
    n_groups: usize,
    n_stocks: usize,
    tick: u64,
    round: u32,
    watchdog: &WatchdogConfig,
) -> RoundOutcome {
    let wrap = |r: Result<TickEvidence, RuntimeError>| match r {
        Ok(ev) => RoundOutcome::Converged(ev),
        Err(e) => RoundOutcome::Failed(e),
    };
    let mut groups_left = n_groups;
    let mut desks_left = n_stocks;
    let mut group_hashes = Vec::with_capacity(n_groups);
    let mut desk_hashes = Vec::with_capacity(n_stocks);
    let mut continuation_any = false;
    let mut entities_out = 0u64;
    let mut desk_views = Vec::with_capacity(n_stocks);
    let mut desk_resting = Vec::with_capacity(n_stocks);
    let mut desk_trades = Vec::new();
    let mut desk_lifecycle = Vec::new();
    let mut desk_receipts = Vec::new();
    let mut group_rejections = Vec::new();
    let mut desk_auction = Vec::new();
    // P2-2：belief 稀疏分片句柄收集（随报告上行——Arc 零拷贝）。
    let mut group_belief = Vec::with_capacity(n_groups);
    while groups_left > 0 || desks_left > 0 {
        let msg = match recv_deadline(up_rx, Phase::TickConverge, watchdog) {
            Ok(m) => m,
            Err(e) => return RoundOutcome::Failed(e),
        };
        match msg {
            UpMsg::GroupReport(r) => {
                if r.tick != tick || r.round != round {
                    return RoundOutcome::Failed(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "收束期 GroupReport 错位：期望 ({tick},{round})，收到 ({},{})",
                            r.tick, r.round
                        ),
                    });
                }
                groups_left -= 1;
                continuation_any |= r.continuation;
                group_hashes.push((r.group, r.committed_hash));
                group_rejections.extend(r.rejections.iter().copied());
                group_belief.push((r.group, std::sync::Arc::clone(&r.belief_sparse)));
            }
            UpMsg::DeskReport(r) => {
                if r.tick != tick || r.round != round {
                    return RoundOutcome::Failed(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "收束期 DeskReport 错位：期望 ({tick},{round})，收到 ({},{})",
                            r.tick, r.round
                        ),
                    });
                }
                desks_left -= 1;
                entities_out += r.entities_out;
                desk_hashes.push((r.stock, r.committed_hash));
                desk_views.push((r.stock, r.view.clone()));
                desk_resting.push((r.stock, r.resting_count));
                desk_trades.extend(r.trades.iter().copied());
                desk_lifecycle.extend(r.lifecycle.iter().copied());
                desk_receipts.extend(r.receipts_out_list.iter().copied());
                desk_auction.extend(r.auction.iter().copied());
            }
            UpMsg::TaskDied { who, payload } => {
                return RoundOutcome::TaskDied { who, payload };
            }
            UpMsg::Failed { who, error } => {
                return RoundOutcome::Failed(RuntimeError::WorkerFailed {
                    who,
                    error: Box::new(error),
                });
            }
            other => {
                return RoundOutcome::Failed(RuntimeError::Disconnected {
                    who: None,
                    stage: format!(
                        "tick ({tick},{round}) 收束期望 GroupReport/DeskReport，收到 {other:?}（worker 已提前退出？）"
                    ),
                });
            }
        }
    }
    wrap(Ok(TickEvidence {
        tick,
        round,
        group_hashes,
        desk_hashes,
        continuation_any,
        entities_out,
        desk_views,
        desk_resting,
        desk_trades,
        desk_lifecycle,
        desk_receipts,
        group_rejections,
        desk_auction,
        group_belief,
    }))
}

/// 广播 Start（每股/每组的下行 channel 逐个 send，无锁无惊群）。
pub fn broadcast_start(
    group_txs: &[Sender<super::protocol::GroupIn>],
    tick: u64,
    civil_date: crate::calendar::CivilDate,
    stocks: &[super::protocol::StockPublic],
) -> Result<Arc<TickSnapshot>, RuntimeError> {
    let snapshot = Arc::new(TickSnapshot {
        tick,
        market_minute: (tick / 60) as u32,
        civil_date,
        stocks: std::sync::Arc::new(stocks.to_vec()),
    });
    for tx in group_txs {
        tx.send(super::protocol::GroupIn::Start {
            tick,
            snapshot: Arc::clone(&snapshot),
        })
        .map_err(|_| RuntimeError::SendFailed {
            detail: format!("Start(tick={tick}) 广播失败（组已退出）"),
        })?;
    }
    Ok(snapshot)
}

/// 广播 Continue（仅组；交易部经组的新一轮 Ops 感知轮次）。
pub fn broadcast_continue(
    group_txs: &[Sender<super::protocol::GroupIn>],
    tick: u64,
    round: u32,
) -> Result<(), RuntimeError> {
    for tx in group_txs {
        tx.send(super::protocol::GroupIn::Continue { tick, round })
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("Continue(tick={tick}, round={round}) 广播失败（组已退出）"),
            })?;
    }
    Ok(())
}
