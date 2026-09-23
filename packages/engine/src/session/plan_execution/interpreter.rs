use super::*;
use crate::plans::{PlanEvent, PlanRevision, TradingPlan};
use std::collections::VecDeque;
mod resume;

pub(in crate::session) enum PlanExecutionProgress {
    Complete(PlanExecutionReport),
    Route(Box<PlanExecutionRoute>),
}

pub(in crate::session) struct PlanExecutionRoute {
    command: PlanRouteCommand,
    continuation: Continuation,
    replaced: Option<OrderId>,
}

impl PlanExecutionRoute {
    pub(in crate::session) fn command(&self) -> &PlanRouteCommand {
        &self.command
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

enum Continuation {
    Restructure {
        plan_id: PlanId,
        order_id: OrderId,
        revision: PlanRevision,
        terminating: bool,
    },
    Cancel {
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
        remaining: VecDeque<OrderId>,
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
                plan_id: plan.plan_id,
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
            continuation: Continuation::Cancel { order_id, reason },
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
    pub(in crate::session) fn consume_plan_execution(
        &mut self,
        plans: &mut PlanBook,
        progress: PlanExecutionProgress,
    ) -> Result<PlanExecutionReport, PlanExecutionError> {
        self.consume_plan_execution_numbered(plans, progress, &mut 0)
    }

    pub(in crate::session) fn consume_plan_execution_numbered(
        &mut self,
        plans: &mut PlanBook,
        mut progress: PlanExecutionProgress,
        next_ordinal: &mut u64,
    ) -> Result<PlanExecutionReport, PlanExecutionError> {
        let mut events = Vec::new();
        loop {
            match progress {
                PlanExecutionProgress::Complete(mut report) => {
                    events.append(&mut report.events);
                    report.events = events;
                    return Ok(report);
                }
                PlanExecutionProgress::Route(route) => {
                    let yielded = YieldedPlanCommand::new(route.command().clone(), next_ordinal)?;
                    let outcome = self.consume_plan_route_command(yielded.command, &mut events)?;
                    progress = route.resume(self, plans, outcome)?;
                }
            }
        }
    }

    pub(super) fn prepare_working_cancels(
        &mut self,
        plan: TradingPlan,
        child: NewChildSpec,
        mut remaining: VecDeque<OrderId>,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        if let Some(order_id) = remaining.pop_front() {
            return Ok(PlanExecutionProgress::Route(Box::new(PlanExecutionRoute {
                replaced: None,
                command: cancel_command(&plan, order_id, PlanCancelCause::ConflictingWorkingOrder),
                continuation: Continuation::Conflicts {
                    plan,
                    child,
                    expected_order_id: order_id,
                    remaining,
                },
            })));
        }
        self.install_plan_parent(&plan, child, None)?;
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
