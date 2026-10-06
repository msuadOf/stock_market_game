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
    assert!(
        registry
            .register("backdated".into(), day("2030-01-01"))
            .is_err()
    );
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
fn share_quantities_and_signed_changes_use_exact_canonical_decimal_strings() {
    let quantity = 9_007_199_254_741_117u64;
    let capital = u64::MAX;
    let mut registry = ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId("issuer-A".into()),
        capital,
        day("2030-01-01"),
        vec![
            ShareHolding {
                holder: account(1),
                lots: vec![lot("large", quantity, "2029-01-01")],
            },
            ShareHolding {
                holder: HolderId::External("remainder".into()),
                lots: vec![lot("remainder-lot", capital - quantity, "2029-01-01")],
            },
        ],
    )
    .unwrap();
    let request = ShareDayRequest {
        event_id: "large-transfer".into(),
        day: day("2030-01-02"),
        scope: MovementScope::PublicMarket,
        changes: vec![
            DayNetChange {
                holder: account(1),
                change: -i128::from(quantity),
                acquisition: None,
            },
            DayNetChange {
                holder: account(2),
                change: quantity as i128,
                acquisition: Some(NetAcquisition {
                    lot_id: "large-buyer".into(),
                    source: AcquisitionSource::SecondaryMarket {
                        settlement: "large-transfer".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            },
        ],
    };
    let serialized_request = serde_json::to_value(&request).unwrap();
    assert_eq!(
        serialized_request["changes"][0]["change"],
        format!("-{quantity}")
    );
    assert_eq!(
        serialized_request["changes"][1]["change"],
        quantity.to_string()
    );
    assert_eq!(
        serde_json::from_value::<ShareDayRequest>(serialized_request.clone()).unwrap(),
        request
    );
    registry.close_day(request).unwrap();

    let json = serde_json::to_value(&registry).unwrap();
    assert_eq!(json["issued_shares"], capital.to_string());
    assert_eq!(json["holdings"][0]["lots"], serde_json::json!([]));
    let buyer = json["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|holding| holding["holder"] == serde_json::json!({"Account": "2"}))
        .unwrap();
    assert_eq!(buyer["lots"][0]["qty"], quantity.to_string());
    assert_eq!(
        json["receipts"][0]["disposals"][0]["lot"]["qty"],
        quantity.to_string()
    );
    let restored: ShareRegistry = serde_json::from_value(json).unwrap();
    assert_eq!(restored, registry);
}

#[test]
fn share_registry_wire_rejects_numeric_and_noncanonical_share_counts() {
    let registry = registry();
    let valid = serde_json::to_value(registry).unwrap();
    for invalid in [
        serde_json::json!(100),
        serde_json::json!("0100"),
        serde_json::json!("+100"),
    ] {
        let mut corrupt = valid.clone();
        corrupt["issued_shares"] = invalid;
        assert!(serde_json::from_value::<ShareRegistry>(corrupt).is_err());
    }

    let mut lot_numeric = valid.clone();
    lot_numeric["holdings"][0]["lots"][0]["qty"] = serde_json::json!(30);
    assert!(serde_json::from_value::<ShareRegistry>(lot_numeric).is_err());

    let mut request = serde_json::to_value(transfer(5)).unwrap();
    request["changes"][0]["change"] = serde_json::json!(-5);
    assert!(serde_json::from_value::<ShareDayRequest>(request).is_err());
}

#[test]
fn signed_share_change_codec_roundtrips_i128_bounds_and_rejects_invalid_values() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct SignedValue {
        #[serde(with = "super::canonical_i128_decimal")]
        value: i128,
    }

    for value in [i128::MIN, i128::MAX] {
        let encoded = serde_json::to_string(&SignedValue { value }).unwrap();
        assert_eq!(encoded, format!("{{\"value\":\"{value}\"}}"));
        assert_eq!(
            serde_json::from_str::<SignedValue>(&encoded).unwrap().value,
            value
        );
    }

    for invalid in [
        "\"-0\"",
        "\"-170141183460469231731687303715884105729\"",
        "\"170141183460469231731687303715884105728\"",
    ] {
        let encoded = format!("{{\"value\":{invalid}}}");
        assert!(serde_json::from_str::<SignedValue>(&encoded).is_err());
    }
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
fn registry_restore_requires_nullable_issuer_repurchase_account_key() {
    let registry = registry();
    let mut wire = serde_json::to_value(&registry).unwrap();
    wire.as_object_mut()
        .unwrap()
        .remove("issuer_repurchase_account");

    assert!(serde_json::from_value::<ShareRegistry>(wire).is_err());
}

#[test]
fn snapshot_restore_requires_nullable_issuer_repurchase_account_key() {
    let mut registry = registry();
    let snapshot = registry
        .register("repurchase-account-facts".into(), day("2030-01-01"))
        .unwrap();
    let mut wire = serde_json::to_value(snapshot).unwrap();
    wire.as_object_mut()
        .unwrap()
        .remove("issuer_repurchase_account");

    assert!(serde_json::from_value::<RegistrationSnapshot>(wire).is_err());
}

#[test]
fn issuer_repurchase_account_fact_is_validated_and_copied_into_registration() {
    let mut registry = registry();
    let facts = IssuerRepurchaseAccountFacts {
        account_reference: "repurchase-account-1".into(),
        source_evidence: "exchange-confirmation-1".into(),
        established_on: day("2029-12-31"),
    };
    registry
        .set_issuer_repurchase_account(facts.clone())
        .unwrap();
    let snapshot = registry
        .register("repurchase-facts".into(), day("2030-01-01"))
        .unwrap()
        .clone();

    assert_eq!(snapshot.issuer_repurchase_account(), Some(&facts));
    registry
        .set_issuer_repurchase_account(facts.clone())
        .unwrap();
    let registry_roundtrip = serde_json::from_value::<ShareRegistry>(
        serde_json::to_value(&registry).unwrap(),
    )
    .unwrap();
    assert_eq!(registry_roundtrip, registry);
    let snapshot_roundtrip = serde_json::from_value::<RegistrationSnapshot>(
        serde_json::to_value(&snapshot).unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot_roundtrip, snapshot);

    let mut no_treasury_holding = ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId("issuer-A".into()),
        100,
        day("2030-01-01"),
        vec![ShareHolding {
            holder: account(1),
            lots: vec![lot("only-holder", 100, "2029-01-01")],
        }],
    )
    .unwrap();
    no_treasury_holding
        .set_issuer_repurchase_account(facts)
        .unwrap();
    assert!(no_treasury_holding
        .issuer_repurchase_account()
        .is_some());
}

#[test]
fn issuer_repurchase_account_fact_cannot_be_future_dated_or_empty() {
    let mut registry = registry();
    for facts in [
        IssuerRepurchaseAccountFacts {
            account_reference: "repurchase-account-1".into(),
            source_evidence: "exchange-confirmation-1".into(),
            established_on: day("2030-01-02"),
        },
        IssuerRepurchaseAccountFacts {
            account_reference: " ".into(),
            source_evidence: "exchange-confirmation-1".into(),
            established_on: day("2029-12-31"),
        },
    ] {
        assert!(registry
            .set_issuer_repurchase_account(facts)
            .is_err());
    }
}

#[test]
fn issuer_repurchase_account_facts_cannot_be_backdated_across_registration() {
    let mut registry = registry();
    registry
        .register("before-facts".into(), day("2030-01-01"))
        .unwrap();
    let before = registry.clone();
    assert!(registry
        .set_issuer_repurchase_account(IssuerRepurchaseAccountFacts {
            account_reference: "repurchase-account-1".into(),
            source_evidence: "exchange-confirmation-1".into(),
            established_on: day("2029-12-31"),
        })
        .is_err());
    assert_eq!(registry, before);
}

#[test]
fn registry_restore_rejects_snapshot_repurchase_facts_that_diverge_from_history() {
    let mut registry = registry();
    let snapshot = registry
        .register("before-facts".into(), day("2030-01-01"))
        .unwrap()
        .clone();
    registry.close_day(transfer(1)).unwrap();
    assert!(registry
        .set_issuer_repurchase_account(IssuerRepurchaseAccountFacts {
            account_reference: "repurchase-account-1".into(),
            source_evidence: "exchange-confirmation-1".into(),
            established_on: day("2030-01-02"),
        })
        .is_ok());
    let later_snapshot = registry
        .register("after-facts".into(), day("2030-01-02"))
        .unwrap()
        .clone();
    assert_eq!(
        later_snapshot.issuer_repurchase_account(),
        registry.issuer_repurchase_account()
    );
    let mut wire = serde_json::to_value(&registry).unwrap();
    assert_eq!(
        wire["registrations"][0]["issuer_repurchase_account"],
        serde_json::Value::Null
    );
    assert!(wire["registrations"][1]["issuer_repurchase_account"].is_object());
    wire["registrations"][0]["issuer_repurchase_account"] = serde_json::json!({
        "account_reference": "forged-account",
        "source_evidence": "forged-evidence",
        "established_on": "2030-01-01"
    });
    assert!(serde_json::from_value::<ShareRegistry>(wire).is_err());

    let mut wire = serde_json::to_value(&registry).unwrap();
    wire["registrations"][1]["issuer_repurchase_account"]["account_reference"] =
        serde_json::json!("different-account");
    assert!(serde_json::from_value::<ShareRegistry>(wire).is_err());
    assert!(snapshot.issuer_repurchase_account().is_none());
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
    // 非交易过户不得伪装普通二级市场过户：来源必须是公司行为。
    assert!(fresh.close_day(request).is_err());
    assert_eq!(fresh, before);
}

fn issuance_request(day_text: &str, event_id: &str) -> ShareDayRequest {
    ShareDayRequest {
        event_id: event_id.into(),
        day: day(day_text),
        scope: MovementScope::NonTradingTransfer {
            basis: "shareholders-resolution-1".into(),
        },
        changes: vec![
            DayNetChange {
                holder: account(1),
                change: 30,
                acquisition: Some(NetAcquisition {
                    lot_id: "bonus-account-1".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "distribution-1".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            },
            DayNetChange {
                holder: HolderId::External("nonfloat-owner".into()),
                change: 20,
                acquisition: Some(NetAcquisition {
                    lot_id: "bonus-external".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "distribution-1".into(),
                    },
                    restriction: ShareRestriction::Restricted {
                        reason: "nonfloat-lock".into(),
                        release_on: day("2030-06-01"),
                    },
                }),
            },
        ],
    }
}

#[test]
fn non_trading_transfer_issues_new_shares_on_the_settled_day_and_grows_issued_shares() {
    let mut registry = registry();
    registry.close_day(transfer(5)).unwrap();
    let settled = registry.settled_on();
    let receipt = registry.close_day(issuance_request("2030-01-02", "issue-1")).unwrap();
    assert!(receipt.disposals.is_empty());
    assert_eq!(registry.settled_on(), settled);
    assert_eq!(registry.issued_shares(), 150);
    // 幂等：同一事件身份重复提交同一请求直接返回原回执。
    assert_eq!(
        registry
            .close_day(issuance_request("2030-01-02", "issue-1"))
            .unwrap()
            .request,
        receipt.request
    );
    assert_eq!(registry.issued_shares(), 150);
    // 公开市场次日继续推进，恢复仍严格合法。
    registry
        .close_day(ShareDayRequest {
            event_id: "day3-empty".into(),
            day: day("2030-01-03"),
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    let mut restored = serde_json::from_value::<ShareRegistry>(
        serde_json::to_value(&registry).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.issued_shares(), 150);
    restored
        .register("record-after-issue".into(), restored.settled_on())
        .unwrap();
    assert!(restored.validate().is_ok());
}

#[test]
fn non_trading_transfer_rejects_disposal_secondary_source_and_zero_total() {
    let mut registry = registry();
    registry.close_day(transfer(5)).unwrap();
    let before = registry.clone();
    let mut disposal = issuance_request("2030-01-02", "issue-disposal");
    disposal.changes.push(DayNetChange {
        holder: account(1),
        change: -1,
        acquisition: None,
    });
    assert!(registry.close_day(disposal).is_err());
    let mut secondary = issuance_request("2030-01-02", "issue-secondary");
    if let Some(acquisition) = secondary.changes[0].acquisition.as_mut() {
        acquisition.source = AcquisitionSource::SecondaryMarket {
            settlement: "not-a-corporate-action".into(),
        };
    }
    assert!(registry.close_day(secondary).is_err());
    let mut zero = issuance_request("2030-01-02", "issue-zero");
    zero.changes.clear();
    assert!(registry.close_day(zero).is_err());
    // 非交易过户不能凭空前移到已结算日前一天，也不能跳到未来自然日。
    assert!(registry
        .close_day(issuance_request("2030-01-01", "issue-past"))
        .is_err());
    assert!(registry
        .close_day(issuance_request("2030-01-03", "issue-future"))
        .is_err());
    assert_eq!(registry, before);
}

#[test]
fn registration_snapshots_replay_issued_shares_before_later_non_trading_issuance() {
    let mut registry = registry();
    registry
        .register("record-1".into(), day("2030-01-01"))
        .unwrap();
    registry.close_day(transfer(5)).unwrap();
    // 同日"先登记、后送转入账"：快照冻结在增发前的发行股数上。
    registry
        .register("record-2".into(), day("2030-01-02"))
        .unwrap();
    assert_eq!(
        registry.registration("record-2").unwrap().issued_shares(),
        100
    );
    assert_eq!(registry.registration("record-2").unwrap().settled_receipts(), 1);
    registry.close_day(issuance_request("2030-01-02", "issue-1")).unwrap();
    // 同日"先送转入账、再登记"（另一事件）同样合法，快照冻结在增发后。
    registry
        .register("record-3".into(), day("2030-01-02"))
        .unwrap();
    assert_eq!(
        registry.registration("record-3").unwrap().issued_shares(),
        150
    );
    assert_eq!(registry.registration("record-3").unwrap().settled_receipts(), 2);
    registry
        .close_day(ShareDayRequest {
            event_id: "day3-empty".into(),
            day: day("2030-01-03"),
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    assert!(registry.validate().is_ok());
    assert_eq!(
        registry.registration("record-1").unwrap().issued_shares(),
        100
    );
    let mut corrupt = serde_json::to_value(&registry).unwrap();
    corrupt["registrations"][1]["settled_receipts"] = serde_json::json!("2");
    assert!(serde_json::from_value::<ShareRegistry>(corrupt).is_err());
    let mut forged = serde_json::to_value(&registry).unwrap();
    forged["registrations"][1]["issued_shares"] = serde_json::json!("150");
    forged["registrations"][1]["holdings"][0]["lots"][0]["qty"] = serde_json::json!("80");
    assert!(serde_json::from_value::<ShareRegistry>(forged).is_err());
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
