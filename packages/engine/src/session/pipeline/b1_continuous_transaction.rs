//! Joint B1 continuous-trading candidate orchestration.
//!
//! Ready NPC, player, and plan requests use the same local admission rules. All candidates
//! share one immutable P1 resource snapshot, one persistent P3 validator, and one per-stock P4
//! shadow. P5-P7 run exactly once after the operation stream is exhausted.

#[cfg(test)]
use super::P3ValidationOutput;
use super::{
    b1_tick_finalizer::{
        finalize_continuous_tick, ContinuousLifecycleProjectionInput, ContinuousTickBoundary,
        ContinuousTickFinalizationContext,
    },
    p3_context::build_p3_validation_context,
    p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator},
    p4_continuous_adapter::prepare_incremental_continuous_inputs,
    p4_p7_session_transaction::{P4P7SessionTransactionError, P4P7SessionTransactionOutput},
    p7_producers::adapt_p3_rejection_facts,
    p9_candidate_commit::{CandidateTickCommitResult, PreparedTickPlanCommit},
    plan_tick,
    ready_ingress::ReadyIngress,
    ready_stock_stream::ReadyStockStream,
    stock_stream::{
        continuous_shards, detached_continuous_shard, drive_stock_stream, finish_continuous_shards,
    },
    EnvelopeReceipt, P2CandidateBatch, P3ConsumeOutcome, P3ValidatorDriver, PhaseInput, StepFatal,
    TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::{Event, GameSession};

#[derive(Debug, thiserror::Error)]
pub(super) enum B1ContinuousTransactionError {
    #[error("B1 continuous candidate preparation failed: {0}")]
    Preparation(#[source] StepFatal),
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
    pub(super) event_keys: Vec<super::EventStableKey>,
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
#[cfg(test)]
pub(super) fn prepare_b1_continuous_tick(
    authority: &mut GameSession,
) -> Result<PreparedB1ContinuousTick<'_>, B1ContinuousTransactionError> {
    prepare_b1_continuous_tick_with_evidence(authority, true)
}

pub(super) fn prepare_b1_continuous_tick_with_evidence(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<PreparedB1ContinuousTick<'_>, B1ContinuousTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_b1_continuous_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = super::p9_candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        authority,
        plan,
        capture_commit_evidence,
    )?;
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
    plan.event_keys.extend(output.event_keys.iter().cloned());
    Ok(output)
}

fn apply_session_b1_continuous_transaction(
    mut candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
    preceding_receipts: &[EnvelopeReceipt],
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    let sources = ReadyIngress::capture_sources(&mut candidate)?;
    // Both preparations read the same post-P0 candidate. Plan root actions may
    // change it only after P3/P4 inputs have been detached and checked.
    let frozen_candidate: &GameSession = &candidate;
    let (ingress, detached) = rayon::join(
        || sources.capture_roots(frozen_candidate, roots_override),
        || -> Result<_, StepFatal> {
            let context = build_p3_validation_context(frozen_candidate)?;
            let stock_inputs = prepare_incremental_continuous_inputs(frozen_candidate)?;
            let ledger = frozen_candidate.envelope_ledger.clone();
            let next_order_id = frozen_candidate.next_order_id;
            let config = frozen_candidate.setup.config.clone();
            Ok((context, stock_inputs, ledger, next_order_id, config))
        },
    );
    let mut ingress = ingress?;
    let (context, stock_inputs, ledger, next_order_id, config) = detached?;
    let (ready, prepared) = rayon::join(
        || ingress.first_ready_batch(&mut candidate),
        || -> Result<_, StepFatal> {
            let p3 = P3ValidatorDriver::new(resources, ledger, next_order_id, config, context)?;
            let p4 = continuous_shards(stock_inputs)?;
            Ok((p3, p4))
        },
    );
    let mut all_candidates = Vec::new();
    let (mut p3, p4) = prepared?;
    let (mut chain, notifications, mut receipts) = ingress.into_parts();
    let phase = candidate.phase();
    let mut stream = ReadyStockStream::new(
        &mut chain,
        &mut receipts,
        &mut candidate,
        &mut p3,
        &mut all_candidates,
    );
    let initial = stream.initial(ready?)?;
    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let p4 = drive_stock_stream(
        p4,
        initial,
        notifications,
        |code| detached_continuous_shard(code, phase),
        |progress| stream.continuous_progress(progress),
    )?;
    stream.finish()?;

    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let mut plan_completion = chain.finish()?;
    let _plan_reports = std::mem::take(&mut plan_completion.reports);
    let validation = p3.finish();
    let candidates =
        P2CandidateBatch::new(all_candidates).map_err(|error| invariant(&error.to_string()))?;
    let preceding_facts = adapt_p3_rejection_facts(&candidates, validation.results())?;

    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let boundary = ContinuousTickBoundary::capture(&candidate)
        .map_err(B1ContinuousTransactionError::Finalization)?;
    let finish = finish_continuous_shards(p4, boundary.ends_day)?;
    let P4P7SessionTransactionOutput {
        events,
        event_keys,
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
        event_keys,
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
            validate_execution_round(&outcomes, &round)?;
            rounds.push(round);
        }
    }
    Ok(rounds)
}

pub(super) fn validate_execution_round(
    outcomes: &[P3ConsumeOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    super::p4_continuous::validate_execution_facts(&round.facts)?;
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
    Ok(())
}

#[cfg(test)]
pub(super) fn validate_execution_round_for_test(
    outcomes: &[P3ConsumeOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    validate_execution_round(outcomes, round)
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
