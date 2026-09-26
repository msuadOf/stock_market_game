//! Advance plan routes as each stock returns its typed P4 result. P3 and plan
//! state stay on the tick coordinator; workers only own their stock shadows.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    b1_continuous_transaction::validate_execution_round,
    p4_continuous::ContinuousExecutionRound,
    ready_ingress::validate_available_ready,
    stock_auction::b2_auction_day_end::AuctionExecutionRound,
    stock_stream::{operation_code, operation_owner, StockStreamProgress},
    P2Candidate, P2CandidateKey, P3ConsumeOutcome, P3ValidatedOperation, P3ValidatorDriver,
    StepFatal,
};
use crate::{GameSession, StockCode};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ReadyStockStream<'a> {
    chain: &'a mut AdaptivePlanChainCoordinator,
    session: &'a mut GameSession,
    p3: &'a mut P3ValidatorDriver,
    candidates: &'a mut Vec<P2Candidate>,
    pending: BTreeMap<P2CandidateKey, P3ConsumeOutcome>,
}

impl<'a> ReadyStockStream<'a> {
    pub(super) fn new(
        chain: &'a mut AdaptivePlanChainCoordinator,
        session: &'a mut GameSession,
        p3: &'a mut P3ValidatorDriver,
        candidates: &'a mut Vec<P2Candidate>,
    ) -> Self {
        Self {
            chain,
            session,
            p3,
            candidates,
            pending: BTreeMap::new(),
        }
    }

    pub(super) fn initial(
        &mut self,
        ready: Vec<P2Candidate>,
    ) -> Result<Vec<P3ValidatedOperation>, StepFatal> {
        self.consume_immediately_ready(ready)
    }

    pub(super) fn continuous_progress(
        &mut self,
        progress: StockStreamProgress<'_, ContinuousExecutionRound>,
    ) -> Result<Vec<P3ValidatedOperation>, StepFatal> {
        let (stock, round) = match progress {
            StockStreamProgress::Idle => {
                let ready = self.chain.next_ready_batch(self.session)?;
                return self.consume_immediately_ready(ready);
            }
            StockStreamProgress::PlanRootReady => {
                let ready = self
                    .chain
                    .ready_batch_without_waiting_for_roots(self.session)?;
                return self.consume_immediately_ready(ready);
            }
            StockStreamProgress::StockCompleted { code, round } => (code, round),
        };
        let outcomes =
            self.take_completed(stock, round.facts.iter().map(|fact| fact.candidate_key()))?;
        validate_execution_round(&outcomes, round)?;
        let plans = plan_outcomes(&outcomes);
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        if plans.is_empty() {
            self.chain.project_execution_round(self.session, round)?;
        } else {
            self.chain
                .advance_after_typed_outcomes(self.session, &plans, Some(round))?;
        }
        self.refresh_unfinished_routes();
        let ready = self
            .chain
            .ready_batch_without_waiting_for_roots(self.session)?;
        self.consume_immediately_ready(ready)
    }

    pub(super) fn auction_progress(
        &mut self,
        progress: StockStreamProgress<'_, AuctionExecutionRound>,
    ) -> Result<Vec<P3ValidatedOperation>, StepFatal> {
        let (stock, round) = match progress {
            StockStreamProgress::Idle => {
                let ready = self.chain.next_ready_batch(self.session)?;
                return self.consume_immediately_ready(ready);
            }
            StockStreamProgress::PlanRootReady => {
                let ready = self
                    .chain
                    .ready_batch_without_waiting_for_roots(self.session)?;
                return self.consume_immediately_ready(ready);
            }
            StockStreamProgress::StockCompleted { code, round } => (code, round),
        };
        let outcomes =
            self.take_completed(stock, round.facts.iter().map(|fact| &fact.candidate_key))?;
        super::b2_auction_transaction::validate_execution_round(&outcomes, round)?;
        let plans = plan_outcomes(&outcomes);
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        if plans.is_empty() {
            self.chain
                .project_auction_execution_round(self.session, round)?;
        } else {
            self.chain
                .advance_after_auction_outcomes(self.session, &plans, Some(round))?;
        }
        self.refresh_unfinished_routes();
        let ready = self
            .chain
            .ready_batch_without_waiting_for_roots(self.session)?;
        self.consume_immediately_ready(ready)
    }

    fn take_completed<'b>(
        &mut self,
        stock: &StockCode,
        keys: impl Iterator<Item = &'b P2CandidateKey>,
    ) -> Result<Vec<P3ConsumeOutcome>, StepFatal> {
        let mut outcomes = Vec::new();
        for key in keys {
            let outcome = self
                .pending
                .remove(key)
                .ok_or_else(|| invariant("P4 returned an unpending candidate"))?;
            let operation = outcome
                .operation()
                .ok_or_else(|| invariant("P4 returned a P3-rejected candidate"))?;
            if operation_code(operation) != stock {
                return Err(invariant("P4 completion came from a different stock"));
            }
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    fn consume_immediately_ready(
        &mut self,
        mut ready: Vec<P2Candidate>,
    ) -> Result<Vec<P3ValidatedOperation>, StepFatal> {
        let mut operations = Vec::new();
        loop {
            if ready.is_empty() {
                return Ok(operations);
            }
            let outcomes = validate_available_ready(
                self.chain,
                self.session,
                self.p3,
                ready,
                self.candidates,
            )?;
            let mut rejected_plans = Vec::new();
            for outcome in outcomes {
                if let Some(operation) = outcome.operation().cloned() {
                    if self
                        .pending
                        .insert(outcome.candidate_key().clone(), outcome)
                        .is_some()
                    {
                        return Err(invariant("P3 returned a duplicate pending candidate"));
                    }
                    operations.push(operation);
                } else if matches!(outcome.candidate_key(), P2CandidateKey::PlanChain { .. }) {
                    rejected_plans.push(outcome);
                }
            }
            if !rejected_plans.is_empty() {
                self.chain
                    .advance_after_typed_outcomes(self.session, &rejected_plans, None)?;
            }
            self.refresh_unfinished_routes();
            ready = self
                .chain
                .ready_batch_without_waiting_for_roots(self.session)?;
        }
    }

    fn refresh_unfinished_routes(&mut self) {
        self.chain.replace_unfinished_routes(
            self.pending
                .values()
                .filter_map(P3ConsumeOutcome::operation)
                .map(|operation| {
                    (
                        operation_owner(operation),
                        operation_code(operation).clone(),
                    )
                })
                .collect::<BTreeSet<_>>(),
        );
    }

    pub(super) fn finish(self) -> Result<(), StepFatal> {
        if !self.pending.is_empty() {
            return Err(invariant(
                "P4 left accepted candidates without typed feedback",
            ));
        }
        Ok(())
    }
}

fn plan_outcomes(outcomes: &[P3ConsumeOutcome]) -> Vec<P3ConsumeOutcome> {
    outcomes
        .iter()
        .filter(|outcome| matches!(outcome.candidate_key(), P2CandidateKey::PlanChain { .. }))
        .cloned()
        .collect()
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ready_stock_stream".to_owned(),
    }
}
