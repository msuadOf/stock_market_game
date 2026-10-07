use super::*;

fn day(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}
fn account(id: u64) -> HolderId {
    HolderId::Account(AccountId(id))
}
fn lot(id: &str, qty: u64, acquired: &str) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on: day(acquired),
        source: AcquisitionSource::InitialAllocation {
            evidence: id.into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}
fn registry() -> ShareRegistry {
    ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId("issuer-A".into()),
        100,
        day("2030-01-01"),
        vec![
            ShareHolding {
                holder: account(1),
                lots: vec![lot("old", 30, "2028-01-01"), lot("new", 20, "2029-01-01")],
            },
            ShareHolding {
                holder: HolderId::External("nonfloat-owner".into()),
                lots: vec![lot("external", 40, "2028-01-01")],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![lot("treasury", 10, "2029-01-01")],
            },
        ],
    )
    .unwrap()
}
fn transfer(qty: i128) -> ShareDayRequest {
    ShareDayRequest {
        event_id: "day2".into(),
        day: day("2030-01-02"),
        scope: MovementScope::PublicMarket,
        changes: vec![
            DayNetChange {
                holder: account(1),
                change: -qty,
                acquisition: None,
            },
            DayNetChange {
                holder: account(u64::MAX),
                change: qty,
                acquisition: Some(NetAcquisition {
                    lot_id: "buyer".into(),
                    source: AcquisitionSource::SecondaryMarket {
                        settlement: "day2".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            },
        ],
    }
}

#[test]
fn net_disposal_consumes_oldest_lot_and_preserves_total() {
    let mut registry = registry();
    let receipt = registry.close_day(transfer(35)).unwrap();
    assert_eq!(receipt.disposals.len(), 2);
    assert_eq!(receipt.disposals[0].lot.id, "old");
    assert_eq!(receipt.disposals[0].lot.qty, 30);
    assert_eq!(receipt.disposals[1].lot.qty, 5);
    assert_eq!(registry.holdings()[0].lots[0].qty, 15);
    registry.validate().unwrap();
}

#[test]
fn insufficient_shares_is_atomic_and_retry_is_idempotent() {
    let mut registry = registry();
    let before = registry.clone();
    assert!(registry.close_day(transfer(51)).is_err());
    assert_eq!(registry, before);
    let request = transfer(5);
    let first = registry.close_day(request.clone()).unwrap();
    let settled = registry.clone();
    assert_eq!(registry.close_day(request).unwrap(), first);
    assert_eq!(registry, settled);
    assert!(registry.close_day(transfer(6)).is_err());
    assert_eq!(registry, settled);
}

#[test]
fn registered_entitlement_does_not_follow_later_sale_or_include_treasury() {
    let mut registry = registry();
    let snapshot = registry
        .register("dividend".into(), day("2030-01-01"))
        .unwrap()
        .clone();
    assert_eq!(snapshot.holdings().len(), 3);
    assert_eq!(snapshot.entitled_holdings().count(), 2);
    registry.close_day(transfer(50)).unwrap();
    assert_eq!(registry.registration("dividend"), Some(&snapshot));
    assert!(registry
        .register("backdated".into(), day("2030-01-01"))
        .is_err());
    assert_eq!(
        registry
            .register("dividend".into(), day("2030-01-01"))
            .unwrap(),
        &snapshot
    );
}

#[test]
fn restore_rejects_invalid_lot_dates_and_preserves_max_account_id_string() {
    let mut registry = registry();
    registry.close_day(transfer(1)).unwrap();
    let json = serde_json::to_value(&registry).unwrap();
    assert!(json.to_string().contains("\"18446744073709551615\""));
    let restored: ShareRegistry = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(restored, registry);
    let mut corrupt = json;
    corrupt["holdings"][0]["lots"][0]["acquired_on"] = serde_json::json!("2031-01-01");
    assert!(serde_json::from_value::<ShareRegistry>(corrupt).is_err());
}

#[test]
fn zero_net_day_does_not_reset_acquisition_and_duplicate_input_is_rejected() {
    let mut registry = registry();
    let holdings = registry.holdings().to_vec();
    registry
        .close_day(ShareDayRequest {
            event_id: "flat".into(),
            day: day("2030-01-02"),
            scope: MovementScope::PublicMarket,
            changes: vec![DayNetChange {
                holder: account(1),
                change: 0,
                acquisition: None,
            }],
        })
        .unwrap();
    assert_eq!(registry.holdings(), holdings);
    let before = registry.clone();
    let mut request = transfer(1);
    request.day = day("2030-01-03");
    request.changes.push(request.changes[0].clone());
    assert!(registry.close_day(request).is_err());
    assert_eq!(registry, before);
}

#[test]
fn restore_rejects_capital_receipt_and_snapshot_corruption() {
    let mut registry = registry();
    registry
        .register("dividend".into(), day("2030-01-01"))
        .unwrap();
    registry.close_day(transfer(5)).unwrap();
    let valid = serde_json::to_value(&registry).unwrap();
    for path in ["capital", "receipt", "snapshot", "date", "extra", "missing"] {
        let mut corrupt = valid.clone();
        match path {
            "capital" => corrupt["issued_shares"] = serde_json::json!(101),
            "receipt" => {
                corrupt["receipts"][0]["disposals"][0]["lot"]["qty"] = serde_json::json!(4)
            }
            "snapshot" => corrupt["registrations"][0]["issuer"] = serde_json::json!("wrong-issuer"),
            "date" => {
                corrupt["registrations"][0]["registered_on"] = serde_json::json!("2030-01-03")
            }
            "extra" => {
                corrupt
                    .as_object_mut()
                    .unwrap()
                    .insert("version".into(), serde_json::json!(3));
            }
            "missing" => {
                corrupt.as_object_mut().unwrap().remove("receipts");
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<ShareRegistry>(corrupt).is_err(),
            "accepted {path}"
        );
    }
}

#[test]
fn a_consumed_lot_identity_cannot_be_reused_in_the_same_day() {
    let mut registry = registry();
    let before = registry.clone();
    let mut request = transfer(50);
    request.changes[1].acquisition.as_mut().unwrap().lot_id = "old".into();
    assert!(registry.close_day(request).is_err());
    assert_eq!(registry, before);
}

#[test]
fn mixed_restricted_and_public_shares_allow_only_real_public_disposal() {
    let original = registry();
    let mut holdings = original.holdings().to_vec();
    holdings[0].lots[0].restriction = ShareRestriction::Restricted {
        reason: "explicit-opening-lock".into(),
        release_on: day("2031-01-01"),
    };
    let mut registry = ShareRegistry::new(
        original.stock().clone(),
        original.issuer().clone(),
        100,
        original.settled_on(),
        holdings,
    )
    .unwrap();
    let snapshot = registry
        .register("mixed-dividend".into(), day("2030-01-01"))
        .unwrap()
        .clone();
    assert_eq!(
        snapshot.holdings()[0]
            .lots
            .iter()
            .map(|lot| lot.qty)
            .sum::<u64>(),
        50
    );
    let receipt = registry.close_day(transfer(5)).unwrap();
    assert_eq!(receipt.disposals.len(), 1);
    assert_eq!(receipt.disposals[0].lot.id, "new");
    assert_eq!(registry.holdings()[0].lots[0].qty, 30);
    assert_eq!(registry.holdings()[0].lots[1].qty, 15);
    let mut excessive = transfer(16);
    excessive.event_id = "day3".into();
    excessive.day = day("2030-01-03");
    excessive.changes[1].acquisition.as_mut().unwrap().lot_id = "next-buyer".into();
    let before = registry.clone();
    assert!(registry.close_day(excessive).is_err());
    assert_eq!(registry, before);
}

#[test]
fn standalone_snapshot_deserialization_rejects_invalid_capital() {
    let mut registry = registry();
    let snapshot = registry
        .register("dividend".into(), day("2030-01-01"))
        .unwrap();
    let mut corrupt = serde_json::to_value(snapshot).unwrap();
    corrupt["issued_shares"] = serde_json::json!(101);
    assert!(serde_json::from_value::<RegistrationSnapshot>(corrupt).is_err());
}

#[test]
fn nullable_acquisition_is_required_and_nontrading_scope_is_not_ordinary_transfer() {
    let mut registry = registry();
    registry.close_day(transfer(5)).unwrap();
    let mut corrupt = serde_json::to_value(&registry).unwrap();
    corrupt["receipts"][0]["request"]["changes"][0]
        .as_object_mut()
        .unwrap()
        .remove("acquisition");
    assert!(serde_json::from_value::<ShareRegistry>(corrupt).is_err());
    let mut fresh = super::tests::registry();
    let before = fresh.clone();
    let mut request = transfer(5);
    request.scope = MovementScope::NonTradingTransfer {
        basis: "judicial-transfer-needs-separate-rule".into(),
    };
    assert!(matches!(
        fresh.close_day(request),
        Err(ShareRegistryError::UnsupportedMovementScope { .. })
    ));
    assert_eq!(fresh, before);
}

#[test]
fn public_market_cannot_disguise_initial_or_restricted_acquisition() {
    for kind in ["initial", "corporate", "restricted"] {
        let mut registry = registry();
        let before = registry.clone();
        let mut request = transfer(5);
        let acquisition = request.changes[1].acquisition.as_mut().unwrap();
        match kind {
            "initial" => {
                acquisition.source = AcquisitionSource::InitialAllocation {
                    evidence: "not-a-market-trade".into(),
                }
            }
            "corporate" => {
                acquisition.source = AcquisitionSource::CorporateAction {
                    event: "not-a-market-trade".into(),
                }
            }
            "restricted" => {
                acquisition.restriction = ShareRestriction::Restricted {
                    reason: "not-a-market-trade".into(),
                    release_on: day("2031-01-01"),
                }
            }
            _ => unreachable!(),
        }
        assert!(registry.close_day(request).is_err());
        assert_eq!(registry, before);
    }
}

#[test]
fn explicitly_released_physical_lot_is_transferable_and_receipt_cannot_forge_unlock() {
    let original = registry();
    let mut holdings = original.holdings().to_vec();
    holdings[0].lots[0].restriction = ShareRestriction::Restricted {
        reason: "explicit-opening-lock".into(),
        release_on: day("2030-01-02"),
    };
    let mut registry = ShareRegistry::new(
        original.stock().clone(),
        original.issuer().clone(),
        100,
        original.settled_on(),
        holdings,
    )
    .unwrap();
    let receipt = registry.close_day(transfer(5)).unwrap();
    assert_eq!(receipt.disposals[0].lot.id, "old");
    let mut corrupt = serde_json::to_value(&registry).unwrap();
    corrupt["receipts"][0]["disposals"][0]["lot"]["restriction"]["Restricted"]["release_on"] =
        serde_json::json!("2031-01-01");
    assert!(serde_json::from_value::<ShareRegistry>(corrupt).is_err());
}
