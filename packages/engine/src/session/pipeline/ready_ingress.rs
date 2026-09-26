//! One production boundary for requests already eligible to enter the current tick.
//! Source identity is retained for receipts, never used as a trading priority.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    local_admission::{admit_ready_batch, AccountReceipts},
    npc_p2_preparation::take_ready_npc_batch,
    p2_composition::compose_projected_p2_candidates,
    stock_stream::StockStreamNotifications,
    P2Candidate, P3ConsumeOutcome, P3ValidatorDriver, StepFatal,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::GameSession;

#[cfg(test)]
#[path = "ready_ingress_tests.rs"]
mod tests;

pub(super) struct ReadyIngress {
    initial: Vec<P2Candidate>,
    chain: AdaptivePlanChainCoordinator,
    notifications: StockStreamNotifications,
    receipts: AccountReceipts,
}

/// Taking queued requests changes the tick candidate. Once they are detached,
/// root observation and P3/P4 setup can read the same post-P0 state concurrently.
pub(super) struct ReadyIngressSources {
    initial: Vec<P2Candidate>,
    observed_accounts: Vec<crate::AccountId>,
}

impl ReadyIngress {
    pub(super) fn capture_sources(
        session: &mut GameSession,
    ) -> Result<ReadyIngressSources, StepFatal> {
        // The NPC batch was produced at the preceding commit. The player queue
        // was populated asynchronously between ticks. Plan roots are produced
        // from this tick's NPC observation; a source label cannot rank accounts.
        let (npc, observed_accounts) = take_ready_npc_batch(session)?;
        let player = session.capture_player_candidate_batch();
        let initial = compose_projected_p2_candidates(npc, player)
            .map_err(|error| invariant(&error.to_string()))?
            .into_candidates();
        Ok(ReadyIngressSources {
            initial,
            observed_accounts,
        })
    }

    pub(super) fn first_ready_batch(
        &mut self,
        session: &mut GameSession,
    ) -> Result<Vec<P2Candidate>, StepFatal> {
        self.chain.block_unfinished_routes(&self.initial);
        let mut ready = std::mem::take(&mut self.initial);
        ready.extend(self.chain.ready_batch_without_waiting_for_roots(session)?);
        Ok(ready)
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        AdaptivePlanChainCoordinator,
        StockStreamNotifications,
        AccountReceipts,
    ) {
        (self.chain, self.notifications, self.receipts)
    }
}

impl ReadyIngressSources {
    pub(super) fn capture_roots(
        self,
        session: &GameSession,
        roots_override: Option<PlanChainOperationBatch>,
    ) -> Result<ReadyIngress, StepFatal> {
        let mut roots = match roots_override {
            Some(roots) => roots,
            None => session.capture_ready_decision_chain_roots(&self.observed_accounts)?,
        };
        let notifications = StockStreamNotifications::new();
        roots.set_completion_sender(notifications.sender.clone())?;
        let chain = AdaptivePlanChainCoordinator::capture_batch(session, roots)?;
        Ok(ReadyIngress {
            initial: self.initial,
            chain,
            notifications,
            receipts: AccountReceipts::default(),
        })
    }
}

/// Keep admitting independent roots that become ready during account validation.
/// A route waiting for its own typed feedback remains blocked by the plan batch;
/// other accounts and stocks can join the same stock-processing wave.
pub(super) fn validate_available_ready(
    chain: &mut AdaptivePlanChainCoordinator,
    receipts: &mut AccountReceipts,
    session: &mut GameSession,
    p3: &mut P3ValidatorDriver,
    mut ready: Vec<P2Candidate>,
    all_candidates: &mut Vec<P2Candidate>,
) -> Result<Vec<P3ConsumeOutcome>, StepFatal> {
    let mut outcomes = Vec::new();
    loop {
        if !ready.is_empty() {
            let admitted = admit_ready_batch(ready, receipts)?;
            chain.block_unfinished_routes(&admitted);
            all_candidates.extend(admitted.iter().cloned());
            crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
            outcomes.extend(p3.consume_round(admitted)?);
        }
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        ready = chain.ready_batch_without_waiting_for_roots(session)?;
        if ready.is_empty() {
            return Ok(outcomes);
        }
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ready_ingress".to_owned(),
    }
}
