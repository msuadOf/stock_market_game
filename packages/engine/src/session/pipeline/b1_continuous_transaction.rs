//! Joint B1 continuous-trading candidate orchestration.
//!
//! NPC and player candidates run first, followed by every adaptive plan-chain command. All
//! candidates share one immutable P1 resource snapshot, one persistent P3 validator, and one
//! per-stock P4 shadow. P5-P7 run exactly once after the operation stream is exhausted.

#[cfg(test)]
use super::P3ValidationOutput;
use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    b1_tick_finalizer::{
        finalize_continuous_tick, ContinuousLifecycleProjectionInput, ContinuousTickBoundary,
        ContinuousTickFinalizationContext,
    },
    npc_p2_preparation::{prepare_npc_p2_source, NpcP2PreparationError, PreparedNpcP2Source},
    p2_composition::{compose_projected_p2_candidates, P2SourceCompositionError},
    p3_context::build_p3_validation_context,
    p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator},
    p4_continuous_adapter::prepare_incremental_continuous_inputs,
    p4_p7_session_transaction::{P4P7SessionTransactionError, P4P7SessionTransactionOutput},
    p7_producers::adapt_p3_rejection_facts_after,
    p9_candidate_commit::{
        prepare_tick_shadow_plan_commit, CandidateTickCommitResult, PreparedTickPlanCommit,
    },
    plan_tick, EnvelopeReceipt, P2CandidateBatch, P3ConsumeOutcome, P3ValidatorDriver, PhaseInput,
    StepFatal, TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::{AccountId, Event, GameSession};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub(super) enum B1ContinuousTransactionError {
    #[error("B1 continuous candidate preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("B1 NPC P2 source failed: {0}")]
    Npc(#[from] NpcP2PreparationError),
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
    #[cfg(test)]
    pub(super) candidates: P2CandidateBatch,
    #[cfg(test)]
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    #[cfg(test)]
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
    #[cfg(test)]
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

/// Fully checked B1 tick whose only remaining operation is the infallible P9 authority swap.
pub(super) struct PreparedB1ContinuousTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    #[cfg(test)]
    output: B1ContinuousTransactionOutput,
}

pub(super) struct B1ContinuousTickResult {
    pub(super) commit: CandidateTickCommitResult,
    #[cfg(test)]
    pub(super) output: B1ContinuousTransactionOutput,
}

/// Builds the isolated P0-P9 continuous candidate used by the authoritative phase dispatcher.
///
/// Callers may inspect a preparation failure, but a successful value has no fallible work
/// remaining after candidate validation.
pub(super) fn prepare_b1_continuous_tick(
    authority: &mut GameSession,
) -> Result<PreparedB1ContinuousTick<'_>, B1ContinuousTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_b1_continuous_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = prepare_tick_shadow_plan_commit(authority, plan)?;
    Ok(PreparedB1ContinuousTick {
        commit,
        #[cfg(test)]
        output: _output,
    })
}

impl PreparedB1ContinuousTick<'_> {
    #[cfg(test)]
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
            #[cfg(test)]
            output: self.output,
        }
    }
}

/// Applies the complete B1 continuous source stream to the owned tick shadow.
pub(super) fn apply_tick_shadow_b1_continuous_transaction(
    plan: &mut TickShadowPlan,
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    apply_tick_shadow_b1_continuous_transaction_with_roots(plan, None)
}

#[cfg(test)]
pub(super) fn apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(
    plan: &mut TickShadowPlan,
    roots: PlanChainOperationBatch,
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    apply_tick_shadow_b1_continuous_transaction_with_roots(plan, Some(roots))
}

fn apply_tick_shadow_b1_continuous_transaction_with_roots(
    plan: &mut TickShadowPlan,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    let resources = plan.decision_resources.take().ok_or_else(|| {
        B1ContinuousTransactionError::Preparation(invariant(
            "P1 decision resource snapshot is absent",
        ))
    })?;
    let preceding_receipts = plan.applied_receipts.clone();
    let mut candidate = plan.state.take_session()?;
    let output = apply_session_b1_continuous_transaction(
        &mut candidate,
        resources,
        roots_override,
        &preceding_receipts,
    )?;
    plan.state.restore_success(candidate)?;
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
    mut candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
    preceding_receipts: &[EnvelopeReceipt],
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    let PreparedNpcP2Source {
        snapshot,
        projection,
        candidates: npc,
    } = prepare_npc_p2_source(&mut candidate, &resources)?;
    let player = candidate.capture_player_candidate_batch();
    let initial = compose_projected_p2_candidates(&npc, player, std::iter::empty())?;
    let mut chain = match roots_override {
        Some(roots) => AdaptivePlanChainCoordinator::capture_batch(&candidate, roots)?,
        None => AdaptivePlanChainCoordinator::capture_roots_before_p4(
            &candidate,
            projection.accepted_due_npc_ids(),
            &snapshot,
        )?,
    };

    crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
    let context = build_p3_validation_context(&candidate)?;
    let mut p3 = P3ValidatorDriver::new(
        resources,
        candidate.envelope_ledger.clone(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )?;
    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate)?,
    )?;

    let mut all_candidates = initial.candidates().to_vec();
    for round in apply_initial_candidate_stream(&mut p3, &mut p4, &initial)? {
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        chain.project_execution_round(&mut candidate, &round)?;
    }

    loop {
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        let batch = chain.next_ready_batch(&mut candidate)?;
        if batch.is_empty() {
            break;
        }
        all_candidates.extend(batch.iter().cloned());
        crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
        let outcomes = p3.consume_round(batch)?;
        let operations = outcomes
            .iter()
            .filter_map(|outcome| outcome.operation().cloned())
            .collect::<Vec<_>>();
        let round = if operations.is_empty() {
            None
        } else {
            crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
            let round = p4.apply_round(operations)?;
            crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
            apply_open_order_feedback(&mut p3, &outcomes, &round)?;
            Some(round)
        };
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        chain.advance_after_typed_outcomes(&mut candidate, &outcomes, round.as_ref())?;
    }

    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let mut plan_completion = chain.finish()?;
    let _plan_reports = std::mem::take(&mut plan_completion.reports);
    let validation = p3.finish();
    let candidates =
        P2CandidateBatch::new(all_candidates).map_err(|error| invariant(&error.to_string()))?;
    let mut next_session_local_index = 0_u64;
    let mut preceding_facts = adapt_p3_rejection_facts_after(
        &candidates,
        validation.results(),
        &mut next_session_local_index,
    )?;
    preceding_facts.extend(plan_completion.take_event_facts(&mut next_session_local_index)?);

    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let boundary = ContinuousTickBoundary::capture(&candidate)
        .map_err(B1ContinuousTransactionError::Finalization)?;
    let finish = p4.finish_for_tick(boundary.ends_day)?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6: _p6,
    } = finalize_continuous_tick(
        &mut candidate,
        finish,
        preceding_facts,
        preceding_receipts,
        ContinuousTickFinalizationContext {
            boundary,
            day_end_event_base: u64::try_from(validation.results().len())
                .map_err(|_| invariant("P3 count exceeds event identity domain"))?,
            next_session_local_index: &mut next_session_local_index,
            lifecycle: ContinuousLifecycleProjectionInput {
                candidates: &candidates,
                validation: &validation,
                consumed: &plan_completion.consumed,
            },
        },
    )?;
    candidate.next_order_id = validation.next_order_id_after();

    Ok(B1ContinuousTransactionOutput {
        #[cfg(test)]
        candidates,
        #[cfg(test)]
        validation,
        events,
        receipts,
        #[cfg(test)]
        p6: _p6,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

fn apply_initial_candidate_stream(
    p3: &mut P3ValidatorDriver,
    p4: &mut IncrementalContinuousStockCoordinator,
    initial: &P2CandidateBatch,
) -> Result<Vec<ContinuousExecutionRound>, StepFatal> {
    let mut rounds = Vec::new();
    if !initial.candidates().is_empty() {
        crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
        let outcomes = p3.consume_round(initial.candidates().iter().cloned())?;
        let operations = outcomes
            .iter()
            .filter_map(|outcome| outcome.operation().cloned())
            .collect::<Vec<_>>();
        if !operations.is_empty() {
            crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
            let round = p4.apply_round(operations)?;
            crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
            apply_open_order_feedback(p3, &outcomes, &round)?;
            rounds.push(round);
        }
    }
    Ok(rounds)
}

fn apply_open_order_feedback(
    p3: &mut P3ValidatorDriver,
    outcomes: &[P3ConsumeOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    super::p4_continuous::validate_execution_facts(&round.facts)?;
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
        .collect::<std::collections::BTreeSet<_>>();
    let fact_identities = round
        .facts
        .iter()
        .map(|fact| (fact.candidate_key.clone(), fact.sealed_index))
        .collect::<std::collections::BTreeSet<_>>();
    if round.facts.len() != accepted.len()
        || accepted_identities.len() != accepted.len()
        || fact_identities != accepted_identities
    {
        return Err(invariant(
            "P4 round fact identities do not match accepted P3 operations",
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
