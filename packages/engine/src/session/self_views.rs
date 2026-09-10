//! 账户自身视图接缝：工作单索引、可用现金预算与持仓视图构建（W1-Task 3 抽出）。

use super::*;

impl GameSession {
    /// 构建账户自身视图：可用现金 + 每只持仓的 [`PositionView`]。
    ///
    /// `sellable` 取 `Position::sellable()`（持仓 − T+1 锁定）；`cost_price` 取派生成本价。
    /// 同样产 owned [`SelfView`]（账户不存在时返回空视图，调用方仅对已知 id 取）。
    #[cfg(test)]
    pub(super) fn build_self_view(&self, id: AccountId) -> SelfView {
        let (continuous, auction) = self.working_orders_by_account();
        self.build_self_views_for(&[id], self.phase(), &continuous, &auction)
            .remove(&id)
            .unwrap_or(SelfView {
                cash: Money::ZERO,
                positions: BTreeMap::new(),
            })
    }

    /// 单次遍历全部订单，按账户建立当前 tick 共享的工作单索引。
    pub(super) fn working_orders_by_account(
        &self,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
        let mut continuous = ContinuousOrdersByAccount::new();
        for (code, market) in &self.markets {
            for order in market.resting_orders() {
                continuous
                    .entry(order.owner)
                    .or_default()
                    .push((code.clone(), order));
            }
        }
        let mut auction = AuctionOrdersByAccount::new();
        for (code, orders) in &self.auction_orders {
            for order in orders {
                auction
                    .entry(order.owner)
                    .or_default()
                    .push((code.clone(), order.clone()));
            }
        }
        (continuous, auction)
    }

    /// 只为本 tick 到期的 NPC 构建自身视图；未到期个体不会承担持仓复制成本。
    pub(super) fn build_self_views_for(
        &self,
        ids: &[AccountId],
        phase: TradingPhase,
        continuous: &ContinuousOrdersByAccount,
        auction: &AuctionOrdersByAccount,
    ) -> BTreeMap<AccountId, SelfView> {
        let auction_cancelable = phase == TradingPhase::CallAuction
            && self.tick % self.setup.ticks_per_day < self.setup.auction_ticks / 3;
        ids.iter()
            .map(|id| {
                let account = self
                    .accounts
                    .get(id)
                    .expect("self views may only be built for existing accounts");
                let mut reserved_cash = Money::ZERO;
                let mut replaceable_cash = Money::ZERO;
                let mut record = |required: Money, replaceable: bool| {
                    reserved_cash = reserved_cash
                        .add(required)
                        .expect("live order reservations must remain representable as Money");
                    if replaceable {
                        replaceable_cash = replaceable_cash
                            .add(required)
                            .expect("replaceable order reservations must remain representable");
                    }
                };
                for (_, order) in continuous.get(id).into_iter().flatten() {
                    let required = match order.side {
                        Side::Buy => buy_order_reservation(
                            &self.setup.config,
                            order.price,
                            order.qty,
                            order.filled_value,
                        ),
                        Side::Sell => sell_order_fee_reservation(
                            &self.setup.config,
                            order.price,
                            order.qty,
                            order.filled_value,
                        ),
                    }
                    .expect("validated continuous reservation must remain computable");
                    record(required, phase == TradingPhase::Continuous);
                }
                for (_, order) in auction.get(id).into_iter().flatten() {
                    let required = match order.side {
                        Side::Buy => buy_order_reservation(
                            &self.setup.config,
                            order.limit,
                            order.qty,
                            Money::ZERO,
                        ),
                        Side::Sell => sell_order_fee_reservation(
                            &self.setup.config,
                            order.limit,
                            order.qty,
                            Money::ZERO,
                        ),
                    }
                    .expect("validated auction reservation must remain computable");
                    record(required, auction_cancelable);
                }
                let available_cash = account
                    .cash
                    .sub(reserved_cash)
                    .and_then(|cash| cash.add(replaceable_cash))
                    .expect("validated live reservations cannot exceed account cash");
                let positions = account
                    .positions
                    .iter()
                    .map(|(code, position)| {
                        (
                            code.clone(),
                            PositionView {
                                qty: position.qty,
                                sellable_qty: position.sellable(),
                                cost_price: position.cost_price(),
                            },
                        )
                    })
                    .collect();
                (
                    *id,
                    SelfView {
                        cash: available_cash,
                        positions,
                    },
                )
            })
            .collect()
    }
}
