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
    DecisionResourceSnapshot, EnvelopeReceipt, P2Candidate, P2CandidateBatch, P2CandidateKey,
    P3ValidationOutput, StepFatal,
};
use crate::session::{
    plan_chain_candidates::PlanChainCandidateBatch, player_candidates::PlayerCandidateBatch,
};
use crate::{AccountId, Event, GameSession, OrderId};
use std::collections::BTreeMap;

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
    let source = run_npc_p2_source(snapshot.clone())?;
    let projection = project_npc_p2(prospective, &snapshot, &source, resources)?;
    let candidates = projected_candidates(&source, &projection)?;
    Ok(PreparedNpcP2Source {
        projection,
        candidates,
    })
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
