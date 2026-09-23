//! Prospective one-yield plan-chain flow from P2 composition through P7.
//!
//! This component owns every mutable value it advances. It never installs its candidate into
//! authoritative `GameSession` or `PlanBook` state; P8/P9 remain responsible for that decision.

use super::{
    p2_composition::compose_p2_candidates,
    p3_context::build_p3_validation_context,
    p3_p7_session_transaction::{apply_session_p3_p7_transaction, P3P7SessionTransactionError},
    p4_p7_session_transaction::P4P7SessionTransactionOutput,
    DecisionResourceSnapshot, EnvelopeReceipt, P2CandidateKey, P3CandidateResult,
    P3ValidationOutput, StepFatal,
};
use crate::plans::PlanBook;
use crate::session::{
    npc_generation::NpcDecisionBatch,
    plan_chain_candidates::{
        PlanChainCandidateBatch, PlanChainContinuationShadow, PlanChainYieldDriver,
        PlanChainYieldDriverError,
    },
    plan_execution::PlanRouteOutcome,
};
use crate::{Event, GameSession, Intent, OrderId};

#[derive(Debug, thiserror::Error)]
pub(super) enum PlanChainP2P7TransactionError {
    #[error("plan-chain continuation yield failed: {0}")]
    Yield(#[source] PlanChainYieldDriverError),
    #[error("plan-chain P2 composition failed: {0}")]
    Composition(#[source] super::P2CandidateError),
    #[error("plan-chain P3 preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("plan-chain P3-P7 transaction failed: {0}")]
    P3P7(#[source] P3P7SessionTransactionError),
    #[error("plan-chain route outcome was not uniquely represented by P3/P7 output: {0}")]
    Outcome(#[source] StepFatal),
    #[error("plan-chain continuation resume failed: {0}")]
    Resume(#[source] PlanChainYieldDriverError),
}

/// Owned prospective output. None of these values has been installed into authority.
pub(super) struct PlanChainP2P7TransactionOutput {
    pub(super) continuation: PlanChainContinuationShadow,
    pub(super) driver: PlanChainYieldDriver,
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
}

/// Forked one-yield state handed to the unified P2 composer.
pub(super) struct PreparedPlanChainCandidate {
    driver: PlanChainYieldDriver,
    candidate: PlanChainCandidateBatch,
}

/// Continuation state after the unified P3-P7 output has been interpreted.
pub(super) struct FinalizedPlanChainCandidate {
    pub(super) continuation: PlanChainContinuationShadow,
    pub(super) driver: PlanChainYieldDriver,
}

impl PreparedPlanChainCandidate {
    pub(super) fn candidate(&self) -> PlanChainCandidateBatch {
        self.candidate.clone()
    }
}

/// Forks the prospective driver and yields exactly one candidate without touching any queue,
/// session, P3 budget, order cursor, ledger, receipt identity, or event sequence.
pub(super) fn prepare_plan_chain_candidate(
    driver: &PlanChainYieldDriver,
) -> Result<PreparedPlanChainCandidate, PlanChainP2P7TransactionError> {
    let mut prospective_driver = driver.fork();
    let candidate = prospective_driver
        .yield_next()
        .map_err(PlanChainP2P7TransactionError::Yield)?;
    Ok(PreparedPlanChainCandidate {
        driver: prospective_driver,
        candidate,
    })
}

/// Resolves this yield from the unified P3/P7 output and resumes only a private continuation
/// shadow captured from the post-P7 prospective session.
pub(super) fn finalize_plan_chain_candidate(
    prepared: PreparedPlanChainCandidate,
    prospective_session: &GameSession,
    plans: &PlanBook,
    validation: &P3ValidationOutput,
    events: &[Event],
) -> Result<FinalizedPlanChainCandidate, PlanChainP2P7TransactionError> {
    let generation = prepared.candidate.chain_generation_index;
    let owner = prepared.candidate.owner;
    let intent = &prepared.candidate.intent;
    let route_outcome = resolve_route_outcome(generation, owner, intent, validation, events)
        .map_err(PlanChainP2P7TransactionError::Outcome)?;
    let mut continuation = PlanChainContinuationShadow::capture(prospective_session, plans)
        .map_err(PlanChainP2P7TransactionError::Resume)?;
    let mut driver = prepared.driver;
    driver
        .resume(&mut continuation, route_outcome)
        .map_err(PlanChainP2P7TransactionError::Resume)?;
    Ok(FinalizedPlanChainCandidate {
        continuation,
        driver,
    })
}

/// Detached test/integration wrapper composed from [`prepare_plan_chain_candidate`] and
/// [`finalize_plan_chain_candidate`]. The B1 main chain should compose all source classes once,
/// run P3-P7 once, then call `finalize`; it should not call this wrapper from `plan_tick`.
///
/// The driver is forked before yielding: a downstream failure drops only this prospective attempt.
/// The authoritative session, plan book, player queue, order/receipt/event cursors, ledger,
/// receipt identities, and continuation owner passed by the caller remain untouched.
pub(super) fn apply_plan_chain_p2_p7_transaction(
    authority: &GameSession,
    plans: &PlanBook,
    npc: NpcDecisionBatch,
    resources: DecisionResourceSnapshot,
    driver: &PlanChainYieldDriver,
) -> Result<PlanChainP2P7TransactionOutput, PlanChainP2P7TransactionError> {
    let prepared = prepare_plan_chain_candidate(driver)?;
    let mut candidate = authority
        .clone_for_tick_shadow()
        .map_err(PlanChainP2P7TransactionError::Preparation)?;
    let player = candidate.capture_player_candidate_batch();
    let candidates = compose_p2_candidates(npc, player, [prepared.candidate()])
        .map_err(PlanChainP2P7TransactionError::Composition)?;
    let context = build_p3_validation_context(&candidate)
        .map_err(PlanChainP2P7TransactionError::Preparation)?;
    let validation = super::P2P3Handoff::new_with_context(
        candidates.clone(),
        resources,
        candidate.envelope_ledger.clone(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )
    .and_then(|handoff| handoff.validate())
    .map_err(PlanChainP2P7TransactionError::Preparation)?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6,
    } = apply_session_p3_p7_transaction(&mut candidate, &candidates, &validation)
        .map_err(PlanChainP2P7TransactionError::P3P7)?;
    let finalized =
        finalize_plan_chain_candidate(prepared, &candidate, plans, &validation, &events)?;

    Ok(PlanChainP2P7TransactionOutput {
        continuation: finalized.continuation,
        driver: finalized.driver,
        validation,
        events,
        receipts,
        p6,
    })
}

fn resolve_route_outcome(
    generation: u64,
    owner: crate::AccountId,
    intent: &Intent,
    validation: &P3ValidationOutput,
    events: &[Event],
) -> Result<PlanRouteOutcome, StepFatal> {
    let key = P2CandidateKey::plan_chain(generation);
    let result = validation
        .results()
        .iter()
        .find(|result| result.key() == &key)
        .ok_or_else(|| invariant("P3 output omitted the yielded plan-chain identity"))?;
    if validation
        .results()
        .iter()
        .filter(|result| result.key() == &key)
        .count()
        != 1
    {
        return Err(invariant(
            "P3 output duplicated the yielded plan-chain identity",
        ));
    }
    if let P3CandidateResult::Rejected { reason, .. } = result {
        return Ok(PlanRouteOutcome::Rejected(reason.clone()));
    }

    match intent {
        Intent::Cancel { code, id } => resolve_cancel_outcome(events, owner, code, *id),
        Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } => {
            let order_id = validation
                .drafts()
                .iter()
                .find(|draft| draft.candidate_key() == &key)
                .map(|draft| draft.order_id())
                .ok_or_else(|| invariant("accepted plan-chain place has no keyed P3 draft"))?;
            resolve_place_outcome(events, owner, code, order_id)
        }
    }
}

fn resolve_cancel_outcome(
    events: &[Event],
    owner: crate::AccountId,
    code: &crate::StockCode,
    order_id: OrderId,
) -> Result<PlanRouteOutcome, StepFatal> {
    let mut outcomes = events.iter().filter_map(|event| match event {
        Event::OrderCanceled {
            account,
            code: actual,
            id,
            ..
        } if *account == owner && actual == code && *id == order_id => {
            Some(PlanRouteOutcome::Canceled(*id))
        }
        Event::IntentRejected {
            account,
            code: actual,
            reason,
            ..
        } if *account == owner && actual == code => {
            Some(PlanRouteOutcome::Rejected(reason.clone()))
        }
        _ => None,
    });
    unique_outcome(&mut outcomes)
}

fn resolve_place_outcome(
    events: &[Event],
    owner: crate::AccountId,
    code: &crate::StockCode,
    order_id: OrderId,
) -> Result<PlanRouteOutcome, StepFatal> {
    let mut rejection = None;
    let mut accepted = false;
    for event in events {
        match event {
            Event::OrderAccepted {
                account,
                code: actual,
                id,
                ..
            } if *account == owner && actual == code && *id == order_id => accepted = true,
            Event::Trade {
                code: actual,
                taker,
                ..
            } if *taker == owner && actual == code => accepted = true,
            Event::IntentRejected {
                account,
                code: actual,
                reason,
                ..
            } if *account == owner && actual == code => {
                if rejection.replace(reason.clone()).is_some() {
                    return Err(invariant(
                        "multiple rejection outcomes match one plan-chain place",
                    ));
                }
            }
            _ => {}
        }
    }
    match (accepted, rejection) {
        (true, None) => Ok(PlanRouteOutcome::Accepted(order_id)),
        (false, Some(reason)) => Ok(PlanRouteOutcome::Rejected(reason)),
        (true, Some(_)) => Err(invariant(
            "accepted and rejected events both match one plan-chain place",
        )),
        (false, None) => Err(invariant(
            "P7 emitted no terminal outcome for plan-chain place",
        )),
    }
}

fn unique_outcome(
    outcomes: &mut impl Iterator<Item = PlanRouteOutcome>,
) -> Result<PlanRouteOutcome, StepFatal> {
    let outcome = outcomes
        .next()
        .ok_or_else(|| invariant("P7 emitted no terminal outcome for plan-chain cancel"))?;
    if outcomes.next().is_some() {
        return Err(invariant(
            "P7 emitted multiple terminal outcomes for plan-chain cancel",
        ));
    }
    Ok(outcome)
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::plan_chain_p2_p7_transaction".to_owned(),
    }
}
