use super::{IntentCandidate, IntentCandidateBatch, IntentCandidateError, IntentCandidateKey};
use crate::session::player_candidates::PlayerCandidateBatch;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::session) enum CandidateCompositionError {
    #[error("DecisionShadow NPC 来源提供了非 NPC key {0:?}")]
    NonNpcKey(IntentCandidateKey),
    #[error("DecisionShadow NPC 来源 key {key:?} 不属于 owner {owner:?}")]
    NpcOwnerMismatch {
        key: IntentCandidateKey,
        owner: crate::AccountId,
    },
    #[error(transparent)]
    Candidate(#[from] IntentCandidateError),
}

/// Combines an already projected NPC batch with queued player requests without
/// changing NPC identities. Plan commands enter through the adaptive coordinator.
pub(in crate::session) fn compose_projected_candidates(
    npc: IntentCandidateBatch,
    player: PlayerCandidateBatch,
) -> Result<IntentCandidateBatch, CandidateCompositionError> {
    for candidate in npc.candidates() {
        validate_projected_npc(candidate.key(), candidate.owner())?;
    }
    compose_initial_candidates(npc.into_candidates(), player).map_err(Into::into)
}

/// Validate the queued request's source without rebuilding an already owned candidate.
fn validate_projected_npc(
    key: &IntentCandidateKey,
    owner: crate::AccountId,
) -> Result<(), CandidateCompositionError> {
    match key {
        IntentCandidateKey::Npc { account, .. } if *account == owner => Ok(()),
        IntentCandidateKey::Npc { .. } => Err(CandidateCompositionError::NpcOwnerMismatch {
            key: key.clone(),
            owner,
        }),
        non_npc => Err(CandidateCompositionError::NonNpcKey(non_npc.clone())),
    }
}

fn compose_initial_candidates(
    mut candidates: Vec<IntentCandidate>,
    player: PlayerCandidateBatch,
) -> Result<IntentCandidateBatch, IntentCandidateError> {
    candidates.reserve(player.intents.len());
    for (index, received) in player.intents.into_iter().enumerate() {
        let index =
            u64::try_from(index).map_err(|_| IntentCandidateError::InvalidSourceSequence)?;
        candidates.push(
            IntentCandidate::new(
                IntentCandidateKey::player(index),
                received.owner,
                received.intent,
            )
            .with_ingress_order(received.account_ordinal, received.stock_ordinal),
        );
    }
    IntentCandidateBatch::new(candidates)
}
