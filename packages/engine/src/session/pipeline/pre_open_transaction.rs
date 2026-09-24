//! Complete 09:25-09:30 (`PreOpen`) shadow transaction.
//!
//! The window remains silent market time: all three real P2 sources are evaluated, but every
//! accepted place/cancel operation receives its phase rejection from stock-owned P4 state. The
//! candidate advances the clock only after P5-P7 succeed and reaches authority solely through the
//! prepared P9 commit token.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    npc_p2_preparation::{prepare_npc_p2_source, PreparedNpcP2Source},
    p2_composition::compose_projected_p2_candidates,
    p3_context::build_p3_validation_context,
    p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator},
    p4_continuous_adapter::prepare_incremental_continuous_inputs,
    p4_p7_session_transaction::{
        apply_incremental_session_p4_p7_transaction, P4P7SessionTransactionOutput,
    },
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
use crate::{AccountId, Event, GameSession, TradingPhase};
use std::collections::BTreeMap;
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
    pub(super) candidates: P2CandidateBatch,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    #[cfg(test)]
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
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

/// Builds a complete PreOpen P0-P9 candidate without invoking the compatibility bridge.
pub(in crate::session) fn prepare_pre_open_tick(
    authority: &mut GameSession,
) -> Result<PreparedPreOpenTick<'_>, PreOpenTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_pre_open_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = prepare_tick_shadow_plan_commit(authority, plan)?;
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
    Ok(output)
}

fn apply_session_pre_open_transaction(
    mut candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreOpenTransactionOutput, PreOpenTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    if candidate.phase() != TradingPhase::PreOpen {
        return Err(invariant("PreOpen transaction requires the PreOpen phase").into());
    }
    let PreparedNpcP2Source {
        snapshot,
        projection,
        candidates: npc,
    } = prepare_npc_p2_source(&mut candidate, &resources).map_err(|error| {
        PreOpenTransactionError::from_source(
            "pipeline::pre_open_transaction::prepare_npc_p2_source",
            error,
        )
    })?;
    let player = candidate.capture_player_candidate_batch();
    let initial =
        compose_projected_p2_candidates(&npc, player, std::iter::empty()).map_err(|error| {
            PreOpenTransactionError::from_source(
                "pipeline::pre_open_transaction::compose_p2_sources",
                error,
            )
        })?;
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
    let inputs = prepare_incremental_continuous_inputs(&candidate)?;
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(inputs)?;

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
    let finish = p4.finish()?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6: _p6,
    } = apply_incremental_session_p4_p7_transaction(&mut candidate, finish, preceding_facts)
        .map_err(|error| {
            PreOpenTransactionError::from_source(
                "pipeline::pre_open_transaction::apply_p4_p7",
                error,
            )
        })?;
    candidate.next_order_id = validation.next_order_id_after();
    for event in &events {
        if let Event::IntentRejected { account, .. } = event {
            candidate.record_retail_intent_rejections(*account, std::slice::from_ref(event));
        }
    }
    advance_silent_pre_open_clock(&mut candidate)?;

    Ok(PreOpenTransactionOutput {
        #[cfg(test)]
        candidates,
        events,
        receipts,
        #[cfg(test)]
        p6: _p6,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

pub(super) fn apply_initial_candidate_stream(
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

pub(super) fn apply_open_order_feedback(
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
    p3.apply_open_order_feedback_round(feedback)
}

fn advance_silent_pre_open_clock(candidate: &mut GameSession) -> Result<(), StepFatal> {
    if candidate.phase() != TradingPhase::PreOpen {
        return Err(invariant("silent PreOpen clock advanced outside PreOpen"));
    }
    let tick_after = candidate
        .tick
        .checked_add(1)
        .ok_or_else(|| invariant("PreOpen tick overflow"))?;
    if tick_after.is_multiple_of(candidate.setup.ticks_per_day) {
        return Err(invariant(
            "PreOpen clock unexpectedly crossed the trading-day boundary",
        ));
    }
    candidate.tick = tick_after;
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::pre_open_transaction".to_owned(),
    }
}
