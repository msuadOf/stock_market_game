use super::*;

fn day(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn holder(id: u64) -> HolderId {
    HolderId::Account(AccountId(id))
}

fn foundation(on: &str) -> ShareRegistry {
    foundation_shares(on, 100)
}

fn foundation_shares(on: &str, quantity: u64) -> ShareRegistry {
    let mut registry = ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId(0),
        quantity,
        day(on),
        vec![ShareHolding {
            holder: holder(1),
            lots: vec![ShareLot {
                id: "original".into(),
                qty: quantity,
                acquired_on: day("2024-01-02"),
                source: AcquisitionSource::InitialAllocation {
                    evidence: "开局原始登记事实".into(),
                },
                restriction: ShareRestriction::Unrestricted,
            }],
        }],
    )
    .unwrap();
    registry.register("R".into(), day(on)).unwrap();
    registry
}

fn rows(on: &str, first_qty: u64, closing_qty: u64) -> Vec<ShareMarketConfirmation> {
    // 条件式 domain fixture：手写归一确认仅验证给定事实的算术，不证明真实撮合或申报合法。
    // 普通与 closing 来源由未来受控 producer 合并；不从 receipt_id 推断 phase。
    [
        (0, Side::Sell, first_qty),
        (1, Side::Buy, first_qty),
        (2, Side::Sell, closing_qty),
        (3, Side::Buy, closing_qty),
    ]
    .into_iter()
    .filter(|(_, _, qty)| *qty != 0)
    .map(|(receipt_id, side, qty)| ShareMarketConfirmation {
        account: if side == Side::Sell {
            AccountId(1)
        } else {
            AccountId(2)
        },
        receipt_id,
        stock: StockCode("600001".into()),
        day: day(on),
        side,
        qty,
    })
    .collect()
}

fn context<'a>(
    calendar: &'a TradingCalendar,
    rows: &'a [ShareMarketConfirmation],
) -> ShareMarketSourceContext<'a> {
    ShareMarketSourceContext {
        calendar,
        exchange: CalendarExchange::Sse,
        confirmations: rows,
        treasury_account: None,
        completed_through: Some(day("2026-12-31")),
    }
}

fn market(
    registry: &ShareRegistry,
    on: &str,
    qty: u64,
    rows: &[ShareMarketConfirmation],
) -> ShareSettlementDay {
    ShareSettlementDay {
        stock: registry.stock().clone(),
        day: day(on),
        predecessor: registry.settlement_frontier().unwrap(),
        opening: vec![],
        actual_market_close: ConfirmedShareMarketClose {
            unit: registry.settlement_frontier().unwrap().unit,
            source: ShareMarketSource::FinalConfirmations {
                receipts: rows
                    .iter()
                    .map(|row| ShareConfirmationRef {
                        account: row.account,
                        receipt_id: row.receipt_id,
                    })
                    .collect(),
            },
            request: ShareDayRequest {
                event_id: format!("market:{on}"),
                day: day(on),
                scope: MovementScope::PublicMarket,
                changes: if qty == 0 {
                    registry
                        .holdings()
                        .iter()
                        .map(|holding| DayNetChange {
                            holder: holding.holder.clone(),
                            change: 0,
                            acquisition: None,
                            consume: None,
                        })
                        .collect()
                } else {
                    vec![
                        DayNetChange {
                            holder: holder(1),
                            change: -i128::from(qty),
                            acquisition: None,
                            consume: None,
                        },
                        DayNetChange {
                            holder: holder(2),
                            change: i128::from(qty),
                            acquisition: Some(NetAcquisition {
                                lot_id: format!("market-buy:{on}"),
                                source: AcquisitionSource::SecondaryMarket {
                                    settlement: format!("market:{on}"),
                                },
                                restriction: ShareRestriction::Unrestricted,
                            }),
                            consume: None,
                        },
                    ]
                },
            },
        },
        post_market_actions: vec![],
    }
}

fn halve(
    registry: &ShareRegistry,
    request: &mut ShareSettlementDay,
    registration: &str,
    event: &str,
) {
    let snapshot = registry.registration(registration).unwrap();
    let mut source_lots = vec![];
    let mut changes = vec![];
    for holding in snapshot.holdings() {
        for lot in &holding.lots {
            assert_eq!(lot.qty % 2, 0, "首片 fixture 仅验证已冻结整数换算");
            source_lots.push(vec![lot.id.clone()]);
            changes.push(DayNetChange {
                holder: holding.holder.clone(),
                change: -i128::from(lot.qty / 2),
                acquisition: None,
                consume: Some(vec![LotConsumption {
                    lot_id: lot.id.clone(),
                    qty: lot.qty / 2,
                }]),
            });
        }
    }
    let to_unit = ShareUnitIdentity::ReDenominated {
        movement_event_id: event.into(),
    };
    request.opening.push(FrozenOpeningMovement {
        reference: OpeningMovementRef {
            movement_event_id: event.into(),
            registration_event_id: registration.into(),
            source_lots,
            from_unit: request.predecessor.unit.clone(),
            to_unit: to_unit.clone(),
        },
        request: ShareDayRequest {
            event_id: event.into(),
            day: request.day,
            scope: MovementScope::ShareReDenomination {
                basis: "已冻结无碎股整数换算".into(),
            },
            changes,
        },
    });
    request.actual_market_close.unit = to_unit;
}

#[test]
fn opening_conversion_precedes_given_final_new_unit_confirmation_facts() {
    let registry = foundation_shares("2026-01-05", 1000);
    let original = registry.clone();
    let rows = rows("2026-01-06", 100, 100);
    let rows_before = rows.clone();
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut request = market(&registry, "2026-01-06", 200, &rows);
    halve(&registry, &mut request, "R", "halve-1");
    let request_before = request.clone();
    let candidate = registry
        .prepare_settlement_day(&request, &context(&calendar, &rows))
        .unwrap();
    assert_eq!(candidate.issued_shares(), 500);
    assert_eq!(candidate.holdings()[0].lots[0].qty, 300);
    assert_eq!(candidate.holdings()[1].lots[0].qty, 200);
    assert_eq!(candidate.receipts().len(), 2);
    assert_eq!(candidate.receipts()[0].disposals[0].lot.qty, 500);
    assert_eq!(candidate.receipts()[1].disposals[0].lot.qty, 200);
    assert_eq!(candidate.settled_on(), day("2026-01-06"));
    assert_eq!(registry, original);
    assert_eq!(request, request_before);
    assert_eq!(rows, rows_before);
    let restored: ShareRegistry =
        serde_json::from_value(serde_json::to_value(&candidate).unwrap()).unwrap();
    assert_eq!(restored, candidate);
    assert_eq!(
        candidate
            .prepare_settlement_day(&request, &context(&calendar, &rows))
            .unwrap(),
        candidate
    );
}

#[test]
fn ordered_captures_keep_exact_100_then_150_receipt_frontiers() {
    let registry = foundation("2026-01-05");
    let rows = rows("2026-01-06", 6, 4);
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut request = market(&registry, "2026-01-06", 10, &rows);
    request.post_market_actions = vec![
        SharePostAction::CaptureRegistration {
            event_id: "A".into(),
        },
        SharePostAction::Movement(ShareDayRequest {
            event_id: "credit-50".into(),
            day: request.day,
            scope: MovementScope::NonTradingTransfer {
                basis: "已冻结送转到账".into(),
            },
            changes: vec![DayNetChange {
                holder: holder(1),
                change: 50,
                consume: None,
                acquisition: Some(NetAcquisition {
                    lot_id: "corporate-50".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "credit-50".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            }],
        }),
        SharePostAction::CaptureRegistration {
            event_id: "B".into(),
        },
    ];
    let candidate = registry
        .prepare_settlement_day(&request, &context(&calendar, &rows))
        .unwrap();
    for (event, capital, frontier) in [("A", 100, 1), ("B", 150, 2)] {
        let snapshot = candidate.registration(event).unwrap();
        assert_eq!(snapshot.issued_shares(), capital);
        assert_eq!(snapshot.settled_receipts(), frontier);
    }
    assert_eq!(
        candidate.registration("A").unwrap().holdings()[0].lots[0].qty,
        90
    );
    assert_eq!(
        candidate.registration("B").unwrap().holdings()[0]
            .lots
            .len(),
        2
    );
    let restored: ShareRegistry =
        serde_json::from_value(serde_json::to_value(&candidate).unwrap()).unwrap();
    assert_eq!(restored, candidate);
    let mut corrupt = candidate.clone();
    corrupt
        .registrations
        .iter_mut()
        .find(|s| s.event_id == "A")
        .unwrap()
        .settled_receipts = 0;
    assert!(corrupt.validate().is_err());
}

#[test]
fn friday_registration_survives_calendar_checked_weekend_zero_then_monday_opening() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut registry = foundation("2026-01-09");
    for on in ["2026-01-10", "2026-01-11"] {
        let mut request = market(&registry, on, 0, &[]);
        assert!(!request.actual_market_close.request.changes.is_empty());
        request.actual_market_close.source = ShareMarketSource::CalendarClosed {
            exchange: CalendarExchange::Sse,
            policy_digest: calendar.policy().content_digest().into(),
        };
        registry = registry
            .prepare_settlement_day(&request, &context(&calendar, &[]))
            .unwrap();
    }
    let rows = rows("2026-01-12", 6, 4);
    let mut monday = market(&registry, "2026-01-12", 10, &rows);
    halve(&registry, &mut monday, "R", "monday-halving");
    let candidate = registry
        .prepare_settlement_day(&monday, &context(&calendar, &rows))
        .unwrap();
    assert_eq!(candidate.receipts().len(), 4);
    assert_eq!(candidate.holdings()[0].lots[0].qty, 40);
    assert_eq!(candidate.registration("R").unwrap().issued_shares(), 100);
}

#[test]
fn old_unit_market_and_changed_complete_retry_are_rejected_atomically() {
    let registry = foundation("2026-01-05");
    let before = registry.clone();
    let rows = rows("2026-01-06", 6, 4);
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut request = market(&registry, "2026-01-06", 10, &rows);
    halve(&registry, &mut request, "R", "halve-1");
    let candidate = registry
        .prepare_settlement_day(&request, &context(&calendar, &rows))
        .unwrap();
    let mut wrong_unit = request.clone();
    wrong_unit.actual_market_close.unit = ShareUnitIdentity::Initial;
    assert!(registry
        .prepare_settlement_day(&wrong_unit, &context(&calendar, &rows))
        .is_err());
    assert_eq!(registry, before);
    let mut changed = request.clone();
    changed
        .post_market_actions
        .push(SharePostAction::CaptureRegistration {
            event_id: "late".into(),
        });
    assert!(candidate
        .prepare_settlement_day(&changed, &context(&calendar, &rows))
        .is_err());
    let mut omitted = request;
    if let ShareMarketSource::FinalConfirmations { receipts } =
        &mut omitted.actual_market_close.source
    {
        receipts.pop();
    }
    assert!(candidate
        .prepare_settlement_day(&omitted, &context(&calendar, &rows))
        .is_err());
}

#[test]
fn grouped_original_lots_keep_one_positive_acquisition_and_existing_release_metadata() {
    for (release, expected_restriction) in [
        (
            "2026-01-06",
            ShareRestriction::Restricted {
                reason: "原冻结限制".into(),
                release_on: day("2026-01-06"),
            },
        ),
        ("2026-01-05", ShareRestriction::Unrestricted),
    ] {
        let restriction = ShareRestriction::Restricted {
            reason: "原冻结限制".into(),
            release_on: day(release),
        };
        let mut registry = foundation_shares("2026-01-05", 50);
        registry.registrations.clear();
        registry.holdings[0].lots[0].qty = 30;
        registry.holdings[0].lots[0].restriction = restriction.clone();
        let mut second = registry.holdings[0].lots[0].clone();
        second.id = "second-source".into();
        second.qty = 20;
        second.acquired_on = day("2025-01-02");
        registry.holdings[0].lots.push(second);
        registry
            .register("R".into(), registry.settled_on())
            .unwrap();
        let mut request = market(&registry, "2026-01-06", 0, &[]);
        let to_unit = ShareUnitIdentity::ReDenominated {
            movement_event_id: "grouped-split".into(),
        };
        request.opening = vec![FrozenOpeningMovement {
            reference: OpeningMovementRef {
                movement_event_id: "grouped-split".into(),
                registration_event_id: "R".into(),
                source_lots: vec![vec!["original".into(), "second-source".into()]],
                from_unit: ShareUnitIdentity::Initial,
                to_unit: to_unit.clone(),
            },
            request: ShareDayRequest {
                event_id: "grouped-split".into(),
                day: request.day,
                scope: MovementScope::ShareReDenomination {
                    basis: "原聚合正向冻结事实".into(),
                },
                changes: vec![DayNetChange {
                    holder: holder(1),
                    change: 50,
                    consume: None,
                    acquisition: Some(NetAcquisition {
                        lot_id: "single-new-50".into(),
                        source: AcquisitionSource::CorporateAction {
                            event: "grouped-split".into(),
                        },
                        restriction: expected_restriction.clone(),
                    }),
                }],
            },
        }];
        request.actual_market_close.unit = to_unit;
        let calendar = TradingCalendar::current_default_calendar().unwrap();
        let candidate = registry
            .prepare_settlement_day(&request, &context(&calendar, &[]))
            .unwrap();
        assert_eq!(candidate.issued_shares(), 100);
        assert_eq!(candidate.holdings()[0].lots.len(), 3);
        assert_eq!(candidate.holdings()[0].lots[2].qty, 50);
        assert_eq!(
            candidate.holdings()[0].lots[2].restriction,
            expected_restriction
        );
        assert_eq!(candidate.holdings()[0].lots[0].qty, 30);
        assert_eq!(candidate.holdings()[0].lots[1].qty, 20);
    }
}

#[test]
fn empty_confirmation_list_requires_completed_market_context_and_closed_policy_is_frozen() {
    let registry = foundation("2026-01-09");
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut request = market(&registry, "2026-01-10", 0, &[]);
    request.actual_market_close.source = ShareMarketSource::CalendarClosed {
        exchange: CalendarExchange::Sse,
        policy_digest: calendar.policy().content_digest().into(),
    };
    let candidate = registry
        .prepare_settlement_day(&request, &context(&calendar, &[]))
        .unwrap();
    assert_eq!(candidate.issued_shares(), 100);
    let mut incomplete = context(&calendar, &[]);
    incomplete.completed_through = None;
    assert!(registry
        .prepare_settlement_day(&request, &incomplete)
        .is_err());
    incomplete.completed_through = Some(day("2026-01-09"));
    assert!(registry
        .prepare_settlement_day(&request, &incomplete)
        .is_err());
    let mut wrong_policy = request.clone();
    if let ShareMarketSource::CalendarClosed { policy_digest, .. } =
        &mut wrong_policy.actual_market_close.source
    {
        *policy_digest = "未冻结的其他政策".into();
    }
    assert!(registry
        .prepare_settlement_day(&wrong_policy, &context(&calendar, &[]))
        .is_err());
    let mut fake_trading = request;
    fake_trading.actual_market_close.source =
        ShareMarketSource::FinalConfirmations { receipts: vec![] };
    assert!(registry
        .prepare_settlement_day(&fake_trading, &context(&calendar, &[]))
        .is_err());
}

#[test]
fn second_natural_day_conversion_after_json_restore_preserves_complete_unit_chain() {
    let registry = foundation_shares("2026-01-05", 1000);
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let first_rows = rows("2026-01-06", 100, 100);
    let mut first = market(&registry, "2026-01-06", 200, &first_rows);
    halve(&registry, &mut first, "R", "first-unit");
    first
        .post_market_actions
        .push(SharePostAction::CaptureRegistration {
            event_id: "R2".into(),
        });
    let first_result = registry
        .prepare_settlement_day(&first, &context(&calendar, &first_rows))
        .unwrap();
    let restored: ShareRegistry =
        serde_json::from_value(serde_json::to_value(&first_result).unwrap()).unwrap();
    let mut next_rows = rows("2026-01-07", 100, 0);
    for row in &mut next_rows {
        row.receipt_id += 4;
    }
    let all_rows: Vec<_> = first_rows
        .into_iter()
        .chain(next_rows.iter().cloned())
        .collect();
    let mut second = market(&restored, "2026-01-07", 100, &next_rows);
    halve(&restored, &mut second, "R2", "second-unit");
    let candidate = restored
        .prepare_settlement_day(&second, &context(&calendar, &all_rows))
        .unwrap();
    assert_eq!(candidate.issued_shares(), 250);
    assert_eq!(candidate.holdings()[0].lots[0].qty, 50);
    assert_eq!(candidate.receipts().len(), 4);
    assert_eq!(
        candidate.settlement_frontier().unwrap().unit,
        ShareUnitIdentity::ReDenominated {
            movement_event_id: "second-unit".into()
        }
    );
    assert_eq!(
        first_result
            .prepare_settlement_day(&second, &context(&calendar, &all_rows))
            .unwrap(),
        candidate
    );
    assert_eq!(
        candidate
            .prepare_settlement_day(&second, &context(&calendar, &all_rows))
            .unwrap(),
        candidate
    );
    assert_eq!(
        candidate
            .prepare_settlement_day(&first, &context(&calendar, &all_rows))
            .unwrap(),
        candidate
    );
    // 重试保留 opening A 的 reference，却把完整 movement 偷换成后来合法 B。
    let mut substituted = first.clone();
    substituted.opening[0].request = second.opening[0].request.clone();
    assert!(candidate
        .prepare_settlement_day(&substituted, &context(&calendar, &all_rows))
        .is_err());
}

#[test]
fn opening_multi_lot_conversion_does_not_convert_later_market_acquisition() {
    let mut registry = foundation_shares("2026-01-05", 1000);
    registry.registrations.clear();
    registry.holdings[0].lots[0].qty = 500;
    let mut second_lot = registry.holdings[0].lots[0].clone();
    second_lot.id = "second-original".into();
    second_lot.qty = 500;
    second_lot.acquired_on = day("2025-01-02");
    registry.holdings[0].lots.push(second_lot);
    let mut seller_lot = registry.holdings[0].lots[0].clone();
    seller_lot.id = "seller-original".into();
    seller_lot.qty = 400;
    registry.holdings.push(ShareHolding {
        holder: holder(2),
        lots: vec![seller_lot],
    });
    registry.issued_shares = 1400;
    registry
        .register("R".into(), registry.settled_on())
        .unwrap();
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut first_rows = rows("2026-01-06", 100, 0);
    for row in &mut first_rows {
        row.account = if row.side == Side::Buy {
            AccountId(1)
        } else {
            AccountId(2)
        };
    }
    let mut first = market(&registry, "2026-01-06", 100, &first_rows);
    first.actual_market_close.request.changes[0].holder = holder(2);
    first.actual_market_close.request.changes[1].holder = holder(1);
    halve(&registry, &mut first, "R", "multi-unit");
    let candidate = registry
        .prepare_settlement_day(&first, &context(&calendar, &first_rows))
        .unwrap();
    assert_eq!(
        candidate.holdings()[0]
            .lots
            .iter()
            .map(|lot| lot.qty)
            .collect::<Vec<_>>(),
        vec![250, 250, 100]
    );
    let mut second_rows = rows("2026-01-07", 100, 200);
    for row in &mut second_rows {
        row.receipt_id += 4;
    }
    let all_rows: Vec<_> = first_rows
        .into_iter()
        .chain(second_rows.iter().cloned())
        .collect();
    let second = market(&candidate, "2026-01-07", 300, &second_rows);
    let sold = candidate
        .prepare_settlement_day(&second, &context(&calendar, &all_rows))
        .unwrap();
    let disposal = &sold.receipts().last().unwrap().disposals;
    assert_eq!(
        disposal
            .iter()
            .map(|item| (item.lot.id.as_str(), item.lot.qty))
            .collect::<Vec<_>>(),
        vec![("original", 250), ("second-original", 50)]
    );
    assert_eq!(sold.holdings()[0].lots.last().unwrap().qty, 100);
    assert_eq!(sold.issued_shares(), 700);
}

#[test]
fn treasury_has_explicit_source_bucket_and_actual_confirmation_account_mapping() {
    let mut registry = foundation_shares("2026-01-05", 1000);
    registry.registrations.clear();
    let mut treasury = registry.holdings[0].lots[0].clone();
    treasury.id = "treasury-source".into();
    treasury.qty = 400;
    registry.holdings.push(ShareHolding {
        holder: HolderId::IssuerTreasury,
        lots: vec![treasury],
    });
    registry.issued_shares = 1400;
    registry
        .register("R".into(), registry.settled_on())
        .unwrap();
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut rows = rows("2026-01-06", 100, 0);
    rows.iter_mut()
        .filter(|row| row.side == Side::Buy)
        .for_each(|row| row.account = AccountId(99));
    let mut request = market(&registry, "2026-01-06", 100, &rows);
    request.actual_market_close.request.changes[1].holder = HolderId::IssuerTreasury;
    halve(&registry, &mut request, "R", "treasury-unit");
    let mut mapped = context(&calendar, &rows);
    mapped.treasury_account = Some(AccountId(99));
    let candidate = registry.prepare_settlement_day(&request, &mapped).unwrap();
    assert_eq!(candidate.issued_shares(), 700);
    assert_eq!(
        candidate.holdings()[1]
            .lots
            .iter()
            .map(|lot| lot.qty)
            .sum::<u64>(),
        300
    );
    assert!(registry
        .prepare_settlement_day(&request, &context(&calendar, &rows))
        .is_err());
}

#[test]
fn standalone_marker_rejects_duplicate_confirmation_refs_before_external_lookup() {
    let registry = foundation_shares("2026-01-05", 1000);
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let rows = rows("2026-01-06", 100, 0);
    let request = market(&registry, "2026-01-06", 100, &rows);
    let candidate = registry
        .prepare_settlement_day(&request, &context(&calendar, &rows))
        .unwrap();
    let mut corrupt = candidate.clone();
    let MovementScope::SettlementDayMarket { binding } = &mut corrupt.receipts[0].request.scope
    else {
        panic!("typed marker必须保留");
    };
    let ShareMarketSource::FinalConfirmations { receipts } = &mut binding.market_source else {
        panic!("确认来源必须保留");
    };
    receipts.push(receipts[0].clone());
    assert!(corrupt.validate().is_err());
    assert!(
        serde_json::from_value::<ShareRegistry>(serde_json::to_value(&corrupt).unwrap()).is_err()
    );
}

#[test]
fn checked_opening_overflow_and_wrong_phase_leave_registry_and_request_unchanged() {
    let registry = foundation_shares("2026-01-05", u64::MAX);
    let before = registry.clone();
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut request = market(&registry, "2026-01-06", 0, &[]);
    request.opening = vec![FrozenOpeningMovement {
        reference: OpeningMovementRef {
            movement_event_id: "overflow".into(),
            registration_event_id: "R".into(),
            source_lots: vec![vec!["original".into()]],
            from_unit: ShareUnitIdentity::Initial,
            to_unit: ShareUnitIdentity::ReDenominated {
                movement_event_id: "overflow".into(),
            },
        },
        request: ShareDayRequest {
            event_id: "overflow".into(),
            day: request.day,
            scope: MovementScope::ShareReDenomination {
                basis: "溢出边界冻结输入".into(),
            },
            changes: vec![DayNetChange {
                holder: holder(1),
                change: 1,
                consume: None,
                acquisition: Some(NetAcquisition {
                    lot_id: "extra".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "overflow".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            }],
        },
    }];
    request.actual_market_close.unit = ShareUnitIdentity::ReDenominated {
        movement_event_id: "overflow".into(),
    };
    let request_before = request.clone();
    assert!(registry
        .prepare_settlement_day(&request, &context(&calendar, &[]))
        .is_err());
    assert_eq!(registry, before);
    assert_eq!(request, request_before);
    let small = foundation("2026-01-05");
    let mut wrong_phase = market(&small, "2026-01-06", 0, &[]);
    wrong_phase
        .post_market_actions
        .push(SharePostAction::Movement(ShareDayRequest {
            event_id: "post-as-market".into(),
            day: wrong_phase.day,
            scope: MovementScope::PublicMarket,
            changes: vec![],
        }));
    assert!(small
        .prepare_settlement_day(&wrong_phase, &context(&calendar, &[]))
        .is_err());
}

#[test]
fn typed_capture_restore_rejects_prior_market_identity_collision() {
    let registry = foundation_shares("2026-01-05", 1000);
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let first_rows = rows("2026-01-06", 100, 0);
    let first = market(&registry, "2026-01-06", 100, &first_rows);
    let next = registry
        .prepare_settlement_day(&first, &context(&calendar, &first_rows))
        .unwrap();
    let mut second = market(&next, "2026-01-07", 0, &[]);
    second
        .post_market_actions
        .push(SharePostAction::CaptureRegistration {
            event_id: "C".into(),
        });
    let candidate = next
        .prepare_settlement_day(&second, &context(&calendar, &first_rows))
        .unwrap();
    let mut corrupt = candidate.clone();
    let prior_market = first.actual_market_close.request.event_id.clone();
    corrupt
        .registrations
        .iter_mut()
        .find(|snapshot| snapshot.event_id == "C")
        .unwrap()
        .event_id = prior_market.clone();
    let MovementScope::SettlementDayMarket { binding } = &mut corrupt.receipts[1].request.scope
    else {
        panic!("typed market必须保留");
    };
    binding.post[0] = SharePostActionRef::CaptureRegistration {
        event_id: prior_market,
    };
    assert!(corrupt.validate().is_err());
    assert!(
        serde_json::from_value::<ShareRegistry>(serde_json::to_value(&corrupt).unwrap()).is_err()
    );
    candidate.validate().unwrap();
}

#[test]
fn post_movement_error_names_event_unit_and_reused_lot() {
    let registry = foundation_shares("2026-01-05", 1000);
    let before = registry.clone();
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let mut request = market(&registry, "2026-01-06", 0, &[]);
    request
        .post_market_actions
        .push(SharePostAction::Movement(ShareDayRequest {
            event_id: "credit-X".into(),
            day: request.day,
            scope: MovementScope::NonTradingTransfer {
                basis: "已冻结到账事实".into(),
            },
            changes: vec![DayNetChange {
                holder: holder(1),
                change: 1,
                consume: None,
                acquisition: Some(NetAcquisition {
                    lot_id: "original".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "credit-X".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            }],
        }));
    let error = registry
        .prepare_settlement_day(&request, &context(&calendar, &[]))
        .unwrap_err()
        .to_string();
    for required in ["credit-X", "original", "Initial", "600001", "2026-01-06"] {
        assert!(
            error.contains(required),
            "错误缺少上下文{required}: {error}"
        );
    }
    assert_eq!(registry, before);
}

#[test]
fn typed_post_unit_movement_binds_result_unit_and_restore_rejects_wrong_transition() {
    let registry = foundation_shares("2026-01-05", 1000);
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let rows = rows("2026-01-06", 100, 100);
    let mut request = market(&registry, "2026-01-06", 200, &rows);
    let post_unit = ShareUnitIdentity::ReDenominated {
        movement_event_id: "post-split".into(),
    };
    request.post_market_actions = vec![
        SharePostAction::UnitMovement {
            request: ShareDayRequest {
                event_id: "post-split".into(),
                day: request.day,
                scope: MovementScope::ShareReDenomination {
                    basis: "明确同日post单位转换".into(),
                },
                changes: [(1, 800, "post-A"), (2, 200, "post-B")]
                    .into_iter()
                    .map(|(id, qty, lot_id)| DayNetChange {
                        holder: holder(id),
                        change: qty,
                        consume: None,
                        acquisition: Some(NetAcquisition {
                            lot_id: lot_id.into(),
                            source: AcquisitionSource::CorporateAction {
                                event: "post-split".into(),
                            },
                            restriction: ShareRestriction::Unrestricted,
                        }),
                    })
                    .collect(),
            },
            from_unit: ShareUnitIdentity::Initial,
            to_unit: post_unit.clone(),
        },
        SharePostAction::CaptureRegistration {
            event_id: "post-R".into(),
        },
    ];
    let candidate = registry
        .prepare_settlement_day(&request, &context(&calendar, &rows))
        .unwrap();
    assert_eq!(candidate.issued_shares(), 2000);
    assert_eq!(candidate.settlement_frontier().unwrap().unit, post_unit);
    assert_eq!(
        candidate.registration("post-R").unwrap().settled_receipts(),
        2
    );
    let restored: ShareRegistry =
        serde_json::from_value(serde_json::to_value(&candidate).unwrap()).unwrap();
    assert_eq!(restored, candidate);
    let mut next = market(&restored, "2026-01-07", 0, &[]);
    halve(&restored, &mut next, "post-R", "next-opening");
    let next_result = restored
        .prepare_settlement_day(&next, &context(&calendar, &rows))
        .unwrap();
    assert_eq!(next_result.issued_shares(), 1000);
    let mut corrupt = candidate.clone();
    let MovementScope::SettlementDayMarket { binding } = &mut corrupt.receipts[0].request.scope
    else {
        panic!("typed市场必须保留");
    };
    let SharePostActionRef::UnitMovement { from_unit, .. } = &mut binding.post[0] else {
        panic!("单位转换必须保留");
    };
    *from_unit = ShareUnitIdentity::ReDenominated {
        movement_event_id: "wrong-prior-unit".into(),
    };
    assert!(corrupt.validate().is_err());
    assert!(
        serde_json::from_value::<ShareRegistry>(serde_json::to_value(&corrupt).unwrap()).is_err()
    );
}
