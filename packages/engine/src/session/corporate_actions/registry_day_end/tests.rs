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
