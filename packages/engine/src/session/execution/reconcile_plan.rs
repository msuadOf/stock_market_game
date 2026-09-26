use super::*;

#[derive(Clone, Debug)]
pub(in crate::session) struct ReconciliationPlan {
    pub(in crate::session) decisions: Vec<WorkingOrderDecision>,
    pub(in crate::session) residual_intents: Vec<Intent>,
}

#[derive(Clone, Debug)]
pub(in crate::session) enum WorkingOrderDecision {
    Keep {
        order_id: OrderId,
    },
    Cancel {
        order_id: OrderId,
        code: StockCode,
    },
    Replace {
        old_order_id: OrderId,
        new_intent: Intent,
    },
}

impl GameSession {
    #[cfg(test)]
    pub(in crate::session) fn plan_npc_working_orders(
        &self,
        account: AccountId,
        desired: Vec<Intent>,
        phase: TradingPhase,
    ) -> ReconciliationPlan {
        let (continuous, auction) = self.working_orders_by_account();
        self.plan_npc_working_order_reconciliation(
            desired,
            phase,
            ReconcileScope::AllWorkingOrders,
            WorkingOrderSlices {
                continuous: continuous.get(&account).map(Vec::as_slice).unwrap_or(&[]),
                auction: auction.get(&account).map(Vec::as_slice).unwrap_or(&[]),
            },
        )
    }

    #[cfg(test)]
    pub(in crate::session) fn plan_npc_working_orders_from_index(
        &self,
        desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
    ) -> ReconciliationPlan {
        self.plan_npc_working_order_reconciliation(desired, phase, scope, working)
    }

    pub(in crate::session) fn plan_npc_working_order_reconciliation(
        &self,
        mut desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
    ) -> ReconciliationPlan {
        let in_scope = |code: &StockCode| match &scope {
            ReconcileScope::AllWorkingOrders => true,
            ReconcileScope::ReviewedStocks(stocks) => stocks.contains(code),
        };
        let mut decisions = Vec::new();
        match phase {
            TradingPhase::Continuous => {
                for (code, order) in working.continuous {
                    if !in_scope(code) {
                        continue;
                    }
                    if take_exact(&mut desired, code, order.side, order.price, order.qty) {
                        decisions.push(WorkingOrderDecision::Keep { order_id: order.id });
                    } else if let Some(intent) = desired
                        .iter()
                        .find(|intent| same_side(intent, code, order.side))
                        .cloned()
                    {
                        decisions.push(WorkingOrderDecision::Replace {
                            old_order_id: order.id,
                            new_intent: intent,
                        });
                    } else {
                        decisions.push(WorkingOrderDecision::Cancel {
                            order_id: order.id,
                            code: code.clone(),
                        });
                    }
                }
            }
            TradingPhase::CallAuction => {
                let cancelable =
                    self.tick % self.setup.ticks_per_day < self.setup.auction_ticks / 3;
                for (code, order) in working.auction {
                    if !in_scope(code) {
                        continue;
                    }
                    if take_exact(&mut desired, code, order.side, order.limit, order.qty) {
                        decisions.push(WorkingOrderDecision::Keep {
                            order_id: OrderId(order.order_id),
                        });
                    } else if cancelable {
                        decisions.push(WorkingOrderDecision::Cancel {
                            order_id: OrderId(order.order_id),
                            code: code.clone(),
                        });
                    } else {
                        suppress_noncancelable(&mut desired, code, order.side, order.limit, &scope);
                    }
                }
            }
            TradingPhase::ClosingAuction | TradingPhase::PreOpen => {}
        }
        ReconciliationPlan {
            decisions,
            residual_intents: desired,
        }
    }
}

fn intent_stock_side(intent: &Intent) -> Option<(StockCode, Side)> {
    match intent {
        Intent::PlaceLimit { code, side, .. } | Intent::PlaceMarket { code, side, .. } => {
            Some((code.clone(), *side))
        }
        Intent::Cancel { .. } => None,
    }
}

fn same_side(intent: &Intent, code: &StockCode, side: Side) -> bool {
    matches!(intent, Intent::PlaceLimit { code: target, side: target_side, .. } if target == code && *target_side == side)
}

fn take_exact(
    desired: &mut Vec<Intent>,
    code: &StockCode,
    side: Side,
    price: Money,
    qty: u32,
) -> bool {
    let Some(index) = desired.iter().position(|intent| matches!(intent, Intent::PlaceLimit { code: target, side: target_side, price: target_price, qty: target_qty } if target == code && *target_side == side && *target_price == price && *target_qty == qty)) else { return false; };
    desired.remove(index);
    true
}

fn crosses(intent: &Intent, code: &StockCode, resting_side: Side, resting_price: Money) -> bool {
    match intent {
        Intent::PlaceLimit {
            code: target,
            side,
            price,
            ..
        } if target == code && *side != resting_side => match side {
            Side::Buy => *price >= resting_price,
            Side::Sell => *price <= resting_price,
        },
        Intent::PlaceMarket {
            code: target, side, ..
        } => target == code && *side != resting_side,
        Intent::PlaceLimit { .. } | Intent::Cancel { .. } => false,
    }
}

fn suppress_noncancelable(
    desired: &mut Vec<Intent>,
    code: &StockCode,
    side: Side,
    price: Money,
    scope: &ReconcileScope,
) {
    let reviewed = matches!(scope, ReconcileScope::ReviewedStocks(stocks) if stocks.contains(code));
    desired.retain(|intent| {
        (!reviewed || intent_stock_side(intent).is_none_or(|(target, _)| target != *code))
            && !same_side(intent, code, side)
            && !crosses(intent, code, side, price)
    });
}
