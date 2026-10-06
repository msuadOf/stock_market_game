//! 按账户范围组织 ledger 行，使 candidate 的单行写入只复制根索引、所属 group 与 page。
use super::EnvelopeKey;
use serde::ser::SerializeMap;
use std::collections::BTreeMap;
use std::sync::Arc;

const PAGE_SHIFT: u32 = super::live_reservations::RESERVATION_PAGE_SHIFT;
const GROUP_SHIFT: u32 = 4;
const ACCOUNTS_PER_GROUP_SHIFT: u32 = PAGE_SHIFT + GROUP_SHIFT;
const PAGES_PER_GROUP: u64 = 1 << GROUP_SHIFT;

type Page<V> = BTreeMap<EnvelopeKey, V>;
type Group<V> = BTreeMap<u64, Arc<Page<V>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LedgerPagedMap<V> {
    groups: BTreeMap<u64, Arc<Group<V>>>,
    len: usize,
}

impl<V> Default for LedgerPagedMap<V> {
    fn default() -> Self {
        Self {
            groups: BTreeMap::new(),
            len: 0,
        }
    }
}

impl<V> LedgerPagedMap<V> {
    fn location(key: &EnvelopeKey) -> (u64, u64) {
        let account = key.account.0;
        (
            account >> ACCOUNTS_PER_GROUP_SHIFT,
            (account >> PAGE_SHIFT) & (PAGES_PER_GROUP - 1),
        )
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn contains_key(&self, key: &EnvelopeKey) -> bool {
        self.get(key).is_some()
    }

    pub(super) fn get(&self, key: &EnvelopeKey) -> Option<&V> {
        let (group_id, page_id) = Self::location(key);
        self.groups.get(&group_id)?.get(&page_id)?.get(key)
    }

    pub(super) fn get_mut(&mut self, key: &EnvelopeKey) -> Option<&mut V>
    where
        V: Clone,
    {
        let (group_id, page_id) = Self::location(key);
        let group = Arc::make_mut(self.groups.get_mut(&group_id)?);
        Arc::make_mut(group.get_mut(&page_id)?).get_mut(key)
    }

    pub(super) fn insert(&mut self, key: EnvelopeKey, value: V) -> Option<V>
    where
        V: Clone,
    {
        let (group_id, page_id) = Self::location(&key);
        let group = Arc::make_mut(self.groups.entry(group_id).or_default());
        let page = Arc::make_mut(group.entry(page_id).or_default());
        let previous = page.insert(key, value);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    pub(super) fn remove(&mut self, key: &EnvelopeKey) -> Option<V>
    where
        V: Clone,
    {
        let (group_id, page_id) = Self::location(key);
        let group = Arc::make_mut(self.groups.get_mut(&group_id)?);
        let page = Arc::make_mut(group.get_mut(&page_id)?);
        let removed = page.remove(key);
        if removed.is_some() {
            self.len -= 1;
        }
        if page.is_empty() {
            group.remove(&page_id);
        }
        if group.is_empty() {
            self.groups.remove(&group_id);
        }
        removed
    }

    pub(super) fn clear(&mut self) {
        self.groups.clear();
        self.len = 0;
    }

    pub(super) fn pages(&self) -> impl Iterator<Item = (u64, &Arc<Page<V>>)> {
        self.groups.iter().flat_map(|(group, pages)| pages.iter()
            .map(move |(page, rows)| ((group << GROUP_SHIFT) | page, rows)))
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = (&EnvelopeKey, &V)> {
        self.groups
            .values()
            .flat_map(|group| group.values().flat_map(|page| page.iter()))
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &EnvelopeKey> {
        self.iter().map(|(key, _)| key)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    #[cfg(test)]
    pub(super) fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut V>
    where
        V: Clone,
    {
        self.groups.values_mut().flat_map(|group| {
            Arc::make_mut(group)
                .values_mut()
                .flat_map(|page| Arc::make_mut(page).values_mut())
        })
    }

    #[cfg(test)]
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&EnvelopeKey, &mut V) -> bool)
    where
        V: Clone,
    {
        let len = &mut self.len;
        self.groups.retain(|_, shared_group| {
            let group = Arc::make_mut(shared_group);
            group.retain(|_, shared_page| {
                let page = Arc::make_mut(shared_page);
                page.retain(|key, value| {
                    let keep_row = keep(key, value);
                    if !keep_row {
                        *len -= 1;
                    }
                    keep_row
                });
                !page.is_empty()
            });
            !group.is_empty()
        });
    }

    #[cfg(test)]
    fn same_group_allocation(&self, other: &Self, key: &EnvelopeKey) -> bool {
        let (group_id, _) = Self::location(key);
        self.groups
            .get(&group_id)
            .zip(other.groups.get(&group_id))
            .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
    }

    #[cfg(test)]
    fn same_page_allocation(&self, other: &Self, key: &EnvelopeKey) -> bool {
        let (group_id, page_id) = Self::location(key);
        self.groups
            .get(&group_id)
            .and_then(|group| group.get(&page_id))
            .zip(
                other
                    .groups
                    .get(&group_id)
                    .and_then(|group| group.get(&page_id)),
            )
            .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
    }
}

impl<V: Clone> From<BTreeMap<EnvelopeKey, V>> for LedgerPagedMap<V> {
    fn from(rows: BTreeMap<EnvelopeKey, V>) -> Self {
        let len = rows.len();
        let mut result = Self::default();
        for (key, value) in rows {
            result.insert(key, value);
        }
        debug_assert_eq!(result.len, len);
        result
    }
}

impl<V> serde::Serialize for LedgerPagedMap<V>
where
    V: serde::Serialize,
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len))?;
        for (key, value) in self.iter() {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<V: PartialEq> PartialEq<BTreeMap<EnvelopeKey, V>> for LedgerPagedMap<V> {
    fn eq(&self, other: &BTreeMap<EnvelopeKey, V>) -> bool {
        self.len == other.len() && self.iter().eq(other.iter())
    }
}

impl<V: PartialEq> PartialEq<LedgerPagedMap<V>> for BTreeMap<EnvelopeKey, V> {
    fn eq(&self, other: &LedgerPagedMap<V>) -> bool {
        other == self
    }
}

impl<V: Clone> IntoIterator for LedgerPagedMap<V> {
    type Item = (EnvelopeKey, V);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut rows = Vec::with_capacity(self.len);
        for (_, group) in self.groups {
            for (_, page) in Arc::unwrap_or_clone(group) {
                rows.extend(Arc::unwrap_or_clone(page));
            }
        }
        rows.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a LedgerPagedMap<V> {
    type Item = (&'a EnvelopeKey, &'a V);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountId, OrderId, Side, StockCode};

    fn key(account: u64, stock: &str, order: u64, side: Side) -> EnvelopeKey {
        EnvelopeKey {
            account: AccountId(account),
            stock: StockCode(stock.to_owned()),
            order: OrderId(order),
            side,
        }
    }

    #[test]
    fn shared_mutation_detaches_only_the_target_group_and_page() {
        let rows = LedgerPagedMap::from(BTreeMap::from([
            (key(1, "AAA", 1, Side::Buy), 10),
            (key(40, "AAA", 1, Side::Buy), 20),
            (key(600, "AAA", 1, Side::Buy), 30),
        ]));
        let mut candidate = rows.clone();
        let target = key(1, "AAA", 1, Side::Buy);
        let other_page_same_group = key(40, "AAA", 1, Side::Buy);
        let other_group = key(600, "AAA", 1, Side::Buy);

        assert!(rows.same_group_allocation(&candidate, &target));
        assert!(rows.same_page_allocation(&candidate, &target));
        *candidate.get_mut(&target).unwrap() = 11;

        assert!(!rows.same_group_allocation(&candidate, &target));
        assert!(!rows.same_page_allocation(&candidate, &target));
        assert!(!rows.same_group_allocation(&candidate, &other_page_same_group));
        assert!(rows.same_page_allocation(&candidate, &other_page_same_group));
        assert!(rows.same_group_allocation(&candidate, &other_group));
        assert!(rows.same_page_allocation(&candidate, &other_group));
        assert_eq!(rows.get(&target), Some(&10));
        assert_eq!(candidate.get(&target), Some(&11));
    }

    #[test]
    fn ordering_and_hash_projection_serde_match_btreemap_for_account_and_key_boundaries() {
        let mut expected = BTreeMap::new();
        for (account, stock, order, side, value) in [
            (u64::MAX, "ZZZ", 9, Side::Sell, 5),
            (u64::MAX, "AAA", 8, Side::Buy, 4),
            ((1_u64 << 53) - 1, "ZZZ", 9, Side::Sell, 7),
            (512, "AAA", 8, Side::Buy, 8),
            (15, "BBB", 1, Side::Buy, 6),
            (15, "AAA", 2, Side::Sell, 3),
            (15, "AAA", 2, Side::Buy, 2),
            (15, "AAA", 1, Side::Buy, 1),
        ] {
            expected.insert(key(account, stock, order, side), value);
        }
        let rows = LedgerPagedMap::from(expected.clone());

        assert_eq!(
            rows.iter().collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            rows.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            rows.values().copied().collect::<Vec<_>>(),
            expected.values().copied().collect::<Vec<_>>()
        );
        assert_eq!(
            serde_json::to_vec(&rows.iter()
                .filter(|(key, _)| key.account.0 <= (1_u64 << 53) - 1).collect::<Vec<_>>()).unwrap(),
            serde_json::to_vec(&expected.iter()
                .filter(|(key, _)| key.account.0 <= (1_u64 << 53) - 1).collect::<Vec<_>>()).unwrap(),
        );
        // Rust 顺序测试保留 u64::MAX；JSON entry 投影必须显式拒绝 JS 不安全整数。
        for entries in [rows.iter().collect::<Vec<_>>(), expected.iter().collect::<Vec<_>>()] {
            assert!(serde_json::to_vec(&entries).unwrap_err().to_string()
                .contains("exceeds JavaScript's safe integer range"));
        }
        // EnvelopeKey 是 struct，JSON map 不支持该 key；hash 使用上面的有序 entry 数组。
        // 明确核对错误契约，不能把两个 Err 相等冒充成功序列化的证据。
        assert_eq!(serde_json::to_vec(&rows).unwrap_err().to_string(), "key must be a string");
        assert_eq!(serde_json::to_vec(&expected).unwrap_err().to_string(), "key must be a string");
        assert_eq!(
            rows.into_iter().collect::<Vec<_>>(),
            expected.into_iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn insert_remove_retain_and_clear_keep_length_and_page_boundaries() {
        let first = key(0, "AAA", 1, Side::Buy);
        let last_in_page = key(15, "AAA", 1, Side::Buy);
        let first_next_page = key(32, "AAA", 1, Side::Buy);
        let mut rows = LedgerPagedMap::default();
        assert_eq!(rows.insert(first.clone(), 1), None);
        assert_eq!(rows.insert(first.clone(), 2), Some(1));
        rows.insert(last_in_page.clone(), 3);
        rows.insert(first_next_page.clone(), 4);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.remove(&last_in_page), Some(3));
        assert!(!rows.contains_key(&last_in_page));
        rows.retain(|key, value| {
            if key == &first {
                *value += 10;
                true
            } else {
                false
            }
        });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.get(&first), Some(&12));
        rows.remove(&first);
        assert!(rows.is_empty());
        rows.insert(first_next_page, 9);
        rows.clear();
        assert!(rows.is_empty());
        assert_eq!(rows.iter().count(), 0);
    }

    #[test]
    fn retain_can_remove_multiple_rows_from_the_last_page_and_group() {
        let mut rows = LedgerPagedMap::from(BTreeMap::from([
            (key(0, "600000", 1, Side::Buy), 1),
            (key(1, "600000", 2, Side::Buy), 2),
            (key(512, "600000", 3, Side::Buy), 3),
            (key(513, "600000", 4, Side::Buy), 4),
        ]));
        let authority = rows.clone();
        rows.retain(|_, _| false);
        assert!(rows.is_empty());
        assert!(rows.groups.is_empty());
        assert_eq!(authority.len(), 4);
    }
}
