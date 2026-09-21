//! Compatibility skeleton only: P0-P8 issue local tokens, not business results.
//! Todo 3-7 will migrate the mutating legacy body into phase-owned shadow outputs.
//! No rollback, ledger validation, decision snapshot or dual-hash proof is claimed here.

mod adaptive_plan_chain;
mod b1_continuous_transaction;
mod b1_tick_finalizer;
mod b2_auction_transaction;
mod commit_evidence;
mod conservation;
mod decision_resources;
mod decision_snapshot;
mod decision_snapshot_capture;
mod envelope;
mod event_key;
mod ledger;
mod ledger_candidate;
mod ledger_conservation;
mod ledger_validation;
mod npc_p2_p7_transaction;
mod npc_p2_projection;
mod npc_p2_source;
mod p0_expiry;
mod p1_allocation;
mod p2_candidates;
mod p2_composition;
mod p3_context;
mod p3_driver;
mod p3_p4_normalizer;
mod p3_p7_session_transaction;
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
mod plan_chain_p2_p7_transaction;
mod player_p2_p7_transaction;
mod pre_open_transaction;
// The formal session authority cutover consumes this deliberately narrow seam.
#[allow(unused_imports)]
pub(super) use pre_open_transaction::{
    prepare_pre_open_tick, PreOpenTickResult, PreOpenTransactionError, PreparedPreOpenTick,
};
mod receipt_key;
mod retail_projection;
mod settlement;
mod shadow;
mod stock_auction;
mod stock_auction_adapter;
mod transaction_error;
pub mod transition;
pub use commit_evidence::{B2FinalizerExecution, CommitEnvelopeChain, TickCommitEvidence};
pub use conservation::{FeeComponents, ReceiptDelta, ResVec};
pub use decision_resources::DecisionResourceSnapshot;
pub use decision_snapshot::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
pub use envelope::{Envelope, EnvelopeAudit, EnvelopeOrigin};
pub use event_key::*;
pub use ledger::{EnvelopeLedger, EnvelopeReceipt, ReceiptKind};
pub use p0_expiry::{ExpiryOutput, ExpiryRelease};
pub use p1_allocation::AllocationSnapshot;
pub use p2_candidates::{
    CandidateSource, CandidateSourceLocalKey, P2Candidate, P2CandidateBatch, P2CandidateError,
    P2CandidateKey,
};
pub use p3_driver::{P3ConsumeOutcome, P3DriverCheckpoint, P3ValidatorDriver};
pub use p3_validation::{
    EnvelopeDraft, P2P3Handoff, P3CandidateResult, P3OpenOrderLimits, P3PlaceKind,
    P3StockValidation, P3ValidatedOperation, P3ValidationContext, P3ValidationOutput,
};
pub use phase::TickPhase;
pub use receipt_key::*;
pub(super) use retail_projection::RetailProjectionSeen;
pub use shadow::TickShadow;

use super::{Event, GameSession, StepFatal};

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
mod initial_candidate_round_tests;
#[cfg(test)]
mod decision_snapshot_capture_tests;
#[cfg(test)]
mod decision_snapshot_tests;
#[cfg(test)]
mod envelope_tests;
#[cfg(test)]
mod ledger_conservation_tests;
#[cfg(test)]
mod ledger_receipt_tests;
#[cfg(test)]
mod ledger_state_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod npc_p2_p7_transaction_tests;
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
mod p3_p4_normalizer_tests;
#[cfg(test)]
mod p3_p7_session_transaction_tests;
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
mod plan_chain_p2_p7_transaction_tests;
#[cfg(test)]
mod player_p2_p7_transaction_tests;
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
#[cfg(test)]
thread_local! {
    static COMMIT_TRACES: std::cell::RefCell<Vec<Vec<TickPhase>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Read-only authoritative input; prospective data belongs exclusively to the plan.
pub struct PhaseInput<'a> {
    pub session: &'a GameSession,
}

/// A completed compatibility traversal token, not proof of migrated business work.
#[derive(Debug)]
pub struct PhaseOutput {
    phase: TickPhase,
}

/// Discardable local plan with an owned, authoritative prospective state.
pub struct TickShadowPlan {
    state: TickShadow,
    tokens: Vec<PhaseOutput>,
    event_outbox: Vec<Event>,
    receipt_keys: Vec<ReceiptLocalKey>,
    applied_receipts: Vec<EnvelopeReceipt>,
    b2_finalizers: Vec<B2FinalizerExecution>,
    expiry: ExpiryOutput,
    expiry_applied: bool,
    decision_resources: Option<std::sync::Arc<DecisionResourceSnapshot>>,
}

impl TickShadowPlan {
    pub fn trace(&self) -> Vec<TickPhase> {
        self.tokens.iter().map(|token| token.phase).collect()
    }

    #[cfg(test)]
    pub(super) fn expiry(&self) -> &ExpiryOutput {
        &self.expiry
    }

    #[cfg(test)]
    pub(super) fn envelope_ledger(&self) -> Result<EnvelopeLedger, StepFatal> {
        self.state.envelope_ledger()
    }

    #[cfg(test)]
    pub(super) fn allocation(&self) -> Result<&AllocationSnapshot, StepFatal> {
        self.decision_resources
            .as_ref()
            .map(|resources| resources.allocation())
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: "P1 allocation snapshot is absent".to_owned(),
                location: "TickShadowPlan::allocation".to_owned(),
            })
    }

    #[cfg(test)]
    pub(super) fn decision_resources(&self) -> Result<&DecisionResourceSnapshot, StepFatal> {
        self.decision_resources
            .as_deref()
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

/// P0 expiry; P1 immutable allocation seal; P2 shadow strategies; P3 validation;
/// P4 stock processing; P5 receipt validation; P6 settlement; P7 audit; P8 hashes.
/// These responsibilities are deferred: this function records compatibility tokens only.
pub fn plan_tick(input: PhaseInput<'_>) -> Result<TickShadowPlan, StepFatal> {
    input.session.require_healthy()?;
    let mut shadow = TickShadowPlan {
        state: TickShadow::capture(input.session)?,
        tokens: Vec::new(),
        event_outbox: Vec::new(),
        receipt_keys: Vec::new(),
        applied_receipts: Vec::new(),
        b2_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
    };
    if !shadow.state.is_non_authoritative_test_strategy() {
        shadow.state.execute(|game| {
            game.last_retail_decisions.clear();
            game.last_retail_order_events.clear();
            Ok(())
        })?;
    }
    let expired = p0_expiry::plan_expiry(&input, TickStart, &mut shadow)?;
    shadow.expiry = expired.clone();
    let sealed = p1_allocation::plan_allocation(&input, expired, &mut shadow)?;
    let decisions = plan_decisions(&input, sealed, &mut shadow)?;
    let validated = plan_accounts(&input, decisions, &mut shadow)?;
    let stocks = plan_stocks(&input, validated, &mut shadow)?;
    let receipts = plan_receipts(&input, stocks, &mut shadow)?;
    let settled = plan_settlement(&input, receipts, &mut shadow)?;
    let audited = plan_audit(&input, settled, &mut shadow)?;
    let _checked = plan_hashes(&input, audited, &mut shadow)?;
    Ok(shadow)
}

/// Builds the phase trace and isolated P9 bridge for the still-authoritative
/// legacy runtime without executing the private P0-P8 implementation.
///
/// The staged phases remain callable through [`plan_tick`] for their focused
/// contracts. Wiring them into public [`GameSession::step`] is the later
/// authority-cutover task; doing so here would both change legacy behavior and
/// rebuild the complete private escrow ledger on the legacy hot path.
pub(super) fn plan_legacy_compatibility_tick(
    input: PhaseInput<'_>,
) -> Result<TickShadowPlan, StepFatal> {
    input.session.require_healthy()?;
    let mut shadow = TickShadowPlan {
        state: TickShadow::capture(input.session)?,
        tokens: Vec::new(),
        event_outbox: Vec::new(),
        receipt_keys: Vec::new(),
        applied_receipts: Vec::new(),
        b2_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: true,
        decision_resources: None,
    };
    if !shadow.state.is_non_authoritative_test_strategy() {
        shadow.state.execute(|game| {
            game.last_retail_decisions.clear();
            game.last_retail_order_events.clear();
            Ok(())
        })?;
    }
    for phase in TickPhase::ALL.into_iter().take(9) {
        shadow.tokens.push(PhaseOutput { phase });
    }
    Ok(shadow)
}

pub(super) struct TickStart;
macro_rules! compatibility_phase {
    ($function:ident, $input:ident, $output:ident, $phase:ident) => {
        struct $output;
        fn $function(
            input: &PhaseInput<'_>,
            _previous: $input,
            shadow: &mut TickShadowPlan,
        ) -> Result<$output, StepFatal> {
            input.session.require_healthy()?;
            shadow.tokens.push(PhaseOutput {
                phase: TickPhase::$phase,
            });
            Ok($output)
        }
    };
}
struct AllocationOutput;
compatibility_phase!(
    plan_decisions,
    AllocationOutput,
    DecisionOutput,
    DecisionShadow
);
compatibility_phase!(
    plan_accounts,
    DecisionOutput,
    AccountOutput,
    AccountValidation
);
compatibility_phase!(plan_stocks, AccountOutput, StockOutput, StockProcessing);
compatibility_phase!(
    plan_receipts,
    StockOutput,
    ReceiptOutput,
    ReceiptAggregation
);
compatibility_phase!(
    plan_settlement,
    ReceiptOutput,
    SettlementOutput,
    SettlementShadow
);
compatibility_phase!(plan_audit, SettlementOutput, AuditOutput, DerivationAudit);
compatibility_phase!(plan_hashes, AuditOutput, HashOutput, DualHashCheck);

/// Actual traversal evidence, returned locally rather than persisted into session state.
pub struct TickCommitResult {
    pub events: Vec<Event>,
    pub trace: Vec<TickPhase>,
}

/// Sole new mutable authority seam. The compatibility body still performs all mutations
/// during P9; it is NOT phase-pure or recoverable after a legacy panic.
pub(super) fn commit_tick(
    session: &mut GameSession,
    mut shadow: TickShadowPlan,
) -> Result<TickCommitResult, StepFatal> {
    validate_receipt_keys(&shadow.receipt_keys)?;
    let skip_initial_npc_expiry = !shadow.expiry.releases.is_empty();
    let p0_retail_order_events = if shadow.state.is_non_authoritative_test_strategy() {
        Vec::new()
    } else {
        shadow
            .state
            .execute(|game| Ok(game.last_retail_order_events.clone()))?
    };
    shadow.tokens.push(PhaseOutput {
        phase: TickPhase::CommitTick,
    });
    #[cfg(test)]
    session.run_post_shadow_hook()?;
    #[cfg(test)]
    let events = match shadow
        .state
        .run_compatibility_bridge(skip_initial_npc_expiry)
    {
        Ok(events) => events,
        Err(_) if shadow.state.is_non_authoritative_test_strategy() => {
            session.step_current_behavior(false)
        }
        Err(error) => return Err(error),
    };
    #[cfg(not(test))]
    let events = shadow
        .state
        .run_compatibility_bridge(skip_initial_npc_expiry)?;
    shadow.event_outbox.extend(events);
    if !shadow.state.is_non_authoritative_test_strategy() {
        shadow.state.execute(|game| {
            let legacy_events = std::mem::take(&mut game.last_retail_order_events);
            game.last_retail_order_events = p0_retail_order_events;
            game.last_retail_order_events.extend(legacy_events);
            game.finish_p0_tick()?;
            Ok(())
        })?;
    }
    let trace = shadow.trace();
    if !shadow.state.is_non_authoritative_test_strategy() {
        shadow.state.commit_into(session)?;
    }
    #[cfg(test)]
    COMMIT_TRACES.with_borrow_mut(|traces| traces.push(trace.clone()));
    Ok(TickCommitResult {
        events: shadow.event_outbox,
        trace,
    })
}
