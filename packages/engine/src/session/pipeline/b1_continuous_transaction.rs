//! Joint B1 continuous-trading candidate orchestration.
//!
//! NPC and player candidates run first, followed by every adaptive plan-chain command. All
//! candidates share one immutable P1 resource snapshot, one persistent P3 validator, and one
//! per-stock P4 shadow. P5-P7 run exactly once after the operation stream is exhausted.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    b1_tick_finalizer::{
        finalize_continuous_tick, ContinuousLifecycleProjectionInput, ContinuousTickBoundary,
    },
    decision_snapshot_capture::capture_decision_snapshot,
    npc_p2_p7_transaction::{
        prepare_npc_p2_source_from_snapshot, NpcP2P7TransactionError, PreparedNpcP2Source,
    },
    p2_composition::{compose_projected_p2_candidates, P2SourceCompositionError},
    p3_context::build_p3_validation_context,
    p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator},
    p4_continuous_adapter::prepare_incremental_continuous_inputs,
    p4_p7_session_transaction::{P4P7SessionTransactionError, P4P7SessionTransactionOutput},
    p7_producers::adapt_p3_rejection_facts,
    p9_candidate_commit::{
        prepare_tick_shadow_plan_commit, CandidateTickCommitResult, P8AuthorityGuard,
        PreparedTickPlanCommit,
    },
    plan_tick, EnvelopeReceipt, P2CandidateBatch, P3ConsumeOutcome, P3ValidationOutput,
    P3ValidatorDriver, PhaseInput, StepFatal, TickShadowPlan,
};
use crate::session::PlanExecutionReport;
use crate::{AccountId, Event, GameSession};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub(super) enum B1ContinuousTransactionError {
    #[error("B1 continuous candidate preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("B1 NPC P2 source failed: {0}")]
    Npc(#[from] NpcP2P7TransactionError),
    #[error("B1 P2 source composition failed: {0}")]
    Composition(#[from] P2SourceCompositionError),
    #[error("B1 P4-P7 transaction failed: {0}")]
    P4P7(#[from] P4P7SessionTransactionError),
    #[error("B1 continuous tick finalization failed: {0}")]
    Finalization(#[source] StepFatal),
}

impl From<StepFatal> for B1ContinuousTransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

impl B1ContinuousTransactionError {
    pub(super) fn into_fatal(self) -> StepFatal {
        super::transaction_error::into_fatal(self, "pipeline::b1_continuous_transaction")
    }
}

pub(super) struct B1ContinuousTransactionOutput {
    pub(super) candidates: P2CandidateBatch,
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

/// Fully checked B1 tick whose only remaining operation is the infallible P9 authority swap.
pub(super) struct PreparedB1ContinuousTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    output: B1ContinuousTransactionOutput,
}

pub(super) struct B1ContinuousTickResult {
    pub(super) commit: CandidateTickCommitResult,
    pub(super) output: B1ContinuousTransactionOutput,
}

/// Builds the isolated P0-P9 continuous candidate without invoking the compatibility bridge.
///
/// This is intentionally not registered in `GameSession::step`: task 8 persistence and the later
/// formal cutover gate must complete before users can enter the new authoritative state path.
pub(super) fn prepare_b1_continuous_tick(
    authority: &mut GameSession,
) -> Result<PreparedB1ContinuousTick<'_>, B1ContinuousTransactionError> {
    let guard = P8AuthorityGuard::capture(authority)?;
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let output = apply_tick_shadow_b1_continuous_transaction(&mut plan)?;
    let commit = prepare_tick_shadow_plan_commit(authority, plan, guard)?;
    Ok(PreparedB1ContinuousTick { commit, output })
}

impl PreparedB1ContinuousTick<'_> {
    pub(super) fn evidence(&self) -> &super::TickCommitEvidence {
        self.commit.evidence()
    }

    #[cfg(test)]
    pub(super) const fn output(&self) -> &B1ContinuousTransactionOutput {
        &self.output
    }

    pub(super) fn commit(self) -> B1ContinuousTickResult {
        B1ContinuousTickResult {
            commit: self.commit.commit(),
            output: self.output,
        }
    }
}

/// Applies the complete B1 continuous source stream to the owned tick shadow.
pub(super) fn apply_tick_shadow_b1_continuous_transaction(
    plan: &mut TickShadowPlan,
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    let resources = plan.decision_resources.as_deref().cloned().ok_or_else(|| {
        B1ContinuousTransactionError::Preparation(invariant(
            "P1 decision resource snapshot is absent",
        ))
    })?;
    let preceding_receipts = plan.applied_receipts.clone();
    let output = plan.state.execute_typed(|prospective| {
        apply_session_b1_continuous_transaction(prospective, resources, &preceding_receipts)
    })?;
    plan.receipt_keys.extend(
        output
            .receipts
            .iter()
            .map(|receipt| receipt.local_key.clone()),
    );
    plan.applied_receipts
        .extend(output.receipts.iter().cloned());
    plan.event_outbox.extend(output.events.iter().cloned());
    Ok(output)
}

fn apply_session_b1_continuous_transaction(
    prospective: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    preceding_receipts: &[EnvelopeReceipt],
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    let mut candidate = prospective.clone_for_tick_shadow()?;
    let snapshot =
        capture_decision_snapshot(&mut candidate).map_err(NpcP2P7TransactionError::from)?;
    let PreparedNpcP2Source {
        projection,
        candidates: npc,
    } = prepare_npc_p2_source_from_snapshot(&mut candidate, &resources, snapshot.clone())?;
    let player = candidate.capture_player_candidate_batch();
    let initial = compose_projected_p2_candidates(&npc, player, std::iter::empty())?;
    let mut chain = AdaptivePlanChainCoordinator::capture_roots_before_p4(
        &candidate,
        projection.accepted_due_npc_ids(),
        &snapshot,
    )?;

    let context = build_p3_validation_context(&candidate)?;
    let mut p3 = P3ValidatorDriver::new(
        resources,
        candidate.envelope_ledger.clone(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )?;
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate)?,
    )?;

    let mut all_candidates = initial.candidates().to_vec();
    for round in apply_initial_candidate_stream(&mut p3, &mut p4, &initial)? {
        chain.project_execution_round(&mut candidate, &round)?;
    }

    while let Some(plan_candidate) = chain.next_candidate(&mut candidate)? {
        all_candidates.push(plan_candidate.clone());
        let outcome = p3.consume(plan_candidate)?;
        let round = match outcome.operation().cloned() {
            Some(operation) => {
                let round = p4.apply_round(vec![operation])?;
                apply_open_order_feedback(&mut p3, std::slice::from_ref(&outcome), &round)?;
                Some(round)
            }
            None => None,
        };
        chain.advance_after_typed_outcome(&mut candidate, &outcome, round.as_ref())?;
    }

    let mut plan_completion = chain.finish()?;
    let plan_reports = std::mem::take(&mut plan_completion.reports);
    let validation = p3.finish();
    let candidates = P2CandidateBatch::from_canonical(all_candidates)
        .map_err(|error| invariant(&error.to_string()))?;
    let mut preceding_facts = adapt_p3_rejection_facts(&candidates, validation.results())?;
    let mut next_session_local_index = 0_u64;
    preceding_facts.extend(plan_completion.take_event_facts(&mut next_session_local_index)?);

    let boundary = ContinuousTickBoundary::capture(&candidate)
        .map_err(B1ContinuousTransactionError::Finalization)?;
    let finish = p4.finish_for_tick(boundary.ends_day)?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6,
    } = finalize_continuous_tick(
        &mut candidate,
        finish,
        preceding_facts,
        preceding_receipts,
        boundary,
        u64::try_from(validation.results().len())
            .map_err(|_| invariant("P3 count exceeds event identity domain"))?,
        ContinuousLifecycleProjectionInput {
            candidates: &candidates,
            validation: &validation,
            consumed: &plan_completion.consumed,
        },
    )?;
    candidate.next_order_id = validation.next_order_id_after();

    prospective.commit_tick_shadow(candidate);
    Ok(B1ContinuousTransactionOutput {
        candidates,
        validation,
        events,
        receipts,
        p6,
        plan_reports,
    })
}

fn apply_initial_candidate_stream(
    p3: &mut P3ValidatorDriver,
    p4: &mut IncrementalContinuousStockCoordinator,
    initial: &P2CandidateBatch,
) -> Result<Vec<ContinuousExecutionRound>, StepFatal> {
    let mut rounds = Vec::new();
    let mut remaining = initial.candidates();
    while !remaining.is_empty() {
        let count = p3.ready_round_len(remaining)?;
        let outcomes = p3.consume_round(remaining[..count].iter().cloned())?;
        let operations = outcomes
            .iter()
            .filter_map(|outcome| outcome.operation().cloned())
            .collect::<Vec<_>>();
        if !operations.is_empty() {
            let round = p4.apply_round(operations)?;
            apply_open_order_feedback(p3, &outcomes, &round)?;
            rounds.push(round);
        }
        remaining = &remaining[count..];
    }
    Ok(rounds)
}

fn apply_open_order_feedback(
    p3: &mut P3ValidatorDriver,
    outcomes: &[P3ConsumeOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    let mut deltas = BTreeMap::<(super::P2CandidateKey, u64), BTreeMap<AccountId, i64>>::new();
    for delta in &round.open_order_deltas {
        let accounts = deltas
            .entry((delta.candidate_key.clone(), delta.sealed_index))
            .or_default();
        let value = accounts.entry(delta.account).or_default();
        *value = value
            .checked_add(i64::from(delta.delta))
            .ok_or_else(|| invariant("P4 open-order feedback delta overflow"))?;
    }

    let accepted = outcomes
        .iter()
        .filter(|outcome| outcome.operation().is_some())
        .collect::<Vec<_>>();
    let accepted_identities = accepted
        .iter()
        .map(|outcome| (outcome.candidate_key().clone(), outcome.sealed_index()))
        .collect::<Vec<_>>();
    let fact_identities = round
        .facts
        .iter()
        .map(|fact| (fact.candidate_key.clone(), fact.sealed_index))
        .collect::<Vec<_>>();
    if round.facts.len() != accepted.len() || fact_identities != accepted_identities {
        return Err(invariant(
            "P4 round fact identities are not the canonical accepted P3 operation sequence",
        ));
    }
    let mut feedback = Vec::with_capacity(accepted.len());
    for outcome in accepted {
        let accounts = deltas
            .remove(&(outcome.candidate_key().clone(), outcome.sealed_index()))
            .unwrap_or_default();
        feedback.push((
            outcome.candidate_key().clone(),
            outcome.sealed_index(),
            accounts,
        ));
    }
    if !deltas.is_empty() {
        return Err(invariant(
            "P4 round returned open-order feedback for an unknown P3 operation",
        ));
    }
    p3.apply_open_order_feedback_round(feedback)?;
    Ok(())
}

#[cfg(test)]
pub(super) fn apply_open_order_feedback_for_test(
    p3: &mut P3ValidatorDriver,
    outcomes: &[P3ConsumeOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    apply_open_order_feedback(p3, outcomes, round)
}

#[cfg(test)]
pub(super) fn apply_initial_candidate_stream_for_test(
    p3: &mut P3ValidatorDriver,
    p4: &mut IncrementalContinuousStockCoordinator,
    initial: &P2CandidateBatch,
) -> Result<Vec<ContinuousExecutionRound>, StepFatal> {
    apply_initial_candidate_stream(p3, p4, initial)
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::b1_continuous_transaction".to_owned(),
    }
}
