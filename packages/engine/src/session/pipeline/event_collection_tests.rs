use super::event_collection::{collect_events, EventFact};
use super::*;
use crate::{AccountId, Event, Money, OrderId, Side, StockId, TradingPhase};

fn code() -> StockId {
    StockId(0)
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

fn fact(event: Event, ordinal: u64) -> EventFact {
    let key = EventStableKey::for_event(&event, ordinal);
    EventFact { key, event }
}

#[test]
fn collector_preserves_arrival_order_and_assigns_continuous_external_sequence() {
    let auction = auction_tick(900);
    let accepted = accepted(1);
    // 输入顺序不排序，输出保持到达序；seq 按分配顺序连续递增
    let output = collect_events(vec![fact(accepted, 4), fact(auction, 9)], 40).unwrap();

    assert_eq!(output.next_seq, 42);
    let seqs: Vec<u64> = output.facts.iter().map(|fact| fact.event.seq()).collect();
    assert_eq!(seqs, vec![41, 42]);
    assert!(matches!(output.facts[0].event, Event::OrderAccepted { .. }));
    assert!(matches!(output.facts[1].event, Event::AuctionTick { .. }));
    // 单记录贯穿：producer 的局部身份原样保留在 facts 中，不再拆分重装
    assert_eq!(output.facts[0].key.local_event_index(), 4);
    assert_eq!(output.facts[1].key.local_event_index(), 9);
}

#[test]
fn collector_rejects_a_key_whose_variant_mapping_does_not_match_its_event() {
    let event = auction_tick(12);
    let mismatched_key = EventStableKey::for_event(&accepted(13), 0);

    assert!(matches!(
        collect_events(
            vec![EventFact {
                key: mismatched_key,
                event,
            }],
            10,
        ),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::event_collection::collect_events"
    ));
}

#[test]
fn collector_rejects_duplicate_session_lifecycle_identity_without_advancing_cursor() {
    let first = Event::CivilDateAdvanced {
        seq: 71,
        settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
        next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
        next_status: crate::DayStatus::Trading,
    };
    let second = Event::CivilDateAdvanced {
        seq: 72,
        settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
        next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
        next_status: crate::DayStatus::Trading,
    };
    let key = EventStableKey::for_event(&first, 0);

    assert!(matches!(
        collect_events(
            vec![
                EventFact {
                    key: key.clone(),
                    event: first,
                },
                EventFact { key, event: second },
            ],
            50,
        ),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::event_collection::collect_events"
    ));

    let output = collect_events(vec![fact(auction_tick(73), 0)], 50).unwrap();
    assert_eq!(output.next_seq, 51);
    assert_eq!(output.facts[0].event.seq(), 51);
}

#[test]
fn collector_rejects_sequence_overflow_without_returning_partial_events() {
    assert!(matches!(
        collect_events(vec![fact(auction_tick(0), 0)], u64::MAX),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::event_collection::collect_events"
    ));
}
