use super::*;

use crate::session::pipeline::npc_split::SessionReadView;

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
        code: StockId,
    },
    Replace {
        old_order_id: OrderId,
        new_intent: Intent,
    },
}

impl GameSession {
    pub(in crate::session) fn plan_npc_working_order_reconciliation(
        &self,
        desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
    ) -> ReconciliationPlan {
        Self::plan_npc_working_order_reconciliation_on(
            &self.read_view(),
            desired,
            phase,
            scope,
            working,
        )
    }

    /// 读视图版本（queue 链转换目标形态）：会话读集仅 tick/setup（竞价不可撤销
    /// 窗口判定）与 markets（压制判定所需的行情快照），其余依赖经参数传入。
    /// 读视图版本（queue 链转换目标形态）：会话读集仅 tick/setup（竞价不可撤销
    /// 窗口判定）与 markets（压制判定所需的行情快照），其余依赖经参数传入。
    pub(in crate::session) fn plan_npc_working_order_reconciliation_on(
        view: &SessionReadView<'_>,
        desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
    ) -> ReconciliationPlan {
        plan_npc_working_order_reconciliation_standalone(
            view.tick % view.setup.ticks_per_day < view.setup.auction_ticks / 3,
            view.markets,
            desired,
            phase,
            scope,
            working,
        )
    }
}

/// 纯参数版本（T1·两阶段审计：决策任务内可调——竞价可撤窗口布尔与行情
/// 快照由调用方捕获一次传入，任务不借用会话视图）。
pub(in crate::session) fn plan_npc_working_order_reconciliation_standalone(
    cancelable_auction_window: bool,
    markets: &std::collections::BTreeMap<StockId, Market>,
    mut desired: Vec<Intent>,
    phase: TradingPhase,
    scope: ReconcileScope,
    working: WorkingOrderSlices<'_>,
) -> ReconciliationPlan {
    let in_scope = |code: &StockId| match &scope {
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
            for (code, order) in working.auction {
                if !in_scope(code) {
                    continue;
                }
                if take_exact(&mut desired, code, order.side, order.limit, order.qty) {
                    decisions.push(WorkingOrderDecision::Keep { order_id: OrderId(order.order_id) });
                } else if cancelable_auction_window {
                    decisions.push(WorkingOrderDecision::Cancel {
                        order_id: OrderId(order.order_id),
                        code: code.clone(),
                    });
                } else {
                    let market = markets.get(code).expect("working auction stock exists");
                    suppress_noncancelable(
                        &mut desired,
                        code,
                        order.side,
                        order.limit,
                        &scope,
                        market,
                    );
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

fn intent_stock_side(intent: &Intent) -> Option<(StockId, Side)> {
    match intent {
        Intent::PlaceLimit { code, side, .. } | Intent::PlaceMarket { code, side, .. } => {
            Some((code.clone(), *side))
        }
        Intent::Cancel { .. } => None,
    }
}

fn same_side(intent: &Intent, code: &StockId, side: Side) -> bool {
    matches!(intent, Intent::PlaceLimit { code: target, side: target_side, .. } if target == code && *target_side == side)
}

fn take_exact(
    desired: &mut Vec<Intent>,
    code: &StockId,
    side: Side,
    price: Money,
    qty: u32,
) -> bool {
    let Some(index) = desired.iter().position(|intent| matches!(intent, Intent::PlaceLimit { code: target, side: target_side, price: LimitPrice::Fixed(target_price), qty: target_qty } if target == code && *target_side == side && *target_price == price && *target_qty == qty)) else { return false; };
    desired.remove(index);
    true
}

fn crosses(
    intent: &Intent,
    code: &StockId,
    resting_side: Side,
    resting_price: Money,
    market: &Market,
) -> bool {
    match intent {
        Intent::PlaceLimit {
            code: target,
            side,
            price,
            ..
        } if target == code && *side != resting_side => {
            let resolved = market
                .resolve_limit_price(*side, *price, false)
                .unwrap_or_else(|error| {
                    panic!(
                        "auction crossing price resolution failed for {}: {error}",
                        code.0
                    )
                });
            match side {
                Side::Buy => resolved >= resting_price,
                Side::Sell => resolved <= resting_price,
            }
        }
        Intent::PlaceMarket {
            code: target, side, ..
        } => target == code && *side != resting_side,
        Intent::PlaceLimit { .. } | Intent::Cancel { .. } => false,
    }
}

fn suppress_noncancelable(
    desired: &mut Vec<Intent>,
    code: &StockId,
    side: Side,
    price: Money,
    scope: &ReconcileScope,
    market: &Market,
) {
    let reviewed = matches!(scope, ReconcileScope::ReviewedStocks(stocks) if stocks.contains(code));
    desired.retain(|intent| {
        (!reviewed || intent_stock_side(intent).is_none_or(|(target, _)| target != *code))
            && !same_side(intent, code, side)
            && !crosses(intent, code, side, price, market)
    });
}
