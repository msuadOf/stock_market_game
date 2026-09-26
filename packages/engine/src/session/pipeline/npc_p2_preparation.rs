//! Prepare NPC P2 candidates from one sealed decision snapshot and resource snapshot.
//!
//! The combined tick coordinator owns later account validation, stock processing, settlement,
//! final events, and the authority commit.

#[cfg(test)]
use super::DecisionSnapshot;
use super::{
    decision_snapshot_capture::{
        capture_decision_snapshot, CapturedDecisionSnapshot, DecisionSnapshotCaptureError,
    },
    npc_p2_projection::{
        project_npc_p2, NpcP2ProjectionError, NpcP2ProjectionOutput, NpcReconciliationDecision,
    },
    npc_p2_source::{run_npc_p2_source, NpcP2SourceError, NpcP2SourceOutput},
    p2_composition::P2SourceCompositionError,
    P2Candidate, P2CandidateBatch, P2CandidateError, P2CandidateKey, StepFatal,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::PendingNpcBatch;
use crate::{AccountId, GameSession, Intent, StockCode};
use rayon::join;
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(super) enum NpcP2PreparationError {
    #[error("NPC decision snapshot capture failed: {0}")]
    Snapshot(#[from] DecisionSnapshotCaptureError),
    #[error("NPC P2 source failed: {0}")]
    Source(#[from] NpcP2SourceError),
    #[error("NPC P2 projection failed: {0}")]
    Projection(#[from] NpcP2ProjectionError),
    #[error("NPC P2 candidate composition failed: {0}")]
    Composition(#[from] P2SourceCompositionError),
    #[error("plan root capture failed: {0}")]
    Roots(#[from] StepFatal),
    #[error("NPC projected intent for account {account:?} has no sealed source identity")]
    UnkeyedProjectedIntent { account: AccountId },
    #[error("NPC projected source key {key:?} does not exist in the sealed raw source")]
    UnknownProjectedSource { key: P2CandidateKey },
}

/// NPC-only P2 output for the main three-source orchestrator.
///
/// Candidate identities cover each account's reconciliation commands followed by residual
/// intents. Raw strategy identities remain in the projection for provenance. Player and
/// plan-chain candidates have not been read or appended at this boundary.
#[cfg(test)]
pub(super) struct PreparedNpcP2Source {
    pub(super) snapshot: Arc<DecisionSnapshot>,
    pub(super) projection: NpcP2ProjectionOutput,
    pub(super) candidates: P2CandidateBatch,
    pub(super) roots: PlanChainOperationBatch,
}

/// Test adapter for inspecting NPC projection and plan roots before next-tick queueing.
#[cfg(test)]
pub(super) fn prepare_npc_p2_source(
    prospective: &mut GameSession,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreparedNpcP2Source, NpcP2PreparationError> {
    let (captured, source, projection, roots) =
        prepare_npc_projection(prospective, roots_override)?;
    let candidates = projected_candidates(&source, &projection)?;
    Ok(PreparedNpcP2Source {
        snapshot: captured.snapshot,
        projection,
        candidates,
        roots,
    })
}

fn prepare_npc_projection(
    prospective: &mut GameSession,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<
    (
        CapturedDecisionSnapshot,
        NpcP2SourceOutput,
        NpcP2ProjectionOutput,
        PlanChainOperationBatch,
    ),
    NpcP2PreparationError,
> {
    let captured = capture_decision_snapshot(prospective)?;
    let (source, roots) = match roots_override {
        Some(roots) => (run_npc_p2_source(captured.snapshot.clone()), Ok(roots)),
        None => join(
            || run_npc_p2_source(captured.snapshot.clone()),
            || {
                prospective.capture_decision_chain_roots(
                    captured.snapshot.due_npc_ids(),
                    &captured.snapshot,
                )
            },
        ),
    };
    let mut source = source?;
    let projection = project_npc_p2(prospective, &captured, &mut source)?;
    let roots = roots?;
    Ok((captured, source, projection, roots))
}

/// Called on the completed tick candidate immediately before P9. Decisions are
/// based on this committed version; their orders enter the next market tick.
pub(in crate::session) fn queue_npc_for_next_tick(
    session: &mut GameSession,
) -> Result<(), StepFatal> {
    if session.pending_npc.is_some() {
        return Err(invariant(
            "next-tick NPC queue still contains an unconsumed batch",
        ));
    }
    if session
        .attention_queue
        .peek()
        .is_none_or(|std::cmp::Reverse((scheduled_tick, _))| *scheduled_tick > session.tick)
    {
        session.pending_npc = Some(PendingNpcBatch {
            observed_tick: session.tick,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        return Ok(());
    }
    let (captured, source, projection, _) =
        prepare_npc_projection(session, Some(PlanChainOperationBatch::empty()))
            .map_err(|error| invariant(&error.to_string()))?;
    let projected = projected_ordered_intents(&source, &projection)
        .map_err(|error| invariant(&error.to_string()))?;
    session.pending_npc = Some(PendingNpcBatch {
        observed_tick: session.tick,
        observed_accounts: captured.snapshot.due_npc_ids().to_vec(),
        intents: projected.intents,
        dependencies: projected.dependencies,
    });
    Ok(())
}

/// The queue is consumed on the discardable tick shadow after P0. Its source
/// keys identify facts only; neither account ID nor key order grants priority.
pub(super) fn take_ready_npc_batch(
    session: &mut GameSession,
) -> Result<(P2CandidateBatch, Vec<AccountId>), StepFatal> {
    let ready = session.pending_npc.take().ok_or_else(|| {
        invariant("next-tick NPC queue is missing from a committed market version")
    })?;
    if ready.observed_tick != session.tick {
        return Err(invariant(
            "queued NPC observation tick does not match the next market tick",
        ));
    }
    ready
        .validate_dependencies()
        .map_err(|error| invariant(&error))?;
    let mut local = BTreeMap::<AccountId, u64>::new();
    let mut candidates = Vec::with_capacity(ready.intents.len());
    for (account, intent) in ready.intents {
        let index = local.entry(account).or_default();
        candidates.push(P2Candidate::new(
            P2CandidateKey::npc(account, *index),
            account,
            intent,
        ));
        *index = index
            .checked_add(1)
            .ok_or_else(|| invariant("queued NPC local sequence overflow"))?;
    }
    Ok((
        P2CandidateBatch::new(with_dependencies(candidates, ready.dependencies))
            .map_err(|error| invariant(&error.to_string()))?,
        ready.observed_accounts,
    ))
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::npc_p2_preparation".to_owned(),
    }
}
#[cfg(test)]
fn projected_candidates(
    source: &NpcP2SourceOutput,
    projection: &NpcP2ProjectionOutput,
) -> Result<P2CandidateBatch, NpcP2PreparationError> {
    let ordered = projected_ordered_intents(source, projection)?;
    let mut local = BTreeMap::<AccountId, u64>::new();
    let mut projected = Vec::with_capacity(ordered.intents.len());
    for (account, intent) in ordered.intents {
        let index = local.entry(account).or_default();
        projected.push(P2Candidate::new(
            P2CandidateKey::npc(account, *index),
            account,
            intent,
        ));
        *index = index
            .checked_add(1)
            .ok_or(NpcP2SourceError::IntentOrdinalOverflow { account })?;
    }
    P2CandidateBatch::new(with_dependencies(projected, ordered.dependencies))
        .map_err(P2SourceCompositionError::Candidate)
        .map_err(Into::into)
}

struct ProjectedNpcIntents {
    intents: Vec<(AccountId, Intent)>,
    dependencies: Vec<(usize, usize)>,
}

fn with_dependencies(
    candidates: Vec<P2Candidate>,
    dependencies: Vec<(usize, usize)>,
) -> Vec<P2Candidate> {
    let mut predecessors = BTreeMap::<usize, Vec<P2CandidateKey>>::new();
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
    source: &NpcP2SourceOutput,
    projection: &NpcP2ProjectionOutput,
) -> Result<ProjectedNpcIntents, NpcP2PreparationError> {
    let mut raw_by_key = BTreeMap::new();
    for raw in source.intents() {
        let key = raw.key();
        let account = match key {
            P2CandidateKey::Npc { account, .. } => *account,
            _ => return Err(P2SourceCompositionError::NonNpcKey(key.clone()).into()),
        };
        if raw_by_key.insert(key, account).is_some() {
            return Err(
                P2SourceCompositionError::Candidate(P2CandidateError::DuplicateKey(key.clone()))
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
        let key =
            intent
                .source_key()
                .cloned()
                .ok_or(NpcP2PreparationError::UnkeyedProjectedIntent {
                    account: intent.account(),
                })?;
        let raw_owner = raw_by_key
            .get(&key)
            .ok_or_else(|| NpcP2PreparationError::UnknownProjectedSource { key: key.clone() })?;
        if *raw_owner != intent.account() {
            return Err(P2SourceCompositionError::NpcOwnerMismatch {
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
