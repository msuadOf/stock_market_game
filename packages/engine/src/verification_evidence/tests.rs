use super::*;
use crate::{
    company::CompanyId,
    information::PublicationId,
    session::pipeline::{
        EnvelopeAudit, EnvelopeKey, FeeComponents, ReceiptDelta, ReceiptLocalKey, ReceiptSource,
        ReceiptTransition,
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
    spent: ResVec,
    released: ResVec,
    live_after: ResVec,
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
        qty_after: live_after.shares,
        value_before: Money::ZERO,
        value_after: Money::from_cents(5_000),
        delta: ReceiptDelta::sealed(spent, released, live_after),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    }
}

fn account_snap() -> AccountSnap {
    AccountSnap {
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
        reserved_cash: Money::ZERO,
        reserved_sell_qty: BTreeMap::new(),
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
            ResVec::new(Money::ZERO, 30),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 70),
        ),
        receipt(
            11,
            envelope_key.clone(),
            ReceiptKind::Rollover,
            ReceiptSource::Auction(0),
            0,
            ResVec::ZERO,
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 70),
        ),
    ];
    assert_eq!(
        receipts[0].local_key.source(),
        ReceiptSource::SealedIntent(0)
    );
    assert_eq!(receipts[0].local_key.transition_ordinal(), 0);
    assert_eq!(receipts[0].local_key.transition_envelope(), &envelope_key);
    let mut final_envelope =
        Envelope::p3_created(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    final_envelope
        .apply(receipts[0].delta, audit(100, 70, 30, 5_000))
        .unwrap();
    final_envelope
        .apply(receipts[1].delta, audit(100, 70, 30, 5_000))
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
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn conservation_rejects_source_ordinal_envelope_and_global_identity_tampering() {
    let envelope_key = key(Side::Sell, 91);
    let base_receipts = vec![
        receipt(
            40,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            0,
            ResVec::new(Money::ZERO, 30),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 70),
        ),
        receipt(
            41,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            1,
            ResVec::new(Money::ZERO, 20),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 50),
        ),
    ];
    let mut envelope = Envelope::p3_created(
        envelope_key.clone(),
        Money::ZERO,
        100,
        audit(100, 100, 0, 0),
    );
    envelope
        .apply(base_receipts[0].delta, audit(100, 70, 30, 5_000))
        .unwrap();
    envelope
        .apply(base_receipts[1].delta, audit(100, 50, 50, 8_000))
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
    assert_eq!(
        project(&source_swapped).unwrap_err(),
        EvidenceError::InvalidReceiptIdentity {
            receipt_index: 41,
            detail: "global receipt-index order disagrees with canonical local-key order",
        }
    );

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
    global_index_swap[1].local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::Auction(0),
        ReceiptTransition {
            envelope: envelope_key,
            ordinal: 0,
        },
    )
    .unwrap();
    global_index_swap[0].index = 41;
    global_index_swap[1].index = 40;
    assert_eq!(
        project(&global_index_swap).unwrap_err(),
        EvidenceError::InvalidReceiptIdentity {
            receipt_index: 41,
            detail: "global receipt-index order disagrees with canonical local-key order",
        }
    );
}

#[test]
fn existing_envelope_projects_the_post_preseal_p1_boundary() {
    let envelope_key = key(Side::Sell, 10);
    let receipts = vec![
        receipt(
            20,
            envelope_key.clone(),
            ReceiptKind::Release,
            ReceiptSource::P0Expiry(0),
            0,
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 10),
            ResVec::new(Money::ZERO, 90),
        ),
        receipt(
            21,
            envelope_key.clone(),
            ReceiptKind::Fill,
            ReceiptSource::SealedIntent(0),
            0,
            ResVec::new(Money::ZERO, 30),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 60),
        ),
    ];
    let mut envelope =
        Envelope::tick_start_existing(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    envelope
        .apply(receipts[0].delta, audit(100, 90, 0, 0))
        .unwrap();
    envelope
        .apply(receipts[1].delta, audit(100, 60, 30, 5_000))
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
        p1_live,
    } = &projected.envelopes[0].basis
    else {
        panic!("existing envelope projected as created")
    };
    assert_eq!(tick_start_live.shares, "100");
    assert_eq!(p1_live.shares, "90");
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
            ResVec::new(Money::ZERO, 30),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 70),
        ),
        receipt(
            32,
            envelope_key.clone(),
            ReceiptKind::Rollover,
            ReceiptSource::Auction(0),
            0,
            ResVec::ZERO,
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 70),
        ),
    ];
    let mut envelope = Envelope::p3_created(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    envelope
        .apply(receipts[0].delta, audit(100, 70, 30, 5_000))
        .unwrap();
    envelope
        .apply(receipts[1].delta, audit(100, 70, 30, 5_000))
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

    drop(chain);
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
    drop(duplicate_chain);
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

#[test]
fn phase_six_session_variants_share_one_ordinal_scope() {
    let events = vec![
        Event::ResourceLimit {
            seq: 1,
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit: 10,
        },
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
        "6:ResourceLimit"
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

    assert_eq!(projected.seq_from, 1, "B5 uses an inclusive sequence range");
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
        Event::ResourceLimit {
            seq: 3,
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit: 10,
        },
    ];

    let projected = project_update(RuntimeUpdateRef::Tick(&frame(events))).unwrap();

    assert_eq!(
        projected.events[0].comparison_event_key.1,
        "4:OrderAccepted"
    );
    assert_eq!(projected.events[1].comparison_event_key.1, "5:DayBoundary");
    assert_eq!(
        projected.events[2].comparison_event_key.1,
        "6:ResourceLimit"
    );
}

fn civil_protocol_session() -> ProtocolSession {
    ProtocolSession::new(
        SessionSetup {
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
                    tick_cents: 1,
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
            float_allocation: FloatAllocation::Random,
            start_date: CivilDate::from_iso("2030-01-05").unwrap(),
            simulation_policy_id: crate::SIMULATION_POLICY_ID_V2.to_owned(),
        },
        42,
    )
    .unwrap()
}

#[test]
fn real_civil_update_projects_validated_phase_six_shared_ordinals() {
    let mut session = civil_protocol_session();
    let mut update = session.end_civil_day_update().unwrap();
    let first_seq = update.seq_from.checked_add(1).unwrap();
    let disclosure_seq = first_seq.checked_add(1).unwrap();
    let limit_seq = disclosure_seq.checked_add(1).unwrap();
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
        Event::ResourceLimit {
            seq: limit_seq,
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit: u32::MAX,
        },
    ];
    update.facts = attach_facts(&update.events).unwrap();
    update.seq_to = limit_seq;
    update.refresh.snapshot.seq = limit_seq;
    update.refresh.public_publication_ids = vec![publication_id.value().to_string()];
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
            "6:ResourceLimit",
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
        Event::ResourceLimit {
            seq: 3,
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit: u32::MAX,
        },
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

fn civil_after_tick(duplicate_resource_limit: bool) -> CivilUpdate {
    let mut session = civil_protocol_session();
    let mut update = session.end_civil_day_update().unwrap();
    let date_event = Event::CivilDateAdvanced {
        seq: if duplicate_resource_limit { 5 } else { 4 },
        settled_date: update.boundary.settled_date,
        next_date: update.boundary.next_date,
        next_status: update.boundary.next_status.clone(),
    };
    let disclosure_event = Event::CompanyDisclosurePublished {
        seq: if duplicate_resource_limit { 6 } else { 5 },
        publication_id: PublicationId::new(u32::MAX),
        company: CompanyId("company".to_owned()),
        published_at: CivilInstant::new(update.boundary.next_date, 86_399).unwrap(),
        kind: CompanyDisclosureKind::Report {
            report_revision: u32::MAX,
        },
    };
    update.events = if duplicate_resource_limit {
        vec![
            Event::ResourceLimit {
                seq: 4,
                resource: crate::session::RuntimeResource::PendingPlanEvents,
                limit: u32::MAX,
            },
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
    update.refresh.public_publication_ids = vec![u32::MAX.to_string()];
    update.validate().unwrap();
    update
}

fn price_cage_corpus<'a>(
    case_id: &str,
    stock: &'a StockCode,
    tick: &'a TickFrame,
    civil: &'a CivilUpdate,
) -> Result<CorpusProjection, EvidenceError> {
    project_corpus_surface(
        case_id,
        "cross-update-phase-six",
        7,
        &[RuntimeUpdateRef::Tick(tick), RuntimeUpdateRef::Civil(civil)],
        CorpusSurfaceInput::PriceCage {
            account: AccountId(1),
            stock,
            inside_order: OrderId(41),
        },
    )
}

#[test]
fn corpus_rejects_same_tick_cross_update_ordinal_reset_and_duplicate_comparison_key() {
    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    project_update(RuntimeUpdateRef::Tick(&tick)).unwrap();

    let reset = civil_after_tick(false);
    project_update(RuntimeUpdateRef::Civil(&reset)).unwrap();
    assert_eq!(
        price_cage_corpus("phase-six-reset", &stock, &tick, &reset).unwrap_err(),
        EvidenceError::InvalidEventIdentity {
            detail: "local_event_index is not zero-based and contiguous in its corpus ADR domain",
        }
    );
    let (envelope, receipts, _) = sell_surface_fixture(&[5], &[1], 10, ReceiptSource::Auction(0));
    let account = seller_account(999);
    assert_eq!(
        project_controlled_sell_corpus(
            "controlled-phase-six-reset",
            "cross-update-phase-six",
            7,
            &[
                RuntimeUpdateRef::Tick(&tick),
                RuntimeUpdateRef::Civil(&reset),
            ],
            ControlledSellSurface::AuctionRollover,
            ControlledSellInput {
                envelope: &envelope,
                receipts: &receipts,
                trade_role: TradeRole::TakerSell,
                submission_cash: Money::from_cents(10_000),
                account_after: &account,
                legacy_sell_reservation: Money::from_cents(400),
                cost_basis: SellerCostBasisInput {
                    invested_cents: 10_000,
                    recovered_cents: 1_000,
                },
                feedback: FeedbackAuditInput::default(),
                continuation: continuation(),
            },
            &TestDigest,
        )
        .unwrap_err(),
        EvidenceError::InvalidEventIdentity {
            detail: "local_event_index is not zero-based and contiguous in its corpus ADR domain",
        }
    );

    let duplicate = civil_after_tick(true);
    project_update(RuntimeUpdateRef::Civil(&duplicate)).unwrap();
    assert_eq!(
        price_cage_corpus("duplicate-comparison-key", &stock, &tick, &duplicate).unwrap_err(),
        EvidenceError::InvalidEventIdentity {
            detail: "comparison_event_key is duplicated across corpus updates",
        }
    );
}

#[test]
fn corpus_accepts_same_tick_cross_update_ordinals_that_continue_from_prior_update() {
    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    let mut civil = civil_after_tick(false);
    for (offset, fact) in civil.facts.iter_mut().enumerate() {
        fact.key = crate::session::pipeline::EventStableKey::for_event(
            &fact.event,
            u64::try_from(offset).unwrap() + 1,
        );
    }
    civil.validate().unwrap();

    assert_eq!(
        project_update(RuntimeUpdateRef::Civil(&civil)).unwrap_err(),
        EvidenceError::InvalidEventIdentity {
            detail: "local_event_index is not zero-based and contiguous in its ADR domain",
        },
        "standalone update projection deliberately owns only a fresh local identity stream"
    );

    let projected = price_cage_corpus("phase-six-continuation", &stock, &tick, &civil).unwrap();
    assert_eq!(projected.updates[0].events[2].comparison_event_key.3, "0");
    assert_eq!(projected.updates[1].events[0].comparison_event_key.3, "1");
    assert_eq!(projected.updates[1].events[1].comparison_event_key.3, "2");
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

fn civil_with_continued_phase_six_ordinals() -> CivilUpdate {
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
fn full_update_stream_projection_shares_same_tick_phase_six_session_ordinals() {
    let stock = StockCode("600001".to_owned());
    let tick = tick_before_civil(&stock);
    let civil = civil_with_continued_phase_six_ordinals();

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
    let civil = civil_with_continued_phase_six_ordinals();
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
    let civil = civil_with_continued_phase_six_ordinals();
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
    let first = frame(vec![Event::ResourceLimit {
        seq: 1,
        resource: crate::session::RuntimeResource::PendingPlanEvents,
        limit: 10,
    }]);
    let events = vec![Event::ResourceLimit {
        seq: 2,
        resource: crate::session::RuntimeResource::PendingPlanEvents,
        limit: 10,
    }];
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
fn event_projection_rejects_gapped_local_indices_and_adr_key_tampering() {
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
        Event::OrderAccepted {
            seq: 2,
            account: AccountId(1),
            code: StockCode("600001".to_owned()),
            id: OrderId(10),
            side: Side::Buy,
            price: Money::from_cents(999),
            remaining_qty: 100,
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
        ("source", serde_json::json!("P0")),
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
            turnover_cents: u64::MAX,
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
        Event::ResourceLimit {
            seq: u64::MAX,
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit: u32::MAX,
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

    let key = crate::session::pipeline::EventStableKey::for_event(&events[11], u64::MAX);
    assert_no_json_numbers(&project_stable_key(&key).unwrap());
    assert_no_json_numbers(&project_candle(&extreme_candle).unwrap());
    assert_no_json_numbers(&Value::Array(
        project_depth(&[(Money::from_cents(i64::MAX), u64::MAX)], "test.depth").unwrap(),
    ));

    let timeseries = TickTimeseriesPayload {
        markets: BTreeMap::from([(
            code.clone(),
            MarketSnap {
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
            ResVec::new(Money::ZERO, qty),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, live),
        ));
    }
    if live > 0 {
        receipts.push(receipt(
            u64::try_from(receipts.len()).unwrap(),
            envelope_key.clone(),
            ReceiptKind::Release,
            ReceiptSource::DayEnd(0),
            0,
            ResVec::ZERO,
            ResVec::new(Money::ZERO, live),
            ResVec::ZERO,
        ));
        live = 0;
    }
    let mut envelope = Envelope::p3_created(envelope_key, Money::ZERO, 100, audit(100, 100, 0, 0));
    for row in &receipts {
        envelope
            .apply(row.delta, audit(100, live, filled, i64::from(filled) * 100))
            .unwrap();
    }
    let account = AccountSnap {
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
        reserved_cash: Money::ZERO,
        reserved_sell_qty: BTreeMap::new(),
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
            Some(b"save-v2-bytes"),
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
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn observation_rejects_restore_byte_and_continuation_mismatches() {
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
            Some(b"save-v2-bytes"),
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

    let bad_continuation = [
        restore("opening"),
        RestoreSlotBytes {
            restored_continuation: b"different",
            ..restore("partial")
        },
    ];
    let error = project_observation(
        observation_input(
            &conservation,
            Some(&bad_continuation),
            Some(b"save-v2-bytes"),
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
        EvidenceError::RestoreContinuationMismatch {
            slot: "partial".to_owned()
        }
    );
}

fn buyer_surface_fixture(spent_cents: i64) -> (Envelope, Vec<EnvelopeReceipt>, TickFrame) {
    let envelope_key = key(Side::Buy, 40);
    let charged = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::ZERO,
        transfer_fee: Money::from_cents(1),
    };
    let mut fill = receipt(
        0,
        envelope_key.clone(),
        ReceiptKind::Fill,
        ReceiptSource::SealedIntent(0),
        0,
        ResVec::new(Money::from_cents(spent_cents), 0),
        ResVec::ZERO,
        ResVec::ZERO,
    );
    fill.qty_before = 100;
    fill.qty_after = 0;
    fill.value_before = Money::ZERO;
    fill.value_after = Money::from_cents(100_000);
    fill.nominal = charged;
    fill.charged = charged;
    fill.charged_after = charged;
    fill.deliver_qty = 100;
    let mut envelope = Envelope::p3_created(
        envelope_key,
        Money::from_cents(spent_cents),
        0,
        audit(1_000, 100, 0, 0),
    );
    envelope
        .apply(
            fill.delta,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 0,
                filled_qty: 100,
                filled_value: Money::from_cents(100_000),
                nominal: charged,
                charged,
            },
        )
        .unwrap();
    let update = frame(vec![Event::Trade {
        seq: 1,
        code: StockCode("600001".to_owned()),
        price: Money::from_cents(1_000),
        qty: 100,
        maker: AccountId(2),
        taker: AccountId(1),
    }]);
    (envelope, vec![fill], update)
}

#[test]
fn buyer_fee_surface_is_derived_from_fill_receipts_and_trade_facts() {
    let (envelope, receipts, update) = buyer_surface_fixture(100_501);
    let projected = project_corpus_surface(
        "buyer-fees",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&update)],
        CorpusSurfaceInput::BuyerFees {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerBuy,
        },
    )
    .unwrap();

    assert_eq!(projected.class, "equivalence");
    assert_eq!(projected.corpus_control["surface"], "buyer-fees");
    assert_eq!(
        projected.state["buyer_fee_control"]["spent_cash_cents"],
        "100501"
    );
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());

    let (bad_envelope, bad_receipts, bad_update) = buyer_surface_fixture(100_500);
    assert!(matches!(
        project_corpus_surface(
            "buyer-fees-bad",
            "auction-rollover",
            7,
            &[RuntimeUpdateRef::Tick(&bad_update)],
            CorpusSurfaceInput::BuyerFees {
                envelope: &bad_envelope,
                receipts: &bad_receipts,
                trade_role: TradeRole::TakerBuy,
            },
        ),
        Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "buyer-fees",
            detail: "spent cash does not equal gross plus charged buyer fees"
        })
    ));
}

#[test]
fn t1_surface_requires_the_actual_bought_quantity_to_become_locked() {
    let (envelope, receipts, update) = buyer_surface_fixture(100_501);
    let before = PositionSnap {
        qty: 100,
        t1_locked: 10,
        invested_cents: 100_000,
        recovered_cents: 0,
    };
    let after = PositionSnap {
        qty: 200,
        t1_locked: 110,
        invested_cents: 200_000,
        recovered_cents: 0,
    };
    let projected = project_corpus_surface(
        "t1",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&update)],
        CorpusSurfaceInput::T1 {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerBuy,
            before: &before,
            after: &after,
        },
    )
    .unwrap();
    assert_eq!(projected.state["t1_control"]["bought_qty"], "100");
    assert_eq!(projected.state["t1_control"]["t1_locked_after"], "110");
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());

    let bad_after = PositionSnap {
        t1_locked: 109,
        ..after
    };
    assert!(matches!(
        project_corpus_surface(
            "t1-bad",
            "auction-rollover",
            7,
            &[RuntimeUpdateRef::Tick(&update)],
            CorpusSurfaceInput::T1 {
                envelope: &envelope,
                receipts: &receipts,
                trade_role: TradeRole::TakerBuy,
                before: &before,
                after: &bad_after,
            },
        ),
        Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "t1",
            detail: "position snapshots do not prove bought shares became T+1 locked"
        })
    ));
}

#[test]
fn price_cage_and_continuous_buy_surfaces_bind_runtime_event_identities() {
    let stock = StockCode("600001".to_owned());
    let cage_update = frame(vec![
        Event::IntentRejected {
            seq: 1,
            account: AccountId(1),
            code: stock.clone(),
            reason: crate::RejectionReason::PriceCageExceeded,
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
    ]);
    let cage = project_corpus_surface(
        "price-cage",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&cage_update)],
        CorpusSurfaceInput::PriceCage {
            account: AccountId(1),
            stock: &stock,
            inside_order: OrderId(41),
        },
    )
    .unwrap();
    assert_eq!(cage.corpus_control["surface"], "price-cage");
    assert_no_json_numbers(&serde_json::to_value(&cage).unwrap());

    let (envelope, receipts, _) = buyer_surface_fixture(100_501);
    let continuous_update = frame(vec![
        Event::OrderAccepted {
            seq: 1,
            account: AccountId(1),
            code: stock,
            id: OrderId(40),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            remaining_qty: 100,
        },
        Event::Trade {
            seq: 2,
            code: StockCode("600001".to_owned()),
            price: Money::from_cents(1_000),
            qty: 100,
            maker: AccountId(2),
            taker: AccountId(1),
        },
    ]);
    let continuous = project_corpus_surface(
        "continuous-buy",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&continuous_update)],
        CorpusSurfaceInput::ContinuousBuyLeg {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerBuy,
        },
    )
    .unwrap();
    assert_eq!(continuous.corpus_control["surface"], "continuous-buy-leg");
    assert_eq!(
        continuous.state["continuous_buy_leg_control"]["order_id"],
        "40"
    );
    assert_no_json_numbers(&serde_json::to_value(&continuous).unwrap());
}

fn seller_account(cash: i64) -> AccountSnap {
    AccountSnap {
        cash: Money::from_cents(cash),
        positions: BTreeMap::from([(
            StockCode("600001".to_owned()),
            PositionSnap {
                qty: 100,
                t1_locked: 0,
                invested_cents: 10_000,
                recovered_cents: 0,
            },
        )]),
        reserved_cash: Money::ZERO,
        reserved_sell_qty: BTreeMap::new(),
    }
}

fn sell_surface_fixture(
    nominal_deltas: &[i64],
    charged_deltas: &[i64],
    remaining: u32,
    source: ReceiptSource,
) -> (Envelope, Vec<EnvelopeReceipt>, Vec<Event>) {
    assert_eq!(nominal_deltas.len(), charged_deltas.len());
    let leg_qty = 10u32;
    let original_qty = u32::try_from(nominal_deltas.len()).unwrap() * leg_qty + remaining;
    let envelope_key = key(Side::Sell, 50);
    let mut envelope = Envelope::p3_created(
        envelope_key.clone(),
        Money::ZERO,
        original_qty,
        audit(100, original_qty, 0, 0),
    );
    let mut receipts = Vec::new();
    let mut events = Vec::new();
    let mut nominal_total = 0i64;
    let mut charged_total = 0i64;
    let mut value_total = 0i64;
    let mut live = original_qty;
    for (ordinal, (&nominal, &charged)) in nominal_deltas.iter().zip(charged_deltas).enumerate() {
        let before_qty = live;
        live -= leg_qty;
        let value_before = value_total;
        value_total += 1_000;
        let charged_before = charged_total;
        nominal_total += nominal;
        charged_total += charged;
        let mut fill = receipt(
            u64::try_from(ordinal).unwrap(),
            envelope_key.clone(),
            ReceiptKind::Fill,
            source,
            u64::try_from(ordinal).unwrap(),
            ResVec::new(Money::ZERO, leg_qty),
            ResVec::ZERO,
            ResVec::new(Money::ZERO, live),
        );
        fill.qty_before = before_qty;
        fill.qty_after = live;
        fill.value_before = Money::from_cents(value_before);
        fill.value_after = Money::from_cents(value_total);
        fill.nominal = FeeComponents {
            commission: Money::from_cents(nominal),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        };
        fill.charged = FeeComponents {
            commission: Money::from_cents(charged),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        };
        fill.charged_before = FeeComponents {
            commission: Money::from_cents(charged_before),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        };
        fill.charged_after = FeeComponents {
            commission: Money::from_cents(charged_total),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        };
        fill.deliver_cash = Money::from_cents(1_000 - charged);
        envelope
            .apply(
                fill.delta,
                EnvelopeAudit {
                    limit: Money::from_cents(100),
                    remaining_qty: live,
                    filled_qty: original_qty - live,
                    filled_value: Money::from_cents(value_total),
                    nominal: FeeComponents {
                        commission: Money::from_cents(nominal_total),
                        stamp_tax: Money::ZERO,
                        transfer_fee: Money::ZERO,
                    },
                    charged: fill.charged_after,
                },
            )
            .unwrap();
        receipts.push(fill);
        events.push(Event::Trade {
            seq: u64::try_from(ordinal + 1).unwrap(),
            code: StockCode("600001".to_owned()),
            price: Money::from_cents(100),
            qty: leg_qty,
            maker: AccountId(2),
            taker: AccountId(1),
        });
    }
    (envelope, receipts, events)
}

#[test]
fn normal_multi_leg_terminal_is_proven_from_sell_receipts() {
    let (envelope, receipts, events) =
        sell_surface_fixture(&[5, 5], &[5, 5], 0, ReceiptSource::SealedIntent(0));
    let update = frame(events);
    let account = seller_account(1_990);
    let projected = project_corpus_surface(
        "normal-seller",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&update)],
        CorpusSurfaceInput::NormalMultiLegTerminal {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerSell,
            submission_cash: Money::ZERO,
            account_after: &account,
            legacy_sell_reservation: Money::ZERO,
            feedback: FeedbackAuditInput::default(),
        },
    )
    .unwrap();

    assert_eq!(projected.class, "equivalence");
    assert_eq!(
        projected.corpus_control["surface"],
        "normal-multi-leg-terminal"
    );
    assert_eq!(
        projected.corpus_control["fee_prefixes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(projected.state["net_delivery_cents"], "1990");
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());

    let (short_envelope, short_receipts, short_events) =
        sell_surface_fixture(&[5, 5], &[1, 5], 0, ReceiptSource::SealedIntent(0));
    assert!(project_corpus_surface(
        "normal-seller-short",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&frame(short_events))],
        CorpusSurfaceInput::NormalMultiLegTerminal {
            envelope: &short_envelope,
            receipts: &short_receipts,
            trade_role: TradeRole::TakerSell,
            submission_cash: Money::ZERO,
            account_after: &account,
            legacy_sell_reservation: Money::ZERO,
            feedback: FeedbackAuditInput::default(),
        },
    )
    .is_err());
}

#[test]
fn acceptance_flip_is_bound_to_current_sell_acceptance_and_frozen_legacy_reservation() {
    let stock = StockCode("600001".to_owned());
    let accepted = frame(vec![Event::OrderAccepted {
        seq: 1,
        account: AccountId(1),
        code: stock.clone(),
        id: OrderId(50),
        side: Side::Sell,
        price: Money::from_cents(100),
        remaining_qty: 100,
    }]);
    let account = seller_account(0);
    let projected = project_corpus_surface(
        "acceptance-flip",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&accepted)],
        CorpusSurfaceInput::AcceptanceFlip {
            account: AccountId(1),
            stock: &stock,
            trade_role: TradeRole::TakerSell,
            submission_cash: Money::ZERO,
            account_after: &account,
            legacy_sell_reservation: Money::from_cents(400),
            feedback: FeedbackAuditInput::default(),
        },
    )
    .unwrap();
    assert_eq!(projected.class, "divergence-9");
    assert_eq!(projected.state["acceptance"], "accepted");
    assert_eq!(projected.state["reserved_cash"], "0");
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn three_leg_fee_catchup_requires_a_real_shortfall_prefix() {
    let (envelope, receipts, events) =
        sell_surface_fixture(&[5, 5, 6], &[1, 1, 7], 0, ReceiptSource::SealedIntent(0));
    let update = frame(events);
    let account = seller_account(2_991);
    let projected = project_corpus_surface(
        "three-leg",
        "auction-rollover",
        7,
        &[RuntimeUpdateRef::Tick(&update)],
        CorpusSurfaceInput::ThreeLegFeeCatchup {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerSell,
            account_after: &account,
            legacy_sell_reservation: Money::from_cents(400),
            feedback: FeedbackAuditInput::default(),
        },
    )
    .unwrap();

    let prefixes = projected.corpus_control["fee_prefixes"].as_array().unwrap();
    assert_eq!(prefixes.len(), 3);
    assert_eq!(prefixes[0]["nominal_cents"], "5");
    assert_eq!(prefixes[0]["charged_cents"], "1");
    assert_eq!(prefixes[2]["charged_cents"], "9");
    assert_eq!(projected.state["net_delivery_cents"], "2991");
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

fn frame_at(tick: u64, seq_from: u64, events: Vec<Event>) -> TickFrame {
    let seq_to = seq_from + u64::try_from(events.len()).unwrap();
    TickFrame {
        tick,
        facts: attach_facts(&events).unwrap(),
        events,
        timeseries_payload: TickTimeseriesPayload::default(),
        seq_from,
        seq_to,
    }
}

fn continuation() -> ControlledContinuationBytes<'static> {
    ControlledContinuationBytes {
        sealed_exogenous_script: b"sealed-script",
        strategy_state: b"strategy-state",
        plan_state: b"plan-state",
        pending_intents: b"pending-intents",
        restore_order: b"restore-order",
        rng_cursor: 17,
    }
}

#[test]
fn auction_rollover_control_uses_receipts_account_state_and_real_control_bytes() {
    let (mut envelope, mut receipts, events) =
        sell_surface_fixture(&[5], &[1], 10, ReceiptSource::Auction(0));
    let rollover = receipt(
        1,
        envelope.key().clone(),
        ReceiptKind::Rollover,
        ReceiptSource::Auction(0),
        1,
        ResVec::ZERO,
        ResVec::ZERO,
        envelope.live(),
    );
    envelope.apply(rollover.delta, envelope.audit()).unwrap();
    receipts.push(rollover);
    let account = seller_account(999);
    let first = frame_at(5, 0, events);
    let continuation_update = frame_at(
        6,
        1,
        vec![Event::PriceTick {
            seq: 2,
            tick: 6,
            code: StockCode("600001".to_owned()),
            last_price: Money::from_cents(100),
            daily_candle: candle(),
            bids: vec![],
            asks: vec![],
        }],
    );
    let projected = project_controlled_sell_corpus(
        "auction-rollover",
        "auction-rollover",
        7,
        &[
            RuntimeUpdateRef::Tick(&first),
            RuntimeUpdateRef::Tick(&continuation_update),
        ],
        ControlledSellSurface::AuctionRollover,
        ControlledSellInput {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerSell,
            submission_cash: Money::from_cents(10_000),
            account_after: &account,
            legacy_sell_reservation: Money::from_cents(400),
            cost_basis: SellerCostBasisInput {
                invested_cents: 10_000,
                recovered_cents: 1_000,
            },
            feedback: FeedbackAuditInput::default(),
            continuation: continuation(),
        },
        &TestDigest,
    )
    .unwrap();

    assert_eq!(projected.class, "controlled-live-sell");
    assert_eq!(projected.corpus_control["surface"], "auction-rollover");
    assert_eq!(
        projected.corpus_control["comparison_points"],
        serde_json::json!(["post-auction-rollover", "post-continuation-tick"])
    );
    assert_eq!(projected.state["live_shares"], "10");
    assert_eq!(
        projected.seller_fee_control.as_ref().unwrap()["gross_cents"],
        "1000"
    );
    assert_eq!(
        projected.corpus_control["sealed_exogenous_script_sha256"],
        TestDigest.digest_hex(b"sealed-script")
    );
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn cross_tick_partial_fill_requires_trades_in_two_runtime_updates() {
    let (envelope, receipts, mut events) =
        sell_surface_fixture(&[5, 5], &[1, 4], 10, ReceiptSource::SealedIntent(0));
    let second = events.pop().unwrap();
    let first = events.pop().unwrap();
    let updates = [frame_at(5, 0, vec![first]), frame_at(6, 1, vec![second])];
    let account = seller_account(1_995);
    let projected = project_controlled_sell_corpus(
        "cross-tick",
        "auction-rollover",
        7,
        &[
            RuntimeUpdateRef::Tick(&updates[0]),
            RuntimeUpdateRef::Tick(&updates[1]),
        ],
        ControlledSellSurface::CrossTickPartialFill,
        ControlledSellInput {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerSell,
            submission_cash: Money::from_cents(10_000),
            account_after: &account,
            legacy_sell_reservation: Money::from_cents(400),
            cost_basis: SellerCostBasisInput {
                invested_cents: 10_000,
                recovered_cents: 2_000,
            },
            feedback: FeedbackAuditInput::default(),
            continuation: continuation(),
        },
        &TestDigest,
    )
    .unwrap();

    assert_eq!(projected.updates.len(), 2);
    assert_eq!(
        projected.corpus_control["comparison_points"],
        serde_json::json!(["post-partial-fill", "post-continuation-tick"])
    );
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());

    let error = project_controlled_sell_corpus(
        "cross-tick-feedback",
        "auction-rollover",
        7,
        &[
            RuntimeUpdateRef::Tick(&updates[0]),
            RuntimeUpdateRef::Tick(&updates[1]),
        ],
        ControlledSellSurface::CrossTickPartialFill,
        ControlledSellInput {
            envelope: &envelope,
            receipts: &receipts,
            trade_role: TradeRole::TakerSell,
            submission_cash: Money::from_cents(10_000),
            account_after: &account,
            legacy_sell_reservation: Money::from_cents(400),
            cost_basis: SellerCostBasisInput {
                invested_cents: 10_000,
                recovered_cents: 2_000,
            },
            feedback: FeedbackAuditInput {
                strategy_generated_intents: 1,
                ..FeedbackAuditInput::default()
            },
            continuation: continuation(),
        },
        &TestDigest,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        EvidenceError::CorpusEvidenceMismatch {
            surface: "cross-tick-partial-fill",
            detail: "feedback audit is non-zero for a controlled corpus surface"
        }
    ));
}

struct SaveRestoreFixture {
    frames: Vec<TickFrame>,
    saved_bytes: Vec<u8>,
    restored_resave_bytes: Vec<u8>,
    uninterrupted_authority: Vec<u8>,
    restored_authority: Vec<u8>,
    continuation_input: Vec<u8>,
    uninterrupted_continuation: Vec<u8>,
    restored_continuation: Vec<u8>,
    no_live_save_bytes: Vec<u8>,
    no_live_authority: Vec<u8>,
    stock: StockCode,
    order: OrderId,
}

impl SaveRestoreFixture {
    fn input(&self) -> SaveRestoreLiveOrderInput<'_> {
        SaveRestoreLiveOrderInput {
            saved_bytes: &self.saved_bytes,
            restored_resave_bytes: &self.restored_resave_bytes,
            uninterrupted_authority: &self.uninterrupted_authority,
            restored_authority: &self.restored_authority,
            continuation_input: &self.continuation_input,
            uninterrupted_continuation: &self.uninterrupted_continuation,
            restored_continuation: &self.restored_continuation,
            expected: SaveLiveOrderIdentity {
                account: AccountId(0),
                stock: &self.stock,
                order: self.order,
                side: Side::Sell,
            },
            legacy_sell_reservation: Money::from_cents(500),
            sha256: &TestDigest,
        }
    }

    fn updates(&self) -> Vec<RuntimeUpdateRef<'_>> {
        self.frames.iter().map(RuntimeUpdateRef::Tick).collect()
    }
}

fn save_restore_setup() -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600001".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: SecurityCategory::MainBoard.limit_pct(),
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
                tick_cents: 1,
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
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-07").unwrap(),
        simulation_policy_id: crate::SIMULATION_POLICY_ID_V2.to_owned(),
    }
}

fn real_save_restore_fixture() -> SaveRestoreFixture {
    let stock = StockCode("600001".to_owned());
    let pristine = crate::GameSession::new(save_restore_setup(), 7).unwrap();
    let mut seeded = pristine.save().unwrap();
    seeded
        .snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            stock.clone(),
            PositionSnap {
                qty: 200,
                t1_locked: 0,
                invested_cents: 200_000,
                recovered_cents: 0,
            },
        );
    let mut uninterrupted = crate::GameSession::restore(&seeded).unwrap();
    uninterrupted
        .enqueue_player_intent(
            AccountId(0),
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: Money::from_cents(1_050),
                qty: 100,
            },
        )
        .unwrap();
    let pre_save_frame = uninterrupted.step_frame().unwrap();
    let order = pre_save_frame
        .events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                account,
                code,
                id,
                side: Side::Sell,
                ..
            } if *account == AccountId(0) && code == &stock => Some(*id),
            _ => None,
        })
        .expect("the public player path must create a real resting Sell order");

    let saved_bytes = serde_json::to_vec(&uninterrupted.save().unwrap()).unwrap();
    let decoded = crate::decode_save_slot(&saved_bytes, &crate::SaveDecodeLimits::default())
        .expect("the real v2 fixture must decode through the public boundary");
    let mut restored = crate::GameSession::restore(&decoded).unwrap();
    let restored_resave_bytes = serde_json::to_vec(&restored.save().unwrap()).unwrap();
    assert_eq!(saved_bytes, restored_resave_bytes);

    let uninterrupted_authority =
        serde_json::to_vec(&uninterrupted.business_state_hash().unwrap()).unwrap();
    let restored_authority = serde_json::to_vec(&restored.business_state_hash().unwrap()).unwrap();
    assert_eq!(uninterrupted_authority, restored_authority);

    let continuation = SaveRestoreContinuationCommand {
        account: AccountId(0),
        intent: crate::Intent::Cancel {
            code: stock.clone(),
            id: order,
        },
    };
    let continuation_input = serde_json::to_vec(&continuation).unwrap();
    uninterrupted
        .enqueue_player_intent(continuation.account, continuation.intent.clone())
        .unwrap();
    restored
        .enqueue_player_intent(continuation.account, continuation.intent)
        .unwrap();
    let uninterrupted_frame = uninterrupted.step_frame().unwrap();
    let restored_frame = restored.step_frame().unwrap();
    let no_live_save_bytes = serde_json::to_vec(&uninterrupted.save().unwrap()).unwrap();
    let restored_final_save = serde_json::to_vec(&restored.save().unwrap()).unwrap();
    assert_eq!(no_live_save_bytes, restored_final_save);
    let uninterrupted_continuation =
        serde_json::to_vec(&(&uninterrupted_frame, &no_live_save_bytes)).unwrap();
    let restored_continuation =
        serde_json::to_vec(&(&restored_frame, &restored_final_save)).unwrap();
    assert_eq!(uninterrupted_continuation, restored_continuation);
    let no_live_decoded =
        crate::decode_save_slot(&no_live_save_bytes, &crate::SaveDecodeLimits::default()).unwrap();
    let no_live_restored = crate::GameSession::restore(&no_live_decoded).unwrap();
    let no_live_authority =
        serde_json::to_vec(&no_live_restored.business_state_hash().unwrap()).unwrap();

    SaveRestoreFixture {
        frames: vec![pre_save_frame, uninterrupted_frame],
        saved_bytes,
        restored_resave_bytes,
        uninterrupted_authority,
        restored_authority,
        continuation_input,
        uninterrupted_continuation,
        restored_continuation,
        no_live_save_bytes,
        no_live_authority,
        stock,
        order,
    }
}

fn project_save_restore(
    fixture: &SaveRestoreFixture,
    input: SaveRestoreLiveOrderInput<'_>,
) -> Result<CorpusProjection, EvidenceError> {
    project_corpus_surface(
        "save-case",
        "save-restore-live-order",
        7,
        &fixture.updates(),
        CorpusSurfaceInput::SaveRestoreLiveOrder(input),
    )
}

#[test]
fn real_save_v2_live_sell_projects_restore_and_continuation_evidence() {
    let fixture = real_save_restore_fixture();
    let projected = project_save_restore(&fixture, fixture.input()).unwrap();

    assert_eq!(projected.class, "controlled-live-sell");
    assert_eq!(projected.state["save_representation"]["schema"], "v2");
    let evidence = &projected.state["save_restore_live_order_control"];
    assert_eq!(evidence["schema_version"], "2");
    assert_eq!(evidence["live_order"]["account_id"], "0");
    assert_eq!(evidence["live_order"]["stock_code"], "600001");
    assert_eq!(
        evidence["live_order"]["order_id"],
        fixture.order.0.to_string()
    );
    assert_eq!(evidence["live_order"]["side"], "Sell");
    assert_eq!(evidence["live_order"]["remaining_qty"], "100");
    assert_eq!(evidence["live_order"]["live_cash_cents"], "0");
    assert_eq!(evidence["live_order"]["live_shares"], "100");
    for artifact in [
        "saved",
        "restored_resave",
        "uninterrupted_authority",
        "restored_authority",
        "continuation_input",
        "uninterrupted_continuation",
        "restored_continuation",
    ] {
        assert!(evidence[artifact]["byte_length"].is_string());
        assert_eq!(evidence[artifact]["sha256"].as_str().unwrap().len(), 64);
    }
    assert_eq!(
        projected.corpus_control["comparison_points"],
        serde_json::json!(["pre-save", "post-restore", "post-continuation-tick"])
    );
    assert!(projected.seller_fee_control.is_some());
    assert_no_json_numbers(&serde_json::to_value(&projected).unwrap());
}

#[test]
fn save_restore_surface_rejects_each_empty_artifact_input() {
    let fixture = real_save_restore_fixture();
    let fields = [
        "saved_bytes",
        "restored_resave_bytes",
        "uninterrupted_authority",
        "restored_authority",
        "continuation_input",
        "uninterrupted_continuation",
        "restored_continuation",
    ];
    for field in fields {
        let mut input = fixture.input();
        match field {
            "saved_bytes" => input.saved_bytes = &[],
            "restored_resave_bytes" => input.restored_resave_bytes = &[],
            "uninterrupted_authority" => input.uninterrupted_authority = &[],
            "restored_authority" => input.restored_authority = &[],
            "continuation_input" => input.continuation_input = &[],
            "uninterrupted_continuation" => input.uninterrupted_continuation = &[],
            "restored_continuation" => input.restored_continuation = &[],
            _ => unreachable!(),
        }
        assert_eq!(
            project_save_restore(&fixture, input).unwrap_err(),
            EvidenceError::EmptyField { field },
        );
    }
}

#[test]
fn save_restore_surface_rejects_legacy_schema_before_restore() {
    let fixture = real_save_restore_fixture();
    let mut legacy: Value = serde_json::from_slice(&fixture.saved_bytes).unwrap();
    legacy["schema_version"] = serde_json::json!(1);
    let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
    let mut input = fixture.input();
    input.saved_bytes = &legacy_bytes;

    assert!(matches!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::InvalidSaveV2 {
            stage: "decode",
            ..
        }
    ));
}

#[test]
fn save_restore_surface_rejects_valid_v2_save_without_a_live_order() {
    let fixture = real_save_restore_fixture();
    let mut input = fixture.input();
    input.saved_bytes = &fixture.no_live_save_bytes;
    input.restored_resave_bytes = &fixture.no_live_save_bytes;
    input.uninterrupted_authority = &fixture.no_live_authority;
    input.restored_authority = &fixture.no_live_authority;

    assert_eq!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "save v2 contains no live order",
        }
    );
}

#[test]
fn save_restore_surface_rejects_restore_resave_byte_drift() {
    let fixture = real_save_restore_fixture();
    let drifted = b"different-resave";
    let mut input = fixture.input();
    input.restored_resave_bytes = drifted;

    assert_eq!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "restored re-save bytes differ from the canonical save",
        }
    );
}

#[test]
fn save_restore_surface_rejects_authority_drift() {
    let fixture = real_save_restore_fixture();
    let drifted = b"different-authority";
    let mut input = fixture.input();
    input.restored_authority = drifted;

    assert_eq!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "restored authority bytes differ from the public restored session",
        }
    );
}

#[test]
fn save_restore_surface_rejects_continuation_drift() {
    let fixture = real_save_restore_fixture();
    let drifted = b"different-continuation";
    let mut input = fixture.input();
    input.restored_continuation = drifted;

    assert_eq!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "restored continuation bytes differ from the public replay",
        }
    );
}

#[test]
fn save_restore_surface_rejects_equal_fabricated_continuation_results() {
    let fixture = real_save_restore_fixture();
    let fabricated = b"same-fabricated-continuation";
    let mut input = fixture.input();
    input.uninterrupted_continuation = fabricated;
    input.restored_continuation = fabricated;

    assert_eq!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "uninterrupted continuation bytes differ from the public replay",
        }
    );
}

#[test]
fn save_restore_surface_rejects_malformed_or_unbound_continuation_commands() {
    let fixture = real_save_restore_fixture();

    let mut malformed = fixture.input();
    malformed.continuation_input = b"{}";
    assert!(matches!(
        project_save_restore(&fixture, malformed).unwrap_err(),
        EvidenceError::InvalidSaveV2 {
            stage: "continuation-decode",
            ..
        }
    ));

    let wrong_account_bytes = serde_json::to_vec(&SaveRestoreContinuationCommand {
        account: AccountId(1),
        intent: crate::Intent::Cancel {
            code: fixture.stock.clone(),
            id: fixture.order,
        },
    })
    .unwrap();
    let mut wrong_account = fixture.input();
    wrong_account.continuation_input = &wrong_account_bytes;
    assert_eq!(
        project_save_restore(&fixture, wrong_account).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "continuation account differs from the expected live order account",
        }
    );

    let wrong_order_bytes = serde_json::to_vec(&SaveRestoreContinuationCommand {
        account: AccountId(0),
        intent: crate::Intent::Cancel {
            code: fixture.stock.clone(),
            id: OrderId(fixture.order.0 + 1),
        },
    })
    .unwrap();
    let mut wrong_order = fixture.input();
    wrong_order.continuation_input = &wrong_order_bytes;
    assert_eq!(
        project_save_restore(&fixture, wrong_order).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "continuation must cancel the expected live order",
        }
    );
}

#[test]
fn save_restore_surface_rejects_expected_identity_mismatch() {
    let fixture = real_save_restore_fixture();
    let mut input = fixture.input();
    input.expected.order = OrderId(fixture.order.0 + 1);

    assert_eq!(
        project_save_restore(&fixture, input).unwrap_err(),
        EvidenceError::CorpusEvidenceMismatch {
            surface: "save-restore-live-order",
            detail: "expected live order identity is absent from the saved order book",
        }
    );
}
