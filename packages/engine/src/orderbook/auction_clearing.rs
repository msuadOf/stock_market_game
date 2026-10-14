//! 从轻量 Order 投影选取集合竞价价格，不修改委托或 Envelope。
//! 算法承接既有沪深差异；深市收盘仍用昨收的已登记简化不在结构重构中改变。

use super::{AuctionClearingReject, Money, OrderError, Side};
use crate::StockExchange;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClearingOrder {
    pub(crate) side: Side,
    pub(crate) limit: Money,
    pub(crate) qty: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClearingSelection {
    pub(crate) price: Money,
    pub(crate) volume: u64,
    pub(crate) imbalance: u64,
}

pub(crate) fn select_clearing(
    orders: &[ClearingOrder],
    previous_close: Money,
    exchange: StockExchange,
    price_tick: Money,
) -> Result<Option<ClearingSelection>, OrderError> {
    let candidates: BTreeSet<Money> = orders.iter().map(|order| order.limit).collect();
    let mut stats = Vec::with_capacity(candidates.len());
    for price in candidates {
        if let Some(candidate) = candidate_stats(orders, price)? {
            stats.push(candidate);
        }
    }
    let Some(max_volume) = stats.iter().map(|candidate| candidate.result.volume).max() else {
        return Ok(None);
    };
    stats.retain(|candidate| candidate.result.volume == max_volume);
    match exchange {
        StockExchange::Shanghai => {
            let min_imbalance = stats
                .iter()
                .map(|candidate| candidate.result.imbalance)
                .min()
                .ok_or_else(|| rejected(AuctionClearingReject::SelectionMismatch))?;
            stats.retain(|candidate| candidate.result.imbalance == min_imbalance);
            let low = stats
                .iter()
                .map(|candidate| candidate.result.price)
                .min()
                .ok_or_else(|| rejected(AuctionClearingReject::SelectionMismatch))?;
            let high = stats
                .iter()
                .map(|candidate| candidate.result.price)
                .max()
                .ok_or_else(|| rejected(AuctionClearingReject::SelectionMismatch))?;
            let midpoint = rounded_midpoint(low, high, price_tick)?;
            Ok(candidate_stats(orders, midpoint)?.map(|candidate| candidate.result))
        }
        StockExchange::Shenzhen => {
            let min_difference = stats
                .iter()
                .map(|candidate| candidate.aggressive_difference)
                .min()
                .ok_or_else(|| rejected(AuctionClearingReject::SelectionMismatch))?;
            Ok(stats
                .into_iter()
                .filter(|candidate| candidate.aggressive_difference == min_difference)
                .min_by_key(|candidate| {
                    (
                        i128::from(candidate.result.price.cents())
                            .abs_diff(i128::from(previous_close.cents())),
                        candidate.result.price,
                    )
                })
                .map(|candidate| candidate.result))
        }
    }
}

pub(crate) fn checked_total_imbalance(orders: &[ClearingOrder]) -> Result<u64, OrderError> {
    let buy = checked_sum(
        orders
            .iter()
            .filter(|order| order.side == Side::Buy)
            .map(|order| order.qty),
    )?;
    let sell = checked_sum(
        orders
            .iter()
            .filter(|order| order.side == Side::Sell)
            .map(|order| order.qty),
    )?;
    Ok(buy.abs_diff(sell))
}

#[derive(Clone, Copy)]
struct CandidateStats {
    result: ClearingSelection,
    aggressive_difference: u64,
}

fn candidate_stats(
    orders: &[ClearingOrder],
    price: Money,
) -> Result<Option<CandidateStats>, OrderError> {
    let sum = |side, predicate: &dyn Fn(Money) -> bool| {
        checked_sum(
            orders
                .iter()
                .filter(|order| order.side == side && predicate(order.limit))
                .map(|order| order.qty),
        )
    };
    let buy = sum(Side::Buy, &|limit| limit >= price)?;
    let sell = sum(Side::Sell, &|limit| limit <= price)?;
    let volume = buy.min(sell);
    if volume == 0 {
        return Ok(None);
    }
    let aggressive_buy = sum(Side::Buy, &|limit| limit > price)?;
    let aggressive_sell = sum(Side::Sell, &|limit| limit < price)?;
    if aggressive_buy > volume || aggressive_sell > volume {
        return Ok(None);
    }
    Ok(Some(CandidateStats {
        result: ClearingSelection {
            price,
            volume,
            imbalance: buy.abs_diff(sell),
        },
        aggressive_difference: aggressive_buy.abs_diff(aggressive_sell),
    }))
}

fn checked_sum(quantities: impl IntoIterator<Item = u64>) -> Result<u64, OrderError> {
    quantities.into_iter().try_fold(0_u64, |total, qty| {
        total
            .checked_add(qty)
            .ok_or_else(|| rejected(AuctionClearingReject::QuantityOverflow))
    })
}

fn rounded_midpoint(low: Money, high: Money, tick: Money) -> Result<Money, OrderError> {
    let tick_cents = i128::from(tick.cents());
    if tick_cents <= 0 {
        return Err(rejected(AuctionClearingReject::InvalidPriceTick));
    }
    let sum = i128::from(low.cents()) + i128::from(high.cents());
    let cents = ((sum + tick_cents) / (tick_cents * 2)) * tick_cents;
    let cents = i64::try_from(cents).map_err(|_| rejected(AuctionClearingReject::PriceOverflow))?;
    Ok(Money::from_cents(cents))
}

fn rejected(reason: AuctionClearingReject) -> OrderError {
    OrderError::InvalidAuctionClearing { reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::{MatchingMode, OrderBook};
    use crate::{AccountId, Order, OrderId};

    fn order(side: Side, cents: i64, qty: u64) -> ClearingOrder {
        ClearingOrder {
            side,
            limit: Money::from_cents(cents),
            qty,
        }
    }

    fn selected(
        orders: &[ClearingOrder],
        previous_close: i64,
        exchange: StockExchange,
    ) -> ClearingSelection {
        select_clearing(
            orders,
            Money::from_cents(previous_close),
            exchange,
            Money::from_cents(1),
        )
        .expect("合法数量必须能选价")
        .expect("交叉竞价必须有清算结果")
    }

    #[test]
    fn shanghai_uses_the_midpoint_of_the_lowest_and_highest_remaining_candidates() {
        assert_eq!(
            selected(
                &[order(Side::Buy, 10_200, 200), order(Side::Sell, 9_800, 200)],
                10_000,
                StockExchange::Shanghai
            ),
            ClearingSelection {
                price: Money::from_cents(10_000),
                volume: 200,
                imbalance: 0
            }
        );
    }

    #[test]
    fn shanghai_rounds_a_positive_half_tick_up_to_the_price_tick() {
        let result = selected(
            &[
                order(Side::Buy, 10_001, 200),
                order(Side::Sell, 10_000, 200),
            ],
            10_000,
            StockExchange::Shanghai,
        );
        assert_eq!(result.price, Money::from_cents(10_001));
        assert_eq!(result.volume, 200);
    }

    #[test]
    fn shenzhen_prefers_the_smallest_price_better_quantity_difference_before_previous_close() {
        let result = selected(
            &[
                order(Side::Buy, 10_200, 300),
                order(Side::Buy, 10_100, 100),
                order(Side::Sell, 9_900, 100),
                order(Side::Sell, 10_000, 300),
                order(Side::Sell, 10_100, 100),
            ],
            10_000,
            StockExchange::Shenzhen,
        );
        assert_eq!(result.price, Money::from_cents(10_100));
        assert_eq!(result.volume, 400);
    }

    #[test]
    fn shenzhen_uses_previous_close_after_price_better_quantity_difference() {
        let result = selected(
            &[order(Side::Buy, 10_200, 200), order(Side::Sell, 9_800, 200)],
            10_100,
            StockExchange::Shenzhen,
        );
        assert_eq!(result.price, Money::from_cents(10_200));
    }

    #[test]
    fn shenzhen_uses_the_documented_lower_price_when_previous_close_distance_is_tied() {
        let result = selected(
            &[order(Side::Buy, 10_200, 200), order(Side::Sell, 9_800, 200)],
            10_000,
            StockExchange::Shenzhen,
        );
        assert_eq!(result.price, Money::from_cents(9_800));
    }

    #[test]
    fn uncrossed_orders_have_no_clearing_result_and_keep_their_checked_imbalance() {
        let orders = [order(Side::Buy, 9_900, 300), order(Side::Sell, 10_100, 200)];
        assert_eq!(
            select_clearing(
                &orders,
                Money::from_cents(10_000),
                StockExchange::Shanghai,
                Money::from_cents(1)
            )
            .unwrap(),
            None
        );
        assert_eq!(checked_total_imbalance(&orders).unwrap(), 100);
    }

    #[test]
    fn clearing_quantity_overflow_is_a_typed_error_not_no_clearing() {
        let orders = [
            order(Side::Buy, 10_100, u64::MAX),
            order(Side::Buy, 10_100, 1),
            order(Side::Sell, 10_000, 1),
        ];
        assert!(matches!(
            select_clearing(
                &orders,
                Money::from_cents(10_000),
                StockExchange::Shanghai,
                Money::from_cents(1)
            ),
            Err(OrderError::InvalidAuctionClearing {
                reason: AuctionClearingReject::QuantityOverflow
            })
        ));
    }

    #[test]
    fn indicative_imbalance_quantity_overflow_is_a_typed_error_not_wraparound() {
        let orders = [
            order(Side::Buy, 9_900, u64::MAX),
            order(Side::Buy, 9_800, 1),
            order(Side::Sell, 10_100, 1),
        ];
        assert!(matches!(
            checked_total_imbalance(&orders),
            Err(OrderError::InvalidAuctionClearing {
                reason: AuctionClearingReject::QuantityOverflow
            })
        ));
    }

    #[test]
    fn unified_book_clearing_matches_selector_on_both_exchanges_and_short_fixtures() {
        let mut random = crate::SplitMix64::new(7);
        for fixture in 0..128 {
            let orders = (0..8)
                .map(|index| {
                    let value = random.next_u64();
                    order(
                        if index % 2 == 0 {
                            Side::Buy
                        } else {
                            Side::Sell
                        },
                        998 + (value % 5) as i64,
                        1 + value % 7,
                    )
                })
                .collect::<Vec<_>>();
            for exchange in [StockExchange::Shanghai, StockExchange::Shenzhen] {
                let expected = select_clearing(
                    &orders,
                    Money::from_cents(1_000),
                    exchange,
                    Money::from_cents(1),
                )
                .unwrap();
                let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
                book.set_mode(MatchingMode::Auction).unwrap();
                for (index, input) in orders.iter().enumerate() {
                    book.place(Order {
                        id: OrderId(index as u64),
                        owner: AccountId(index as u64),
                        side: input.side,
                        price: input.limit,
                        qty: input.qty as u32,
                        original_qty: input.qty as u32,
                        filled_qty: 0,
                        filled_value: Money::ZERO,
                        seq: 0,
                    })
                    .unwrap();
                }
                let before = book.resting_orders();
                assert_eq!(
                    book.indicative_clearing_price(Money::from_cents(1_000), exchange)
                        .unwrap(),
                    expected.map(|result| result.price),
                    "fixture={fixture}, exchange={exchange:?}, orders={orders:?}"
                );
                assert_eq!(book.resting_orders(), before, "指示价不能执行 Fill");
                let cleared = book
                    .execute_auction_clearing(Money::from_cents(1_000), exchange)
                    .unwrap();
                assert_eq!(cleared.price, expected.map(|result| result.price));
                assert_eq!(
                    cleared.matched_volume,
                    expected.map_or(0, |result| result.volume)
                );
                assert_eq!(
                    cleared
                        .trades
                        .iter()
                        .map(|trade| u64::from(trade.qty))
                        .sum::<u64>(),
                    cleared.matched_volume
                );
                for remaining in book.resting_orders() {
                    remaining.validate_progress().unwrap();
                }
            }
        }
    }
}
