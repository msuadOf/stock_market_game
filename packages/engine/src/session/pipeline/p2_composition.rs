use super::{P2Candidate, P2CandidateBatch, P2CandidateError, P2CandidateKey};
use crate::session::player_candidates::PlayerCandidateBatch;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::session) enum P2SourceCompositionError {
    #[error("P2 NPC source supplied a non-NPC key {0:?}")]
    NonNpcKey(P2CandidateKey),
    #[error("P2 NPC source key {key:?} does not belong to owner {owner:?}")]
    NpcOwnerMismatch {
        key: P2CandidateKey,
        owner: crate::AccountId,
    },
    #[error(transparent)]
    Candidate(#[from] P2CandidateError),
}

/// Combines an already projected NPC batch with queued player requests without
/// changing NPC identities. Plan commands enter through the adaptive coordinator.
pub(in crate::session) fn compose_projected_p2_candidates(
    npc: P2CandidateBatch,
    player: PlayerCandidateBatch,
) -> Result<P2CandidateBatch, P2SourceCompositionError> {
    for candidate in npc.candidates() {
        validate_projected_npc(candidate.key(), candidate.owner())?;
    }
    compose_initial_candidates(npc.into_candidates(), player).map_err(Into::into)
}

/// Validate the queued request's source without rebuilding an already owned candidate.
fn validate_projected_npc(
    key: &P2CandidateKey,
    owner: crate::AccountId,
) -> Result<(), P2SourceCompositionError> {
    match key {
        P2CandidateKey::Npc { account, .. } if *account == owner => Ok(()),
        P2CandidateKey::Npc { .. } => Err(P2SourceCompositionError::NpcOwnerMismatch {
            key: key.clone(),
            owner,
        }),
        non_npc => Err(P2SourceCompositionError::NonNpcKey(non_npc.clone())),
    }
}

fn compose_initial_candidates(
    mut candidates: Vec<P2Candidate>,
    player: PlayerCandidateBatch,
) -> Result<P2CandidateBatch, P2CandidateError> {
    candidates.reserve(player.intents.len());
    for (index, (owner, intent)) in player.intents.into_iter().enumerate() {
        let index = u64::try_from(index).map_err(|_| P2CandidateError::InvalidSourceSequence)?;
        candidates.push(P2Candidate::new(
            P2CandidateKey::player(index),
            owner,
            intent,
        ));
    }
    P2CandidateBatch::new(candidates)
}
