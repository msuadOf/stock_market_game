//! Joint incremental call-auction transaction.
//!
//! P3 and stock-owned P4 alternate at every continuation edge. The auction coordinator is
//! initialized once from post-P0 state; only after all commands have drained does its consuming
//! finish seam run AuctionTick, completion, DayEnd and P5-P7 once.

#[cfg(test)]
use super::P3ValidationOutput;
use super::{
    p3_context::build_p3_validation_context,
    p7_producers::adapt_p3_rejection_facts,
    p9_candidate_commit::{CandidateTickCommitResult, PreparedTickPlanCommit},
    plan_tick,
    ready_ingress::ReadyIngress,
    ready_stock_stream::ReadyStockStream,
    stock_auction::b2_auction_day_end::{
        apply_incremental_auction_finish_with_prepared_facts_and_receipts, auction_tail_boundaries,
        AuctionExecutionRound, B2AuctionDayEndError, B2AuctionDayEndOutput,
        IncrementalAuctionStockCoordinator, PreparedAuctionFinishContext,
    },
    stock_auction_adapter::prepare_incremental_auction_inputs,
    stock_stream::{
        auction_shards, detached_auction_shard, drive_stock_stream, finish_auction_shards,
    },
    P2CandidateBatch, P3ConsumeOutcome, P3ValidatorDriver, PhaseInput, StepFatal, TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::GameSession;

#[derive(Debug, thiserror::Error)]
pub(super) enum B2AuctionTransactionError {
    #[error("B2 incremental auction preparation failed: {0}")]
    Preparation(#[source] StepFatal),
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
#[cfg(test)]
pub(super) fn prepare_b2_auction_tick(
    authority: &mut GameSession,
) -> Result<PreparedB2AuctionTick<'_>, B2AuctionTransactionError> {
    prepare_b2_auction_tick_with_evidence(authority, true)
}

pub(super) fn prepare_b2_auction_tick_with_evidence(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<PreparedB2AuctionTick<'_>, B2AuctionTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_b2_auction_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = super::p9_candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        authority,
        plan,
        capture_commit_evidence,
    )?;
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
    plan.event_keys
        .extend(output.auction.event_keys.iter().cloned());
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
    let sources = ReadyIngress::capture_sources(&mut candidate)?;
    // Root observation and stock/account setup share post-P0 facts. No plan
    // action mutates the discardable candidate until both branches finish.
    let frozen_candidate: &GameSession = &candidate;
    let (ingress, detached) = rayon::join(
        || sources.capture_roots(frozen_candidate, roots_override),
        || -> Result<_, StepFatal> {
            let context = build_p3_validation_context(frozen_candidate)?;
            let stock_inputs = prepare_incremental_auction_inputs(frozen_candidate)?;
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
            let p4 = auction_shards(stock_inputs)?;
            Ok((p3, p4))
        },
    );
    let mut all_candidates = Vec::new();
    let (mut p3, p4) = prepared?;
    let (mut chain, notifications) = ingress.into_parts();
    let mut stream =
        ReadyStockStream::new(&mut chain, &mut candidate, &mut p3, &mut all_candidates);
    let initial = stream.initial(ready?)?;
    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let p4 = drive_stock_stream(
        p4,
        initial,
        notifications,
        detached_auction_shard,
        |progress| stream.auction_progress(progress),
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
    let (tick_after, finish_auction, finish_day) = auction_tail_boundaries(&candidate)?;
    let finish =
        finish_auction_shards(p4, tick_after, finish_auction, finish_day).map_err(|source| {
            B2AuctionDayEndError::Worker {
                code: crate::StockCode("<incremental>".to_owned()),
                source,
            }
        })?;
    let auction = apply_incremental_auction_finish_with_prepared_facts_and_receipts(
        &mut candidate,
        &candidates,
        &validation,
        finish,
        PreparedAuctionFinishContext {
            preceding_facts,
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
            validate_execution_round(&outcomes, &round)?;
            rounds.push(round);
        }
    }
    Ok(rounds)
}

pub(super) fn validate_execution_round(
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
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::b2_auction_transaction".to_owned(),
    }
}
