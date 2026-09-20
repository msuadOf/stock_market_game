use engine::session::protocol::*;
use engine::{AccountId, Event, Money, StockCode, TradingPhase};

fn events() -> Vec<Event> {
    (1..=3)
        .map(|seq| Event::IntentRejected {
            seq,
            account: AccountId(0),
            code: StockCode("600001".into()),
            reason: engine::RejectionReason::UnknownStock,
        })
        .collect()
}

fn frame() -> TickFrame {
    let events = events();
    TickFrame {
        tick: 1,
        facts: attach_facts(&events).unwrap(),
        events,
        timeseries_payload: Default::default(),
        seq_from: 0,
        seq_to: 3,
    }
}

fn update(frame: TickFrame) -> EngineUpdate {
    EngineUpdate::TickBatch(TickBatch {
        frames: vec![frame],
        runtime_snapshot: None,
    })
}

#[test]
fn optional_imbalance_rejects_unsafe_wire_values() {
    let event = Event::AuctionTick {
        seq: 1,
        tick: 1,
        phase: TradingPhase::CallAuction,
        code: StockCode("600001".into()),
        indicative_price: Some(Money::from_cents(1000)),
        matched_volume: 100,
        imbalance: 0,
    };
    let key = attach_facts(&[event]).unwrap().remove(0).key;
    let mut point = AuctionPoint {
        key,
        tick: 1,
        kind: AuctionPointKind::Indication,
        phase: TradingPhase::CallAuction,
        indicative_price: None,
        matched_volume: 100,
        imbalance: None,
    };
    for value in [None, Some(0), Some(9_007_199_254_740_991)] {
        point.imbalance = value;
        let wire = serde_json::to_value(&point).unwrap();
        assert_eq!(
            serde_json::from_value::<AuctionPoint>(wire)
                .unwrap()
                .imbalance,
            value
        );
    }
    let mut wire = serde_json::to_value(&point).unwrap();
    wire["imbalance"] = serde_json::json!(9_007_199_254_740_992_u64);
    assert!(serde_json::from_value::<AuctionPoint>(wire).is_err());
    point.imbalance = Some(9_007_199_254_740_992);
    assert!(serde_json::to_value(point).is_err());
}

#[test]
fn fact_correspondence_preserves_multiplicity_and_rejects_substitutions() {
    let original = frame();
    assert_eq!(original.facts.len(), 3);
    original.validate().unwrap();
    let mut reversed = original.clone();
    reversed.facts.reverse();
    reversed.events.reverse();
    reversed.validate().unwrap();
    for variant in 0..5 {
        let mut malformed = original.clone();
        match variant {
            0 => {
                malformed.facts.pop();
            }
            1 => {
                malformed.facts[1] = malformed.facts[0].clone();
            }
            2 => {
                malformed.events[1] = malformed.events[0].clone();
            }
            3 => {
                malformed.facts[0].key = engine::session::pipeline::EventStableKey::for_event(
                    &Event::IntentRejected {
                        seq: 1,
                        account: AccountId(99),
                        code: StockCode("600001".into()),
                        reason: engine::RejectionReason::UnknownStock,
                    },
                    0,
                );
            }
            4 => malformed.facts[0].canonical_payload.push(' '),
            _ => unreachable!(),
        }
        assert!(malformed.validate().is_err(), "variant {variant}");
    }
}

#[test]
fn coherent_mutation_is_replay_mismatch_not_identity_failure() {
    let original = frame();
    let mut guard = ReplayGuard::new(0, 0);
    assert_eq!(
        guard.ingest(&update(original.clone())).unwrap(),
        ReplayDecision::Applied
    );
    let mut reordered = original.clone();
    reordered.facts.reverse();
    reordered.events.reverse();
    assert_eq!(
        guard.ingest(&update(reordered)).unwrap(),
        ReplayDecision::ExactRetry
    );
    let mut changed = original;
    if let Event::IntentRejected { code, .. } = &mut changed.events[0] {
        *code = StockCode("600002".into());
    }
    changed.facts = attach_facts(&changed.events).unwrap();
    changed.validate().unwrap();
    assert_eq!(
        guard.ingest(&update(changed)),
        Err(ProtocolError::ReplayMismatch)
    );
}

#[test]
fn starting_context_rejects_stale_future_and_sequence_gaps() {
    for (tick, seq) in [(1, 0), (2, 3), (0, 1)] {
        assert_eq!(
            ReplayGuard::new(tick, seq).ingest(&update(frame())),
            Err(ProtocolError::ReplayMismatch)
        );
    }
}

#[test]
fn equal_sets_with_different_multiplicity_are_rejected() {
    let mut malformed = frame();
    malformed.events[2] = malformed.events[0].clone();
    malformed.facts[2] = malformed.facts[1].clone();
    malformed.facts[2].key =
        engine::session::pipeline::EventStableKey::for_event(&malformed.facts[2].event, 2);
    assert_eq!(malformed.validate(), Err(ProtocolError::FactIdentity));
}
