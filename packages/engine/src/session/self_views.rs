//! 账户自身视图接缝：工作单索引、可用现金预算与持仓视图构建（W1-Task 3 抽出）。

use super::*;
use rayon::prelude::*;

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

    /// 按股票并行读取工作单，再按股票代码归并为账户视图。
    pub(super) fn working_orders_by_account(
        &self,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
        let (mut continuous_stocks, mut auction_stocks) = rayon::join(
            || {
                self.markets
                    .par_iter()
                    .map(|(code, market)| (code.clone(), market.resting_orders()))
                    .collect::<Vec<_>>()
            },
            || {
                self.auction_orders
                    .par_iter()
                    .map(|(code, orders)| (code.clone(), orders.clone()))
                    .collect::<Vec<_>>()
            },
        );
        continuous_stocks.sort_by(|left, right| left.0.cmp(&right.0));
        auction_stocks.sort_by(|left, right| left.0.cmp(&right.0));

        let mut continuous = ContinuousOrdersByAccount::new();
        for (code, orders) in continuous_stocks {
            for order in orders {
                continuous
                    .entry(order.owner)
                    .or_default()
                    .push((code.clone(), order));
            }
        }
        let mut auction = AuctionOrdersByAccount::new();
        for (code, orders) in auction_stocks {
            for order in orders {
                auction
                    .entry(order.owner)
                    .or_default()
                    .push((code.clone(), order));
            }
        }
        (continuous, auction)
    }

    /// 只为本 tick 到期的 NPC 构建自身视图；未到期个体不会承担持仓复制成本。
    #[cfg(test)]
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
                    let required = live_cash_reservation(
                        &self.setup.config,
                        order.side,
                        order.price,
                        order.qty,
                        order.filled_value,
                    )
                    .expect("validated continuous reservation must remain computable");
                    record(required, phase == TradingPhase::Continuous);
                }
                for (_, order) in auction.get(id).into_iter().flatten() {
                    let required = live_cash_reservation(
                        &self.setup.config,
                        order.side,
                        order.limit,
                        order.qty,
                        Money::ZERO,
                    )
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

#[cfg(test)]
mod parallel_index_tests {
    use super::*;

    #[test]
    fn stock_parallel_working_order_index_keeps_account_and_stock_order() {
        let mut session = GameSession::new(
            super::super::npc_working_quote_tests::two_stock_quote_setup(),
            0x5EED,
        )
        .unwrap();
        for (stock_index, code) in ["600888", "600889"].into_iter().enumerate() {
            let code = StockCode(code.to_owned());
            for (owner, offset) in [(AccountId(0), 0_u64), (AccountId(1), 1_u64)] {
                let seq = stock_index as u64 * 10 + offset;
                session
                    .markets
                    .get_mut(&code)
                    .unwrap()
                    .place(Order {
                        id: OrderId(seq + 1),
                        side: Side::Buy,
                        price: Money::from_cents(900 + offset as i64),
                        qty: 100,
                        original_qty: 100,
                        filled_qty: 0,
                        filled_value: Money::ZERO,
                        owner,
                        seq,
                    })
                    .unwrap();
                session
                    .auction_orders
                    .entry(code.clone())
                    .or_default()
                    .push(AuctionOrderSnap {
                        owner,
                        side: Side::Buy,
                        limit: Money::from_cents(900 + offset as i64),
                        qty: 100,
                        arrival_seq: seq,
                    });
            }
        }

        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| session.working_orders_by_account());
        let four = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap()
            .install(|| session.working_orders_by_account());

        assert_eq!(
            serde_json::to_vec(&one).unwrap(),
            serde_json::to_vec(&four).unwrap()
        );
        for owner in [AccountId(0), AccountId(1)] {
            assert_eq!(
                one.0[&owner]
                    .iter()
                    .map(|(code, order)| (code.0.as_str(), order.seq))
                    .collect::<Vec<_>>(),
                vec![("600888", owner.0), ("600889", owner.0)]
            );
            assert_eq!(
                one.1[&owner]
                    .iter()
                    .map(|(code, order)| (code.0.as_str(), order.arrival_seq))
                    .collect::<Vec<_>>(),
                vec![("600888", owner.0), ("600889", 10 + owner.0)]
            );
        }
    }
}
