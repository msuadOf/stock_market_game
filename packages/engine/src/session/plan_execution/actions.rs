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
        let required = match plan.direction {
            Side::Buy => {
                buy_order_reservation(&self.setup.config, child.price, child.qty, Money::ZERO)
            }
            Side::Sell => {
                sell_order_fee_reservation(&self.setup.config, child.price, child.qty, Money::ZERO)
            }
        }?;
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
    ) -> Result<PlanExecutionReport, PlanExecutionError> {
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
                self.install_plan_parent(plan, child, Some((candidate.id, candidate.qty)))?;
                self.remove_npc_order_lifecycle(plan.account, &plan.code, candidate.id);
                self.pending_plan_events.push(PendingPlanEvent::Accepted {
                    plan_id: plan.plan_id,
                    order_id: candidate.id,
                    trading_day: u64::from(self.day),
                });
                self.synchronize_plan_execution(plans)?;
                return Ok(PlanExecutionReport {
                    disposition: PlanExecutionDisposition::Adopted {
                        order_id: candidate.id,
                        reason: child.reason,
                    },
                    events: Vec::new(),
                });
            }
        }
        if let Some(first) = working.first() {
            if !self.plan_child_is_cancellable_now() {
                return Ok(
                    self.pending_reconsideration(first.id, QuoteReason::PendingReconsideration)
                );
            }
            let mut events = Vec::new();
            for order in working {
                self.route_plan_intent(
                    plan.account,
                    Intent::Cancel {
                        code: plan.code.clone(),
                        id: order.id,
                    },
                    &mut events,
                );
                if let Some(disposition) = Self::route_failure(&events) {
                    return Ok(PlanExecutionReport {
                        disposition,
                        events,
                    });
                }
            }
        }

        self.install_plan_parent(plan, child, None)?;
        let order_id = OrderId(self.next_order_id);
        let mut events = Vec::new();
        self.route_plan_intent(
            plan.account,
            Intent::PlaceLimit {
                code: plan.code.clone(),
                side: plan.direction,
                price: child.price,
                qty: child.qty,
            },
            &mut events,
        );
        if let Some(disposition) = Self::route_failure(&events) {
            self.remove_empty_linked_parent(plan.plan_id);
            return Ok(PlanExecutionReport {
                disposition,
                events,
            });
        }
        self.synchronize_plan_execution(plans)?;
        Ok(PlanExecutionReport {
            disposition: PlanExecutionDisposition::Submitted {
                order_id,
                reason: child.reason,
            },
            events,
        })
    }

    pub(super) fn cancel_plan_child(
        &mut self,
        plan: &crate::plans::TradingPlan,
        order_id: OrderId,
        reason: QuoteReason,
    ) -> PlanExecutionReport {
        let mut events = Vec::new();
        self.route_plan_intent(
            plan.account,
            Intent::Cancel {
                code: plan.code.clone(),
                id: order_id,
            },
            &mut events,
        );
        let disposition = Self::route_failure(&events)
            .unwrap_or(PlanExecutionDisposition::Canceled { order_id, reason });
        PlanExecutionReport {
            disposition,
            events,
        }
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

    pub(super) fn plan_child_is_cancellable_now(&self) -> bool {
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
