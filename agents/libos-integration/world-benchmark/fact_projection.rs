//! 按事实类型分别比较同股与同账户的实际记录；不推断不同类型之间的顺序。
use std::{collections::BTreeMap, fmt::Debug};

/// 每个 key 的记录保持原顺序；独立 key 间的交错不影响比较。
pub fn same_scoped<T: Eq + Debug, K: Ord>(left: &[T], right: &[T], keys: impl Fn(&T) -> Vec<K>) -> bool {
    fn project<'a, T, K: Ord>(items: &'a [T], keys: &impl Fn(&T) -> Vec<K>) -> BTreeMap<K, Vec<&'a T>> {
        let mut result = BTreeMap::new();
        for item in items {
            let mut item_keys = keys(item);
            item_keys.sort_unstable();
            item_keys.dedup();
            for key in item_keys { result.entry(key).or_insert_with(Vec::new).push(item); }
        }
        result
    }
    project(left, &keys) == project(right, &keys)
}

#[cfg(test)]
mod tests {
    use super::same_scoped;

    #[test]
    fn 同股顺序变化必须被发现而跨股交错可不同() {
        let left = [(1, 10), (2, 20), (1, 11)];
        assert!(same_scoped(&left, &[(2, 20), (1, 10), (1, 11)], |x| vec![x.0]));
        assert!(!same_scoped(&left, &[(1, 11), (2, 20), (1, 10)], |x| vec![x.0]));
        assert!(!same_scoped(&left, &[(1, 10), (2, 20)], |x| vec![x.0]));
    }

    #[test]
    fn 双方账户都保持顺序且重复身份不重复计数() {
        let left = [(1, 2, 10), (1, 1, 11), (3, 4, 12)];
        assert!(same_scoped(&left, &[(3, 4, 12), (1, 2, 10), (1, 1, 11)], |x| vec![x.0, x.1]));
        assert!(!same_scoped(&left, &[(1, 1, 11), (1, 2, 10), (3, 4, 12)], |x| vec![x.0, x.1]));
    }
}
