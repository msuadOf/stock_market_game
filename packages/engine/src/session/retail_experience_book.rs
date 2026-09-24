//! Retail experience shared by account page between tick candidates.

use crate::experience::RetailExperienceState;
use crate::AccountId;
use serde::ser::SerializeMap;
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::Arc;

const PAGE_SHIFT: u32 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RetailExperienceBook {
    pages: BTreeMap<u64, Arc<BTreeMap<AccountId, RetailExperienceState>>>,
    len: usize,
}

impl Default for RetailExperienceBook {
    fn default() -> Self {
        Self {
            pages: BTreeMap::new(),
            len: 0,
        }
    }
}

impl RetailExperienceBook {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn get(&self, id: &AccountId) -> Option<&RetailExperienceState> {
        self.pages.get(&page_id(*id))?.get(id)
    }

    pub(super) fn get_mut(&mut self, id: &AccountId) -> Option<&mut RetailExperienceState> {
        let page = self.pages.get_mut(&page_id(*id))?;
        if !page.contains_key(id) {
            return None;
        }
        Arc::make_mut(page).get_mut(id)
    }

    pub(super) fn contains_key(&self, id: &AccountId) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn insert(
        &mut self,
        id: AccountId,
        value: RetailExperienceState,
    ) -> Option<RetailExperienceState> {
        let previous = Arc::make_mut(self.pages.entry(page_id(id)).or_default()).insert(id, value);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    pub(super) fn remove(&mut self, id: &AccountId) -> Option<RetailExperienceState> {
        let page_id = page_id(*id);
        let page = self.pages.get_mut(&page_id)?;
        if !page.contains_key(id) {
            return None;
        }
        let previous = Arc::make_mut(page).remove(id);
        if previous.is_some() {
            self.len -= 1;
        }
        if page.is_empty() {
            self.pages.remove(&page_id);
        }
        previous
    }

    pub(super) fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&AccountId, &RetailExperienceState)> {
        self.pages.values().flat_map(|page| page.iter())
    }

    #[cfg(test)]
    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &AccountId> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn to_map(&self) -> BTreeMap<AccountId, RetailExperienceState> {
        self.iter()
            .map(|(id, value)| (*id, value.clone()))
            .collect()
    }
}

impl Extend<(AccountId, RetailExperienceState)> for RetailExperienceBook {
    fn extend<I: IntoIterator<Item = (AccountId, RetailExperienceState)>>(&mut self, entries: I) {
        for (id, value) in entries {
            self.insert(id, value);
        }
    }
}

impl FromIterator<(AccountId, RetailExperienceState)> for RetailExperienceBook {
    fn from_iter<I: IntoIterator<Item = (AccountId, RetailExperienceState)>>(entries: I) -> Self {
        let mut map = Self::default();
        map.extend(entries);
        map
    }
}

impl From<BTreeMap<AccountId, RetailExperienceState>> for RetailExperienceBook {
    fn from(value: BTreeMap<AccountId, RetailExperienceState>) -> Self {
        value.into_iter().collect()
    }
}

impl Index<&AccountId> for RetailExperienceBook {
    type Output = RetailExperienceState;

    fn index(&self, id: &AccountId) -> &Self::Output {
        self.get(id).expect("account ID is absent from paged map")
    }
}

impl<'a> IntoIterator for &'a RetailExperienceBook {
    type Item = (&'a AccountId, &'a RetailExperienceState);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl serde::Serialize for RetailExperienceBook {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
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
    fn tick_candidate_shares_quiet_pages_and_detaches_only_the_changed_page() {
        let mut authority = RetailExperienceBook::default();
        for id in [1, 32, 33] {
            authority.insert(
                AccountId(id),
                RetailExperienceState::without_equity_reference(),
            );
        }
        let mut candidate = authority.clone();
        assert!(Arc::ptr_eq(&authority.pages[&0], &candidate.pages[&0]));
        assert!(Arc::ptr_eq(&authority.pages[&1], &candidate.pages[&1]));

        candidate
            .get_mut(&AccountId(32))
            .unwrap()
            .consecutive_failed_buys = 2;
        assert!(Arc::ptr_eq(&authority.pages[&0], &candidate.pages[&0]));
        assert!(!Arc::ptr_eq(&authority.pages[&1], &candidate.pages[&1]));
        assert_eq!(authority[&AccountId(32)].consecutive_failed_buys, 0);
        assert_eq!(candidate[&AccountId(32)].consecutive_failed_buys, 2);
        assert_eq!(
            candidate.keys().copied().collect::<Vec<_>>(),
            [1, 32, 33].map(AccountId)
        );
    }

    #[test]
    fn sparse_pages_keep_the_existing_save_and_hash_json_shape() {
        let original = BTreeMap::from([
            (
                AccountId(1),
                RetailExperienceState::without_equity_reference(),
            ),
            (
                AccountId(96),
                RetailExperienceState::without_equity_reference(),
            ),
            (
                AccountId(4097),
                RetailExperienceState::without_equity_reference(),
            ),
        ]);
        let authority = RetailExperienceBook::from(original.clone());
        assert_eq!(
            serde_json::to_vec(&authority).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );

        let mut candidate = authority.clone();
        candidate
            .get_mut(&AccountId(96))
            .unwrap()
            .consecutive_failed_buys = 2;
        assert_eq!(
            serde_json::to_vec(&authority).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
        assert_ne!(
            serde_json::to_vec(&candidate).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
    }
}
