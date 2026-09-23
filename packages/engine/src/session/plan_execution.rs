//! Explicit K6 adapter from persistent plan decisions to the existing order lifecycle.

use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{AllocationGrant, PlanBook, PlanId, PlanStatus, PlanTarget};

mod actions;
mod commands;
#[cfg(test)]
mod continuation_tests;
mod interpreter;
mod routing;
mod synchronization;
mod types;

pub(in crate::session) use commands::{
    PlanCancelCause, PlanRouteCommand, PlanRouteOutcome, YieldedPlanCommand,
};
pub(in crate::session) use interpreter::{PlanExecutionProgress, PlanExecutionRoute};
use types::NewChildSpec;
pub use types::PendingPlanEvent;
pub use types::{
    PlanExecutionDisposition, PlanExecutionError, PlanExecutionReport, PlanExecutionRequest,
};

impl GameSession {
    /// Executes one person's explicit observation through the authoritative router.
    pub fn execute_plan_observation(
        &mut self,
        plans: &mut PlanBook,
        request: PlanExecutionRequest,
    ) -> Result<PlanExecutionReport, PlanExecutionError> {
        let progress = self.prepare_plan_observation(plans, request)?;
        self.consume_plan_execution(plans, progress)
    }

    pub(in crate::session) fn prepare_plan_observation(
        &mut self,
        plans: &mut PlanBook,
        request: PlanExecutionRequest,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        self.synchronize_plan_execution(plans)?;
        let plan = plans.plan(request.plan_id)?.clone();
        self.validate_plan_execution_request(&plan, &request)?;
        let remaining =
            plan.remaining_share_qty()
                .ok_or(PlanExecutionError::UnconvertedFractionTarget {
                    plan_id: plan.plan_id,
                })?;
        match request.decision.action {
            QuoteAction::Wait => Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                disposition: PlanExecutionDisposition::Waiting {
                    reason: request.decision.reason,
                },
                events: Vec::new(),
            })),
            QuoteAction::Keep { order_id } => {
                self.require_active_child(plan.plan_id, &plan.code, order_id)?;
                Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition: PlanExecutionDisposition::Kept {
                        order_id,
                        reason: request.decision.reason,
                    },
                    events: Vec::new(),
                }))
            }
            QuoteAction::Cancel { order_id } => {
                if !self.plan_child_is_cancellable_now() {
                    return Ok(PlanExecutionProgress::Complete(
                        self.pending_reconsideration(order_id, request.decision.reason),
                    ));
                }
                Ok(PlanExecutionProgress::cancel(
                    &plan,
                    order_id,
                    request.decision.reason,
                ))
            }
            QuoteAction::Replace {
                order_id,
                price,
                qty,
            } => {
                if !self.plan_child_is_cancellable_now() {
                    return Ok(PlanExecutionProgress::Complete(
                        self.pending_reconsideration(order_id, request.decision.reason),
                    ));
                }
                let child = NewChildSpec {
                    price,
                    qty,
                    remaining,
                    reason: request.decision.reason,
                };
                self.validate_new_child(&plan, &request.allocation, child)?;
                Ok(PlanExecutionProgress::replace(plan, child, order_id))
            }
            QuoteAction::Submit { price, qty } => {
                if plan.direction == Side::Buy && remaining < self.setup.config.lot_size {
                    return Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                        disposition: PlanExecutionDisposition::RemainingBelowBoardLot {
                            remaining_qty: remaining,
                        },
                        events: Vec::new(),
                    }));
                }
                let child = NewChildSpec {
                    price,
                    qty,
                    remaining,
                    reason: request.decision.reason,
                };
                self.validate_new_child(&plan, &request.allocation, child)?;
                self.submit_plan_child(&plan, child, plans)
            }
        }
    }

    fn validate_plan_execution_request(
        &self,
        plan: &crate::plans::TradingPlan,
        request: &PlanExecutionRequest,
    ) -> Result<(), PlanExecutionError> {
        if !self.accounts.contains_key(&plan.account) {
            return Err(PlanExecutionError::UnknownAccount {
                plan_id: plan.plan_id,
                account: plan.account,
            });
        }
        if !self.markets.contains_key(&plan.code) {
            return Err(PlanExecutionError::UnknownStock {
                plan_id: plan.plan_id,
                code: plan.code.clone(),
            });
        }
        let session_day = u64::from(self.day);
        if request.trading_day != session_day {
            return Err(PlanExecutionError::TradingDayMismatch {
                plan_id: plan.plan_id,
                session_day,
                request_day: request.trading_day,
            });
        }
        if request.allocation.plan_id != plan.plan_id || request.allocation.code != plan.code {
            return Err(PlanExecutionError::AllocationMismatch {
                plan_id: plan.plan_id,
                allocation_plan_id: request.allocation.plan_id,
                allocation_code: request.allocation.code.clone(),
            });
        }
        if matches!(
            request.decision.action,
            QuoteAction::Submit { .. } | QuoteAction::Replace { .. }
        ) && plan.status != PlanStatus::Active
        {
            return Err(PlanExecutionError::PlanCannotSubmit {
                plan_id: plan.plan_id,
                status: plan.status,
            });
        }
        Ok(())
    }
}
