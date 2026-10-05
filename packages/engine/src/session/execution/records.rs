//! 母单订单记录：成交推进、子单关联与撤单清除。

use super::*;

impl GameSession {
    /// 母单进度仅由撮合结算成功后的实际成交推进。
    pub(in crate::session) fn record_parent_order_fills(
        &mut self,
        code: &StockCode,
        fills: &[OrderFillSettlement],
    ) {
        let trading_day = self
            .stock_trading_day(code)
            .expect("filled parent stock trading day is valid");
        let mut completed = Vec::new();
        for fill in fills {
            #[cfg(feature = "simulation-diagnostics")]
            {
                let time = self.causal_time();
                self.state.causal.record_continuous_fill(
                    time,
                    fill.order_id,
                    fill.account,
                    code,
                    fill.qty,
                    fill.gross.cents(),
                );
            }
            let Some(plan) = self
                .state
                .parent_orders
                .get_mut(&fill.account)
                .and_then(|plans| plans.get_mut(code))
            else {
                continue;
            };
            let Some(transition) =
                plan.record_fill_after_settlement(fill.side, fill.order_id, fill.qty)
            else {
                continue;
            };
            let pending_event = transition.linked_plan_id.map(|plan_id| {
                crate::session::plan_execution::PendingPlanEvent::Filled {
                    plan_id,
                    order_id: fill.order_id,
                    qty: fill.qty,
                    child_complete: transition.child_complete,
                    trading_day,
                }
            });
            if transition.completed_unlinked {
                completed.push((fill.account, code.clone()));
            }
            if let Some(event) = pending_event {
                self.state.pending_plan_events.push(event);
            }
        }
        for (account, code) in completed {
            let empty = {
                let plans = self
                    .state
                    .parent_orders
                    .get_mut(&account)
                    .expect("completed parent-order owner must exist");
                plans.remove(&code);
                plans.is_empty()
            };
            if empty {
                self.state.parent_orders.remove(&account);
            }
        }
    }

    /// 订单已被权威路由接受后，将其与当前母单的唯一在途子单关联。
    pub(in crate::session) fn record_parent_order_submission(
        &mut self,
        account: AccountId,
        code: &StockCode,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) {
        let trading_day = self
            .stock_trading_day(code)
            .expect("accepted parent stock trading day is valid");
        let Some(plan) = self
            .state
            .parent_orders
            .get_mut(&account)
            .and_then(|plans| plans.get_mut(code))
        else {
            return;
        };
        if !plan.record_submission(side, order_id, qty) {
            return;
        }
        let pending_event = plan.linked_plan_id.map(|plan_id| {
            crate::session::plan_execution::PendingPlanEvent::Accepted {
                plan_id,
                order_id,
                trading_day,
            }
        });
        if let Some(event) = pending_event {
            self.state.pending_plan_events.push(event);
        }
    }

    pub(in crate::session) fn record_parent_order_canceled(
        &mut self,
        account: AccountId,
        code: &StockCode,
        order_id: OrderId,
    ) {
        if let Some(plan) = self
            .state
            .parent_orders
            .get_mut(&account)
            .and_then(|plans| plans.get_mut(code))
        {
            plan.clear_matching_child(order_id);
        }
    }
}
