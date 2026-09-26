//! Per-account state shared between a committed session and its tick candidate.

use crate::AccountId;
use rayon::prelude::*;
use serde::ser::SerializeMap;
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::Arc;

const PAGE_SHIFT: u32 = 5;
const GROUP_SHIFT: u32 = 4;
type PageGroup<Value> = BTreeMap<u64, Arc<BTreeMap<AccountId, Arc<Value>>>>;

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
        self.page_arc(page_id(*id))?.get(id).map(Arc::as_ref)
    }

    pub(super) fn contains_key(&self, id: &AccountId) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = (&AccountId, &Value)> {
        self.groups.values().flat_map(|group| {
            group
                .values()
                .flat_map(|page| page.iter().map(|(id, value)| (id, value.as_ref())))
        })
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &AccountId> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.iter().map(|(_, value)| value)
    }

    fn page_arc(&self, page: u64) -> Option<&Arc<BTreeMap<AccountId, Arc<Value>>>> {
        self.groups.get(&(page >> GROUP_SHIFT))?.get(&page)
    }
}

impl<Value: Clone> AccountPagedMap<Value> {
    /// Retire the old authority's independent page groups after installing a
    /// completed tick candidate. Destruction can be substantial when most
    /// accounts acquired a fresh value during the tick.
    pub(super) fn replace_and_drop_parallel(&mut self, replacement: Self)
    where
        Value: Send + Sync,
    {
        let old = std::mem::replace(self, replacement);
        old.groups
            .into_values()
            .collect::<Vec<_>>()
            .into_par_iter()
            .for_each(drop);
    }

    pub(super) fn replace_existing_shared_parallel(
        &mut self,
        updates: Vec<(AccountId, Arc<Value>)>,
    ) -> Result<(), AccountId>
    where
        Value: Send + Sync,
    {
        let mut by_group = BTreeMap::<u64, Vec<(AccountId, Arc<Value>)>>::new();
        for (id, value) in updates {
            by_group.entry(group_id(id)).or_default().push((id, value));
        }
        let work = self
            .groups
            .iter_mut()
            .filter_map(|(group_id, group)| by_group.remove(group_id).map(|rows| (group, rows)))
            .collect::<Vec<_>>();
        if let Some(rows) = by_group.into_values().next() {
            return Err(rows[0].0);
        }
        work.into_par_iter().try_for_each(|(group, rows)| {
            let pages = Arc::make_mut(group);
            for (id, value) in rows {
                let page = pages.get_mut(&page_id(id)).ok_or(id)?;
                if Arc::make_mut(page).insert(id, value).is_none() {
                    return Err(id);
                }
            }
            Ok(())
        })
    }

    /// Change independent existing accounts on disjoint pages of a private tick state.
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
                let pages = Arc::make_mut(group);
                let mut results = Vec::with_capacity(ids.len());
                for id in ids {
                    let page = pages.get_mut(&page_id(id)).ok_or_else(|| missing(id))?;
                    let value = Arc::make_mut(page)
                        .get_mut(&id)
                        .ok_or_else(|| missing(id))?;
                    results.push((id, change(id, Arc::make_mut(value))?));
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
        let page = group.get_mut(&page_id(*id))?;
        Arc::make_mut(page).get_mut(id).map(Arc::make_mut)
    }

    pub(super) fn insert(&mut self, id: AccountId, value: Value) -> Option<Value> {
        self.insert_shared(id, value)
            .map(|previous| Arc::try_unwrap(previous).unwrap_or_else(|shared| (*shared).clone()))
    }

    pub(super) fn insert_without_previous(&mut self, id: AccountId, value: Value) {
        self.insert_shared(id, value);
    }

    fn insert_shared(&mut self, id: AccountId, value: Value) -> Option<Arc<Value>> {
        self.insert_arc(id, Arc::new(value))
    }

    fn insert_arc(&mut self, id: AccountId, value: Arc<Value>) -> Option<Arc<Value>> {
        let group = Arc::make_mut(self.groups.entry(group_id(id)).or_default());
        let previous = Arc::make_mut(group.entry(page_id(id)).or_default()).insert(id, value);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    pub(super) fn remove(&mut self, id: &AccountId) -> Option<Value> {
        if !self.contains_key(id) {
            return None;
        }
        let page_key = page_id(*id);
        let group_key = group_id(*id);
        let group = Arc::make_mut(self.groups.get_mut(&group_key)?);
        let page = group.get_mut(&page_key)?;
        let removed = Arc::make_mut(page).remove(id);
        if removed.is_some() {
            self.len -= 1;
        }
        if page.is_empty() {
            group.remove(&page_key);
        }
        if group.is_empty() {
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
