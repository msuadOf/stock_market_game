//! Per-account state shared between a committed session and its tick candidate.

use crate::AccountId;
use serde::ser::SerializeMap;
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::Arc;

const PAGE_SHIFT: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AccountPagedMap<Value> {
    pages: BTreeMap<u64, Arc<BTreeMap<AccountId, Value>>>,
    len: usize,
}

impl<Value: Eq> Eq for AccountPagedMap<Value> {}

impl<Value> Default for AccountPagedMap<Value> {
    fn default() -> Self {
        Self {
            pages: BTreeMap::new(),
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
        self.pages.get(&page_id(*id))?.get(id)
    }

    pub(super) fn contains_key(&self, id: &AccountId) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = (&AccountId, &Value)> {
        self.pages.values().flat_map(|page| page.iter())
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &AccountId> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.iter().map(|(_, value)| value)
    }
}

impl<Value: Clone> AccountPagedMap<Value> {
    pub(super) fn get_mut(&mut self, id: &AccountId) -> Option<&mut Value> {
        let page = self.pages.get_mut(&page_id(*id))?;
        if !page.contains_key(id) {
            return None;
        }
        Arc::make_mut(page).get_mut(id)
    }

    pub(super) fn insert(&mut self, id: AccountId, value: Value) -> Option<Value> {
        let previous = Arc::make_mut(self.pages.entry(page_id(id)).or_default()).insert(id, value);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    pub(super) fn remove(&mut self, id: &AccountId) -> Option<Value> {
        let page_key = page_id(*id);
        let page = self.pages.get_mut(&page_key)?;
        if !page.contains_key(id) {
            return None;
        }
        let removed = Arc::make_mut(page).remove(id);
        if removed.is_some() {
            self.len -= 1;
        }
        if page.is_empty() {
            self.pages.remove(&page_key);
        }
        removed
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_detaches_only_changed_page_and_keeps_save_shape() {
        let original = BTreeMap::from([
            (AccountId(1), 10_u32),
            (AccountId(32), 20),
            (AccountId(33), 30),
            (AccountId(96), 40),
            (AccountId(4097), 50),
        ]);
        let authority = AccountPagedMap::from(original.clone());
        let mut candidate = authority.clone();
        assert!(Arc::ptr_eq(&authority.pages[&0], &candidate.pages[&0]));
        assert!(Arc::ptr_eq(&authority.pages[&1], &candidate.pages[&1]));

        *candidate.get_mut(&AccountId(32)).unwrap() = 21;

        assert!(Arc::ptr_eq(&authority.pages[&0], &candidate.pages[&0]));
        assert!(!Arc::ptr_eq(&authority.pages[&1], &candidate.pages[&1]));
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
