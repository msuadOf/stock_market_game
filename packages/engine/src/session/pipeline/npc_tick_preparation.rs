//! 从同一密封决策快照与资源快照准备 NPC intent candidates。
//!
//! 后续账户校验、股票处理、结算、最终事件与权威提交由完整 tick coordinator 负责。

#[cfg(test)]
use super::decision_snapshot_capture::CapturedDecisionSnapshot;
#[cfg(test)]
use super::npc_decisions::run_npc_decisions;
#[cfg(test)]
use super::npc_state_projection::project_npc_state;
#[cfg(test)]
use super::DecisionSnapshot;
use super::{
    candidate_composition::CandidateCompositionError,
    decision_snapshot_capture::{capture_decision_snapshot, DecisionSnapshotCaptureError},
    npc_decisions::{
        stream_npc_decisions, NpcDecisionSourceError, NpcDecisionSourceOutput,
        NpcDecisionStreamError,
    },
    npc_state_projection::{
        project_npc_account, NpcDecisionProjectionError, NpcDecisionProjectionOutput,
        NpcReconciliationDecision,
    },
    IntentCandidate, IntentCandidateBatch, IntentCandidateError, IntentCandidateKey, StepFatal,
};
#[cfg(test)]
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::PendingNpcBatch;
use crate::{AccountId, GameSession, Intent, StockCode};
#[cfg(test)]
use rayon::join;
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(super) enum NpcDecisionPreparationError {
    #[error("NPC decision snapshot capture failed: {0}")]
    Snapshot(#[from] DecisionSnapshotCaptureError),
    #[error("NPC DecisionShadow 来源失败：{0}")]
    Source(#[from] NpcDecisionSourceError),
    #[error("NPC DecisionShadow 投影失败：{0}")]
    Projection(#[from] NpcDecisionProjectionError),
    #[error("NPC DecisionShadow 候选 composition 失败：{0}")]
    Composition(#[from] CandidateCompositionError),
    #[error("plan root capture failed: {0}")]
    Roots(#[from] StepFatal),
    #[error("NPC projected intent for account {account:?} has no sealed source identity")]
    UnkeyedProjectedIntent { account: AccountId },
    #[error("NPC projected source key {key:?} does not exist in the sealed raw source")]
    UnknownProjectedSource { key: IntentCandidateKey },
}

/// 为三来源 orchestrator 提供仅含 NPC 的 DecisionShadow 输出。
///
/// candidate 身份按账户先覆盖 reconciliation commands，再覆盖 residual intents。
/// raw strategy 身份保留在投影中用于来源追踪；此边界尚未读取或追加玩家与计划链候选。
#[cfg(test)]
pub(super) struct PreparedNpcDecisionSource {
    pub(super) snapshot: Arc<DecisionSnapshot>,
    pub(super) projection: NpcDecisionProjectionOutput,
    pub(super) candidates: IntentCandidateBatch,
    pub(super) roots: PlanChainOperationBatch,
}

/// Test adapter for inspecting NPC projection and plan roots before next-tick queueing.
#[cfg(test)]
pub(super) fn prepare_npc_decisions(
    prospective: &mut GameSession,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreparedNpcDecisionSource, NpcDecisionPreparationError> {
    let (captured, source, projection, roots) =
        prepare_npc_projection(prospective, roots_override)?;
    let candidates = projected_candidates(&source, &projection)?;
    Ok(PreparedNpcDecisionSource {
        snapshot: captured.snapshot,
        projection,
        candidates,
        roots,
    })
}

#[cfg(test)]
fn prepare_npc_projection(
    prospective: &mut GameSession,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<
    (
        CapturedDecisionSnapshot,
        NpcDecisionSourceOutput,
        NpcDecisionProjectionOutput,
        PlanChainOperationBatch,
    ),
    NpcDecisionPreparationError,
> {
    let captured = capture_decision_snapshot(prospective)?;
    let config = prospective.state.setup.config.clone();
    let (source, roots) = match roots_override {
        Some(roots) => (
            run_npc_decisions(captured.snapshot.clone(), &config),
            Ok(roots),
        ),
        None => join(
            || run_npc_decisions(captured.snapshot.clone(), &config),
            || {
                prospective.capture_decision_chain_roots(
                    captured.snapshot.due_npc_ids(),
                    &captured.snapshot,
                )
            },
        ),
    };
    let mut source = source?;
    let projection = project_npc_state(prospective, &captured, &mut source)?;
    let roots = roots?;
    Ok((captured, source, projection, roots))
}

/// 在完整 tick candidate 的 CommitTick 之前调用。
/// 决策基于即将提交的状态，其订单进入下一市场 tick。
pub(in crate::session) fn queue_npc_for_next_tick(
    session: &mut GameSession,
) -> Result<(), StepFatal> {
    if session.state.pending_npc.is_some() {
        return Err(invariant(
            "next-tick NPC queue still contains an unconsumed batch",
        ));
    }
    if session
        .state
        .attention_scheduler
        .next_scheduled_tick()
        .is_none_or(|scheduled_tick| scheduled_tick > session.state.tick)
    {
        session.state.pending_npc = Some(PendingNpcBatch {
            observed_tick: session.state.tick,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        return Ok(());
    }
    let captured =
        capture_decision_snapshot(session).map_err(|error| invariant(&error.to_string()))?;
    let config = session.state.setup.config.clone();
    let mut intents = Vec::new();
    let mut dependencies = Vec::new();
    let stream_error = stream_npc_decisions(captured.snapshot.clone(), &config, |mut result| {
        let projection = project_npc_account(session, &captured, &mut result.output)
            .map_err(|error| error.to_string())?;
        let projected = projected_ordered_intents(&result.output, &projection)
            .map_err(|error| error.to_string())?;
        let offset = intents.len();
        for (owner, intent) in projected.intents {
            intents.push(
                session
                    .receive_private_intent(owner, intent)
                    .map_err(|error| error.to_string())?,
            );
        }
        dependencies.extend(
            projected
                .dependencies
                .into_iter()
                .map(|(before, after)| (offset + before, offset + after)),
        );
        Ok::<_, String>(())
    });
    if let Err(error) = stream_error {
        let description = match error {
            NpcDecisionStreamError::Decision(error) => error.to_string(),
            NpcDecisionStreamError::Consumer(error) => error,
        };
        return Err(invariant(&description));
    }
    session.state.pending_npc = Some(PendingNpcBatch {
        observed_tick: session.state.tick,
        observed_accounts: captured.snapshot.due_npc_ids().to_vec(),
        intents,
        dependencies,
    });
    Ok(())
}

/// ExpiryShadow 完成后，在可丢弃 tick shadow 上消费队列。
/// 来源 key 仅标识事实，账户 ID 或 key 顺序不授予交易优先级。
pub(super) fn take_ready_npc_batch(
    session: &mut GameSession,
) -> Result<(IntentCandidateBatch, Vec<AccountId>), StepFatal> {
    let ready = session.state.pending_npc.take().ok_or_else(|| {
        invariant("next-tick NPC queue is missing from a committed market version")
    })?;
    if ready.observed_tick != session.state.tick {
        return Err(invariant(
            "queued NPC observation tick does not match the next market tick",
        ));
    }
    ready
        .validate_dependencies()
        .map_err(|error| invariant(&error))?;
    let mut local = BTreeMap::<AccountId, u64>::new();
    let mut candidates = Vec::with_capacity(ready.intents.len());
    for received in ready.intents {
        let account = received.owner;
        let index = local.entry(account).or_default();
        candidates.push(
            IntentCandidate::new(
                IntentCandidateKey::npc(account, *index),
                account,
                received.intent,
            )
            .with_ingress_order(received.account_ordinal, received.stock_ordinal),
        );
        *index = index
            .checked_add(1)
            .ok_or_else(|| invariant("queued NPC local sequence overflow"))?;
    }
    Ok((
        IntentCandidateBatch::new(with_dependencies(candidates, ready.dependencies))
            .map_err(|error| invariant(&error.to_string()))?,
        ready.observed_accounts,
    ))
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::npc_tick_preparation".to_owned(),
    }
}
#[cfg(test)]
fn projected_candidates(
    source: &NpcDecisionSourceOutput,
    projection: &NpcDecisionProjectionOutput,
) -> Result<IntentCandidateBatch, NpcDecisionPreparationError> {
    let ordered = projected_ordered_intents(source, projection)?;
    let mut local = BTreeMap::<AccountId, u64>::new();
    let mut projected = Vec::with_capacity(ordered.intents.len());
    for (account, intent) in ordered.intents {
        let index = local.entry(account).or_default();
        projected.push(IntentCandidate::new(
            IntentCandidateKey::npc(account, *index),
            account,
            intent,
        ));
        *index = index
            .checked_add(1)
            .ok_or(NpcDecisionSourceError::IntentOrdinalOverflow { account })?;
    }
    IntentCandidateBatch::new(with_dependencies(projected, ordered.dependencies))
        .map_err(CandidateCompositionError::Candidate)
        .map_err(Into::into)
}

struct ProjectedNpcIntents {
    intents: Vec<(AccountId, Intent)>,
    dependencies: Vec<(usize, usize)>,
}

fn with_dependencies(
    candidates: Vec<IntentCandidate>,
    dependencies: Vec<(usize, usize)>,
) -> Vec<IntentCandidate> {
    let mut predecessors = BTreeMap::<usize, Vec<IntentCandidateKey>>::new();
    for (before, after) in dependencies {
        predecessors
            .entry(after)
            .or_default()
            .push(candidates[before].key().clone());
    }
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| match predecessors.remove(&index) {
            Some(keys) => candidate.with_predecessors(keys),
            None => candidate,
        })
        .collect()
}

fn projected_ordered_intents(
    source: &NpcDecisionSourceOutput,
    projection: &NpcDecisionProjectionOutput,
) -> Result<ProjectedNpcIntents, NpcDecisionPreparationError> {
    let mut raw_by_key = BTreeMap::new();
    for raw in source.intents() {
        let key = raw.key();
        let account = match key {
            IntentCandidateKey::Npc { account, .. } => *account,
            _ => return Err(CandidateCompositionError::NonNpcKey(key.clone()).into()),
        };
        if raw_by_key.insert(key, account).is_some() {
            return Err(
                CandidateCompositionError::Candidate(IntentCandidateError::DuplicateKey(
                    key.clone(),
                ))
                .into(),
            );
        }
    }
    let mut by_account = BTreeMap::<AccountId, Vec<Intent>>::new();
    let mut reconciliation_cancels = BTreeMap::<(AccountId, StockCode), Vec<usize>>::new();
    let mut local_dependencies = BTreeMap::<AccountId, Vec<(usize, usize)>>::new();
    for decision in projection.reconciliation_decisions() {
        let (account, code, order_id) = match decision {
            NpcReconciliationDecision::Keep { .. } => continue,
            NpcReconciliationDecision::Cancel {
                account,
                code,
                order_id,
            } => (*account, code, *order_id),
            NpcReconciliationDecision::Replace {
                account,
                old_order_id,
                new_intent,
            } => {
                let code = match new_intent {
                    Intent::PlaceLimit { code, .. }
                    | Intent::PlaceMarket { code, .. }
                    | Intent::Cancel { code, .. } => code,
                };
                (*account, code, *old_order_id)
            }
        };
        // The reconciliation planner keeps a replacement's place in residual_intents.
        // Several old quotes can refer to that same desired place: cancel all of them,
        // then route the cash-capped residual once, as in the existing NPC executor.
        let ordered = by_account.entry(account).or_default();
        reconciliation_cancels
            .entry((account, code.clone()))
            .or_default()
            .push(ordered.len());
        ordered.push(Intent::Cancel {
            code: code.clone(),
            id: order_id,
        });
    }
    for intent in projection.residual_intents() {
        let key = intent.source_key().cloned().ok_or(
            NpcDecisionPreparationError::UnkeyedProjectedIntent {
                account: intent.account(),
            },
        )?;
        let raw_owner = raw_by_key.get(&key).ok_or_else(|| {
            NpcDecisionPreparationError::UnknownProjectedSource { key: key.clone() }
        })?;
        if *raw_owner != intent.account() {
            return Err(CandidateCompositionError::NpcOwnerMismatch {
                key,
                owner: intent.account(),
            }
            .into());
        }
        let ordered = by_account.entry(intent.account()).or_default();
        // Only cancellations produced by reconciliation are predecessors. A raw
        // cancellation remains an independent request even on the same stock.
        if let Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } =
            intent.projected_intent()
        {
            if let Some(cancellations) =
                reconciliation_cancels.get(&(intent.account(), code.clone()))
            {
                local_dependencies
                    .entry(intent.account())
                    .or_default()
                    .extend(cancellations.iter().map(|before| (*before, ordered.len())));
            }
        }
        ordered.push(intent.projected_intent().clone());
    }
    let mut projected = Vec::new();
    let mut dependencies = Vec::new();
    for (account, intents) in by_account {
        let offset = projected.len();
        if let Some(local) = local_dependencies.remove(&account) {
            dependencies.extend(
                local
                    .into_iter()
                    .map(|(before, after)| (offset + before, offset + after)),
            );
        }
        for intent in intents {
            projected.push((account, intent));
        }
    }
    Ok(ProjectedNpcIntents {
        intents: projected,
        dependencies,
    })
}
