use super::{
    allocate_stock_distribution, SourceLotAttribution, StockDistributionError,
    StockDistributionKind, StockDistributionPlan, StockDistributionReceipt,
};
use crate::company::{
    share_registry::{
        AcquisitionSource, HolderId, IssuerRepurchaseAccountFacts, RegistrationSnapshot,
        ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    },
    CompanyId,
};
use crate::{account::StockCode, calendar::CivilDate, orderbook::AccountId};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.to_owned(),
        qty,
        acquired_on: date("2020-01-02"),
        source: AcquisitionSource::InitialAllocation {
            evidence: format!("opening-{id}"),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

fn snapshot(holdings: Vec<ShareHolding>, issued_shares: u64) -> RegistrationSnapshot {
    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId("issuer-1".to_owned()),
        issued_shares,
        date("2030-06-03"),
        holdings,
    )
    .unwrap();
    registry
        .register("record-date-1".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone()
}

fn holding(holder: HolderId, lots: Vec<ShareLot>) -> ShareHolding {
    ShareHolding { holder, lots }
}

fn plan(approved_total_new_shares: u64) -> StockDistributionPlan {
    StockDistributionPlan {
        event_id: "distribution-1".to_owned(),
        approval_reference: "shareholders-resolution-1".to_owned(),
        kind: StockDistributionKind::BonusShares,
        shares_per_existing_share_micros: 500_000,
        approved_total_new_shares,
    }
}

#[test]
fn allocates_holder_level_floors_then_awards_one_fragment_share() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 3)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 3)]),
            holding(HolderId::IssuerTreasury, vec![lot("treasury", 2)]),
        ],
        8,
    );

    let receipt = allocate_stock_distribution(&snapshot, &plan(3), 17).unwrap();

    assert_eq!(receipt.issuer_treasury_shares_excluded, 2);
    assert_eq!(receipt.approved_total_new_shares, 3);
    assert_eq!(
        receipt
            .holders
            .iter()
            .map(|holder| holder.whole_shares)
            .sum::<u64>(),
        3
    );
    assert_eq!(
        receipt
            .holders
            .iter()
            .map(|holder| holder.fractional_numerator)
            .collect::<Vec<_>>(),
        vec![500_000, 500_000]
    );
    assert_eq!(
        receipt.source_lot_attribution,
        SourceLotAttribution::SourceLotAttributionPending
    );
    assert!(receipt
        .holders
        .iter()
        .all(|holder| holder.original_lots.len() == 1));
    assert!(matches!(
        allocate_stock_distribution(&snapshot, &plan(4), 17),
        Err(StockDistributionError::ApprovedTotalInfeasible { .. })
    ));
}

#[test]
fn seeded_equal_fraction_tie_break_is_repeatable_and_assigns_exact_target() {
    let record_date_snapshot = snapshot(
        (1..=8)
            .map(|id| {
                holding(
                    HolderId::Account(AccountId(id)),
                    vec![lot(&format!("a{id}"), 1)],
                )
            })
            .collect(),
        8,
    );

    let first = allocate_stock_distribution(&record_date_snapshot, &plan(4), 42).unwrap();
    let repeated = allocate_stock_distribution(&record_date_snapshot, &plan(4), 42).unwrap();

    assert_eq!(first, repeated);
    assert_eq!(
        first
            .holders
            .iter()
            .map(|holder| holder.whole_shares)
            .sum::<u64>(),
        4
    );
    let different_seed = allocate_stock_distribution(&record_date_snapshot, &plan(4), 43).unwrap();
    assert_ne!(
        first
            .holders
            .iter()
            .map(|holder| holder.whole_shares)
            .collect::<Vec<_>>(),
        different_seed
            .holders
            .iter()
            .map(|holder| holder.whole_shares)
            .collect::<Vec<_>>()
    );
    let reversed_snapshot = self::snapshot(
        (1..=8)
            .rev()
            .map(|id| {
                holding(
                    HolderId::Account(AccountId(id)),
                    vec![lot(&format!("a{id}"), 1)],
                )
            })
            .collect(),
        8,
    );
    assert_eq!(
        first,
        allocate_stock_distribution(&reversed_snapshot, &plan(4), 42).unwrap()
    );
    assert_eq!(
        first
            .holders
            .iter()
            .filter(|holder| holder.whole_shares == 1)
            .count(),
        4
    );
}

#[test]
fn rejects_approved_total_outside_holder_floor_and_ceiling_interval() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 3)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 3)]),
        ],
        6,
    );

    for target in [1, 5] {
        assert!(matches!(
            allocate_stock_distribution(&snapshot, &plan(target), 1),
            Err(StockDistributionError::ApprovedTotalInfeasible { .. })
        ));
    }
}

#[test]
fn rejects_holder_feasible_total_that_exceeds_aggregate_ratio_entitlement() {
    let snapshot = snapshot(
        (1..=1_000)
            .map(|id| {
                holding(
                    HolderId::Account(AccountId(id)),
                    vec![lot(&format!("a{id}"), 1)],
                )
            })
            .collect(),
        1_000,
    );
    let mut aggregate_plan = plan(1_000);
    aggregate_plan.shares_per_existing_share_micros = 100_000;

    assert!(matches!(
        allocate_stock_distribution(&snapshot, &aggregate_plan, 5),
        Err(StockDistributionError::ApprovedTotalInfeasible { .. })
    ));
}

#[test]
fn awards_the_largest_holder_fraction_first() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("largest", 9)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("middle", 2)]),
            holding(HolderId::Account(AccountId(3)), vec![lot("smallest", 1)]),
        ],
        12,
    );
    let mut fractional_plan = plan(1);
    fractional_plan.shares_per_existing_share_micros = 100_000;

    let receipt = allocate_stock_distribution(&snapshot, &fractional_plan, 3).unwrap();

    assert_eq!(
        receipt
            .holders
            .iter()
            .find(|holder| holder.holder == HolderId::Account(AccountId(1)))
            .unwrap()
            .whole_shares,
        1
    );
    assert_eq!(
        receipt
            .holders
            .iter()
            .filter(|holder| holder.whole_shares > 0)
            .count(),
        1
    );
}

#[test]
fn preserves_large_share_quantities_without_floating_point_conversion() {
    let shares = 9_007_199_254_740_993_u64;
    let snapshot = snapshot(
        vec![holding(
            HolderId::Account(AccountId(1)),
            vec![lot("large", shares)],
        )],
        shares,
    );
    let mut large_plan = plan(shares);
    large_plan.shares_per_existing_share_micros = 1_000_000;
    large_plan.kind = StockDistributionKind::CapitalReserveConversion;

    let receipt = allocate_stock_distribution(&snapshot, &large_plan, 0).unwrap();

    assert_eq!(receipt.holders[0].original_shares, shares);
    assert_eq!(receipt.holders[0].whole_shares, shares);
    assert_eq!(receipt.holders[0].fractional_numerator, 0);
}

#[test]
fn rejects_missing_approval_reference_and_zero_ratio() {
    let snapshot = snapshot(
        vec![holding(HolderId::Account(AccountId(1)), vec![lot("a1", 1)])],
        1,
    );
    let mut missing_approval = plan(1);
    missing_approval.approval_reference.clear();
    assert!(matches!(
        allocate_stock_distribution(&snapshot, &missing_approval, 0),
        Err(StockDistributionError::InvalidPlan { .. })
    ));
    let mut zero_ratio = plan(0);
    zero_ratio.shares_per_existing_share_micros = 0;
    assert!(matches!(
        allocate_stock_distribution(&snapshot, &zero_ratio, 0),
        Err(StockDistributionError::InvalidPlan { .. })
    ));
}

#[test]
fn capital_reserve_conversion_requires_registered_repurchase_account_facts_for_treasury() {
    let holdings = vec![holding(
        HolderId::IssuerTreasury,
        vec![lot("treasury", 1)],
    )];
    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId("issuer-1".to_owned()),
        1,
        date("2030-06-03"),
        holdings,
    )
    .unwrap();
    let mut conversion = plan(0);
    conversion.kind = StockDistributionKind::CapitalReserveConversion;
    let snapshot_without_facts = registry
        .register("record-date-without-facts".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone();
    assert!(matches!(
        allocate_stock_distribution(&snapshot_without_facts, &conversion, 0),
        Err(StockDistributionError::InvalidPlan { .. })
    ));

    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId("issuer-1".to_owned()),
        1,
        date("2030-06-03"),
        vec![holding(
            HolderId::IssuerTreasury,
            vec![lot("treasury", 1)],
        )],
    )
    .unwrap();
    registry
        .set_issuer_repurchase_account(IssuerRepurchaseAccountFacts {
            account_reference: "repurchase-account-1".to_owned(),
            source_evidence: "exchange-confirmation-1".to_owned(),
            established_on: date("2030-06-02"),
        })
        .unwrap();
    let snapshot_with_facts = registry
        .register("record-date-with-facts".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone();
    let receipt = allocate_stock_distribution(&snapshot_with_facts, &conversion, 0).unwrap();
    assert_eq!(receipt.issuer_treasury_shares_excluded, 1);
}

#[test]
fn serializes_full_width_ratio_and_tie_seed_as_canonical_decimal_strings() {
    let mut full_width = plan(0);
    full_width.shares_per_existing_share_micros = u64::MAX;
    let snapshot = snapshot(
        vec![holding(
            HolderId::IssuerTreasury,
            vec![lot("treasury", 1)],
        )],
        1,
    );
    let receipt = allocate_stock_distribution(&snapshot, &full_width, u64::MAX).unwrap();
    let encoded_plan = serde_json::to_value(&full_width).unwrap();
    let encoded = serde_json::to_value(&receipt).unwrap();

    assert_eq!(
        encoded_plan["shares_per_existing_share_micros"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(
        serde_json::from_value::<StockDistributionPlan>(encoded_plan).unwrap(),
        full_width
    );
    assert_eq!(
        encoded["shares_per_existing_share_micros"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(
        encoded["tie_break_seed"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(
        serde_json::from_value::<super::StockDistributionReceipt>(encoded).unwrap(),
        receipt
    );

    let mut numeric_ratio = serde_json::to_value(&full_width).unwrap();
    numeric_ratio["shares_per_existing_share_micros"] = serde_json::json!(u64::MAX);
    assert!(serde_json::from_value::<StockDistributionPlan>(numeric_ratio).is_err());
    let mut noncanonical_ratio = serde_json::to_value(&full_width).unwrap();
    noncanonical_ratio["shares_per_existing_share_micros"] = serde_json::json!("01");
    assert!(serde_json::from_value::<StockDistributionPlan>(noncanonical_ratio).is_err());
    let mut unknown_field = serde_json::to_value(&full_width).unwrap();
    unknown_field["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<StockDistributionPlan>(unknown_field).is_err());
    let mut unknown_receipt_field = serde_json::to_value(&receipt).unwrap();
    unknown_receipt_field["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<StockDistributionReceipt>(unknown_receipt_field).is_err());
    let mut missing_receipt_field = serde_json::to_value(&receipt).unwrap();
    missing_receipt_field
        .as_object_mut()
        .unwrap()
        .remove("tie_break_seed");
    assert!(serde_json::from_value::<StockDistributionReceipt>(missing_receipt_field).is_err());
}

mod book {
    use super::super::{
        holder_credit_lots, tie_break_seed, HolderDistribution, StockDistributionBook,
        StockDistributionError, StockDistributionEventPlan, StockDistributionKind,
        StockDistributionStatus,
    };
    use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    };
    use crate::company::{CompanyId, StockCode};
    use crate::orderbook::AccountId;

    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }

    fn event_plan(ex_rights_on: &str) -> StockDistributionEventPlan {
        StockDistributionEventPlan {
            event_id: "distribution-1".into(),
            approval_reference: "shareholders-resolution-1".into(),
            issuer: CompanyId("issuer-1".into()),
            stock: StockCode("600001".into()),
            exchange: CalendarExchange::Sse,
            kind: StockDistributionKind::BonusShares,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-03"),
            registered_on: date("2030-06-06"),
            ex_rights_on: date(ex_rights_on),
            shares_per_existing_share_micros: 500_000,
            approved_total_new_shares: 3,
        }
    }

    fn restricted_lot(id: &str, qty: u64, release_on: &str) -> ShareLot {
        ShareLot {
            id: id.into(),
            qty,
            acquired_on: date("2030-01-02"),
            source: AcquisitionSource::InitialAllocation {
                evidence: format!("opening-{id}"),
            },
            restriction: ShareRestriction::Restricted {
                reason: "nonfloat-lock".into(),
                release_on: date(release_on),
            },
        }
    }

    fn frozen_snapshot(
        stock: &StockCode,
        issuer: &CompanyId,
        holdings: Vec<ShareHolding>,
        issued_shares: u64,
    ) -> crate::company::share_registry::RegistrationSnapshot {
        let mut registry = ShareRegistry::new(
            stock.clone(),
            issuer.clone(),
            issued_shares,
            date("2030-06-06"),
            holdings,
        )
        .unwrap();
        registry
            .register("distribution-1".to_owned(), date("2030-06-06"))
            .unwrap()
            .clone()
    }

    fn registered_book(calendar: &TradingCalendar) -> StockDistributionBook {
        let mut book = StockDistributionBook::new(event_plan("2030-06-07")).unwrap();
        book.announce(date("2030-06-03")).unwrap();
        let snapshot = frozen_snapshot(
            &StockCode("600001".into()),
            &CompanyId("issuer-1".into()),
            vec![
                ShareHolding {
                    holder: HolderId::Account(AccountId(1)),
                    lots: vec![super::lot("a1", 3)],
                },
                ShareHolding {
                    holder: HolderId::Account(AccountId(2)),
                    lots: vec![super::lot("a2", 3)],
                },
                ShareHolding {
                    holder: HolderId::IssuerTreasury,
                    lots: vec![super::lot("treasury", 4)],
                },
            ],
            10,
        );
        book.register(snapshot, calendar).unwrap();
        book
    }

    #[test]
    fn event_plan_requires_ordered_dates_and_next_trading_day_ex_date() {
        let calendar = TradingCalendar::current_default_calendar().unwrap();
        assert!(event_plan("2030-06-07").validate().is_ok());
        assert!(event_plan("2030-06-07").validate_calendar(&calendar).is_ok());
        assert!(matches!(
            event_plan("2030-06-08").validate_calendar(&calendar),
            Err(StockDistributionError::InvalidExRightsDate { .. })
        ));
        let mut weekend_registration = event_plan("2030-06-10");
        weekend_registration.registered_on = date("2030-06-09");
        assert!(matches!(
            weekend_registration.validate_calendar(&calendar),
            Err(StockDistributionError::InvalidTradingDate { .. })
        ));
        let mut unordered = event_plan("2030-06-07");
        unordered.announced_on = date("2030-05-31");
        assert!(matches!(
            unordered.validate(),
            Err(StockDistributionError::InvalidPlan { .. })
        ));
    }

    #[test]
    fn book_lifecycle_freezes_allocation_and_records_credit_idempotently() {
        let calendar = TradingCalendar::current_default_calendar().unwrap();
        let mut book = registered_book(&calendar);
        assert_eq!(book.status(), &StockDistributionStatus::Registered);
        let receipt = book.receipt().unwrap();
        assert_eq!(receipt.issuer_treasury_shares_excluded, 4);
        assert_eq!(
            receipt
                .holders
                .iter()
                .map(|holder| holder.whole_shares)
                .sum::<u64>(),
            3
        );
        assert_eq!(receipt.tie_break_seed, tie_break_seed("distribution-1"));
        book.mark_credited(date("2030-06-07")).unwrap();
        assert_eq!(book.status(), &StockDistributionStatus::Credited);
        assert_eq!(book.credited_on(), Some(date("2030-06-07")));
        book.mark_credited(date("2030-06-07")).unwrap();
        assert!(matches!(
            book.mark_credited(date("2030-06-10")),
            Err(StockDistributionError::WrongStage { .. })
        ));
        book.validate_with_calendar(&calendar).unwrap();
        let restored: StockDistributionBook =
            serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
        assert_eq!(restored, book);
    }

    #[test]
    fn book_register_rejects_wrong_stage_and_foreign_snapshots_and_replays_strictly() {
        let calendar = TradingCalendar::current_default_calendar().unwrap();
        let mut book = StockDistributionBook::new(event_plan("2030-06-07")).unwrap();
        let snapshot = frozen_snapshot(
            &StockCode("600001".into()),
            &CompanyId("issuer-1".into()),
            vec![ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![super::lot("a1", 6)],
            }],
            6,
        );
        assert!(matches!(
            book.register(snapshot.clone(), &calendar),
            Err(StockDistributionError::WrongStage {
                operation: "register",
                ..
            })
        ));
        book.announce(date("2030-06-03")).unwrap();
        let foreign = frozen_snapshot(
            &StockCode("600002".into()),
            &CompanyId("issuer-1".into()),
            vec![ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![super::lot("a1", 6)],
            }],
            6,
        );
        assert!(matches!(
            book.register(foreign, &calendar),
            Err(StockDistributionError::SnapshotMismatch { .. })
        ));
        book.register(snapshot.clone(), &calendar).unwrap();
        book.register(snapshot, &calendar).unwrap();
        let mut corrupt = serde_json::to_value(&book).unwrap();
        corrupt["receipt"]["approved_total_new_shares"] = serde_json::json!("2");
        assert!(serde_json::from_value::<StockDistributionBook>(corrupt).is_err());
        let mut bad_seed = serde_json::to_value(&book).unwrap();
        bad_seed["receipt"]["tie_break_seed"] = serde_json::json!("12345");
        assert!(serde_json::from_value::<StockDistributionBook>(bad_seed).is_err());
    }

    #[test]
    fn holder_credit_lots_inherit_restriction_and_reject_mixed_sources() {
        let calendar = TradingCalendar::current_default_calendar().unwrap();
        let book = registered_book(&calendar);
        let receipt = book.receipt().unwrap().clone();
        let lots = holder_credit_lots(&receipt, date("2030-06-07")).unwrap();
        assert_eq!(lots.len(), 2);
        assert!(lots
            .iter()
            .all(|lot| lot.restriction == ShareRestriction::Unrestricted));
        assert_eq!(
            lots.iter().map(|lot| lot.qty).sum::<u64>(),
            receipt.approved_total_new_shares
        );
        assert!(matches!(
            holder_credit_lots(&receipt, date("2030-06-06")),
            Err(StockDistributionError::InvalidPlan { .. })
        ));

        let mut mixed = receipt.clone();
        mixed.holders.push(HolderDistribution {
            holder: HolderId::External("mixed-holder".into()),
            original_shares: 2,
            whole_shares: 1,
            fractional_numerator: 0,
            original_lots: vec![restricted_lot("r1", 1, "2030-08-01"), super::lot("u1", 1)],
        });
        assert!(matches!(
            holder_credit_lots(&mixed, date("2030-06-07")),
            Err(StockDistributionError::SourceLotAttributionConflict { .. })
        ));

        let mut restricted = receipt.clone();
        restricted.holders.push(HolderDistribution {
            holder: HolderId::External("restricted-holder".into()),
            original_shares: 2,
            whole_shares: 1,
            fractional_numerator: 0,
            original_lots: vec![
                restricted_lot("r1", 1, "2030-08-01"),
                restricted_lot("r2", 1, "2030-08-01"),
            ],
        });
        let lots = holder_credit_lots(&restricted, date("2030-06-07")).unwrap();
        let inherited = lots
            .iter()
            .find(|lot| lot.holder == HolderId::External("restricted-holder".into()))
            .unwrap();
        assert_eq!(
            inherited.restriction,
            ShareRestriction::Restricted {
                reason: "nonfloat-lock".into(),
                release_on: date("2030-08-01"),
            }
        );

        let mut matured = receipt.clone();
        matured.holders.push(HolderDistribution {
            holder: HolderId::External("matured-holder".into()),
            original_shares: 2,
            whole_shares: 1,
            fractional_numerator: 0,
            original_lots: vec![
                restricted_lot("r1", 1, "2030-06-01"),
                restricted_lot("r2", 1, "2030-06-01"),
            ],
        });
        let lots = holder_credit_lots(&matured, date("2030-06-07")).unwrap();
        let matured_lot = lots
            .iter()
            .find(|lot| lot.holder == HolderId::External("matured-holder".into()))
            .unwrap();
        assert_eq!(
            matured_lot.restriction,
            ShareRestriction::Unrestricted
        );
    }
}
