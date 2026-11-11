//! 失败 tick 的遗留下行载荷对账。
//!
//! 这里只在所有 Group/Desk Final 都逐项证明回到同一已提交基线后，才会
//! 一次性消费已完整 drain 的失败 tick 队列尾部。消息本身永不重放；Final
//! 中的 `undelivered_*` 诊断副本也不进入本模块的资产计算。

use super::{
    ExitReconcileContext, ExitReconcilePhase, OwnedWorkerExit, RetainedExitDrainState,
    RetainedExitMessages, RetainedMessageAssessment,
};
use crate::runtime::desk::DeskFinalState;
use crate::runtime::group::GroupFinalState;
use crate::runtime::protocol::{DeskIn, DeskMsg, GroupIn, RetainedWorkerPayload, WorkerId};

/// 只有完整的全体 worker Abort 证明才能构造此值。
#[derive(Clone, Debug)]
pub(super) struct VerifiedFailedTick {
    context: ExitReconcileContext,
    groups: usize,
    desks: usize,
}

/// 对账器拒绝消费载荷的原因。失败时原始 payload 仍留在 `RetainedWorkerPayload`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReconcileFailure {
    detail: String,
}

impl ReconcileFailure {
    fn new(detail: impl Into<String>) -> Self {
        Self { detail: detail.into() }
    }

    pub(super) fn detail(&self) -> &str {
        &self.detail
    }
}

/// 已验证回滚后被一次性废弃的消息摘要；数量仅用于记录处置结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReconciledExitMessages {
    pub(super) worker: WorkerId,
    pub(super) context: ExitReconcileContext,
    pub(super) group_messages_abandoned: usize,
    pub(super) desk_messages_abandoned: usize,
    pub(super) assessment: RetainedMessageAssessment,
    /// 表示全体 actor 已逐项回到基线且该失败事务队列已被消费，不表示消息被重放。
    pub(super) assets_reconciled: bool,
}

/// 用每个真实 Final 携带的字段级 Abort 证明建立失败 tick 的处置凭证。
///
/// `last_committed_tick` 只作为事务匹配字段；是否等于完整业务基线由 Group/Desk
/// 各自的 `verify_rollback` 依据其保存的真实字段级快照核验，本函数不使用 hash。
pub(super) fn verify_failed_tick_rollback(
    context: &ExitReconcileContext,
    expected_groups: usize,
    expected_desks: usize,
    group_finals: &[Option<GroupFinalState>],
    desk_finals: &[Option<DeskFinalState>],
) -> Result<VerifiedFailedTick, ReconcileFailure> {
    if context.phase != ExitReconcilePhase::FatalTick
        || !context.candidate_unpublished
        || context.last_committed_tick.is_none()
        || context.active_tick.is_none()
        || context.round.is_none()
    {
        return Err(ReconcileFailure::new(
            "仅完整 FatalTick 事务可进入资产对账；上下文缺少失败阶段、提交边界或未发布证明",
        ));
    }
    if group_finals.len() != expected_groups || desk_finals.len() != expected_desks {
        return Err(ReconcileFailure::new(format!(
            "Final 名册不完整：Group {}/{expected_groups}，Desk {}/{expected_desks}",
            group_finals.len(),
            desk_finals.len(),
        )));
    }

    let mut groups_seen = vec![false; expected_groups];
    for final_state in group_finals {
        let final_state = final_state.as_ref().ok_or_else(|| {
            ReconcileFailure::new("GroupFinal 名册含缺失项")
        })?;
        let index = usize::from(final_state.group);
        let Some(seen) = groups_seen.get_mut(index) else {
            return Err(ReconcileFailure::new(format!(
                "GroupFinal 身份越界：group={}",
                final_state.group
            )));
        };
        if *seen {
            return Err(ReconcileFailure::new(format!(
                "GroupFinal 身份重复：group={}",
                final_state.group
            )));
        }
        *seen = true;
        final_state
            .verify_rollback(context)
            .map_err(|reason| ReconcileFailure::new(format!("Group {} Abort 证明失败：{reason}", final_state.group)))?;
    }
    if groups_seen.iter().any(|seen| !seen) {
        return Err(ReconcileFailure::new("GroupFinal 缺少预期 worker 身份"));
    }

    let mut desks_seen = vec![false; expected_desks];
    for final_state in desk_finals {
        let final_state = final_state.as_ref().ok_or_else(|| {
            ReconcileFailure::new("DeskFinal 名册含缺失项")
        })?;
        let index = usize::from(final_state.stock);
        let Some(seen) = desks_seen.get_mut(index) else {
            return Err(ReconcileFailure::new(format!(
                "DeskFinal 身份越界：stock={}",
                final_state.stock
            )));
        };
        if *seen {
            return Err(ReconcileFailure::new(format!(
                "DeskFinal 身份重复：stock={}",
                final_state.stock
            )));
        }
        *seen = true;
        final_state
            .verify_rollback(context)
            .map_err(|reason| ReconcileFailure::new(format!("Desk {} Abort 证明失败：{reason}", final_state.stock)))?;
    }
    if desks_seen.iter().any(|seen| !seen) {
        return Err(ReconcileFailure::new("DeskFinal 缺少预期 worker 身份"));
    }

    Ok(VerifiedFailedTick {
        context: context.clone(),
        groups: expected_groups,
        desks: expected_desks,
    })
}

/// 先核验这一批所有 payload，再统一消费；任何预检失败都不会发生部分 take。
///
/// 调用方在 World 内持有唯一未发布的 worker result 集，函数返回前不暴露其
/// `RetainedWorkerPayload` clone；因此预检与 take 之间没有宿主并发消费者。
pub(super) fn reconcile_worker_results(
    worker_results: &mut [(WorkerId, Result<(), super::super::protocol::RuntimeError>)],
    proof: &VerifiedFailedTick,
) -> Result<Vec<ReconciledExitMessages>, ReconcileFailure> {
    let mut pending = Vec::new();
    let mut seen_workers = std::collections::HashSet::new();

    if worker_results.len() != proof.groups + proof.desks {
        return Err(ReconcileFailure::new(format!(
            "worker result 名册不完整：{}/{}, expected Group {} + Desk {}",
            worker_results.len(),
            proof.groups + proof.desks,
            proof.groups,
            proof.desks
        )));
    }

    for (index, (expected_worker, result)) in worker_results.iter().enumerate() {
        let index_in_kind = match expected_worker {
            WorkerId::Group(group) => usize::from(*group),
            WorkerId::Desk(stock) => usize::from(*stock),
        };
        let expected_count = match expected_worker {
            WorkerId::Group(_) => proof.groups,
            WorkerId::Desk(_) => proof.desks,
        };
        if index_in_kind >= expected_count || !seen_workers.insert(*expected_worker) {
            return Err(ReconcileFailure::new(format!(
                "worker result 名册身份越界或重复：{expected_worker}"
            )));
        }
        if matches!(
            result,
            Err(super::super::protocol::RuntimeError::WorkerJoinFailed { .. })
                | Err(super::super::protocol::RuntimeError::WorkerJoinPending { .. })
        ) {
            return Err(ReconcileFailure::new(format!(
                "worker {expected_worker} 未证明 receiver 已收回且下行完整 drain"
            )));
        }
        let Err(super::super::protocol::RuntimeError::DownlinkCleanupFailed {
            payload, ..
        }) = result
        else {
            continue;
        };
        let payload_worker = payload.worker();
        if payload_worker != *expected_worker {
            return Err(ReconcileFailure::new(format!(
                "worker result 与 retained payload 身份不一致：result={expected_worker}，payload={payload_worker}"
            )));
        }
        let checked = payload.inspect::<RetainedExitMessages, _>(|retained| {
            validate_retained(retained, payload_worker, proof)
        });
        let summary = checked
            .ok_or_else(|| ReconcileFailure::new(format!(
                "worker {expected_worker} 的 payload 不含 RetainedExitMessages，原始载荷仍保留"
            )))??;
        pending.push((index, summary));
    }

    // 所有 retained 消息与全体 Final 已通过校验后才取走真实载荷。
    let mut taken = Vec::with_capacity(pending.len());
    for (index, summary) in pending {
        let take_result = match &worker_results[index].1 {
            Err(super::super::protocol::RuntimeError::DownlinkCleanupFailed {
                payload, ..
            }) => Some(payload.take::<RetainedExitMessages>()),
            _ => None,
        };
        let Some(Some(retained)) = take_result else {
            restore_taken(worker_results, taken);
            let detail = format!(
                "worker {} 的 retained payload 在预检后被消费或 worker result 发生变化；未重放任何消息",
                summary.worker
            );
            return Err(ReconcileFailure::new(detail));
        };
        taken.push((index, summary, retained));
    }

    let mut dispositions = Vec::with_capacity(taken.len());
    for (index, summary, retained) in taken {
        match &mut worker_results[index].1 {
            Err(super::super::protocol::RuntimeError::DownlinkCleanupFailed {
                detail,
                assets_reconciled,
                ..
            }) => {
                *assets_reconciled = summary.assets_reconciled;
                *detail = format!(
                    "全体 Abort 已证明；失败事务下行消息已逐项核验并一次性废弃，不回放；处置={summary:?}"
                );
                drop(retained);
            }
            _ => super::retain_unreported_payload(RetainedWorkerPayload::new(summary.worker, retained)),
        }
        dispositions.push(summary);
    }
    Ok(dispositions)
}

fn restore_taken(
    worker_results: &mut [(WorkerId, Result<(), super::super::protocol::RuntimeError>)],
    taken: Vec<(usize, ReconciledExitMessages, RetainedExitMessages)>,
) {
    for (index, summary, retained) in taken {
        let replacement = RetainedWorkerPayload::new(summary.worker, retained);
        match &mut worker_results[index].1 {
            Err(super::super::protocol::RuntimeError::DownlinkCleanupFailed {
                payload,
                detail,
                assets_reconciled,
                ..
            }) => {
                *payload = replacement;
                *assets_reconciled = false;
                *detail = "批量对账未完成；已取出的退出消息已重新保留".to_owned();
            }
            _ => super::retain_unreported_payload(replacement),
        }
    }
}

fn validate_retained(
    retained: &RetainedExitMessages,
    payload_worker: WorkerId,
    proof: &VerifiedFailedTick,
) -> Result<ReconciledExitMessages, ReconcileFailure> {
    if retained.context != proof.context {
        return Err(ReconcileFailure::new(
            "retained exit 与全体 Final Abort 证明不属于同一失败事务",
        ));
    }
    if retained.context.phase != ExitReconcilePhase::FatalTick
        || !retained.context.candidate_unpublished
    {
        return Err(ReconcileFailure::new("retained exit 不属于未发布的 FatalTick"));
    }
    if !matches!(
        &retained.drain_state,
        &RetainedExitDrainState::DrainedWithMessages
    ) {
        return Err(ReconcileFailure::new(format!(
            "下行队列未证明完整 drain：{:?}；消息和 receiver 原样保留",
            retained.drain_state
        )));
    }

    let exit_worker = match &retained.exit {
        OwnedWorkerExit::Group { who, .. } | OwnedWorkerExit::Desk { who, .. } => *who,
    };
    if exit_worker != payload_worker {
        return Err(ReconcileFailure::new(format!(
            "payload worker 与 owned exit 身份不一致：payload={payload_worker}，exit={exit_worker}"
        )));
    }

    let active_tick = retained.context.active_tick.expect("上下文已验证");
    let failed_round = retained.context.round.expect("上下文已验证");
    let (group_messages_abandoned, desk_messages_abandoned) = match exit_worker {
        WorkerId::Group(group) => {
            if usize::from(group) >= proof.groups {
                return Err(ReconcileFailure::new(format!(
                    "Group exit 身份越界：group={group}, groups={}",
                    proof.groups
                )));
            }
            if !retained.desk_messages.is_empty() {
                return Err(ReconcileFailure::new("Group exit 携带 Desk 下行消息"));
            }
            validate_group_messages(
                &retained.group_messages,
                active_tick,
                failed_round,
                proof.desks,
            )?;
            (retained.group_messages.len(), 0)
        }
        WorkerId::Desk(stock) => {
            if !retained.group_messages.is_empty() {
                return Err(ReconcileFailure::new("Desk exit 携带 Group 下行消息"));
            }
            validate_desk_messages(
                &retained.desk_messages,
                active_tick,
                failed_round,
                group,
                stock,
                proof.desks,
            )?;
            (0, retained.desk_messages.len())
        }
    };
    let assessment = retained.assessment();
    Ok(ReconciledExitMessages {
        worker: exit_worker,
        context: retained.context.clone(),
        group_messages_abandoned,
        desk_messages_abandoned,
        assessment,
        assets_reconciled: true,
    })
}

fn validate_group_messages(
    messages: &[GroupIn],
    active_tick: u64,
    failed_round: u32,
    expected_desks: usize,
) -> Result<(), ReconcileFailure> {
    let mut last_round_by_stock = vec![None; expected_desks];
    let mut seen_batch_positions = std::collections::BTreeSet::new();
    for message in messages {
        match message {
            GroupIn::Batch(batch) => {
                if batch.tick != active_tick || batch.round > failed_round {
                    return Err(ReconcileFailure::new(format!(
                        "GroupIn::Batch 事务位置不匹配：tick={}, round={}, expected tick={active_tick}, failed round≤{failed_round}",
                        batch.tick, batch.round
                    )));
                }
                if usize::from(batch.stock) >= expected_desks {
                    return Err(ReconcileFailure::new(format!(
                        "GroupIn::Batch stock 越界：stock={}, desks={expected_desks}",
                        batch.stock
                    )));
                }
                let stock = usize::from(batch.stock);
                if last_round_by_stock[stock].is_some_and(|last| batch.round < last) {
                    return Err(ReconcileFailure::new(format!(
                        "GroupIn::Batch 破坏同一交易部的 round FIFO：stock={}, round={}",
                        batch.stock, batch.round
                    )));
                }
                if !seen_batch_positions.insert((batch.stock, batch.round)) {
                    return Err(ReconcileFailure::new(format!(
                        "同一交易部同一 round 出现重复非空 Batch：stock={}, round={}",
                        batch.stock, batch.round
                    )));
                }
                last_round_by_stock[stock] = Some(batch.round);
            }
            GroupIn::Start { tick, .. }
            | GroupIn::Abort { tick }
            | GroupIn::AbortDayEnd { tick } => {
                if *tick != active_tick {
                    return Err(ReconcileFailure::new(format!(
                        "Group control tick 不匹配：{tick} != {active_tick}"
                    )));
                }
                if matches!(message, GroupIn::AbortDayEnd { .. }) {
                    return Err(ReconcileFailure::new(
                        "FatalTick 队列含 AbortDayEnd，日结边界未证明",
                    ));
                }
            }
            GroupIn::Continue { tick, round } => {
                if *tick != active_tick || *round > failed_round {
                    return Err(ReconcileFailure::new(format!(
                        "GroupIn::Continue 事务位置不匹配：tick={tick}, round={round}"
                    )));
                }
            }
            GroupIn::Shutdown => {}
            _ => {
                return Err(ReconcileFailure::new(
                    "Group 队列含尚未映射到 FatalTick 的控制或业务载荷",
                ));
            }
        }
    }
    Ok(())
}

fn validate_desk_messages(
    messages: &[DeskMsg],
    active_tick: u64,
    failed_round: u32,
    expected_groups: usize,
    stock: u16,
    expected_desks: usize,
) -> Result<(), ReconcileFailure> {
    if usize::from(stock) >= expected_desks {
        return Err(ReconcileFailure::new(format!(
            "Desk exit stock 越界：stock={stock}, desks={expected_desks}"
        )));
    }
    let mut last_round_by_group = std::collections::BTreeMap::<u16, (u32, bool)>::new();
    for message in messages {
        match message {
            DeskMsg::Ops {
                tick,
                round,
                group,
                ops,
                final_flush,
            } => {
                if *tick != active_tick || *round > failed_round {
                    return Err(ReconcileFailure::new(format!(
                        "DeskMsg::Ops 事务位置不匹配：tick={tick}, round={round}"
                    )));
                }
                if usize::from(*group) >= expected_groups {
                    return Err(ReconcileFailure::new(format!(
                        "DeskMsg::Ops group 越界：group={group}, groups={expected_groups}"
                    )));
                }
                if let Some((prior_round, prior_final)) = last_round_by_group.get(group).copied() {
                    if *round < prior_round
                        || (*round == prior_round && prior_final)
                        || (*round > prior_round
                            && (!prior_final || prior_round.checked_add(1) != Some(*round)))
                    {
                        return Err(ReconcileFailure::new(format!(
                            "DeskMsg::Ops 破坏同组 FIFO/final_flush：group={group}, round={round}, prior=({prior_round}, final={prior_final})"
                        )));
                    }
                }
                last_round_by_group.insert(*group, (*round, *final_flush));
                for operation in ops {
                    let (owner, cancel_order_stock) = match operation {
                        DeskIn::Place { owner, .. }
                        | DeskIn::PlaceIssuerRepurchase { owner, .. }
                        | DeskIn::CancelAll { owner } => (*owner, None),
                        DeskIn::Cancel { owner, order, .. } => (*owner, Some(order.0)),
                    };
                    if usize::from(owner.0) != usize::from(*group) {
                        return Err(ReconcileFailure::new(format!(
                            "DeskMsg::Ops owner group 与信封不一致：owner={}, envelope={group}",
                            owner.0
                        )));
                    }
                    if cancel_order_stock.is_some_and(|order_stock| order_stock != stock) {
                        return Err(ReconcileFailure::new(format!(
                            "DeskMsg::Ops Cancel OrderRef 指向其他交易部：stock={stock}"
                        )));
                    }
                }
            }
            DeskMsg::Start { tick, .. } | DeskMsg::Abort { tick } => {
                if *tick != active_tick {
                    return Err(ReconcileFailure::new(format!(
                        "Desk control tick 不匹配：{tick} != {active_tick}"
                    )));
                }
            }
            DeskMsg::Shutdown => {}
            _ => {
                return Err(ReconcileFailure::new(
                    "Desk 队列含尚未映射到 FatalTick 的控制或业务载荷",
                ));
            }
        }
    }
    Ok(())
}
