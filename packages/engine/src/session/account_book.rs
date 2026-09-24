//! Account storage for a private tick candidate.
//!
//! Pages are ordered ranges of account IDs. Cloning a candidate shares every
//! page; changing one account copies only that page and the account's own state.

use crate::strategy::StrategyStateError;
use crate::{Account, AccountId};
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::{Arc, OnceLock};

const PAGE_SHIFT: u32 = 5;

#[derive(Clone, Default)]
pub(super) struct AccountBook {
    pages: BTreeMap<u64, Arc<AccountPage>>,
    len: usize,
}

#[derive(Default)]
struct AccountPage {
    accounts: BTreeMap<AccountId, Account>,
    validated: OnceLock<Result<(), StrategyStateError>>,
}

impl Clone for AccountPage {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            validated: OnceLock::new(),
        }
    }
}

impl AccountPage {
    fn validate(&self) -> Result<(), StrategyStateError> {
        self.validated
            .get_or_init(|| {
                for account in self.accounts.values() {
                    if let Some(strategy) = &account.strategy {
                        strategy.validate_for_shadow()?;
                    }
                }
                Ok(())
            })
            .clone()
    }

    fn invalidate(&mut self) {
        self.validated = OnceLock::new();
    }
}

impl AccountBook {
    pub(super) fn clone_for_shadow(&self) -> Result<Self, StrategyStateError> {
        for page in self.pages.values() {
            page.validate()?;
        }
        Ok(self.clone())
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = (&AccountId, &Account)> {
        self.pages.values().flat_map(|page| page.accounts.iter())
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &AccountId> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &Account> {
        self.iter().map(|(_, account)| account)
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut Account> {
        self.pages.values_mut().flat_map(|page| {
            let page = Arc::make_mut(page);
            page.invalidate();
            page.accounts.values_mut()
        })
    }

    pub(super) fn get(&self, id: &AccountId) -> Option<&Account> {
        self.pages.get(&page_id(*id))?.accounts.get(id)
    }

    pub(super) fn get_mut(&mut self, id: &AccountId) -> Option<&mut Account> {
        let page = self.pages.get_mut(&page_id(*id))?;
        if !page.accounts.contains_key(id) {
            return None;
        }
        let page = Arc::make_mut(page);
        page.invalidate();
        page.accounts.get_mut(id)
    }

    pub(super) fn contains_key(&self, id: &AccountId) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn unlock_t1_positions(&mut self) {
        let locked_accounts = self
            .iter()
            .filter_map(|(id, account)| {
                account
                    .positions
                    .values()
                    .any(|position| position.t1_locked > 0)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in locked_accounts {
            self.get_mut(&id)
                .expect("the locked account came from this account book")
                .unlock_t1_positions();
        }
    }

    pub(super) fn insert(&mut self, id: AccountId, account: Account) -> Option<Account> {
        let page = self.pages.entry(page_id(id)).or_default();
        let page = Arc::make_mut(page);
        page.invalidate();
        let previous = page.accounts.insert(id, account);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }
}

impl Extend<(AccountId, Account)> for AccountBook {
    fn extend<T: IntoIterator<Item = (AccountId, Account)>>(&mut self, entries: T) {
        for (id, account) in entries {
            self.insert(id, account);
        }
    }
}

impl FromIterator<(AccountId, Account)> for AccountBook {
    fn from_iter<T: IntoIterator<Item = (AccountId, Account)>>(entries: T) -> Self {
        let mut book = Self::default();
        book.extend(entries);
        book
    }
}

impl From<BTreeMap<AccountId, Account>> for AccountBook {
    fn from(accounts: BTreeMap<AccountId, Account>) -> Self {
        accounts.into_iter().collect()
    }
}

impl Index<&AccountId> for AccountBook {
    type Output = Account;

    fn index(&self, id: &AccountId) -> &Self::Output {
        self.get(id).expect("account ID is absent")
    }
}

impl<'a> IntoIterator for &'a AccountBook {
    type Item = (&'a AccountId, &'a Account);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

fn page_id(id: AccountId) -> u64 {
    id.0 >> PAGE_SHIFT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{MomentumStrategy, StrategyState};
    use crate::{AccountKind, Money, StockCode};

    #[test]
    fn shadow_detaches_only_a_changed_account_page() {
        let mut authority = AccountBook::default();
        for id in [0, 1, 32, 33] {
            authority.insert(
                AccountId(id),
                Account::new(AccountId(id), AccountKind::Player, Money::ZERO),
            );
        }
        let mut shadow = authority.clone_for_shadow().unwrap();
        assert!(Arc::ptr_eq(&authority.pages[&0], &shadow.pages[&0]));
        assert!(Arc::ptr_eq(&authority.pages[&1], &shadow.pages[&1]));
        assert!(authority.pages[&0].validated.get().is_some());
        assert!(authority.pages[&1].validated.get().is_some());

        shadow.get_mut(&AccountId(1)).unwrap().cash = Money::from_cents(100);
        assert!(!Arc::ptr_eq(&authority.pages[&0], &shadow.pages[&0]));
        assert!(Arc::ptr_eq(&authority.pages[&1], &shadow.pages[&1]));
        assert!(shadow.pages[&0].validated.get().is_none());
        assert!(shadow.pages[&1].validated.get().is_some());
        let next = shadow.clone_for_shadow().unwrap();
        assert!(next.pages[&0].validated.get().is_some());
        assert_eq!(authority[&AccountId(1)].cash, Money::ZERO);
        assert_eq!(shadow[&AccountId(1)].cash, Money::from_cents(100));
        assert_eq!(
            shadow.keys().copied().collect::<Vec<_>>(),
            [0, 1, 32, 33].map(AccountId)
        );
    }

    #[test]
    fn replaced_strategy_invalidates_the_page_validation_cache() {
        let mut authority = AccountBook::default();
        authority.insert(
            AccountId(1),
            Account::new(AccountId(1), AccountKind::Hot, Money::ZERO),
        );
        authority
            .get_mut(&AccountId(1))
            .unwrap()
            .set_strategy(Box::new(MomentumStrategy::new(5, 0.02, 100).unwrap()));
        authority.clone_for_shadow().unwrap();
        assert!(authority.pages[&0].validated.get().is_some());

        let valid = StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap());
        let mut invalid = serde_json::to_value(valid).unwrap();
        invalid["Momentum"]["order_size"] = serde_json::json!(0);
        let StrategyState::Momentum(invalid) = serde_json::from_value(invalid).unwrap() else {
            panic!("the malformed test state changed strategy variant");
        };
        authority
            .get_mut(&AccountId(1))
            .unwrap()
            .set_strategy(Box::new(invalid));
        assert!(authority.pages[&0].validated.get().is_none());
        assert!(authority.clone_for_shadow().is_err());
    }

    #[test]
    fn day_end_unlock_detaches_only_pages_with_locked_shares() {
        let code = StockCode("600001".to_owned());
        let mut authority = AccountBook::default();
        for id in [1, 32] {
            let mut account = Account::new(AccountId(id), AccountKind::Player, Money::ZERO);
            account
                .grant_position(code.clone(), 100, Money::from_cents(1_000))
                .unwrap();
            authority.insert(AccountId(id), account);
        }
        authority
            .get_mut(&AccountId(1))
            .unwrap()
            .positions
            .get_mut(&code)
            .unwrap()
            .t1_locked = 100;
        let mut shadow = authority.clone_for_shadow().unwrap();

        shadow.unlock_t1_positions();

        assert!(!Arc::ptr_eq(&authority.pages[&0], &shadow.pages[&0]));
        assert!(Arc::ptr_eq(&authority.pages[&1], &shadow.pages[&1]));
        assert_eq!(authority[&AccountId(1)].positions[&code].t1_locked, 100);
        assert_eq!(shadow[&AccountId(1)].positions[&code].t1_locked, 0);
        assert_eq!(shadow[&AccountId(32)].positions[&code].t1_locked, 0);
    }
}
