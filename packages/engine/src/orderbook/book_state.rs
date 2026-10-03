use super::{
    AccountId, FilledOrders, Money, Order, OrderBookChange, OrderError, OrderId, RestingKey,
    RestingOrderIndex, Side,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

/// 买卖盘口与身份索引的共同 owner；价格时间键是唯一成交优先级。
#[derive(Clone, Debug, Default)]
pub(super) struct BookState {
    bids: BTreeMap<(Reverse<Money>, u64), Order>,
    asks: BTreeMap<(Money, u64), Order>,
    live_by_id: RestingOrderIndex,
    filled_orders: FilledOrders,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_maker_qty_write_precedes_progress_overflow_on_both_sides() {
        for side in [Side::Buy, Side::Sell] {
            let maker_side = if side == Side::Buy {
                Side::Sell
            } else {
                Side::Buy
            };
            let maker = Order {
                id: OrderId(1),
                owner: AccountId(1),
                side: maker_side,
                price: Money::from_cents(100),
                qty: 2,
                original_qty: 2,
                filled_qty: u32::MAX,
                filled_value: Money::ZERO,
                seq: 3,
            };
            // 私有异常 fixture 固定既有中途写入；生产入口仍会拒绝这种数量进度。
            let mut state = BookState::default();
            state.insert_resting(maker.clone()).unwrap();
            let error = state
                .apply_maker_fill(side, &maker, 1, Money::from_cents(100))
                .unwrap_err();
            assert!(matches!(
                error,
                OrderError::InvalidQuantityProgress {
                    remaining_qty: 1,
                    filled_qty: u32::MAX,
                    ..
                }
            ));
            let remaining = state.resting_order_by_id(OrderId(1)).unwrap();
            assert_eq!(remaining.qty, 1);
            assert_eq!(remaining.filled_value, Money::ZERO);
            assert_eq!(remaining.seq, 3);
            assert!(state.filled_orders.owner(OrderId(1)).is_none());
        }
    }
}

impl BookState {
    pub(super) fn bids(&self) -> &BTreeMap<(Reverse<Money>, u64), Order> {
        &self.bids
    }
    pub(super) fn asks(&self) -> &BTreeMap<(Money, u64), Order> {
        &self.asks
    }
    pub(super) fn live_by_id(&self) -> &RestingOrderIndex {
        &self.live_by_id
    }
    pub(super) fn filled_orders(&self) -> &FilledOrders {
        &self.filled_orders
    }

    pub(super) fn changed_orders_since(
        &self,
        originals: BTreeMap<OrderId, Option<Order>>,
    ) -> Vec<OrderBookChange> {
        originals
            .into_iter()
            .filter_map(|(id, before)| {
                let after = self.resting_order_by_id(id).cloned();
                let filled_owner = self.filled_orders.owner(id);
                (before != after || filled_owner.is_some()).then_some(OrderBookChange {
                    id,
                    before,
                    after,
                    filled_owner,
                })
            })
            .collect()
    }

    pub(super) fn insert_resting(&mut self, order: Order) -> Result<(), OrderError> {
        if self.live_by_id.get(order.id).is_some() || self.filled_orders.owner(order.id).is_some() {
            return Err(OrderError::DuplicateOrderId(order.id));
        }
        let id = order.id;
        let key = match order.side {
            Side::Buy => {
                let key = (Reverse(order.price), order.seq);
                if self.bids.contains_key(&key) {
                    return Err(OrderError::ProjectionMismatch {
                        reason: "bid price-time key already exists".to_owned(),
                    });
                }
                self.bids.insert(key, order);
                RestingKey::Bid(key.0, key.1)
            }
            Side::Sell => {
                let key = (order.price, order.seq);
                if self.asks.contains_key(&key) {
                    return Err(OrderError::ProjectionMismatch {
                        reason: "ask price-time key already exists".to_owned(),
                    });
                }
                self.asks.insert(key, order);
                RestingKey::Ask(key.0, key.1)
            }
        };
        self.live_by_id.insert(id, key)?;
        Ok(())
    }

    pub(super) fn cancel(&mut self, id: OrderId) -> Result<Order, OrderError> {
        if let Some(key) = self.live_by_id.remove(id) {
            let order = match key {
                RestingKey::Bid(price, seq) => self.bids.remove(&(price, seq)),
                RestingKey::Ask(price, seq) => self.asks.remove(&(price, seq)),
            };
            return order.ok_or_else(|| OrderError::ProjectionMismatch {
                reason: format!("live order index points at an absent order: {id:?}"),
            });
        }
        if self.filled_orders.owner(id).is_some() {
            return Err(OrderError::OrderAlreadyFilled(id));
        }
        Err(OrderError::OrderNotFound(id))
    }

    pub(super) fn restore_filled_orders(
        &mut self,
        entries: impl IntoIterator<Item = (OrderId, AccountId)>,
    ) -> Result<(), OrderError> {
        for (id, owner) in entries {
            if self.live_by_id.get(id).is_some() {
                return Err(OrderError::DuplicateOrderId(id));
            }
            self.filled_orders.insert(id, owner)?;
        }
        Ok(())
    }

    pub(super) fn resting_order_by_id(&self, id: OrderId) -> Option<&Order> {
        match self.live_by_id.get(id)? {
            RestingKey::Bid(price, seq) => self.bids.get(&(price, seq)),
            RestingKey::Ask(price, seq) => self.asks.get(&(price, seq)),
        }
    }

    pub(super) fn apply_maker_fill(
        &mut self,
        side: Side,
        maker: &Order,
        fill_qty: u32,
        maker_filled_value_after: Money,
    ) -> Result<(), OrderError> {
        match side {
            Side::Buy => {
                if maker.qty == fill_qty {
                    self.asks.pop_first();
                    self.live_by_id.remove(maker.id);
                    self.filled_orders.insert(maker.id, maker.owner)?;
                } else {
                    let key = (maker.price, maker.seq);
                    if let Some(m) = self.asks.get_mut(&key) {
                        m.qty -= fill_qty;
                        m.filled_qty = m.filled_qty_after(fill_qty)?;
                        m.filled_value = maker_filled_value_after;
                    }
                }
            }
            Side::Sell => {
                if maker.qty == fill_qty {
                    self.bids.pop_first();
                    self.live_by_id.remove(maker.id);
                    self.filled_orders.insert(maker.id, maker.owner)?;
                } else {
                    let key = (Reverse(maker.price), maker.seq);
                    if let Some(m) = self.bids.get_mut(&key) {
                        m.qty -= fill_qty;
                        m.filled_qty = m.filled_qty_after(fill_qty)?;
                        m.filled_value = maker_filled_value_after;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn record_filled(
        &mut self,
        id: OrderId,
        owner: AccountId,
    ) -> Result<(), OrderError> {
        self.filled_orders.insert(id, owner)
    }

    pub(super) fn clear_resting(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.live_by_id.clear();
    }

    pub(super) fn apply_changes(
        &mut self,
        changes: Vec<OrderBookChange>,
    ) -> Result<(), OrderError> {
        for change in &changes {
            if self.resting_order_by_id(change.id) != change.before.as_ref()
                || (change.before.is_none() && self.filled_orders.owner(change.id).is_some())
                || change
                    .after
                    .as_ref()
                    .is_some_and(|order| order.id != change.id)
                || (change.after.is_some() && change.filled_owner.is_some())
            {
                return Err(OrderError::ProjectionMismatch {
                    reason: format!("candidate order differs from stock worker: {:?}", change.id),
                });
            }
        }
        for change in changes {
            if change.before.is_some() {
                self.cancel(change.id)?;
            }
            if let Some(after) = change.after {
                self.insert_resting(after)?;
            } else if let Some(owner) = change.filled_owner {
                self.filled_orders.insert(change.id, owner)?;
            }
        }
        Ok(())
    }
}
