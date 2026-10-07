//! Account storage for a private tick candidate.
//!
//! Adjacent account pages share a directory group across tick candidates.
//! Changing an account detaches its group and page, leaving unrelated groups shared.

use crate::strategy::StrategyStateError;
use crate::{Account, AccountId};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::{Arc, Mutex, OnceLock};

const PAGE_SHIFT: u32 = super::pipeline::RESERVATION_PAGE_SHIFT;
const GROUP_SHIFT: u32 = 4;
#[cfg(test)]
thread_local! {
    static STRATEGY_PRESENCE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
type PageGroup = BTreeMap<u64, Arc<AccountPage>>;

#[derive(Clone, Default)]
pub(super) struct AccountBook {
    groups: BTreeMap<u64, Arc<PageGroup>>,
    len: usize,
}

#[derive(Default)]
struct AccountPage {
    accounts: BTreeMap<AccountId, Account>,
    validated: OnceLock<Result<(), StrategyStateError>>,
    missing_npc_strategy: OnceLock<bool>,
    resource_validation: Mutex<Option<ResourceValidationCache>>,
}

struct ResourceValidationCache {
    prices: Arc<BTreeMap<crate::StockId, crate::Money>>,
    reservations: Option<Arc<super::pipeline::ReservationPage>>,
}

impl Clone for AccountPage {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            validated: OnceLock::new(),
            missing_npc_strategy: OnceLock::new(),
            resource_validation: Mutex::new(None),
        }
    }
}

impl AccountPage {
    fn validate(&self) -> Result<(), StrategyStateError> {
        self.validated
            .get_or_init(|| {
                for account in self.accounts.values() {
                    if let Some(strategy) = account.strategy() {
                        strategy.validate_for_shadow()?;
                    }
                }
                Ok(())
            })
            .clone()
    }

    fn invalidate(&mut self) {
        self.validated = OnceLock::new();
        self.missing_npc_strategy = OnceLock::new();
        self.resource_validation = Mutex::new(None);
    }
}

impl AccountBook {
    pub(super) fn has_missing_npc_strategy(&self) -> bool {
        self.groups
            .values()
            .flat_map(|group| group.values())
            .any(|page| {
                *page.missing_npc_strategy.get_or_init(|| {
                    page.accounts.values().any(|account| {
                        #[cfg(test)]
                        STRATEGY_PRESENCE_SCANS.with(|count| count.set(count.get() + 1));
                        account.kind() != crate::AccountKind::Player && account.strategy().is_none()
                    })
                })
            })
    }
    /// 在相同账户页、价格及预留事实下校验资源。
    pub(super) fn validate_resources(
        &self,
        prices: &BTreeMap<crate::StockId, crate::Money>,
        reservations: &super::pipeline::LiveReservations,
        validate: impl Fn(AccountId, &Account) -> Result<(), super::StepFatal> + Sync,
    ) -> Result<(), super::StepFatal> {
        let prices = Arc::new(prices.clone());
        self.groups
            .values()
            .flat_map(|group| group.iter())
            .collect::<Vec<_>>()
            .into_par_iter()
            .try_for_each(|(page_id, page)| {
                let projection = reservations.page(*page_id);
                let mut cache = page.resource_validation.lock().map_err(|error| {
                    super::StepFatal::InvariantViolation {
                        description: format!("账户资源校验缓存锁失败：{error}"),
                        location: "AccountBook::validate_resources".to_owned(),
                    }
                })?;
                if cache.as_ref().is_some_and(|cached| {
                    cached.prices.as_ref() == prices.as_ref()
                        && match (&cached.reservations, projection) {
                            (None, None) => true,
                            (Some(old), Some(current)) => {
                                Arc::ptr_eq(old, current) || old == current
                            }
                            _ => false,
                        }
                }) {
                    return Ok(());
                }
                for (id, account) in &page.accounts {
                    validate(*id, account)?;
                }
                *cache = Some(ResourceValidationCache {
                    prices: prices.clone(),
                    reservations: projection.cloned(),
                });
                Ok(())
            })
    }
    pub(super) fn clone_for_shadow(&self) -> Result<Self, StrategyStateError> {
        for group in self.groups.values() {
            for page in group.values() {
                page.validate()?;
            }
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
        self.groups
            .values()
            .flat_map(|group| group.values().flat_map(|page| page.accounts.iter()))
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &AccountId> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &Account> {
        self.iter().map(|(_, account)| account)
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut Account> {
        self.groups.values_mut().flat_map(|group| {
            Arc::make_mut(group).values_mut().flat_map(|page| {
                let page = Arc::make_mut(page);
                page.invalidate();
                page.accounts.values_mut()
            })
        })
    }

    pub(super) fn get(&self, id: &AccountId) -> Option<&Account> {
        self.page_arc(page_id(*id))?.accounts.get(id)
    }

    pub(super) fn get_mut(&mut self, id: &AccountId) -> Option<&mut Account> {
        if !self.contains_key(id) {
            return None;
        }
        let group = Arc::make_mut(self.groups.get_mut(&group_id(*id))?);
        let page = group.get_mut(&page_id(*id))?;
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
                    .positions()
                    .values()
                    .any(|position| position.t1_locked() > 0)
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
        let group = Arc::make_mut(self.groups.entry(group_id(id)).or_default());
        let page = group.entry(page_id(id)).or_default();
        let page = Arc::make_mut(page);
        page.invalidate();
        let previous = page.accounts.insert(id, account);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    fn page_arc(&self, page: u64) -> Option<&Arc<AccountPage>> {
        self.groups.get(&(page >> GROUP_SHIFT))?.get(&page)
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

fn group_id(id: AccountId) -> u64 {
    page_id(id) >> GROUP_SHIFT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{MomentumStrategy, StrategyState};
    use crate::{AccountKind, Money, StockId};

    #[test]
    fn strategy_presence_reuses_unchanged_pages_and_rechecks_mutation() {
        let mut accounts = AccountBook::default();
        for id in [0, 32] {
            accounts.insert(
                AccountId(id),
                Account::new(AccountId(id), AccountKind::Player, Money::ZERO),
            );
        }
        STRATEGY_PRESENCE_SCANS.with(|count| count.set(0));
        assert!(!accounts.has_missing_npc_strategy());
        let mut candidate = accounts.clone_for_shadow().unwrap();
        assert!(!candidate.has_missing_npc_strategy());
        assert_eq!(
            STRATEGY_PRESENCE_SCANS.with(|count| count.get()),
            2,
            "共享页不应重复全账户检查"
        );
        candidate
            .get_mut(&AccountId(0))
            .unwrap()
            .fixture_set_kind(AccountKind::Retail);
        assert!(candidate.has_missing_npc_strategy());
        assert_eq!(STRATEGY_PRESENCE_SCANS.with(|count| count.get()), 3);
        assert!(!accounts.has_missing_npc_strategy());
        candidate
            .get_mut(&AccountId(0))
            .unwrap()
            .fixture_set_kind(AccountKind::Player);
        assert!(!candidate.has_missing_npc_strategy());
        assert_eq!(
            STRATEGY_PRESENCE_SCANS.with(|count| count.get()),
            4,
            "恢复策略合法性后只重验变化页"
        );
    }

    #[test]
    fn resource_validation_reuses_immutable_pages_and_invalidates_every_input() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut accounts = AccountBook::default();
        for id in [0, 32] {
            accounts.insert(
                AccountId(id),
                Account::new(AccountId(id), AccountKind::Player, Money::from_cents(100)),
            );
        }
        let calls = AtomicUsize::new(0);
        let validate = |_: AccountId, _: &Account| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        };
        let mut prices = BTreeMap::from([(StockId(0), Money::from_cents(100))]);
        let mut cash = BTreeMap::new();
        let mut sell = BTreeMap::new();
        accounts
            .validate_resources(
                &prices,
                &super::super::pipeline::LiveReservations::fixture(&cash, &sell),
                &validate,
            )
            .unwrap();
        let mut shadow = accounts.clone_for_shadow().unwrap();
        shadow
            .validate_resources(
                &prices,
                &super::super::pipeline::LiveReservations::fixture(&cash, &sell),
                &validate,
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2, "未变化的页不应重复计算");
        shadow
            .get_mut(&AccountId(0))
            .unwrap()
            .fixture_set_cash(Money::from_cents(90));
        shadow
            .validate_resources(
                &prices,
                &super::super::pipeline::LiveReservations::fixture(&cash, &sell),
                &validate,
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 3, "只重验发生 mutation 的页");
        prices.insert(StockId(0), Money::from_cents(101));
        shadow
            .validate_resources(
                &prices,
                &super::super::pipeline::LiveReservations::fixture(&cash, &sell),
                &validate,
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 5, "价格变化必须重验全部页");
        cash.insert(AccountId(32), Money::from_cents(5));
        shadow
            .validate_resources(
                &prices,
                &super::super::pipeline::LiveReservations::fixture(&cash, &sell),
                &validate,
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 6, "现金预留只使受影响页失效");
        sell.insert(AccountId(0), BTreeMap::from([(StockId(0), 100)]));
        shadow
            .validate_resources(
                &prices,
                &super::super::pipeline::LiveReservations::fixture(&cash, &sell),
                &validate,
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 7, "股份预留只使受影响页失效");
        accounts
            .validate_resources(
                &BTreeMap::from([(StockId(0), Money::from_cents(100))]),
                &super::super::pipeline::LiveReservations::fixture(
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                ),
                &validate,
            )
            .unwrap();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            8,
            "shadow 的预留缓存不得错误适用于旧 authority"
        );
    }

    #[test]
    fn failed_resource_validation_is_not_cached_as_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut accounts = AccountBook::default();
        accounts.insert(
            AccountId(0),
            Account::new(AccountId(0), AccountKind::Player, Money::ZERO),
        );
        let prices = BTreeMap::from([(StockId(0), Money::from_cents(100))]);
        let reservations =
            super::super::pipeline::LiveReservations::fixture(&BTreeMap::new(), &BTreeMap::new());
        let calls = AtomicUsize::new(0);
        let error = super::super::StepFatal::InvariantViolation {
            description: "资源校验必须重复报告同一个错误".to_owned(),
            location: "test::resource_validation".to_owned(),
        };
        for _ in 0..2 {
            assert_eq!(
                accounts.validate_resources(&prices, &reservations, |_, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Err(error.clone())
                }),
                Err(error.clone())
            );
        }
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert!(accounts
            .page_arc(0)
            .unwrap()
            .resource_validation
            .lock()
            .unwrap()
            .is_none());
    }

    #[test]
    fn shadow_detaches_only_a_changed_group_and_account_page() {
        let mut authority = AccountBook::default();
        for id in [0, 1, 32, 33, 512] {
            authority.insert(
                AccountId(id),
                Account::new(AccountId(id), AccountKind::Player, Money::ZERO),
            );
        }
        let mut shadow = authority.clone_for_shadow().unwrap();
        assert!(Arc::ptr_eq(
            authority.page_arc(0).unwrap(),
            shadow.page_arc(0).unwrap()
        ));
        assert!(Arc::ptr_eq(
            authority.page_arc(1).unwrap(),
            shadow.page_arc(1).unwrap()
        ));
        assert!(Arc::ptr_eq(&authority.groups[&0], &shadow.groups[&0]));
        assert!(Arc::ptr_eq(&authority.groups[&1], &shadow.groups[&1]));
        assert!(authority.page_arc(0).unwrap().validated.get().is_some());
        assert!(authority.page_arc(1).unwrap().validated.get().is_some());

        shadow
            .get_mut(&AccountId(1))
            .unwrap()
            .fixture_set_cash(Money::from_cents(100));
        assert!(!Arc::ptr_eq(
            authority.page_arc(0).unwrap(),
            shadow.page_arc(0).unwrap()
        ));
        assert!(Arc::ptr_eq(
            authority.page_arc(1).unwrap(),
            shadow.page_arc(1).unwrap()
        ));
        assert!(!Arc::ptr_eq(&authority.groups[&0], &shadow.groups[&0]));
        assert!(Arc::ptr_eq(&authority.groups[&1], &shadow.groups[&1]));
        assert!(shadow.page_arc(0).unwrap().validated.get().is_none());
        assert!(shadow.page_arc(1).unwrap().validated.get().is_some());
        let next = shadow.clone_for_shadow().unwrap();
        assert!(next.page_arc(0).unwrap().validated.get().is_some());
        assert_eq!(authority[&AccountId(1)].cash(), Money::ZERO);
        assert_eq!(shadow[&AccountId(1)].cash(), Money::from_cents(100));
        assert_eq!(
            shadow.keys().copied().collect::<Vec<_>>(),
            [0, 1, 32, 33, 512].map(AccountId)
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
        assert!(authority.page_arc(0).unwrap().validated.get().is_some());

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
        assert!(authority.page_arc(0).unwrap().validated.get().is_none());
        assert!(authority.clone_for_shadow().is_err());
    }

    #[test]
    fn day_end_unlock_detaches_only_pages_with_locked_shares() {
        let code = StockId(0);
        let mut authority = AccountBook::default();
        for id in [1, 32] {
            let mut account = Account::new(AccountId(id), AccountKind::Player, Money::ZERO);
            account
                .grant_position(code.clone(), 100, Money::from_cents(1_000))
                .unwrap();
            authority.insert(AccountId(id), account);
        }
        let position = authority[&AccountId(1)].position(&code).unwrap();
        let locked = crate::Position::from_restored_parts(
            position.qty(),
            100,
            position.invested_cents(),
            position.recovered_cents(),
        );
        authority
            .get_mut(&AccountId(1))
            .unwrap()
            .fixture_insert_position(code.clone(), locked);
        let mut shadow = authority.clone_for_shadow().unwrap();

        shadow.unlock_t1_positions();

        assert!(!Arc::ptr_eq(
            authority.page_arc(0).unwrap(),
            shadow.page_arc(0).unwrap()
        ));
        assert!(Arc::ptr_eq(
            authority.page_arc(1).unwrap(),
            shadow.page_arc(1).unwrap()
        ));
        assert_eq!(authority[&AccountId(1)].positions()[&code].t1_locked(), 100);
        assert_eq!(shadow[&AccountId(1)].positions()[&code].t1_locked(), 0);
        assert_eq!(shadow[&AccountId(32)].positions()[&code].t1_locked(), 0);
    }
}
