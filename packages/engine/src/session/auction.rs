//! 集合竞价委托路由、撤单、撮合与清算。

use super::*;

impl GameSession {
    pub(super) fn route_auction_intent(
        &mut self,
        acct: AccountId,
        intent: Intent,
        events: &mut Vec<Event>,
    ) {
        let (code, side, price, qty) = match intent {
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } => (code, side, price, qty),
            Intent::PlaceMarket { code, .. } => {
                events.push(Event::IntentRejected {
                    seq: self.next_seq(),
                    account: acct,
                    code,
                    reason: RejectionReason::AuctionLimitOrderRequired,
                });
                return;
            }
            Intent::Cancel { code, id } => {
                self.cancel_auction_order(acct, code, id, events);
                return;
            }
        };
        let Some(market) = self.markets.get(&code) else {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::UnknownStock,
            });
            return;
        };
        let price_limits = market
            .down_stop()
            .and_then(|down| market.up_stop().map(|up| (down, up)));
        let (down, up) = match price_limits {
            Ok(limits) => limits,
            Err(error) => {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: acct,
                    code,
                    reason: error.to_string(),
                });
                return;
            }
        };
        if price < down || price > up {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::LimitExceeded,
            });
            return;
        }
        let tick = self
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == code)
            .expect("market code must have a stock spec")
            .tick;
        if qty == 0 || price.cents() < 0 || price.cents() % tick.cents() != 0 {
            events.push(Event::SettlementError {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: format!("invalid auction limit order: price={price:?}, qty={qty}"),
            });
            return;
        }
        // 集合竞价中的 NPC 同样只维护一张同向工作报价。09:15–09:20 可以合法撤换；
        // 进入不可撤单阶段后保留原报价，不再重复堆叠，也绝不绕过交易所撤单约束。
        if self
            .accounts
            .get(&acct)
            .is_some_and(|account| account.kind != AccountKind::Player)
        {
            let working: Vec<AuctionOrderSnap> = self
                .auction_orders
                .get(&code)
                .into_iter()
                .flatten()
                .filter(|order| order.owner == acct && order.side == side)
                .cloned()
                .collect();
            if working.len() == 1 && working[0].limit == price && working[0].qty == qty {
                return;
            }
            let cancelable_ticks = self.setup.auction_ticks / 3;
            if self.tick % self.setup.ticks_per_day >= cancelable_ticks && !working.is_empty() {
                return;
            }
            for order in working {
                self.cancel_auction_order(acct, code.clone(), OrderId(order.arrival_seq), events);
            }
        }
        if !self.prevalidate_order(
            OrderValidationInput {
                account: acct,
                code: &code,
                side,
                price,
                qty,
                is_market: false,
            },
            events,
        ) {
            return;
        }
        let arrival_seq = self.next_order_id;
        self.next_order_id += 1;
        self.auction_orders
            .entry(code.clone())
            .or_default()
            .push(AuctionOrderSnap {
                owner: acct,
                side,
                limit: price,
                qty,
                arrival_seq,
            });
        *self.auction_order_counts.entry(acct).or_default() += 1;
        self.record_retail_order_submitted(acct, code.clone(), side, OrderId(arrival_seq), qty);
        events.push(Event::OrderAccepted {
            seq: self.next_seq(),
            account: acct,
            code,
            id: OrderId(arrival_seq),
            side,
            price,
            remaining_qty: qty,
        });
    }

    /// A 股开盘集合竞价前 5 分钟（09:15–09:20）允许撤单；之后直到连续竞价
    /// 开始不接受撤单。`auction_ticks` 表示完整 15 分钟，因此前三分之一为可撤时段。
    pub(super) fn cancel_auction_order(
        &mut self,
        acct: AccountId,
        code: StockCode,
        id: OrderId,
        events: &mut Vec<Event>,
    ) {
        let day_tick = self.tick % self.setup.ticks_per_day;
        let cancelable_ticks = self.setup.auction_ticks / 3;
        if day_tick >= cancelable_ticks {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::AuctionOrderNotCancelable,
            });
            return;
        }
        let Some(orders) = self.auction_orders.get_mut(&code) else {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::OrderNotFound,
            });
            return;
        };
        let Some(index) = orders.iter().position(|order| order.arrival_seq == id.0) else {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::OrderNotFound,
            });
            return;
        };
        if orders[index].owner != acct {
            events.push(Event::IntentRejected {
                seq: self.next_seq(),
                account: acct,
                code,
                reason: RejectionReason::NotOrderOwner,
            });
            return;
        }
        let removed = orders.remove(index);
        self.decrement_auction_order_count(removed.owner);
        self.record_retail_order_canceled(acct, code.clone(), id, removed.qty);
        events.push(Event::OrderCanceled {
            seq: self.next_seq(),
            account: acct,
            code,
            id,
            remaining_qty: removed.qty,
        });
    }

    pub(super) fn complete_auction(&mut self, code: &StockCode, events: &mut Vec<Event>) {
        let previous_close = self
            .markets
            .get(code)
            .expect("code collected from markets must exist")
            .last_close();
        let stock = self
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .expect("code collected from markets must have a stock spec");
        let exchange = stock.exchange;
        let price_tick = stock.tick;
        // 此股委托从集合竞价队列移出；未成交部分将原子地转入连续竞价簿。
        // 先移出可避免在冻结校验时对同一委托重复计数。
        let orders = self.auction_orders.remove(code).unwrap_or_default();
        for order in &orders {
            self.decrement_auction_order_count(order.owner);
        }
        let Some(clearing) = clearing_result(&orders, previous_close, exchange, price_tick) else {
            let candidate_market = match stage_auction_remainders(
                self.markets
                    .get(code)
                    .expect("code collected from markets must exist"),
                &orders,
                &BTreeMap::new(),
                None,
            ) {
                Ok(market) => market,
                Err((account, reason)) => {
                    events.push(Event::SettlementError {
                        seq: self.next_seq(),
                        account,
                        code: code.clone(),
                        reason,
                    });
                    self.record_retail_auction_orders_aborted(code, &orders);
                    events.push(Event::AuctionCompleted {
                        seq: self.next_seq(),
                        tick: self.tick,
                        code: code.clone(),
                        opening_price: None,
                        matched_volume: 0,
                    });
                    return;
                }
            };
            if let Err((account, reason)) =
                self.validate_live_reservations_with(code, &candidate_market)
            {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account,
                    code: code.clone(),
                    reason,
                });
                self.record_retail_auction_orders_aborted(code, &orders);
            } else {
                self.markets.insert(code.clone(), candidate_market);
            }
            self.update_active_daily_candle(code, previous_close, 0);
            events.push(Event::AuctionCompleted {
                seq: self.next_seq(),
                tick: self.tick,
                code: code.clone(),
                opening_price: None,
                matched_volume: 0,
            });
            return;
        };

        let mut buys: Vec<AuctionOrderSnap> = orders
            .iter()
            .filter(|order| order.side == Side::Buy && order.limit >= clearing.price)
            .cloned()
            .collect();
        let mut sells: Vec<AuctionOrderSnap> = orders
            .iter()
            .filter(|order| order.side == Side::Sell && order.limit <= clearing.price)
            .cloned()
            .collect();
        buys.sort_by_key(|order| (std::cmp::Reverse(order.limit), order.arrival_seq));
        sells.sort_by_key(|order| (order.limit, order.arrival_seq));

        let mut buy_index = 0;
        let mut sell_index = 0;
        let mut matched_volume = 0_u64;
        let mut planned_trades = Vec::new();
        let mut filled_by_order: BTreeMap<u64, u32> = BTreeMap::new();
        while buy_index < buys.len() && sell_index < sells.len() {
            let qty = buys[buy_index].qty.min(sells[sell_index].qty);
            let buyer = buys[buy_index].owner;
            let seller = sells[sell_index].owner;
            let buy_order_id = OrderId(buys[buy_index].arrival_seq);
            let sell_order_id = OrderId(sells[sell_index].arrival_seq);
            let (maker, taker) = if buys[buy_index].arrival_seq < sells[sell_index].arrival_seq {
                (buyer, seller)
            } else {
                (seller, buyer)
            };
            let Some(next_matched_volume) = matched_volume.checked_add(u64::from(qty)) else {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account: buyer,
                    code: code.clone(),
                    reason: "auction matched volume overflow".to_string(),
                });
                self.record_retail_auction_orders_aborted(code, &orders);
                events.push(Event::AuctionCompleted {
                    seq: self.next_seq(),
                    tick: self.tick,
                    code: code.clone(),
                    opening_price: None,
                    matched_volume: 0,
                });
                return;
            };
            matched_volume = next_matched_volume;
            for order_id in [buy_order_id.0, sell_order_id.0] {
                let filled = filled_by_order.entry(order_id).or_default();
                let Some(next_filled) = filled.checked_add(qty) else {
                    events.push(Event::SettlementError {
                        seq: self.next_seq(),
                        account: buyer,
                        code: code.clone(),
                        reason: format!("auction fill quantity overflow for order {order_id}"),
                    });
                    self.record_retail_auction_orders_aborted(code, &orders);
                    events.push(Event::AuctionCompleted {
                        seq: self.next_seq(),
                        tick: self.tick,
                        code: code.clone(),
                        opening_price: None,
                        matched_volume: 0,
                    });
                    return;
                };
                *filled = next_filled;
            }
            planned_trades.push((
                buyer,
                seller,
                buy_order_id,
                sell_order_id,
                maker,
                taker,
                qty,
            ));
            buys[buy_index].qty -= qty;
            sells[sell_index].qty -= qty;
            if buys[buy_index].qty == 0 {
                buy_index += 1;
            }
            if sells[sell_index].qty == 0 {
                sell_index += 1;
            }
        }

        let mut candidate_market = match stage_auction_remainders(
            self.markets
                .get(code)
                .expect("code collected from markets must exist"),
            &orders,
            &filled_by_order,
            Some(clearing.price),
        ) {
            Ok(market) => market,
            Err((account, reason)) => {
                events.push(Event::SettlementError {
                    seq: self.next_seq(),
                    account,
                    code: code.clone(),
                    reason,
                });
                self.record_retail_auction_orders_aborted(code, &orders);
                events.push(Event::AuctionCompleted {
                    seq: self.next_seq(),
                    tick: self.tick,
                    code: code.clone(),
                    opening_price: None,
                    matched_volume: 0,
                });
                return;
            }
        };
        candidate_market.set_last_price(clearing.price);

        let mut account_backups: BTreeMap<AccountId, (Money, BTreeMap<StockCode, Position>)> =
            BTreeMap::new();
        for (buyer, seller, _, _, _, _, _) in &planned_trades {
            for participant in [*buyer, *seller] {
                if let Some(account) = self.accounts.get(&participant) {
                    account_backups
                        .entry(participant)
                        .or_insert_with(|| (account.cash, account.positions.clone()));
                }
            }
        }
        let mut order_fills = Vec::with_capacity(planned_trades.len() * 2);
        for (buyer, seller, buy_order_id, sell_order_id, _, _, qty) in &planned_trades {
            let gross = match clearing.price.mul_shares(*qty) {
                Ok(gross) => gross,
                Err(error) => {
                    events.push(Event::SettlementError {
                        seq: self.next_seq(),
                        account: *buyer,
                        code: code.clone(),
                        reason: error.to_string(),
                    });
                    self.record_retail_auction_orders_aborted(code, &orders);
                    return;
                }
            };
            order_fills.push(OrderFillSettlement {
                account: *buyer,
                side: Side::Buy,
                order_id: *buy_order_id,
                filled_value_before: Money::ZERO,
                gross,
                qty: *qty,
            });
            order_fills.push(OrderFillSettlement {
                account: *seller,
                side: Side::Sell,
                order_id: *sell_order_id,
                filled_value_before: Money::ZERO,
                gross,
                qty: *qty,
            });
        }
        let mut settlement_failure = self.settle_order_fills(code, &order_fills);
        if settlement_failure.is_none() {
            if let Err((account, reason)) =
                self.validate_live_reservations_with(code, &candidate_market)
            {
                settlement_failure = Some((account, AccountError::ReservationInvariant { reason }));
            }
        }
        if let Some((failed_account, error)) = settlement_failure {
            for (id, (cash, positions)) in account_backups {
                if let Some(account) = self.accounts.get_mut(&id) {
                    account.cash = cash;
                    account.positions = positions;
                }
            }
            events.push(Event::SettlementError {
                seq: self.next_seq(),
                account: failed_account,
                code: code.clone(),
                reason: error.to_string(),
            });
            self.record_retail_auction_orders_aborted(code, &orders);
            events.push(Event::AuctionCompleted {
                seq: self.next_seq(),
                tick: self.tick,
                code: code.clone(),
                opening_price: None,
                matched_volume: 0,
            });
            return;
        }

        self.record_retail_fill_experience(code, &order_fills, &account_backups);
        self.record_retail_order_fills(code, &order_fills);
        self.markets.insert(code.clone(), candidate_market);
        for (_, _, _, _, maker, taker, qty) in planned_trades {
            self.update_active_daily_candle(code, clearing.price, u64::from(qty));
            events.push(Event::Trade {
                seq: self.next_seq(),
                code: code.clone(),
                price: clearing.price,
                qty,
                maker,
                taker,
            });
        }
        events.push(Event::AuctionCompleted {
            seq: self.next_seq(),
            tick: self.tick,
            code: code.clone(),
            opening_price: Some(clearing.price),
            matched_volume,
        });
    }

    fn decrement_auction_order_count(&mut self, owner: AccountId) {
        let count = self
            .auction_order_counts
            .get_mut(&owner)
            .expect("every auction order owner must have a maintained count");
        *count -= 1;
        if *count == 0 {
            self.auction_order_counts.remove(&owner);
        }
    }

    fn record_retail_auction_orders_aborted(
        &mut self,
        code: &StockCode,
        orders: &[AuctionOrderSnap],
    ) {
        for order in orders {
            self.record_retail_order_aborted(
                order.owner,
                code.clone(),
                OrderId(order.arrival_seq),
                order.qty,
            );
        }
    }
}

/// 把集合竞价的未成交部分按原申报顺序转入连续竞价簿。
/// 任何意外交叉或数量不变式失败都显式返回，不会吞掉委托。
fn stage_auction_remainders(
    market: &Market,
    orders: &[AuctionOrderSnap],
    filled_by_order: &BTreeMap<u64, u32>,
    clearing_price: Option<Money>,
) -> Result<Market, (AccountId, String)> {
    let mut candidate = market.clone();
    let mut ordered = orders.to_vec();
    ordered.sort_by_key(|order| order.arrival_seq);
    for order in ordered {
        let filled_qty = filled_by_order
            .get(&order.arrival_seq)
            .copied()
            .unwrap_or(0);
        let Some(remaining_qty) = order.qty.checked_sub(filled_qty) else {
            return Err((
                order.owner,
                format!(
                    "auction fill quantity {filled_qty} exceeds original quantity {} for order {}",
                    order.qty, order.arrival_seq
                ),
            ));
        };
        if remaining_qty == 0 {
            continue;
        }
        let filled_value = if filled_qty == 0 {
            Money::ZERO
        } else {
            let price = clearing_price.ok_or_else(|| {
                (
                    order.owner,
                    format!(
                        "auction order {} has fills without a clearing price",
                        order.arrival_seq
                    ),
                )
            })?;
            price
                .mul_shares(filled_qty)
                .map_err(|error| (order.owner, error.to_string()))?
        };
        let result = candidate
            .place(Order {
                id: OrderId(order.arrival_seq),
                side: order.side,
                price: order.limit,
                qty: remaining_qty,
                original_qty: order.qty,
                filled_qty,
                filled_value,
                owner: order.owner,
                seq: order.arrival_seq,
            })
            .map_err(|error| (order.owner, error.to_string()))?;
        if !result.trades.is_empty() || result.resting.is_none() {
            return Err((
                order.owner,
                format!(
                    "auction remainder for order {} unexpectedly crossed during transfer",
                    order.arrival_seq
                ),
            ));
        }
    }
    Ok(candidate)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) struct ClearingResult {
    pub(super) price: Money,
    pub(super) volume: u64,
    pub(super) imbalance: u64,
}

#[derive(Copy, Clone)]
struct CandidateStats {
    result: ClearingResult,
    aggressive_difference: u64,
}

/// 沪深 A 股开盘集合竞价成交价规则。
///
/// 两市都先要求最大成交量、价优委托全部成交、成交价同价位至少一方全部成交。
/// 上海在最小未成交量仍有多价时取中间价；深圳改以价优买卖申报量差最小、再取最接近昨收。
pub(super) fn clearing_result(
    orders: &[AuctionOrderSnap],
    previous_close: Money,
    exchange: StockExchange,
    price_tick: Money,
) -> Option<ClearingResult> {
    let candidates: BTreeSet<Money> = orders.iter().map(|order| order.limit).collect();
    let mut stats: Vec<CandidateStats> = candidates
        .into_iter()
        .filter_map(|price| candidate_stats(orders, price))
        .collect();
    let max_volume = stats
        .iter()
        .map(|candidate| candidate.result.volume)
        .max()?;
    stats.retain(|candidate| candidate.result.volume == max_volume);

    match exchange {
        StockExchange::Shanghai => {
            let min_unmatched = stats
                .iter()
                .map(|candidate| candidate.result.imbalance)
                .min()?;
            stats.retain(|candidate| candidate.result.imbalance == min_unmatched);
            let low = stats.iter().map(|candidate| candidate.result.price).min()?;
            let high = stats.iter().map(|candidate| candidate.result.price).max()?;
            let midpoint = rounded_midpoint(low, high, price_tick)?;
            candidate_stats(orders, midpoint).map(|candidate| candidate.result)
        }
        StockExchange::Shenzhen => {
            let min_difference = stats
                .iter()
                .map(|candidate| candidate.aggressive_difference)
                .min()?;
            stats
                .into_iter()
                .filter(|candidate| candidate.aggressive_difference == min_difference)
                .min_by_key(|candidate| {
                    (
                        i128::from(candidate.result.price.cents())
                            .abs_diff(i128::from(previous_close.cents())),
                        candidate.result.price,
                    )
                })
                .map(|candidate| candidate.result)
        }
    }
}

fn candidate_stats(orders: &[AuctionOrderSnap], price: Money) -> Option<CandidateStats> {
    let sum = |side: Side, predicate: &dyn Fn(Money) -> bool| {
        orders
            .iter()
            .filter(|order| order.side == side && predicate(order.limit))
            .try_fold(0_u64, |total, order| {
                total.checked_add(u64::from(order.qty))
            })
    };
    let buy = sum(Side::Buy, &|limit| limit >= price)?;
    let sell = sum(Side::Sell, &|limit| limit <= price)?;
    let volume = buy.min(sell);
    if volume == 0 {
        return None;
    }
    let aggressive_buy = sum(Side::Buy, &|limit| limit > price)?;
    let aggressive_sell = sum(Side::Sell, &|limit| limit < price)?;
    if aggressive_buy > volume || aggressive_sell > volume {
        return None;
    }
    Some(CandidateStats {
        result: ClearingResult {
            price,
            volume,
            imbalance: buy.abs_diff(sell),
        },
        aggressive_difference: aggressive_buy.abs_diff(aggressive_sell),
    })
}

fn rounded_midpoint(low: Money, high: Money, tick: Money) -> Option<Money> {
    let tick_cents = i128::from(tick.cents());
    if tick_cents <= 0 {
        return None;
    }
    let sum = i128::from(low.cents()).checked_add(i128::from(high.cents()))?;
    let denominator = tick_cents.checked_mul(2)?;
    let rounded_ticks = sum.checked_add(tick_cents)?.checked_div(denominator)?;
    let cents = rounded_ticks.checked_mul(tick_cents)?;
    Some(Money::from_cents(i64::try_from(cents).ok()?))
}

pub(super) fn auction_total_imbalance(orders: &[AuctionOrderSnap]) -> u64 {
    let (buy, sell) = orders
        .iter()
        .fold((0_u64, 0_u64), |(buy, sell), order| match order.side {
            Side::Buy => (
                buy.checked_add(u64::from(order.qty))
                    .expect("auction buy quantity overflow"),
                sell,
            ),
            Side::Sell => (
                buy,
                sell.checked_add(u64::from(order.qty))
                    .expect("auction sell quantity overflow"),
            ),
        });
    buy.abs_diff(sell)
}
