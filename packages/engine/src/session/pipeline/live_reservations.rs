//! 从不可变 live ledger 页投影资源预留；终结证据不进入下一密封批的预留。
use super::decision_resources::{allocation_invariant, allocation_money};
use super::{ledger_paged_map::LedgerPagedMap, Envelope, EnvelopeKey, StepFatal};
use crate::{AccountId, Money, StockId};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

pub(in crate::session) const RESERVATION_PAGE_SHIFT: u32 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session) struct AccountReservation {
    pub(in crate::session) account: AccountId,
    pub(in crate::session) cash: Money,
    pub(in crate::session) sell: BTreeMap<StockId, u32>,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::session) struct ReservationPage {
    rows: Vec<AccountReservation>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::session) struct LiveReservations {
    pages: Arc<BTreeMap<u64, CachedPage>>,
}

impl LiveReservations {
    #[cfg(test)]
    pub(in crate::session) fn fixture(
        cash: &BTreeMap<AccountId, Money>,
        sell: &BTreeMap<AccountId, BTreeMap<StockId, u32>>,
    ) -> Self {
        let ids = cash
            .keys()
            .chain(sell.keys())
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let mut pages = BTreeMap::<u64, Vec<AccountReservation>>::new();
        for account in ids {
            pages
                .entry(account.0 >> RESERVATION_PAGE_SHIFT)
                .or_default()
                .push(AccountReservation {
                    account,
                    cash: cash.get(&account).copied().unwrap_or(Money::ZERO),
                    sell: sell.get(&account).cloned().unwrap_or_default(),
                });
        }
        Self {
            pages: pages
                .into_iter()
                .map(|(id, rows)| {
                    (
                        id,
                        CachedPage {
                            source: Weak::new(),
                            projection: Arc::new(ReservationPage { rows }),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .into(),
        }
    }

    pub(in crate::session) fn page(&self, id: u64) -> Option<&Arc<ReservationPage>> {
        self.pages.get(&id).map(|cached| &cached.projection)
    }
    pub(in crate::session) fn get(&self, account: AccountId) -> Option<&AccountReservation> {
        let rows = &self.page(account.0 >> RESERVATION_PAGE_SHIFT)?.rows;
        let index = rows
            .binary_search_by_key(&account, |row| row.account)
            .ok()?;
        Some(&rows[index])
    }
    pub(in crate::session) fn cash(&self, account: AccountId) -> Money {
        self.get(account).map_or(Money::ZERO, |row| row.cash)
    }
    pub(in crate::session) fn iter(&self) -> impl Iterator<Item = &AccountReservation> {
        self.pages
            .values()
            .flat_map(|page| page.projection.rows.iter())
    }
}

#[derive(Clone, Debug)]
struct CachedPage {
    source: Weak<BTreeMap<EnvelopeKey, Envelope>>,
    projection: Arc<ReservationPage>,
}

impl PartialEq for CachedPage {
    fn eq(&self, other: &Self) -> bool {
        self.projection == other.projection
    }
}
impl Eq for CachedPage {}

#[derive(Debug, Default)]
pub(super) struct ReservationCache {
    current: LiveReservations,
}

impl ReservationCache {
    pub(super) fn project(
        &mut self,
        source: &LedgerPagedMap<Envelope>,
    ) -> Result<LiveReservations, StepFatal> {
        self.project_with_observer(source, || {})
    }

    fn project_with_observer(
        &mut self,
        source: &LedgerPagedMap<Envelope>,
        observe: impl Fn() + Sync,
    ) -> Result<LiveReservations, StepFatal> {
        let work = source.pages().collect::<Vec<_>>();
        let layout_changed = !self
            .current
            .pages
            .keys()
            .copied()
            .eq(work.iter().map(|(id, _)| *id));
        let missing =
            work.iter()
                .copied()
                .filter(|(id, page)| {
                    !self.current.pages.get(id).is_some_and(|cached| {
                        std::ptr::eq(Arc::as_ptr(page), cached.source.as_ptr())
                    })
                })
                .collect::<Vec<_>>();
        if missing.is_empty() && !layout_changed {
            return Ok(self.current.clone());
        }
        let project_page =
            |(id, page): (u64, &Arc<BTreeMap<EnvelopeKey, Envelope>>)| -> Result<_, StepFatal> {
                let mut rows: Vec<AccountReservation> = Vec::new();
                for (key, envelope) in page.iter() {
                    observe();
                    if rows.last().is_none_or(|row| row.account != key.account) {
                        rows.push(AccountReservation {
                            account: key.account,
                            cash: Money::ZERO,
                            sell: BTreeMap::new(),
                        });
                    }
                    let row = rows
                        .last_mut()
                        .expect("当前 envelope 已安装 account 预留行");
                    let live = envelope.live();
                    row.cash = row.cash.add(live.cash).map_err(allocation_money)?;
                    // 保留零股份条目：未知 StockId 即使 Buy shares=0 也须显式拒绝。
                    let shares = row.sell.entry(key.stock.clone()).or_insert(0);
                    *shares = shares
                        .checked_add(live.shares)
                        .ok_or_else(|| allocation_invariant("live sell reservation overflow"))?;
                }
                Ok((
                    id,
                    CachedPage {
                        source: Arc::downgrade(page),
                        projection: Arc::new(ReservationPage { rows }),
                    },
                ))
            };
        // 热路径仅有少量失效页；短计算在 caller 完成，冷/密集输入仍分发到所有 worker。
        let row_count = missing.iter().map(|(_, page)| page.len()).sum::<usize>();
        let prepared = if row_count >= 512 {
            missing
                .into_par_iter()
                .map(project_page)
                .collect::<Vec<_>>()
        } else {
            missing.into_iter().map(project_page).collect::<Vec<_>>()
        };
        // 所有可失败 checked 汇总先完成；page/order 顺序决定原有首错。
        let prepared = prepared.into_iter().collect::<Result<Vec<_>, _>>()?;
        let pages = Arc::make_mut(&mut self.current.pages);
        if layout_changed {
            pages.retain(|id, _| work.binary_search_by_key(id, |(id, _)| *id).is_ok());
        }
        pages.extend(prepared);
        Ok(self.current.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::super::ledger_tests;
    use super::super::{EnvelopeLedger, ResVec};
    use super::*;
    use crate::{OrderId, Side};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn unchanged_pages_reuse_reservations_and_a_single_mutation_reprojects_only_its_page() {
        let mut source = LedgerPagedMap::default();
        for account in 0..64 {
            let key = EnvelopeKey {
                account: AccountId(account),
                stock: StockId(0),
                order: OrderId(account),
                side: Side::Buy,
            };
            source.insert(
                key.clone(),
                ledger_tests::existing(key, Money::from_cents(100)),
            );
        }
        let mut cache = ReservationCache::default();
        let count = AtomicUsize::new(0);
        let first = cache
            .project_with_observer(&source, || {
                count.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 64);
        let second = cache
            .project_with_observer(&source, || {
                count.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        assert_eq!(
            count.load(Ordering::Relaxed),
            64,
            "相同不可变页不应重复汇总"
        );
        assert!(Arc::ptr_eq(first.page(0).unwrap(), second.page(0).unwrap()));
        let key = source.keys().next().unwrap().clone();
        source.insert(
            key.clone(),
            ledger_tests::existing(key, Money::from_cents(200)),
        );
        let changed = cache
            .project_with_observer(&source, || {
                count.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        assert_eq!(
            count.load(Ordering::Relaxed),
            96,
            "单页 mutation 只重算32行"
        );
        assert_eq!(changed.cash(AccountId(0)), Money::from_cents(200));
        assert_eq!(
            first.cash(AccountId(0)),
            Money::from_cents(100),
            "旧密封快照保持旧预留"
        );
        assert!(!Arc::ptr_eq(
            first.page(0).unwrap(),
            changed.page(0).unwrap()
        ));
        assert!(Arc::ptr_eq(
            first.page(1).unwrap(),
            changed.page(1).unwrap()
        ));
    }

    #[test]
    fn project_matches_canonical_sequential_fold_and_keeps_zero_share_stock_keys() {
        let mut source = LedgerPagedMap::default();
        let entries = [
            (0, StockId(0), 1, Side::Buy, 31, 0),
            (0, StockId(0), 6, Side::Buy, 5, 0),
            (0, StockId(1), 2, Side::Sell, 0, 4),
            (0, StockId(1), 3, Side::Sell, 0, 7),
            (u64::MAX, StockId(0), 4, Side::Sell, 0, 19),
            (u64::MAX, StockId(0), 5, Side::Buy, 23, 0),
        ];
        for (account, stock, order, side, cash, shares) in entries {
            let key = EnvelopeKey {
                account: AccountId(account),
                stock,
                order: OrderId(order),
                side,
            };
            source.insert(
                key.clone(),
                Envelope::tick_start_existing(
                    key,
                    Money::from_cents(cash),
                    shares,
                    ledger_tests::audit(),
                ),
            );
        }

        // 用旧式按 key 顺序逐行 fold 作为参考，固定账户内聚合和溢出语义。
        let mut expected = BTreeMap::<AccountId, AccountReservation>::new();
        for (key, envelope) in source.iter() {
            let row = expected
                .entry(key.account)
                .or_insert_with(|| AccountReservation {
                    account: key.account,
                    cash: Money::ZERO,
                    sell: BTreeMap::new(),
                });
            row.cash = row.cash.add(envelope.live().cash).unwrap();
            let shares = row.sell.entry(key.stock.clone()).or_insert(0);
            *shares = shares.checked_add(envelope.live().shares).unwrap();
        }

        let projected = ReservationCache::default().project(&source).unwrap();
        assert_eq!(
            projected.iter().cloned().collect::<Vec<_>>(),
            expected.values().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            projected.iter().map(|row| row.account).collect::<Vec<_>>(),
            vec![AccountId(0), AccountId(u64::MAX)],
            "完整 account 范围仍按 canonical 顺序投影"
        );
        assert_eq!(
            projected.get(AccountId(0)).unwrap().sell.get(&StockId(0)),
            Some(&0),
            "Buy shares=0 仍须保留 StockId 条目"
        );
        assert_eq!(
            projected.get(AccountId(0)).unwrap().sell.get(&StockId(1)),
            Some(&11),
            "同一 StockId 的多笔委托股份应 checked 汇总"
        );
        assert_eq!(projected.cash(AccountId(0)), Money::from_cents(36));
        assert_eq!(projected.cash(AccountId(u64::MAX)), Money::from_cents(23));
    }

    #[test]
    fn project_overflow_matches_allocation_errors_and_reports_first_page_error() {
        let mut cash_source = LedgerPagedMap::default();
        for (order, cash) in [(1, i64::MAX), (2, 1)] {
            let key = EnvelopeKey {
                account: AccountId(3),
                stock: StockId(0),
                order: OrderId(order),
                side: Side::Buy,
            };
            cash_source.insert(
                key.clone(),
                ledger_tests::existing(key, Money::from_cents(cash)),
            );
        }
        let expected_cash_error = allocation_money(
            Money::from_cents(i64::MAX)
                .add(Money::from_cents(1))
                .unwrap_err(),
        );
        assert_eq!(
            ReservationCache::default().project(&cash_source),
            Err(expected_cash_error),
            "cash overflow 必须沿用 allocation helper 的错误"
        );

        let mut shares_source = LedgerPagedMap::default();
        for (order, shares) in [(1, u32::MAX), (2, 1)] {
            let key = EnvelopeKey {
                account: AccountId(3),
                stock: StockId(0),
                order: OrderId(order),
                side: Side::Sell,
            };
            shares_source.insert(
                key.clone(),
                Envelope::tick_start_existing(key, Money::ZERO, shares, ledger_tests::audit()),
            );
        }
        assert_eq!(
            ReservationCache::default().project(&shares_source),
            Err(allocation_invariant("live sell reservation overflow")),
            "shares overflow 必须沿用 allocation helper 的错误"
        );

        let mut independent_pages = LedgerPagedMap::default();
        for (account, order, cash, shares, side) in [
            (3, 1, i64::MAX, 0, Side::Buy),
            (3, 2, 1, 0, Side::Buy),
            (64, 3, 0, u32::MAX, Side::Sell),
            (64, 4, 0, 1, Side::Sell),
        ] {
            let key = EnvelopeKey {
                account: AccountId(account),
                stock: StockId(0),
                order: OrderId(order),
                side,
            };
            independent_pages.insert(
                key.clone(),
                Envelope::tick_start_existing(
                    key,
                    Money::from_cents(cash),
                    shares,
                    ledger_tests::audit(),
                ),
            );
        }
        // 达到生产512行并行门槛；不能仅更换pool预算却仍验证短fixture的串行分支。
        for index in 0..512 {
            let key = EnvelopeKey {
                account: AccountId(128 + index),
                stock: StockId(0),
                order: OrderId(1000 + index),
                side: Side::Buy,
            };
            independent_pages.insert(
                key.clone(),
                ledger_tests::existing(key, Money::from_cents(1)),
            );
        }
        let expected_first_error = allocation_money(
            Money::from_cents(i64::MAX)
                .add(Money::from_cents(1))
                .unwrap_err(),
        );
        for threads in [1, 4] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            assert_eq!(
                pool.install(|| ReservationCache::default().project(&independent_pages)),
                Err(expected_first_error.clone()),
                "Rayon {threads} threads 下都应返回更早 account page 的首错"
            );
        }

        let mut separate_accounts = LedgerPagedMap::default();
        for (account, shares) in [(3, u32::MAX), (4, 1)] {
            let key = EnvelopeKey {
                account: AccountId(account),
                stock: StockId(0),
                order: OrderId(account),
                side: Side::Sell,
            };
            separate_accounts.insert(
                key.clone(),
                Envelope::tick_start_existing(key, Money::ZERO, shares, ledger_tests::audit()),
            );
        }
        let separate_projection = ReservationCache::default()
            .project(&separate_accounts)
            .unwrap();
        assert_eq!(
            separate_projection
                .get(AccountId(3))
                .unwrap()
                .sell
                .values()
                .copied()
                .sum::<u32>(),
            u32::MAX
        );
        assert_eq!(
            separate_projection
                .get(AccountId(4))
                .unwrap()
                .sell
                .values()
                .copied()
                .sum::<u32>(),
            1
        );
    }

    #[test]
    fn ledger_release_removes_page_without_ghosts_and_keeps_sealed_snapshots() {
        let key = ledger_tests::key();
        let first_cash = Money::from_cents(100);
        let mut ledger =
            EnvelopeLedger::new(0, [ledger_tests::existing(key.clone(), first_cash)]).unwrap();
        let first_projection = ledger.live_reservations().unwrap();
        let sealed_ledger = ledger.clone();
        assert_eq!(first_projection.cash(key.account), first_cash);

        let mut release = [ledger_tests::expiry_release(
            key.clone(),
            ResVec::new(first_cash, 0),
        )];
        ledger.apply(&mut release).unwrap();
        ledger.remove_terminal(std::slice::from_ref(&key)).unwrap();
        let empty_projection = ledger.live_reservations().unwrap();
        assert_eq!(
            empty_projection.page(key.account.0 >> RESERVATION_PAGE_SHIFT),
            None,
            "删除最后一行后缓存目录不应保留空 page"
        );
        assert_eq!(
            empty_projection.get(key.account),
            None,
            "删除最后一行后 page 不应残留 ghost"
        );
        assert_eq!(
            first_projection.cash(key.account),
            first_cash,
            "旧投影保持密封时金额"
        );
        assert_eq!(
            sealed_ledger.live_reservations().unwrap().cash(key.account),
            first_cash,
            "clone ledger 快照仍能投影原来的预留"
        );

        let new_key = EnvelopeKey {
            account: key.account,
            stock: StockId(1),
            order: OrderId(2),
            side: Side::Buy,
        };
        let new_cash = Money::from_cents(250);
        let new_envelope = ledger_tests::existing(new_key.clone(), new_cash);
        ledger.envelopes.insert(new_key.clone(), new_envelope);
        ledger.audits.insert(new_key.clone(), ledger_tests::audit());
        ledger
            .conservation
            .insert(new_key, super::super::ledger::ConservationState::EMPTY);
        let new_projection = ledger.live_reservations().unwrap();
        let new_sealed_ledger = ledger.clone();
        assert_eq!(new_projection.cash(key.account), new_cash);
        assert_ne!(new_projection, empty_projection, "新 envelope 应形成新投影");
        assert_eq!(
            first_projection.cash(key.account),
            first_cash,
            "旧投影不受新 envelope 影响"
        );
        assert_eq!(
            sealed_ledger.live_reservations().unwrap().cash(key.account),
            first_cash,
            "旧 ledger 快照金额保持不变"
        );
        assert_eq!(
            new_sealed_ledger
                .live_reservations()
                .unwrap()
                .cash(key.account),
            new_cash,
            "新 ledger 快照保持新预留金额"
        );
    }
    #[test]
    fn sealing_rejects_zero_share_unknown_stock_and_keeps_unknown_account_error_priority() {
        let mut session =
            crate::GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42)
                .unwrap();
        let unknown_stock = EnvelopeKey {
            account: AccountId(0),
            // 目录仅含 quote_setup 的单只股票（StockId(0)）；9 作为未知 StockId 哨兵。
            stock: StockId(9),
            order: OrderId(1),
            side: Side::Buy,
        };
        session.state.envelope_ledger = EnvelopeLedger::new(
            0,
            [ledger_tests::existing(
                unknown_stock.clone(),
                Money::from_cents(1),
            )],
        )
        .unwrap();
        let expected_stock_error =
            allocation_invariant("envelope ledger references an unknown account or stock");
        for _ in 0..2 {
            assert_eq!(
                super::super::DecisionResourceSnapshot::seal(&session),
                Err(expected_stock_error.clone()),
                "相同不可变页的cache命中也不能吞掉未知StockId，即使shares=0"
            );
        }
        let unknown_account = EnvelopeKey {
            account: AccountId(999),
            stock: session
                .stock_id(&session.state.setup.stocks[0].code.0)
                .unwrap(),
            order: OrderId(2),
            side: Side::Buy,
        };
        session.state.envelope_ledger = EnvelopeLedger::new(
            0,
            [
                ledger_tests::existing(unknown_stock, Money::from_cents(1)),
                ledger_tests::existing(unknown_account, Money::from_cents(1)),
            ],
        )
        .unwrap();
        assert_eq!(
            super::super::DecisionResourceSnapshot::seal(&session),
            Err(allocation_invariant(
                "envelope ledger references an unknown account"
            ))
        );
    }
    #[test]
    fn warm_projection_rechecks_unique_mutation_and_does_not_install_failed_rows() {
        let key = ledger_tests::key();
        let mut source = LedgerPagedMap::default();
        source.insert(
            key.clone(),
            ledger_tests::existing(key.clone(), Money::from_cents(100)),
        );
        let mut cache = ReservationCache::default();
        let old = cache.project(&source).unwrap();
        *source.get_mut(&key).unwrap() =
            ledger_tests::existing(key.clone(), Money::from_cents(200));
        let changed = cache.project(&source).unwrap();
        assert_eq!(old.cash(key.account), Money::from_cents(100));
        assert_eq!(changed.cash(key.account), Money::from_cents(200));
        assert!(!Arc::ptr_eq(old.page(0).unwrap(), changed.page(0).unwrap()));
        *source.get_mut(&key).unwrap() =
            ledger_tests::existing(key.clone(), Money::from_cents(i64::MAX));
        let mut later = key.clone();
        later.order = OrderId(2);
        source.insert(
            later.clone(),
            ledger_tests::existing(later.clone(), Money::from_cents(1)),
        );
        let before_failure = cache.current.clone();
        assert!(cache.project(&source).is_err());
        assert_eq!(
            cache.current, before_failure,
            "失败的checked汇总不能安装任何新projection行"
        );
        assert!(Arc::ptr_eq(&cache.current.pages, &before_failure.pages));
        source.remove(&later);
        *source.get_mut(&key).unwrap() =
            ledger_tests::existing(key.clone(), Money::from_cents(300));
        let repaired = cache.project(&source).unwrap();
        assert_eq!(repaired.cash(key.account), Money::from_cents(300));
        assert_eq!(changed.cash(key.account), Money::from_cents(200));
    }

    #[test]
    fn warm_reservations_follow_created_row_rebase_and_reset_without_affecting_hash_or_serde() {
        let key = ledger_tests::key();
        let mut ledger = EnvelopeLedger::new(
            0,
            [ledger_tests::existing(key.clone(), Money::from_cents(100))],
        )
        .unwrap();
        let before = ledger.clone();
        let before_projection = serde_json::to_vec(&ledger.hash_projection()).unwrap();
        let sealed = ledger.live_reservations().unwrap();
        assert_eq!(ledger, before, "派生cache预热不改变业务Eq");
        assert_eq!(
            serde_json::to_vec(&ledger.hash_projection()).unwrap(),
            before_projection
        );
        let created_key = EnvelopeKey {
            order: OrderId(2),
            ..key.clone()
        };
        ledger
            .insert_created([Envelope::created_at_validation(
                created_key.clone(),
                Money::from_cents(50),
                0,
                ledger_tests::audit_with_remaining(100),
            )])
            .unwrap();
        let created = ledger.live_reservations().unwrap();
        assert_eq!(created.cash(key.account), Money::from_cents(150));
        ledger.rebase_private_for_tick_commit().unwrap();
        let rebased = ledger.live_reservations().unwrap();
        assert_eq!(rebased, created, "origin/basis重置不改变实际live预留");
        assert!(
            !Arc::ptr_eq(rebased.page(0).unwrap(), created.page(0).unwrap()),
            "rebase修改源页必须让Weak证明失效并重新投影"
        );
        assert_eq!(
            ledger.get(&created_key).unwrap().origin(),
            super::super::EnvelopeOrigin::TickStart
        );
        assert_eq!(sealed.cash(key.account), Money::from_cents(100));
        ledger.reset_tick_state();
        let reset = ledger.live_reservations().unwrap();
        assert_eq!(reset.iter().count(), 0);
        assert_eq!(reset.cash(key.account), Money::ZERO);
        let serialized = serde_json::to_vec(&ledger).unwrap();
        let empty_before_warm = EnvelopeLedger::new(ledger.next_receipt_index(), []).unwrap();
        assert_eq!(serialized, serde_json::to_vec(&empty_before_warm).unwrap());
        let fields = serde_json::to_value(&ledger).unwrap();
        assert!(fields.get("reservation_cache").is_none());
        assert_eq!(rebased.cash(key.account), Money::from_cents(150));
    }
    #[test]
    fn prewarming_nonempty_reservations_keeps_session_hashes_and_save_bytes_identical() {
        let mut session =
            crate::GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42)
                .unwrap();
        let code = session
            .stock_id(&session.state.setup.stocks[0].code.0)
            .unwrap();
        session.seed_order_for_test(
            AccountId(0),
            crate::Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
            &mut Vec::new(),
        );
        session.hydrate_or_validate_envelope_ledger().unwrap();
        let before_ledger = session.state.envelope_ledger.clone();
        let before_business = session.business_state_hash().unwrap();
        let before_session = session.session_state_hash().unwrap();
        let before_save = serde_json::to_vec(&session.save().unwrap()).unwrap();
        let projection = session.state.envelope_ledger.live_reservations().unwrap();
        assert!(projection.cash(AccountId(0)) > Money::ZERO);
        assert_eq!(session.state.envelope_ledger, before_ledger);
        assert_eq!(session.business_state_hash().unwrap(), before_business);
        assert_eq!(session.session_state_hash().unwrap(), before_session);
        assert_eq!(
            serde_json::to_vec(&session.save().unwrap()).unwrap(),
            before_save
        );
    }
}
