use super::{P2Candidate, P2CandidateBatch, P2CandidateError, P2CandidateKey};
use crate::session::{
    npc_generation::NpcDecisionBatch, pipeline::npc_p2_source::NpcP2SourceOutput,
    plan_chain_candidates::PlanChainCandidateBatch, player_candidates::PlayerCandidateBatch,
};
use std::collections::BTreeMap;

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

/// Composes the sealed L1 NPC source with the player queue and plan-chain source.
///
/// NPC keys are already authoritative at this boundary. Their owner is derived from
/// the key rather than re-indexed, so later P2 consumers retain L1's identity.
pub(in crate::session) fn compose_p2_source_candidates(
    npc: &NpcP2SourceOutput,
    player: PlayerCandidateBatch,
    plan_chain: impl IntoIterator<Item = PlanChainCandidateBatch>,
) -> Result<P2CandidateBatch, P2SourceCompositionError> {
    let npc_candidates = npc
        .intents()
        .iter()
        .map(|raw| {
            let key = raw.key().clone();
            let owner = match &key {
                P2CandidateKey::Npc { account, .. } => *account,
                non_npc => return Err(P2SourceCompositionError::NonNpcKey(non_npc.clone())),
            };
            p2_candidate_from_keyed_npc_raw(key, owner, raw.intent().clone())
        })
        .collect::<Result<Vec<_>, _>>()?;
    compose_source_classes(npc_candidates, player, plan_chain).map_err(Into::into)
}

/// Composes an already projected NPC batch with the later source classes without
/// changing the sealed NPC identities. This is the integration boundary used by
/// the joint B1 transaction after NPC projection has applied cash caps and
/// reconciliation decisions to the prospective session.
pub(in crate::session) fn compose_projected_p2_candidates(
    npc: &P2CandidateBatch,
    player: PlayerCandidateBatch,
    plan_chain: impl IntoIterator<Item = PlanChainCandidateBatch>,
) -> Result<P2CandidateBatch, P2SourceCompositionError> {
    let npc_candidates = npc
        .candidates()
        .iter()
        .map(|candidate| {
            p2_candidate_from_keyed_npc_raw(
                candidate.key().clone(),
                candidate.owner(),
                candidate.intent().clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    compose_source_classes(npc_candidates, player, plan_chain).map_err(Into::into)
}

/// Validates a keyed NPC intent at the source boundary before it becomes a P2 candidate.
/// This is visible within `session` so source-adapter tests can exercise malformed
/// producer data without exposing a mutable L1 output type.
pub(in crate::session) fn p2_candidate_from_keyed_npc_raw(
    key: P2CandidateKey,
    owner: crate::AccountId,
    intent: crate::Intent,
) -> Result<P2Candidate, P2SourceCompositionError> {
    match &key {
        P2CandidateKey::Npc { account, .. } if *account == owner => {
            Ok(P2Candidate::new(key, owner, intent))
        }
        P2CandidateKey::Npc { .. } => {
            Err(P2SourceCompositionError::NpcOwnerMismatch { key, owner })
        }
        non_npc => Err(P2SourceCompositionError::NonNpcKey(non_npc.clone())),
    }
}

pub(in crate::session) fn compose_p2_candidates(
    npc: NpcDecisionBatch,
    player: PlayerCandidateBatch,
    plan_chain: impl IntoIterator<Item = PlanChainCandidateBatch>,
) -> Result<P2CandidateBatch, P2CandidateError> {
    compose_source_classes(index_legacy_npc_candidates(npc)?, player, plan_chain)
}

fn index_legacy_npc_candidates(
    npc: NpcDecisionBatch,
) -> Result<Vec<P2Candidate>, P2CandidateError> {
    let mut candidates = Vec::with_capacity(npc.intents.len());
    let mut npc_indexes = BTreeMap::new();
    for (owner, intent) in npc.intents {
        let index = npc_indexes.entry(owner).or_insert(0_u64);
        candidates.push(P2Candidate::new(
            P2CandidateKey::npc(owner, *index),
            owner,
            intent,
        ));
        *index = index
            .checked_add(1)
            .ok_or(P2CandidateError::NonCanonicalBatch)?;
    }
    Ok(candidates)
}

fn compose_source_classes(
    mut candidates: Vec<P2Candidate>,
    player: PlayerCandidateBatch,
    plan_chain: impl IntoIterator<Item = PlanChainCandidateBatch>,
) -> Result<P2CandidateBatch, P2CandidateError> {
    let plan_chain = plan_chain.into_iter();
    candidates.reserve(player.intents.len() + plan_chain.size_hint().0);
    for (index, (owner, intent)) in player.intents.into_iter().enumerate() {
        let index = u64::try_from(index).map_err(|_| P2CandidateError::NonCanonicalBatch)?;
        candidates.push(P2Candidate::new(
            P2CandidateKey::player(index),
            owner,
            intent,
        ));
    }
    let mut next_plan_chain_index = 0_u64;
    for candidate in plan_chain {
        if candidate.chain_generation_index != next_plan_chain_index {
            return Err(P2CandidateError::NonCanonicalBatch);
        }
        candidates.push(P2Candidate::new(
            P2CandidateKey::plan_chain(candidate.chain_generation_index),
            candidate.owner,
            candidate.intent,
        ));
        next_plan_chain_index = next_plan_chain_index
            .checked_add(1)
            .ok_or(P2CandidateError::NonCanonicalBatch)?;
    }
    P2CandidateBatch::from_canonical(candidates)
}
