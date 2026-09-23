use super::p4_continuous::{ContinuousPlaceFact, ContinuousTradeFact};
use super::p4_p5_p6_transaction::P4P5P6StockOutput;
use super::p7_continuous_transaction::collect_continuous_transaction_events;
use crate::{AccountId, Event, Market, Money, OrderId, Side, StockCode, Trade};
use std::collections::BTreeMap;

fn stock_output(code: &str, sealed_index: u64) -> (StockCode, P4P5P6StockOutput) {
    let code = StockCode(code.to_owned());
    let market = Market::new(
        code.clone(),
        Money::from_cents(1_000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap();
    (
        code.clone(),
        P4P5P6StockOutput {
            market,
            trades: Vec::new(),
            place_facts: vec![ContinuousPlaceFact::Resting {
                sealed_index,
                account: AccountId(0),
                code,
                order_id: OrderId(sealed_index + 1),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                remaining_qty: 100,
            }],
            cancel_facts: Vec::new(),
        },
    )
}

fn stock_output_with_trade(code: &str) -> (StockCode, P4P5P6StockOutput) {
    let (code, mut output) = stock_output(code, 7);
    output.place_facts.clear();
    output.trades.push(ContinuousTradeFact {
        stock: code.clone(),
        triggering_sealed_index: 7,
        stock_local_trade_event_index: 0,
        trade: Trade {
            price: Money::from_cents(1_000),
            qty: 100,
            maker: AccountId(1),
            taker: AccountId(2),
            maker_order_id: OrderId(11),
            taker_order_id: OrderId(12),
            maker_filled_value_before: Money::ZERO,
            taker_filled_value_before: Money::ZERO,
        },
    });
    (code, output)
}

#[test]
fn same_local_identity_on_two_stocks_collects_canonically_and_advances_seq() {
    let (beta_code, beta) = stock_output_with_trade("600002");
    let (alpha_code, alpha) = stock_output_with_trade("600001");
    let stocks = BTreeMap::from([(beta_code, beta), (alpha_code.clone(), alpha)]);

    let collected = collect_continuous_transaction_events(&stocks, 9).unwrap();

    assert_eq!(collected.next_seq, 11);
    assert!(matches!(
        collected.events.as_slice(),
        [
            Event::Trade {
                seq: 10,
                code: first_code,
                ..
            },
            Event::Trade {
                seq: 11,
                code: second_code,
                ..
            }
        ] if first_code == &alpha_code && second_code == &StockCode("600002".to_owned())
    ));
}

#[test]
fn duplicate_p4_identity_is_rejected_without_an_event_or_seq_output() {
    let (code, mut stock) = stock_output("600001", 3);
    stock.place_facts.push(stock.place_facts[0].clone());
    let stocks = BTreeMap::from([(code, stock)]);

    assert!(matches!(
        collect_continuous_transaction_events(&stocks, 17),
        Err(super::StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_p4_producers"
    ));
}

#[test]
fn event_sequence_overflow_is_typed_before_any_output_is_returned() {
    let (code, stock) = stock_output("600001", 0);
    let stocks = BTreeMap::from([(code, stock)]);

    assert!(matches!(
        collect_continuous_transaction_events(&stocks, u64::MAX),
        Err(super::StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_events::collect_events"
    ));
}
