//! Joint incremental call-auction transaction.
//!
//! P3 and stock-owned P4 alternate at every continuation edge. The auction coordinator is
//! initialized once from post-P0 state; only after all commands have drained does its consuming
//! finish seam run AuctionTick, completion, DayEnd and P5-P7 once.

#[cfg(test)]
use super::P3ValidationOutput;
use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    npc_p2_preparation::{prepare_npc_p2_source, NpcP2PreparationError, PreparedNpcP2Source},
    p2_composition::{compose_projected_p2_candidates, P2SourceCompositionError},
    p3_context::build_p3_validation_context,
    p7_producers::adapt_p3_rejection_facts_after,
    p9_candidate_commit::{
        prepare_tick_shadow_plan_commit, CandidateTickCommitResult, PreparedTickPlanCommit,
    },
    plan_tick,
    stock_auction::b2_auction_day_end::{
        apply_incremental_auction_finish_with_prepared_facts_and_receipts,
        finish_incremental_auction_coordinator, AuctionExecutionRound, B2AuctionDayEndError,
        B2AuctionDayEndOutput, IncrementalAuctionStockCoordinator, PreparedAuctionFinishContext,
    },
    stock_auction_adapter::prepare_incremental_auction_inputs,
    P2CandidateBatch, P3ConsumeOutcome, P3ValidatorDriver, PhaseInput, StepFatal, TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::{AccountId, GameSession};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub(super) enum B2AuctionTransactionError {
    #[error("B2 incremental auction preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("B2 incremental auction NPC source failed: {0}")]
    Npc(#[from] NpcP2PreparationError),
    #[error("B2 incremental auction P2 composition failed: {0}")]
    Composition(#[from] P2SourceCompositionError),
    #[error("B2 incremental auction finalization failed: {0}")]
    Finalization(#[from] B2AuctionDayEndError),
}

impl From<StepFatal> for B2AuctionTransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

impl B2AuctionTransactionError {
    pub(super) fn into_fatal(self) -> StepFatal {
        super::transaction_error::into_fatal(self, "pipeline::b2_auction_transaction")
    }
}

pub(super) struct B2AuctionTransactionOutput {
    #[cfg(test)]
    pub(super) candidates: P2CandidateBatch,
    #[cfg(test)]
    pub(super) validation: P3ValidationOutput,
    pub(super) auction: B2AuctionDayEndOutput,
    #[cfg(test)]
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

pub(super) struct PreparedB2AuctionTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    #[cfg(test)]
    output: B2AuctionTransactionOutput,
}

pub(super) struct B2AuctionTickResult {
    pub(super) commit: CandidateTickCommitResult,
    #[cfg(test)]
    pub(super) output: B2AuctionTransactionOutput,
}

/// Builds the auction candidate selected by the authoritative phase dispatcher.
///
/// Opening and closing auctions share the same prepared P0-P9 transaction; phase-specific
/// completion and day-end work remain inside the candidate before the infallible P9 swap.
pub(super) fn prepare_b2_auction_tick(
    authority: &mut GameSession,
) -> Result<PreparedB2AuctionTick<'_>, B2AuctionTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_b2_auction_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = prepare_tick_shadow_plan_commit(authority, plan)?;
    Ok(PreparedB2AuctionTick {
        commit,
        #[cfg(test)]
        output: _output,
    })
}

impl PreparedB2AuctionTick<'_> {
    #[cfg(test)]
    pub(super) fn evidence(&self) -> &super::TickCommitEvidence {
        self.commit.evidence()
    }

    pub(super) fn commit(self) -> B2AuctionTickResult {
        B2AuctionTickResult {
            commit: self.commit.commit(),
            #[cfg(test)]
            output: self.output,
        }
    }
}

pub(super) fn apply_tick_shadow_b2_auction_transaction(
    plan: &mut TickShadowPlan,
) -> Result<B2AuctionTransactionOutput, B2AuctionTransactionError> {
    apply_tick_shadow_b2_auction_transaction_inner(plan, None)
}

fn apply_tick_shadow_b2_auction_transaction_inner(
    plan: &mut TickShadowPlan,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<B2AuctionTransactionOutput, B2AuctionTransactionError> {
    let resources = plan.decision_resources.take().ok_or_else(|| {
        B2AuctionTransactionError::Preparation(invariant("P1 decision resource snapshot is absent"))
    })?;
    let preceding_receipts = plan.applied_receipts.clone();
    let mut candidate = plan.state.take_session()?;
    let output = apply_session_b2_auction_transaction(
        &mut candidate,
        resources,
        roots_override,
        &preceding_receipts,
    )?;
    plan.state.restore_success(candidate)?;
    plan.receipt_keys.extend(
        output
            .auction
            .receipts
            .iter()
            .map(|receipt| receipt.local_key.clone()),
    );
    plan.applied_receipts
        .extend(output.auction.receipts.iter().cloned());
    plan.b2_finalizers
        .extend(output.auction.finalizer_executions.iter().cloned());
    plan.event_outbox
        .extend(output.auction.events.iter().cloned());
    Ok(output)
}

fn apply_session_b2_auction_transaction(
    mut candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
    preceding_receipts: &[super::EnvelopeReceipt],
) -> Result<B2AuctionTransactionOutput, B2AuctionTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    if !matches!(
        candidate.phase(),
        crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction
    ) {
        return Err(invariant("incremental B2 requires an auction phase").into());
    }
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
    let mut p4 = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&candidate)?,
    )?;

    let mut all_candidates = initial.candidates().to_vec();
    for round in apply_initial_candidate_stream(&mut p3, &mut p4, &initial)? {
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        chain.project_auction_execution_round(&mut candidate, &round)?;
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
        chain.advance_after_auction_outcomes(&mut candidate, &outcomes, round.as_ref())?;
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
    let finish = finish_incremental_auction_coordinator(&candidate, p4)?;
    let auction = apply_incremental_auction_finish_with_prepared_facts_and_receipts(
        &mut candidate,
        &candidates,
        &validation,
        finish,
        PreparedAuctionFinishContext {
            preceding_facts,
            next_session_local_index: &mut next_session_local_index,
            consumed: &plan_completion.consumed,
            preceding_receipts,
        },
    )?;

    Ok(B2AuctionTransactionOutput {
        #[cfg(test)]
        candidates,
        #[cfg(test)]
        validation,
        auction,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

#[cfg(test)]
pub(super) fn apply_tick_shadow_b2_auction_transaction_with_roots_for_test(
    plan: &mut TickShadowPlan,
    roots: PlanChainOperationBatch,
) -> Result<B2AuctionTransactionOutput, B2AuctionTransactionError> {
    apply_tick_shadow_b2_auction_transaction_inner(plan, Some(roots))
}

pub(super) fn apply_initial_candidate_stream(
    p3: &mut P3ValidatorDriver,
    p4: &mut IncrementalAuctionStockCoordinator,
    initial: &P2CandidateBatch,
) -> Result<Vec<AuctionExecutionRound>, StepFatal> {
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
    round: &AuctionExecutionRound,
) -> Result<(), StepFatal> {
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
            "auction P4 fact identities do not match accepted P3 operations",
        ));
    }
    let mut deltas = BTreeMap::<(super::P2CandidateKey, u64), BTreeMap<AccountId, i64>>::new();
    for delta in &round.open_order_deltas {
        let accounts = deltas
            .entry((delta.candidate_key.clone(), delta.sealed_index))
            .or_default();
        let value = accounts.entry(delta.account).or_default();
        *value = value
            .checked_add(i64::from(delta.delta))
            .ok_or_else(|| invariant("auction P4 open-order feedback delta overflow"))?;
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
            "auction P4 returned open-order feedback for an unknown P3 operation",
        ));
    }
    p3.apply_open_order_feedback_round(feedback)?;
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::b2_auction_transaction".to_owned(),
    }
}
