//! One production boundary for requests already eligible to enter the current tick.
//! Source identity is retained for receipts, never used as a trading priority.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    candidate_composition::compose_projected_candidates,
    local_admission::{admit_ready_batch, AccountReceipts},
    npc_tick_preparation::take_ready_npc_batch,
    stock_stream::StockStreamNotifications,
    AccountValidatorDriver, CandidateValidationOutcome, IntentCandidate, StepFatal,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::GameSession;

#[cfg(test)]
#[path = "ready_ingress_tests.rs"]
mod tests;

pub(super) struct ReadyIngress {
    initial: Vec<IntentCandidate>,
    chain: AdaptivePlanChainCoordinator,
    notifications: StockStreamNotifications,
    receipts: AccountReceipts,
}

/// 取出排队请求会修改 tick candidate；请求隔离后，
/// 根观察与 AccountValidation/StockProcessing 准备可并发读取同一报价过期后状态。
pub(super) struct ReadyIngressSources {
    initial: Vec<IntentCandidate>,
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
        let initial = compose_projected_candidates(npc, player)
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
    ) -> Result<Vec<IntentCandidate>, StepFatal> {
        self.chain.block_unfinished_routes(&self.initial);
        let mut ready = std::mem::take(&mut self.initial);
        ready.extend(self.chain.ready_batch_without_waiting_for_roots(session)?);
        stamp_ready_receipts(session, &mut ready)?;
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
    validator: &mut AccountValidatorDriver,
    mut ready: Vec<IntentCandidate>,
    all_candidates: &mut Vec<IntentCandidate>,
) -> Result<Vec<CandidateValidationOutcome>, StepFatal> {
    let mut outcomes = Vec::new();
    loop {
        if !ready.is_empty() {
            stamp_ready_receipts(session, &mut ready)?;
            let admitted = admit_ready_batch(ready, receipts)?;
            chain.block_unfinished_routes(&admitted);
            all_candidates.extend(admitted.iter().cloned());
            crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
            outcomes.extend(validator.consume_round(admitted)?);
        }
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        ready = chain.ready_batch_without_waiting_for_roots(session)?;
        if ready.is_empty() {
            return Ok(outcomes);
        }
    }
}

fn stamp_ready_receipts(
    session: &mut GameSession,
    candidates: &mut [IntentCandidate],
) -> Result<(), StepFatal> {
    for candidate in candidates {
        if candidate.ingress_order().is_some() {
            continue;
        }
        if !matches!(candidate.key(), super::IntentCandidateKey::PlanChain { .. }) {
            return Err(invariant("queued candidate is missing its receive ordinal"));
        }
        let received = session
            .receive_private_intent(candidate.owner(), candidate.intent().clone())
            .map_err(|error| invariant(&error.to_string()))?;
        candidate.set_ingress_order(received.account_ordinal, received.stock_ordinal);
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ready_ingress".to_owned(),
    }
}

#[cfg(test)]
mod shared_receipt_tests {
    use super::*;
    use crate::{AccountId, Intent, LimitPrice, Money, Side};

    #[test]
    fn ready_plan_stamping_shares_late_player_and_npc_receipt_source() {
        let mut session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let source = session.shared_ingress();
        let code = session.state.setup.stocks[0].code.clone();
        let intent = || Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(990)),
            qty: 100,
        };
        let mut shadow = session.clone_for_tick_shadow().unwrap();
        shadow.freeze_shared_ingress().unwrap();
        source
            .enqueue_player_intent(AccountId(0), intent())
            .unwrap();
        let mut plans = vec![IntentCandidate::new(
            super::super::IntentCandidateKey::plan_chain(0),
            AccountId(0),
            intent(),
        )];
        stamp_ready_receipts(&mut shadow, &mut plans).unwrap();
        assert_eq!(plans[0].ingress_order(), Some((1, 1)));
        let npc = shadow
            .receive_private_intent(AccountId(0), intent())
            .unwrap();
        assert_eq!((npc.account_ordinal, npc.stock_ordinal), (2, 2));
    }
}
