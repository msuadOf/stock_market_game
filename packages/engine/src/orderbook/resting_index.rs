use super::{OrderError, OrderId};
use crate::Money;
use std::cmp::Reverse;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub(super) enum RestingKey {
    Bid(Reverse<Money>, u64),
    Ask(Money, u64),
}

/// Order identity lookup shared by tick candidates. It never determines book priority.
/// A change copies only the search path instead of the entire live-order index.
#[derive(Clone, Debug, Default)]
pub(super) struct RestingOrderIndex(Option<Arc<Node>>);

#[derive(Debug)]
struct Node {
    id: OrderId,
    key: RestingKey,
    priority: u64,
    left: RestingOrderIndex,
    right: RestingOrderIndex,
}

impl RestingOrderIndex {
    pub(super) fn get(&self, id: OrderId) -> Option<RestingKey> {
        let mut cursor = self.0.as_deref();
        while let Some(node) = cursor {
            if id == node.id {
                return Some(node.key);
            }
            cursor = if id < node.id {
                node.left.0.as_deref()
            } else {
                node.right.0.as_deref()
            };
        }
        None
    }

    pub(super) fn insert(&mut self, id: OrderId, key: RestingKey) -> Result<(), OrderError> {
        self.0 = Some(insert(self.0.as_ref(), id, key)?);
        Ok(())
    }

    pub(super) fn remove(&mut self, id: OrderId) -> Option<RestingKey> {
        let (root, key) = remove(self.0.as_ref(), id);
        self.0 = root;
        key
    }

    pub(super) fn clear(&mut self) {
        self.0 = None;
    }
}

fn insert(root: Option<&Arc<Node>>, id: OrderId, key: RestingKey) -> Result<Arc<Node>, OrderError> {
    let Some(root) = root else {
        return Ok(Arc::new(Node {
            id,
            key,
            priority: priority(id),
            left: RestingOrderIndex::default(),
            right: RestingOrderIndex::default(),
        }));
    };
    if root.id == id {
        return Err(OrderError::DuplicateOrderId(id));
    }
    let mut next = clone_node(root);
    if id < root.id {
        next.left.0 = Some(insert(root.left.0.as_ref(), id, key)?);
        let child = next.left.0.as_ref().expect("inserted child exists");
        if child.priority < next.priority {
            let mut promoted = clone_node(child);
            next.left = promoted.right;
            promoted.right = RestingOrderIndex(Some(Arc::new(next)));
            return Ok(Arc::new(promoted));
        }
    } else {
        next.right.0 = Some(insert(root.right.0.as_ref(), id, key)?);
        let child = next.right.0.as_ref().expect("inserted child exists");
        if child.priority < next.priority {
            let mut promoted = clone_node(child);
            next.right = promoted.left;
            promoted.left = RestingOrderIndex(Some(Arc::new(next)));
            return Ok(Arc::new(promoted));
        }
    }
    Ok(Arc::new(next))
}

fn remove(root: Option<&Arc<Node>>, id: OrderId) -> (Option<Arc<Node>>, Option<RestingKey>) {
    let Some(root) = root else {
        return (None, None);
    };
    if id == root.id {
        return (
            merge(root.left.0.as_ref(), root.right.0.as_ref()),
            Some(root.key),
        );
    }
    let mut next = clone_node(root);
    let removed = if id < root.id {
        let (left, removed) = remove(root.left.0.as_ref(), id);
        next.left.0 = left;
        removed
    } else {
        let (right, removed) = remove(root.right.0.as_ref(), id);
        next.right.0 = right;
        removed
    };
    if removed.is_some() {
        (Some(Arc::new(next)), removed)
    } else {
        (Some(root.clone()), None)
    }
}

fn merge(left: Option<&Arc<Node>>, right: Option<&Arc<Node>>) -> Option<Arc<Node>> {
    match (left, right) {
        (None, None) => None,
        (Some(node), None) | (None, Some(node)) => Some(node.clone()),
        (Some(left), Some(right)) if left.priority < right.priority => {
            let mut next = clone_node(left);
            next.right.0 = merge(left.right.0.as_ref(), Some(right));
            Some(Arc::new(next))
        }
        (Some(left), Some(right)) => {
            let mut next = clone_node(right);
            next.left.0 = merge(Some(left), right.left.0.as_ref());
            Some(Arc::new(next))
        }
    }
}

fn clone_node(node: &Node) -> Node {
    Node {
        id: node.id,
        key: node.key,
        priority: node.priority,
        left: node.left.clone(),
        right: node.right.clone(),
    }
}

fn priority(id: OrderId) -> u64 {
    let mut value = id.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
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
