//! Transactional PlanBook synchronization from authoritative order facts.

use super::*;
use crate::plans::PlanEvent;

impl GameSession {
    /// Applies accepted/fill/day-end facts captured by real routing since the previous observation.
    ///
    /// 任务 26：pending 队列可能同时携带外部计划簿（集成测试自带 PlanBook）
    /// 或已终止计划的迟到事件——这些条目**原样保留**给其属主/不再适用，
    /// 其余按序应用（任务 24 的恰好一次语义不变）。
    pub fn synchronize_plan_execution(
        &mut self,
        plans: &mut PlanBook,
    ) -> Result<(), PlanExecutionError> {
        let mut candidate = plans.clone();
        let pending = self.pending_plan_events.clone();
        let mut retained = Vec::new();
        let mut completed_fills = Vec::new();
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
            let alive = candidate
                .plan(plan_id)
                .map(|plan| !plan.is_terminal())
                .unwrap_or(false);
            if alive {
                let is_fill = matches!(plan_event, PlanEvent::ChildOrderFilled { .. });
                candidate.apply(plan_id, plan_event)?;
                if is_fill
                    && candidate
                        .plan(plan_id)
                        .map(|plan| plan.is_terminal())
                        .unwrap_or(false)
                {
                    completed_fills.push(plan_id);
                }
            } else {
                retained.push(*event);
            }
        }
        *plans = candidate;
        self.pending_plan_events = retained;
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
