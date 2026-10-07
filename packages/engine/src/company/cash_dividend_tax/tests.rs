use super::*;

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}
fn lot(id: &str, qty: u64, acquired_on: &str) -> DividendTaxLot {
    DividendTaxLot {
        id: id.into(),
        qty,
        acquired_on: date(acquired_on),
        source: TaxAcquisitionSource::InitialAllocation {
            evidence: id.into(),
        },
        class: TaxShareClass::PublicMarket,
    }
}
fn book() -> CashDividendTaxBook {
    CashDividendTaxBook::new(
        AccountId(u64::MAX),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("old", 10, "2029-12-01"), lot("new", 10, "2029-12-20")],
    )
    .unwrap()
}

#[test]
fn natural_month_and_year_boundaries_use_disposal_day_exclusive_holding() {
    for (sale, rate) in [
        ("2030-02-01", 20),
        ("2030-02-02", 10),
        ("2031-01-01", 10),
        ("2031-01-02", 0),
    ] {
        assert_eq!(
            personal_cash_dividend_rate(date("2030-01-01"), date(sale)).unwrap(),
            rate
        );
    }
}

#[test]
fn fifo_partial_sale_assesses_only_paid_dividend_and_collection_is_atomic() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    assert_eq!(book.lots()[0].qty, 5);
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "actual-cash-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(50, 1).unwrap()
    );
    assert_eq!(
        book.collect_due("collect1".into(), date("2030-01-02"), Money::from_cents(20))
            .unwrap()
            .collected,
        Money::from_cents(20)
    );
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(30, 1).unwrap()
    );
    let before = book.clone();
    assert_eq!(
        book.collect_due("collect1".into(), date("2030-01-02"), Money::from_cents(20))
            .unwrap()
            .collected,
        Money::from_cents(20)
    );
    assert_eq!(book, before);
    assert!(book
        .collect_due("collect1".into(), date("2030-01-02"), Money::from_cents(21))
        .is_err());
    assert_eq!(book, before);
}

#[test]
fn nonindividual_profile_is_not_silently_taxed_as_individual() {
    assert!(matches!(
        CashDividendTaxBook::new(
            AccountId(1),
            StockCode("600001".into()),
            DividendTaxProfile::ResidentEnterprise,
            date("2030-01-01"),
            vec![]
        ),
        Err(DividendTaxError::UnsupportedProfile { .. })
    ));
}

#[test]
fn exact_subcent_tax_is_not_rounded_or_erased() {
    let mut book = CashDividendTaxBook::new(
        AccountId(u64::MAX),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("fractional", 3, "2029-12-01")],
    )
    .unwrap();
    book.register_dividend(
        "fraction".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(1, 3).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -1, None)
        .unwrap();
    book.record_payment(
        "fraction",
        "actual-one-cent".into(),
        date("2030-01-02"),
        Money::from_cents(1),
        "actual-cash-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(1, 30).unwrap()
    );
    let before = book.clone();
    assert_eq!(
        book.collect_due("collect".into(), date("2030-01-02"), Money::from_cents(100)),
        Err(DividendTaxError::NeedRoundingEvidence)
    );
    assert_eq!(book, before);
}
