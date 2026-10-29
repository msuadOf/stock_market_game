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
    /// 仅测试：从本轮 GroupReport 原样搬移的真实动作。
    #[cfg(test)]
    pub lifecycle_actions_for_test: Vec<super::protocol::PlanLifecycleWitnessForTest>,
    #[cfg(feature = "simulation-diagnostics")]
    pub decision_traces: Vec<super::diagnostics::GroupDecisionTrace>,
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
    pub desk_views: Vec<(StockIdx, std::sync::Arc<super::protocol::StockPublic>)>,
    /// 各交易部本收束点挂单数（观察面）。
    pub desk_resting: Vec<(StockIdx, u64)>,
    /// 本 tick 双腿成交事实（事件映射输入）。
    pub desk_trades: Vec<super::protocol::DeskTradeFact>,
    /// 按 stock 排的非空原报告片段；片段内保持原操作序。
    pub retail: Vec<super::retail_lifecycle::Fact>,
    /// 单股原顺序的本轮诊断 sideband。
    #[cfg(feature = "simulation-diagnostics")]
    pub desk_causal: Vec<super::diagnostics::DeskCausalFact>,
    /// 本 tick 生命周期事实（受理/撤单/到期）。
    pub desk_lifecycle: Vec<super::protocol::DeskLifecycleFact>,
    /// 本轮回执（拒收/不可撤）。
    pub desk_receipts: Vec<super::DeskReceipt>,
    /// 组端业务拒绝回执（资金/可卖/零股/竞价×市价——与 desk 回执
    /// 同入事件流，Sol 复审修复 2）。
    pub group_rejections: Vec<super::DeskReceipt>,
    /// 本轮竞价事实。
    pub desk_auction: Vec<super::protocol::DeskAuctionFact>,
    /// P2-3：各交易部在途单观察面（随 DeskReport 上行——下标=股）。
    pub desk_resting_facts: Vec<std::sync::Arc<Vec<super::protocol::RestingFact>>>,
    /// 各交易部序号游标（随 DeskReport 上行；save 投影权威源）。
    pub desk_serial_cursors: Vec<u64>,
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
    Converged(Box<TickEvidence>),
    /// 显式失败（看门狗/断连/worker Failed/协议不变量）。
    Failed(RuntimeError),
    /// 来源门已拒绝分派本轮 Start；协调者核对后可先等待 producer 停止。
    PendingStartRejected(super::protocol::PendingStartRejection),
    /// worker panic：原 payload 待运输。
    TaskDied {
        /// panic 的 worker。
        who: WorkerId,
        /// 原 panic payload。
        payload: Box<dyn std::any::Any + Send>,
    },
}

/// 收束一轮：收齐 G 个 `GroupReport` + N 个 `DeskReport`。
pub(crate) fn converge_round(
    up_rx: &Receiver<UpMsg>,
    n_groups: usize,
    n_stocks: usize,
    tick: u64,
    round: u32,
    watchdog: &WatchdogConfig,
) -> RoundOutcome {
    let wrap = |r: Result<TickEvidence, RuntimeError>| match r {
        Ok(ev) => RoundOutcome::Converged(Box::new(ev)),
        Err(e) => RoundOutcome::Failed(e),
    };
    #[cfg(test)]
    let mut lifecycle_actions_for_test = Vec::new();
    let mut groups_left = n_groups;
    let mut desks_left = n_stocks;
    let mut group_hashes = Vec::with_capacity(n_groups);
    let mut desk_hashes = Vec::with_capacity(n_stocks);
    let mut continuation_any = false;
    let mut entities_out = 0u64;
    let mut desk_views = Vec::with_capacity(n_stocks);
    let mut desk_resting = Vec::with_capacity(n_stocks);
    let mut desk_trades = Vec::new();
    let mut retail_fragments = Vec::new();
    #[cfg(feature = "simulation-diagnostics")]
    let mut desk_causal = Vec::new();
    #[cfg(feature = "simulation-diagnostics")]
    let mut decision_traces = Vec::new();
    #[cfg(feature = "simulation-diagnostics")]
    let mut decision_trace_groups = std::collections::BTreeSet::new();
    let mut desk_lifecycle = Vec::new();
    let mut desk_receipts = Vec::new();
    let mut group_rejections = Vec::new();
    let mut desk_auction = Vec::new();
    // P2-3：在途单观察面+游标收集（随 DeskReport 上行——发布源）。
    let mut desk_resting_facts: Vec<Option<std::sync::Arc<Vec<super::protocol::RestingFact>>>> =
        vec![None; n_stocks];
    let mut desk_serial_cursors = vec![0u64; n_stocks];
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
                #[cfg(feature = "simulation-diagnostics")]
                {
                    if usize::from(r.group) >= n_groups || !decision_trace_groups.insert(r.group) {
                        return RoundOutcome::Failed(RuntimeError::ProtocolInvariant {
                            detail: format!("机构 trace GroupReport 来源组越界或重复：{}", r.group),
                        });
                    }
                    let mut locals = std::collections::BTreeSet::new();
                    for trace in &r.decision_traces {
                        if round != 0
                            || trace.owner.0 != r.group
                            || trace.record.tick != tick
                            || !locals.insert(trace.owner.1)
                        {
                            return RoundOutcome::Failed(RuntimeError::ProtocolInvariant {
                                detail: format!("机构 trace 本轮来源错位或重复：group={} tick={tick} round={round} owner={:?}", r.group, trace.owner),
                            });
                        }
                    }
                    decision_traces.extend(r.decision_traces);
                }
                #[cfg(test)]
                lifecycle_actions_for_test.extend(r.lifecycle_actions_for_test);
                groups_left -= 1;
                continuation_any |= r.continuation;
                group_hashes.push((r.group, r.committed_hash));
                group_rejections.extend(r.rejections.iter().copied());
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
                #[cfg(feature = "simulation-diagnostics")]
                if r.causal.iter().any(|fact| {
                    fact.context
                        != (super::retail_lifecycle::BatchContext::Tick {
                            tick,
                            round,
                            stock: r.stock,
                        })
                }) {
                    return RoundOutcome::Failed(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "causal sideband 上下文错配：tick={tick} round={round} stock={}",
                            r.stock
                        ),
                    });
                }
                #[cfg(all(test, feature = "simulation-diagnostics", not(target_arch = "wasm32")))]
                let r = super::diagnostics::report_fault_tests::remove_execution_once(r);
                #[cfg(feature = "simulation-diagnostics")]
                if let Err(error) = super::diagnostics::validate_executions(
                    r.stock,
                    &r.causal,
                    &r.trades,
                    r.auction
                        .iter()
                        .any(|fact| fact.completed && fact.matched_volume > 0),
                ) {
                    return RoundOutcome::Failed(error);
                }
                #[cfg(feature = "simulation-diagnostics")]
                if let Err(error) = super::diagnostics::validate_rejections(
                    r.stock,
                    &r.causal,
                    &r.receipts_out_list,
                ) {
                    return RoundOutcome::Failed(error);
                }
                if !r.retail.is_empty() || !r.trades.is_empty() {
                    let context = super::retail_lifecycle::BatchContext::Tick {
                        tick,
                        round,
                        stock: r.stock,
                    };
                    match super::retail_lifecycle::resolve_report(context, r.retail, &r.trades) {
                        Ok(facts) => retail_fragments.push((r.stock, facts)),
                        Err(error) => return RoundOutcome::Failed(error),
                    }
                }
                desks_left -= 1;
                entities_out += r.entities_out;
                desk_hashes.push((r.stock, r.committed_hash));
                desk_views.push((r.stock, std::sync::Arc::clone(&r.view)));
                desk_resting.push((r.stock, r.resting_count));
                desk_resting_facts[r.stock as usize] = Some(r.resting_orders);
                desk_serial_cursors[r.stock as usize] = r.next_serial;
                desk_trades.extend(r.trades.iter().copied());
                #[cfg(feature = "simulation-diagnostics")]
                desk_causal.extend(r.causal);
                desk_lifecycle.extend(r.lifecycle.iter().copied());
                desk_receipts.extend(r.receipts_out_list.iter().copied());
                desk_auction.extend(r.auction.iter().copied());
            }
            UpMsg::TaskDied { who, payload } => {
                return RoundOutcome::TaskDied { who, payload };
            }
            UpMsg::SealAck { .. } => {
                // Sol 复核阻断 P1-3：迟到 SealAck=已判死尝试（SealAck
                // 收集超时已 Abort）的 Ack 竞态落入重试收束窗——协议
                // 合法产物（组在 Abort 后已回滚重跑，本轮回执以本轮
                // GroupReport/DeskReport 为准），跳过不报错。单次尝试
                // 内 Seal 只在收束后广播，收束期不可能有本拍真 Ack。
                continue;
            }
            UpMsg::Failed { who, error } => {
                return RoundOutcome::Failed(RuntimeError::WorkerFailed {
                    who,
                    error: Box::new(error),
                });
            }
            UpMsg::PendingStartRejected(rejection) => {
                if usize::from(rejection.group) >= n_groups
                    || round != 0
                    || rejection.tick != tick
                    || rejection.prior_tick.checked_add(1) != Some(tick)
                    || rejection.calendar.before_input().tick != rejection.prior_tick
                    || rejection.calendar.after_input().tick != tick
                    || group_hashes
                        .iter()
                        .any(|(group, _)| *group == rejection.group)
                {
                    return RoundOutcome::Failed(RuntimeError::WorkerFailed {
                        who: WorkerId::Group(rejection.group),
                        error: Box::new(RuntimeError::ProtocolInvariant {
                            detail: format!("pending Start 拒绝阶段证据失配：expected=({tick},{round}), actual={rejection:?}；原错误={}", rejection.error),
                        }),
                    });
                }
                return RoundOutcome::PendingStartRejected(rejection);
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
    retail_fragments.sort_unstable_by_key(|(stock, _)| *stock);
    let retail = retail_fragments
        .into_iter()
        .flat_map(|(_, facts)| facts)
        .collect();
    #[cfg(feature = "simulation-diagnostics")]
    decision_traces.sort_unstable_by_key(|trace| trace.owner);
    wrap(Ok(TickEvidence {
        #[cfg(test)]
        lifecycle_actions_for_test,
        #[cfg(feature = "simulation-diagnostics")]
        decision_traces,
        tick,
        round,
        group_hashes,
        desk_hashes,
        continuation_any,
        entities_out,
        desk_views,
        desk_resting,
        desk_trades,
        retail,
        #[cfg(feature = "simulation-diagnostics")]
        desk_causal,
        desk_lifecycle,
        desk_receipts,
        group_rejections,
        desk_auction,
        desk_resting_facts: desk_resting_facts
            .into_iter()
            .map(|slot| slot.unwrap_or_default())
            .collect(),
        desk_serial_cursors,
    }))
}

/// 广播 Start（每股/每组的下行 channel 逐个 send，无锁无惊群）。
pub(crate) fn broadcast_start(
    group_txs: &[Sender<super::protocol::GroupIn>],
    tick: u64,
    calendar: Arc<super::MarketTickContext>,
    stocks: &[super::protocol::StockPublic],
) -> Result<Arc<TickSnapshot>, RuntimeError> {
    let snapshot = Arc::new(TickSnapshot {
        tick,
        market_minute: u32::try_from(calendar.before_projection().completed_shared_minute_count())
            .map_err(|_| RuntimeError::ProtocolInvariant {
                detail: "共享 before minute 超过 u32".to_owned(),
            })?,
        civil_date: calendar.day().day_model().current_date(),
        calendar,
        stocks: std::sync::Arc::new(stocks.to_vec()),
    });
    for tx in group_txs {
        #[cfg(all(test, not(target_family = "wasm")))]
        crate::session::calendar_entry_cache_tests::record_entry_call(
            crate::session::calendar_entry_cache_tests::EntryCall::Start,
        );
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
pub(crate) fn broadcast_continue(
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

#[cfg(test)]
mod transport_layout_tests {
    #[test]
    fn round_outcome_envelope_stays_within_transport_budget() {
        let size = std::mem::size_of::<super::RoundOutcome>();
        assert!(
            size <= 128,
            "RoundOutcome 占 {size} 字节，超过 128 字节运输预算"
        );
    }
}
