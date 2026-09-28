//! 账户自身视图接缝：工作单索引、可用现金预算与持仓视图构建（W1-Task 3 抽出）。

use super::*;
use rayon::prelude::*;

impl GameSession {
    /// 按股票并行读取本轮观察账户的工作单，再归并为账户视图。
    pub(super) fn working_orders_for_accounts(
        &self,
        owners: &BTreeSet<AccountId>,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
        let (mut continuous_stocks, mut auction_stocks) = rayon::join(
            || {
                self.markets
                    .par_iter()
                    .map(|(code, market)| (code.clone(), market.resting_orders_for_owners(owners)))
                    .collect::<Vec<_>>()
            },
            || {
                self.auction_orders
                    .par_iter()
                    .map(|(code, orders)| {
                        (
                            code.clone(),
                            orders
                                .iter()
                                .filter(|order| owners.contains(&order.owner))
                                .cloned()
                                .collect::<Vec<_>>(),
                        )
                    })
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

    #[cfg(test)]
    pub(super) fn working_orders_by_account(
        &self,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
        let owners = self.accounts.keys().copied().collect();
        self.working_orders_for_accounts(&owners)
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
                        order_id: seq,
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
                    .map(|(code, order)| (code.0.as_str(), order.order_id))
                    .collect::<Vec<_>>(),
                vec![("600888", owner.0), ("600889", 10 + owner.0)]
            );
        }
    }
}
