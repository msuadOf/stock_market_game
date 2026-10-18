//! Real submit/cancel/adoption operations for the plan execution adapter.

use super::*;

impl GameSession {
    pub(super) fn validate_new_child(
        &self,
        plan: &crate::plans::TradingPlan,
        allocation: &AllocationGrant,
        child: NewChildSpec,
    ) -> Result<(), PlanExecutionError> {
        if child.qty == 0 {
            return Err(PlanExecutionError::InvalidChildQuantity {
                plan_id: plan.plan_id(),
            });
        }
        if child.qty > child.remaining {
            return Err(PlanExecutionError::QuantityExceedsRemaining {
                plan_id: plan.plan_id(),
                requested_qty: child.qty,
                remaining_qty: child.remaining,
            });
        }
        let required = live_cash_reservation(
            &self.state.setup.config,
            plan.direction(),
            child.price,
            child.qty,
            Money::ZERO,
        )?;
        if required > allocation.allocated_cash {
            return Err(PlanExecutionError::AllocationInsufficient {
                plan_id: plan.plan_id(),
                allocated_cents: allocation.allocated_cash.cents(),
                required_cents: required.cents(),
            });
        }
        Ok(())
    }

    pub(super) fn submit_plan_child(
        &self,
        plan: &crate::plans::TradingPlan,
        child: NewChildSpec,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        if let Some(progress) = self.active_plan_child_feedback(plan)? {
            return Ok(progress);
        }

        let working = self.plan_working_orders(plan.account(), plan.code());
        if let [candidate] = working.as_slice() {
            if candidate.side == plan.direction()
                && candidate.price == child.price
                && candidate.qty == child.qty
            {
                return Ok(PlanExecutionProgress::Adoption {
                    plan: Box::new(plan.clone()),
                    child,
                    order_id: candidate.id,
                    replaced: None,
                });
            }
        }
        if let Some(first) = working.first() {
            if !self.plan_child_is_cancellable_now() {
                return Ok(PlanExecutionProgress::Complete(
                    self.pending_reconsideration(first.id, QuoteReason::PendingReconsideration),
                ));
            }
        }
        self.prepare_working_cancels(
            plan.clone(),
            child,
            working.into_iter().map(|order| order.id).collect(),
        )
    }

    pub(super) fn active_plan_child_feedback(
        &self,
        plan: &crate::plans::TradingPlan,
    ) -> Result<Option<PlanExecutionProgress>, PlanExecutionError> {
        if let Some(order_id) = plan.active_child_order_id() {
            let leg = plan
                .execution()
                .expect("an active child implies an installed execution leg");
            let working = self
                .plan_working_orders(plan.account(), plan.code())
                .into_iter()
                .find(|working| working.id == order_id);
            if !matches!(working, Some(order)
                if order.side == plan.direction()
                    && leg.limit_price() == order.price
                    && leg.active_child_remaining_qty() == Some(order.qty))
            {
                return Err(PlanExecutionError::IncompatibleExecutionState {
                    plan_id: plan.plan_id(),
                });
            }
            // A quote prepared before an earlier route completed can now
            // find this plan's child in flight. Ask the plan to re-evaluate
            // it instead of treating a normal timing change as a fatal tick.
            return Ok(Some(PlanExecutionProgress::Complete(
                self.pending_reconsideration(order_id, QuoteReason::PendingReconsideration),
            )));
        }
        Ok(None)
    }

    /// Materialize only effects whose decision has reached this account's execution position.
    /// The current coordinator calls this immediately; a future delayed caller must refresh
    /// observations instead of trusting a stale adoption or capacity decision.
    pub(in crate::session) fn materialize_plan_progress(
        &mut self,
        plans: &mut PlanBook,
        progress: PlanExecutionProgress,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        match progress {
            PlanExecutionProgress::Adoption {
                plan,
                child,
                order_id,
                replaced,
            } => {
                let current = self.plan_working_orders(plan.account(), plan.code());
                if !matches!(current.as_slice(), [order]
                    if order.id == order_id
                        && order.side == plan.direction()
                        && order.price == child.price
                        && order.qty == child.qty)
                {
                    return Err(PlanExecutionError::IncompatibleExecutionState {
                        plan_id: plan.plan_id(),
                    });
                }
                self.install_plan_execution(
                    plans,
                    &plan,
                    child,
                    Some((order_id, child.qty)),
                )?;
                self.remove_npc_order_lifecycle(plan.account(), plan.code(), order_id);
                let disposition = match replaced {
                    Some(canceled_order_id) => PlanExecutionDisposition::Replaced {
                        canceled_order_id,
                        order_id,
                        reason: child.reason,
                    },
                    None => PlanExecutionDisposition::Adopted {
                        order_id,
                        reason: child.reason,
                    },
                };
                Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition,
                }))
            }
            progress => Ok(progress),
        }
    }

    pub(super) fn require_active_child(
        &self,
        plan: &crate::plans::TradingPlan,
        order_id: OrderId,
    ) -> Result<(), PlanExecutionError> {
        let plan_id = plan.plan_id();
        let active = plan.active_child_order_id();
        if active != Some(order_id) {
            return Err(PlanExecutionError::ChildMismatch {
                plan_id,
                requested: order_id,
                active,
            });
        }
        Ok(())
    }

    pub(in crate::session) fn plan_child_is_cancellable_now(&self) -> bool {
        match self.phase() {
            TradingPhase::Continuous => true,
            TradingPhase::CallAuction => {
                self.state.tick % self.state.setup.ticks_per_day
                    < self.state.setup.auction_ticks / 3
            }
            TradingPhase::PreOpen | TradingPhase::ClosingAuction => false,
        }
    }

    pub(super) fn pending_reconsideration(
        &self,
        order_id: OrderId,
        reason: QuoteReason,
    ) -> PlanExecutionReport {
        PlanExecutionReport {
            disposition: PlanExecutionDisposition::PendingReconsideration { order_id, reason },
        }
    }
}
