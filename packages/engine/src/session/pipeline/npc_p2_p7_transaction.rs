//! Atomic real-NPC candidate flow from the sealed P2 snapshot through P7.
//!
//! This is a detached prospective-session seam. It does not consume the later player or
//! plan-chain source classes, claim their final combined event stream, or commit authority.

use super::{
    decision_snapshot_capture::{capture_decision_snapshot, DecisionSnapshotCaptureError},
    npc_p2_projection::{project_npc_p2, NpcP2ProjectionError, NpcP2ProjectionOutput},
    npc_p2_source::{run_npc_p2_source, NpcP2SourceError, NpcP2SourceOutput},
    p2_composition::{compose_p2_source_candidates, P2SourceCompositionError},
    p3_context::build_p3_validation_context,
    p3_p7_session_transaction::{apply_session_p3_p7_transaction, P3P7SessionTransactionError},
    p4_p7_session_transaction::P4P7SessionTransactionOutput,
    DecisionResourceSnapshot, DecisionSnapshot, EnvelopeReceipt, P2Candidate, P2CandidateBatch,
    P2CandidateKey, P3ValidationOutput, StepFatal,
};
use crate::session::{
    plan_chain_candidates::PlanChainCandidateBatch, player_candidates::PlayerCandidateBatch,
};
use crate::{AccountId, Event, GameSession, OrderId};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(super) enum NpcP2P7TransactionError {
    #[error("NPC decision snapshot capture failed: {0}")]
    Snapshot(#[from] DecisionSnapshotCaptureError),
    #[error("NPC P2 source failed: {0}")]
    Source(#[from] NpcP2SourceError),
    #[error("NPC P2 projection failed: {0}")]
    Projection(#[from] NpcP2ProjectionError),
    #[error("NPC P2 candidate composition failed: {0}")]
    Composition(#[from] P2SourceCompositionError),
    #[error(
        "NPC reconciliation for account {account:?}, order {order_id:?} has no sealed candidate identity"
    )]
    UnkeyedReconciliation {
        account: AccountId,
        order_id: OrderId,
    },
    #[error("NPC projected intent for account {account:?} has no sealed source identity")]
    UnkeyedProjectedIntent { account: AccountId },
    #[error("NPC projected source key {key:?} does not exist in the sealed raw source")]
    UnknownProjectedSource { key: P2CandidateKey },
    #[error("NPC P2-P3 preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("NPC P3-P7 transaction failed: {0}")]
    P3P7(#[from] P3P7SessionTransactionError),
}

pub(super) struct NpcP2P7TransactionOutput {
    pub(super) projection: NpcP2ProjectionOutput,
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
}

/// NPC-only P2 output for the main three-source orchestrator.
///
/// The candidates retain their sealed NPC source identities. Player and plan-chain candidates
/// have not been read or appended at this boundary.
pub(super) struct PreparedNpcP2Source {
    pub(super) projection: NpcP2ProjectionOutput,
    pub(super) candidates: P2CandidateBatch,
}

/// Advances only the NPC portion of a caller-owned prospective session.
///
/// This function performs no P3-P7 work and never commits into another session. If it returns an
/// error after an earlier NPC substage succeeded, the outer orchestrator must discard the passed
/// prospective session together with the rest of its unpublished tick candidate.
pub(super) fn prepare_npc_p2_source(
    prospective: &mut GameSession,
    resources: &DecisionResourceSnapshot,
) -> Result<PreparedNpcP2Source, NpcP2P7TransactionError> {
    resources
        .validate_source_session(prospective)
        .map_err(NpcP2P7TransactionError::Preparation)?;
    let snapshot = capture_decision_snapshot(prospective)?;
    prepare_npc_p2_source_from_snapshot(prospective, resources, snapshot)
}

/// Advances NPC P2 from a decision snapshot already captured by the unified orchestrator.
///
/// The snapshot is borrowed by identity through its `Arc`, so NPC projection and the plan-chain
/// source can consume the same sealed decision input without advancing attention twice.
pub(super) fn prepare_npc_p2_source_from_snapshot(
    prospective: &mut GameSession,
    resources: &DecisionResourceSnapshot,
    snapshot: Arc<DecisionSnapshot>,
) -> Result<PreparedNpcP2Source, NpcP2P7TransactionError> {
    resources
        .validate_source_session(prospective)
        .map_err(NpcP2P7TransactionError::Preparation)?;
    validate_snapshot_source_session(prospective, &snapshot)
        .map_err(NpcP2P7TransactionError::Preparation)?;
    let source = run_npc_p2_source(snapshot.clone())?;
    let projection = project_npc_p2(prospective, &snapshot, &source, resources)?;
    let candidates = projected_candidates(&source, &projection)?;
    Ok(PreparedNpcP2Source {
        projection,
        candidates,
    })
}

fn validate_snapshot_source_session(
    prospective: &GameSession,
    snapshot: &DecisionSnapshot,
) -> Result<(), StepFatal> {
    if prospective.tick != snapshot.tick()
        || prospective.seed != snapshot.npc_seed_base()
        || prospective.phase() != snapshot.phase()
        || prospective.current_market_minute() != snapshot.market_minute()
    {
        return Err(snapshot_source_mismatch(format!(
            "decision snapshot tick/seed/phase/minute {}/{}/{:?}/{} does not match its source session {}/{}/{:?}/{}",
            snapshot.tick(),
            snapshot.npc_seed_base(),
            snapshot.phase(),
            snapshot.market_minute(),
            prospective.tick,
            prospective.seed,
            prospective.phase(),
            prospective.current_market_minute(),
        )));
    }
    let sealed_market = serde_json::to_vec(snapshot.market()).map_err(|error| {
        snapshot_source_mismatch(format!("cannot compare sealed market input: {error}"))
    })?;
    let source_market = serde_json::to_vec(&prospective.build_market_view()).map_err(|error| {
        snapshot_source_mismatch(format!(
            "cannot compare source-session market input: {error}"
        ))
    })?;
    if sealed_market != source_market {
        return Err(snapshot_source_mismatch(
            "decision snapshot market input does not match its source session".to_owned(),
        ));
    }
    for account in snapshot.due_npc_ids() {
        let sealed = snapshot.account(*account).map_err(|error| {
            snapshot_source_mismatch(format!(
                "decision snapshot has invalid due account {account:?}: {error}"
            ))
        })?;
        let source = prospective.accounts.get(account).ok_or_else(|| {
            snapshot_source_mismatch(format!(
                "decision snapshot due account {account:?} is missing from its source session"
            ))
        })?;
        let source_strategy = source
            .strategy
            .as_ref()
            .ok_or_else(|| {
                snapshot_source_mismatch(format!(
                    "decision snapshot due account {account:?} has no source-session strategy"
                ))
            })?
            .production_state()
            .map_err(|error| {
                snapshot_source_mismatch(format!(
                    "decision snapshot due account {account:?} has invalid source-session strategy: {error}"
                ))
            })?;
        if source.kind != sealed.kind()
            || &source_strategy != sealed.strategy_state()
            || prospective.retail_experience.get(account) != sealed.retail_experience()
        {
            return Err(snapshot_source_mismatch(format!(
                "decision snapshot due account {account:?} does not match its source-session decision state"
            )));
        }
    }
    Ok(())
}

fn snapshot_source_mismatch(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::npc_p2_p7_transaction::validate_snapshot_source_session".to_owned(),
    }
}

/// Runs only the NPC source class on a private candidate and installs it after the complete
/// P2-P7 operation succeeds. The caller-owned P1 snapshot is borrowed once by both projection
/// and P3, so neither stage can silently reseal resources from later candidate mutations.
pub(super) fn apply_session_npc_p2_p7_transaction(
    session: &mut GameSession,
    resources: &DecisionResourceSnapshot,
) -> Result<NpcP2P7TransactionOutput, NpcP2P7TransactionError> {
    let mut candidate = session
        .clone_for_tick_shadow()
        .map_err(NpcP2P7TransactionError::Preparation)?;
    let PreparedNpcP2Source {
        projection,
        candidates,
    } = prepare_npc_p2_source(&mut candidate, resources)?;
    let context =
        build_p3_validation_context(&candidate).map_err(NpcP2P7TransactionError::Preparation)?;
    let validation = super::P2P3Handoff::new_with_context(
        candidates.clone(),
        resources.clone(),
        candidate.envelope_ledger.clone(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )
    .and_then(|handoff| handoff.validate())
    .map_err(NpcP2P7TransactionError::Preparation)?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6,
    } = apply_session_p3_p7_transaction(&mut candidate, &candidates, &validation)?;

    session.commit_tick_shadow(candidate);
    Ok(NpcP2P7TransactionOutput {
        projection,
        validation,
        events,
        receipts,
        p6,
    })
}

fn projected_candidates(
    source: &NpcP2SourceOutput,
    projection: &NpcP2ProjectionOutput,
) -> Result<P2CandidateBatch, NpcP2P7TransactionError> {
    for decision in projection.reconciliation_decisions() {
        let (account, order_id, code, replacement) = decision.contract_parts();
        if code.is_some() || replacement.is_some() {
            return Err(NpcP2P7TransactionError::UnkeyedReconciliation { account, order_id });
        }
    }

    let raw = compose_p2_source_candidates(
        source,
        PlayerCandidateBatch {
            intents: Vec::new(),
        },
        std::iter::empty::<PlanChainCandidateBatch>(),
    )?;
    let raw_by_key = raw
        .candidates()
        .iter()
        .map(|candidate| (candidate.key().clone(), candidate))
        .collect::<BTreeMap<_, _>>();
    let mut projected = Vec::with_capacity(projection.residual_intents().len());
    for intent in projection.residual_intents() {
        let key = intent.source_key().cloned().ok_or(
            NpcP2P7TransactionError::UnkeyedProjectedIntent {
                account: intent.account(),
            },
        )?;
        let raw = raw_by_key
            .get(&key)
            .ok_or_else(|| NpcP2P7TransactionError::UnknownProjectedSource { key: key.clone() })?;
        if raw.owner() != intent.account() {
            return Err(P2SourceCompositionError::NpcOwnerMismatch {
                key,
                owner: intent.account(),
            }
            .into());
        }
        projected.push(P2Candidate::new(
            key,
            intent.account(),
            intent.projected_intent().clone(),
        ));
    }
    P2CandidateBatch::from_canonical(projected)
        .map_err(P2SourceCompositionError::Candidate)
        .map_err(Into::into)
}
