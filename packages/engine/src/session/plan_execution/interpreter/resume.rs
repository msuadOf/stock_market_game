use super::*;

impl GameSession {
    pub(super) fn resume_plan_execution(
        &mut self,
        plans: &mut PlanBook,
        continuation: Continuation,
        outcome: PlanRouteOutcome,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        Ok(match continuation {
            Continuation::Restructure {
                plan_id,
                order_id,
                revision,
                terminating,
            } => {
                let disposition = match outcome {
                    PlanRouteOutcome::Canceled(id) if id == order_id => {
                        if terminating {
                            self.remove_linked_parent(plan_id);
                        }
                        plans.apply(plan_id, PlanEvent::Revised { revision })?;
                        PlanExecutionDisposition::Canceled {
                            order_id,
                            reason: QuoteReason::PendingReconsideration,
                        }
                    }
                    other => {
                        let failure = other.failure()?;
                        Self::observe_plan(plans, plan_id, u64::from(self.day));
                        failure
                    }
                };
                PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition,
                    events: Vec::new(),
                })
            }
            Continuation::Cancel { order_id, reason } => {
                let disposition = match outcome {
                    PlanRouteOutcome::Canceled(id) if id == order_id => {
                        PlanExecutionDisposition::Canceled { order_id, reason }
                    }
                    other => other.failure()?,
                };
                PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition,
                    events: Vec::new(),
                })
            }
            Continuation::Replace {
                plan,
                child,
                order_id,
            } => match outcome {
                PlanRouteOutcome::Canceled(id) if id == order_id => self
                    .submit_plan_child(&plan, child, plans)?
                    .after_replace(order_id),
                other => PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition: other.failure()?,
                    events: Vec::new(),
                }),
            },
            Continuation::Conflicts {
                plan,
                child,
                expected_order_id,
                remaining,
            } => match outcome {
                PlanRouteOutcome::Canceled(order_id) if order_id == expected_order_id => {
                    self.prepare_working_cancels(plan, child, remaining)?
                }
                other => PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition: other.failure()?,
                    events: Vec::new(),
                }),
            },
            Continuation::Submit { plan, child } => {
                let disposition = match outcome {
                    PlanRouteOutcome::Accepted(order_id) => {
                        self.synchronize_owned_plan_execution(plans)?;
                        PlanExecutionDisposition::Submitted {
                            order_id,
                            reason: child.reason,
                        }
                    }
                    other => {
                        self.remove_empty_linked_parent(plan.plan_id);
                        other.failure()?
                    }
                };
                PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition,
                    events: Vec::new(),
                })
            }
        })
    }
}
