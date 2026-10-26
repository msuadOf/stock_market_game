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
    missing_predecessor["transitions"][0].as_object_mut().unwrap().remove("from_unit_event");
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
    assert_eq!(assessment.consumptions[0].original, origin("original", q(0), q(20)));
    assert_eq!(assessment.consumptions[1].original, origin("original", q(20), q(100)));
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
        book.assessment_with_unit_lineage(&bad_snapshot).unwrap_err(),
        TaxLineageError::TransitionMismatch { .. }
    ));
    let mut bad_predecessor = lineage;
    bad_predecessor.transitions[0].from_unit_event = Some("不存在的前驱".into());
    assert!(matches!(
        book.assessment_with_unit_lineage(&bad_predecessor).unwrap_err(),
        TaxLineageError::UnitPredecessorMismatch { .. }
    ));
    assert_eq!(book, before);
}

#[test]
fn each_registration_keeps_its_own_unit_basis_and_actual_payment() {
    let (mut book, lineage) = single_consolidation();
    // 第二次登记发生于缩股后：50当前份，每份200分，也实际收到10000分。
    book.register_dividend("later".into(), date("2030-01-02"), ExactDividendTaxAmount::new(200, 1).unwrap())
        .unwrap();
    book.record_payment("later", "paid-later".into(), date("2030-01-02"), Money::from_cents(10_000), "第二笔实际付款".into())
        .unwrap();
    book.record_net_day("sell50".into(), date("2030-01-03"), -50, None)
        .unwrap();
    let assessment = book.assessment_with_unit_lineage(&lineage).unwrap();
    assert_eq!(assessment.outstanding.numerator(), 4_000);
    assert_eq!(assessment.consumptions.len(), 2);
    assert_eq!(assessment.consumptions[0].key, TaxEntitlementKey { kind: TaxIncomeKind::CashDividend, event_id: "cash".into() });
    assert_eq!(assessment.consumptions[0].original.end, q(100));
    assert_eq!(assessment.consumptions[1].key, TaxEntitlementKey { kind: TaxIncomeKind::CashDividend, event_id: "later".into() });
    assert_eq!(assessment.consumptions[1].original.end, q(50));
}
