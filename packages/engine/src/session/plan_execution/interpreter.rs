use super::*;
use crate::plans::{PlanEvent, PlanRevision, TradingPlan};
mod resume;

#[derive(Clone)]
pub(in crate::session) enum PlanExecutionProgress {
    Complete(PlanExecutionReport),
    Route(Box<PlanExecutionRoute>),
    Adoption {
        plan: TradingPlan,
        child: NewChildSpec,
        order_id: OrderId,
        replaced: Option<OrderId>,
    },
}

#[derive(Clone)]
pub(in crate::session) struct PlanExecutionRoute {
    command: PlanRouteCommand,
    continuation: Continuation,
    replaced: Option<OrderId>,
}

impl PlanExecutionRoute {
    pub(in crate::session) fn command(&self) -> &PlanRouteCommand {
        &self.command
    }

    /// A new parent becomes visible to P4 fact projection only after P4 accepts its child.
    pub(in crate::session) fn install_accepted_submit_parent(
        &self,
        session: &mut GameSession,
        outcome: &PlanRouteOutcome,
    ) -> Result<(), PlanExecutionError> {
        if let (Continuation::Submit { plan, child }, PlanRouteOutcome::Accepted(_)) =
            (&self.continuation, outcome)
        {
            session.install_plan_parent(plan, *child, None)?;
        }
        Ok(())
    }

    pub(in crate::session) fn resume(
        self,
        session: &mut GameSession,
        plans: &mut PlanBook,
        outcome: PlanRouteOutcome,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        let progress = session.resume_plan_execution(plans, self.continuation, outcome)?;
        Ok(match self.replaced {
            Some(order_id) => progress.after_replace(order_id),
            None => progress,
        })
    }
}

#[derive(Clone)]
enum Continuation {
    Restructure {
        observed: TradingPlan,
        order_id: OrderId,
        revision: PlanRevision,
        terminating: bool,
    },
    Cancel {
        plan_id: crate::plans::PlanId,
        order_id: OrderId,
        reason: QuoteReason,
    },
    Replace {
        plan: TradingPlan,
        child: NewChildSpec,
        order_id: OrderId,
    },
    Conflicts {
        plan: TradingPlan,
        child: NewChildSpec,
        expected_order_id: OrderId,
    },
    Submit {
        plan: TradingPlan,
        child: NewChildSpec,
    },
}

impl PlanExecutionProgress {
    pub(in crate::session) fn restructure(
        plan: &TradingPlan,
        order_id: OrderId,
        revision: PlanRevision,
        terminating: bool,
    ) -> Self {
        Self::Route(Box::new(PlanExecutionRoute {
            command: cancel_command(plan, order_id, PlanCancelCause::Restructure),
            continuation: Continuation::Restructure {
                observed: plan.clone(),
                order_id,
                revision,
                terminating,
            },
            replaced: None,
        }))
    }
    fn after_replace(mut self, canceled_order_id: OrderId) -> Self {
        match &mut self {
            Self::Route(route) => route.replaced = Some(canceled_order_id),
            Self::Adoption { replaced, .. } => *replaced = Some(canceled_order_id),
            Self::Complete(report) => match report.disposition {
                PlanExecutionDisposition::Submitted { order_id, reason }
                | PlanExecutionDisposition::Adopted { order_id, reason } => {
                    report.disposition = PlanExecutionDisposition::Replaced {
                        canceled_order_id,
                        order_id,
                        reason,
                    };
                }
                _ => {}
            },
        }
        self
    }
    pub(super) fn cancel(plan: &TradingPlan, order_id: OrderId, reason: QuoteReason) -> Self {
        Self::Route(Box::new(PlanExecutionRoute {
            replaced: None,
            command: cancel_command(plan, order_id, PlanCancelCause::Explicit),
            continuation: Continuation::Cancel {
                plan_id: plan.plan_id,
                order_id,
                reason,
            },
        }))
    }

    pub(super) fn replace(plan: TradingPlan, child: NewChildSpec, order_id: OrderId) -> Self {
        Self::Route(Box::new(PlanExecutionRoute {
            replaced: None,
            command: cancel_command(&plan, order_id, PlanCancelCause::Replace),
            continuation: Continuation::Replace {
                plan,
                child,
                order_id,
            },
        }))
    }
}

fn cancel_command(
    plan: &TradingPlan,
    order_id: OrderId,
    cause: PlanCancelCause,
) -> PlanRouteCommand {
    PlanRouteCommand::Cancel {
        account: plan.account,
        code: plan.code.clone(),
        order_id,
        cause,
    }
}

impl GameSession {
    pub(super) fn prepare_working_cancels(
        &self,
        plan: TradingPlan,
        child: NewChildSpec,
        working: Vec<OrderId>,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        if let Some(order_id) = working.first().copied() {
            return Ok(PlanExecutionProgress::Route(Box::new(PlanExecutionRoute {
                replaced: None,
                command: cancel_command(&plan, order_id, PlanCancelCause::ConflictingWorkingOrder),
                continuation: Continuation::Conflicts {
                    plan,
                    child,
                    expected_order_id: order_id,
                },
            })));
        }
        Ok(PlanExecutionProgress::Route(Box::new(PlanExecutionRoute {
            replaced: None,
            command: PlanRouteCommand::SubmitLimit {
                account: plan.account,
                code: plan.code.clone(),
                side: plan.direction,
                price: child.price,
                qty: child.qty,
            },
            continuation: Continuation::Submit { plan, child },
        })))
    }
}
