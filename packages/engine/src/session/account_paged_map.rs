//! 已提交Session与tick candidate通过页级COW共享每个账户的状态。

#[cfg(test)]
use crate::compute::parallel::*;
use crate::AccountId;
use serde::ser::SerializeMap;
#[cfg(test)]
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::Arc;

const PAGE_SHIFT: u32 = 5;
const GROUP_SHIFT: u32 = 4;
const PAGE_SIZE: usize = 1 << PAGE_SHIFT;
const GROUP_SIZE: usize = 1 << GROUP_SHIFT;
type Page<Value> = [Option<(AccountId, Arc<Value>)>; PAGE_SIZE];
type PageGroup<Value> = [Option<Arc<Page<Value>>>; GROUP_SIZE];

#[cfg(test)]
thread_local! {
    static SHARED_PREVIOUS_VALUE_CLONES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn reset_shared_previous_value_clones() {
    SHARED_PREVIOUS_VALUE_CLONES.with(|count| count.set(0));
}

#[cfg(test)]
pub(super) fn shared_previous_value_clones() -> usize {
    SHARED_PREVIOUS_VALUE_CLONES.with(Cell::get)
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AccountPagedMap<Value> {
    groups: BTreeMap<u64, Arc<PageGroup<Value>>>,
    len: usize,
}

impl<Value: Eq> Eq for AccountPagedMap<Value> {}

impl<Value> Default for AccountPagedMap<Value> {
    fn default() -> Self {
        Self {
            groups: BTreeMap::new(),
            len: 0,
        }
    }
}

impl<Value> AccountPagedMap<Value> {
    pub(super) fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn get(&self, id: &AccountId) -> Option<&Value> {
        let page = self.page_arc(page_id(*id))?;
        let (stored_id, value) = page[page_slot(*id)].as_ref()?;
        (stored_id == id).then_some(value.as_ref())
    }

    pub(super) fn contains_key(&self, id: &AccountId) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = (&AccountId, &Value)> {
        self.groups.values().flat_map(|group| {
            group.iter().filter_map(Option::as_ref).flat_map(|page| {
                page.iter()
                    .filter_map(Option::as_ref)
                    .map(|(id, value)| (id, value.as_ref()))
            })
        })
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &AccountId> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.iter().map(|(_, value)| value)
    }

    fn page_arc(&self, page: u64) -> Option<&Arc<Page<Value>>> {
        self.groups
            .get(&(page >> GROUP_SHIFT))?
            .get((page & ((1 << GROUP_SHIFT) - 1)) as usize)?
            .as_ref()
    }
}

impl<Value: Clone> AccountPagedMap<Value> {
    /// 在私有tick状态中修改分属不同页的独立既有账户。
    #[cfg(test)]
    pub(super) fn mutate_existing_parallel<Output, Error>(
        &mut self,
        ids: &[AccountId],
        missing: impl Fn(AccountId) -> Error + Sync,
        change: impl Fn(AccountId, &mut Value) -> Result<Output, Error> + Sync,
    ) -> Result<Vec<(AccountId, Output)>, Error>
    where
        Value: Send + Sync,
        Output: Send,
        Error: Send,
    {
        let mut by_group = BTreeMap::<u64, Vec<AccountId>>::new();
        for id in ids {
            by_group.entry(group_id(*id)).or_default().push(*id);
        }
        let work = self
            .groups
            .iter_mut()
            .filter_map(|(group_id, group)| by_group.remove(group_id).map(|ids| (group, ids)))
            .collect::<Vec<_>>();
        if let Some(ids) = by_group.into_values().next() {
            return Err(missing(ids[0]));
        }
        let batches = work
            .into_par_iter()
            .map(|(group, ids)| {
                let groups = Arc::make_mut(group);
                let mut results = Vec::with_capacity(ids.len());
                for id in ids {
                    let page = groups[group_slot(id)].as_mut().ok_or_else(|| missing(id))?;
                    let slots = Arc::make_mut(page);
                    let entry = slots[page_slot(id)].as_mut().ok_or_else(|| missing(id))?;
                    if entry.0 != id {
                        return Err(missing(id));
                    }
                    results.push((id, change(id, Arc::make_mut(&mut entry.1))?));
                }
                Ok(results)
            })
            .collect::<Vec<Result<Vec<_>, Error>>>();
        Ok(batches
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    pub(super) fn get_mut(&mut self, id: &AccountId) -> Option<&mut Value> {
        if !self.contains_key(id) {
            return None;
        }
        let group = Arc::make_mut(self.groups.get_mut(&group_id(*id))?);
        let page = group[group_slot(*id)].as_mut()?;
        let slots = Arc::make_mut(page);
        let (_, value) = slots[page_slot(*id)].as_mut()?;
        Some(Arc::make_mut(value))
    }

    pub(super) fn insert(&mut self, id: AccountId, value: Value) -> Option<Value> {
        self.insert_shared(id, value).map(|previous| {
            Arc::try_unwrap(previous).unwrap_or_else(|shared| {
                #[cfg(test)]
                SHARED_PREVIOUS_VALUE_CLONES.with(|count| count.set(count.get() + 1));
                (*shared).clone()
            })
        })
    }

    #[cfg(test)]
    pub(super) fn insert_without_previous(&mut self, id: AccountId, value: Value) {
        self.insert_shared(id, value);
    }

    fn insert_shared(&mut self, id: AccountId, value: Value) -> Option<Arc<Value>> {
        self.insert_arc(id, Arc::new(value))
    }

    pub(super) fn insert_arc(&mut self, id: AccountId, value: Arc<Value>) -> Option<Arc<Value>> {
        let group = self
            .groups
            .entry(group_id(id))
            .or_insert_with(|| Arc::new(std::array::from_fn(|_| None)));
        let slots = Arc::make_mut(group);
        let page =
            slots[group_slot(id)].get_or_insert_with(|| Arc::new(std::array::from_fn(|_| None)));
        let entries = Arc::make_mut(page);
        let previous = entries[page_slot(id)]
            .replace((id, value))
            .map(|(_, value)| value);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    pub(super) fn remove(&mut self, id: &AccountId) -> Option<Value> {
        if !self.contains_key(id) {
            return None;
        }
        let group_key = group_id(*id);
        let group = Arc::make_mut(self.groups.get_mut(&group_key)?);
        let (removed, page_is_empty) = {
            let page = group[group_slot(*id)].as_mut()?;
            let entries = Arc::make_mut(page);
            let removed = entries[page_slot(*id)].take().map(|(_, value)| value);
            (removed, entries.iter().all(Option::is_none))
        };
        if removed.is_some() {
            self.len -= 1;
        }
        if page_is_empty {
            group[group_slot(*id)] = None;
        }
        let group_is_empty = group.iter().all(Option::is_none);
        if group_is_empty {
            self.groups.remove(&group_key);
        }
        removed.map(|value| Arc::try_unwrap(value).unwrap_or_else(|shared| (*shared).clone()))
    }

    pub(super) fn to_map(&self) -> BTreeMap<AccountId, Value> {
        self.iter()
            .map(|(id, value)| (*id, value.clone()))
            .collect()
    }
}

impl<Value: Clone> Extend<(AccountId, Value)> for AccountPagedMap<Value> {
    fn extend<Entries: IntoIterator<Item = (AccountId, Value)>>(&mut self, entries: Entries) {
        for (id, value) in entries {
            self.insert(id, value);
        }
    }
}

impl<Value: Clone> FromIterator<(AccountId, Value)> for AccountPagedMap<Value> {
    fn from_iter<Entries: IntoIterator<Item = (AccountId, Value)>>(entries: Entries) -> Self {
        let mut map = Self::default();
        map.extend(entries);
        map
    }
}

impl<Value: Clone> From<BTreeMap<AccountId, Value>> for AccountPagedMap<Value> {
    fn from(value: BTreeMap<AccountId, Value>) -> Self {
        value.into_iter().collect()
    }
}

impl<Value> Index<&AccountId> for AccountPagedMap<Value> {
    type Output = Value;

    fn index(&self, id: &AccountId) -> &Self::Output {
        self.get(id).expect("account ID is absent from paged map")
    }
}

impl<'a, Value> IntoIterator for &'a AccountPagedMap<Value> {
    type Item = (&'a AccountId, &'a Value);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<Value: serde::Serialize> serde::Serialize for AccountPagedMap<Value> {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.len))?;
        for (id, value) in self {
            map.serialize_entry(id, value)?;
        }
        map.end()
    }
}

fn page_id(id: AccountId) -> u64 {
    id.0 >> PAGE_SHIFT
}

fn group_id(id: AccountId) -> u64 {
    page_id(id) >> GROUP_SHIFT
}

fn group_slot(id: AccountId) -> usize {
    (page_id(id) & ((1 << GROUP_SHIFT) - 1)) as usize
}

fn page_slot(id: AccountId) -> usize {
    (id.0 & ((1 << PAGE_SHIFT) - 1)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CountedValue {
        value: u32,
        clones: Arc<AtomicUsize>,
    }

    impl Clone for CountedValue {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::Relaxed);
            Self {
                value: self.value,
                clones: Arc::clone(&self.clones),
            }
        }
    }

    #[test]
    fn shared_previous_clone_counter_is_a_precise_negative_control() {
        let old_value_clones = Arc::new(AtomicUsize::new(0));
        let mut committed = AccountPagedMap::default();
        committed.insert(
            AccountId(7),
            CountedValue {
                value: 7,
                clones: Arc::clone(&old_value_clones),
            },
        );
        let mut candidate = committed.clone();
        reset_shared_previous_value_clones();

        let previous = candidate.insert(
            AccountId(7),
            CountedValue {
                value: 70,
                clones: Arc::new(AtomicUsize::new(0)),
            },
        );

        assert_eq!(previous.unwrap().value, 7);
        assert_eq!(shared_previous_value_clones(), 1);
        assert_eq!(old_value_clones.load(Ordering::Relaxed), 1);
        assert_eq!(committed.get(&AccountId(7)).unwrap().value, 7);
        assert_eq!(candidate.get(&AccountId(7)).unwrap().value, 70);
    }

    #[test]
    fn a_shared_page_clones_only_the_account_being_changed() {
        let clones = Arc::new(AtomicUsize::new(0));
        let authority = (0..32)
            .map(|id| {
                (
                    AccountId(id),
                    CountedValue {
                        value: id as u32,
                        clones: Arc::clone(&clones),
                    },
                )
            })
            .collect::<AccountPagedMap<_>>();
        let mut candidate = authority.clone();

        candidate.get_mut(&AccountId(7)).unwrap().value = 700;

        assert_eq!(clones.load(Ordering::Relaxed), 1);
        assert_eq!(authority.get(&AccountId(7)).unwrap().value, 7);
        assert_eq!(candidate.get(&AccountId(7)).unwrap().value, 700);
        assert_eq!(candidate.get(&AccountId(8)).unwrap().value, 8);

        let mut replacement = authority.clone();
        replacement.insert_without_previous(
            AccountId(7),
            CountedValue {
                value: 701,
                clones: Arc::clone(&clones),
            },
        );
        assert_eq!(clones.load(Ordering::Relaxed), 1);
        assert_eq!(authority.get(&AccountId(7)).unwrap().value, 7);
        assert_eq!(replacement.get(&AccountId(7)).unwrap().value, 701);
    }

    #[test]
    fn sparse_account_ids_match_btree_map_order_and_save_bytes() {
        let ids = [
            u64::MAX,
            512,
            31,
            (1_u64 << 53) + 17,
            0,
            511,
            u64::MAX - 1,
            32,
        ];
        let mut oracle = BTreeMap::new();
        let mut candidate = AccountPagedMap::default();

        for (index, id) in ids.into_iter().enumerate() {
            let id = AccountId(id);
            let value = (index as u32) + 10;
            assert_eq!(candidate.insert(id, value), oracle.insert(id, value));
            assert_eq!(candidate.len(), oracle.len());
        }
        assert_eq!(
            candidate.insert(AccountId(32), 320),
            oracle.insert(AccountId(32), 320)
        );

        {
            let expected = oracle.iter().collect::<Vec<_>>();
            let mut forward = candidate.iter();
            let mut backward = candidate.iter();
            for (expected_id, expected_value) in &expected {
                assert_eq!(forward.next(), Some((*expected_id, *expected_value)));
            }
            assert_eq!(forward.next(), None);
            for (expected_id, expected_value) in expected.iter().rev() {
                assert_eq!(backward.next_back(), Some((*expected_id, *expected_value)));
            }
            assert_eq!(backward.next_back(), None);

            let mut mixed = candidate.iter();
            let mut remaining = expected.as_slice();
            while !remaining.is_empty() {
                let (first, rest) = remaining.split_first().unwrap();
                assert_eq!(mixed.next(), Some((first.0, first.1)));
                remaining = rest;
                if !remaining.is_empty() {
                    let (last, rest) = remaining.split_last().unwrap();
                    assert_eq!(mixed.next_back(), Some((last.0, last.1)));
                    remaining = rest;
                }
            }
            assert_eq!(mixed.next(), None);
        }
        assert_eq!(candidate.len(), oracle.len());
        assert_eq!(
            serde_json::to_vec(&candidate).unwrap(),
            serde_json::to_vec(&oracle).unwrap()
        );

        for id in [
            0,
            31,
            32,
            511,
            512,
            (1_u64 << 53) + 17,
            u64::MAX - 1,
            u64::MAX,
        ] {
            let id = AccountId(id);
            assert_eq!(candidate.remove(&id), oracle.remove(&id));
            assert_eq!(candidate.len(), oracle.len());
            assert_eq!(
                candidate.page_arc(page_id(id)).is_some(),
                oracle.keys().any(|key| page_id(*key) == page_id(id))
            );
            assert_eq!(
                candidate.groups.contains_key(&group_id(id)),
                oracle.keys().any(|key| group_id(*key) == group_id(id))
            );
        }
        assert!(candidate.is_empty());
        assert!(candidate.groups.is_empty());
        assert_eq!(
            candidate.remove(&AccountId(32)),
            oracle.remove(&AccountId(32))
        );
        let original = Arc::new(999);
        assert!(candidate
            .insert_arc(AccountId(32), Arc::clone(&original))
            .is_none());
        assert_eq!(oracle.insert(AccountId(32), 999), None);
        let replacement = Arc::new(1000);
        let previous = candidate
            .insert_arc(AccountId(32), Arc::clone(&replacement))
            .unwrap();
        assert!(Arc::ptr_eq(&previous, &original));
        assert_eq!(*previous, 999);
        assert_eq!(candidate.len(), 1);
        oracle.insert(AccountId(32), 1000);
        assert_eq!(candidate.get(&AccountId(32)), Some(replacement.as_ref()));
        assert_eq!(candidate.to_map(), oracle);
        assert_eq!(
            serde_json::to_vec(&candidate).unwrap(),
            serde_json::to_vec(&oracle).unwrap()
        );
    }

    #[test]
    fn shared_candidate_keeps_extreme_sparse_ids_isolated() {
        let id = AccountId(u64::MAX);
        let mut authority = AccountPagedMap::default();
        authority.insert(id, 7_u32);
        let mut candidate = authority.clone();

        for missing in [u64::MAX - 1, u64::MAX - 32, 0] {
            assert_eq!(candidate.get_mut(&AccountId(missing)), None);
            assert_eq!(candidate.remove(&AccountId(missing)), None);
        }
        assert!(Arc::ptr_eq(
            &authority.groups[&(id.0 >> (PAGE_SHIFT + GROUP_SHIFT))],
            &candidate.groups[&(id.0 >> (PAGE_SHIFT + GROUP_SHIFT))]
        ));
        assert!(Arc::ptr_eq(
            authority.page_arc(page_id(id)).unwrap(),
            candidate.page_arc(page_id(id)).unwrap()
        ));
        assert_eq!(candidate.insert(id, 70), Some(7));
        assert_eq!(authority.get(&id), Some(&7));
        assert_eq!(candidate.get(&id), Some(&70));
    }

    #[test]
    fn removing_shared_and_unique_values_keeps_ownership_and_page_cleanup() {
        let shared_clones = Arc::new(AtomicUsize::new(0));
        let authority = [31, 32, 512]
            .into_iter()
            .map(|id| {
                (
                    AccountId(id),
                    CountedValue {
                        value: id as u32,
                        clones: Arc::clone(&shared_clones),
                    },
                )
            })
            .collect::<AccountPagedMap<_>>();
        let mut candidate = authority.clone();

        assert_eq!(candidate.remove(&AccountId(31)).unwrap().value, 31);
        assert_eq!(shared_clones.load(Ordering::Relaxed), 1);
        assert_eq!(authority.get(&AccountId(31)).unwrap().value, 31);
        assert!(!candidate.contains_key(&AccountId(31)));
        assert!(candidate.page_arc(0).is_none());
        assert!(candidate.groups.contains_key(&0));
        assert_eq!(candidate.get(&AccountId(32)).unwrap().value, 32);
        assert_eq!(candidate.len(), 2);
        assert!(Arc::ptr_eq(&authority.groups[&1], &candidate.groups[&1]));

        let unique_clones = Arc::new(AtomicUsize::new(0));
        let mut unique = AccountPagedMap::default();
        unique.insert(
            AccountId(u64::MAX),
            CountedValue {
                value: 9,
                clones: Arc::clone(&unique_clones),
            },
        );
        assert_eq!(unique.remove(&AccountId(u64::MAX)).unwrap().value, 9);
        assert_eq!(unique_clones.load(Ordering::Relaxed), 0);
        assert!(unique.groups.is_empty());
        assert!(unique.is_empty());
    }

    #[test]
    fn candidate_detaches_only_changed_group_and_page_and_keeps_save_shape() {
        let original = BTreeMap::from([
            (AccountId(1), 10_u32),
            (AccountId(32), 20),
            (AccountId(33), 30),
            (AccountId(96), 40),
            (AccountId(4097), 50),
        ]);
        let authority = AccountPagedMap::from(original.clone());
        let mut candidate = authority.clone();
        assert!(Arc::ptr_eq(&authority.groups[&0], &candidate.groups[&0]));
        assert!(Arc::ptr_eq(&authority.groups[&8], &candidate.groups[&8]));
        assert!(Arc::ptr_eq(
            authority.page_arc(0).unwrap(),
            candidate.page_arc(0).unwrap()
        ));
        assert!(Arc::ptr_eq(
            authority.page_arc(1).unwrap(),
            candidate.page_arc(1).unwrap()
        ));

        *candidate.get_mut(&AccountId(32)).unwrap() = 21;

        assert!(!Arc::ptr_eq(&authority.groups[&0], &candidate.groups[&0]));
        assert!(Arc::ptr_eq(&authority.groups[&8], &candidate.groups[&8]));
        assert!(Arc::ptr_eq(
            authority.page_arc(0).unwrap(),
            candidate.page_arc(0).unwrap()
        ));
        assert!(!Arc::ptr_eq(
            authority.page_arc(1).unwrap(),
            candidate.page_arc(1).unwrap()
        ));
        assert_eq!(authority.to_map(), original);
        assert_eq!(candidate[&AccountId(33)], 30);
        assert_eq!(
            serde_json::to_vec(&authority).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&candidate).unwrap(),
            serde_json::to_vec(&BTreeMap::from([
                (AccountId(1), 10_u32),
                (AccountId(32), 21),
                (AccountId(33), 30),
                (AccountId(96), 40),
                (AccountId(4097), 50),
            ]))
            .unwrap()
        );
    }
}
