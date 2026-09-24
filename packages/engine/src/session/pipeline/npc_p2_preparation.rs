//! Prepare NPC P2 candidates from one sealed decision snapshot and resource snapshot.
//!
//! The combined tick coordinator owns later account validation, stock processing, settlement,
//! final events, and the authority commit.

use super::{
    decision_snapshot_capture::{capture_decision_snapshot, DecisionSnapshotCaptureError},
    npc_p2_projection::{
        project_npc_p2, NpcP2ProjectionError, NpcP2ProjectionOutput, NpcReconciliationDecision,
    },
    npc_p2_source::{run_npc_p2_source, NpcP2SourceError, NpcP2SourceOutput},
    p2_composition::{compose_p2_source_candidates, P2SourceCompositionError},
    DecisionResourceSnapshot, DecisionSnapshot, P2Candidate, P2CandidateBatch, P2CandidateKey,
};
use crate::session::{
    plan_chain_candidates::PlanChainCandidateBatch, player_candidates::PlayerCandidateBatch,
};
use crate::{AccountId, GameSession, Intent};
use std::collections::BTreeMap;
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
pub(super) struct PreparedNpcP2Source {
    pub(super) snapshot: Arc<DecisionSnapshot>,
    pub(super) projection: NpcP2ProjectionOutput,
    pub(super) candidates: P2CandidateBatch,
}

/// Captures P2 input and prepares NPC candidates from the same owned tick candidate.
///
/// The snapshot is returned for the plan-chain source. The caller cannot pass a separately
/// captured snapshot. The tick coordinator seals P1 immediately before this call and does not
/// change account or market resources in between.
pub(super) fn prepare_npc_p2_source(
    prospective: &mut GameSession,
    resources: &DecisionResourceSnapshot,
) -> Result<PreparedNpcP2Source, NpcP2PreparationError> {
    let snapshot = capture_decision_snapshot(prospective)?;
    let source = run_npc_p2_source(snapshot.clone())?;
    let projection = project_npc_p2(prospective, &snapshot, &source, resources)?;
    let candidates = projected_candidates(&source, &projection)?;
    Ok(PreparedNpcP2Source {
        snapshot,
        projection,
        candidates,
    })
}
fn projected_candidates(
    source: &NpcP2SourceOutput,
    projection: &NpcP2ProjectionOutput,
) -> Result<P2CandidateBatch, NpcP2PreparationError> {
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
    let mut by_account = BTreeMap::<AccountId, Vec<Intent>>::new();
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
        by_account.entry(account).or_default().push(Intent::Cancel {
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
        let raw = raw_by_key
            .get(&key)
            .ok_or_else(|| NpcP2PreparationError::UnknownProjectedSource { key: key.clone() })?;
        if raw.owner() != intent.account() {
            return Err(P2SourceCompositionError::NpcOwnerMismatch {
                key,
                owner: intent.account(),
            }
            .into());
        }
        by_account
            .entry(intent.account())
            .or_default()
            .push(intent.projected_intent().clone());
    }
    let mut projected = Vec::new();
    for (account, intents) in by_account {
        for (index, intent) in intents.into_iter().enumerate() {
            let index = u64::try_from(index)
                .map_err(|_| NpcP2SourceError::IntentOrdinalOverflow { account })?;
            projected.push(P2Candidate::new(
                P2CandidateKey::npc(account, index),
                account,
                intent,
            ));
        }
    }
    P2CandidateBatch::new(projected)
        .map_err(P2SourceCompositionError::Candidate)
        .map_err(Into::into)
}
