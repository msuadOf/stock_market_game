//! 诊断专用的不可变共享 u64 索引；每次更新仅触及固定的 16 层路径。

use std::sync::Arc;

#[derive(Debug)]
pub(super) struct SharedDiagnosticIndex<V> {
    root: Option<Arc<Node<V>>>,
    len: usize,
}

#[derive(Debug)]
enum Node<V> {
    Branch(Box<[Option<Arc<Node<V>>>; 16]>),
    Leaf(V),
}

impl<V: Clone> Clone for Node<V> {
    fn clone(&self) -> Self {
        match self {
            Self::Branch(children) => {
                #[cfg(test)]
                counts::change(|counts| {
                    counts.branch_clones += 1;
                    counts.boxes += 1;
                });
                Self::Branch(children.clone())
            }
            Self::Leaf(value) => {
                #[cfg(test)]
                counts::change(|counts| counts.leaf_clones += 1);
                Self::Leaf(value.clone())
            }
        }
    }
}

impl<V> Default for SharedDiagnosticIndex<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Clone for SharedDiagnosticIndex<V> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            len: self.len,
        }
    }
}

impl<V> SharedDiagnosticIndex<V> {
    pub(super) fn new() -> Self {
        Self { root: None, len: 0 }
    }
    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn get(&self, key: u64) -> Option<&V> {
        let mut node = self.root.as_deref()?;
        for depth in 0..16 {
            let Node::Branch(children) = node else {
                panic!("诊断索引在深度 {depth} 提前遇到 Leaf");
            };
            node = children[((key >> (60 - depth * 4)) & 15) as usize].as_deref()?;
        }
        let Node::Leaf(value) = node else {
            panic!("诊断索引在深度 16 遇到 Branch");
        };
        Some(value)
    }

    pub(super) fn visit_sorted<E>(
        &self,
        mut visitor: impl FnMut(u64, &V) -> Result<(), E>,
    ) -> Result<(), E> {
        fn visit<V, E>(
            node: &Node<V>,
            depth: u32,
            key: u64,
            visitor: &mut impl FnMut(u64, &V) -> Result<(), E>,
        ) -> Result<(), E> {
            match node {
                Node::Branch(children) => {
                    assert!(depth < 16, "诊断索引在深度 16 遇到 Branch");
                    for (nibble, child) in children.iter().enumerate() {
                        if let Some(child) = child {
                            visit(child, depth + 1, (key << 4) | nibble as u64, visitor)?;
                        }
                    }
                    Ok(())
                }
                Node::Leaf(value) => {
                    assert_eq!(depth, 16, "诊断索引提前遇到 Leaf");
                    visitor(key, value)
                }
            }
        }
        match self.root.as_deref() {
            Some(root) => visit(root, 0, 0, &mut visitor),
            None => Ok(()),
        }
    }
}

impl<V: Clone> SharedDiagnosticIndex<V> {
    pub(super) fn insert(&mut self, key: u64, value: V) -> Option<V> {
        // 在任何路径修改前验证长度，避免内部不变量失败后留下半更新。
        let next_len = if self.get(key).is_none() {
            self.len.checked_add(1).expect("诊断索引长度溢出")
        } else {
            self.len
        };
        fn insert<V: Clone>(
            slot: &mut Option<Arc<Node<V>>>,
            depth: u32,
            key: u64,
            value: V,
        ) -> Option<V> {
            if depth == 16 {
                if let Some(node) = slot {
                    let Node::Leaf(previous) = Arc::make_mut(node) else {
                        panic!("诊断索引在深度 16 遇到 Branch");
                    };
                    return Some(std::mem::replace(previous, value));
                }
                #[cfg(test)]
                counts::change(|counts| counts.new_leaves += 1);
                *slot = Some(Arc::new(Node::Leaf(value)));
                return None;
            }
            let node = slot.get_or_insert_with(|| {
                #[cfg(test)]
                counts::change(|counts| {
                    counts.new_branches += 1;
                    counts.boxes += 1;
                });
                Arc::new(Node::Branch(Box::new(std::array::from_fn(|_| None))))
            });
            let Node::Branch(children) = Arc::make_mut(node) else {
                panic!("诊断索引在深度 {depth} 提前遇到 Leaf");
            };
            let nibble = ((key >> (60 - depth * 4)) & 15) as usize;
            insert(&mut children[nibble], depth + 1, key, value)
        }
        let previous = insert(&mut self.root, 0, key, value);
        self.len = next_len;
        previous
    }
}

#[cfg(test)]
pub(super) mod counts {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Counts {
        pub branch_clones: usize,
        pub leaf_clones: usize,
        pub new_branches: usize,
        pub new_leaves: usize,
        pub boxes: usize,
    }
    thread_local! { static COUNTS: Cell<Counts> = Cell::new(Counts::default()); }
    pub(crate) fn change(update: impl FnOnce(&mut Counts)) {
        COUNTS.with(|cell| {
            let mut value = cell.get();
            update(&mut value);
            cell.set(value);
        });
    }
    pub(crate) fn reset() {
        COUNTS.with(|cell| cell.set(Counts::default()));
    }
    pub(crate) fn get() -> Counts {
        COUNTS.with(Cell::get)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static VALUE_CLONES: Cell<(usize, usize)> = const { Cell::new((0, 0)) }; }
    #[derive(Debug, PartialEq, Eq)]
    struct Value {
        key: u64,
        version: u64,
    }
    impl Clone for Value {
        fn clone(&self) -> Self {
            VALUE_CLONES.with(|cell| {
                let (target, other) = cell.get();
                cell.set(if self.key == 0 {
                    (target + 1, other)
                } else {
                    (target, other + 1)
                });
            });
            Self {
                key: self.key,
                version: self.version,
            }
        }
    }
    fn reset() {
        counts::reset();
        VALUE_CLONES.with(|cell| cell.set((0, 0)));
    }
    fn keys(count: usize) -> Vec<u64> {
        let mut keys = vec![0, u32::MAX as u64, u64::MAX];
        keys.extend((1..count - 2).map(|index| (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)));
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count);
        keys
    }
    fn bounded_path(count: usize) {
        let keys = keys(count);
        let mut index = SharedDiagnosticIndex::new();
        for &key in &keys {
            assert!(index.insert(key, Value { key, version: 1 }).is_none());
        }
        assert_eq!(index.len(), count);
        reset();
        let checkpoint = index.clone();
        assert!(Arc::ptr_eq(
            index.root.as_ref().unwrap(),
            checkpoint.root.as_ref().unwrap()
        ));
        assert_eq!(counts::get(), counts::Counts::default());
        assert_eq!(VALUE_CLONES.with(Cell::get), (0, 0));
        assert_eq!(
            index
                .insert(0, Value { key: 0, version: 2 })
                .unwrap()
                .version,
            1
        );
        let work = counts::get();
        assert_eq!(
            (
                work.branch_clones,
                work.leaf_clones,
                work.new_branches,
                work.new_leaves,
                work.boxes
            ),
            (16, 1, 0, 0, 16)
        );
        assert_eq!(
            VALUE_CLONES.with(Cell::get),
            (1, 0),
            "不得 clone 无关 key 的 value"
        );
        assert_eq!(index.len(), count);
        assert_eq!(checkpoint.get(0).unwrap().version, 1);
        assert_eq!(index.get(0).unwrap().version, 2);
        assert!(std::ptr::eq(
            index.get(u64::MAX).unwrap(),
            checkpoint.get(u64::MAX).unwrap()
        ));
        reset();
        let absent = 0x8123_4567_89ab_cdef;
        assert!(!keys.contains(&absent));
        assert!(index
            .insert(
                absent,
                Value {
                    key: absent,
                    version: 3
                }
            )
            .is_none());
        let work = counts::get();
        assert!(work.branch_clones + work.new_branches <= 16);
        assert_eq!(work.new_leaves, 1);
        assert_eq!(work.leaf_clones, 0);
        assert!(work.boxes <= 16);
        assert!(
            work.branch_clones
                + work.new_branches
                + work.leaf_clones
                + work.new_leaves
                + work.boxes
                <= 33
        );
        assert_eq!(VALUE_CLONES.with(Cell::get), (0, 0));
        assert_eq!(index.len(), count + 1);
        assert!(checkpoint.get(absent).is_none());
        for &key in &keys {
            assert_eq!(checkpoint.get(key).unwrap().version, 1);
        }
        let mut visited = Vec::new();
        index
            .visit_sorted(|key, value| {
                assert_eq!(key, value.key);
                visited.push(key);
                Ok::<_, ()>(())
            })
            .unwrap();
        let mut expected = keys;
        expected.push(absent);
        expected.sort_unstable();
        assert_eq!(visited, expected);
    }
    #[test]
    fn d0b_radix_257_keys_clone_and_update_have_fixed_bounds() {
        bounded_path(257);
    }
    #[test]
    fn d0b_radix_4097_keys_clone_and_update_have_fixed_bounds() {
        bounded_path(4097);
    }
    #[test]
    fn d0b_radix_empty_and_sorted_visitor_errors_remain_explicit() {
        let mut index = SharedDiagnosticIndex::new();
        assert_eq!(index.len(), 0);
        assert!(index.get(0).is_none());
        index
            .visit_sorted(|_, _: &u64| Err("不应访问空树"))
            .unwrap();
        for key in [u64::MAX, 0, u32::MAX as u64, 1 << 63] {
            index.insert(key, key);
        }
        let mut visited = Vec::new();
        assert_eq!(
            index.visit_sorted(|key, _| {
                visited.push(key);
                if key == 1 << 63 {
                    Err("停止")
                } else {
                    Ok(())
                }
            }),
            Err("停止")
        );
        assert_eq!(visited, vec![0, u32::MAX as u64, 1 << 63]);
    }
}
