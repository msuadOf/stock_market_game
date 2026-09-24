//! P7 adapters for producer-owned facts that already carry complete identity.
//!
//! These adapters deliberately do not inspect `GameSession`, allocate external event
//! sequences, or infer identity from worker vector position.  P3 results are checked
//! against the canonical P2 batch's sealed identity before they are projected. P4
//! continuous facts are intentionally absent until their owning output contract carries
//! explicit sealed and per-stock trade identities.

use super::{
    p7_events::OwnedEventFact, EventStableKey, P2Candidate, P2CandidateBatch, P2CandidateKey,
    P3CandidateResult, StepFatal,
};
use crate::session::RuntimeResource;
use crate::{Event, Intent, StockCode};
use std::collections::{BTreeMap, BTreeSet};

/// Converts ordinary P3 rejections into owned P7 facts.
///
/// P3 rejection results retain their sealed identity, while the immutable P2 batch is
/// the source for the account and intent code that the public rejection event requires.
/// This validates the entire P2/P3 result correspondence before producing any facts.
#[cfg(test)]
pub(super) fn adapt_p3_rejection_facts(
    candidates: &P2CandidateBatch,
    results: &[P3CandidateResult],
) -> Result<Vec<OwnedEventFact>, StepFatal> {
    let mut next_session_local_index = 0;
    adapt_p3_rejection_facts_after(candidates, results, &mut next_session_local_index)
}

pub(super) fn adapt_p3_rejection_facts_after(
    candidates: &P2CandidateBatch,
    results: &[P3CandidateResult],
    next_session_local_index: &mut u64,
) -> Result<Vec<OwnedEventFact>, StepFatal> {
    let candidates_by_key = index_candidates(candidates)?;
    validate_p3_result_contract(&candidates_by_key, results)?;

    let mut facts = Vec::new();
    let mut pending_plan_events_limited = false;
    let mut session_cursor = *next_session_local_index;
    for result in results {
        match result {
            P3CandidateResult::Rejected {
                key,
                sealed_index,
                reason,
            } => {
                let binding = candidates_by_key
                    .get(key)
                    .ok_or_else(|| invariant("P3 rejection references no P2 candidate"))?;
                let candidate = binding.candidate;
                let event = Event::IntentRejected {
                    seq: 0,
                    account: candidate.owner(),
                    code: candidate_code(candidate)?.clone(),
                    reason: reason.clone(),
                };
                facts.push(OwnedEventFact {
                    key: EventStableKey::for_event(&event, *sealed_index),
                    event,
                });
            }
            P3CandidateResult::PendingPlanEventsLimited { .. } => {
                pending_plan_events_limited = true;
            }
            P3CandidateResult::Accepted { .. } => {}
        }
    }
    if pending_plan_events_limited {
        push_pending_plan_events_resource_limit_fact_after(&mut facts, &mut session_cursor)?;
    }
    *next_session_local_index = session_cursor;
    Ok(facts)
}

/// Adds the tick-wide pending-plan-event capacity signal to the shared phase-6 Session stream.
/// Multiple producers can discover the same saturated resource, but the public tick reports it
/// exactly once and consumes exactly one shared Session-local identity.
pub(super) fn push_pending_plan_events_resource_limit_fact_after(
    facts: &mut Vec<OwnedEventFact>,
    next_session_local_index: &mut u64,
) -> Result<(), StepFatal> {
    if facts.iter().any(|fact| {
        matches!(
            fact.event,
            Event::ResourceLimit {
                resource: RuntimeResource::PendingPlanEvents,
                ..
            }
        )
    }) {
        return Ok(());
    }
    let event = Event::ResourceLimit {
        seq: 0,
        resource: RuntimeResource::PendingPlanEvents,
        limit: crate::session::MAX_SAVED_PLAN_EVENTS as u32,
    };
    facts.push(OwnedEventFact {
        key: EventStableKey::for_event(&event, *next_session_local_index),
        event,
    });
    *next_session_local_index = next_session_local_index
        .checked_add(1)
        .ok_or_else(|| invariant("pending-plan-event Session ordinal overflow"))?;
    Ok(())
}

struct CandidateBinding<'a> {
    candidate: &'a P2Candidate,
    sealed_index: u64,
}

fn index_candidates(
    candidates: &P2CandidateBatch,
) -> Result<BTreeMap<P2CandidateKey, CandidateBinding<'_>>, StepFatal> {
    let mut indexed = BTreeMap::new();
    for (canonical_ordinal, candidate) in candidates.candidates().iter().enumerate() {
        let sealed_index = u64::try_from(canonical_ordinal)
            .map_err(|_| invariant("P2 batch exceeds the sealed identity domain"))?;
        if indexed
            .insert(
                candidate.key().clone(),
                CandidateBinding {
                    candidate,
                    sealed_index,
                },
            )
            .is_some()
        {
            return Err(invariant("P2 batch contains a duplicate candidate key"));
        }
    }
    Ok(indexed)
}

fn validate_p3_result_contract(
    candidates: &BTreeMap<P2CandidateKey, CandidateBinding<'_>>,
    results: &[P3CandidateResult],
) -> Result<(), StepFatal> {
    if results.len() != candidates.len() {
        return Err(invariant(
            "P3 result count does not match the immutable P2 candidate batch",
        ));
    }

    let mut result_keys = BTreeSet::new();
    let mut sealed_indices = BTreeSet::new();
    for result in results {
        let binding = candidates
            .get(result.key())
            .ok_or_else(|| invariant("P3 result references no P2 candidate"))?;
        if result.sealed_index() != binding.sealed_index {
            return Err(invariant(
                "P3 result sealed identity disagrees with the canonical P2 batch sealed identity",
            ));
        }
        if !result_keys.insert(result.key()) {
            return Err(invariant("P3 results contain a duplicate candidate key"));
        }
        if !sealed_indices.insert(result.sealed_index()) {
            return Err(invariant("P3 results contain a duplicate sealed identity"));
        }
    }

    Ok(())
}

fn candidate_code(candidate: &P2Candidate) -> Result<&StockCode, StepFatal> {
    match candidate.intent() {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => Ok(code),
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p7_producers".to_owned(),
    }
}
