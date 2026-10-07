use super::*;
use crate::{
    company::CompanyId,
    information::PublicationId,
    session::pipeline::{
        EnvelopeAudit, EnvelopeKey, EventStableKey, FeeComponents, ReceiptDelta, ReceiptLocalKey,
        ReceiptSource, ReceiptTransition,
    },
    session::{
        protocol::{
            attach_facts, AuctionPoint, AuctionPointKind, ContinuousPoint, ProtocolSession,
            TickTimeseriesPayload,
        },
        CompanyDisclosureKind, FloatAllocation, NpcSetup, SecurityCategory, SessionSetup,
        StockExchange, StockSpec,
    },
    CivilDate, CivilInstant, DailyCandle, DailyTradeStats, DayStatus, Event, GameConfig, HotParams,
    InstParams, MarketSnap, Money, RejectionReason, RetailParams, StrategyParams, TradingPhase,
};

fn assert_no_json_numbers(value: &Value) {
    fn visit(value: &Value, path: &str) {
        match value {
            Value::Number(number) => panic!("integer leaked as a JSON number at {path}: {number}"),
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    visit(value, &format!("{path}[{index}]"));
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    visit(value, &format!("{path}.{key}"));
                }
            }
            Value::Null | Value::Bool(_) | Value::String(_) => {}
        }
    }

    visit(value, "$");
}

fn assert_versioned_evidence_runtime_numbers_are_decimal_strings(value: &Value) {
    let mut payload = value.clone();
    let metadata = payload.as_object_mut().expect("验证证据必须是 object");
    assert_eq!(
        metadata.remove("schema_version"),
        Some(serde_json::json!(1))
    );
    assert_no_json_numbers(&payload);
}

#[test]
fn quiet_committed_tick_preserves_zero_interval_and_empty_sum_conservation() {
    let quiet = TickFrame {
        tick: 1,
        events: Vec::new(),
        facts: Vec::new(),
        timeseries_payload: TickTimeseriesPayload::default(),
        seq_from: 0,
        seq_to: 0,
    };
    quiet.validate().unwrap();
    let mut projector = UpdateStreamProjector::new();
    let projected = projector
        .project_update(RuntimeUpdateRef::Tick(&quiet))
        .unwrap();
    assert_eq!((projected.seq_from, projected.seq_to), (1, 0));
    assert!(projected.events.is_empty());
    let second = TickFrame {
        tick: 2,
        ..quiet.clone()
    };
    assert_eq!(
        projector
            .project_update(RuntimeUpdateRef::Tick(&second))
            .unwrap()
            .tick,
        2
    );
    let snapshot = project_conservation_snapshot(
        "quiet",
        1,
        1,
        &[],
        &BTreeMap::from([(AccountId(1), account_snap())]),
    )
    .unwrap();
    assert!(snapshot.envelopes.is_empty());
    assert_eq!(snapshot.accounts[0].aggregate.left.cash_cents, "0");
    assert_eq!(
        snapshot.accounts[0].aggregate.left,
        snapshot.accounts[0].aggregate.right
    );
    let mut invalid = account_snap();
    invalid.cash = Money::from_cents(-1);
    assert!(matches!(
        project_conservation_snapshot(
            "quiet",
            1,
            1,
            &[],
            &BTreeMap::from([(AccountId(1), invalid)])
        ),
        Err(EvidenceError::NegativeMoney { .. })
    ));
}

fn key(side: Side, order: u64) -> EnvelopeKey {
    EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(order),
        side,
    }
}

fn audit(limit: i64, remaining_qty: u32, filled_qty: u32, filled_value: i64) -> EnvelopeAudit {
    EnvelopeAudit {
        limit: Money::from_cents(limit),
        remaining_qty,
        filled_qty,
        filled_value: Money::from_cents(filled_value),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    }
}

fn receipt(
    index: u64,
    key: EnvelopeKey,
    kind: ReceiptKind,
    source: ReceiptSource,
    ordinal: u64,
    delta: ReceiptDelta,
) -> EnvelopeReceipt {
    EnvelopeReceipt {
        index,
        local_key: ReceiptLocalKey::new(
            source.journal(),
            source,
            ReceiptTransition {
                envelope: key.clone(),
                ordinal,
            },
        )
        .unwrap(),
        envelope: key,
        kind,
        qty_before: 100,
        qty_after: delta.live_after.shares,
        value_before: Money::ZERO,
        value_after: Money::from_cents(5_000),
        delta,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    }
}

fn account_snap() -> SaveAccountSnap {
    SaveAccountSnap {
        cash: Money::from_cents(50_000),
        positions: BTreeMap::from([(
            StockCode("600001".to_owned()),
            PositionSnap {
                qty: 100,
                t1_locked: 20,
                invested_cents: 10_000,
                recovered_cents: 0,
            },
        )]),
    }
}

#[test]
fn projects_real_receipt_chain_and_account_aggregate() {
    let envelope_key = key(Side::Sell, 9);
    let receipts = vec![
        receipt(
            10,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            0,
            ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, 30),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, 70),
            ),
        ),
        receipt(
            11,
            envelope_key.clone(),
            ReceiptKind::Rollover,
            ReceiptSource::Auction(0),
            0,
            ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::new(Money::ZERO, 70)),
        ),
    ];
    assert_eq!(
        receipts[0].local_key.source(),
        ReceiptSource::SealedIntent(0)
    );
    assert_eq!(receipts[0].local_key.transition_ordinal(), 0);
    assert_eq!(receipts[0].local_key.transition_envelope(), &envelope_key);
    let mut final_envelope =
        Envelope::created_at_validation(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    final_envelope
        .apply(receipts[0].delta, audit(100, 70, 30, 5_000), false)
        .unwrap();
    final_envelope
        .apply(receipts[1].delta, audit(100, 70, 30, 5_000), false)
        .unwrap();
    let accounts = BTreeMap::from([(AccountId(1), account_snap())]);

    let projected = project_conservation_snapshot(
        "auction-rollover",
        7,
        12,
        &[EnvelopeChainInput {
            envelope: &final_envelope,
            receipts: &receipts,
        }],
        &accounts,
    )
    .unwrap();

    assert_eq!(projected.schema, CONSERVATION_SCHEMA);
    assert_eq!(projected.schema, "escrow-conservation-snapshot");
    assert_eq!(projected.schema_version, 1);
    assert_eq!(projected.envelopes[0].origin, "created");
    assert_eq!(projected.envelopes[0].receipts[0].live_before.shares, "100");
    assert_eq!(projected.envelopes[0].receipts[1].live_before.shares, "70");
    let first_receipt = serde_json::to_value(&projected.envelopes[0].receipts[0]).unwrap();
    assert_eq!(
        first_receipt["source"],
        serde_json::json!({ "kind": "SealedIntent", "index": "0" })
    );
    assert_eq!(first_receipt["transition_ordinal_within_source"], "0");
    assert_eq!(projected.accounts[0].aggregate.left.shares, "100");
    assert_eq!(projected.accounts[0].aggregate.right.shares, "100");
    assert_versioned_evidence_runtime_numbers_are_decimal_strings(
        &serde_json::to_value(&projected).unwrap(),
    );
}

#[test]
fn conservation_accepts_independent_source_order_and_rejects_broken_local_chains() {
    let envelope_key = key(Side::Sell, 91);
    let base_receipts = vec![
        receipt(
            40,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            0,
            ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, 30),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, 70),
            ),
        ),
        receipt(
            41,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            1,
            ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, 20),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, 50),
            ),
        ),
    ];
    let mut envelope = Envelope::created_at_validation(
        envelope_key.clone(),
        Money::ZERO,
        100,
        audit(100, 100, 0, 0),
    );
    envelope
        .apply(base_receipts[0].delta, audit(100, 70, 30, 5_000), false)
        .unwrap();
    envelope
        .apply(base_receipts[1].delta, audit(100, 50, 50, 8_000), false)
        .unwrap();
    let accounts = BTreeMap::from([(AccountId(1), account_snap())]);

    let project = |receipts: &[EnvelopeReceipt]| {
        project_conservation_snapshot(
            "forged-receipt-identity",
            7,
            12,
            &[EnvelopeChainInput {
                envelope: &envelope,
                receipts,
            }],
            &accounts,
        )
    };

    let mut source_swapped = base_receipts.clone();
    source_swapped[0].local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(1),
        ReceiptTransition {
            envelope: envelope_key.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    source_swapped[1].local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(0),
        ReceiptTransition {
            envelope: envelope_key.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    assert!(project(&source_swapped).is_ok());

    let mut ordinal_gap = base_receipts.clone();
    ordinal_gap[1].local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(0),
        ReceiptTransition {
            envelope: envelope_key.clone(),
            ordinal: 2,
        },
    )
    .unwrap();
    assert_eq!(
        project(&ordinal_gap).unwrap_err(),
        EvidenceError::InvalidReceiptIdentity {
            receipt_index: 41,
            detail: "source-local transition ordinal is not zero-based and contiguous",
        }
    );

    let mut envelope_mismatch = base_receipts.clone();
    envelope_mismatch[0].local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(0),
        ReceiptTransition {
            envelope: key(Side::Sell, 999),
            ordinal: 0,
        },
    )
    .unwrap();
    assert_eq!(
        project(&envelope_mismatch).unwrap_err(),
        EvidenceError::InvalidReceiptIdentity {
            receipt_index: 40,
            detail: "local transition envelope disagrees with receipt envelope",
        }
    );

    let mut global_index_swap = base_receipts.clone();
    global_index_swap[0].index = 41;
    global_index_swap[1].index = 40;
    assert_eq!(
        project(&global_index_swap).unwrap_err(),
        EvidenceError::InvalidReceiptIdentity {
            receipt_index: 40,
            detail: "source-local transition ordinal is not zero-based and contiguous",
        }
    );

    let mut duplicate_identity = base_receipts.clone();
    duplicate_identity[1].local_key = duplicate_identity[0].local_key.clone();
    assert_eq!(
        project(&duplicate_identity).unwrap_err(),
        EvidenceError::InvalidReceiptIdentity {
            receipt_index: 41,
            detail: "duplicate local receipt identity",
        }
    );

    let mut duplicate_index = base_receipts.clone();
    duplicate_index[1].index = 40;
    assert_eq!(
        project(&duplicate_index).unwrap_err(),
        EvidenceError::ReceiptIndexSequence
    );
}

#[test]
fn existing_envelope_projects_the_post_preseal_decision_resources_boundary() {
    let envelope_key = key(Side::Sell, 10);
    let receipts = vec![
        receipt(
            20,
            envelope_key.clone(),
            ReceiptKind::Release,
            ReceiptSource::QuoteExpiry(0),
            0,
            ReceiptDelta::sealed(
                ResVec::ZERO,
                ResVec::new(Money::ZERO, 10),
                ResVec::new(Money::ZERO, 90),
            ),
        ),
        receipt(
            21,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            0,
            ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, 30),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, 60),
            ),
        ),
    ];
    let mut envelope =
        Envelope::tick_start_existing(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    envelope
        .apply(receipts[0].delta, audit(100, 90, 0, 0), false)
        .unwrap();
    envelope
        .apply(receipts[1].delta, audit(100, 60, 30, 5_000), false)
        .unwrap();

    let projected = project_conservation_snapshot(
        "preseal-release",
        7,
        12,
        &[EnvelopeChainInput {
            envelope: &envelope,
            receipts: &receipts,
        }],
        &BTreeMap::from([(AccountId(1), account_snap())]),
    )
    .unwrap();

    assert_eq!(projected.envelopes[0].origin, "existing");
    let ConservationBasisProjection::Existing {
        tick_start_live,
        allocation_live,
    } = &projected.envelopes[0].basis
    else {
        panic!("existing envelope projected as created")
    };
    assert_eq!(tick_start_live.shares, "100");
    assert_eq!(allocation_live.shares, "90");
    let basis = serde_json::to_value(&projected.envelopes[0].basis).unwrap();
    assert!(basis.get("allocation_live").is_some());
    assert!(basis.get("p1_live").is_none());
}

#[test]
fn conservation_rejects_receipt_gaps_negative_cash_and_t1_overflow() {
    let envelope_key = key(Side::Sell, 11);
    let mut receipts = vec![
        receipt(
            30,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            0,
            ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, 30),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, 70),
            ),
        ),
        receipt(
            32,
            envelope_key.clone(),
            ReceiptKind::Rollover,
            ReceiptSource::Auction(0),
            0,
            ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::new(Money::ZERO, 70)),
        ),
    ];
    let mut envelope =
        Envelope::created_at_validation(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    envelope
        .apply(receipts[0].delta, audit(100, 70, 30, 5_000), false)
        .unwrap();
    envelope
        .apply(receipts[1].delta, audit(100, 70, 30, 5_000), false)
        .unwrap();
    let chain = [EnvelopeChainInput {
        envelope: &envelope,
        receipts: &receipts,
    }];
    let accounts = BTreeMap::from([(AccountId(1), account_snap())]);
    assert_eq!(
        project_conservation_snapshot("bad-index", 7, 12, &chain, &accounts).unwrap_err(),
        EvidenceError::ReceiptIndexSequence
    );

    receipts[1].index = 30;
    let duplicate_chain = [EnvelopeChainInput {
        envelope: &envelope,
        receipts: &receipts,
    }];
    assert_eq!(
        project_conservation_snapshot("duplicate-index", 7, 12, &duplicate_chain, &accounts,)
            .unwrap_err(),
        EvidenceError::ReceiptIndexSequence
    );
    receipts[1].index = 31;
    let chain = [EnvelopeChainInput {
        envelope: &envelope,
        receipts: &receipts,
    }];
    let mut negative = account_snap();
    negative.cash = Money::from_cents(-1);
    assert!(matches!(
        project_conservation_snapshot(
            "negative-cash",
            7,
            12,
            &chain,
            &BTreeMap::from([(AccountId(1), negative)]),
        ),
        Err(EvidenceError::NegativeMoney {
            field: "account.cash",
            cents: -1
        })
    ));

    let mut overflow = account_snap();
    overflow
        .positions
        .get_mut(&StockCode("600001".to_owned()))
        .unwrap()
        .t1_locked = 101;
    assert!(matches!(
        project_conservation_snapshot(
            "t1-overflow",
            7,
            12,
            &chain,
            &BTreeMap::from([(AccountId(1), overflow)]),
        ),
        Err(EvidenceError::T1Overflow { .. })
    ));
}

fn candle() -> DailyCandle {
    DailyCandle {
        time: 0,
        open: Money::from_cents(1_000),
        high: Money::from_cents(1_000),
        low: Money::from_cents(1_000),
        close: Money::from_cents(1_000),
        volume: 100,
        trade_stats: None,
    }
}

fn frame(events: Vec<Event>) -> TickFrame {
    let seq_to = u64::try_from(events.len()).unwrap();
    TickFrame {
        tick: 5,
        facts: attach_facts(&events).unwrap(),
        events,
        timeseries_payload: TickTimeseriesPayload::default(),
        seq_from: 0,
        seq_to,
    }
}

fn announcement_event(seq: u64, publication_id: u32) -> Event {
    Event::CompanyDisclosurePublished {
        seq,
        publication_id: PublicationId::new(publication_id),
        company: CompanyId("company".to_owned()),
        published_at: CivilInstant::new(CivilDate::from_iso("2030-01-05").unwrap(), 0).unwrap(),
        kind: CompanyDisclosureKind::Announcement,
    }
}

fn account_identity_frame(indices: &[u64]) -> TickFrame {
    let events = indices
        .iter()
        .enumerate()
        .map(|(position, _)| {
            let seq = u64::try_from(position + 1).unwrap();
            if position == 0 {
                Event::IntentRejected {
                    seq,
                    account: AccountId(3),
                    code: StockCode("600001".to_owned()),
                    reason: RejectionReason::InsufficientCash,
                }
            } else {
                Event::OrderCanceled {
                    seq,
                    account: AccountId(3),
                    code: StockCode("600001".to_owned()),
                    id: OrderId(seq),
                    remaining_qty: 100,
                }
            }
        })
        .collect();
    let mut frame = frame(events);
    for (fact, index) in frame.facts.iter_mut().zip(indices) {
        fact.key = EventStableKey::for_event(&fact.event, *index);
    }
    frame
}

#[test]
fn account_sealed_identity_accepts_sparse_operation_and_reserved_expiry_indices() {
    let reserved = crate::orderbook::js_safe_u64::MAX - u32::MAX as u64;
    let frame = account_identity_frame(&[7, reserved, crate::orderbook::js_safe_u64::MAX]);
    let projected = project_update(RuntimeUpdateRef::Tick(&frame)).unwrap();
    assert_eq!(projected.events[0].comparison_event_key.3, "7");
    assert_eq!(
        projected.events[1].comparison_event_key.3,
        reserved.to_string()
    );
}

#[test]
fn account_sealed_identity_rejects_cross_variant_key_collision() {
    let reserved = crate::orderbook::js_safe_u64::MAX - u32::MAX as u64;
    for indices in [vec![7, 7], vec![7, reserved, reserved]] {
        let frame = account_identity_frame(&indices);
        assert!(matches!(
            project_update(RuntimeUpdateRef::Tick(&frame)),
            Err(EvidenceError::InvalidEventIdentity { .. })
        ));
    }
}

#[test]
fn account_sealed_identity_rejects_unsafe_and_non_cancel_reserved_indices() {
    for index in [
        crate::orderbook::js_safe_u64::MAX + 1,
        crate::orderbook::js_safe_u64::MAX - u32::MAX as u64,
    ] {
        let frame = account_identity_frame(&[index]);
        assert!(matches!(
            project_update(RuntimeUpdateRef::Tick(&frame)),
            Err(EvidenceError::InvalidEventIdentity { .. })
        ));
    }
}

#[test]
fn stock_event_identity_still_requires_zero_based_contiguous_indices() {
    let event = Event::Trade {
        seq: 1,
        code: StockCode("600001".to_owned()),
        price: Money::from_cents(1_000),
        qty: 100,
        maker: AccountId(3),
        taker: AccountId(4),
    };
    let mut frame = frame(vec![event]);
    frame.facts[0].key = EventStableKey::for_event(&frame.facts[0].event, 7);
    assert!(matches!(
        project_update(RuntimeUpdateRef::Tick(&frame)),
        Err(EvidenceError::InvalidEventIdentity { .. })
    ));
}

#[test]
fn session_lifecycle_variants_share_one_ordinal_scope() {
    let events = vec![
        announcement_event(1, 10),
        Event::CivilDateAdvanced {
            seq: 2,
            settled_date: CivilDate::from_ymd(2026, 9, 18).unwrap(),
            next_date: CivilDate::from_ymd(2026, 9, 21).unwrap(),
            next_status: DayStatus::Trading,
        },
    ];

    let projected = project_update(RuntimeUpdateRef::Tick(&frame(events))).unwrap();

    assert_eq!(
        projected.events[0].comparison_event_key.1,
        "6:CompanyDisclosurePublished"
    );
    assert_eq!(
        projected.events[1].comparison_event_key.1,
        "6:CivilDateAdvanced"
    );
    assert_eq!(projected.events[0].comparison_event_key.3, "0");
    assert_eq!(projected.events[1].comparison_event_key.3, "1");
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn projects_event_identity_by_variant_entity_scope_not_seq_or_array_position() {
    let events = vec![
        Event::OrderAccepted {
            seq: 1,
            account: AccountId(1),
            code: StockCode("600001".to_owned()),
            id: OrderId(9),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            remaining_qty: 100,
        },
        Event::PriceTick {
            seq: 2,
            tick: 5,
            code: StockCode("600001".to_owned()),
            last_price: Money::from_cents(1_000),
            daily_candle: candle(),
            bids: vec![],
            asks: vec![],
        },
        Event::OrderAccepted {
            seq: 3,
            account: AccountId(1),
            code: StockCode("600001".to_owned()),
            id: OrderId(10),
            side: Side::Buy,
            price: Money::from_cents(999),
            remaining_qty: 100,
        },
    ];
    let frame = frame(events);

    let projected = project_update(RuntimeUpdateRef::Tick(&frame)).unwrap();

    assert_eq!(projected.seq_from, 1, "序号范围采用包含端点的范围");
    assert_eq!(projected.events[0].comparison_event_key.0, "5");
    assert_eq!(
        projected.events[0].comparison_event_key.1,
        "4:OrderAccepted"
    );
    assert_eq!(projected.events[0].comparison_event_key.2, "Account:1");
    assert_eq!(projected.events[0].comparison_event_key.3, "0");
    assert_eq!(projected.events[1].comparison_event_key.0, "5");
    assert_eq!(projected.events[1].comparison_event_key.1, "4:PriceTick");
    assert_eq!(projected.events[1].comparison_event_key.2, "Stock:600001");
    assert_eq!(projected.events[1].comparison_event_key.3, "0");
    assert_eq!(projected.events[2].comparison_event_key.0, "5");
    assert_eq!(
        projected.events[2].comparison_event_key.1,
        "4:OrderAccepted"
    );
    assert_eq!(projected.events[2].comparison_event_key.2, "Account:1");
    assert_eq!(projected.events[2].comparison_event_key.3, "1");
}

#[test]
fn event_comparison_tags_keep_phase_four_five_and_six_distinct() {
    let events = vec![
        Event::OrderAccepted {
            seq: 1,
            account: AccountId(1),
            code: StockCode("600001".to_owned()),
            id: OrderId(9),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            remaining_qty: 100,
        },
        Event::DayBoundary {
            seq: 2,
            day: 1,
            closed_daily_candles: BTreeMap::new(),
        },
        announcement_event(3, 10),
    ];

    let projected = project_update(RuntimeUpdateRef::Tick(&frame(events))).unwrap();

    assert_eq!(
        projected.events[0].comparison_event_key.1,
        "4:OrderAccepted"
    );
    assert_eq!(projected.events[1].comparison_event_key.1, "5:DayBoundary");
    assert_eq!(
        projected.events[2].comparison_event_key.1,
        "6:CompanyDisclosurePublished"
    );
}

fn civil_protocol_session() -> ProtocolSession {
    ProtocolSession::new(
        SessionSetup {
            company_system: simple_company_fixture!(crate; ["600001"]),
            stocks: vec![StockSpec {
                code: StockCode("600001".to_owned()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.1,
                tick: Money::from_cents(1),
                total_shares: 1_000_000,
                float_shares: 0,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 0,
                hot_count: 0,
                retail_cash_median: Money::ZERO,
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                },
                inst: InstParams {
                    margin: 0.03,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 100,
                },
            },
            ticks_per_day: 20,
            auction_ticks: 6,
            closing_auction_ticks: 2,
            history_len: 20,
            t1_enabled: true,
            report_frequency: crate::information::ReportFrequency::Quarterly,
            float_allocation: FloatAllocation::random(),
            start_date: CivilDate::from_iso("2030-01-05").unwrap(),
            simulation_policy_id: crate::SIMULATION_POLICY_ID.to_owned(),
            dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
            flat_withholding_bp: Some(1000),
            rights_offering_enabled: false,
            issuer_repurchase_enabled: false,
            par_value_per_share: crate::Money::from_cents(100),
            auto_corporate_foundation: false,
        },
        42,
    )
    .unwrap()
}

#[test]
fn real_civil_update_projects_validated_session_lifecycle_shared_ordinals() {
    let mut session = civil_protocol_session();
    let mut update = session.end_civil_day_update().unwrap();
    let first_seq = update.seq_from.checked_add(1).unwrap();
    let disclosure_seq = first_seq.checked_add(1).unwrap();
    let announcement_seq = disclosure_seq.checked_add(1).unwrap();
    let publication_id = PublicationId::new(u32::MAX);
    update.events = vec![
        Event::CivilDateAdvanced {
            seq: first_seq,
            settled_date: update.boundary.settled_date,
            next_date: update.boundary.next_date,
            next_status: update.boundary.next_status.clone(),
        },
        Event::CompanyDisclosurePublished {
            seq: disclosure_seq,
            publication_id,
            company: CompanyId("company".to_owned()),
            published_at: CivilInstant::new(update.boundary.next_date, 86_399).unwrap(),
            kind: CompanyDisclosureKind::Report {
                report_revision: u32::MAX,
            },
        },
        announcement_event(announcement_seq, u32::MAX - 1),
    ];
    update.facts = attach_facts(&update.events).unwrap();
    update.seq_to = announcement_seq;
    update.refresh.snapshot.seq = announcement_seq;
    update.refresh.public_publication_ids = vec![
        publication_id.value().to_string(),
        (u32::MAX - 1).to_string(),
    ];
    update.validate().unwrap();

    let projected = project_update(RuntimeUpdateRef::Civil(&update)).unwrap();

    assert_eq!(projected.kind, "CivilUpdate");
    assert_eq!(
        projected
            .events
            .iter()
            .map(|event| event.comparison_event_key.1.as_str())
            .collect::<Vec<_>>(),
        [
            "6:CivilDateAdvanced",
            "6:CompanyDisclosurePublished",
            "6:CompanyDisclosurePublished",
        ]
    );
    assert_eq!(
        projected
            .events
            .iter()
            .map(|event| event.comparison_event_key.3.as_str())
            .collect::<Vec<_>>(),
        ["0", "1", "2"]
    );
    assert_eq!(
        projected.civil_payload.as_ref().unwrap()["refresh"]["public_publication_ids"][0],
        u32::MAX.to_string()
    );
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

fn tick_before_civil(stock: &StockCode) -> TickFrame {
    let events = vec![
        Event::IntentRejected {
            seq: 1,
            account: AccountId(1),
            code: stock.clone(),
            reason: RejectionReason::PriceCageExceeded,
        },
        Event::OrderAccepted {
            seq: 2,
            account: AccountId(1),
            code: stock.clone(),
            id: OrderId(41),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            remaining_qty: 100,
        },
        announcement_event(3, u32::MAX - 1),
    ];
    let frame = TickFrame {
        tick: 5,
        facts: attach_facts(&events).unwrap(),
        events,
        timeseries_payload: TickTimeseriesPayload::default(),
        seq_from: 0,
        seq_to: 3,
    };
    frame.validate().unwrap();
    frame
}

fn civil_after_tick(duplicate_disclosure: bool) -> CivilUpdate {
    let mut session = civil_protocol_session();
    let mut update = session.end_civil_day_update().unwrap();
    let date_event = Event::CivilDateAdvanced {
        seq: if duplicate_disclosure { 5 } else { 4 },
        settled_date: update.boundary.settled_date,
        next_date: update.boundary.next_date,
        next_status: update.boundary.next_status.clone(),
    };
    let disclosure_event = Event::CompanyDisclosurePublished {
        seq: if duplicate_disclosure { 6 } else { 5 },
        publication_id: PublicationId::new(u32::MAX),
        company: CompanyId("company".to_owned()),
        published_at: CivilInstant::new(update.boundary.next_date, 86_399).unwrap(),
        kind: CompanyDisclosureKind::Report {
            report_revision: u32::MAX,
        },
    };
    update.events = if duplicate_disclosure {
        vec![
            announcement_event(4, u32::MAX - 1),
            date_event,
            disclosure_event,
        ]
    } else {
        vec![date_event, disclosure_event]
    };
    update.tick = 5;
    update.seq_from = 3;
    update.seq_to = update.events.last().unwrap().seq();
    update.facts = attach_facts(&update.events).unwrap();
    update.refresh.snapshot.tick = update.tick;
    update.refresh.snapshot.seq = update.seq_to;
    update.refresh.public_publication_ids = vec![u32::MAX.to_string(), (u32::MAX - 1).to_string()];
    update.validate().unwrap();
    update
}

fn civil_with_continued_session_lifecycle_ordinals() -> CivilUpdate {
    let mut civil = civil_after_tick(false);
    for (offset, fact) in civil.facts.iter_mut().enumerate() {
        fact.key = crate::session::pipeline::EventStableKey::for_event(
            &fact.event,
            u64::try_from(offset).unwrap() + 1,
        );
    }
    civil.validate().unwrap();
    civil
}

#[test]
fn full_update_stream_projection_shares_same_tick_session_lifecycle_ordinals() {
    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    let civil = civil_with_continued_session_lifecycle_ordinals();

    let projected = project_update_stream(&[
        RuntimeUpdateRef::Tick(&tick),
        RuntimeUpdateRef::Civil(&civil),
    ])
    .unwrap();

    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].events[2].comparison_event_key.3, "0");
    assert_eq!(projected[1].events[0].comparison_event_key.3, "1");
    assert_eq!(projected[1].events[1].comparison_event_key.3, "2");
    assert_eq!(projected[0].seq_to + 1, projected[1].seq_from);
    assert_eq!(projected[0].tick, projected[1].tick);
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn stateful_update_stream_projection_matches_batch_and_rejects_cross_update_reset() {
    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    let reset = civil_after_tick(false);
    let civil = civil_with_continued_session_lifecycle_ordinals();
    let expected = project_update_stream(&[
        RuntimeUpdateRef::Tick(&tick),
        RuntimeUpdateRef::Civil(&civil),
    ])
    .unwrap();

    let mut projector = UpdateStreamProjector::new();
    let first = projector
        .project_update(RuntimeUpdateRef::Tick(&tick))
        .unwrap();
    assert_eq!(
        projector
            .project_update(RuntimeUpdateRef::Civil(&reset))
            .unwrap_err(),
        EvidenceError::InvalidEventIdentity {
            detail:
                "local_event_index is not zero-based and contiguous in its full update-stream ADR domain",
        }
    );
    let second = projector
        .project_update(RuntimeUpdateRef::Civil(&civil))
        .unwrap();

    assert_eq!(vec![first, second], expected);
}

#[test]
fn update_stream_failure_does_not_consume_sequence_or_identity_state() {
    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    let civil = civil_with_continued_session_lifecycle_ordinals();
    let mut sequence_gap = civil.clone();
    sequence_gap.seq_from += 1;
    sequence_gap.seq_to += 1;
    sequence_gap.refresh.snapshot.seq += 1;
    for event in &mut sequence_gap.events {
        match event {
            Event::CivilDateAdvanced { seq, .. }
            | Event::CompanyDisclosurePublished { seq, .. } => *seq += 1,
            unexpected => panic!("unexpected CivilUpdate event: {unexpected:?}"),
        }
    }
    sequence_gap.facts = attach_facts(&sequence_gap.events).unwrap();
    for (offset, fact) in sequence_gap.facts.iter_mut().enumerate() {
        fact.key = crate::session::pipeline::EventStableKey::for_event(
            &fact.event,
            u64::try_from(offset).unwrap() + 1,
        );
    }
    sequence_gap.validate().unwrap();

    let mut projector = UpdateStreamProjector::new();
    projector
        .project_update(RuntimeUpdateRef::Tick(&tick))
        .unwrap();
    assert_eq!(
        projector
            .project_update(RuntimeUpdateRef::Civil(&sequence_gap))
            .unwrap_err(),
        EvidenceError::InvalidUpdate {
            detail: "full runtime update stream has a sequence or tick gap".to_owned(),
        }
    );
    let recovered = projector
        .project_update(RuntimeUpdateRef::Civil(&civil))
        .unwrap();
    assert_eq!(recovered.events[0].comparison_event_key.3, "1");
}

#[test]
fn stateful_update_stream_projection_resets_ordinal_domains_on_the_next_tick() {
    let first = frame(vec![announcement_event(1, 10)]);
    let events = vec![announcement_event(2, 10)];
    let second = TickFrame {
        tick: 6,
        facts: attach_facts(&events).unwrap(),
        events,
        timeseries_payload: TickTimeseriesPayload::default(),
        seq_from: 1,
        seq_to: 2,
    };
    second.validate().unwrap();

    let mut projector = UpdateStreamProjector::new();
    let first = projector
        .project_update(RuntimeUpdateRef::Tick(&first))
        .unwrap();
    let second = projector
        .project_update(RuntimeUpdateRef::Tick(&second))
        .unwrap();

    assert_eq!(first.events[0].comparison_event_key.3, "0");
    assert_eq!(second.events[0].comparison_event_key.3, "0");
}

#[test]
fn batch_update_stream_projection_rejects_empty_and_duplicate_identity_streams() {
    assert_eq!(
        project_update_stream(&[]).unwrap_err(),
        EvidenceError::InvalidUpdate {
            detail: "full runtime update stream contains no updates".to_owned(),
        }
    );

    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    let duplicate = civil_after_tick(true);
    assert_eq!(
        project_update_stream(&[
            RuntimeUpdateRef::Tick(&tick),
            RuntimeUpdateRef::Civil(&duplicate),
        ])
        .unwrap_err(),
        EvidenceError::InvalidEventIdentity {
            detail: "comparison_event_key is duplicated across the full update stream",
        }
    );
}

#[test]
fn event_projection_rejects_gapped_stock_indices_and_adr_key_tampering() {
    let events = vec![
        Event::Trade {
            seq: 1,
            code: StockCode("600001".to_owned()),
            price: Money::from_cents(1_000),
            qty: 100,
            maker: AccountId(1),
            taker: AccountId(2),
        },
        Event::Trade {
            seq: 2,
            code: StockCode("600001".to_owned()),
            price: Money::from_cents(999),
            qty: 100,
            maker: AccountId(1),
            taker: AccountId(2),
        },
    ];
    let mut gapped = frame(events);
    gapped.facts[1].key =
        crate::session::pipeline::EventStableKey::for_event(&gapped.facts[1].event, 7);
    assert!(project_update(RuntimeUpdateRef::Tick(&gapped)).is_err());

    let event = Event::OrderAccepted {
        seq: 1,
        account: AccountId(1),
        code: StockCode("600001".to_owned()),
        id: OrderId(9),
        side: Side::Buy,
        price: Money::from_cents(1_000),
        remaining_qty: 100,
    };
    for (field, forged_value) in [
        ("phase_rank", serde_json::json!(5)),
        ("entity", serde_json::json!({ "Account": 2 })),
        ("source", serde_json::json!("QuoteExpiry")),
    ] {
        let mut tampered = frame(vec![event.clone()]);
        let mut key = serde_json::to_value(&tampered.facts[0].key).unwrap();
        key[field] = forged_value;
        tampered.facts[0].key = serde_json::from_value(key).unwrap();
        let error = project_update(RuntimeUpdateRef::Tick(&tampered)).unwrap_err();
        assert!(
            error.to_string().starts_with("event fact identity"),
            "producer must reject {field} before generic protocol validation: {error}"
        );
    }
}

#[test]
fn every_runtime_integer_projection_uses_canonical_decimal_strings() {
    let code = StockCode("600001".to_owned());
    let date = CivilDate::from_ymd(2199, 12, 31).unwrap();
    let instant = CivilInstant::new(date, 86_399).unwrap();
    let extreme_candle = DailyCandle {
        time: i64::MIN,
        open: Money::from_cents(i64::MAX),
        high: Money::from_cents(i64::MAX),
        low: Money::from_cents(i64::MIN),
        close: Money::from_cents(i64::MAX),
        volume: u64::MAX,
        trade_stats: Some(DailyTradeStats {
            turnover_cents: u128::from(u64::MAX),
            trade_count: u64::MAX,
        }),
    };
    let events = vec![
        Event::Trade {
            seq: u64::MAX,
            code: code.clone(),
            price: Money::from_cents(i64::MAX),
            qty: u32::MAX,
            maker: AccountId(u64::MAX),
            taker: AccountId(u64::MAX - 1),
        },
        Event::AuctionTick {
            seq: u64::MAX,
            tick: u64::MAX,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            indicative_price: Some(Money::from_cents(i64::MAX)),
            matched_volume: u64::MAX,
            imbalance: u64::MAX,
        },
        Event::AuctionCompleted {
            seq: u64::MAX,
            tick: u64::MAX,
            phase: TradingPhase::ClosingAuction,
            code: code.clone(),
            clearing_price: Some(Money::from_cents(i64::MAX)),
            matched_volume: u64::MAX,
        },
        Event::PriceTick {
            seq: u64::MAX,
            tick: u64::MAX,
            code: code.clone(),
            last_price: Money::from_cents(i64::MAX),
            daily_candle: extreme_candle.clone(),
            bids: vec![(Money::from_cents(i64::MAX), u64::MAX)],
            asks: vec![(Money::from_cents(i64::MIN), u64::MAX)],
        },
        Event::DayBoundary {
            seq: u64::MAX,
            day: u32::MAX,
            closed_daily_candles: BTreeMap::from([(code.clone(), extreme_candle.clone())]),
        },
        Event::CivilDateAdvanced {
            seq: u64::MAX,
            settled_date: date,
            next_date: date,
            next_status: DayStatus::Trading,
        },
        Event::CompanyDisclosurePublished {
            seq: u64::MAX,
            publication_id: PublicationId::new(u32::MAX),
            company: CompanyId("company".to_owned()),
            published_at: instant,
            kind: CompanyDisclosureKind::Report {
                report_revision: u32::MAX,
            },
        },
        Event::IntentRejected {
            seq: u64::MAX,
            account: AccountId(u64::MAX),
            code: code.clone(),
            reason: RejectionReason::InsufficientCash,
        },
        Event::SettlementError {
            seq: u64::MAX,
            account: AccountId(u64::MAX),
            code: code.clone(),
            reason: "fixture".to_owned(),
        },
        Event::OrderCanceled {
            seq: u64::MAX,
            account: AccountId(u64::MAX),
            code: code.clone(),
            id: OrderId(u64::MAX),
            remaining_qty: u32::MAX,
        },
        Event::OrderAccepted {
            seq: u64::MAX,
            account: AccountId(u64::MAX),
            code: code.clone(),
            id: OrderId(u64::MAX),
            side: Side::Buy,
            price: Money::from_cents(i64::MAX),
            remaining_qty: u32::MAX,
        },
    ];
    for event in &events {
        let (_, projected) = project_event(event).unwrap();
        assert_no_json_numbers(&projected);
    }

    let key = crate::session::pipeline::EventStableKey::for_event(events.last().unwrap(), u64::MAX);
    assert_no_json_numbers(&project_stable_key(&key).unwrap());
    assert_no_json_numbers(&project_candle(&extreme_candle).unwrap());
    assert_no_json_numbers(&Value::Array(
        project_depth(&[(Money::from_cents(i64::MAX), u64::MAX)], "test.depth").unwrap(),
    ));

    let timeseries = TickTimeseriesPayload {
        markets: BTreeMap::from([(
            code.clone(),
            MarketSnap {
                cash_ex_reference_pending_trade: false,
                day_market_activity: false,
                last_cash_ex_reference: None,
                last_price: Money::from_cents(i64::MAX),
                last_close: Money::from_cents(i64::MIN),
                best_bid: Some(Money::from_cents(i64::MAX)),
                best_ask: Some(Money::from_cents(i64::MIN)),
                bids: vec![(Money::from_cents(i64::MAX), u64::MAX)],
                asks: vec![(Money::from_cents(i64::MIN), u64::MAX)],
            },
        )]),
        active_daily_candles: BTreeMap::from([(code.clone(), extreme_candle.clone())]),
        closed_daily_candles: BTreeMap::new(),
        auction_points: BTreeMap::from([(
            code.clone(),
            vec![AuctionPoint {
                key: key.clone(),
                tick: u64::MAX,
                kind: AuctionPointKind::Completion,
                phase: TradingPhase::ClosingAuction,
                indicative_price: Some(Money::from_cents(i64::MAX)),
                matched_volume: u64::MAX,
                imbalance: Some(u64::MAX),
            }],
        )]),
        continuous_points: BTreeMap::from([(
            code,
            ContinuousPoint {
                tick: u64::MAX,
                phase: TradingPhase::Continuous,
                last_price: Money::from_cents(i64::MAX),
                cumulative_volume: u64::MAX,
                bids: vec![(Money::from_cents(i64::MAX), u64::MAX)],
                asks: vec![(Money::from_cents(i64::MIN), u64::MAX)],
            },
        )]),
    };
    assert_no_json_numbers(&project_timeseries_payload(&timeseries).unwrap());
}

struct TestDigest;
impl Sha256Provider for TestDigest {
    fn digest_hex(&self, bytes: &[u8]) -> String {
        format!(
            "{:064x}",
            bytes.iter().fold(0u64, |sum, byte| sum + u64::from(*byte))
        )
    }
}

fn restore(slot: &'static str) -> RestoreSlotBytes<'static> {
    RestoreSlotBytes {
        slot,
        saved: b"save",
        restored: b"save",
        uninterrupted_continuation: b"continuation",
        restored_continuation: b"continuation",
    }
}

fn observation_input<'a>(
    conservation: &'a [ConservationSnapshot],
    restore_slots: Option<&'a [RestoreSlotBytes<'a>]>,
    save_slot: Option<&'a [u8]>,
    account_shards: &'a [String],
    stock_shards: &'a [String],
    completion_order: &'a [String],
    finalizers: &'a [FinalizerAuditInput],
) -> ObservationInput<'a> {
    ObservationInput {
        scenario: "auction-rollover",
        seed: 7,
        budget: "2",
        repeat: u32::MAX,
        mode: ObservationMode::Canonical,
        canonical_merge_disabled: None,
        artifacts: ObservationArtifactBytes {
            authoritative_state: b"state",
            event_stream: b"events",
            receipts: b"receipts",
            save_slot,
        },
        precanonical_order: PrecanonicalOrderInput {
            account_shards,
            stock_shards,
            completion_order,
        },
        finalizers,
        conservation,
        restore_slots,
    }
}

fn coverage_snapshot(
    tick: u64,
    code: &str,
    order: u64,
    fill_quantities: &[u32],
) -> ConservationSnapshot {
    let stock = StockCode(code.to_owned());
    let envelope_key = EnvelopeKey {
        account: AccountId(1),
        stock: stock.clone(),
        order: OrderId(order),
        side: Side::Sell,
    };
    let mut live = 100u32;
    let mut receipts = Vec::new();
    let mut filled = 0u32;
    for (ordinal, qty) in fill_quantities.iter().copied().enumerate() {
        live = live.checked_sub(qty).unwrap();
        filled = filled.checked_add(qty).unwrap();
        receipts.push(receipt(
            u64::from(ordinal as u32),
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            u64::from(ordinal as u32),
            ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, qty),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, live),
            ),
        ));
    }
    if live > 0 {
        receipts.push(receipt(
            u64::try_from(receipts.len()).unwrap(),
            envelope_key.clone(),
            ReceiptKind::Release,
            ReceiptSource::DayEnd(0),
            0,
            ReceiptDelta::sealed(ResVec::ZERO, ResVec::new(Money::ZERO, live), ResVec::ZERO),
        ));
        live = 0;
    }
    let mut envelope =
        Envelope::created_at_validation(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    for row in &receipts {
        envelope
            .apply(
                row.delta,
                audit(100, live, filled, i64::from(filled) * 100),
                false,
            )
            .unwrap();
    }
    let account = SaveAccountSnap {
        cash: Money::from_cents(50_000),
        positions: BTreeMap::from([(
            stock,
            PositionSnap {
                qty: 100,
                t1_locked: 0,
                invested_cents: 10_000,
                recovered_cents: 0,
            },
        )]),
    };
    project_conservation_snapshot(
        "auction-rollover",
        7,
        tick,
        &[EnvelopeChainInput {
            envelope: &envelope,
            receipts: &receipts,
        }],
        &BTreeMap::from([(AccountId(1), account)]),
    )
    .unwrap()
}

#[test]
fn observation_requires_real_save_and_two_restore_byte_chains() {
    let empty: Vec<ConservationSnapshot> = vec![];
    let accounts = vec!["account:1".to_owned(), "account:2".to_owned()];
    let stocks = vec!["stock:000001".to_owned(), "stock:600001".to_owned()];
    let completion = vec!["worker:0".to_owned(), "worker:1".to_owned()];
    let finalizers = [FinalizerAuditInput {
        auction_finalizations: 1,
        day_end_finalizations: 1,
    }];

    let error = project_observation(
        observation_input(
            &empty,
            Some(&[restore("opening"), restore("partial")]),
            None,
            &accounts,
            &stocks,
            &completion,
            &finalizers,
        ),
        &TestDigest,
    )
    .unwrap_err();
    assert_eq!(error, EvidenceError::MissingSaveBytes);

    let error = project_observation(
        observation_input(
            &empty,
            None,
            Some(b"save"),
            &accounts,
            &stocks,
            &completion,
            &finalizers,
        ),
        &TestDigest,
    )
    .unwrap_err();
    assert_eq!(error, EvidenceError::MissingRestoreSlots);
}

#[test]
fn observation_projects_real_multi_stock_multi_leg_and_finalizer_coverage() {
    let conservation = vec![
        coverage_snapshot(12, "600001", 9, &[30, 70]),
        coverage_snapshot(13, "000001", 10, &[100]),
    ];
    let accounts = vec!["account:1".to_owned(), "account:2".to_owned()];
    let stocks = vec!["stock:000001".to_owned(), "stock:600001".to_owned()];
    let completion = vec!["worker:1".to_owned(), "worker:0".to_owned()];
    let restores = [restore("opening"), restore("partial")];
    let finalizers = [
        FinalizerAuditInput {
            auction_finalizations: u64::MAX,
            day_end_finalizations: 0,
        },
        FinalizerAuditInput {
            auction_finalizations: 0,
            day_end_finalizations: u64::MAX,
        },
    ];

    let projected = project_observation(
        observation_input(
            &conservation,
            Some(&restores),
            Some(b"serialized-save-slot-bytes"),
            &accounts,
            &stocks,
            &completion,
            &finalizers,
        ),
        &TestDigest,
    )
    .unwrap();

    assert_eq!(projected.execution_coverage.tick_from, 12);
    assert_eq!(projected.execution_coverage.tick_to, 13);
    assert_eq!(
        projected.execution_coverage.stock_codes,
        ["000001".to_owned(), "600001".to_owned()]
    );
    assert_eq!(projected.execution_coverage.multi_leg_order_ids, ["9"]);
    assert_eq!(projected.repeat, u32::MAX);
    assert_eq!(projected.execution_coverage.auction_finalizations, u64::MAX);
    assert_eq!(projected.execution_coverage.day_end_finalizations, u64::MAX);
    assert_eq!(projected.execution_coverage.restore_slots.len(), 2);
    assert_eq!(projected.schema, "escrow-determinism-observation");
    assert_eq!(projected.schema_version, 1);
    assert_versioned_evidence_runtime_numbers_are_decimal_strings(
        &serde_json::to_value(&projected).unwrap(),
    );
}

#[test]
fn observation_rejects_restore_byte_mismatch_but_allows_independent_continuations() {
    let conservation = vec![
        coverage_snapshot(12, "600001", 9, &[30, 70]),
        coverage_snapshot(13, "000001", 10, &[100]),
    ];
    let accounts = vec!["account:1".to_owned(), "account:2".to_owned()];
    let stocks = vec!["stock:000001".to_owned(), "stock:600001".to_owned()];
    let completion = vec!["worker:1".to_owned(), "worker:0".to_owned()];
    let finalizers = [FinalizerAuditInput {
        auction_finalizations: 1,
        day_end_finalizations: 1,
    }];
    let bad_save = [
        RestoreSlotBytes {
            restored: b"different",
            ..restore("opening")
        },
        restore("partial"),
    ];
    let error = project_observation(
        observation_input(
            &conservation,
            Some(&bad_save),
            Some(b"serialized-save-slot-bytes"),
            &accounts,
            &stocks,
            &completion,
            &finalizers,
        ),
        &TestDigest,
    )
    .unwrap_err();
    assert_eq!(
        error,
        EvidenceError::RestoreBytesMismatch {
            slot: "opening".to_owned()
        }
    );

    let independent_continuation = [
        restore("opening"),
        RestoreSlotBytes {
            restored_continuation: b"different",
            ..restore("partial")
        },
    ];
    let projected = project_observation(
        observation_input(
            &conservation,
            Some(&independent_continuation),
            Some(b"serialized-save-slot-bytes"),
            &accounts,
            &stocks,
            &completion,
            &finalizers,
        ),
        &TestDigest,
    )
    .unwrap();
    let slot = &projected.execution_coverage.restore_slots[1];
    assert_ne!(slot.uninterrupted_continuation, slot.restored_continuation);
}
