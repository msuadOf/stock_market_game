//! 使用 Escrow 的权威市场 tick pipeline。
//!
//! 每种交易阶段在隔离 shadow 上准备从过期处理到提交的完整 candidate。
//! 只有唯一交易阶段 dispatcher 可以安装公共运行时权威状态；
//! ledger、settlement 与事件检查成功后执行不会失败的 CommitTick 交换。

mod account_settlement;
mod account_validation;
mod account_validation_context;
mod account_validation_driver;
mod adaptive_plan_chain;
mod auction_tick_transaction;
mod authoritative_tick;
mod candidate_commit;
mod candidate_composition;
mod commit_evidence;
mod conservation;
mod continuous_lifecycle_projection;
mod continuous_matching;
mod continuous_matching_adapter;
mod continuous_projection_transaction;
mod continuous_tick_finalizer;
mod continuous_tick_transaction;
mod decision_resources;
mod decision_snapshot;
mod decision_snapshot_capture;
mod envelope;
mod event_collection;
mod event_key;
mod execution_fact_producers;
#[cfg(any(test, feature = "verification-harness"))]
mod executor_perturbation;
#[cfg(test)]
mod executor_perturbation_tests;
mod institutional_experience_projection;
mod intent_candidates;
mod ledger;
mod ledger_candidate;
mod ledger_conservation;
mod ledger_validation;
mod local_admission;
mod npc_decisions;
mod npc_state_projection;
mod npc_tick_preparation;
mod phase;
mod pre_open_transaction;
mod price_resolution;
pub(in crate::session) mod quote_expiry;
mod ready_ingress;
mod ready_stock_stream;
mod receipt_aggregation;
mod receipt_key;
mod retail_projection;
mod session_execution_transaction;
mod session_fact_producers;
mod settlement;
mod shadow;
mod stock_auction;
mod stock_auction_adapter;
mod stock_execution_transaction;
mod stock_stream;
pub(in crate::session) use stock_stream::TickWorkReady;
mod transaction_error;
pub mod transition;
pub use account_validation::{
    AccountValidationContext, AccountValidationOutput, CandidateValidationInput,
    CandidateValidationResult, EnvelopeDraft, PlaceKind, StockValidation, ValidatedOperation,
};
pub use account_validation_driver::{
    AccountValidationCheckpoint, AccountValidatorDriver, CandidateValidationOutcome,
};
pub(super) use authoritative_tick::execute_authoritative_tick;
pub(in crate::session) use authoritative_tick::AuthoritativeTickCommit;
pub use commit_evidence::{AuctionFinalizerExecution, CommitEnvelopeChain, TickCommitEvidence};
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
pub use intent_candidates::{
    CandidateSource, CandidateSourceLocalKey, IntentCandidate, IntentCandidateBatch,
    IntentCandidateError, IntentCandidateKey,
};
pub use ledger::{EnvelopeLedger, EnvelopeReceipt, ReceiptKind};
pub use phase::TickPhase;
pub use quote_expiry::{ExpiryOutput, ExpiryRelease};
pub use receipt_key::*;
pub(super) use retail_projection::RetailProjectionSeen;
pub use shadow::TickShadow;

use super::{Event, GameSession, StepFatal};

#[cfg(test)]
mod account_settlement_tests;
#[cfg(test)]
mod account_validation_context_tests;
#[cfg(test)]
mod account_validation_driver_tests;
#[cfg(test)]
mod account_validation_tests;
#[cfg(test)]
mod auction_tick_transaction_tests;
#[cfg(test)]
mod authoritative_tick_tests;
#[cfg(test)]
mod candidate_commit_tests;
#[cfg(test)]
mod candidate_composition_tests;
#[cfg(test)]
mod commit_evidence_tests;
#[cfg(test)]
mod continuous_matching_adapter_tests;
#[cfg(test)]
mod continuous_matching_tests;
#[cfg(test)]
mod continuous_projection_transaction_tests;
#[cfg(test)]
mod continuous_tick_finalizer_tests;
#[cfg(test)]
mod continuous_tick_transaction_tests;
#[cfg(test)]
mod continuous_trade_acceptance_tests;
#[cfg(test)]
mod decision_resources_tests;
#[cfg(test)]
mod decision_snapshot_capture_tests;
#[cfg(test)]
mod decision_snapshot_tests;
#[cfg(test)]
mod envelope_tests;
#[cfg(test)]
mod event_collection_tests;
#[cfg(test)]
mod execution_fact_producers_tests;
#[cfg(test)]
mod initial_candidate_round_tests;
#[cfg(test)]
mod intent_candidates_tests;
#[cfg(test)]
mod ledger_conservation_tests;
#[cfg(test)]
mod ledger_receipt_tests;
#[cfg(test)]
mod ledger_state_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod npc_decisions_tests;
#[cfg(test)]
mod npc_state_projection_tests;
#[cfg(test)]
mod npc_tick_preparation_tests;
#[cfg(test)]
mod pre_open_transaction_tests;
#[cfg(test)]
mod quote_expiry_checkpoint_tests;
#[cfg(test)]
mod quote_expiry_tests;
#[cfg(test)]
mod receipt_aggregation_tests;
#[cfg(test)]
mod receipt_key_tests;
#[cfg(test)]
mod retail_projection_persistence_tests;
#[cfg(test)]
mod retail_projection_tests;
#[cfg(test)]
mod session_execution_transaction_tests;
#[cfg(test)]
mod session_fact_producers_tests;
#[cfg(test)]
mod settlement_tests;
#[cfg(test)]
mod stock_auction_adapter_tests;
#[cfg(test)]
mod stock_auction_tests;
#[cfg(test)]
mod stock_execution_transaction_tests;
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
    auction_finalizers: Vec<AuctionFinalizerExecution>,
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
                description: "SealAllocationSnapshot 缺少决策资源快照".to_owned(),
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

/// 捕获隔离 tick candidate，应用报价过期并密封账户分配资源。
/// 后续步骤在交易阶段专属事务内运行，并在最后提交 tick。
pub fn plan_tick(input: PhaseInput<'_>) -> Result<TickShadowPlan, StepFatal> {
    input.session.require_healthy()?;
    let mut shadow = TickShadowPlan {
        state: TickShadow::capture(input.session)?,
        event_outbox: Vec::new(),
        event_keys: Vec::new(),
        receipt_keys: Vec::new(),
        applied_receipts: Vec::new(),
        auction_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
    };
    shadow.state.execute(|game| {
        game.state.last_retail_decisions.clear();
        game.state.last_retail_order_events.clear();
        Ok(())
    })?;
    crate::verification_evidence::enter_phase(TickPhase::ExpiryShadow);
    shadow.expiry = quote_expiry::plan_expiry(&mut shadow)?;
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
    let mut plan = plan_tick(PhaseInput { session: authority })
        .expect("计划根 tick 的 ExpiryShadow/SealAllocationSnapshot");
    match phase {
        TradingPhase::Continuous => {
            continuous_tick_transaction::apply_tick_shadow_continuous_transaction_with_roots_for_test(
                &mut plan,
                roots,
            )
            .expect("plan-root continuous transaction");
        }
        TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
            auction_tick_transaction::apply_tick_shadow_auction_transaction_with_roots_for_test(
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
    candidate_commit::prepare_tick_shadow_plan_commit(authority, plan)
        .expect("计划根 tick 的 CommitTick 准备")
        .commit()
        .tick
        .events
}

/// Events produced by a successfully committed tick.
pub struct TickCommitResult {
    pub events: Vec<Event>,
    pub(in crate::session) event_keys: Vec<EventStableKey>,
}
pub(in crate::session) use npc_tick_preparation::queue_npc_for_next_tick;
