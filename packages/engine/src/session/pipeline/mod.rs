//! Authoritative escrow-backed market-tick pipeline.
//!
//! Each trading phase prepares its complete P0-P9 candidate against an isolated shadow.  The
//! public runtime reaches authority only through the single phase dispatcher and an infallible
//! P9 swap after ledger, settlement and event checks have succeeded.

mod adaptive_plan_chain;
mod authoritative_tick;
mod b1_continuous_transaction;
mod b1_tick_finalizer;
mod b2_auction_transaction;
mod commit_evidence;
mod conservation;
mod continuous_lifecycle_projection;
mod decision_resources;
mod decision_snapshot;
mod decision_snapshot_capture;
mod envelope;
mod event_key;
#[cfg(any(test, feature = "verification-harness"))]
mod executor_perturbation;
#[cfg(test)]
mod executor_perturbation_tests;
mod ledger;
mod ledger_candidate;
mod ledger_conservation;
mod ledger_validation;
mod local_admission;
mod npc_p2_preparation;
mod npc_p2_projection;
mod npc_p2_source;
mod p0_expiry;
mod p2_candidates;
mod p2_composition;
mod p3_context;
mod p3_driver;
mod p3_validation;
mod p4_continuous;
mod p4_continuous_adapter;
mod p4_p5_p6_transaction;
mod p4_p7_session_transaction;
mod p5_receipts;
mod p6_transaction;
mod p7_continuous_transaction;
mod p7_events;
mod p7_p4_producers;
mod p7_producers;
mod p9_candidate_commit;
mod phase;
mod pre_open_transaction;
mod ready_ingress;
mod ready_stock_stream;
mod receipt_key;
mod retail_projection;
mod settlement;
mod shadow;
mod stock_auction;
mod stock_auction_adapter;
mod stock_stream;
pub(in crate::session) use stock_stream::TickWorkReady;
mod transaction_error;
pub mod transition;
pub(super) use authoritative_tick::execute_authoritative_tick;
pub(in crate::session) use authoritative_tick::AuthoritativeTickCommit;
pub use commit_evidence::{B2FinalizerExecution, CommitEnvelopeChain, TickCommitEvidence};
pub use conservation::{FeeComponents, ReceiptDelta, ResVec};
pub use decision_resources::DecisionResourceSnapshot;
pub use decision_snapshot::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
pub use envelope::{Envelope, EnvelopeAudit, EnvelopeOrigin};
pub use event_key::*;
#[cfg(any(test, feature = "verification-harness"))]
pub use executor_perturbation::{
    with_executor_perturbation, CanonicalMerge, ExecutorBoundary, ExecutorOrderRecord,
    ExecutorPermutation, ExecutorPerturbation,
};
pub use ledger::{EnvelopeLedger, EnvelopeReceipt, ReceiptKind};
pub use p0_expiry::{ExpiryOutput, ExpiryRelease};
pub use p2_candidates::{
    CandidateSource, CandidateSourceLocalKey, P2Candidate, P2CandidateBatch, P2CandidateError,
    P2CandidateKey,
};
pub use p3_driver::{P3ConsumeOutcome, P3DriverCheckpoint, P3ValidatorDriver};
pub use p3_validation::{
    EnvelopeDraft, P2P3Handoff, P3CandidateResult, P3PlaceKind, P3StockValidation,
    P3ValidatedOperation, P3ValidationContext, P3ValidationOutput,
};
pub use phase::TickPhase;
pub use receipt_key::*;
pub(super) use retail_projection::RetailProjectionSeen;
pub use shadow::TickShadow;

use super::{Event, GameSession, StepFatal};

#[cfg(test)]
mod authoritative_tick_tests;
#[cfg(test)]
mod b1_continuous_trade_acceptance_tests;
#[cfg(test)]
mod b1_continuous_transaction_tests;
#[cfg(test)]
mod b1_tick_finalizer_tests;
#[cfg(test)]
mod b2_auction_transaction_tests;
#[cfg(test)]
mod commit_evidence_tests;
#[cfg(test)]
mod decision_snapshot_capture_tests;
#[cfg(test)]
mod decision_snapshot_tests;
#[cfg(test)]
mod envelope_tests;
#[cfg(test)]
mod initial_candidate_round_tests;
#[cfg(test)]
mod ledger_conservation_tests;
#[cfg(test)]
mod ledger_receipt_tests;
#[cfg(test)]
mod ledger_state_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod npc_p2_preparation_tests;
#[cfg(test)]
mod npc_p2_projection_tests;
#[cfg(test)]
mod npc_p2_source_tests;
#[cfg(test)]
mod p0_expiry_checkpoint_tests;
#[cfg(test)]
mod p0_expiry_tests;
#[cfg(test)]
mod p1_allocation_tests;
#[cfg(test)]
mod p2_candidates_tests;
#[cfg(test)]
mod p2_composition_tests;
#[cfg(test)]
mod p3_context_tests;
#[cfg(test)]
mod p3_driver_tests;
#[cfg(test)]
mod p3_validation_tests;
#[cfg(test)]
mod p4_continuous_adapter_tests;
#[cfg(test)]
mod p4_continuous_tests;
#[cfg(test)]
mod p4_p5_p6_transaction_tests;
#[cfg(test)]
mod p4_p7_session_transaction_tests;
#[cfg(test)]
mod p5_receipts_tests;
#[cfg(test)]
mod p6_transaction_tests;
#[cfg(test)]
mod p7_continuous_transaction_tests;
#[cfg(test)]
mod p7_events_tests;
#[cfg(test)]
mod p7_p4_producers_tests;
#[cfg(test)]
mod p7_producers_tests;
#[cfg(test)]
mod p9_candidate_commit_tests;
#[cfg(test)]
mod pre_open_transaction_tests;
#[cfg(test)]
mod receipt_key_tests;
#[cfg(test)]
mod retail_projection_persistence_tests;
#[cfg(test)]
mod retail_projection_tests;
#[cfg(test)]
mod settlement_tests;
#[cfg(test)]
mod stock_auction_adapter_tests;
#[cfg(test)]
mod stock_auction_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transition_tests;
/// Read-only authoritative input; prospective data belongs exclusively to the plan.
pub struct PhaseInput<'a> {
    pub session: &'a GameSession,
}

/// Discardable local plan with an owned, authoritative prospective state.
pub struct TickShadowPlan {
    state: TickShadow,
    event_outbox: Vec<Event>,
    event_keys: Vec<EventStableKey>,
    receipt_keys: Vec<ReceiptLocalKey>,
    applied_receipts: Vec<EnvelopeReceipt>,
    b2_finalizers: Vec<B2FinalizerExecution>,
    expiry: ExpiryOutput,
    expiry_applied: bool,
    decision_resources: Option<DecisionResourceSnapshot>,
}

impl TickShadowPlan {
    #[cfg(test)]
    pub(super) fn expiry(&self) -> &ExpiryOutput {
        &self.expiry
    }

    #[cfg(test)]
    pub(super) fn envelope_ledger(&self) -> Result<EnvelopeLedger, StepFatal> {
        self.state.envelope_ledger()
    }

    #[cfg(test)]
    pub(super) fn decision_resources(&self) -> Result<&DecisionResourceSnapshot, StepFatal> {
        self.decision_resources
            .as_ref()
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: "P1 decision resource snapshot is absent".to_owned(),
                location: "TickShadowPlan::decision_resources".to_owned(),
            })
    }

    #[cfg(test)]
    pub(super) fn strategy_state(
        &self,
        account: crate::AccountId,
    ) -> Result<crate::strategy::StrategyState, StepFatal> {
        self.state.strategy_state(account)
    }
}

/// Capture an isolated tick candidate, apply P0 expiry, and seal P1 resources.
/// Later stages run in the phase-specific transaction before the P9 commit.
pub fn plan_tick(input: PhaseInput<'_>) -> Result<TickShadowPlan, StepFatal> {
    input.session.require_healthy()?;
    let mut shadow = TickShadowPlan {
        state: TickShadow::capture(input.session)?,
        event_outbox: Vec::new(),
        event_keys: Vec::new(),
        receipt_keys: Vec::new(),
        applied_receipts: Vec::new(),
        b2_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
    };
    shadow.state.execute(|game| {
        game.last_retail_decisions.clear();
        game.last_retail_order_events.clear();
        Ok(())
    })?;
    crate::verification_evidence::enter_phase(TickPhase::ExpiryShadow);
    shadow.expiry = p0_expiry::plan_expiry(&mut shadow)?;
    crate::verification_evidence::enter_phase(TickPhase::SealAllocationSnapshot);
    decision_resources::plan_allocation(&mut shadow)?;
    Ok(shadow)
}

#[cfg(test)]
pub(in crate::session) fn commit_injected_plan_roots_for_test(
    authority: &mut GameSession,
    roots: crate::session::plan_chain_candidates::PlanChainOperationBatch,
) -> Vec<Event> {
    use crate::TradingPhase;

    authority
        .hydrate_or_validate_envelope_ledger()
        .expect("plan-root fixture must have a valid starting ledger");
    let phase = authority.phase();
    let mut plan = plan_tick(PhaseInput { session: authority }).expect("plan-root tick P0/P1");
    match phase {
        TradingPhase::Continuous => {
            b1_continuous_transaction::apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(
                &mut plan,
                roots,
            )
            .expect("plan-root continuous transaction");
        }
        TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
            b2_auction_transaction::apply_tick_shadow_b2_auction_transaction_with_roots_for_test(
                &mut plan, roots,
            )
            .expect("plan-root auction transaction");
        }
        TradingPhase::PreOpen => {
            pre_open_transaction::apply_tick_shadow_pre_open_transaction_with_roots_for_test(
                &mut plan, roots,
            )
            .expect("plan-root pre-open transaction");
        }
    }
    p9_candidate_commit::prepare_tick_shadow_plan_commit(authority, plan)
        .expect("plan-root tick P9 preparation")
        .commit()
        .tick
        .events
}

/// Events produced by a successfully committed tick.
pub struct TickCommitResult {
    pub events: Vec<Event>,
    pub(in crate::session) event_keys: Vec<EventStableKey>,
}
pub(in crate::session) use npc_p2_preparation::queue_npc_for_next_tick;
