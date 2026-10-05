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

#[test]
fn later_same_day_payment_does_not_rewrite_earlier_collection() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "partial1".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "actual-receipt1".into(),
    )
    .unwrap();
    let first = book
        .collect_due("tax1".into(), date("2030-01-02"), Money::from_cents(25))
        .unwrap();
    assert_eq!(first.collected, Money::from_cents(25));
    assert_eq!(
        first.outstanding,
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    book.record_payment(
        "cash",
        "partial2".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "actual-receipt2".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(25, 1).unwrap()
    );
    assert_eq!(
        book.collect_due("tax1".into(), date("2030-01-02"), Money::from_cents(25))
            .unwrap(),
        first
    );
    let second = book
        .collect_due("tax2".into(), date("2030-01-02"), Money::from_cents(25))
        .unwrap();
    assert_eq!(second.collected, Money::from_cents(25));
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}

#[test]
fn sale_on_statutory_release_day_has_short_term_tax_not_t1_rejection() {
    let mut restricted = lot("restricted", 10, "2029-01-01");
    restricted.class = TaxShareClass::StatutoryRestricted {
        release_on: date("2030-01-02"),
        basis: StatutoryRestrictedBasis::FinanceTax2009167,
        qualification_evidence: "explicit-qualifying-shares".into(),
    };
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![restricted],
    )
    .unwrap();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell-on-release".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "actual-paid-after-release".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "actual-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(100, 1).unwrap()
    );
    assert_eq!(
        book.collect_due("tax".into(), date("2030-01-02"), Money::from_cents(100))
            .unwrap()
            .collected,
        Money::from_cents(100)
    );
}

#[test]
fn restore_cannot_replace_registered_tax_disposition_identity_to_erase_tax() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "real-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(50, 1).unwrap()
    );
    let mut corrupt = serde_json::to_value(&book).unwrap();
    corrupt["days"][0]["dispositions"][0]["lot"]["id"] = serde_json::json!("unknown-lot");
    assert!(serde_json::from_value::<CashDividendTaxBook>(corrupt).is_err());
}

#[test]
fn tax_restore_rejects_conflicting_metadata_sequence_and_cash_and_failures_are_atomic() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "real-receipt".into(),
    )
    .unwrap();
    book.collect_due("tax".into(), date("2030-01-02"), Money::from_cents(20))
        .unwrap();
    let valid = serde_json::to_value(&book).unwrap();
    assert!(valid.to_string().contains("\"18446744073709551615\""));
    for kind in [
        "metadata", "quantity", "snapshot", "sequence", "cash", "nullable", "version",
    ] {
        let mut corrupt = valid.clone();
        match kind {
            "metadata" => {
                corrupt["days"][0]["dispositions"][0]["lot"]["acquired_on"] =
                    serde_json::json!("2029-11-01")
            }
            "quantity" => corrupt["lots"][0]["qty"] = serde_json::json!(6),
            "snapshot" => corrupt["dividends"][0]["lots"][0]["qty"] = serde_json::json!(11),
            "sequence" => {
                corrupt["dividends"][0]["payments"][0]["operation_seq"] = serde_json::json!(2)
            }
            "cash" => corrupt["collections"][0]["collected"] = serde_json::json!("21"),
            "nullable" => {
                corrupt["days"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("acquisition");
            }
            "version" => {
                corrupt
                    .as_object_mut()
                    .unwrap()
                    .insert("version".into(), serde_json::json!(3));
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<CashDividendTaxBook>(corrupt).is_err(),
            "accepted {kind}"
        );
    }
    let before = book.clone();
    assert!(book
        .record_payment(
            "cash",
            "overpaid".into(),
            date("2030-01-02"),
            Money::from_cents(1),
            "real-extra".into()
        )
        .is_err());
    assert_eq!(book, before);
    assert!(book
        .record_net_day("oversell".into(), date("2030-01-03"), -16, None)
        .is_err());
    assert_eq!(book, before);
}

#[test]
fn mixed_restricted_tax_disposal_and_unverified_month_end_boundary_are_explicit() {
    let mut restricted = lot("restricted", 5, "2029-01-01");
    restricted.class = TaxShareClass::StatutoryRestricted {
        release_on: date("2031-01-01"),
        basis: StatutoryRestrictedBasis::FinanceTax201070,
        qualification_evidence: "explicit-qualifying".into(),
    };
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![restricted, lot("public", 5, "2029-12-01")],
    )
    .unwrap();
    let before = book.clone();
    assert_eq!(
        book.record_net_day("mixed-sell".into(), date("2030-01-02"), -1, None),
        Err(DividendTaxError::UnsupportedMixedRestrictedTaxLots)
    );
    assert_eq!(book, before);
    assert_eq!(
        personal_cash_dividend_rate(date("2030-01-31"), date("2030-03-01")),
        Err(DividendTaxError::NeedHoldingPeriodBoundaryEvidence)
    );
}

#[test]
fn payment_retry_does_not_allocate_sequence_at_counter_limit() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-01"),
        Money::from_cents(2000),
        "real-receipt".into(),
    )
    .unwrap();
    book.operation_seq = u64::MAX;
    let before = book.clone();
    assert!(book
        .record_payment(
            "cash",
            "paid".into(),
            date("2030-01-01"),
            Money::from_cents(2000),
            "real-receipt".into()
        )
        .is_ok());
    assert_eq!(book, before);
    assert!(matches!(
        book.record_payment(
            "cash",
            "new-payment".into(),
            date("2030-01-01"),
            Money::from_cents(1),
            "new-receipt".into()
        ),
        Err(DividendTaxError::Overflow {
            operation: "operation sequence"
        })
    ));
    assert_eq!(book, before);
}

#[test]
fn dividend_received_before_statutory_unlock_is_ten_percent_and_not_retaxed() {
    let mut restricted = lot("restricted", 10, "2028-01-01");
    restricted.class = TaxShareClass::StatutoryRestricted {
        release_on: date("2030-01-02"),
        basis: StatutoryRestrictedBasis::FinanceTax2009167,
        qualification_evidence: "explicit-qualifying".into(),
    };
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![restricted],
    )
    .unwrap();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "cash",
        "before-unlock".into(),
        date("2030-01-01"),
        Money::from_cents(1000),
        "real-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(100, 1).unwrap()
    );
    assert_eq!(
        book.collect_due(
            "preunlock-tax".into(),
            date("2030-01-01"),
            Money::from_cents(100)
        )
        .unwrap()
        .collected,
        Money::from_cents(100)
    );
    book.record_net_day("sell-on-unlock".into(), date("2030-01-02"), -5, None)
        .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}
