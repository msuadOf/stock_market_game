use super::*;

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}
fn q(n: i128) -> ExactShareUnits {
    ExactShareUnits::new(n, 1).unwrap()
}
fn fraction(n: i128, d: u64) -> ExactShareUnits {
    ExactShareUnits::new(n, d).unwrap()
}
fn lot(id: &str, qty: u64, acquired: &str) -> DividendTaxLot {
    DividendTaxLot {
        id: id.into(),
        qty,
        acquired_on: date(acquired),
        source: TaxAcquisitionSource::InitialAllocation {
            evidence: id.into(),
        },
        class: TaxShareClass::PublicMarket,
    }
}
fn cash_book(lots: Vec<DividendTaxLot>) -> CashDividendTaxBook {
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        lots,
    )
    .unwrap();
    let gross = total_quantity(book.lots()).unwrap() as i64 * 100;
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "cash",
        "actual-paid".into(),
        date("2030-01-01"),
        Money::from_cents(gross),
        "真实现金红利回执".into(),
    )
    .unwrap();
    book
}
fn origin(id: &str, start: ExactShareUnits, end: ExactShareUnits) -> TaxOriginSpan {
    TaxOriginSpan {
        lot_id: id.into(),
        start,
        end,
    }
}
fn current(id: &str, start: ExactShareUnits, end: ExactShareUnits) -> TaxCurrentSpan {
    TaxCurrentSpan {
        lot_id: id.into(),
        start,
        end,
    }
}
fn fragment(
    from: &str,
    a: ExactShareUnits,
    b: ExactShareUnits,
    to: &str,
    c: ExactShareUnits,
    d: ExactShareUnits,
) -> TaxUnitFragment {
    TaxUnitFragment {
        from: origin(from, a, b),
        to: current(to, c, d),
    }
}
fn consolidate(
    book: &mut CashDividendTaxBook,
    remove: u64,
    fragments: Vec<TaxUnitFragment>,
    retire: Vec<TaxOriginSpan>,
) -> TaxUnitLineage {
    // 条件算术 fixture：该完整自然日市场净额明确为0，不代表生产可补造空日结。
    book.record_net_day("conditional-zero-jan02".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let before = book.lots().to_vec();
    book.record_redenomination_reduction("consolidation".into(), date("2030-01-02"), remove)
        .unwrap();
    TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![TaxUnitTransition {
            event_id: "consolidation".into(),
            operation_seq: book.redenominations.last().unwrap().operation_seq,
            day: date("2030-01-02"),
            from_unit_event: None,
            before_lots: before,
            after_lots: book.lots().to_vec(),
            fragments,
            non_disposal_retirements: retire,
        }],
    }
}
fn single_consolidation() -> (CashDividendTaxBook, TaxUnitLineage) {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    let lineage = consolidate(
        &mut book,
        50,
        vec![fragment("original", q(0), q(100), "original", q(0), q(50))],
        vec![],
    );
    (book, lineage)
}
#[test]
fn real_cash_book_consolidation_must_assess_original_hundred_units() {
    let (mut book, lineage) = single_consolidation();
    book.record_net_day("sell50".into(), date("2030-01-03"), -50, None)
        .unwrap();
    // 真实旧 API 当前算1000分，证明失去一半原登记所得的实际错误。
    assert_eq!(book.outstanding().unwrap().numerator(), 1_000);
    let before = book.clone();
    let mapped = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(mapped.outstanding.numerator(), 2_000);
    assert_eq!(mapped.outstanding.denominator(), 1);
    assert_eq!(book, before);
}
#[test]
fn partial_disposals_consume_twenty_then_eighty_original_units() {
    let (mut book, lineage) = single_consolidation();
    book.record_net_day("sell10".into(), date("2030-01-03"), -10, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        400
    );
    book.record_net_day("sell40".into(), date("2030-01-04"), -40, None)
        .unwrap();
    let mapped = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(mapped.outstanding.numerator(), 2_000);
    assert_eq!(mapped.consumptions.len(), 2);
    assert_eq!(mapped.consumptions[0].original.start, q(0));
    assert_eq!(mapped.consumptions[0].original.end, q(20));
    assert_eq!(mapped.consumptions[1].original.start, q(20));
    assert_eq!(mapped.consumptions[1].original.end, q(100));
}
#[test]
fn overlapping_target_is_rejected_even_when_both_totals_match() {
    let mut book = cash_book(vec![lot("a", 50, "2029-12-25"), lot("b", 50, "2029-12-25")]);
    let lineage = consolidate(
        &mut book,
        50,
        vec![
            fragment("a", q(0), q(50), "b", q(0), q(25)),
            fragment("b", q(0), q(50), "b", q(0), q(25)),
        ],
        vec![],
    );
    let before = book.clone();
    assert!(matches!(
        book.assessment_with_unit_lineage(&lineage).unwrap_err(),
        TaxLineageError::InvalidPartition {
            side: PartitionSide::Current,
            ..
        }
    ));
    assert_eq!(book, before);
}
#[test]
fn mixed_holding_periods_follow_explicit_target_fragments() {
    let mut book = cash_book(vec![
        lot("long", 50, "2028-12-01"),
        lot("short", 50, "2029-12-25"),
    ]);
    let lineage = consolidate(
        &mut book,
        50,
        vec![
            fragment("long", q(0), q(50), "short", q(0), q(25)),
            fragment("short", q(0), q(50), "short", q(25), q(50)),
        ],
        vec![],
    );
    book.record_net_day("sell_first25".into(), date("2030-01-03"), -25, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        0
    );
    book.record_net_day("sell_last25".into(), date("2030-01-04"), -25, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        1_000
    );
}
#[test]
fn fractional_internal_target_spans_are_not_floored_into_lost_rights() {
    let mut book = cash_book(vec![
        lot("long", 1, "2028-12-01"),
        lot("short", 1, "2029-12-25"),
    ]);
    let lineage = consolidate(
        &mut book,
        1,
        vec![
            fragment("long", q(0), q(1), "short", q(0), fraction(1, 2)),
            fragment("short", q(0), q(1), "short", fraction(1, 2), q(1)),
        ],
        vec![],
    );
    book.record_net_day("sell1".into(), date("2030-01-03"), -1, None)
        .unwrap();
    let mapped = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(mapped.outstanding.numerator(), 20);
    assert_eq!(mapped.consumptions.len(), 2);
    assert!(mapped
        .consumptions
        .iter()
        .all(|c| c.original.start == q(0) && c.original.end == q(1)));
}
#[test]
fn complete_source_partition_and_context_are_required() {
    let (book, lineage) = single_consolidation();
    let mut missing = lineage.clone();
    missing.transitions.clear();
    assert!(matches!(
        book.assessment_with_unit_lineage(&missing).unwrap_err(),
        TaxLineageError::MissingTransition { .. }
    ));
    let mut bad = lineage.clone();
    bad.account = AccountId(2);
    assert!(matches!(
        book.assessment_with_unit_lineage(&bad).unwrap_err(),
        TaxLineageError::ContextMismatch { .. }
    ));
    let mut gap = lineage.clone();
    gap.transitions[0].fragments[0].from.end = q(99);
    assert!(matches!(
        book.assessment_with_unit_lineage(&gap).unwrap_err(),
        TaxLineageError::InvalidPartition {
            side: PartitionSide::Origin,
            ..
        }
    ));
}
#[test]
fn duplicate_identical_transition_is_idempotent_but_conflict_is_explicit() {
    let (mut book, lineage) = single_consolidation();
    book.record_net_day("sell".into(), date("2030-01-03"), -50, None)
        .unwrap();
    let once = book.assessment_with_unit_lineage(&lineage).unwrap();
    let mut twice = lineage.clone();
    twice.transitions.push(lineage.transitions[0].clone());
    assert_eq!(book.assessment_with_unit_lineage(&twice).unwrap(), once);
    twice.transitions[1].day = date("2030-01-04");
    assert!(matches!(
        book.assessment_with_unit_lineage(&twice).unwrap_err(),
        TaxLineageError::ConflictingTransition { .. }
    ));
}
#[test]
fn strict_lineage_round_trip_replays_same_assessment() {
    let (mut book, lineage) = single_consolidation();
    book.record_net_day("sell".into(), date("2030-01-03"), -50, None)
        .unwrap();
    let encoded = serde_json::to_value(&lineage).unwrap();
    let restored: TaxUnitLineage = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&restored)
            .unwrap()
            .outstanding
            .numerator(),
        2_000
    );
    let mut bad = encoded.clone();
    bad["transitions"][0]["fragments"][0]["from"]["end"]["numerator"] = serde_json::json!("0100");
    assert!(serde_json::from_value::<TaxUnitLineage>(bad).is_err());
    let mut missing = encoded;
    missing.as_object_mut().unwrap().remove("transitions");
    assert!(serde_json::from_value::<TaxUnitLineage>(missing).is_err());
    let mut missing_predecessor = serde_json::to_value(&lineage).unwrap();
    missing_predecessor["transitions"][0]
        .as_object_mut()
        .unwrap()
        .remove("from_unit_event");
    assert!(serde_json::from_value::<TaxUnitLineage>(missing_predecessor).is_err());
}
#[test]
fn retired_original_rights_are_not_assigned_to_later_new_purchase() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    let lineage = consolidate(
        &mut book,
        100,
        vec![],
        vec![origin("original", q(0), q(100))],
    );
    book.record_net_day(
        "buy-new".into(),
        date("2030-01-03"),
        10,
        Some(DividendTaxLot {
            id: "new".into(),
            qty: 10,
            acquired_on: date("2030-01-03"),
            source: TaxAcquisitionSource::SecondaryMarket {
                settlement: "真实新买入".into(),
            },
            class: TaxShareClass::PublicMarket,
        }),
    )
    .unwrap();
    book.record_net_day("sell-new".into(), date("2030-01-04"), -10, None)
        .unwrap();
    let mapped = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(mapped.outstanding.numerator(), 0);
    assert!(mapped.consumptions.is_empty());
    assert_eq!(mapped.retirements.len(), 1);
    assert_eq!(mapped.retirements[0].original.end, q(100));
}

#[test]
fn partial_disposal_then_second_transition_preserves_remaining_original_span() {
    let (mut book, mut lineage) = single_consolidation();
    book.record_net_day("sell10".into(), date("2030-01-03"), -10, None)
        .unwrap();
    book.record_net_day("conditional-zero-jan04".into(), date("2030-01-04"), 0, None)
        .unwrap();
    let before = book.lots().to_vec();
    book.record_redenomination_reduction("second".into(), date("2030-01-04"), 20)
        .unwrap();
    lineage.transitions.push(TaxUnitTransition {
        event_id: "second".into(),
        operation_seq: book.redenominations.last().unwrap().operation_seq,
        day: date("2030-01-04"),
        from_unit_event: Some("consolidation".into()),
        before_lots: before,
        after_lots: book.lots().to_vec(),
        fragments: vec![fragment("original", q(0), q(40), "original", q(0), q(20))],
        non_disposal_retirements: vec![],
    });
    book.record_net_day("sell20".into(), date("2030-01-05"), -20, None)
        .unwrap();
    let assessment = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(assessment.outstanding.numerator(), 2_000);
    assert_eq!(assessment.consumptions.len(), 2);
    assert_eq!(
        assessment.consumptions[0].original,
        origin("original", q(0), q(20))
    );
    assert_eq!(
        assessment.consumptions[1].original,
        origin("original", q(20), q(100))
    );
}

#[test]
fn transition_must_match_real_operation_snapshot_and_unit_predecessor() {
    let (book, lineage) = single_consolidation();
    let before = book.clone();
    let mut bad_seq = lineage.clone();
    bad_seq.transitions[0].operation_seq = 1;
    assert!(matches!(
        book.assessment_with_unit_lineage(&bad_seq).unwrap_err(),
        TaxLineageError::InvalidOperation { .. }
    ));
    let mut bad_snapshot = lineage.clone();
    bad_snapshot.transitions[0].before_lots[0].acquired_on = date("2029-12-24");
    assert!(matches!(
        book.assessment_with_unit_lineage(&bad_snapshot)
            .unwrap_err(),
        TaxLineageError::TransitionMismatch { .. }
    ));
    let mut bad_predecessor = lineage;
    bad_predecessor.transitions[0].from_unit_event = Some("不存在的前驱".into());
    assert!(matches!(
        book.assessment_with_unit_lineage(&bad_predecessor)
            .unwrap_err(),
        TaxLineageError::UnitPredecessorMismatch { .. }
    ));
    assert_eq!(book, before);
}

#[test]
fn each_registration_keeps_its_own_unit_basis_and_actual_payment() {
    let (mut book, lineage) = single_consolidation();
    // 第二次登记发生于缩股后：50当前份，每份200分，也实际收到10000分。
    book.register_dividend(
        "later".into(),
        date("2030-01-02"),
        ExactDividendTaxAmount::new(200, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "later",
        "paid-later".into(),
        date("2030-01-02"),
        Money::from_cents(10_000),
        "第二笔实际付款".into(),
    )
    .unwrap();
    book.record_net_day("sell50".into(), date("2030-01-03"), -50, None)
        .unwrap();
    let assessment = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(assessment.outstanding.numerator(), 4_000);
    assert_eq!(assessment.consumptions.len(), 2);
    assert_eq!(
        assessment.consumptions[0].key,
        TaxEntitlementKey {
            kind: TaxIncomeKind::CashDividend,
            event_id: "cash".into()
        }
    );
    assert_eq!(assessment.consumptions[0].original.end, q(100));
    assert_eq!(
        assessment.consumptions[1].key,
        TaxEntitlementKey {
            kind: TaxIncomeKind::CashDividend,
            event_id: "later".into()
        }
    );
    assert_eq!(assessment.consumptions[1].original.end, q(50));
}

#[test]
fn partition_rejects_source_overlap_target_gap_out_of_bounds_and_ghost_lot() {
    let (book, lineage) = single_consolidation();
    let before = book.clone();
    let mut cases = Vec::new();
    let mut source_overlap = lineage.clone();
    source_overlap.transitions[0]
        .non_disposal_retirements
        .push(origin("original", q(0), q(1)));
    cases.push((source_overlap, PartitionSide::Origin));
    let mut target_gap = lineage.clone();
    target_gap.transitions[0].fragments[0].to.start = q(1);
    cases.push((target_gap, PartitionSide::Current));
    let mut target_overflow = lineage.clone();
    target_overflow.transitions[0].fragments[0].to.end = q(51);
    cases.push((target_overflow, PartitionSide::Current));
    let mut ghost = lineage;
    ghost.transitions[0].fragments[0].to.lot_id = "ghost".into();
    cases.push((ghost, PartitionSide::Current));
    for (bad, expected) in cases {
        assert!(matches!(book.assessment_with_unit_lineage(&bad),
            Err(TaxLineageError::InvalidPartition { side, .. }) if side == expected));
        assert_eq!(book, before);
    }
}

#[test]
fn one_origin_can_span_multiple_target_fragments_and_retirement() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    let lineage = consolidate(
        &mut book,
        50,
        vec![
            fragment("original", q(0), q(40), "original", q(0), q(25)),
            fragment("original", q(40), q(80), "original", q(25), q(50)),
        ],
        vec![origin("original", q(80), q(100))],
    );
    book.record_net_day("sell-first".into(), date("2030-01-03"), -25, None)
        .unwrap();
    let first = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(first.outstanding.numerator(), 800);
    assert_eq!(
        first.consumptions[0].original,
        origin("original", q(0), q(40))
    );
    assert_eq!(
        first.retirements[0].original,
        origin("original", q(80), q(100))
    );
    book.record_net_day("sell-last".into(), date("2030-01-04"), -25, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        1600
    );
}

#[test]
fn expansion_transports_original_rights_to_both_real_current_lots() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    book.record_net_day("zero".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let before = book.lots().to_vec();
    book.record_net_day(
        "credit-receipt".into(),
        date("2030-01-02"),
        100,
        Some(DividendTaxLot {
            id: "additional".into(),
            qty: 100,
            acquired_on: date("2030-01-02"),
            source: TaxAcquisitionSource::CorporateAction {
                event: "expand".into(),
            },
            class: TaxShareClass::PublicMarket,
        }),
    )
    .unwrap();
    let lineage = TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![TaxUnitTransition {
            event_id: "expand".into(),
            operation_seq: book.days.last().unwrap().operation_seq,
            day: date("2030-01-02"),
            from_unit_event: None,
            before_lots: before,
            after_lots: book.lots().to_vec(),
            fragments: vec![
                fragment("original", q(0), q(50), "original", q(0), q(100)),
                fragment("original", q(50), q(100), "additional", q(0), q(100)),
            ],
            non_disposal_retirements: vec![],
        }],
    };
    book.record_net_day("sell-old".into(), date("2030-01-03"), -100, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        1000
    );
    book.record_net_day("sell-extra".into(), date("2030-01-04"), -100, None)
        .unwrap();
    let assessment = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(assessment.outstanding.numerator(), 2000);
    assert_eq!(
        assessment.consumptions[1].original,
        origin("original", q(50), q(100))
    );
}

#[test]
fn no_mapping_preserves_cash_activation_rounding_collection_and_restore() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    let lineage = TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![],
    };
    book.record_net_day("sell40".into(), date("2030-01-02"), -40, None)
        .unwrap();
    let collected = book
        .collect_due("collect".into(), date("2030-01-02"), Money::from_cents(300))
        .unwrap();
    assert_eq!(collected.collected.cents(), 300);
    let mapped = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(mapped.outstanding, book.outstanding().unwrap());
    assert_eq!(mapped.outstanding.numerator(), 500);
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(
        restored.assessment_with_unit_lineage(&lineage).unwrap(),
        mapped
    );
    let mut corrupt = restored;
    corrupt.lots[0].qty += 1;
    let before = corrupt.clone();
    assert!(corrupt.assessment_with_unit_lineage(&lineage).is_err());
    assert_eq!(corrupt, before);
}

#[test]
fn exact_fraction_serialization_and_extreme_comparison_do_not_overflow_or_round() {
    let maximal = ExactShareUnits::new(i128::MAX, u64::MAX).unwrap();
    assert_eq!(units_cmp(&maximal, &q(1)), std::cmp::Ordering::Greater);
    let restored: ExactShareUnits =
        serde_json::from_str(&serde_json::to_string(&fraction(1, 2)).unwrap()).unwrap();
    assert_eq!(restored, fraction(1, 2));
    assert!(ExactShareUnits::new(1, 0).is_err());
    assert!(ExactShareUnits::new(-1, 1).is_err());
    assert!(units_product(&[q(i128::MAX), q(2)]).is_err());
    assert_eq!(
        project(&fraction(1, 2), &q(0), &q(1), &q(0), &q(100)).unwrap(),
        q(50)
    );
}

#[test]
fn actual_partial_payments_activate_only_received_cash_and_retry_is_idempotent() {
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("original", 100, "2029-12-25")],
    )
    .unwrap();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    let lineage = consolidate(
        &mut book,
        50,
        vec![fragment("original", q(0), q(100), "original", q(0), q(50))],
        vec![],
    );
    book.record_net_day("sell".into(), date("2030-01-03"), -50, None)
        .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        0
    );
    book.record_payment(
        "cash",
        "first".into(),
        date("2030-01-03"),
        Money::from_cents(5000),
        "实际第一笔".into(),
    )
    .unwrap();
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        1000
    );
    book.record_payment(
        "cash",
        "second".into(),
        date("2030-01-03"),
        Money::from_cents(5000),
        "实际第二笔".into(),
    )
    .unwrap();
    let before = book.clone();
    book.record_payment(
        "cash",
        "second".into(),
        date("2030-01-03"),
        Money::from_cents(5000),
        "实际第二笔".into(),
    )
    .unwrap();
    assert_eq!(book, before);
    assert_eq!(
        book.assessment_with_unit_lineage(&lineage)
            .unwrap()
            .outstanding
            .numerator(),
        2000
    );
}

#[test]
fn mapped_outstanding_deducts_actual_legacy_collection_without_rewriting_receipt() {
    let (mut book, lineage) = single_consolidation();
    book.record_net_day("sell".into(), date("2030-01-03"), -50, None)
        .unwrap();
    let receipt = book
        .collect_due(
            "legacy-collected".into(),
            date("2030-01-03"),
            Money::from_cents(300),
        )
        .unwrap();
    assert_eq!(receipt.outstanding.numerator(), 700);
    let before = book.clone();
    let mapped = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(mapped.outstanding.numerator(), 1700);
    assert_eq!(book, before);
    assert_eq!(book.collections()[0], receipt);
}

#[test]
fn restoring_book_and_lineage_between_partial_disposals_preserves_remaining_basis() {
    let (mut book, lineage) = single_consolidation();
    book.record_net_day("sell10".into(), date("2030-01-03"), -10, None)
        .unwrap();
    let mut restored: CashDividendTaxBook =
        serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
    let restored_lineage: TaxUnitLineage =
        serde_json::from_str(&serde_json::to_string(&lineage).unwrap()).unwrap();
    book.record_net_day("sell40".into(), date("2030-01-04"), -40, None)
        .unwrap();
    restored
        .record_net_day("sell40".into(), date("2030-01-04"), -40, None)
        .unwrap();
    let continued = restored
        .assessment_with_unit_lineage(&restored_lineage)
        .unwrap();
    assert_eq!(
        continued,
        book.assessment_with_unit_lineage(&lineage).unwrap()
    );
    assert_eq!(continued.outstanding.numerator(), 2000);
    assert_eq!(continued.consumptions[0].key, cash_key("cash"));
    assert_eq!(continued.consumptions[0].disposition_event_id, "sell10");
    assert_eq!(continued.consumptions[0].disposed_on, date("2030-01-03"));
    assert_eq!(
        continued.consumptions[1].original,
        origin("original", q(20), q(100))
    );
    assert_eq!(continued.consumptions[1].disposition_event_id, "sell40");
}

#[test]
fn source_overlap_with_matching_quantity_sum_and_invalid_target_intervals_are_rejected() {
    let (book, lineage) = single_consolidation();
    let mut overlap = lineage.clone();
    overlap.transitions[0].fragments = vec![
        fragment("original", q(0), q(50), "original", q(0), q(25)),
        fragment("original", q(0), q(50), "original", q(25), q(50)),
    ];
    assert!(matches!(
        book.assessment_with_unit_lineage(&overlap),
        Err(TaxLineageError::InvalidPartition {
            side: PartitionSide::Origin,
            ..
        })
    ));
    for (start, end) in [(q(1), q(1)), (q(2), q(1))] {
        let mut bad = lineage.clone();
        bad.transitions[0].fragments[0].to.start = start;
        bad.transitions[0].fragments[0].to.end = end;
        assert!(matches!(
            book.assessment_with_unit_lineage(&bad),
            Err(TaxLineageError::InvalidPartition {
                side: PartitionSide::Current,
                ..
            })
        ));
    }
    let mut encoded = serde_json::to_value(&lineage).unwrap();
    encoded["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<TaxUnitLineage>(encoded).is_err());
    for (numerator, denominator) in [("2", "2"), ("1", "0")] {
        let mut bad = serde_json::to_value(&lineage).unwrap();
        bad["transitions"][0]["fragments"][0]["from"]["end"] =
            serde_json::json!({"numerator":numerator,"denominator":denominator});
        assert!(serde_json::from_value::<TaxUnitLineage>(bad).is_err());
    }
}

#[test]
fn valid_boundary_fractions_that_overflow_projection_fail_typed_and_leave_inputs_unchanged() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    let tiny = fraction(1, u64::MAX);
    let lineage = consolidate(
        &mut book,
        50,
        vec![
            fragment("original", q(0), tiny.clone(), "original", q(0), q(25)),
            fragment("original", tiny, q(100), "original", q(25), q(50)),
        ],
        vec![],
    );
    book.record_net_day("sell10".into(), date("2030-01-03"), -10, None)
        .unwrap();
    let before = book.clone();
    let lineage_before = lineage.clone();
    assert!(matches!(
        book.assessment_with_unit_lineage(&lineage),
        Err(TaxLineageError::Tax(DividendTaxError::Overflow { .. }))
    ));
    assert_eq!(book, before);
    assert_eq!(lineage, lineage_before);
}

#[test]
fn corporate_credit_without_unit_classification_is_rejected_instead_of_guessing_new_rights() {
    let mut book = cash_book(vec![lot("original", 100, "2029-12-25")]);
    book.record_net_day("zero".into(), date("2030-01-02"), 0, None)
        .unwrap();
    // 条件事实：这100新单位实际为1拆2的正向取得；旧 source 不能区别独立配股。
    book.record_net_day(
        "credit".into(),
        date("2030-01-02"),
        100,
        Some(DividendTaxLot {
            id: "additional".into(),
            qty: 100,
            acquired_on: date("2030-01-02"),
            source: TaxAcquisitionSource::CorporateAction {
                event: "expand".into(),
            },
            class: TaxShareClass::PublicMarket,
        }),
    )
    .unwrap();
    book.record_net_day("sell20".into(), date("2030-01-03"), -20, None)
        .unwrap();
    let lineage = TaxUnitLineage {
        account: book.account(),
        stock: book.stock().clone(),
        transitions: vec![],
    };
    let before = book.clone();
    assert!(matches!(book.assessment_with_unit_lineage(&lineage),
        Err(TaxLineageError::MissingTransition{event_id,operation_seq})
        if event_id=="expand" && operation_seq==4));
    assert_eq!(book, before);
}
