use super::p7_events::{collect_events, OwnedEventFact};
use super::*;
use crate::{AccountId, Event, Money, OrderId, Side, StockCode, TradingPhase};

fn code() -> StockCode {
    StockCode("600001".to_owned())
}

fn auction_tick(seq: u64) -> Event {
    Event::AuctionTick {
        seq,
        tick: 1,
        phase: TradingPhase::CallAuction,
        code: code(),
        indicative_price: None,
        matched_volume: 0,
        imbalance: 100,
    }
}

fn accepted(seq: u64) -> Event {
    Event::OrderAccepted {
        seq,
        account: AccountId(7),
        code: code(),
        id: OrderId(3),
        side: Side::Buy,
        price: Money::from_cents(1_000),
        remaining_qty: 100,
    }
}

fn fact(event: Event, ordinal: u64) -> OwnedEventFact {
    let key = EventStableKey::for_event(&event, ordinal);
    OwnedEventFact { key, event }
}

#[test]
fn collector_sorts_explicit_worker_keys_then_assigns_external_sequence() {
    let auction = auction_tick(900);
    let accepted = accepted(1);
    let output = collect_events(vec![fact(accepted, 4), fact(auction, 9)], 40).unwrap();

    assert_eq!(output.next_seq, 42);
    assert!(matches!(
        output.events.as_slice(),
        [
            Event::AuctionTick { seq: 41, .. },
            Event::OrderAccepted { seq: 42, .. }
        ]
    ));
}

#[test]
fn collector_rejects_a_key_whose_variant_mapping_does_not_match_its_event() {
    let event = auction_tick(12);
    let mismatched_key = EventStableKey::for_event(&accepted(13), 0);

    assert!(matches!(
        collect_events(
            vec![OwnedEventFact {
                key: mismatched_key,
                event,
            }],
            10,
        ),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_events::collect_events"
    ));
}

#[test]
fn collector_rejects_duplicate_phase_six_session_identity_without_advancing_cursor() {
    let first = Event::ResourceLimit {
        seq: 71,
        resource: crate::session::RuntimeResource::PendingPlanEvents,
        limit: 5,
    };
    let second = Event::ResourceLimit {
        seq: 72,
        resource: crate::session::RuntimeResource::PendingPlanEvents,
        limit: 6,
    };
    let key = EventStableKey::for_event(&first, 0);

    assert!(matches!(
        collect_events(
            vec![
                OwnedEventFact {
                    key: key.clone(),
                    event: first,
                },
                OwnedEventFact { key, event: second },
            ],
            50,
        ),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_events::collect_events"
    ));

    let output = collect_events(vec![fact(auction_tick(73), 0)], 50).unwrap();
    assert_eq!(output.next_seq, 51);
    assert_eq!(output.events[0].seq(), 51);
}

#[test]
fn collector_rejects_sequence_overflow_without_returning_partial_events() {
    assert!(matches!(
        collect_events(vec![fact(auction_tick(0), 0)], u64::MAX),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_events::collect_events"
    ));
}
