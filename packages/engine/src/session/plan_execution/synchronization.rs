//! Transactional PlanBook synchronization from authoritative order facts.

use super::*;
use crate::plans::PlanEvent;

impl GameSession {
    /// The tick already owns this PlanBook inside its private session shadow.
    /// Touch only plans named by pending facts; the caller restores the book to
    /// the session before the complete tick is committed.
    pub(in crate::session) fn synchronize_owned_plan_execution(
        &mut self,
        plans: &mut PlanBook,
    ) -> Result<(), PlanExecutionError> {
        let pending = &self.pending_plan_events;
        let mut events = Vec::with_capacity(pending.len());
        for event in pending {
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
                    child_complete,
                    trading_day,
                } => (
                    plan_id,
                    PlanEvent::ChildOrderFilled {
                        order_id,
                        qty,
                        child_complete,
                        trading_day,
                    },
                ),
                PendingPlanEvent::DayEnded {
                    plan_id,
                    trading_day,
                } => (plan_id, PlanEvent::TradingDayEnded { trading_day }),
            };
            events.push((plan_id, plan_event));
        }
        let outcomes = plans.apply_active_events_atomically(&events)?;
        let mut completed_fills = Vec::new();
        for ((plan_id, plan_event), outcome) in events.iter().zip(outcomes) {
            if let Some(status) = outcome {
                if matches!(plan_event, PlanEvent::ChildOrderFilled { .. })
                    && matches!(
                        status,
                        crate::plans::PlanStatus::Completed
                            | crate::plans::PlanStatus::Terminated { .. }
                    )
                {
                    completed_fills.push(*plan_id);
                }
            }
        }
        // A day-end fact after a completing fill in this sealed batch has no
        // further plan transition. Consume it now: saved pending facts may only
        // target live plans, and replaying it every tick would grow without bound.
        self.pending_plan_events.clear();
        for plan_id in completed_fills {
            if plans
                .plan(plan_id)
                .map(|p| p.is_terminal())
                .unwrap_or(false)
            {
                self.remove_linked_parent(plan_id);
            }
        }
        Ok(())
    }
}
