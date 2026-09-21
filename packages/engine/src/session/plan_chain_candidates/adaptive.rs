//! Root traversal and continuation share one tick-wide command sequence.

use super::*;
#[cfg(feature = "simulation-diagnostics")]
use crate::session::plan_execution::PlanCancelCause;
use crate::session::plan_execution::PlanRouteOutcome;

/// Only the account/market observation containers are borrowed while generating a decision.
/// Parent orders, PlanBook, information acquisition and attention remain private execution state.
pub(in crate::session) struct FrozenPlanChainObservation {
    session: GameSession,
}

impl FrozenPlanChainObservation {
    pub(in crate::session) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        Ok(Self {
            session: session.clone_for_tick_shadow()?,
        })
    }

    fn observe<T>(
        &mut self,
        execution: &mut GameSession,
        action: impl FnOnce(&mut GameSession) -> T,
    ) -> T {
        // This is not a new allocation snapshot. The same post-P0 containers are reused for
        // every root and quote. Restore them even when the action returns a typed failure.
        std::mem::swap(&mut execution.accounts, &mut self.session.accounts);
        std::mem::swap(&mut execution.markets, &mut self.session.markets);
        std::mem::swap(
            &mut execution.auction_orders,
            &mut self.session.auction_orders,
        );
        let result = action(execution);
        std::mem::swap(
            &mut execution.auction_orders,
            &mut self.session.auction_orders,
        );
        std::mem::swap(&mut execution.markets, &mut self.session.markets);
        std::mem::swap(&mut execution.accounts, &mut self.session.accounts);
        result
    }
}

impl PlanChainOperationBatch {
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn pending_cancel_cause(&self) -> Option<PlanCancelCause> {
        self.pending_route
            .as_ref()
            .and_then(|route| match route.command() {
                PlanRouteCommand::Cancel { cause, .. } => Some(*cause),
                PlanRouteCommand::SubmitLimit { .. } => None,
            })
    }

    /// Advances real account/lifecycle/quote roots until one command needs P3/P4 execution.
    /// It never routes an intent, settles an account, or reconstructs a decision snapshot.
    pub(in crate::session) fn yield_adaptive_candidate(
        &mut self,
        session: &mut GameSession,
        observation: &mut FrozenPlanChainObservation,
    ) -> Result<Option<PlanChainCandidateBatch>, StepFatal> {
        if self.pending_route.is_some() {
            return Err(invariant(
                "a plan-chain command is still awaiting its typed outcome",
            ));
        }
        loop {
            let Some(operation) = self.operations.pop_front() else {
                if matches!(self.source, AccountSource::Empty) {
                    return Ok(None);
                }
                observation.observe(session, |session| self.prepare_next_account(session));
                continue;
            };
            if let PlanChainOperation::ExecutionRoute(route) = operation {
                let candidate = self
                    .enumerate_candidate(route.command())
                    .map_err(execution)?;
                self.pending_route = Some(route);
                return Ok(Some(candidate));
            }
            let mut plans = std::mem::take(&mut session.plans);
            let progress = match operation {
                PlanChainOperation::QuotePlans(mut cursor) => {
                    session
                        .synchronize_plan_execution(&mut plans)
                        .map_err(execution)?;
                    let request = observation.observe(session, |session| {
                        session.generate_next_plan_quote(&mut cursor, &plans)
                    });
                    if let Some(request) = request {
                        self.operations
                            .push_front(PlanChainOperation::QuotePlans(cursor));
                        self.operations
                            .push_front(PlanChainOperation::Execute(request));
                    }
                    None
                }
                PlanChainOperation::Lifecycle {
                    account,
                    mut assessments,
                    market,
                } => {
                    if let Some((code, assessment)) = assessments.pop_first() {
                        let mut generated = PlanChainOperationBatch::empty();
                        observation.observe(session, |session| {
                            session.drive_plans_for_account(
                                account,
                                &BTreeMap::from([(code, assessment)]),
                                &market,
                                &mut plans,
                                &mut generated,
                            );
                        });
                        self.operations.push_front(PlanChainOperation::Lifecycle {
                            account,
                            assessments,
                            market,
                        });
                        while let Some(operation) = generated.operations.pop_back() {
                            self.operations.push_front(operation);
                        }
                    }
                    None
                }
                PlanChainOperation::Restructure {
                    plan_id,
                    child_order_id,
                    terminating,
                    revision,
                } => match child_order_id {
                    Some(order_id) => Some(PlanExecutionProgress::restructure(
                        plans
                            .plan(plan_id)
                            .map_err(|error| invariant(&error.to_string()))?,
                        order_id,
                        revision,
                        terminating,
                    )),
                    None => {
                        if terminating {
                            session.remove_linked_parent(plan_id);
                        }
                        plans
                            .apply(plan_id, PlanEvent::Revised { revision })
                            .map_err(|error| invariant(&error.to_string()))?;
                        None
                    }
                },
                PlanChainOperation::AccountExecution { account, market } => {
                    observation.observe(session, |session| {
                        session.execute_plans_for_account(account, &market, &mut plans, self);
                    });
                    None
                }
                // The execution adapter observes current working orders and parent facts,
                // while its quote/allocation request was made from the frozen observation.
                PlanChainOperation::Execute(request) => Some(
                    session
                        .prepare_plan_observation(&mut plans, request)
                        .map_err(execution)?,
                ),
                PlanChainOperation::ExecutionRoute(_) => unreachable!("route handled above"),
            };
            session.plans = plans;
            self.append_progress(progress);
        }
    }

    pub(in crate::session) fn resume_adaptive_candidate(
        &mut self,
        session: &mut GameSession,
        outcome: PlanRouteOutcome,
    ) -> Result<(), StepFatal> {
        let route = self
            .pending_route
            .take()
            .ok_or_else(|| invariant("typed plan outcome has no pending route"))?;
        let mut plans = std::mem::take(&mut session.plans);
        let progress = route.resume(session, &mut plans, outcome);
        session.plans = plans;
        self.append_progress(Some(progress.map_err(execution)?));
        Ok(())
    }

    fn append_progress(&mut self, progress: Option<PlanExecutionProgress>) {
        match progress {
            Some(PlanExecutionProgress::Complete(report)) => self.reports.push(report),
            Some(PlanExecutionProgress::Route(route)) => {
                self.operations
                    .push_front(PlanChainOperation::ExecutionRoute(route));
            }
            None => {}
        }
    }

    pub(in crate::session) fn finish_adaptive(self) -> Result<Vec<PlanExecutionReport>, StepFatal> {
        if self.pending_route.is_some()
            || !self.operations.is_empty()
            || !matches!(self.source, AccountSource::Empty)
        {
            return Err(invariant(
                "plan-chain roots or continuation remain undrained",
            ));
        }
        Ok(self.reports)
    }

    #[cfg(test)]
    pub(in crate::session) fn set_adaptive_generation_for_test(&mut self, value: u64) {
        self.candidate_source.next_generation_index = value;
    }
}

fn execution(error: PlanExecutionError) -> StepFatal {
    invariant(&error.to_string())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "plan_chain_candidates::adaptive".to_owned(),
    }
}

#[cfg(test)]
#[path = "adaptive_tests.rs"]
mod tests;
