//! Complete 09:25-09:30 (`PreOpen`) shadow transaction.
//!
//! The window remains silent market time: all three real P2 sources are evaluated, but every
//! accepted place/cancel operation receives its phase rejection from stock-owned P4 state. The
//! candidate advances the clock only after P5-P7 succeed and reaches authority solely through the
//! prepared P9 commit token.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    decision_snapshot_capture::capture_decision_snapshot,
    npc_p2_p7_transaction::{prepare_npc_p2_source_from_snapshot, PreparedNpcP2Source},
    p2_composition::compose_projected_p2_candidates,
    p3_context::build_p3_validation_context,
    p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator},
    p4_continuous_adapter::prepare_incremental_continuous_inputs,
    p4_p7_session_transaction::{
        apply_incremental_session_p4_p7_transaction, P4P7SessionTransactionOutput,
    },
    p7_producers::adapt_p3_rejection_facts_after,
    p9_candidate_commit::{
        prepare_tick_shadow_plan_commit, CandidateTickCommitResult, P8AuthorityGuard,
        PreparedTickPlanCommit,
    },
    plan_tick, EnvelopeReceipt, P2CandidateBatch, P3ConsumeOutcome, P3ValidationOutput,
    P3ValidatorDriver, PhaseInput, StepFatal, TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
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
    /// Main-cutover adapter. Any nested `StepFatal`, including `Internal`, is returned unchanged.
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
    pub(super) candidates: P2CandidateBatch,
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

/// Fully checked PreOpen tick whose only remaining operation is the infallible P9 swap.
pub(in crate::session) struct PreparedPreOpenTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    output: PreOpenTransactionOutput,
}

pub(in crate::session) struct PreOpenTickResult {
    pub(super) commit: CandidateTickCommitResult,
    pub(super) output: PreOpenTransactionOutput,
}

/// Builds a complete PreOpen P0-P9 candidate without invoking the compatibility bridge.
pub(in crate::session) fn prepare_pre_open_tick(
    authority: &mut GameSession,
) -> Result<PreparedPreOpenTick<'_>, PreOpenTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DualHashCheck);
    let guard = P8AuthorityGuard::capture(authority)?;
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let output = apply_tick_shadow_pre_open_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::DualHashCheck);
    let commit = prepare_tick_shadow_plan_commit(authority, plan, guard)?;
    Ok(PreparedPreOpenTick { commit, output })
}

impl PreparedPreOpenTick<'_> {
    pub(in crate::session) fn commit(self) -> PreOpenTickResult {
        PreOpenTickResult {
            commit: self.commit.commit(),
            output: self.output,
        }
    }
}

impl PreOpenTickResult {
    /// Consumes the committed result at the session authority boundary without widening P9 types.
    pub(in crate::session) fn into_events(self) -> Vec<Event> {
        self.commit.tick.events
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
    let resources = plan.decision_resources.as_deref().cloned().ok_or_else(|| {
        PreOpenTransactionError::from(invariant("P1 decision resource snapshot is absent"))
    })?;
    let output = plan.state.execute_typed(|prospective| {
        apply_session_pre_open_transaction(prospective, resources, roots_override)
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

fn apply_session_pre_open_transaction(
    prospective: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreOpenTransactionOutput, PreOpenTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    let mut candidate = prospective.clone_for_tick_shadow()?;
    if candidate.phase() != TradingPhase::PreOpen {
        return Err(invariant("PreOpen transaction requires the PreOpen phase").into());
    }
    let snapshot = capture_decision_snapshot(&mut candidate).map_err(|error| {
        PreOpenTransactionError::from_source(
            "pipeline::pre_open_transaction::capture_decision_snapshot",
            error,
        )
    })?;
    let PreparedNpcP2Source {
        projection,
        candidates: npc,
    } = prepare_npc_p2_source_from_snapshot(&mut candidate, &resources, snapshot.clone()).map_err(
        |error| {
            PreOpenTransactionError::from_source(
                "pipeline::pre_open_transaction::prepare_npc_p2_source",
                error,
            )
        },
    )?;
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

    while let Some(plan_candidate) = {
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        chain.next_candidate(&mut candidate)?
    } {
        all_candidates.push(plan_candidate.clone());
        crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
        let outcome = p3.consume(plan_candidate)?;
        let round = match outcome.operation().cloned() {
            Some(operation) => {
                crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
                let round = p4.apply_round(vec![operation])?;
                crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
                apply_open_order_feedback(&mut p3, std::slice::from_ref(&outcome), &round)?;
                Some(round)
            }
            None => None,
        };
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        chain.advance_after_typed_outcome(&mut candidate, &outcome, round.as_ref())?;
    }

    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let mut plan_completion = chain.finish()?;
    let plan_reports = std::mem::take(&mut plan_completion.reports);
    let validation = p3.finish();
    let candidates = P2CandidateBatch::from_canonical(all_candidates)
        .map_err(|error| invariant(&error.to_string()))?;
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
        p6,
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

    prospective.commit_tick_shadow(candidate);
    Ok(PreOpenTransactionOutput {
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
    for candidate in initial.candidates().iter().cloned() {
        crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
        let outcome = p3.consume(candidate)?;
        if let Some(operation) = outcome.operation().cloned() {
            crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
            let round = p4.apply_round(vec![operation])?;
            crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
            apply_open_order_feedback(p3, std::slice::from_ref(&outcome), &round)?;
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
    for outcome in accepted {
        let accounts = deltas
            .remove(&(outcome.candidate_key().clone(), outcome.sealed_index()))
            .unwrap_or_default();
        p3.apply_open_order_feedback(outcome.candidate_key(), outcome.sealed_index(), accounts)?;
    }
    if !deltas.is_empty() {
        return Err(invariant(
            "P4 round returned open-order feedback for an unknown P3 operation",
        ));
    }
    Ok(())
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
