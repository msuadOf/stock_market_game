//! Complete 09:25-09:30 (`PreOpen`) shadow transaction.
//!
//! The window remains silent market time: all three real P2 sources are evaluated, but every
//! accepted place/cancel operation receives its phase rejection from stock-owned P4 state. The
//! candidate advances the clock only after ReceiptAggregation-Projection succeed and reaches authority solely through the
//! prepared P9 commit token.

#[cfg(test)]
use super::continuous_matching::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator};
#[cfg(test)]
use super::CandidateValidationOutcome;
use super::{
    account_validation_context::build_account_validation_context,
    candidate_commit::{CandidateTickCommitResult, PreparedTickPlanCommit},
    continuous_matching_adapter::prepare_incremental_continuous_inputs,
    plan_tick,
    ready_ingress::ReadyIngress,
    ready_stock_stream::ReadyStockStream,
    session_execution_transaction::{
        apply_incremental_session_execution_transaction, SessionExecutionTransactionOutput,
    },
    session_fact_producers::adapt_account_validation_rejection_facts,
    stock_stream::{
        continuous_shards, detached_continuous_shard, drive_stock_stream, finish_continuous_shards,
    },
    AccountValidatorDriver, EnvelopeReceipt, IntentCandidateBatch, PhaseInput, StepFatal,
    TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::{Event, GameSession, TradingPhase};
use std::error::Error;

#[derive(Debug, thiserror::Error)]
#[error("PreOpen tick transaction failed: {fatal}")]
pub(in crate::session) struct PreOpenTransactionError {
    #[source]
    fatal: StepFatal,
}

impl PreOpenTransactionError {
    /// Any nested `StepFatal` keeps its original location and description.
    pub(in crate::session) fn into_fatal(self) -> StepFatal {
        self.fatal
    }

    fn from_source<E>(location: &'static str, error: E) -> Self
    where
        E: Error + 'static,
    {
        let description = error.to_string();
        let mut current: Option<&(dyn Error + 'static)> = Some(&error);
        while let Some(source) = current {
            if let Some(fatal) = source.downcast_ref::<StepFatal>() {
                return Self {
                    fatal: fatal.clone(),
                };
            }
            current = source.source();
        }
        Self {
            fatal: StepFatal::InvariantViolation {
                description,
                location: location.to_owned(),
            },
        }
    }

    #[cfg(test)]
    pub(super) fn from_source_for_test<E>(error: E) -> Self
    where
        E: Error + 'static,
    {
        Self::from_source("pipeline::pre_open_transaction::test", error)
    }
}

impl From<StepFatal> for PreOpenTransactionError {
    fn from(fatal: StepFatal) -> Self {
        Self { fatal }
    }
}

pub(super) struct PreOpenTransactionOutput {
    #[cfg(test)]
    pub(super) candidates: IntentCandidateBatch,
    pub(super) events: Vec<Event>,
    pub(super) event_keys: Vec<super::EventStableKey>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    #[cfg(test)]
    pub(super) settlement: super::account_settlement::SettlementTransactionOutput,
    #[cfg(test)]
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

/// Fully checked PreOpen tick whose only remaining operation is the infallible P9 swap.
pub(in crate::session) struct PreparedPreOpenTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    #[cfg(test)]
    output: PreOpenTransactionOutput,
}

pub(in crate::session) struct PreOpenTickResult {
    pub(super) commit: CandidateTickCommitResult,
    #[cfg(test)]
    pub(super) output: PreOpenTransactionOutput,
}

pub(in crate::session) fn prepare_pre_open_tick_with_evidence(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<PreparedPreOpenTick<'_>, PreOpenTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_pre_open_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = super::candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        authority,
        plan,
        capture_commit_evidence,
    )?;
    Ok(PreparedPreOpenTick {
        commit,
        #[cfg(test)]
        output: _output,
    })
}

impl PreparedPreOpenTick<'_> {
    pub(in crate::session) fn commit(self) -> PreOpenTickResult {
        PreOpenTickResult {
            commit: self.commit.commit(),
            #[cfg(test)]
            output: self.output,
        }
    }
}

impl PreOpenTickResult {
    /// Consumes the committed result at the session authority boundary without widening P9 types.
    #[cfg(test)]
    pub(in crate::session) fn into_events(self) -> Vec<Event> {
        self.commit.tick.events
    }

    pub(super) fn into_commit(self) -> CandidateTickCommitResult {
        self.commit
    }
}

pub(super) fn apply_tick_shadow_pre_open_transaction(
    plan: &mut TickShadowPlan,
) -> Result<PreOpenTransactionOutput, PreOpenTransactionError> {
    apply_tick_shadow_pre_open_transaction_inner(plan, None)
}

#[cfg(test)]
pub(super) fn apply_tick_shadow_pre_open_transaction_with_roots_for_test(
    plan: &mut TickShadowPlan,
    roots: PlanChainOperationBatch,
) -> Result<PreOpenTransactionOutput, PreOpenTransactionError> {
    apply_tick_shadow_pre_open_transaction_inner(plan, Some(roots))
}

fn apply_tick_shadow_pre_open_transaction_inner(
    plan: &mut TickShadowPlan,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreOpenTransactionOutput, PreOpenTransactionError> {
    let resources = plan.decision_resources.take().ok_or_else(|| {
        PreOpenTransactionError::from(invariant("P1 decision resource snapshot is absent"))
    })?;
    let mut candidate = plan.state.take_session()?;
    let output = apply_session_pre_open_transaction(&mut candidate, resources, roots_override)?;
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

fn apply_session_pre_open_transaction(
    candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreOpenTransactionOutput, PreOpenTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    if candidate.phase() != TradingPhase::PreOpen {
        return Err(invariant("PreOpen transaction requires the PreOpen phase").into());
    }
    let sources = ReadyIngress::capture_sources(candidate)?;
    // Plan observation and validator/book setup read the same post-P0 facts.
    // Plan actions start only after both branches have detached their inputs.
    let frozen_candidate: &GameSession = candidate;
    let (ingress, detached) = rayon::join(
        || sources.capture_roots(frozen_candidate, roots_override),
        || -> Result<_, StepFatal> {
            let context = build_account_validation_context(frozen_candidate)?;
            let stock_inputs = prepare_incremental_continuous_inputs(frozen_candidate)?;
            let ledger = frozen_candidate.state.envelope_ledger.clone();
            let next_order_id = frozen_candidate.state.next_order_id;
            let config = frozen_candidate.state.setup.config.clone();
            Ok((context, stock_inputs, ledger, next_order_id, config))
        },
    );
    let mut ingress = ingress?;
    let (context, stock_inputs, ledger, next_order_id, config) = detached?;
    let (ready, prepared) = rayon::join(
        || ingress.first_ready_batch(candidate),
        || -> Result<_, StepFatal> {
            let validator =
                AccountValidatorDriver::new(resources, ledger, next_order_id, config, context)?;
            let stock_execution = continuous_shards(stock_inputs)?;
            Ok((validator, stock_execution))
        },
    );
    let mut all_candidates = Vec::new();
    let (mut validator, stock_execution) = prepared?;
    let (mut chain, notifications, mut receipts) = ingress.into_parts();
    let phase = candidate.phase();
    let mut stream = ReadyStockStream::new(
        &mut chain,
        &mut receipts,
        candidate,
        &mut validator,
        &mut all_candidates,
    );
    let initial = stream.initial(ready?)?;
    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let stock_execution = drive_stock_stream(
        stock_execution,
        initial,
        notifications,
        |code| detached_continuous_shard(code, phase),
        |progress| stream.continuous_progress(progress),
    )?;
    stream.finish()?;

    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let mut plan_completion = chain.finish()?;
    let _plan_reports = std::mem::take(&mut plan_completion.reports);
    crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
    let validation = validator.finish();
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let candidates =
        IntentCandidateBatch::new(all_candidates).map_err(|error| invariant(&error.to_string()))?;
    let preceding_facts =
        adapt_account_validation_rejection_facts(&candidates, validation.results())?;

    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let finish = finish_continuous_shards(stock_execution, false)?;
    let SessionExecutionTransactionOutput {
        events,
        event_keys,
        receipts,
        settlement: _settlement,
    } = apply_incremental_session_execution_transaction(candidate, finish, preceding_facts)
        .map_err(|error| {
            PreOpenTransactionError::from_source(
                "pipeline::pre_open_transaction::apply_p4_p7",
                error,
            )
        })?;
    candidate.state.next_order_id = validation.next_order_id_after();
    for event in &events {
        if let Event::IntentRejected { account, .. } = event {
            candidate.record_retail_intent_rejections(*account, std::slice::from_ref(event));
        }
    }
    advance_silent_pre_open_clock(candidate)?;

    Ok(PreOpenTransactionOutput {
        #[cfg(test)]
        candidates,
        events,
        event_keys,
        receipts,
        #[cfg(test)]
        settlement: _settlement,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

#[cfg(test)]
pub(super) fn apply_initial_candidate_stream(
    validator: &mut AccountValidatorDriver,
    stock_execution: &mut IncrementalContinuousStockCoordinator,
    initial: &IntentCandidateBatch,
) -> Result<Vec<ContinuousExecutionRound>, StepFatal> {
    let mut rounds = Vec::new();
    if !initial.candidates().is_empty() {
        crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
        let outcomes = validator.consume_round(initial.candidates().iter().cloned())?;
        let operations = outcomes
            .iter()
            .filter_map(|outcome| outcome.operation().cloned())
            .collect::<Vec<_>>();
        if !operations.is_empty() {
            crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
            let round = stock_execution.apply_round(operations)?;
            validate_execution_round(&outcomes, &round)?;
            rounds.push(round);
        }
    }
    Ok(rounds)
}

#[cfg(test)]
pub(super) fn validate_execution_round(
    outcomes: &[CandidateValidationOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    super::continuous_matching::validate_execution_facts(&round.facts)?;
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

fn advance_silent_pre_open_clock(candidate: &mut GameSession) -> Result<(), StepFatal> {
    if candidate.phase() != TradingPhase::PreOpen {
        return Err(invariant("silent PreOpen clock advanced outside PreOpen"));
    }
    let tick_after = candidate
        .state
        .tick
        .checked_add(1)
        .ok_or_else(|| invariant("PreOpen tick overflow"))?;
    if tick_after.is_multiple_of(candidate.state.setup.ticks_per_day) {
        return Err(invariant(
            "PreOpen clock unexpectedly crossed the trading-day boundary",
        ));
    }
    candidate.state.tick = tick_after;
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::pre_open_transaction".to_owned(),
    }
}
