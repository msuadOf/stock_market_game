//! Explicit K6 adapter from persistent plan decisions to the existing order lifecycle.

use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{AllocationGrant, PlanBook, PlanId, PlanStatus, PlanTarget};

mod actions;
mod routing;
mod synchronization;
mod types;

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
        self.synchronize_plan_execution(plans)?;
        let plan = plans.plan(request.plan_id)?.clone();
        self.validate_plan_execution_request(&plan, &request)?;
        let remaining =
            plan.remaining_share_qty()
                .ok_or(PlanExecutionError::UnconvertedFractionTarget {
                    plan_id: plan.plan_id,
                })?;
        match request.decision.action {
            QuoteAction::Wait => Ok(PlanExecutionReport {
                disposition: PlanExecutionDisposition::Waiting {
                    reason: request.decision.reason,
                },
                events: Vec::new(),
            }),
            QuoteAction::Keep { order_id } => {
                self.require_active_child(plan.plan_id, &plan.code, order_id)?;
                Ok(PlanExecutionReport {
                    disposition: PlanExecutionDisposition::Kept {
                        order_id,
                        reason: request.decision.reason,
                    },
                    events: Vec::new(),
                })
            }
            QuoteAction::Cancel { order_id } => {
                if !self.plan_child_is_cancellable_now() {
                    return Ok(self.pending_reconsideration(order_id, request.decision.reason));
                }
                Ok(self.cancel_plan_child(&plan, order_id, request.decision.reason))
            }
            QuoteAction::Replace {
                order_id,
                price,
                qty,
            } => {
                if !self.plan_child_is_cancellable_now() {
                    return Ok(self.pending_reconsideration(order_id, request.decision.reason));
                }
                let child = NewChildSpec {
                    price,
                    qty,
                    remaining,
                    reason: request.decision.reason,
                };
                self.validate_new_child(&plan, &request.allocation, child)?;
                let mut canceled = self.cancel_plan_child(&plan, order_id, request.decision.reason);
                if !matches!(
                    canceled.disposition,
                    PlanExecutionDisposition::Canceled { .. }
                ) {
                    return Ok(canceled);
                }
                let submitted = self.submit_plan_child(&plan, child, plans)?;
                let new_id = match submitted.disposition {
                    PlanExecutionDisposition::Submitted { order_id, .. } => order_id,
                    PlanExecutionDisposition::Adopted { order_id, .. } => order_id,
                    _ => return Ok(submitted),
                };
                canceled.events.extend(submitted.events);
                canceled.disposition = PlanExecutionDisposition::Replaced {
                    canceled_order_id: order_id,
                    order_id: new_id,
                    reason: request.decision.reason,
                };
                Ok(canceled)
            }
            QuoteAction::Submit { price, qty } => {
                if plan.direction == Side::Buy && remaining < self.setup.config.lot_size {
                    return Ok(PlanExecutionReport {
                        disposition: PlanExecutionDisposition::RemainingBelowBoardLot {
                            remaining_qty: remaining,
                        },
                        events: Vec::new(),
                    });
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
