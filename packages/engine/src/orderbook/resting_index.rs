use super::{OrderError, OrderId};
use crate::Money;
use std::cmp::Reverse;

#[derive(Clone, Copy, Debug)]
pub(super) enum RestingKey {
    Bid(Reverse<Money>, u64),
    Ask(Money, u64),
}

/// 实时身份索引仅负责 RestingKey 查询，不决定价格和时间优先级。
#[derive(Clone, Debug, Default)]
pub(super) struct RestingOrderIndex(super::persistent_index::OrderIdPersistentIndex<RestingKey>);

impl RestingOrderIndex {
    pub(super) fn get(&self, id: OrderId) -> Option<RestingKey> {
        self.0.get(id)
    }
    pub(super) fn insert(&mut self, id: OrderId, key: RestingKey) -> Result<(), OrderError> {
        self.0.insert(id, key)
    }
    pub(super) fn remove(&mut self, id: OrderId) -> Option<RestingKey> {
        self.0.remove(id)
    }
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_orders_leave_shared_tick_snapshot_intact() {
        let mut index = RestingOrderIndex::default();
        for offset in 0..512_u64 {
            let id = OrderId((offset * 211) % 512);
            index
                .insert(id, RestingKey::Ask(Money::from_cents(1_000), offset))
                .unwrap();
        }
        let snapshot = index.clone();
        for offset in 0..512_u64 {
            let id = OrderId((offset * 211) % 512);
            assert!(index.remove(id).is_some());
            assert!(index.get(id).is_none());
            assert!(snapshot.get(id).is_some());
        }
        assert!(index.remove(OrderId(1)).is_none());
        index
            .insert(
                OrderId(1),
                RestingKey::Bid(Reverse(Money::from_cents(990)), 700),
            )
            .unwrap();
        assert!(index.get(OrderId(1)).is_some());
    }
}
