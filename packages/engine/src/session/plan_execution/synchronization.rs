//! Transactional PlanBook synchronization from authoritative order facts.

use super::*;
use crate::plans::PlanEvent;

impl GameSession {
    /// Applies accepted/fill/day-end facts captured by real routing since the previous observation.
    pub fn synchronize_plan_execution(
        &mut self,
        plans: &mut PlanBook,
    ) -> Result<(), PlanExecutionError> {
        let mut candidate = plans.clone();
        let pending = self.pending_plan_events.clone();
        for event in &pending {
            let (plan_id, plan_event) = match *event {
                PendingPlanEvent::Accepted {
                    plan_id,
                    order_id,
                    trading_day,
                } => (
                    plan_id,
                    PlanEvent::ChildOrderAccepted {
                        order_id,
                        trading_day,
                    },
                ),
                PendingPlanEvent::Filled {
                    plan_id,
                    order_id,
                    qty,
                    trading_day,
                } => (
                    plan_id,
                    PlanEvent::ChildOrderFilled {
                        order_id,
                        qty,
                        trading_day,
                    },
                ),
                PendingPlanEvent::DayEnded {
                    plan_id,
                    trading_day,
                } => (plan_id, PlanEvent::TradingDayEnded { trading_day }),
            };
            candidate.apply(plan_id, plan_event)?;
        }
        *plans = candidate;
        self.pending_plan_events.clear();
        for event in pending {
            if let PendingPlanEvent::Filled { plan_id, .. } = event {
                if plans.plan(plan_id)?.is_terminal() {
                    self.remove_linked_parent(plan_id);
                }
            }
        }
        Ok(())
    }

    pub(in crate::session) fn record_plan_execution_day_end(&mut self) {
        let trading_day = u64::from(self.day);
        for plan_id in self
            .parent_orders
            .values()
            .flat_map(|plans| plans.values())
            .filter(|plan| plan.filled_qty < plan.target_qty)
            .filter_map(|plan| plan.linked_plan_id)
        {
            self.pending_plan_events.push(PendingPlanEvent::DayEnded {
                plan_id,
                trading_day,
            });
        }
    }
}
