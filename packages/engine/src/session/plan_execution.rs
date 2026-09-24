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
        // The public observation adapter is itself a quiet-point transaction. Its legacy
        // router mutates the order book directly, while schema-v2 saves require the live
        // envelope ledger to describe exactly the same orders. Execute both the route and the
        // ledger rebase on private candidates so an invariant failure cannot expose a half-
        // updated session or PlanBook.
        let mut session_candidate = self.clone_for_tick_shadow()?;
        let mut plan_candidate = session_candidate.merge_external_plan_book_handoff(plans)?;
        let progress = session_candidate.prepare_plan_observation(&mut plan_candidate, request)?;
        let report = session_candidate.consume_plan_execution(&mut plan_candidate, progress)?;
        // `SaveSlot` persists the session-owned PlanBook. Keep it byte-for-byte aligned with the
        // public adapter's successful transactional result before the session becomes visible.
        session_candidate.plans = plan_candidate.clone();
        session_candidate.rebase_legacy_envelope_ledger_for_quiet_point()?;
        self.commit_tick_shadow(session_candidate);
        *plans = plan_candidate;
        Ok(report)
    }

    pub(in crate::session) fn prepare_plan_observation(
        &mut self,
        plans: &mut PlanBook,
        request: PlanExecutionRequest,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        self.synchronize_owned_plan_execution(plans)?;
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
