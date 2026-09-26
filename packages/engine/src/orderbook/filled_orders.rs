use super::{AccountId, OrderError, OrderId};
use std::sync::Arc;

/// A persistent index: cloning a tick candidate shares old nodes, while a fill
/// copies only the search path. The numeric key is an identity, never priority.
#[derive(Clone, Debug, Default)]
pub(super) struct FilledOrders(Option<Arc<Node>>);

#[derive(Debug)]
struct Node {
    id: OrderId,
    owner: AccountId,
    priority: u64,
    left: FilledOrders,
    right: FilledOrders,
}

impl FilledOrders {
    pub(super) fn owner(&self, id: OrderId) -> Option<AccountId> {
        let mut cursor = self.0.as_deref();
        while let Some(node) = cursor {
            if id == node.id {
                return Some(node.owner);
            }
            cursor = if id < node.id {
                node.left.0.as_deref()
            } else {
                node.right.0.as_deref()
            };
        }
        None
    }

    pub(super) fn insert(&mut self, id: OrderId, owner: AccountId) -> Result<(), OrderError> {
        self.0 = Some(insert(self.0.as_ref(), id, owner)?);
        Ok(())
    }

    pub(super) fn entries(&self) -> Vec<(OrderId, AccountId)> {
        let mut result = Vec::new();
        let mut stack = Vec::new();
        let mut cursor = self.0.as_deref();
        while cursor.is_some() || !stack.is_empty() {
            while let Some(node) = cursor {
                stack.push(node);
                cursor = node.left.0.as_deref();
            }
            let node = stack.pop().expect("nonempty stack after left descent");
            result.push((node.id, node.owner));
            cursor = node.right.0.as_deref();
        }
        result
    }
}

fn insert(
    root: Option<&Arc<Node>>,
    id: OrderId,
    owner: AccountId,
) -> Result<Arc<Node>, OrderError> {
    let Some(root) = root else {
        return Ok(Arc::new(Node {
            id,
            owner,
            priority: priority(id),
            left: FilledOrders::default(),
            right: FilledOrders::default(),
        }));
    };
    if root.id == id {
        return Err(OrderError::DuplicateOrderId(id));
    }
    let mut next = Node {
        id: root.id,
        owner: root.owner,
        priority: root.priority,
        left: root.left.clone(),
        right: root.right.clone(),
    };
    if id < root.id {
        next.left.0 = Some(insert(root.left.0.as_ref(), id, owner)?);
        let child = next.left.0.as_ref().expect("inserted child exists");
        if child.priority < next.priority {
            let mut promoted = clone_node(child);
            next.left = promoted.right;
            promoted.right = FilledOrders(Some(Arc::new(next)));
            return Ok(Arc::new(promoted));
        }
    } else {
        next.right.0 = Some(insert(root.right.0.as_ref(), id, owner)?);
        let child = next.right.0.as_ref().expect("inserted child exists");
        if child.priority < next.priority {
            let mut promoted = clone_node(child);
            next.right = promoted.left;
            promoted.left = FilledOrders(Some(Arc::new(next)));
            return Ok(Arc::new(promoted));
        }
    }
    Ok(Arc::new(next))
}

fn clone_node(node: &Node) -> Node {
    Node {
        id: node.id,
        owner: node.owner,
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
