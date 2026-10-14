//! 构建账户自身的工作单索引、可用现金预算与持仓视图。

use super::*;
use crate::compute::parallel::*;

use super::pipeline::npc_split::SessionReadView;

/// working_orders_for_accounts 的读视图版本（与 GameSession 同名方法逐行同源）。
pub(in crate::session) fn working_orders_for_accounts_on(
    view: &SessionReadView,
    owners: &BTreeSet<AccountId>,
) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
    let mut stocks = view.compute.parallel(|| {
        view.markets
            .par_iter()
            .map(|(code, market)| {
                let mut orders = market
                    .resting_order_refs()
                    .filter(|order| owners.contains(&order.owner))
                    .collect::<Vec<_>>();
                // 仍按既有arrival seq组织账户DTO；只排序引用，不提前clone撮合Order。
                orders.sort_by_key(|order| order.seq);
                (*code, market.matching_mode(), orders)
            })
            .collect::<Vec<_>>()
    });
    stocks.sort_by_key(|(code, _, _)| *code);
    let mut continuous = ContinuousOrdersByAccount::new();
    let mut auction = AuctionOrdersByAccount::new();
    for (code, mode, orders) in stocks {
        for order in orders {
            if mode == crate::orderbook::MatchingMode::Auction {
                auction.entry(order.owner).or_default().push((
                    code,
                    AuctionOrderSnap {
                        owner: order.owner,
                        side: order.side,
                        limit: order.price,
                        qty: order.qty,
                        order_id: order.id.0,
                    },
                ));
            } else {
                continuous
                    .entry(order.owner)
                    .or_default()
                    // 最终跨阶段owned持有仍必要；直接从book借读构建，避免中间Order快照。
                    .push((code, order.clone()));
            }
        }
    }
    (continuous, auction)
}

impl GameSession {
    /// 按股票并行读取本轮观察账户的工作单，再归并为账户视图。
    pub(super) fn working_orders_for_accounts(
        &self,
        owners: &BTreeSet<AccountId>,
    ) -> (ContinuousOrdersByAccount, AuctionOrdersByAccount) {
        working_orders_for_accounts_on(&self.read_view(), owners)
    }

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
        // 统一 book：同一会话内两只股票显式处于不同 venue，验证账户视图跨市场归并。
        for (stock_index, mode) in [
            (0, crate::orderbook::MatchingMode::Continuous),
            (1, crate::orderbook::MatchingMode::Auction),
        ] {
            let code = StockId(stock_index as u32);
            for (owner, offset) in [(AccountId(0), 0_u64), (AccountId(1), 1_u64)] {
                let seq = stock_index as u64 * 10 + offset;
                let market = session.state.markets.get_mut(&code).unwrap();
                market.set_matching_mode(mode).unwrap();
                market
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
            }
        }

        let one = crate::compute::ComputeContext::from_workers(1)
            .install(|| session.working_orders_by_account());
        let four = crate::compute::ComputeContext::from_workers(4)
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
                vec![(0, owner.0)]
            );
            assert_eq!(
                one.1[&owner]
                    .iter()
                    .map(|(code, order)| (code.0, order.order_id))
                    .collect::<Vec<_>>(),
                vec![(1, 11 + owner.0)]
            );
        }
    }
}
