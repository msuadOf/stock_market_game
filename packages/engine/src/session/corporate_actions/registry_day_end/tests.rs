use super::*;
use crate::company::share_registry::{AcquisitionSource, ShareHolding, ShareLot, ShareRestriction};
use crate::company::CompanyId;
use crate::fees::FeeComponents;
use crate::orderbook::Side;
use crate::stock::StockMeta;

fn date(day: u8) -> CivilDate {
    CivilDate::from_ymd(2026, 10, day).unwrap()
}
fn catalog() -> StockCatalog {
    StockCatalog::from_metas(vec![StockMeta::new("600000")]).unwrap()
}
fn foundation() -> SessionCorporateActions {
    SessionCorporateActions {
        registries: vec![ShareRegistry::new(
            StockCode("600000".into()),
            CompanyId(0),
            1000,
            date(22),
            vec![ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![ShareLot {
                    id: "opening".into(),
                    qty: 1000,
                    acquired_on: date(22),
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "开局分配".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            }],
        )
        .unwrap()],
        ..SessionCorporateActions::default()
    }
}
fn confirmation(receipt_id: u64, side: Side, quantity_shares: u32) -> PersonalTradeConfirmation {
    PersonalTradeConfirmation {
        receipt_id,
        civil_date: date(23),
        stock: StockId(0),
        side,
        price_cents: 100,
        quantity_shares,
        gross_cents: i64::from(quantity_shares) * 100,
        actual_fees: FeeComponents::ZERO,
    }
}
fn holdings(a: u64, b: u64) -> BTreeMap<AccountId, BTreeMap<StockId, u64>> {
    [
        (AccountId(1), [(StockId(0), a)].into()),
        (AccountId(7), [(StockId(0), b)].into()),
    ]
    .into()
}
fn confirmations() -> BTreeMap<AccountId, Vec<PersonalTradeConfirmation>> {
    // 同一尾组账户的普通与 closing 两笔真实确认必须都只计一次。
    [
        (
            AccountId(1),
            vec![
                confirmation(0, Side::Sell, 100),
                confirmation(2, Side::Sell, 200),
            ],
        ),
        (
            AccountId(7),
            vec![
                confirmation(1, Side::Buy, 100),
                confirmation(3, Side::Buy, 200),
            ],
        ),
    ]
    .into()
}
fn input<'a>(
    catalog: &'a StockCatalog,
    accounts: &'a BTreeMap<AccountId, BTreeMap<StockId, u64>>,
    confirmations: &'a BTreeMap<AccountId, Vec<PersonalTradeConfirmation>>,
    eligible: Option<&'a BTreeSet<AccountId>>,
) -> RegistryDayEndInput<'a> {
    RegistryDayEndInput {
        day: date(23),
        catalog,
        accounts,
        confirmations,
        issuer_repurchase_account: None,
        late_holder_tax_accounts: eligible,
    }
}

#[test]
fn ordinary_and_closing_confirmations_produce_candidate_without_mutating_facts() {
    let state = foundation();
    let before = state.clone();
    let accounts = holdings(700, 300);
    let rows = confirmations();
    let candidate = state
        .registry_day_end_candidate(input(&catalog(), &accounts, &rows, None))
        .unwrap();
    assert_eq!(state, before);
    assert_eq!(accounts, holdings(700, 300));
    let registry = &candidate.registries[0];
    assert_eq!(registry.settled_on(), date(23));
    assert_eq!(registry.receipts().len(), 1);
    assert_eq!(
        registry
            .holdings()
            .iter()
            .find(|h| h.holder == HolderId::Account(AccountId(7)))
            .unwrap()
            .lots[0]
            .qty,
        300
    );
    let installed = SessionCorporateActions {
        registries: candidate.registries,
        dividend_tax_books: candidate.dividend_tax_books,
        ..before
    };
    let repeated = installed
        .registry_day_end_candidate(input(&catalog(), &accounts, &rows, None))
        .unwrap();
    assert_eq!(repeated.registries, installed.registries);
    assert!(repeated.dividend_tax_books.is_empty());
}

#[test]
fn stale_pre_closing_accounts_fail_with_numeric_context_and_leave_original_unchanged() {
    let state = foundation();
    let before = state.clone();
    let err = state
        .registry_day_end_candidate(input(
            &catalog(),
            &holdings(900, 100),
            &confirmations(),
            None,
        ))
        .unwrap_err();
    assert!(
        matches!(err, SessionCorporateActionsError::RegistryPositionMismatch {
        date: d, stock: StockId(0), account: AccountId(1), expected: 700, actual: 900
    } if d == date(23))
    );
    assert_eq!(state, before);
}

#[test]
fn late_personal_holder_opens_tax_book_once_and_non_personal_stays_unconfigured() {
    let state = foundation();
    let eligible = [AccountId(7)].into();
    let candidate = state
        .registry_day_end_candidate(input(
            &catalog(),
            &holdings(700, 300),
            &confirmations(),
            Some(&eligible),
        ))
        .unwrap();
    assert_eq!(candidate.dividend_tax_books.len(), 1);
    let book = &candidate.dividend_tax_books[0];
    assert_eq!(book.account(), AccountId(7));
    assert_eq!(book.lots().iter().map(|l| l.qty).sum::<u64>(), 300);
    let installed = SessionCorporateActions {
        registries: candidate.registries,
        dividend_tax_books: candidate.dividend_tax_books,
        ..state
    };
    let repeated = installed
        .registry_day_end_candidate(input(
            &catalog(),
            &holdings(700, 300),
            &confirmations(),
            Some(&eligible),
        ))
        .unwrap();
    assert_eq!(repeated.dividend_tax_books, installed.dividend_tax_books);
    assert!(foundation()
        .registry_day_end_candidate(input(
            &catalog(),
            &holdings(700, 300),
            &confirmations(),
            None
        ))
        .unwrap()
        .dividend_tax_books
        .is_empty());
}

#[test]
fn repurchase_account_is_registered_as_treasury_and_reconciled_separately() {
    let state = foundation();
    let accounts = holdings(700, 300);
    let rows = confirmations();
    let cat = catalog();
    let mut request = input(&cat, &accounts, &rows, None);
    request.issuer_repurchase_account = Some(AccountId(7));
    let candidate = state.registry_day_end_candidate(request).unwrap();
    assert!(!candidate.registries[0]
        .holdings()
        .iter()
        .any(|h| h.holder == HolderId::Account(AccountId(7))));
    assert_eq!(
        candidate.registries[0]
            .holdings()
            .iter()
            .find(|h| h.holder == HolderId::IssuerTreasury)
            .unwrap()
            .lots[0]
            .qty,
        300
    );
    assert!(candidate.dividend_tax_books.is_empty());
}

#[test]
fn duplicate_confirmation_is_explicitly_rejected_and_unregistered_balance_is_not_hidden() {
    let mut rows = confirmations();
    rows.get_mut(&AccountId(7))
        .unwrap()
        .push(confirmation(3, Side::Buy, 200));
    assert!(
        matches!(foundation().registry_day_end_candidate(input(&catalog(), &holdings(700,300), &rows, None)), Err(SessionCorporateActionsError::Invalid(reason)) if reason.contains("重复"))
    );
    let mut accounts = holdings(700, 300);
    accounts.insert(AccountId(9), [(StockId(0), 100)].into());
    assert!(matches!(
        foundation().registry_day_end_candidate(input(
            &catalog(),
            &accounts,
            &confirmations(),
            None
        )),
        Err(SessionCorporateActionsError::RegistryPositionMismatch {
            account: AccountId(9),
            expected: 0,
            actual: 100,
            ..
        })
    ));
}

#[test]
fn treasury_final_position_mismatch_is_explicit_and_candidate_failure_is_atomic() {
    let state = foundation();
    let before = state.clone();
    let accounts = holdings(700, 200);
    let rows = confirmations();
    let cat = catalog();
    let mut request = input(&cat, &accounts, &rows, None);
    request.issuer_repurchase_account = Some(AccountId(7));
    assert!(matches!(
        state.registry_day_end_candidate(request),
        Err(SessionCorporateActionsError::RegistryPositionMismatch {
            account: AccountId(7),
            expected: 300,
            actual: 200,
            ..
        })
    ));
    assert_eq!(state, before);
}

/// #119 的小活动覆盖：三组含非满尾组，真实 Drain 成交的最终 facts 与确认可勾稽。
/// 这是当前机制验证，不声称复现旧 1200/1500 场景或证明其历史根因。
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn runtime_closing_final_facts_reconcile_with_tail_group_confirmations() {
    use crate::runtime::drive::RuntimeDriver;
    use crate::GameSession;
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(1);
    setup.npcs.retail_count = 4;
    setup.npcs.inst_count = 0;
    setup.stocks[0].float_shares = 800_000;
    setup.start_date = CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 6;
    setup.closing_auction_ticks = 2;
    let mut session = GameSession::new(setup, 21).unwrap();
    let day = session.civil_date();
    let opened_on = day.prev().unwrap();
    let cat = session.stock_catalog().clone();
    let mut registered = 0;
    let mut initial_holdings = Vec::new();
    for id in session.account_roster() {
        let qty = session
            .account(id)
            .unwrap()
            .positions()
            .get(&StockId(0))
            .map_or(0, |p| p.qty());
        if qty > 0 {
            registered += u64::from(qty);
            initial_holdings.push(ShareHolding {
                holder: HolderId::Account(id),
                lots: vec![ShareLot {
                    id: format!("initial:{}", id.0),
                    qty: u64::from(qty),
                    acquired_on: opened_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "开局账户快照".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            });
        }
    }
    initial_holdings.push(ShareHolding {
        holder: HolderId::External("非流通持有人".into()),
        lots: vec![ShareLot {
            id: "external".into(),
            qty: 10_000_000 - registered,
            acquired_on: opened_on,
            source: AcquisitionSource::InitialAllocation {
                evidence: "开局未入账户股份".into(),
            },
            restriction: ShareRestriction::Unrestricted,
        }],
    });
    let state = SessionCorporateActions {
        registries: vec![ShareRegistry::new(
            StockCode(cat.code(StockId(0)).unwrap().into()),
            CompanyId(0),
            10_000_000,
            opened_on,
            initial_holdings,
        )
        .unwrap()],
        ..SessionCorporateActions::default()
    };
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 3).unwrap();
    driver.adopt_session_seq(&session);
    let closing = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured = std::sync::Arc::clone(&closing);
    driver.set_day_end_validator_for_test(Box::new(move |candidate| {
        *captured.lock().unwrap() = Some(candidate.clone());
        Ok(())
    }));
    for tick in 1..=6 {
        if tick == 3 || tick == 5 {
            let seller = driver
                .world_accounts_snapshot()
                .unwrap()
                .into_iter()
                .find_map(|(g, accounts)| {
                    accounts.iter().enumerate().find_map(|(l, a)| {
                        let id = driver.account_by_owner((g, l as u32)).unwrap();
                        (id != AccountId(4) && a.shares[0] >= 100).then_some(id)
                    })
                })
                .unwrap();
            let intents: Vec<_> = [(AccountId(4), Side::Buy), (seller, Side::Sell)]
                .into_iter()
                .map(|(owner, side)| crate::session::ReceiptBearingIntent {
                    owner,
                    intent: crate::Intent::PlaceLimit {
                        code: StockId(0),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(Money::from_cents(1000)),
                        qty: 100,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                })
                .collect();
            let upper: Vec<_> = driver
                .world_public_views()
                .iter()
                .map(|v| v.daily_upper_limit)
                .collect();
            driver.inject_player_intents(&intents, &upper).unwrap();
        }
        driver.step_frame_equivalent(&mut session).unwrap();
        driver.install_market_views(&mut session).unwrap();
        driver.install_candles(&mut session).unwrap();
        // 手动 driver 与生产 failure::step_via_runtime 使用同一确认安装入口。
        let receipts = driver.take_pending_receipts();
        session.runtime_append_trade_receipts(&receipts).unwrap();
    }
    driver
        .day_end_with_runtime_market_day(&mut session)
        .unwrap();
    let closing = closing.lock().unwrap().clone().unwrap();
    assert_eq!(closing.groups.len(), 3);
    assert_eq!(driver.account_by_owner((2, 0)), Some(AccountId(4)));
    let mut accounts = BTreeMap::new();
    for (group, rows) in closing.groups {
        for (local, a) in rows.iter().enumerate() {
            // candidate 含尾组 padding 槽；它们没有 AccountId，不属于账户全集。
            let Some(id) = driver.account_by_owner((group, local as u32)) else {
                continue;
            };
            let qty = u64::from(a.shares[0]) + u64::from(a.pending_sell_qty[0]);
            accounts.insert(id, [(StockId(0), qty)].into());
        }
    }
    let confirmations: BTreeMap<_, Vec<_>> = session
        .account_roster()
        .into_iter()
        .filter_map(|id| {
            session
                .personal_trade_confirmations(id)
                .map(|rows| (id, rows))
        })
        .collect();
    assert_eq!(
        confirmations[&AccountId(4)]
            .iter()
            .filter(|r| r.side == Side::Buy)
            .map(|r| r.quantity_shares)
            .sum::<u32>(),
        200
    );
    assert_eq!(
        confirmations[&AccountId(4)]
            .iter()
            .filter(|r| r.side == Side::Buy)
            .count(),
        2
    );
    let candidate = state
        .registry_day_end_candidate(RegistryDayEndInput {
            day,
            catalog: &cat,
            accounts: &accounts,
            confirmations: &confirmations,
            issuer_repurchase_account: None,
            late_holder_tax_accounts: None,
        })
        .unwrap();
    assert_eq!(candidate.registries[0].settled_on(), day);
    assert_eq!(candidate.registries[0].receipts().len(), 1);
}

#[test]
fn same_day_round_trip_preserves_existing_registry_and_tax_acquisition_dates() {
    use crate::company::cash_dividend_tax::DividendTaxProfile;
    let mut state = foundation();
    state
        .configure_cash_dividend_tax_book(
            AccountId(1),
            StockCode("600000".into()),
            DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
    let initial_registry_lots = state.registries[0].holdings()[0].lots.clone();
    let initial_tax_lots = state.dividend_tax_books[0].lots().to_vec();
    let rows = [(
        AccountId(1),
        vec![
            confirmation(0, Side::Sell, 100),
            confirmation(1, Side::Buy, 100),
        ],
    )]
    .into();
    let candidate = state
        .registry_day_end_candidate(input(&catalog(), &holdings(1000, 0), &rows, None))
        .unwrap();
    assert_eq!(
        candidate.registries[0].holdings()[0].lots,
        initial_registry_lots
    );
    assert_eq!(candidate.dividend_tax_books[0].lots(), initial_tax_lots);
    assert_eq!(candidate.dividend_tax_books[0].settled_on(), date(23));
}

#[test]
fn unknown_final_stock_and_missing_treasury_identity_are_explicitly_rejected() {
    let state = foundation();
    let cat = catalog();
    let mut accounts = holdings(1000, 0);
    accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .insert(StockId(99), 100);
    let rows = BTreeMap::new();
    assert!(
        matches!(state.registry_day_end_candidate(input(&cat,&accounts,&rows,None)),Err(SessionCorporateActionsError::Invalid(reason)) if reason.contains("未知证券"))
    );
    let accounts = holdings(1000, 0);
    let mut request = input(&cat, &accounts, &rows, None);
    request.issuer_repurchase_account = Some(AccountId(99));
    assert!(
        matches!(state.registry_day_end_candidate(request),Err(SessionCorporateActionsError::Invalid(reason)) if reason.contains("回购专户") && reason.contains("不存在"))
    );
}
