//! Atomic real-player candidate flow from P2 ownership transfer through P7 event collection.
//!
//! This remains an isolated integration seam. Callers pass the immutable P1 resource snapshot
//! and a prospective `GameSession`; no public `plan_tick` or authority commit is performed here.

use super::{
    p3_context::build_p3_validation_context,
    p3_p7_session_transaction::{apply_session_p3_p7_transaction, P3P7SessionTransactionError},
    p4_p7_session_transaction::P4P7SessionTransactionOutput,
    DecisionResourceSnapshot, EnvelopeReceipt, P2Candidate, P2CandidateBatch, P2CandidateError,
    P2CandidateKey, P3ValidationOutput, StepFatal, TickShadowPlan,
};
use crate::{Event, GameSession};

#[derive(Debug, thiserror::Error)]
pub(super) enum PlayerP2P7TransactionError {
    #[error("player P2-P3 preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("player P2 candidate batch failed: {0}")]
    Candidate(#[from] P2CandidateError),
    #[error("player P3-P7 transaction failed: {0}")]
    P3P7(#[from] P3P7SessionTransactionError),
}

impl From<StepFatal> for PlayerP2P7TransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

pub(super) struct PlayerP2P7TransactionOutput {
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
}

/// Transfers the real player FIFO into a private candidate and installs that candidate only after
/// P3 validation, all stock workers, P5/P6, and P7 have succeeded.
pub(super) fn apply_tick_shadow_player_p2_p7_transaction(
    plan: &mut TickShadowPlan,
) -> Result<PlayerP2P7TransactionOutput, PlayerP2P7TransactionError> {
    let resources = plan.decision_resources.as_deref().cloned().ok_or_else(|| {
        PlayerP2P7TransactionError::Preparation(invariant(
            "P1 decision resource snapshot is absent".to_owned(),
        ))
    })?;
    let output = plan
        .state
        .execute_typed(|session| apply_session_player_p2_p7_transaction(session, resources))?;
    plan.event_outbox.extend(output.events.iter().cloned());
    Ok(output)
}

fn apply_session_player_p2_p7_transaction(
    session: &mut GameSession,
    resources: DecisionResourceSnapshot,
) -> Result<PlayerP2P7TransactionOutput, PlayerP2P7TransactionError> {
    let mut candidate = session
        .clone_for_tick_shadow()
        .map_err(PlayerP2P7TransactionError::Preparation)?;
    resources
        .validate_source_session(&mut candidate)
        .map_err(PlayerP2P7TransactionError::Preparation)?;
    let context =
        build_p3_validation_context(&candidate).map_err(PlayerP2P7TransactionError::Preparation)?;
    let player = candidate.capture_player_candidate_batch();
    let candidates = player
        .intents
        .into_iter()
        .enumerate()
        .map(|(index, (owner, intent))| {
            let index = u64::try_from(index).map_err(|error| {
                PlayerP2P7TransactionError::Preparation(invariant(error.to_string()))
            })?;
            Ok(P2Candidate::new(
                P2CandidateKey::player(index),
                owner,
                intent,
            ))
        })
        .collect::<Result<Vec<_>, PlayerP2P7TransactionError>>()?;
    let candidates = P2CandidateBatch::from_canonical(candidates)?;
    let validation = super::P2P3Handoff::new_with_context(
        candidates.clone(),
        resources,
        candidate.envelope_ledger.clone(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )
    .and_then(|handoff| handoff.validate())
    .map_err(PlayerP2P7TransactionError::Preparation)?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6,
    } = apply_session_p3_p7_transaction(&mut candidate, &candidates, &validation)?;

    session.commit_tick_shadow(candidate);
    Ok(PlayerP2P7TransactionOutput {
        validation,
        events,
        receipts,
        p6,
    })
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::player_p2_p7_transaction".to_owned(),
    }
}
