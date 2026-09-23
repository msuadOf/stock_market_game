use engine::session::pipeline::*;
use engine::{AccountId, Event, Money, OrderId, Side, StockCode, TradingPhase};

fn facts() -> Vec<Event> {
    vec![
        Event::OrderAccepted {
            seq: 1,
            account: AccountId(2),
            code: StockCode("600001".into()),
            id: OrderId(1),
            side: Side::Buy,
            price: Money::from_cents(1000),
            remaining_qty: 100,
        },
        Event::AuctionTick {
            seq: 2,
            tick: 1,
            phase: TradingPhase::CallAuction,
            code: StockCode("600001".into()),
            indicative_price: None,
            matched_volume: 0,
            imbalance: 100,
        },
    ]
}

#[test]
fn step_phases_have_fixed_complete_rank_mapping() {
    let observed: Vec<_> = TickPhase::ALL
        .into_iter()
        .map(|phase| (phase.rank(), phase.name()))
        .collect();
    assert_eq!(
        observed,
        vec![
            (0, "expiry_shadow"),
            (1, "seal_allocation_snapshot"),
            (2, "decision_shadow"),
            (3, "account_validation"),
            (4, "stock_processing"),
            (5, "receipt_aggregation"),
            (6, "settlement_shadow"),
            (7, "derivation_audit"),
            (8, "dual_hash_check"),
            (9, "commit_tick")
        ]
    );
}

#[test]
fn attached_event_identity_survives_fact_permutation() {
    let events = facts();
    let mut keyed = EventKeyStream::default()
        .attach_legacy_emission(&events)
        .unwrap();
    let mut expected = keyed.clone();
    expected.sort_by(|left, right| left.key.cmp(&right.key));
    keyed.reverse();
    keyed.sort_by(|left, right| left.key.cmp(&right.key));
    assert_eq!(keyed, expected);
    assert_eq!(
        keyed[0].key.entity(),
        &EntityTag::Stock(StockCode("600001".into()))
    );
    assert_eq!(keyed[1].key.entity(), &EntityTag::Account(AccountId(2)));
}

#[test]
fn duplicate_identity_is_rejected_even_with_same_payload() {
    let events = facts();
    let keyed = EventKeyStream::default()
        .attach_legacy_emission(&events)
        .unwrap();
    for payload in [&events[0], &events[1]] {
        let duplicate = KeyedEvent {
            key: keyed[0].key.clone(),
            event: payload,
        };
        assert!(matches!(
            validate_event_keys(&[keyed[0].clone(), duplicate]),
            Err(engine::session::StepFatal::InvariantViolation { .. })
        ));
    }
}

#[test]
fn session_variants_share_one_ordinal_domain() {
    use engine::calendar::{CivilDate, CivilInstant, DayStatus};
    use engine::session::{CompanyDisclosureKind, RuntimeResource};
    let date = CivilDate::from_iso("2030-01-02").unwrap();
    let events = vec![
        Event::CivilDateAdvanced {
            seq: 1,
            settled_date: date,
            next_date: date.next().unwrap(),
            next_status: DayStatus::Trading,
        },
        Event::CompanyDisclosurePublished {
            seq: 2,
            publication_id: engine::information::PublicationId::new(1),
            company: engine::company::CompanyId("600001".into()),
            published_at: CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
            kind: CompanyDisclosureKind::Announcement,
        },
        Event::ResourceLimit {
            seq: 3,
            resource: RuntimeResource::PendingPlanEvents,
            limit: 100,
        },
    ];
    let mut stream = EventKeyStream::default();
    let mut keyed = stream.attach_legacy_emission(&events[..1]).unwrap();
    keyed.extend(stream.attach_legacy_emission(&events[1..]).unwrap());
    assert_eq!(
        keyed
            .iter()
            .map(|fact| fact.key.local_event_index())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert!(keyed.iter().all(|fact| fact.key.phase_rank() == 6
        && fact.key.source() == EventSourceIndex::Session
        && fact.key.entity() == &EntityTag::Session));
    validate_event_keys(&keyed).unwrap();
    let duplicate = KeyedEvent {
        key: EventStableKey::for_event(&events[1], 0),
        event: &events[1],
    };
    assert!(validate_event_keys(&[keyed[0].clone(), duplicate]).is_err());
    assert_eq!(
        EventKeyStream::default()
            .attach_legacy_emission(&events)
            .unwrap(),
        keyed
    );
}

#[test]
fn event_source_numeric_contract_and_entity_order_are_explicit() {
    assert_eq!(
        [
            EventSourceIndex::Sealed,
            EventSourceIndex::P0,
            EventSourceIndex::PriceTick,
            EventSourceIndex::DayEnd,
            EventSourceIndex::Session
        ]
        .map(EventSourceIndex::rank),
        [0, 1, 2, 3, 4]
    );
    let expected = vec![
        EntityTag::Stock(StockCode("000001".into())),
        EntityTag::Stock(StockCode("600001".into())),
        EntityTag::Account(AccountId(1)),
        EntityTag::Account(AccountId(2)),
        EntityTag::Session,
    ];
    for shift in 0..expected.len() {
        let mut permutation = expected.clone();
        permutation.rotate_left(shift);
        permutation.sort();
        assert_eq!(permutation, expected);
    }
}

#[test]
fn legacy_adapter_rejects_rekeying_reordered_emission() {
    let mut events = facts();
    events.reverse();
    assert!(matches!(
        EventKeyStream::default().attach_legacy_emission(&events),
        Err(engine::session::StepFatal::InvariantViolation { .. })
    ));
}

#[test]
fn receipt_payload_precedes_envelope_and_envelope_precedes_ordinal() {
    let envelope = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".into()),
        order: OrderId(1),
        side: Side::Buy,
    };
    let mut later_envelope = envelope.clone();
    later_envelope.order = OrderId(2);
    let inputs = [
        (0, envelope.clone(), 0),
        (0, envelope.clone(), 1),
        (0, later_envelope, 0),
        (1, envelope, 0),
    ];
    let expected: Vec<_> = inputs
        .into_iter()
        .map(|(payload, envelope, ordinal)| {
            ReceiptLocalKey::new(
                JournalRank::SealedBatch,
                ReceiptSource::SealedIntent(payload),
                ReceiptTransition { envelope, ordinal },
            )
            .unwrap()
        })
        .collect();
    for shift in 0..expected.len() {
        let mut observed = expected.clone();
        observed.rotate_left(shift);
        observed.sort();
        assert_eq!(observed, expected);
    }
}

#[test]
fn receipt_order_is_journal_source_payload_envelope_then_transition() {
    let envelope = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".into()),
        order: OrderId(1),
        side: Side::Buy,
    };
    let sources = [
        ReceiptSource::P0Expiry(9),
        ReceiptSource::SealedIntent(0),
        ReceiptSource::SealedIntent(1),
        ReceiptSource::Auction(0),
        ReceiptSource::DayEnd(0),
    ];
    let expected: Vec<_> = sources
        .into_iter()
        .map(|source| {
            ReceiptLocalKey::new(
                source.journal(),
                source,
                ReceiptTransition {
                    envelope: envelope.clone(),
                    ordinal: 0,
                },
            )
            .unwrap()
        })
        .collect();
    let mut observed = expected.clone();
    observed.reverse();
    observed.sort();
    assert_eq!(observed, expected);
    assert_eq!(JournalRank::PreSeal.rank(), 0);
    assert_eq!(JournalRank::SealedBatch.rank(), 1);
    assert_eq!(sources.map(|source| source.rank()), [0, 0, 0, 1, 2]);
    assert!(matches!(
        ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::P0Expiry(0),
            ReceiptTransition {
                envelope,
                ordinal: 0
            }
        ),
        Err(engine::session::StepFatal::InvariantViolation { .. })
    ));
    assert!(validate_receipt_keys(&[expected[0].clone(), expected[0].clone()]).is_err());
}
