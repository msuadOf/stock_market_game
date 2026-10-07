//! 构建账户自身的工作单索引、可用现金预算与持仓视图。

use super::*;
use rayon::prelude::*;

use super::pipeline::npc_split::SessionReadView;

/// working_orders_for_accounts 的读视图版本（与 GameSession 同名方法逐行同源）。
pub(in crate::session) fn working_orders_for_accounts_on(
    view: &SessionReadView,
    owners: &BTreeSet<AccountId>,
) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
    let (mut continuous_stocks, mut auction_stocks) = rayon::join(
        || {
            view.markets
                .par_iter()
                .map(|(code, market)| (code.clone(), market.resting_orders_for_owners(owners)))
                .collect::<Vec<_>>()
        },
        || {
            view.auction_orders
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

impl GameSession {
    /// 按股票并行读取本轮观察账户的工作单，再归并为账户视图。
    pub(super) fn working_orders_for_accounts(
        &self,
        owners: &BTreeSet<AccountId>,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) { working_orders_for_accounts_on(&self.read_view(), owners) }

    #[cfg(test)]
    pub(super) fn working_orders_by_account(
        &self,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
        let owners = self.state.accounts.keys().copied().collect();
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
        // 目录按代码升序分配 id："600888"=0、"600889"=1，与枚举下标一致。
        for (stock_index, _code) in ["600888", "600889"].into_iter().enumerate() {
            let code = StockId(stock_index as u32);
            for (owner, offset) in [(AccountId(0), 0_u64), (AccountId(1), 1_u64)] {
                let seq = stock_index as u64 * 10 + offset;
                session
                    .state
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
                    .state
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
                    .map(|(code, order)| (code.0, order.seq))
                    .collect::<Vec<_>>(),
                vec![(0, owner.0), (1, owner.0)]
            );
            assert_eq!(
                one.1[&owner]
                    .iter()
                    .map(|(code, order)| (code.0, order.order_id))
                    .collect::<Vec<_>>(),
                vec![(0, owner.0), (1, 10 + owner.0)]
            );
        }
    }
}
