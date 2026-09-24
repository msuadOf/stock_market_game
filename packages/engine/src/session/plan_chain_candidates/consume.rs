use super::*;

impl GameSession {
    pub(in crate::session) fn consume_plan_chain_operation_batch(
        &mut self,
        mut batch: PlanChainOperationBatch,
        events: &mut Vec<Event>,
    ) {
        loop {
            let Some(operation) = batch.operations.pop_front() else {
                if matches!(batch.source, AccountSource::Empty) {
                    break;
                }
                batch.prepare_next_account(self);
                continue;
            };
            let mut plans = std::mem::take(&mut self.plans);
            let progress = match operation {
                PlanChainOperation::ExecutionRoute(route) => {
                    let candidate =
                        batch
                            .enumerate_candidate(route.command())
                            .unwrap_or_else(|error| {
                                panic!("plan-chain candidate yield failed: {error}")
                            });
                    let ordinal = candidate.chain_generation_index;
                    let outcome = self
                        .consume_plan_route_command(route.command().clone(), events)
                        .unwrap_or_else(|error| {
                            panic!("plan command {ordinal} route failed: {error}")
                        });
                    Some(
                        route
                            .resume(self, &mut plans, outcome)
                            .unwrap_or_else(|error| {
                                panic!("plan command continuation failed: {error}")
                            }),
                    )
                }
                PlanChainOperation::QuotePlans(mut cursor) => {
                    self.synchronize_owned_plan_execution(&mut plans)
                        .unwrap_or_else(|error| {
                            panic!("plan quote synchronization failed: {error}")
                        });
                    if let Some(request) = self.generate_next_plan_quote(&mut cursor, &plans) {
                        batch
                            .operations
                            .push_front(PlanChainOperation::QuotePlans(cursor));
                        batch
                            .operations
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
                        self.drive_plans_for_account(
                            account,
                            &BTreeMap::from([(code, assessment)]),
                            &market,
                            &mut plans,
                            &mut generated,
                        );
                        batch.operations.push_front(PlanChainOperation::Lifecycle {
                            account,
                            assessments,
                            market,
                        });
                        while let Some(operation) = generated.operations.pop_back() {
                            batch.operations.push_front(operation);
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
                    Some(order_id) => {
                        let plan = plans
                            .plan(plan_id)
                            .unwrap_or_else(|error| panic!("restructure plan missing: {error}"));
                        Some(PlanExecutionProgress::restructure(
                            plan,
                            order_id,
                            revision,
                            terminating,
                        ))
                    }
                    None => {
                        if terminating {
                            self.remove_linked_parent(plan_id);
                        }
                        plans
                            .apply(plan_id, PlanEvent::Revised { revision })
                            .unwrap_or_else(|error| {
                                panic!("deferred plan revision failed for {plan_id:?}: {error}")
                            });
                        None
                    }
                },
                PlanChainOperation::AccountExecution { account, market } => {
                    self.execute_plans_for_account(account, &market, &mut plans, &mut batch);
                    None
                }
                PlanChainOperation::Execute(request) => Some(
                    self.prepare_plan_observation(&mut plans, request)
                        .unwrap_or_else(|error| {
                            panic!("plan operation preparation failed: {error}")
                        }),
                ),
            };
            match progress {
                Some(PlanExecutionProgress::Complete(report)) => events.extend(report.events),
                Some(PlanExecutionProgress::Route(route)) => batch
                    .operations
                    .push_front(PlanChainOperation::ExecutionRoute(route)),
                None => {}
            }
            self.plans = plans;
        }
    }
}
