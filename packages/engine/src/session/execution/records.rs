//! 母单订单记录：成交推进、子单关联与撤单清除。

use super::*;

impl GameSession {
    /// 母单进度仅由撮合结算成功后的实际成交推进。
    pub(in crate::session) fn record_parent_order_fills(
        &mut self,
        code: &StockCode,
        fills: &[OrderFillSettlement],
    ) {
        let mut completed = Vec::new();
        for fill in fills {
            let Some(plan) = self
                .parent_orders
                .get_mut(&fill.account)
                .and_then(|plans| plans.get_mut(code))
            else {
                continue;
            };
            if plan.side != fill.side || plan.active_child_order_id != Some(fill.order_id) {
                continue;
            }
            let remaining_child_qty = plan
                .active_child_remaining_qty
                .expect("active parent-order id must carry its remaining quantity")
                .checked_sub(fill.qty)
                .expect("a parent-order fill cannot exceed its active child quantity");
            plan.filled_qty = plan
                .filled_qty
                .checked_add(fill.qty)
                .expect("a parent-order child cannot fill beyond u32 capacity");
            assert!(
                plan.filled_qty <= plan.target_qty,
                "a parent-order child filled beyond its target"
            );
            if remaining_child_qty == 0 {
                plan.active_child_order_id = None;
                plan.active_child_remaining_qty = None;
            } else {
                plan.active_child_remaining_qty = Some(remaining_child_qty);
            }
            if plan.filled_qty == plan.target_qty {
                completed.push((fill.account, code.clone()));
            }
        }
        for (account, code) in completed {
            let empty = {
                let plans = self
                    .parent_orders
                    .get_mut(&account)
                    .expect("completed parent-order owner must exist");
                plans.remove(&code);
                plans.is_empty()
            };
            if empty {
                self.parent_orders.remove(&account);
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
        let Some(plan) = self
            .parent_orders
            .get_mut(&account)
            .and_then(|plans| plans.get_mut(code))
        else {
            return;
        };
        if plan.side != side {
            return;
        }
        assert!(
            plan.active_child_order_id.is_none(),
            "parent-order accepted a second active child before the first resolved"
        );
        plan.active_child_order_id = Some(order_id);
        plan.active_child_remaining_qty = Some(qty);
    }

    pub(in crate::session) fn record_parent_order_canceled(
        &mut self,
        account: AccountId,
        code: &StockCode,
        order_id: OrderId,
    ) {
        if let Some(plan) = self
            .parent_orders
            .get_mut(&account)
            .and_then(|plans| plans.get_mut(code))
        {
            if plan.active_child_order_id == Some(order_id) {
                plan.active_child_order_id = None;
                plan.active_child_remaining_qty = None;
            }
        }
    }
}
