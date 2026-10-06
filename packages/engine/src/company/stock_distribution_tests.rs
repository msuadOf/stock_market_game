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
