//! 每只股票返回 typed StockProcessing 结果后立即推进依赖计划路由。
//! AccountValidation 与计划状态留在 tick coordinator；workers 只拥有各自股票 shadows。

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    continuous_matching::ContinuousExecutionRound,
    continuous_tick_transaction::validate_execution_round,
    local_admission::AccountReceipts,
    ready_ingress::validate_available_ready,
    stock_auction::auction_day_end::AuctionExecutionRound,
    stock_stream::{operation_code, operation_owner, StockStreamProgress},
    AccountValidatorDriver, CandidateValidationOutcome, IntentCandidate, IntentCandidateKey,
    StepFatal, ValidatedOperation,
};
use crate::{GameSession, StockCode};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ReadyStockStream<'a> {
    chain: &'a mut AdaptivePlanChainCoordinator,
    receipts: &'a mut AccountReceipts,
    session: &'a mut GameSession,
    validator: &'a mut AccountValidatorDriver,
    candidates: &'a mut Vec<IntentCandidate>,
    pending: BTreeMap<IntentCandidateKey, CandidateValidationOutcome>,
}

impl<'a> ReadyStockStream<'a> {
    pub(super) fn new(
        chain: &'a mut AdaptivePlanChainCoordinator,
        receipts: &'a mut AccountReceipts,
        session: &'a mut GameSession,
        validator: &'a mut AccountValidatorDriver,
        candidates: &'a mut Vec<IntentCandidate>,
    ) -> Self {
        Self {
            chain,
            receipts,
            session,
            validator,
            candidates,
            pending: BTreeMap::new(),
        }
    }

    pub(super) fn initial(
        &mut self,
        ready: Vec<IntentCandidate>,
    ) -> Result<Vec<ValidatedOperation>, StepFatal> {
        self.consume_immediately_ready(ready)
    }

    pub(super) fn continuous_progress(
        &mut self,
        progress: StockStreamProgress<'_, ContinuousExecutionRound>,
    ) -> Result<Vec<ValidatedOperation>, StepFatal> {
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
    ) -> Result<Vec<ValidatedOperation>, StepFatal> {
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
        super::auction_tick_transaction::validate_execution_round(&outcomes, round)?;
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
        keys: impl Iterator<Item = &'b IntentCandidateKey>,
    ) -> Result<Vec<CandidateValidationOutcome>, StepFatal> {
        let mut outcomes = Vec::new();
        for key in keys {
            let outcome = self
                .pending
                .remove(key)
                .ok_or_else(|| invariant("StockProcessing 返回了非待处理候选"))?;
            let operation = outcome.operation().ok_or_else(|| {
                invariant("StockProcessing 返回了 AccountValidation 已拒绝的候选")
            })?;
            if operation_code(operation) != stock {
                return Err(invariant("StockProcessing completion 来自其他股票"));
            }
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    fn consume_immediately_ready(
        &mut self,
        mut ready: Vec<IntentCandidate>,
    ) -> Result<Vec<ValidatedOperation>, StepFatal> {
        let mut operations = Vec::new();
        loop {
            if ready.is_empty() {
                return Ok(operations);
            }
            let outcomes = validate_available_ready(
                self.chain,
                self.receipts,
                self.session,
                self.validator,
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
                        return Err(invariant("AccountValidation 返回了重复待处理候选"));
                    }
                    operations.push(operation);
                } else if matches!(
                    outcome.candidate_key(),
                    IntentCandidateKey::PlanChain { .. }
                ) {
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
                .filter_map(CandidateValidationOutcome::operation)
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
            return Err(invariant("StockProcessing 的已受理候选缺少 typed feedback"));
        }
        Ok(())
    }
}

fn plan_outcomes(outcomes: &[CandidateValidationOutcome]) -> Vec<CandidateValidationOutcome> {
    outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.candidate_key(),
                IntentCandidateKey::PlanChain { .. }
            )
        })
        .cloned()
        .collect()
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ready_stock_stream".to_owned(),
    }
}
