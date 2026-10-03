//! Root traversal assigns stable command identities; stock-local pending routes track real continuation dependencies.

use super::*;
use crate::session::decision_chain::PlanLifecycleAction;
#[cfg(feature = "simulation-diagnostics")]
use crate::session::plan_execution::PlanCancelCause;
use crate::session::plan_execution::PlanRouteOutcome;

/// Only the account/market observation containers are borrowed while generating a decision.
/// Parent orders, PlanBook, information acquisition and attention remain private execution state.
pub(in crate::session) struct FrozenPlanChainObservation {
    accounts: account_book::AccountBook,
    markets: BTreeMap<StockCode, Market>,
    auction_orders: BTreeMap<StockCode, Vec<AuctionOrderSnap>>,
}

impl FrozenPlanChainObservation {
    pub(in crate::session) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        let accounts = session.state.accounts.clone_for_shadow().map_err(|error| {
            StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "FrozenPlanChainObservation::capture".to_owned(),
            }
        })?;
        Ok(Self {
            accounts,
            markets: session.state.markets.clone(),
            auction_orders: session.state.auction_orders.clone(),
        })
    }

    fn observe<T>(
        &mut self,
        execution: &mut GameSession,
        action: impl FnOnce(&mut GameSession) -> T,
    ) -> T {
        // This is not a new allocation snapshot. The same post-P0 containers are reused for
        // every root and quote. Restore them even when the action returns a typed failure.
        std::mem::swap(&mut execution.state.accounts, &mut self.accounts);
        std::mem::swap(&mut execution.state.markets, &mut self.markets);
        std::mem::swap(
            &mut execution.state.auction_orders,
            &mut self.auction_orders,
        );
        let result = action(execution);
        std::mem::swap(
            &mut execution.state.auction_orders,
            &mut self.auction_orders,
        );
        std::mem::swap(&mut execution.state.markets, &mut self.markets);
        std::mem::swap(&mut execution.state.accounts, &mut self.accounts);
        result
    }
}

impl PlanChainOperationBatch {
    #[cfg(test)]
    pub(in crate::session) fn yield_adaptive_candidates(
        &mut self,
        session: &mut GameSession,
        observation: &mut FrozenPlanChainObservation,
    ) -> Result<Vec<PlanChainCandidateBatch>, StepFatal> {
        self.yield_adaptive_candidates_with_root_wait(session, observation, true, &BTreeSet::new())
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn pending_cancel_cause(
        &self,
        generation_index: u64,
    ) -> Option<PlanCancelCause> {
        self.routes.pending_cancel_cause(generation_index)
    }

    /// Drains independent roots until each account/stock has at most one command
    /// awaiting a typed result. Account-wide allocation still precedes quote
    /// generation; separate stocks can use those grants in the same ready batch.
    pub(in crate::session) fn yield_adaptive_candidates_with_root_wait(
        &mut self,
        session: &mut GameSession,
        observation: &mut FrozenPlanChainObservation,
        blocking_roots: bool,
        unfinished_routes: &BTreeSet<(AccountId, StockCode)>,
    ) -> Result<Vec<PlanChainCandidateBatch>, StepFatal> {
        let mut ready = Vec::new();
        let mut deferred = VecDeque::new();
        loop {
            let Some(operation) = self.operations.pop_front() else {
                if self.roots.is_empty() {
                    break;
                }
                // Once a route is ready, collect any other roots already complete
                // without waiting for a slow, unrelated account.
                let wait_for_root = blocking_roots && ready.is_empty();
                if !observation.observe(session, |session| {
                    self.prepare_ready_accounts(session, wait_for_root)
                })? {
                    break;
                }
                continue;
            };
            if self.operation_blocked(&operation, session, unfinished_routes)? {
                deferred.push_back(operation);
                continue;
            }
            if let PlanChainOperation::ExecutionRoute(route) = operation {
                let resource = route_resource(route.command());
                let candidate = self
                    .enumerate_candidate(route.command())
                    .map_err(execution)?;
                self.routes
                    .install_pending(resource, candidate.chain_generation_index, route);
                ready.push(candidate);
                continue;
            }
            let mut plans = std::mem::take(&mut session.state.plans);
            let progress = match operation {
                PlanChainOperation::QuotePlans(mut cursor) => {
                    session
                        .synchronize_owned_plan_execution(&mut plans)
                        .map_err(execution)?;
                    let request = observation.observe(session, |session| {
                        session.generate_next_plan_quote(&mut cursor, &plans, unfinished_routes)
                    });
                    if let Some(request) = request {
                        let plan = plans
                            .plan(request.plan_id)
                            .map_err(|error| invariant(&error.to_string()))?;
                        if plan.active_child_order_id().is_some()
                            || !session
                                .plan_working_orders(plan.account(), plan.code())
                                .is_empty()
                        {
                            self.routes.remember_retry(
                                (plan.account(), plan.code().clone()),
                                cursor.market.clone(),
                            );
                        }
                        self.operations
                            .push_front(PlanChainOperation::QuotePlans(cursor));
                        self.operations
                            .push_front(PlanChainOperation::Execute(request));
                    } else if !cursor.plans.is_empty() {
                        // All remaining plans wait for a stock that has earlier
                        // requests in flight. Other roots may still progress.
                        deferred.push_back(PlanChainOperation::QuotePlans(cursor));
                    }
                    None
                }
                PlanChainOperation::Lifecycle {
                    account,
                    assessments,
                    market,
                } => {
                    let (ready, pending): (BTreeMap<_, _>, BTreeMap<_, _>) =
                        assessments.into_iter().partition(|(code, _)| {
                            !self
                                .routes
                                .is_blocked(&(account, code.clone()), unfinished_routes)
                        });
                    if !ready.is_empty() {
                        let mut generated = PlanChainOperationBatch::empty();
                        let actions = observation.observe(session, |session| {
                            session.collect_plan_lifecycle_actions(account, &ready, &market, &plans)
                        });
                        for action in &actions {
                            if let PlanLifecycleAction::Restructure { code, .. }
                            | PlanLifecycleAction::Terminate { code, .. } = action
                            {
                                if let Some(assessment) = ready.get(code) {
                                    self.routes.remember_reconsideration(
                                        (account, code.clone()),
                                        assessment.clone(),
                                        market.clone(),
                                    );
                                }
                            }
                        }
                        crate::session::decision_chain::apply_plan_lifecycle_actions(
                            actions,
                            session,
                            &market,
                            &mut plans,
                            &mut generated,
                        );
                        if !pending.is_empty() {
                            self.operations.push_front(PlanChainOperation::Lifecycle {
                                account,
                                assessments: pending,
                                market,
                            });
                        }
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
                    event,
                } => match child_order_id {
                    Some(order_id) => Some(PlanExecutionProgress::restructure_event(
                        plans
                            .plan(plan_id)
                            .map_err(|error| invariant(&error.to_string()))?,
                        order_id,
                        event,
                        terminating,
                    )),
                    None => {
                        plans
                            .apply(plan_id, event)
                            .map_err(|error| invariant(&error.to_string()))?;
                        if terminating {
                            session.remove_linked_parent(plan_id);
                        }
                        None
                    }
                },
                PlanChainOperation::AccountExecution { account, market } => {
                    session
                        .synchronize_owned_plan_execution(&mut plans)
                        .map_err(execution)?;
                    let cursor = observation.observe(session, |session| {
                        session.prepare_plan_quotes_for_account(account, &market, &plans)
                    });
                    if let Some(cursor) = cursor {
                        #[cfg(feature = "simulation-diagnostics")]
                        if let Some(result) = &cursor.grants {
                            session.causal_record(
                                crate::diagnostics::causal::CausalFactKind::Budget {
                                    account,
                                    available_cents: result.available_cash.cents(),
                                    allocated_cents: result
                                        .grants
                                        .iter()
                                        .map(|grant| grant.allocated_cash.cents())
                                        .collect(),
                                },
                            );
                        }
                        self.push_quote_plans(cursor);
                    }
                    None
                }
                // The execution adapter observes current working orders and parent facts,
                // while its quote/allocation request was made from the frozen observation.
                PlanChainOperation::Execute(request) => {
                    session
                        .synchronize_owned_plan_execution(&mut plans)
                        .map_err(execution)?;
                    Some(
                        session
                            .prepare_plan_observation(&plans, request)
                            .map_err(execution)?,
                    )
                }
                PlanChainOperation::ExecutionRoute(_) => unreachable!("route handled above"),
            };
            let progress = progress
                .map(|progress| session.materialize_plan_progress(&mut plans, progress))
                .transpose()
                .map_err(execution)?;
            session.state.plans = plans;
            self.append_progress(progress);
        }
        self.operations = deferred;
        Ok(ready)
    }

    fn operation_blocked(
        &self,
        operation: &PlanChainOperation,
        session: &GameSession,
        unfinished_routes: &BTreeSet<(AccountId, StockCode)>,
    ) -> Result<bool, StepFatal> {
        Ok(match operation {
            PlanChainOperation::ExecutionRoute(route) => {
                let resource = route_resource(route.command());
                self.routes.is_blocked(&resource, unfinished_routes)
            }
            // The cursor owns grants already allocated across the account. Its next plan can
            // be quoted while another stock awaits P4; Execute checks the target stock below.
            PlanChainOperation::QuotePlans(_) => false,
            PlanChainOperation::Lifecycle {
                account,
                assessments,
                ..
            } => {
                !assessments.is_empty()
                    && assessments.keys().all(|code| {
                        self.routes
                            .is_blocked(&(*account, code.clone()), unfinished_routes)
                    })
            }
            PlanChainOperation::Restructure { plan_id, .. }
            | PlanChainOperation::Execute(PlanExecutionRequest { plan_id, .. }) => {
                let plan = session
                    .state
                    .plans
                    .plan(*plan_id)
                    .map_err(|error| invariant(&error.to_string()))?;
                self.routes
                    .is_blocked(&(plan.account(), plan.code().clone()), unfinished_routes)
            }
            // This operation computes shared account grants and discovers all active plans.
            // It must see prior lifecycle results before constructing the quote cursor.
            PlanChainOperation::AccountExecution { account, .. } => {
                self.routes.pending_for_account(*account)
                    || session
                        .state
                        .plans
                        .active_plan_ids_for_account(*account)
                        .into_iter()
                        .filter_map(|plan_id| session.state.plans.plan(plan_id).ok())
                        .any(|plan| unfinished_routes.contains(&(*account, plan.code().clone())))
            }
        })
    }

    pub(in crate::session) fn install_accepted_submit_parents(
        &self,
        session: &mut GameSession,
        outcomes: &[(AccountId, StockCode, u64, PlanRouteOutcome)],
    ) -> Result<(), StepFatal> {
        for (account, code, generation_index, outcome) in outcomes {
            let route = self
                .routes
                .pending_for_outcome(&(*account, code.clone()), *generation_index)?;
            route
                .install_accepted_submit_parent(session, outcome)
                .map_err(execution)?;
        }
        Ok(())
    }

    pub(in crate::session) fn resume_adaptive_candidates(
        &mut self,
        session: &mut GameSession,
        outcomes: impl IntoIterator<Item = (AccountId, StockCode, u64, PlanRouteOutcome)>,
    ) -> Result<(), StepFatal> {
        let mut followups = Vec::new();
        for (account, code, generation_index, outcome) in outcomes {
            let route = self
                .routes
                .take_pending(&(account, code.clone()), generation_index)?;
            let mut plans = std::mem::take(&mut session.state.plans);
            let progress = route
                .resume(session, &mut plans, outcome)
                .and_then(|progress| session.materialize_plan_progress(&mut plans, progress));
            session.state.plans = plans;
            match progress.map_err(execution)? {
                PlanExecutionProgress::Complete(report) => {
                    let needs_reconsideration = matches!(
                        report.disposition,
                        crate::session::PlanExecutionDisposition::Waiting {
                            reason: crate::plans::QuoteReason::PendingReconsideration
                        } | crate::session::PlanExecutionDisposition::RouteRejected {
                            reason: RejectionReason::OrderAlreadyFilled
                        }
                    );
                    if needs_reconsideration
                        && session.state.plans.active_plan(account, &code).is_some()
                    {
                        if let Some(followup) = self.routes.take_follow_up((account, code)) {
                            followups.push(followup);
                        }
                    }
                    self.reports.push(report);
                }
                PlanExecutionProgress::Route(route) => {
                    followups.push(PlanChainOperation::ExecutionRoute(route));
                }
                PlanExecutionProgress::Adoption { .. } => {
                    return Err(invariant("unmaterialized plan execution progress"));
                }
            }
        }
        // Push in reverse so newly enabled continuations preserve the order
        // of the commands whose outcomes made them ready.
        for operation in followups.into_iter().rev() {
            self.operations.push_front(operation);
        }
        Ok(())
    }

    fn append_progress(&mut self, progress: Option<PlanExecutionProgress>) {
        match progress {
            Some(PlanExecutionProgress::Complete(report)) => self.reports.push(report),
            Some(PlanExecutionProgress::Route(route)) => {
                self.operations
                    .push_front(PlanChainOperation::ExecutionRoute(route));
            }
            Some(PlanExecutionProgress::Adoption { .. }) => {
                unreachable!("plan execution progress must be materialized before queuing")
            }
            None => {}
        }
    }

    pub(in crate::session) fn finish_adaptive(self) -> Result<Vec<PlanExecutionReport>, StepFatal> {
        if self.routes.has_pending() || !self.operations.is_empty() || !self.roots.is_empty() {
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

fn route_resource(command: &PlanRouteCommand) -> (AccountId, StockCode) {
    match command {
        PlanRouteCommand::Cancel { account, code, .. }
        | PlanRouteCommand::SubmitLimit { account, code, .. } => (*account, code.clone()),
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
