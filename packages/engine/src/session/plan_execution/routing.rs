//! Target-stock working orders and parent execution-state maintenance.

use super::*;

#[derive(Clone, Copy)]
pub(in crate::session) struct WorkingPlanOrder {
    pub(super) id: OrderId,
    pub(super) side: Side,
    pub(super) price: Money,
    pub(super) qty: u32,
}

impl GameSession {
    pub(in crate::session) fn plan_working_orders(
        &self,
        account: AccountId,
        code: &StockCode,
    ) -> Vec<WorkingPlanOrder> {
        self.state
            .markets
            .get(code)
            .into_iter()
            .flat_map(|market| {
                let mut orders = market.resting_orders_for(account);
                orders.sort_by_key(|order| order.seq);
                orders
            })
            .map(|order| WorkingPlanOrder {
                id: order.id,
                side: order.side,
                price: order.price,
                qty: order.qty,
            })
            .chain(
                self.state
                    .auction_orders
                    .get(code)
                    .into_iter()
                    .flatten()
                    .filter(|order| order.owner == account)
                    .map(|order| WorkingPlanOrder {
                        id: OrderId(order.order_id),
                        side: order.side,
                        price: order.limit,
                        qty: order.qty,
                    }),
            )
            .collect()
    }

    pub(super) fn install_plan_parent(
        &mut self,
        plan: &crate::plans::TradingPlan,
        child: NewChildSpec,
        active: Option<(OrderId, u32)>,
    ) -> Result<(), PlanExecutionError> {
        let target_qty = match plan.target() {
            PlanTarget::ShareCount(target_qty) => target_qty,
            PlanTarget::PositionFractionBp(_) => {
                return Err(PlanExecutionError::UnconvertedFractionTarget {
                    plan_id: plan.plan_id(),
                });
            }
        };
        let day_end = (self
            .stock_trading_day(plan.code())
            .expect("plan stock trading day is valid")
            + 1)
            * u64::from(GAME_INTRADAY_MINUTES_PER_DAY);
        self.state
            .parent_orders
            .entry(plan.account())
            .or_default()
            .insert(
                plan.code().clone(),
                ParentOrderPlan::from_facts(
                    plan.code().clone(),
                    plan.direction(),
                    target_qty,
                    plan.filled_qty(),
                    child.qty,
                    active,
                    Some(plan.plan_id()),
                    child.price,
                    day_end,
                ),
            );
        Ok(())
    }

    pub(super) fn remove_empty_linked_parent(&mut self, plan_id: PlanId) {
        self.retain_other_linked_parents(plan_id, true);
    }

    pub(in crate::session) fn remove_linked_parent(&mut self, plan_id: PlanId) {
        self.retain_other_linked_parents(plan_id, false);
    }

    fn retain_other_linked_parents(&mut self, plan_id: PlanId, retain_progress: bool) {
        let accounts: Vec<_> = self.state.parent_orders.keys().copied().collect();
        for account in accounts {
            let empty = if let Some(parents) = self.state.parent_orders.get_mut(&account) {
                parents.retain(|_, parent| {
                    parent.linked_plan_id() != Some(plan_id)
                        || (retain_progress
                            && (parent.active_child_order_id().is_some()
                                || parent.filled_qty() > 0))
                });
                parents.is_empty()
            } else {
                false
            };
            if empty {
                self.state.parent_orders.remove(&account);
            }
        }
    }
}
