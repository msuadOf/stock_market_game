use super::{OrderError, OrderId};
use std::sync::Arc;

/// 以 OrderId 查找的持久 treap；路径复制保留候选快照，priority 不代表撮合优先级。
#[derive(Clone, Debug)]
pub(super) struct OrderIdPersistentIndex<V: Copy>(Option<Arc<Node<V>>>);

#[derive(Debug)]
struct Node<V: Copy> {
    id: OrderId,
    value: V,
    priority: u64,
    left: OrderIdPersistentIndex<V>,
    right: OrderIdPersistentIndex<V>,
}

impl<V: Copy> Default for OrderIdPersistentIndex<V> {
    fn default() -> Self {
        Self(None)
    }
}

impl<V: Copy> OrderIdPersistentIndex<V> {
    pub(super) fn get(&self, id: OrderId) -> Option<V> {
        let mut cursor = self.0.as_deref();
        while let Some(node) = cursor {
            if id == node.id {
                return Some(node.value);
            }
            cursor = if id < node.id {
                node.left.0.as_deref()
            } else {
                node.right.0.as_deref()
            };
        }
        None
    }

    pub(super) fn insert(&mut self, id: OrderId, value: V) -> Result<(), OrderError> {
        self.0 = Some(insert(self.0.as_ref(), id, value)?);
        Ok(())
    }

    pub(super) fn remove(&mut self, id: OrderId) -> Option<V> {
        let (root, value) = remove(self.0.as_ref(), id);
        self.0 = root;
        value
    }

    pub(super) fn entries(&self) -> Vec<(OrderId, V)> {
        let mut result = Vec::new();
        let mut stack = Vec::new();
        let mut cursor = self.0.as_deref();
        while cursor.is_some() || !stack.is_empty() {
            while let Some(node) = cursor {
                stack.push(node);
                cursor = node.left.0.as_deref();
            }
            let node = stack.pop().expect("nonempty stack after left descent");
            result.push((node.id, node.value));
            cursor = node.right.0.as_deref();
        }
        result
    }

    pub(super) fn clear(&mut self) {
        self.0 = None;
    }
}

fn insert<V: Copy>(
    root: Option<&Arc<Node<V>>>,
    id: OrderId,
    value: V,
) -> Result<Arc<Node<V>>, OrderError> {
    let Some(root) = root else {
        return Ok(Arc::new(Node {
            id,
            value,
            priority: priority(id),
            left: OrderIdPersistentIndex::default(),
            right: OrderIdPersistentIndex::default(),
        }));
    };
    if root.id == id {
        return Err(OrderError::DuplicateOrderId(id));
    }
    let mut next = clone_node(root);
    if id < root.id {
        next.left.0 = Some(insert(root.left.0.as_ref(), id, value)?);
        let child = next.left.0.as_ref().expect("inserted child exists");
        if child.priority < next.priority {
            let mut promoted = clone_node(child);
            next.left = promoted.right;
            promoted.right = OrderIdPersistentIndex(Some(Arc::new(next)));
            return Ok(Arc::new(promoted));
        }
    } else {
        next.right.0 = Some(insert(root.right.0.as_ref(), id, value)?);
        let child = next.right.0.as_ref().expect("inserted child exists");
        if child.priority < next.priority {
            let mut promoted = clone_node(child);
            next.right = promoted.left;
            promoted.left = OrderIdPersistentIndex(Some(Arc::new(next)));
            return Ok(Arc::new(promoted));
        }
    }
    Ok(Arc::new(next))
}

fn remove<V: Copy>(root: Option<&Arc<Node<V>>>, id: OrderId) -> (Option<Arc<Node<V>>>, Option<V>) {
    let Some(root) = root else {
        return (None, None);
    };
    if id == root.id {
        return (
            merge(root.left.0.as_ref(), root.right.0.as_ref()),
            Some(root.value),
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

fn merge<V: Copy>(
    left: Option<&Arc<Node<V>>>,
    right: Option<&Arc<Node<V>>>,
) -> Option<Arc<Node<V>>> {
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

fn clone_node<V: Copy>(node: &Node<V>) -> Node<V> {
    Node {
        id: node.id,
        value: node.value,
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
    fn boundary_ids_remove_missing_and_duplicate_keep_original_root() {
        let mut index = OrderIdPersistentIndex::default();
        for id in [u64::MAX, 0, 2, 8, 4, 6] {
            index.insert(OrderId(id), id).unwrap();
        }
        let snapshot = index.clone();
        let root = index.0.clone().unwrap();
        assert_eq!(index.remove(OrderId(5)), None);
        assert!(Arc::ptr_eq(&root, index.0.as_ref().unwrap()));
        assert!(matches!(
            index.insert(OrderId(4), 99),
            Err(OrderError::DuplicateOrderId(OrderId(4)))
        ));
        assert!(Arc::ptr_eq(&root, index.0.as_ref().unwrap()));
        for id in [0, u64::MAX, 4, 8, 2, 6] {
            assert_eq!(index.remove(OrderId(id)), Some(id));
            assert_eq!(snapshot.get(OrderId(id)), Some(id));
        }
        assert!(index.entries().is_empty());
        assert_eq!(snapshot.entries().len(), 6);
    }
}
