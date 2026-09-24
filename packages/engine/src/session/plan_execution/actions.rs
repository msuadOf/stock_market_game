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
                plan_id: plan.plan_id,
            });
        }
        if child.qty > child.remaining {
            return Err(PlanExecutionError::QuantityExceedsRemaining {
                plan_id: plan.plan_id,
                requested_qty: child.qty,
                remaining_qty: child.remaining,
            });
        }
        let required = live_cash_reservation(
            &self.setup.config,
            plan.direction,
            child.price,
            child.qty,
            Money::ZERO,
        )?;
        if required > allocation.allocated_cash {
            return Err(PlanExecutionError::AllocationInsufficient {
                plan_id: plan.plan_id,
                allocated_cents: allocation.allocated_cash.cents(),
                required_cents: required.cents(),
            });
        }
        Ok(())
    }

    pub(super) fn submit_plan_child(
        &mut self,
        plan: &crate::plans::TradingPlan,
        child: NewChildSpec,
        plans: &mut PlanBook,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        if self
            .parent_orders
            .get(&plan.account)
            .and_then(|parents| parents.get(&plan.code))
            .is_some_and(|parent| {
                parent.linked_plan_id != Some(plan.plan_id)
                    || parent.active_child_order_id.is_some()
            })
        {
            return Err(PlanExecutionError::IncompatibleExecutionState {
                plan_id: plan.plan_id,
            });
        }

        let working = self.plan_working_orders(plan.account, &plan.code);
        if let [candidate] = working.as_slice() {
            if candidate.side == plan.direction
                && candidate.price == child.price
                && candidate.qty == child.qty
            {
                let mut events = Vec::new();
                if self.pending_plan_events.len() >= crate::session::MAX_SAVED_PLAN_EVENTS {
                    self.report_pending_plan_event_capacity(&mut events);
                    return Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                        disposition: PlanExecutionDisposition::SettlementFailed {
                            reason: "pending plan event capacity exhausted".to_string(),
                        },
                        events,
                    }));
                }
                self.install_plan_parent(plan, child, Some((candidate.id, candidate.qty)))?;
                self.remove_npc_order_lifecycle(plan.account, &plan.code, candidate.id);
                let appended = self.push_pending_plan_event(
                    PendingPlanEvent::Accepted {
                        plan_id: plan.plan_id,
                        order_id: candidate.id,
                        trading_day: u64::from(self.day),
                    },
                    &mut events,
                );
                if !appended {
                    self.report_pending_plan_event_capacity(&mut events);
                    return Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                        disposition: PlanExecutionDisposition::SettlementFailed {
                            reason: "pending plan event capacity exhausted".to_string(),
                        },
                        events,
                    }));
                }
                self.synchronize_owned_plan_execution(plans)?;
                return Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                    disposition: PlanExecutionDisposition::Adopted {
                        order_id: candidate.id,
                        reason: child.reason,
                    },
                    events,
                }));
            }
        }
        if !self.has_pending_plan_event_capacity(2) {
            let mut events = Vec::new();
            self.report_pending_plan_event_capacity(&mut events);
            return Ok(PlanExecutionProgress::Complete(PlanExecutionReport {
                disposition: PlanExecutionDisposition::SettlementFailed {
                    reason: "pending plan event capacity exhausted".to_string(),
                },
                events,
            }));
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

    pub(super) fn require_active_child(
        &self,
        plan_id: PlanId,
        code: &StockCode,
        order_id: OrderId,
    ) -> Result<(), PlanExecutionError> {
        let active = self
            .parent_orders
            .values()
            .flat_map(|parents| parents.get(code))
            .find(|parent| parent.linked_plan_id == Some(plan_id))
            .and_then(|parent| parent.active_child_order_id);
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
                self.tick % self.setup.ticks_per_day < self.setup.auction_ticks / 3
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
            events: Vec::new(),
        }
    }
}
