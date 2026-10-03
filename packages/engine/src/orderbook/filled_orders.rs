use super::{AccountId, OrderError, OrderId};

/// 完全成交身份事实的持久索引；不暴露删除，保持与实时索引不同的领域操作。
#[derive(Clone, Debug, Default)]
pub(super) struct FilledOrders(super::persistent_index::OrderIdPersistentIndex<AccountId>);

impl FilledOrders {
    pub(super) fn owner(&self, id: OrderId) -> Option<AccountId> {
        self.0.get(id)
    }
    pub(super) fn insert(&mut self, id: OrderId, owner: AccountId) -> Result<(), OrderError> {
        self.0.insert(id, owner)
    }
    pub(super) fn entries(&self) -> Vec<(OrderId, AccountId)> {
        self.0.entries()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_and_duplicate_preserve_filled_identity_snapshot() {
        let mut index = FilledOrders::default();
        for id in [u64::MAX, 0, 7, 2] {
            index.insert(OrderId(id), AccountId(id)).unwrap();
        }
        let snapshot = index.clone();
        index.insert(OrderId(3), AccountId(90)).unwrap();
        assert!(matches!(
            index.insert(OrderId(7), AccountId(91)),
            Err(OrderError::DuplicateOrderId(OrderId(7)))
        ));
        assert_eq!(index.owner(OrderId(7)), Some(AccountId(7)));
        assert_eq!(snapshot.owner(OrderId(3)), None);
        assert_eq!(
            snapshot.entries(),
            [0, 2, 7, u64::MAX].map(|id| (OrderId(id), AccountId(id)))
        );
    }
}
