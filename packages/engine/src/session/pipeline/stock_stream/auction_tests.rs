use super::{auction_shards, finish_auction_shards, AuctionStockInput};
use crate::session::pipeline::stock_auction::b2_auction_day_end::AuctionFinishProbe;
use crate::session::pipeline::stock_auction::{
    AuctionCompletionInput, AuctionOperation, AuctionOrder, AuctionPhase, StockAuctionState,
};
use crate::session::pipeline::{Envelope, EnvelopeAudit, EnvelopeKey, FeeComponents, ReceiptKind};
use crate::{AccountId, Event, GameConfig, Market, Money, OrderId, Side, StockCode, StockExchange};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct FinishGate {
    state: Mutex<FinishGateState>,
    entered: Condvar,
}

#[derive(Default)]
struct FinishGateState {
    arrivals: usize,
    timed_out: bool,
}

impl FinishGate {
    fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        state.arrivals += 1;
        self.entered.notify_all();
        let (mut state, timeout) = self
            .entered
            .wait_timeout_while(state, Duration::from_secs(1), |state| state.arrivals < 2)
            .unwrap();
        state.timed_out |= timeout.timed_out();
    }
}

#[test]
fn independent_stock_auction_finishes_overlap_and_clear_each_book_once() {
    let inputs = [
        auction_input("600001", StockExchange::Shanghai, 1),
        auction_input("000001", StockExchange::Shenzhen, 3),
    ];
    let mut shards = auction_shards(inputs.into()).unwrap();
    let gate = Arc::new(FinishGate::default());
    for shard in shards.values_mut() {
        let gate = Arc::clone(&gate);
        shard.finish_probe = Some(AuctionFinishProbe(Arc::new(move || gate.enter())));
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let finished = pool
        .install(|| finish_auction_shards(shards, 600, true, false))
        .unwrap();

    assert_eq!(finished.workers.len(), 2);
    assert!(finished.detached_event_facts.is_empty());
    assert!(finished.detached_lifecycle_facts.is_empty());
    for (code, worker) in &finished.workers {
        let buy_id = if code.0 == "600001" {
            OrderId(1)
        } else {
            OrderId(3)
        };
        assert_eq!(worker.clearing_price, Some(Money::from_cents(1_000)));
        assert_eq!(worker.matches.len(), 1);
        let matched = &worker.matches[0];
        assert_eq!(matched.buy.order, buy_id);
        assert_eq!(matched.sell.order, OrderId(buy_id.0 + 1));
        assert_eq!(matched.qty, 100);
        assert_eq!(matched.price, Money::from_cents(1_000));
        assert!(matched.maker_is_buy);
        assert_eq!(worker.finalizer.auction_tail_passes, 1);
        assert_eq!(worker.finalizer.auction_completion_passes, 1);
        assert_eq!(worker.finalizer.day_end_passes, 0);
        assert!(worker.auction_orders.is_empty());
        let remainders = worker.market.resting_orders();
        assert_eq!(remainders.len(), 1);
        assert_eq!(remainders[0].id, buy_id);
        assert_eq!(remainders[0].qty, 100);
        assert_eq!(remainders[0].filled_qty, 100);
        assert_eq!(worker.receipts.len(), 3);
        assert_eq!(
            worker
                .receipts
                .iter()
                .filter(|receipt| receipt.kind == ReceiptKind::Fill)
                .count(),
            2
        );
        assert_eq!(
            worker
                .receipts
                .iter()
                .filter(|receipt| receipt.kind == ReceiptKind::Rollover)
                .count(),
            1
        );
        assert_eq!(
            worker
                .event_facts
                .iter()
                .filter(|fact| matches!(
                    &fact.event,
                    Event::AuctionCompleted { code: event_code, matched_volume: 100, .. }
                        if event_code == code
                ))
                .count(),
            1
        );
    }
    let state = gate.state.lock().unwrap();
    assert_eq!(state.arrivals, 2);
    assert!(
        !state.timed_out,
        "one stock waited for an unrelated stock's entire auction finish"
    );
}

#[test]
fn auction_finish_keeps_first_stock_error_when_second_stock_finishes_first() {
    let first = "600001";
    let second = "600002";
    let errors = [first, second].map(|code| {
        let input = failing_auction_input(code);
        let shard = auction_shards(vec![input]).unwrap().pop_first().unwrap().1;
        shard.finish(600, true, false).err().unwrap()
    });
    assert_ne!(errors[0], errors[1]);
    assert!(errors[0]
        .to_string()
        .contains("auction price tick must be positive"));
    assert!(errors[1].to_string().contains("duplicate order id"));
    let shards = || {
        auction_shards(vec![
            failing_auction_input(first),
            failing_auction_input(second),
        ])
        .unwrap()
    };
    let serial = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    assert_eq!(
        serial
            .install(|| finish_auction_shards(shards(), 600, true, false))
            .err()
            .unwrap(),
        errors[0]
    );

    let completion = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
    let mut shards = shards();
    for (code, shard) in &mut shards {
        // The callback has one owner: the consuming finish call. Dropping that
        // owner records its return, including the real error path through `?`.
        let returned = FinishReturned {
            code: code.clone(),
            completion: Arc::clone(&completion),
        };
        shard.finish_probe = Some(AuctionFinishProbe(Arc::new(move || {
            returned.wait_for_second_stock_if_first();
        })));
    }
    let parallel = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let error = parallel
        .install(|| finish_auction_shards(shards, 600, true, false))
        .err()
        .unwrap();
    assert_eq!(error, errors[0]);
    assert_eq!(
        *completion.0.lock().unwrap(),
        [StockCode(second.to_owned()), StockCode(first.to_owned())]
    );
}

struct FinishReturned {
    code: StockCode,
    completion: Arc<(Mutex<Vec<StockCode>>, Condvar)>,
}

impl FinishReturned {
    fn wait_for_second_stock_if_first(&self) {
        if self.code.0 == "600001" {
            let state = self.completion.0.lock().unwrap();
            // Timeout releases the worker even if dispatch stops early. The
            // completion-order assertion then reports the missed dependency.
            drop(
                self.completion
                    .1
                    .wait_timeout_while(state, Duration::from_secs(1), |completed| {
                        completed.is_empty()
                    })
                    .unwrap(),
            );
        }
    }
}

impl Drop for FinishReturned {
    fn drop(&mut self) {
        self.completion.0.lock().unwrap().push(self.code.clone());
        self.completion.1.notify_all();
    }
}

fn failing_auction_input(code: &str) -> AuctionStockInput {
    let first_order_id = if code == "600001" { 1 } else { 3 };
    let mut input = auction_input(code, StockExchange::Shanghai, first_order_id);
    if code == "600001" {
        input.completion.price_tick = Money::ZERO;
    } else {
        input
            .market
            .record_filled_order(OrderId(first_order_id + 1), AccountId(2))
            .unwrap();
    }
    input
}

fn auction_input(code: &str, exchange: StockExchange, first_order_id: u64) -> AuctionStockInput {
    let code = StockCode(code.to_owned());
    let price = Money::from_cents(1_000);
    let config = GameConfig::proposed_defaults();
    let phase = AuctionPhase::Opening {
        elapsed_ticks: 599,
        cancelable_ticks: 300,
    };
    let mut state = StockAuctionState::new(code.clone());
    for (side, qty, order_id, account) in [
        (Side::Buy, 200, first_order_id, AccountId(1)),
        (Side::Sell, 100, first_order_id + 1, AccountId(2)),
    ] {
        let key = EnvelopeKey {
            account,
            stock: code.clone(),
            order: OrderId(order_id),
            side,
        };
        let audit = EnvelopeAudit {
            limit: price,
            remaining_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            nominal: FeeComponents::ZERO,
            charged: FeeComponents::ZERO,
        };
        let (cash, shares) = match side {
            Side::Buy => (
                crate::session::buy_order_reservation(&config, price, qty, Money::ZERO).unwrap(),
                0,
            ),
            Side::Sell => (Money::ZERO, qty),
        };
        state
            .apply_operation(
                phase,
                AuctionOperation::Place(AuctionOrder {
                    envelope: Envelope::tick_start_existing(key, cash, shares, audit),
                    arrival_seq: 0,
                }),
            )
            .unwrap();
    }
    AuctionStockInput {
        market: Market::new(code.clone(), price, 0.10, Money::from_cents(1)).unwrap(),
        code,
        completion: AuctionCompletionInput {
            state,
            phase,
            previous_close: price,
            exchange,
            price_tick: Money::from_cents(1),
            config,
            day_end_envelopes: Vec::new(),
        },
        continuous_envelopes: Vec::new(),
        operations: Vec::new(),
    }
}
