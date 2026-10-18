//! Root traversal assigns stable command identities; stock-local pending routes track real continuation dependencies.

use super::*;
use crate::session::decision_chain::PlanLifecycleAction;
use crate::session::plan_continuation_domain::PlanContinuationDomain;
#[cfg(feature = "simulation-diagnostics")]
use crate::session::plan_execution::PlanCancelCause;
use crate::session::plan_execution::PlanRouteOutcome;

/// Only the account/market observation containers are borrowed while generating a decision.
/// Parent orders, PlanBook, information acquisition and attention remain private execution state.
pub(in crate::session) struct FrozenPlanChainObservation {
    accounts: account_book::AccountBook,
    markets: BTreeMap<StockId, Market>,
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
        })
    }

    /// 增量一百（ADR-0042 §10）：冻结容器交换只经
    /// `PlanContinuationDomain::observe_frozen` 驱动——观察动作收到的是
    /// 窄写域视图，continuation 代码不再接触 `&mut GameSession`。
    /// 每个根与报价重用同一组报价过期后容器，不创建新的 allocation
    /// snapshot；即使动作返回 typed 失败，也须归还这些容器（对称双交换
    /// 由 `observe_frozen` 保证）。
    pub(in crate::session) fn swap_session_containers(&mut self, session: &mut GameSession) {
        std::mem::swap(&mut session.state.accounts, &mut self.accounts);
        std::mem::swap(&mut session.state.markets, &mut self.markets);
    }

    /// 测试侧冻结容器检视（cfg(test)）：测试直接观察交换视图（账户现金/
    /// 冻结订单簿等）；生产 continuation 一律经
    /// `PlanContinuationDomain::observe_frozen`（增量一百收窄）。
    #[cfg(test)]
    pub(in crate::session) fn observe<T>(
        &mut self,
        session: &mut GameSession,
        action: impl FnOnce(&mut GameSession) -> T,
    ) -> T {
        self.swap_session_containers(session);
        let result = action(session);
        self.swap_session_containers(session);
        result
    }
}

impl PlanChainOperationBatch {
    #[cfg(test)]
    pub(in crate::session) fn yield_adaptive_candidates(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        observation: &mut FrozenPlanChainObservation,
    ) -> Result<Vec<PlanChainCandidateBatch>, StepFatal> {
        self.yield_adaptive_candidates_after_capture_drain(domain, observation, &BTreeSet::new())
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
    /// Sol 终审·冗余收口：执行期 root-wait 穿透删除——生产 root 全部在
    /// capture 期 `drain_ready_accounts_at_capture` 闭环（含容量等待），
    /// 本方法只消费已入队 operations（continuation 不重建 root）。root
    /// 决策期的容量等待保留在 drain 内部；`AdaptiveYield::PendingCapacity`/
    /// `blocking_roots` 穿透链（NextReady→FeedOutcome→drive 循环容量等待）
    /// 已失去生产依赖，一并退役。yield 时 roots 非空=capture 未排空=显式
    /// invariant 错误（防御：不再静默回落到执行期懒计算）。
    pub(in crate::session) fn yield_adaptive_candidates_after_capture_drain(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        observation: &mut FrozenPlanChainObservation,
        unfinished_routes: &BTreeSet<(AccountId, StockId)>,
    ) -> Result<Vec<PlanChainCandidateBatch>, StepFatal> {
        let mut ready = Vec::new();
        let mut deferred = VecDeque::new();
        loop {
            let Some(operation) = self.operations.pop_front() else {
                if !self.roots.is_empty() {
                    return Err(route_invariant(
                        "plan-chain roots exist at execution yield (must be drained at capture)",
                    ));
                }
                break;
            };
            if self.operation_blocked(&operation, domain, unfinished_routes)? {
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
            #[cfg(feature = "simulation-diagnostics")]
            let trace_account = match &operation {
                PlanChainOperation::Lifecycle { account, .. }
                | PlanChainOperation::AccountExecution { account, .. } => *account,
                PlanChainOperation::QuotePlans(cursor) => cursor.account,
                PlanChainOperation::Restructure { plan_id, .. }
                | PlanChainOperation::Execute(PlanExecutionRequest { plan_id, .. }) => domain
                    .plans()
                    .plan(*plan_id)
                    .expect("diagnostic operation plan exists")
                    .account(),
                PlanChainOperation::ExecutionRoute(_) => unreachable!("route handled above"),
            };
            #[cfg(feature = "simulation-diagnostics")]
            let trace_before =
                super::super::decision_chain::plan_trace_states(trace_account, domain.plans());
            let progress = match operation {
                PlanChainOperation::QuotePlans(mut cursor) => {
                    let request = domain.observe_frozen(observation, |domain| {
                        domain.generate_next_plan_quote(&mut cursor, unfinished_routes)
                    });
                    if let Some(request) = request {
                        let plan = domain
                            .plans()
                            .plan(request.plan_id)
                            .map_err(|error| invariant(&error.to_string()))?;
                        if plan.active_child_order_id().is_some()
                            || !domain
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
                        let actions = domain.observe_frozen(observation, |domain| {
                            domain.collect_plan_lifecycle_actions(account, &ready, &market)
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
                        domain.apply_plan_lifecycle_actions(actions, &market, &mut generated);
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
                        domain
                            .plans()
                            .plan(plan_id)
                            .map_err(|error| invariant(&error.to_string()))?,
                        order_id,
                        event,
                        terminating,
                    )),
                    None => {
                        domain
                            .plans_mut()
                            .apply(plan_id, event)
                            .map_err(|error| invariant(&error.to_string()))?;
                        None
                    }
                },
                PlanChainOperation::AccountExecution { account, market } => {
                    domain.prune_plan_personal_memory(account);
                    let cursor = domain.observe_frozen(observation, |domain| {
                        domain.prepare_classified_plan_quotes(
                            account,
                            &market,
                            &self.existing_plan_ids,
                        )
                    });
                    if let Some(cursor) = cursor {
                        #[cfg(feature = "simulation-diagnostics")]
                        if let Some(result) = &cursor.grants {
                            domain.enrich_npc_decision_trace_constraints(
                                account,
                                result.grants.iter().filter_map(|grant| {
                                    grant.constraint.map(|constraint| {
                                        format!("{:?}: {constraint:?}", grant.plan_id)
                                    })
                                }),
                            );
                            domain.causal_record(
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
                PlanChainOperation::Execute(request) => Some(
                    domain
                        .prepare_plan_observation(request)
                        .map_err(execution)?,
                ),
                PlanChainOperation::ExecutionRoute(_) => unreachable!("route handled above"),
            };
            let progress = progress
                .map(|progress| domain.materialize_plan_progress(progress))
                .transpose()
                .map_err(execution)?;
            #[cfg(feature = "simulation-diagnostics")]
            domain.record_plan_trace_changes(trace_account, &trace_before);
            self.append_progress(progress);
        }
        self.operations = deferred;
        Ok(ready)
    }

    fn operation_blocked(
        &self,
        operation: &PlanChainOperation,
        domain: &PlanContinuationDomain<'_>,
        unfinished_routes: &BTreeSet<(AccountId, StockId)>,
    ) -> Result<bool, StepFatal> {
        Ok(match operation {
            PlanChainOperation::ExecutionRoute(route) => {
                let resource = route_resource(route.command());
                self.routes.is_blocked(&resource, unfinished_routes)
            }
            // cursor 持有该账户已分配的 grants；其他股票尚在等待 StockProcessing 时，
            // 下一计划仍可报价。Execute 会检查下方目标股票的状态。
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
                let plan = domain
                    .plans()
                    .plan(*plan_id)
                    .map_err(|error| invariant(&error.to_string()))?;
                self.routes
                    .is_blocked(&(plan.account(), plan.code().clone()), unfinished_routes)
            }
            // This operation computes shared account grants and discovers all active plans.
            // It must see prior lifecycle results before constructing the quote cursor.
            PlanChainOperation::AccountExecution { account, .. } => {
                self.routes.pending_for_account(*account)
                    || domain
                        .plans()
                        .active_plan_ids_for_account(*account)
                        .into_iter()
                        .filter_map(|plan_id| domain.plans().plan(plan_id).ok())
                        .any(|plan| unfinished_routes.contains(&(*account, plan.code().clone())))
            }
        })
    }

    pub(in crate::session) fn install_accepted_submit_parents(
        &self,
        domain: &mut PlanContinuationDomain<'_>,
        outcomes: &[(AccountId, StockId, u64, PlanRouteOutcome)],
    ) -> Result<(), StepFatal> {
        for (account, code, generation_index, outcome) in outcomes {
            let route = self
                .routes
                .pending_for_outcome(&(*account, code.clone()), *generation_index)?;
            domain
                .install_accepted_route(route, outcome)
                .map_err(execution)?;
        }
        Ok(())
    }

    pub(in crate::session) fn resume_adaptive_candidates(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        outcomes: impl IntoIterator<Item = (AccountId, StockId, u64, PlanRouteOutcome)>,
    ) -> Result<(), StepFatal> {
        let mut followups = Vec::new();
        for (account, code, generation_index, outcome) in outcomes {
            let route = self
                .routes
                .take_pending(&(account, code.clone()), generation_index)?;
            #[cfg(feature = "simulation-diagnostics")]
            let trace_before =
                super::super::decision_chain::plan_trace_states(account, domain.plans());
            #[cfg(feature = "simulation-diagnostics")]
            let trace_order = match &outcome {
                PlanRouteOutcome::Accepted(order) | PlanRouteOutcome::Canceled(order) => {
                    Some(*order)
                }
                PlanRouteOutcome::Rejected(_) => None,
            };
            let progress = domain
                .resume_route(*route, outcome)
                .and_then(|progress| domain.materialize_plan_progress(progress));
            #[cfg(feature = "simulation-diagnostics")]
            {
                domain.record_plan_trace_changes(account, &trace_before);
                domain.enrich_npc_decision_trace_order(account, trace_order);
            }
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
                    if needs_reconsideration && domain.plans().active_plan(account, &code).is_some()
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

fn route_resource(command: &PlanRouteCommand) -> (AccountId, StockId) {
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
