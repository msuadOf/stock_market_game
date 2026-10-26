use super::tests::{cash_book, date, fraction, fragment, lot, origin, q};
use super::*;
fn empty_lineage(book: &CashDividendTaxBook) -> TaxUnitLineage {
    TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![],
    }
}
fn registration(event: &str, prefix: u64, day: &str, ratio: u64) -> ConditionalBonusTaxEvent {
    ConditionalBonusTaxEvent {
        after_book_operation: prefix,
        day: date(day),
        action: ConditionalBonusTaxAction::RegisterBonus {
            event_id: event.into(),
            shares_per_existing_share_micros: ratio,
            par_value_per_share: Money::from_cents(100),
        },
    }
}
fn realization(
    book: &CashDividendTaxBook,
    event: &str,
    prefix: u64,
    day: &str,
    qty: u64,
    acquisition: Option<u64>,
) -> ConditionalBonusTaxEvent {
    ConditionalBonusTaxEvent {
        after_book_operation: prefix,
        day: date(day),
        action: ConditionalBonusTaxAction::RealizeBonus {
            event_id: event.into(),
            actual_credit: ConditionalBonusCredit {
                event_id: event.into(),
                account: book.account(),
                stock: book.stock().clone(),
                credited_qty: qty,
            },
            tax_acquisition: acquisition,
        },
    }
}
fn bonus_book(acquired: &str) -> CashDividendTaxBook {
    CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("original", 100, acquired)],
    )
    .unwrap()
}
fn credit(book: &mut CashDividendTaxBook, qty: u64) {
    book.record_net_day(
        "credit".into(),
        date("2030-01-02"),
        i128::from(qty),
        Some(DividendTaxLot {
            id: "new".into(),
            qty,
            acquired_on: date("2030-01-02"),
            source: TaxAcquisitionSource::CorporateAction {
                event: "bonus".into(),
            },
            class: TaxShareClass::PublicMarket,
        }),
    )
    .unwrap();
}
#[test]
fn long_original_bonus_stays_exempt_when_new_short_lot_is_sold() {
    let mut book = bonus_book("2028-12-01");
    credit(&mut book, 100);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, Some(1)),
        ],
    };
    book.record_net_day("sell-original".into(), date("2030-01-03"), -100, None)
        .unwrap();
    book.record_net_day("sell-new".into(), date("2030-01-04"), -100, None)
        .unwrap();
    let result = book
        .assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
        .unwrap();
    assert_eq!(result.outstanding.numerator(), 0);
    assert_eq!(result.consumptions.len(), 1);
    assert_eq!(
        result.consumptions[0].key,
        TaxEntitlementKey {
            kind: TaxIncomeKind::StockBonus,
            event_id: "bonus".into()
        }
    );
    assert_eq!(
        result.consumptions[0].original,
        origin("original", q(0), q(100))
    );
}
#[test]
fn short_original_partial_disposals_assess_bonus_once_without_waiting_for_new_lot() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 100);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, Some(1)),
        ],
    };
    book.record_net_day("sell20".into(), date("2030-01-03"), -20, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        400
    );
    book.record_net_day("sell80".into(), date("2030-01-04"), -80, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        2000
    );
    book.record_net_day("sell-new".into(), date("2030-01-05"), -100, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        2000
    );
}
#[test]
fn no_tax_acquisition_is_needed_to_realize_net_negative_stock_bonus_income() {
    let mut book = bonus_book("2029-12-25");
    // 给定同一真实税日：market-100、actual credit30、tax net-70。
    book.record_net_day("tax-net-minus70".into(), date("2030-01-02"), -70, None)
        .unwrap();
    let registered = ConditionalBonusTaxFacts {
        events: vec![registration("bonus", 0, "2030-01-01", 300_000)],
    };
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &registered)
            .unwrap()
            .outstanding
            .numerator(),
        0
    );
    let mut realized = registered;
    realized
        .events
        .push(realization(&book, "bonus", 1, "2030-01-02", 30, None));
    let before = book.clone();
    let result = book
        .assessment_with_conditional_bonus(&empty_lineage(&book), &realized)
        .unwrap();
    assert_eq!(result.outstanding.numerator(), 420);
    assert_eq!(book, before);
    book.record_net_day("sell-last30".into(), date("2030-01-03"), -30, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &realized)
            .unwrap()
            .outstanding
            .numerator(),
        600
    );
}
#[test]
fn net_positive_thirty_tax_shares_do_not_shrink_actual_hundred_share_income() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 30);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, Some(1)),
        ],
    };
    book.record_net_day("sell-original".into(), date("2030-01-03"), -100, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        2000
    );
}

#[test]
fn net_zero_realizes_actual_credit_without_creating_tax_lot() {
    let mut book = bonus_book("2029-12-25");
    book.record_net_day("given-net-zero".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, None),
        ],
    };
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        0
    );
    assert_eq!(book.lots().len(), 1);
    book.record_net_day("sell100".into(), date("2030-01-03"), -100, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        2000
    );
}
#[test]
fn acquisition_p_and_later_realization_q_preserve_prior_original_disposals() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 100);
    book.record_net_day("sell40".into(), date("2030-01-03"), -40, None)
        .unwrap();
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 2, "2030-01-03", 100, Some(1)),
        ],
    };
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        800
    );
    book.record_net_day("sell60".into(), date("2030-01-04"), -60, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        2000
    );
}
#[test]
fn cash_and_bonus_same_event_text_keep_distinct_income_keys() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    book.record_net_day(
        "actual-credit".into(),
        date("2030-01-02"),
        100,
        Some(DividendTaxLot {
            id: "new".into(),
            qty: 100,
            acquired_on: date("2030-01-02"),
            source: TaxAcquisitionSource::CorporateAction {
                event: "cash".into(),
            },
            class: TaxShareClass::PublicMarket,
        }),
    )
    .unwrap();
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("cash", 2, "2030-01-01", 1_000_000),
            realization(&book, "cash", 3, "2030-01-02", 100, Some(3)),
        ],
    };
    book.record_net_day("sell20".into(), date("2030-01-03"), -20, None)
        .unwrap();
    let first = book
        .assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
        .unwrap();
    assert_eq!(first.outstanding.numerator(), 800);
    assert_eq!(first.consumptions.len(), 2);
    assert_eq!(first.consumptions[0].key.kind, TaxIncomeKind::CashDividend);
    assert_eq!(first.consumptions[1].key.kind, TaxIncomeKind::StockBonus);
    assert!(first.consumptions.iter().all(|c| c.key.event_id == "cash"
        && c.disposition_event_id == "sell20"
        && c.original == origin("original", q(0), q(20))));
    book.record_net_day("sell80".into(), date("2030-01-04"), -80, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        4000
    );
}
#[test]
fn future_cash_and_bonus_r_use_new_tax_acquisition_without_recharging_first_bonus() {
    let mut book = bonus_book("2028-12-01");
    credit(&mut book, 100);
    book.record_net_day("sell-original".into(), date("2030-01-03"), -100, None)
        .unwrap();
    book.register_dividend(
        "future".into(),
        date("2030-01-03"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "future",
        "future-paid".into(),
        date("2030-01-03"),
        Money::from_cents(10000),
        "后次现金实付".into(),
    )
    .unwrap();
    // 给定另一个完整税日净0：后次红股100抵消同日市场净减100，不增加tax lot。
    book.record_net_day("future-net-zero".into(), date("2030-01-04"), 0, None)
        .unwrap();
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, Some(1)),
            registration("future", 4, "2030-01-03", 1_000_000),
            realization(&book, "future", 5, "2030-01-04", 100, None),
        ],
    };
    book.record_net_day("later-tax-disposal".into(), date("2030-01-05"), -100, None)
        .unwrap();
    let result = book
        .assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
        .unwrap();
    assert_eq!(result.outstanding.numerator(), 4000);
    assert_eq!(
        result
            .consumptions
            .iter()
            .filter(|c| c.key.event_id == "bonus")
            .count(),
        1
    );
    assert_eq!(
        result
            .consumptions
            .iter()
            .filter(|c| c.key.event_id == "future")
            .count(),
        2
    );
}
#[test]
fn typed_action_duplicates_are_idempotent_and_conflicting_frontier_or_terms_are_rejected() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 100);
    book.record_net_day("sell".into(), date("2030-01-03"), -100, None)
        .unwrap();
    let reg = registration("bonus", 0, "2030-01-01", 1_000_000);
    let actual = realization(&book, "bonus", 1, "2030-01-02", 100, Some(1));
    let once = ConditionalBonusTaxFacts {
        events: vec![reg.clone(), actual.clone()],
    };
    let twice = ConditionalBonusTaxFacts {
        events: vec![reg.clone(), reg.clone(), actual.clone(), actual.clone()],
    };
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &once)
            .unwrap(),
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &twice)
            .unwrap()
    );
    let mut conflict = twice;
    if let ConditionalBonusTaxAction::RegisterBonus {
        par_value_per_share,
        ..
    } = &mut conflict.events[1].action
    {
        *par_value_per_share = Money::from_cents(101);
    }
    assert!(matches!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &conflict),
        Err(TaxLineageError::InvalidConditionalFact { .. })
    ));
    let reversed = ConditionalBonusTaxFacts {
        events: vec![
            reg,
            actual.clone(),
            actual,
            registration("different-new-event", 0, "2030-01-01", 1_000_000),
        ],
    };
    assert!(book
        .assessment_with_conditional_bonus(&empty_lineage(&book), &reversed)
        .is_err());
}
#[test]
fn conditional_credit_scope_quantity_dates_and_classification_fail_without_input_changes() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 30);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, Some(1)),
        ],
    };
    let before = book.clone();
    for field in 0..8 {
        let mut bad = facts.clone();
        if let ConditionalBonusTaxAction::RealizeBonus {
            event_id,
            actual_credit,
            tax_acquisition,
        } = &mut bad.events[1].action
        {
            match field {
                0 => actual_credit.account = AccountId(9),
                1 => actual_credit.stock = StockCode("wrong".into()),
                2 => actual_credit.event_id = "wrong".into(),
                3 => actual_credit.credited_qty = 99,
                4 => *tax_acquisition = Some(2),
                5 => *event_id = "missing-registration".into(),
                6 => *tax_acquisition = None,
                _ => {}
            }
        }
        if field == 7 {
            bad.events[1].day = date("2030-01-01");
        }
        let snapshot = bad.clone();
        assert!(
            book.assessment_with_conditional_bonus(&empty_lineage(&book), &bad)
                .is_err(),
            "field={field}"
        );
        assert_eq!(bad, snapshot);
        assert_eq!(book, before);
    }
}
#[test]
fn strict_bonus_facts_round_trip_and_restore_then_continue_preserve_basis() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 100);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 1, "2030-01-02", 100, Some(1)),
        ],
    };
    book.record_net_day("sell20".into(), date("2030-01-03"), -20, None)
        .unwrap();
    let mut restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    let decoded: ConditionalBonusTaxFacts =
        serde_json::from_value(serde_json::to_value(&facts).unwrap()).unwrap();
    book.record_net_day("sell80".into(), date("2030-01-04"), -80, None)
        .unwrap();
    restored
        .record_net_day("sell80".into(), date("2030-01-04"), -80, None)
        .unwrap();
    assert_eq!(
        restored
            .assessment_with_conditional_bonus(&empty_lineage(&restored), &decoded)
            .unwrap(),
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
    );
    let mut unknown = serde_json::to_value(&facts).unwrap();
    unknown["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ConditionalBonusTaxFacts>(unknown).is_err());
    let mut missing = serde_json::to_value(&facts).unwrap();
    missing["events"][1]["action"]["RealizeBonus"]
        .as_object_mut()
        .unwrap()
        .remove("tax_acquisition");
    assert!(serde_json::from_value::<ConditionalBonusTaxFacts>(missing).is_err());
}
#[test]
fn fractional_original_bonus_carriers_follow_the_same_frozen_partition() {
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("long", 1, "2028-12-01"), lot("short", 1, "2029-12-25")],
    )
    .unwrap();
    book.record_net_day("zero".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let before = book.lots().to_vec();
    book.record_redenomination_reduction("consolidate".into(), date("2030-01-02"), 1)
        .unwrap();
    let lineage = TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![TaxUnitTransition {
            event_id: "consolidate".into(),
            operation_seq: 2,
            day: date("2030-01-02"),
            from_unit_event: None,
            before_lots: before,
            after_lots: book.lots().to_vec(),
            fragments: vec![
                fragment("long", q(0), q(1), "short", q(0), fraction(1, 2)),
                fragment("short", q(0), q(1), "short", fraction(1, 2), q(1)),
            ],
            non_disposal_retirements: vec![],
        }],
    };
    credit(&mut book, 2);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 3, "2030-01-02", 2, Some(3)),
        ],
    };
    book.record_net_day("sell-one".into(), date("2030-01-03"), -1, None)
        .unwrap();
    let result = book
        .assessment_with_conditional_bonus(&lineage, &facts)
        .unwrap();
    assert_eq!(result.outstanding.numerator(), 20);
    assert_eq!(result.consumptions.len(), 2);
    assert_eq!(result.consumptions[0].original, origin("long", q(0), q(1)));
    assert_eq!(result.consumptions[1].original, origin("short", q(0), q(1)));
}
#[test]
fn corrected_legacy_cash_overcollection_cannot_be_hidden_by_activated_bonus() {
    let mut book = cash_book(vec![
        lot("long", 50, "2028-12-01"),
        lot("short", 50, "2029-12-25"),
    ]);
    book.record_net_day("zero".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let before = book.lots().to_vec();
    book.record_redenomination_reduction("consolidate".into(), date("2030-01-02"), 50)
        .unwrap();
    let lineage = TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![TaxUnitTransition {
            event_id: "consolidate".into(),
            operation_seq: 4,
            day: date("2030-01-02"),
            from_unit_event: None,
            before_lots: before,
            after_lots: book.lots().to_vec(),
            fragments: vec![
                fragment("long", q(0), q(50), "short", q(0), q(25)),
                fragment("short", q(0), q(50), "short", q(25), q(50)),
            ],
            non_disposal_retirements: vec![],
        }],
    };
    credit(&mut book, 50);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 4, "2030-01-02", 1_000_000),
            realization(&book, "bonus", 5, "2030-01-02", 50, Some(5)),
        ],
    };
    book.record_net_day("sell25".into(), date("2030-01-03"), -25, None)
        .unwrap();
    book.collect_due(
        "old-cash".into(),
        date("2030-01-03"),
        Money::from_cents(500),
    )
    .unwrap();
    let snapshot = book.clone();
    assert!(
        matches!(book.assessment_with_conditional_bonus(&lineage,&facts),
        Err(TaxLineageError::LegacyCashOvercollection {event_id,operation_seq:7,assessed_cash:0,previously_collected:0,attempted_cents:500}) if event_id=="old-cash")
    );
    assert_eq!(book, snapshot);
}

#[test]
fn fractional_distribution_and_nonpositive_terms_are_explicitly_rejected() {
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("original", 1, "2029-12-25")],
    )
    .unwrap();
    book.record_net_day("given-net-zero".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 0, "2030-01-01", 500_000),
            realization(&book, "bonus", 1, "2030-01-02", 1, None),
        ],
    };
    assert!(matches!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts),
        Err(TaxLineageError::UnsupportedBonusAllocation {
            original_qty: 1,
            ratio_micros: 500_000,
            actual_qty: 1,
            ..
        })
    ));
    for invalid in [0, -1] {
        let mut bad = facts.clone();
        if let ConditionalBonusTaxAction::RegisterBonus {
            par_value_per_share,
            ..
        } = &mut bad.events[0].action
        {
            *par_value_per_share = Money::from_cents(invalid);
        }
        assert!(matches!(
            book.assessment_with_conditional_bonus(&empty_lineage(&book), &bad),
            Err(TaxLineageError::InvalidConditionalFact { .. })
        ));
    }
}
#[test]
fn exact_real_cash_collection_is_deducted_before_late_bonus_realization() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    book.record_net_day("given-net-minus40".into(), date("2030-01-02"), -40, None)
        .unwrap();
    book.collect_due(
        "actual-cash".into(),
        date("2030-01-02"),
        Money::from_cents(300),
    )
    .unwrap();
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 2, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 4, "2030-01-02", 100, None),
        ],
    };
    let before = book.clone();
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &facts)
            .unwrap()
            .outstanding
            .numerator(),
        1300
    );
    assert_eq!(book, before);
    assert_eq!(book.collections()[0].outstanding.numerator(), 500);
}
#[test]
fn cash_and_bonus_activated_before_consolidation_share_exact_original_unit_consumption() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    credit(&mut book, 100);
    let facts = ConditionalBonusTaxFacts {
        events: vec![
            registration("bonus", 2, "2030-01-01", 1_000_000),
            realization(&book, "bonus", 3, "2030-01-02", 100, Some(3)),
        ],
    };
    book.record_net_day("zero".into(), date("2030-01-03"), 0, None)
        .unwrap();
    let before = book.lots().to_vec();
    book.record_redenomination_reduction("consolidate".into(), date("2030-01-03"), 100)
        .unwrap();
    let lineage = TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![TaxUnitTransition {
            event_id: "consolidate".into(),
            operation_seq: 5,
            day: date("2030-01-03"),
            from_unit_event: None,
            before_lots: before,
            after_lots: book.lots().to_vec(),
            fragments: vec![
                fragment("original", q(0), q(100), "new", q(0), q(50)),
                fragment("new", q(0), q(100), "new", q(50), q(100)),
            ],
            non_disposal_retirements: vec![],
        }],
    };
    book.record_net_day("sell10".into(), date("2030-01-04"), -10, None)
        .unwrap();
    let partial = book
        .assessment_with_conditional_bonus(&lineage, &facts)
        .unwrap();
    assert_eq!(partial.outstanding.numerator(), 800);
    assert_eq!(partial.consumptions.len(), 2);
    assert!(partial
        .consumptions
        .iter()
        .all(|c| c.original == origin("original", q(0), q(20))));
    book.record_net_day("sell-rest90".into(), date("2030-01-05"), -90, None)
        .unwrap();
    let complete = book
        .assessment_with_conditional_bonus(&lineage, &facts)
        .unwrap();
    assert_eq!(complete.outstanding.numerator(), 4000);
    assert_eq!(complete.consumptions.len(), 4);
    assert!(complete.consumptions[2..]
        .iter()
        .all(|c| c.original == origin("original", q(20), q(100))));
}

#[test]
fn appending_identical_register_and_realize_retries_after_later_frontier_is_idempotent() {
    let mut book = bonus_book("2029-12-25");
    credit(&mut book, 100);
    book.record_net_day("sell-original".into(), date("2030-01-03"), -100, None)
        .unwrap();
    let reg = registration("bonus", 0, "2030-01-01", 1_000_000);
    let actual = realization(&book, "bonus", 1, "2030-01-02", 100, Some(1));
    let once = ConditionalBonusTaxFacts {
        events: vec![
            reg.clone(),
            actual.clone(),
            registration("later", 2, "2030-01-03", 1_000_000),
        ],
    };
    let expected = book
        .assessment_with_conditional_bonus(&empty_lineage(&book), &once)
        .unwrap();
    assert_eq!(expected.outstanding.numerator(), 2000);
    let mut retry = once;
    retry.events.push(reg.clone());
    retry.events.push(actual);
    assert_eq!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &retry)
            .unwrap(),
        expected
    );
    if let ConditionalBonusTaxAction::RegisterBonus {
        par_value_per_share,
        ..
    } = &mut reg.clone().action
    {
        *par_value_per_share = Money::from_cents(101);
    }
    let mut conflicting = reg;
    if let ConditionalBonusTaxAction::RegisterBonus {
        par_value_per_share,
        ..
    } = &mut conflicting.action
    {
        *par_value_per_share = Money::from_cents(101);
    }
    retry.events.push(conflicting);
    assert!(matches!(
        book.assessment_with_conditional_bonus(&empty_lineage(&book), &retry),
        Err(TaxLineageError::InvalidConditionalFact { .. })
    ));
}
