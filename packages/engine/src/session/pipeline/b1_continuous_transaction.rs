//! Joint B1 continuous-trading candidate orchestration.
//!
//! All source classes are captured into one prospective session, composed once in
//! `npc -> player -> plan_chain` order, and passed through P3-P7 exactly once. Authority remains
//! untouched; P8/P9 own the later commit decision.

use super::{
    npc_p2_p7_transaction::{prepare_npc_p2_source, NpcP2P7TransactionError, PreparedNpcP2Source},
    p2_composition::{compose_projected_p2_candidates, P2SourceCompositionError},
    p3_context::build_p3_validation_context,
    p3_p7_session_transaction::{apply_session_p3_p7_transaction, P3P7SessionTransactionError},
    p4_p7_session_transaction::P4P7SessionTransactionOutput,
    plan_chain_p2_p7_transaction::{
        finalize_plan_chain_candidate, prepare_plan_chain_candidate, PlanChainP2P7TransactionError,
    },
    DecisionResourceSnapshot, EnvelopeReceipt, P2CandidateBatch, P3ValidationOutput, StepFatal,
    TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainYieldDriver;
use crate::{Event, GameSession};

#[derive(Debug, thiserror::Error)]
pub(super) enum B1ContinuousTransactionError {
    #[error("B1 continuous candidate preparation failed: {0}")]
    Preparation(#[source] StepFatal),
    #[error("B1 NPC P2 source failed: {0}")]
    Npc(#[from] NpcP2P7TransactionError),
    #[error("B1 plan-chain source failed: {0}")]
    PlanChain(#[from] PlanChainP2P7TransactionError),
    #[error("B1 P2 source composition failed: {0}")]
    Composition(#[from] P2SourceCompositionError),
    #[error("B1 P3-P7 transaction failed: {0}")]
    P3P7(#[from] P3P7SessionTransactionError),
}

impl From<StepFatal> for B1ContinuousTransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

pub(super) struct B1ContinuousTransactionOutput {
    pub(super) candidates: P2CandidateBatch,
    pub(super) validation: P3ValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: super::p6_transaction::P6TransactionOutput,
    pub(super) plan_chain_driver: Option<PlanChainYieldDriver>,
}

/// Applies the complete B1 continuous source batch to the owned tick shadow.
///
/// A plan-chain driver is optional because many ticks have no routed continuation. When present,
/// exactly one command is yielded into this sealed batch; any later continuation belongs to a
/// subsequent prospective batch and cannot re-enter the already sealed P1 allocation.
pub(super) fn apply_tick_shadow_b1_continuous_transaction(
    plan: &mut TickShadowPlan,
    plan_chain_driver: Option<&PlanChainYieldDriver>,
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    let resources = plan.decision_resources.as_deref().cloned().ok_or_else(|| {
        B1ContinuousTransactionError::Preparation(invariant(
            "P1 decision resource snapshot is absent",
        ))
    })?;
    let output = plan.state.execute_typed(|prospective| {
        apply_session_b1_continuous_transaction(prospective, resources, plan_chain_driver)
    })?;
    plan.event_outbox.extend(output.events.iter().cloned());
    Ok(output)
}

fn apply_session_b1_continuous_transaction(
    prospective: &mut GameSession,
    resources: DecisionResourceSnapshot,
    plan_chain_driver: Option<&PlanChainYieldDriver>,
) -> Result<B1ContinuousTransactionOutput, B1ContinuousTransactionError> {
    let mut candidate = prospective.clone_for_tick_shadow()?;
    let PreparedNpcP2Source {
        projection: _,
        candidates: npc,
    } = prepare_npc_p2_source(&mut candidate, &resources)?;
    let player = candidate.capture_player_candidate_batch();
    let prepared_plan_chain = plan_chain_driver
        .map(prepare_plan_chain_candidate)
        .transpose()?;
    let plan_chain_candidates = prepared_plan_chain
        .as_ref()
        .map(|prepared| prepared.candidate())
        .into_iter();
    let candidates = compose_projected_p2_candidates(&npc, player, plan_chain_candidates)?;
    let context = build_p3_validation_context(&candidate)?;
    let validation = super::P2P3Handoff::new_with_context(
        candidates.clone(),
        resources,
        candidate.envelope_ledger.clone(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )?
    .validate()?;
    let P4P7SessionTransactionOutput {
        events,
        receipts,
        p6,
    } = apply_session_p3_p7_transaction(&mut candidate, &candidates, &validation)?;

    let plan_chain_driver = match prepared_plan_chain {
        Some(prepared) => {
            let plans = candidate.plans.clone();
            let finalized =
                finalize_plan_chain_candidate(prepared, &candidate, &plans, &validation, &events)?;
            let (mut continued_session, continued_plans) = finalized.continuation.into_parts();
            continued_session.plans = continued_plans;
            candidate.commit_tick_shadow(continued_session);
            Some(finalized.driver)
        }
        None => None,
    };

    prospective.commit_tick_shadow(candidate);
    Ok(B1ContinuousTransactionOutput {
        candidates,
        validation,
        events,
        receipts,
        p6,
        plan_chain_driver,
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::b1_continuous_transaction".to_owned(),
    }
}
