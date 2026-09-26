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
        let accounts =
            session
                .accounts
                .clone_for_shadow()
                .map_err(|error| StepFatal::InvariantViolation {
                    description: error.to_string(),
                    location: "FrozenPlanChainObservation::capture".to_owned(),
                })?;
        Ok(Self {
            accounts,
            markets: session.markets.clone(),
            auction_orders: session.auction_orders.clone(),
        })
    }

    fn observe<T>(
        &mut self,
        execution: &mut GameSession,
        action: impl FnOnce(&mut GameSession) -> T,
    ) -> T {
        // This is not a new allocation snapshot. The same post-P0 containers are reused for
        // every root and quote. Restore them even when the action returns a typed failure.
        std::mem::swap(&mut execution.accounts, &mut self.accounts);
        std::mem::swap(&mut execution.markets, &mut self.markets);
        std::mem::swap(&mut execution.auction_orders, &mut self.auction_orders);
        let result = action(execution);
        std::mem::swap(&mut execution.auction_orders, &mut self.auction_orders);
        std::mem::swap(&mut execution.markets, &mut self.markets);
        std::mem::swap(&mut execution.accounts, &mut self.accounts);
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
        self.pending_routes
            .values()
            .find(|(index, _)| *index == generation_index)
            .and_then(|(_, route)| match route.command() {
                PlanRouteCommand::Cancel { cause, .. } => Some(*cause),
                PlanRouteCommand::SubmitLimit { .. } => None,
            })
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
                if matches!(self.source, AccountSource::Empty) {
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
                self.pending_routes
                    .insert(resource, (candidate.chain_generation_index, route));
                ready.push(candidate);
                continue;
            }
            let mut plans = std::mem::take(&mut session.plans);
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
                        if plan.active_child_order_id.is_some()
                            || !session
                                .plan_working_orders(plan.account, &plan.code)
                                .is_empty()
                        {
                            self.retry_market
                                .insert((plan.account, plan.code.clone()), cursor.market.clone());
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
                            !self.pending_routes.contains_key(&(account, code.clone()))
                                && !unfinished_routes.contains(&(account, code.clone()))
                        });
                    if !ready.is_empty() {
                        let mut generated = PlanChainOperationBatch::empty();
                        let actions = observation.observe(session, |session| {
                            session.collect_plan_lifecycle_actions(account, &ready, &market, &plans)
                        });
                        for action in &actions {
                            if let PlanLifecycleAction::Restructure { code, .. } = action {
                                if let Some(assessment) = ready.get(code) {
                                    self.reconsideration.insert(
                                        (account, code.clone()),
                                        (assessment.clone(), market.clone()),
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
            session.plans = plans;
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
        let pending = &self.pending_routes;
        Ok(match operation {
            PlanChainOperation::ExecutionRoute(route) => {
                let resource = route_resource(route.command());
                pending.contains_key(&resource) || unfinished_routes.contains(&resource)
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
                        pending.contains_key(&(*account, code.clone()))
                            || unfinished_routes.contains(&(*account, code.clone()))
                    })
            }
            PlanChainOperation::Restructure { plan_id, .. }
            | PlanChainOperation::Execute(PlanExecutionRequest { plan_id, .. }) => {
                let plan = session
                    .plans
                    .plan(*plan_id)
                    .map_err(|error| invariant(&error.to_string()))?;
                pending.contains_key(&(plan.account, plan.code.clone()))
                    || unfinished_routes.contains(&(plan.account, plan.code.clone()))
            }
            // This operation computes shared account grants and discovers all active plans.
            // It must see prior lifecycle results before constructing the quote cursor.
            PlanChainOperation::AccountExecution { account, .. } => {
                pending.keys().any(|(owner, _)| owner == account)
                    || session
                        .plans
                        .active_plan_ids_for_account(*account)
                        .into_iter()
                        .filter_map(|plan_id| session.plans.plan(plan_id).ok())
                        .any(|plan| unfinished_routes.contains(&(*account, plan.code.clone())))
            }
        })
    }

    pub(in crate::session) fn install_accepted_submit_parents(
        &self,
        session: &mut GameSession,
        outcomes: &[(AccountId, StockCode, u64, PlanRouteOutcome)],
    ) -> Result<(), StepFatal> {
        for (account, code, generation_index, outcome) in outcomes {
            let (pending_index, route) = self
                .pending_routes
                .get(&(*account, code.clone()))
                .ok_or_else(|| invariant("typed plan outcome has no pending stock route"))?;
            if pending_index != generation_index {
                return Err(invariant("typed plan outcome names a different command"));
            }
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
            let (pending_index, route) = self
                .pending_routes
                .remove(&(account, code.clone()))
                .ok_or_else(|| invariant("typed plan outcome has no pending stock route"))?;
            if pending_index != generation_index {
                return Err(invariant("typed plan outcome names a different command"));
            }
            let mut plans = std::mem::take(&mut session.plans);
            let progress = route
                .resume(session, &mut plans, outcome)
                .and_then(|progress| session.materialize_plan_progress(&mut plans, progress));
            session.plans = plans;
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
                    if needs_reconsideration && session.plans.active_plan(account, &code).is_some()
                    {
                        if let Some((assessment, market)) =
                            self.reconsideration.remove(&(account, code.clone()))
                        {
                            followups.push(PlanChainOperation::Lifecycle {
                                account,
                                assessments: BTreeMap::from([(code, assessment)]),
                                market,
                            });
                        } else if let Some(market) = self.retry_market.remove(&(account, code)) {
                            followups
                                .push(PlanChainOperation::AccountExecution { account, market });
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
        if !self.pending_routes.is_empty()
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
