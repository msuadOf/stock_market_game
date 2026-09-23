use super::p4_continuous::{
    ContinuousCancelFact, ContinuousCancelRejection, ContinuousPlaceFact, ContinuousTradeFact,
};
use super::p7_p4_producers::adapt_continuous_facts;
use super::*;
use crate::{AccountId, Event, Money, OrderId, RejectionReason, Side, StockCode, Trade};

fn code(value: &str) -> StockCode {
    StockCode(value.to_owned())
}

fn trade(stock: &StockCode, sealed_index: u64, local_index: u64) -> ContinuousTradeFact {
    ContinuousTradeFact {
        stock: stock.clone(),
        triggering_sealed_index: sealed_index,
        stock_local_trade_event_index: local_index,
        trade: Trade {
            price: Money::from_cents(1_000),
            qty: 100,
            maker: AccountId(3),
            taker: AccountId(7),
            maker_order_id: OrderId(30),
            taker_order_id: OrderId(70),
            maker_filled_value_before: Money::ZERO,
            taker_filled_value_before: Money::ZERO,
        },
    }
}

#[test]
fn continuous_adapter_projects_explicit_place_cancel_and_trade_identities() {
    let alpha = code("600001");
    let beta = code("600002");
    let places = vec![
        ContinuousPlaceFact::Resting {
            sealed_index: 4,
            account: AccountId(7),
            code: alpha.clone(),
            order_id: OrderId(70),
            side: Side::Buy,
            price: Money::from_cents(1_010),
            remaining_qty: 200,
        },
        ContinuousPlaceFact::Filled {
            sealed_index: 5,
            account: AccountId(8),
            code: alpha.clone(),
            order_id: OrderId(80),
            side: Side::Sell,
            filled_qty: 100,
        },
        ContinuousPlaceFact::Rejected {
            sealed_index: 8,
            account: AccountId(9),
            code: beta.clone(),
            order_id: OrderId(90),
            reason: RejectionReason::LimitExceeded,
        },
    ];
    let cancels = vec![
        ContinuousCancelFact::Canceled {
            sealed_index: 6,
            account: AccountId(7),
            code: alpha.clone(),
            order_id: OrderId(22),
            side: Side::Buy,
            remaining_qty: 300,
        },
        ContinuousCancelFact::Rejected {
            sealed_index: 7,
            account: AccountId(10),
            code: beta.clone(),
            order_id: OrderId(23),
            reason: ContinuousCancelRejection::SameTickEnvelope,
        },
    ];
    let trades = vec![trade(&beta, 11, 0), trade(&alpha, 12, 3)];

    let facts = adapt_continuous_facts(&places, &cancels, &trades).unwrap();

    assert_eq!(facts.len(), 6, "Filled has no standalone public Event");
    assert_eq!(
        facts[0].event,
        Event::OrderAccepted {
            seq: 0,
            account: AccountId(7),
            code: alpha.clone(),
            id: OrderId(70),
            side: Side::Buy,
            price: Money::from_cents(1_010),
            remaining_qty: 200,
        }
    );
    assert_eq!(facts[0].key, EventStableKey::for_event(&facts[0].event, 4));
    assert_eq!(
        facts[1].event,
        Event::IntentRejected {
            seq: 0,
            account: AccountId(9),
            code: beta.clone(),
            reason: RejectionReason::LimitExceeded,
        }
    );
    assert_eq!(facts[1].key, EventStableKey::for_event(&facts[1].event, 8));
    assert_eq!(
        facts[2].event,
        Event::OrderCanceled {
            seq: 0,
            account: AccountId(7),
            code: alpha.clone(),
            id: OrderId(22),
            remaining_qty: 300,
        }
    );
    assert_eq!(facts[2].key, EventStableKey::for_event(&facts[2].event, 6));
    assert_eq!(
        facts[3].event,
        Event::IntentRejected {
            seq: 0,
            account: AccountId(10),
            code: beta.clone(),
            reason: RejectionReason::SameTickOrderNotCancelable,
        }
    );
    assert_eq!(facts[3].key, EventStableKey::for_event(&facts[3].event, 7));
    assert_eq!(
        facts[4].event,
        Event::Trade {
            seq: 0,
            code: beta.clone(),
            price: Money::from_cents(1_000),
            qty: 100,
            maker: AccountId(3),
            taker: AccountId(7),
        }
    );
    assert_eq!(facts[4].key, EventStableKey::for_event(&facts[4].event, 0));
    assert_eq!(
        facts[5].event,
        Event::Trade {
            seq: 0,
            code: alpha,
            price: Money::from_cents(1_000),
            qty: 100,
            maker: AccountId(3),
            taker: AccountId(7),
        }
    );
    assert_eq!(facts[5].key, EventStableKey::for_event(&facts[5].event, 3));
}

#[test]
fn continuous_adapter_accepts_equal_stock_local_trade_indices_for_distinct_stocks() {
    let alpha = code("600001");
    let beta = code("600002");

    let facts = adapt_continuous_facts(&[], &[], &[trade(&alpha, 71, 0), trade(&beta, 9, 0)])
        .expect("the stock identity scopes a stock-local trade event index");

    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].key.local_event_index(), 0);
    assert_eq!(facts[1].key.local_event_index(), 0);
    assert_ne!(facts[0].key, facts[1].key);
    assert_eq!(facts[0].key, EventStableKey::for_event(&facts[0].event, 0));
    assert_eq!(facts[1].key, EventStableKey::for_event(&facts[1].event, 0));
}

#[test]
fn continuous_adapter_rejects_duplicate_explicit_event_identity_without_output() {
    let alpha = code("600001");
    let places = [ContinuousPlaceFact::Resting {
        sealed_index: 4,
        account: AccountId(7),
        code: alpha.clone(),
        order_id: OrderId(70),
        side: Side::Buy,
        price: Money::from_cents(1_010),
        remaining_qty: 200,
    }];
    let cancels = [ContinuousCancelFact::Canceled {
        sealed_index: 4,
        account: AccountId(7),
        code: alpha.clone(),
        order_id: OrderId(22),
        side: Side::Buy,
        remaining_qty: 300,
    }];
    let duplicate_trades = [trade(&alpha, 5, 0), trade(&alpha, 6, 0)];

    assert!(matches!(
        adapt_continuous_facts(&places, &cancels, &[]),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_p4_producers"
    ));
    assert!(matches!(
        adapt_continuous_facts(&[], &[], &duplicate_trades),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_p4_producers"
    ));

    assert_eq!(
        adapt_continuous_facts(&places, &[], &[]).unwrap().len(),
        1,
        "a failed call returns no partial output or retained collision state"
    );
}

#[test]
fn continuous_adapter_preserves_explicit_max_trade_index_without_allocating_or_overflowing() {
    let alpha = code("600001");
    let facts = adapt_continuous_facts(&[], &[], &[trade(&alpha, 99, u64::MAX)]).unwrap();

    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].event.seq(), 0);
    assert_eq!(
        facts[0].key,
        EventStableKey::for_event(&facts[0].event, u64::MAX)
    );
}
