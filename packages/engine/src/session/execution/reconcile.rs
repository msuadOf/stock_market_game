//! NPC 工作单对齐：目标报价与在簿/在队旧单的撤换与压制规则。

use super::*;

impl GameSession {
    /// 将 NPC 在本次真实观察点产出的目标报价与现有工作单对齐。
    ///
    /// 完全相同的限价单继续排队；已经失去信号或参数变化的旧单撤销。集合竞价
    /// 09:20 之后依法不可撤，因而保留旧单，并由路由层阻止同方向报价继续堆叠。
    /// 未到个体观察节奏时不调用本函数，旧报价保持不变。
    #[cfg(test)]
    pub(in crate::session) fn reconcile_npc_working_orders(
        &mut self,
        account: AccountId,
        desired: Vec<Intent>,
        phase: TradingPhase,
        events: &mut Vec<Event>,
    ) -> Vec<Intent> {
        let (mut continuous, mut auction) = self.working_orders_by_account();
        self.reconcile_npc_working_orders_from_index(
            account,
            desired,
            phase,
            ReconcileScope::AllWorkingOrders,
            WorkingOrderSlices {
                continuous: continuous.remove(&account).as_deref().unwrap_or(&[]),
                auction: auction.remove(&account).as_deref().unwrap_or(&[]),
            },
            events,
        )
    }

    pub(in crate::session) fn reconcile_npc_working_orders_from_index(
        &mut self,
        account: AccountId,
        mut desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
        events: &mut Vec<Event>,
    ) -> Vec<Intent> {
        let desired_stock_sides: Vec<(StockCode, Side)> = desired
            .iter()
            .filter_map(|intent| match intent {
                Intent::PlaceLimit { code, side, .. } | Intent::PlaceMarket { code, side, .. } => {
                    Some((code.clone(), *side))
                }
                Intent::Cancel { .. } => None,
            })
            .collect();
        let in_scope = |code: &StockCode, side: Side| match &scope {
            ReconcileScope::AllWorkingOrders => true,
            ReconcileScope::DesiredStockSides => desired_stock_sides
                .iter()
                .any(|(desired_code, desired_side)| desired_code == code && *desired_side == side),
            ReconcileScope::ReviewedStocks(stocks) => stocks.contains(code),
        };

        fn take_exact_limit(
            desired: &mut Vec<Intent>,
            code: &StockCode,
            side: Side,
            price: Money,
            qty: u32,
        ) -> bool {
            let Some(index) = desired.iter().position(|intent| {
                matches!(
                    intent,
                    Intent::PlaceLimit {
                        code: desired_code,
                        side: desired_side,
                        price: desired_price,
                        qty: desired_qty,
                    } if desired_code == code
                        && *desired_side == side
                        && *desired_price == price
                        && *desired_qty == qty
                )
            }) else {
                return false;
            };
            desired.remove(index);
            true
        }

        fn intent_crosses_resting_order(
            intent: &Intent,
            code: &StockCode,
            resting_side: Side,
            resting_price: Money,
        ) -> bool {
            match intent {
                Intent::PlaceLimit {
                    code: desired_code,
                    side: desired_side,
                    price,
                    ..
                } if desired_code == code && *desired_side != resting_side => match desired_side {
                    Side::Buy => *price >= resting_price,
                    Side::Sell => *price <= resting_price,
                },
                Intent::PlaceMarket {
                    code: desired_code,
                    side: desired_side,
                    ..
                } => desired_code == code && *desired_side != resting_side,
                Intent::PlaceLimit { .. } | Intent::Cancel { .. } => false,
            }
        }

        match phase {
            TradingPhase::Continuous => {
                for (code, order) in working.continuous {
                    let crossed_by_own_intent = desired.iter().any(|intent| {
                        intent_crosses_resting_order(intent, code, order.side, order.price)
                    });
                    if !in_scope(code, order.side) && !crossed_by_own_intent {
                        continue;
                    }
                    if !take_exact_limit(&mut desired, code, order.side, order.price, order.qty) {
                        self.cancel_continuous_order(account, code.clone(), order.id, events);
                    }
                }
            }
            TradingPhase::CallAuction => {
                let cancelable =
                    self.tick % self.setup.ticks_per_day < self.setup.auction_ticks / 3;
                for (code, order) in working.auction {
                    let crossed_by_own_intent = desired.iter().any(|intent| {
                        intent_crosses_resting_order(intent, code, order.side, order.limit)
                    });
                    if !in_scope(code, order.side) && !crossed_by_own_intent {
                        continue;
                    }
                    if take_exact_limit(&mut desired, code, order.side, order.limit, order.qty) {
                        continue;
                    }
                    if cancelable {
                        self.cancel_auction_order(
                            account,
                            code.clone(),
                            OrderId(order.arrival_seq),
                            events,
                        );
                    } else {
                        // 09:20 后旧单不可撤。若本轮已重新审视整只股票，就不能在旧目标
                        // 仍生效时执行相反的新目标；否则只抑制同向堆叠和会自成交的报价。
                        let reviewed_stock = matches!(
                            &scope,
                            ReconcileScope::ReviewedStocks(stocks) if stocks.contains(code)
                        );
                        desired.retain(|intent| {
                            (!reviewed_stock
                                || !matches!(
                                    intent,
                                    Intent::PlaceLimit { code: desired_code, .. }
                                        | Intent::PlaceMarket { code: desired_code, .. }
                                        if desired_code == code
                                ))
                                && !matches!(
                                    intent,
                                    Intent::PlaceLimit {
                                        code: desired_code,
                                        side: desired_side,
                                        ..
                                    } if desired_code == code && *desired_side == order.side
                                )
                                && !intent_crosses_resting_order(
                                    intent,
                                    code,
                                    order.side,
                                    order.limit,
                                )
                        });
                    }
                }
            }
            TradingPhase::ClosingAuction => {}
            TradingPhase::PreOpen => {}
        }
        desired
    }
}
