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
                observed,
                order_id,
                revision,
                terminating,
            } => {
                let plan_id = observed.plan_id;
                let disposition = match outcome {
                    PlanRouteOutcome::Canceled(id) if id == order_id => {
                        let current = plans.plan(plan_id)?;
                        let changed = current.version != observed.version
                            || current.filled_qty != observed.filled_qty
                            || current.status != observed.status
                            || current.active_child_order_id != observed.active_child_order_id;
                        self.clear_canceled_plan_child(plans, plan_id, order_id)?;
                        if changed {
                            PlanExecutionDisposition::Waiting {
                                reason: QuoteReason::PendingReconsideration,
                            }
                        } else {
                            plans.apply(plan_id, PlanEvent::Revised { revision })?;
                            if terminating || plans.plan(plan_id)?.is_terminal() {
                                self.remove_linked_parent(plan_id);
                            }
                            PlanExecutionDisposition::Canceled {
                                order_id,
                                reason: QuoteReason::PendingReconsideration,
                            }
                        }
                    }
                    other => {
                        let failure = other.failure()?;
                        if !plans.plan(plan_id)?.is_terminal() {
                            Self::observe_plan(plans, plan_id, u64::from(self.day));
                        }
                        failure
                    }
                };
                PlanExecutionProgress::Complete(PlanExecutionReport { disposition })
            }
            Continuation::Cancel {
                plan_id,
                order_id,
                reason,
            } => {
                let disposition = match outcome {
                    PlanRouteOutcome::Canceled(id) if id == order_id => {
                        self.clear_canceled_plan_child(plans, plan_id, order_id)?;
                        PlanExecutionDisposition::Canceled { order_id, reason }
                    }
                    other => other.failure()?,
                };
                PlanExecutionProgress::Complete(PlanExecutionReport { disposition })
            }
            Continuation::Replace {
                plan,
                child,
                order_id,
            } => match outcome {
                PlanRouteOutcome::Canceled(id) if id == order_id => {
                    self.clear_canceled_plan_child(plans, plan.plan_id, order_id)?;
                    match self.refresh_canceled_child_plan(plans, &plan, child)? {
                        Ok((plan, child)) => self
                            .submit_plan_child(&plan, child)?
                            .after_replace(order_id),
                        Err(progress) => progress,
                    }
                }
                other => PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition: other.failure()?,
                }),
            },
            Continuation::Conflicts {
                plan,
                child,
                expected_order_id,
            } => match outcome {
                PlanRouteOutcome::Canceled(order_id) if order_id == expected_order_id => {
                    self.clear_canceled_plan_child(plans, plan.plan_id, order_id)?;
                    match self.refresh_canceled_child_plan(plans, &plan, child)? {
                        Ok((plan, child)) => self.submit_plan_child(&plan, child)?,
                        Err(progress) => progress,
                    }
                }
                other => PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition: other.failure()?,
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
                PlanExecutionProgress::Complete(PlanExecutionReport { disposition })
            }
        })
    }

    fn refresh_canceled_child_plan(
        &self,
        plans: &PlanBook,
        observed: &TradingPlan,
        child: NewChildSpec,
    ) -> Result<Result<(TradingPlan, NewChildSpec), PlanExecutionProgress>, PlanExecutionError>
    {
        let current = plans.plan(observed.plan_id)?;
        let remaining =
            current
                .remaining_share_qty()
                .ok_or(PlanExecutionError::UnconvertedFractionTarget {
                    plan_id: current.plan_id,
                })?;
        if current.status != crate::plans::PlanStatus::Active
            || current.version != observed.version
            || remaining != child.remaining
        {
            // A fill or revision during P4 invalidates the quote quantity and possibly its
            // price. The affected plan must observe the new state before making another quote.
            return Ok(Err(PlanExecutionProgress::Complete(PlanExecutionReport {
                disposition: PlanExecutionDisposition::Waiting {
                    reason: QuoteReason::PendingReconsideration,
                },
            })));
        }
        Ok(Ok((current.clone(), child)))
    }

    fn clear_canceled_plan_child(
        &self,
        plans: &mut PlanBook,
        plan_id: crate::plans::PlanId,
        order_id: OrderId,
    ) -> Result<(), PlanExecutionError> {
        let current = plans.plan(plan_id)?;
        if !current.is_terminal() && current.active_child_order_id == Some(order_id) {
            plans.apply(
                plan_id,
                PlanEvent::ChildOrderCanceled {
                    order_id,
                    trading_day: u64::from(self.day),
                },
            )?;
        }
        Ok(())
    }
}
