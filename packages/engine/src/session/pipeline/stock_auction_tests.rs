use super::stock_auction::{
    checked_total_imbalance, complete_stock_auction, select_clearing, AuctionCancelRejection,
    AuctionCompletionInput, AuctionOperation, AuctionOperationFact, AuctionOrder, AuctionPhase,
    ClearingOrder, ClearingSelection, StockAuctionState,
};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents, JournalRank, ReceiptKind,
    ReceiptLocalKey, ReceiptSource, ReceiptTransition, ResVec,
};
use crate::{AccountId, GameConfig, Money, OrderId, Side, StockCode, StockExchange};

fn order(side: Side, limit_cents: i64, qty: u64) -> ClearingOrder {
    ClearingOrder {
        side,
        limit: Money::from_cents(limit_cents),
        qty,
    }
}

fn selected(
    orders: &[ClearingOrder],
    previous_close_cents: i64,
    exchange: StockExchange,
) -> ClearingSelection {
    select_clearing(
        orders,
        Money::from_cents(previous_close_cents),
        exchange,
        Money::from_cents(1),
    )
    .expect("valid auction quantities must be selectable")
    .expect("crossing auction must have a clearing result")
}

#[test]
fn shanghai_uses_the_midpoint_of_the_lowest_and_highest_remaining_candidates() {
    let result = selected(
        &[order(Side::Buy, 10_200, 200), order(Side::Sell, 9_800, 200)],
        10_000,
        StockExchange::Shanghai,
    );

    assert_eq!(
        result,
        ClearingSelection {
            price: Money::from_cents(10_000),
            volume: 200,
            imbalance: 0,
        }
    );
}

#[test]
fn shanghai_rounds_a_positive_half_tick_up_to_the_price_tick() {
    let result = selected(
        &[
            order(Side::Buy, 10_001, 200),
            order(Side::Sell, 10_000, 200),
        ],
        10_000,
        StockExchange::Shanghai,
    );

    assert_eq!(result.price, Money::from_cents(10_001));
    assert_eq!(result.volume, 200);
}

#[test]
fn shenzhen_prefers_the_smallest_price_better_quantity_difference_before_previous_close() {
    let result = selected(
        &[
            order(Side::Buy, 10_200, 300),
            order(Side::Buy, 10_100, 100),
            order(Side::Sell, 9_900, 100),
            order(Side::Sell, 10_000, 300),
            order(Side::Sell, 10_100, 100),
        ],
        10_000,
        StockExchange::Shenzhen,
    );

    assert_eq!(result.price, Money::from_cents(10_100));
    assert_eq!(result.volume, 400);
}

#[test]
fn shenzhen_uses_previous_close_after_price_better_quantity_difference() {
    let result = selected(
        &[order(Side::Buy, 10_200, 200), order(Side::Sell, 9_800, 200)],
        10_100,
        StockExchange::Shenzhen,
    );

    assert_eq!(result.price, Money::from_cents(10_200));
}

#[test]
fn shenzhen_uses_the_documented_lower_price_when_previous_close_distance_is_tied() {
    let result = selected(
        &[order(Side::Buy, 10_200, 200), order(Side::Sell, 9_800, 200)],
        10_000,
        StockExchange::Shenzhen,
    );

    assert_eq!(result.price, Money::from_cents(9_800));
}

#[test]
fn uncrossed_orders_have_no_clearing_result_and_keep_their_checked_imbalance() {
    let orders = [order(Side::Buy, 9_900, 300), order(Side::Sell, 10_100, 200)];

    assert_eq!(
        select_clearing(
            &orders,
            Money::from_cents(10_000),
            StockExchange::Shanghai,
            Money::from_cents(1),
        )
        .expect("bounded uncrossed quantities are valid"),
        None
    );
    assert_eq!(checked_total_imbalance(&orders).unwrap(), 100);
}

#[test]
fn clearing_quantity_overflow_is_a_typed_fatal_error() {
    let orders = [
        order(Side::Buy, 10_100, u64::MAX),
        order(Side::Buy, 10_100, 1),
        order(Side::Sell, 10_000, 1),
    ];

    let error = select_clearing(
        &orders,
        Money::from_cents(10_000),
        StockExchange::Shanghai,
        Money::from_cents(1),
    )
    .expect_err("candidate quantity overflow must not become no-clearing");

    assert!(matches!(
        error,
        crate::session::StepFatal::InvariantViolation { description, location }
            if description.contains("quantity overflow")
                && location == "pipeline::stock_auction::select_clearing"
    ));
}

#[test]
fn indicative_imbalance_quantity_overflow_is_a_typed_fatal_error() {
    let orders = [
        order(Side::Buy, 9_900, u64::MAX),
        order(Side::Buy, 9_800, 1),
        order(Side::Sell, 10_100, 1),
    ];

    let error = checked_total_imbalance(&orders)
        .expect_err("indicative imbalance overflow must not panic or wrap");

    assert!(matches!(
        error,
        crate::session::StepFatal::InvariantViolation { description, location }
            if description.contains("imbalance quantity overflow")
                && location == "pipeline::stock_auction::checked_total_imbalance"
    ));
}

#[test]
fn opening_cancel_is_allowed_before_0920_and_rejected_at_the_boundary() {
    let order = live_order(1, 7, Side::Buy, 1_000, 100);
    let key = order.envelope.key().clone();
    let ledger_envelope = order.envelope.clone();
    let mut cancelable = auction_state([order.clone()]);

    let canceled = cancelable
        .apply_operation(
            AuctionPhase::Opening {
                elapsed_ticks: 4,
                cancelable_ticks: 5,
            },
            AuctionOperation::Cancel {
                sealed_index: 41,
                account: AccountId(1),
                order_id: OrderId(7),
            },
        )
        .expect("09:15-09:20 opening cancellation must be accepted");

    assert_eq!(
        canceled.fact,
        AuctionOperationFact::Canceled {
            account: AccountId(1),
            order_id: OrderId(7),
            remaining_qty: 100,
        }
    );
    assert_eq!(canceled.terminal_key.as_ref(), Some(&key));
    assert_eq!(cancelable.orders().len(), 0);
    let receipt = canceled
        .receipt
        .expect("a successful auction cancellation releases escrow");
    assert_eq!(receipt.kind, ReceiptKind::Release);
    assert_eq!(
        receipt.local_key,
        receipt_key(ReceiptSource::SealedIntent(41), key.clone(), 0)
    );

    let mut ledger = EnvelopeLedger::new(0, [ledger_envelope]).unwrap();
    ledger.apply(&mut [receipt]).unwrap();
    ledger.remove_terminal(&[key]).unwrap();

    let mut boundary = auction_state([order]);
    let rejected = boundary
        .apply_operation(
            AuctionPhase::Opening {
                elapsed_ticks: 5,
                cancelable_ticks: 5,
            },
            AuctionOperation::Cancel {
                sealed_index: 42,
                account: AccountId(1),
                order_id: OrderId(7),
            },
        )
        .expect("the 09:20 boundary is a business rejection");
    assert_eq!(
        rejected.fact,
        AuctionOperationFact::Rejected {
            account: AccountId(1),
            order_id: OrderId(7),
            reason: AuctionCancelRejection::NotCancelable,
        }
    );
    assert!(rejected.receipt.is_none());
    assert_eq!(boundary.orders().len(), 1);
}

#[test]
fn closing_auction_cancel_is_never_allowed() {
    let mut state = auction_state([live_order(1, 7, Side::Sell, 1_000, 100)]);

    let rejected = state
        .apply_operation(
            AuctionPhase::Closing,
            AuctionOperation::Cancel {
                sealed_index: 9,
                account: AccountId(1),
                order_id: OrderId(7),
            },
        )
        .expect("closing cancellation is a typed business rejection");

    assert_eq!(
        rejected.fact,
        AuctionOperationFact::Rejected {
            account: AccountId(1),
            order_id: OrderId(7),
            reason: AuctionCancelRejection::NotCancelable,
        }
    );
    assert!(rejected.receipt.is_none());
    assert_eq!(state.orders().len(), 1);
}

#[test]
fn same_tick_p3_auction_envelope_cannot_be_canceled_or_released() {
    let order = p3_live_order(1, 7, Side::Buy, 1_000, 100);
    let key = order.envelope.key().clone();
    let live = order.envelope.live();
    let mut state = auction_state([order]);

    let rejected = state
        .apply_operation(
            AuctionPhase::Opening {
                elapsed_ticks: 4,
                cancelable_ticks: 5,
            },
            AuctionOperation::Cancel {
                sealed_index: 43,
                account: AccountId(1),
                order_id: OrderId(7),
            },
        )
        .expect("same-tick cross-envelope cancellation is a business rejection");

    assert_eq!(
        rejected.fact,
        AuctionOperationFact::Rejected {
            account: AccountId(1),
            order_id: OrderId(7),
            reason: AuctionCancelRejection::SameTickEnvelope,
        }
    );
    assert!(rejected.receipt.is_none());
    assert!(rejected.terminal_key.is_none());
    assert_eq!(state.orders().len(), 1);
    assert_eq!(state.orders()[0].envelope.key(), &key);
    assert_eq!(state.orders()[0].envelope.live(), live);
}

#[test]
fn uncrossed_opening_order_rolls_into_continuous_with_identity_and_audit_intact() {
    let order = live_order(2, 12, Side::Buy, 990, 100);
    let key = order.envelope.key().clone();
    let ledger_envelope = order.envelope.clone();

    let mut output = complete(auction_state([order]), opening_phase()).unwrap();

    assert_eq!(output.clearing, None);
    assert_eq!(output.matched_volume, 0);
    assert_eq!(output.continuous_orders.len(), 1);
    let remainder = &output.continuous_orders[0];
    assert_eq!(remainder.envelope.key(), &key);
    assert_eq!(remainder.arrival_seq, 12);
    assert_eq!(remainder.envelope.audit().remaining_qty, 100);
    assert_eq!(remainder.envelope.audit().filled_qty, 0);
    assert_eq!(output.receipts.len(), 1);
    assert_eq!(output.receipts[0].kind, ReceiptKind::Rollover);
    assert_eq!(
        output.receipts[0].local_key,
        receipt_key(ReceiptSource::Auction(0), key.clone(), 0)
    );

    let mut ledger = EnvelopeLedger::new(0, [ledger_envelope]).unwrap();
    ledger.apply(&mut output.receipts).unwrap();
    assert_eq!(ledger.get(&key).unwrap().live(), remainder.envelope.live());
}

#[test]
fn auction_ordinals_are_envelope_local_and_partial_remainder_follows_last_fill() {
    let sell_first = live_order(1, 10, Side::Sell, 900, 100);
    let sell_second = live_order(2, 20, Side::Sell, 900, 100);
    let buy = live_order(3, 30, Side::Buy, 1_100, 300);
    let sell_first_key = sell_first.envelope.key().clone();
    let sell_second_key = sell_second.envelope.key().clone();
    let buy_key = buy.envelope.key().clone();
    let ledger_envelopes = [
        sell_first.envelope.clone(),
        sell_second.envelope.clone(),
        buy.envelope.clone(),
    ];

    let mut output = complete(
        auction_state([buy, sell_second, sell_first]),
        opening_phase(),
    )
    .unwrap();

    assert_eq!(output.matched_volume, 200);
    assert_eq!(output.matches.len(), 2);
    assert_eq!(output.continuous_orders.len(), 1);
    assert_eq!(output.continuous_orders[0].envelope.key(), &buy_key);
    assert_eq!(output.continuous_orders[0].arrival_seq, 30);
    assert_eq!(
        output.continuous_orders[0].envelope.audit().remaining_qty,
        100
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Fill,
        receipt_key(ReceiptSource::Auction(2), buy_key.clone(), 0),
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Fill,
        receipt_key(ReceiptSource::Auction(2), buy_key.clone(), 1),
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Rollover,
        receipt_key(ReceiptSource::Auction(2), buy_key.clone(), 2),
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Fill,
        receipt_key(ReceiptSource::Auction(0), sell_first_key, 0),
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Fill,
        receipt_key(ReceiptSource::Auction(1), sell_second_key, 0),
    );

    let mut ledger = EnvelopeLedger::new(0, ledger_envelopes).unwrap();
    ledger.apply(&mut output.receipts).unwrap();
    assert_eq!(
        ledger.get(&buy_key).unwrap().live(),
        output.continuous_orders[0].envelope.live()
    );
}

#[test]
fn fully_filled_auction_orders_end_on_the_fill_without_zero_quantity_receipts() {
    let buy = live_order(1, 1, Side::Buy, 1_100, 100);
    let sell = live_order(2, 2, Side::Sell, 900, 100);
    let buy_key = buy.envelope.key().clone();
    let sell_key = sell.envelope.key().clone();
    let ledger_envelopes = [buy.envelope.clone(), sell.envelope.clone()];

    let mut output = complete(auction_state([buy, sell]), opening_phase()).unwrap();

    assert_eq!(output.matched_volume, 100);
    assert!(output.continuous_orders.is_empty());
    assert_eq!(output.receipts.len(), 2);
    assert!(output
        .receipts
        .iter()
        .all(|receipt| receipt.kind == ReceiptKind::Fill && receipt.qty_after == 0));
    assert!(output
        .receipts
        .iter()
        .all(|receipt| receipt.kind != ReceiptKind::Rollover));

    let mut ledger = EnvelopeLedger::new(0, ledger_envelopes).unwrap();
    ledger.apply(&mut output.receipts).unwrap();
    ledger.remove_terminal(&[buy_key, sell_key]).unwrap();
    assert_eq!(ledger.terminal_count(), 2);
}

#[test]
fn closing_partial_remainder_rolls_over_then_terminates_in_a_fresh_day_end_source() {
    let buy = live_order(1, 1, Side::Buy, 1_100, 200);
    let sell = live_order(2, 2, Side::Sell, 900, 100);
    let buy_key = buy.envelope.key().clone();
    let sell_key = sell.envelope.key().clone();
    let ledger_envelopes = [buy.envelope.clone(), sell.envelope.clone()];

    let mut output = complete(auction_state([buy, sell]), AuctionPhase::Closing).unwrap();

    assert!(output.continuous_orders.is_empty());
    assert_eq!(
        output.terminal_keys,
        vec![buy_key.clone(), sell_key.clone()]
    );
    let rollover = receipt_by_key(
        &output.receipts,
        &receipt_key(ReceiptSource::Auction(0), buy_key.clone(), 1),
    );
    let day_end = receipt_by_key(
        &output.receipts,
        &receipt_key(ReceiptSource::DayEnd(0), buy_key.clone(), 0),
    );
    assert_eq!(rollover.kind, ReceiptKind::Rollover);
    assert_eq!(day_end.kind, ReceiptKind::Release);
    assert_eq!(rollover.qty_after, day_end.qty_before);
    assert_eq!(rollover.value_after, day_end.value_before);
    assert_eq!(rollover.delta.live_after, day_end.delta.released);
    assert_eq!(day_end.delta.live_after, ResVec::ZERO);

    let mut ledger = EnvelopeLedger::new(0, ledger_envelopes).unwrap();
    ledger.apply(&mut output.receipts).unwrap();
    ledger.remove_terminal(&output.terminal_keys).unwrap();
    assert_eq!(ledger.terminal_count(), 2);
}

#[test]
fn day_end_payload_is_reindexed_over_remainders_after_earlier_envelopes_fill() {
    let fully_filled_buy = live_order(1, 1, Side::Buy, 1_100, 100);
    let remaining_sell = live_order(2, 2, Side::Sell, 900, 200);
    let buy_key = fully_filled_buy.envelope.key().clone();
    let sell_key = remaining_sell.envelope.key().clone();
    let ledger_envelopes = [
        fully_filled_buy.envelope.clone(),
        remaining_sell.envelope.clone(),
    ];

    let mut output = complete(
        auction_state([fully_filled_buy, remaining_sell]),
        AuctionPhase::Closing,
    )
    .unwrap();

    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Fill,
        receipt_key(ReceiptSource::Auction(0), buy_key.clone(), 0),
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Rollover,
        receipt_key(ReceiptSource::Auction(1), sell_key.clone(), 1),
    );
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Release,
        receipt_key(ReceiptSource::DayEnd(0), sell_key.clone(), 0),
    );
    assert!(!output.receipts.iter().any(|receipt| {
        receipt.local_key == receipt_key(ReceiptSource::DayEnd(1), sell_key.clone(), 0)
    }));

    let mut ledger = EnvelopeLedger::new(0, ledger_envelopes).unwrap();
    ledger.apply(&mut output.receipts).unwrap();
    ledger.remove_terminal(&output.terminal_keys).unwrap();
    assert_eq!(ledger.terminal_count(), 2);
}

#[test]
fn closing_day_end_source_domain_includes_continuous_and_auction_remainders() {
    let auction = live_order(2, 20, Side::Buy, 990, 100);
    let auction_key = auction.envelope.key().clone();
    let continuous_key = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(10),
        side: Side::Buy,
    };

    let output = complete_stock_auction(AuctionCompletionInput {
        state: auction_state([auction]),
        phase: AuctionPhase::Closing,
        previous_close: Money::from_cents(1_000),
        exchange: StockExchange::Shanghai,
        price_tick: Money::from_cents(1),
        config: GameConfig::proposed_defaults(),
        day_end_envelopes: vec![continuous_key.clone()],
    })
    .unwrap();

    assert_eq!(output.day_end_source_indices[&continuous_key], 0);
    assert_eq!(output.day_end_source_indices[&auction_key], 1);
    assert_receipt_exists(
        &output.receipts,
        ReceiptKind::Release,
        receipt_key(ReceiptSource::DayEnd(1), auction_key, 0),
    );
}

#[test]
fn buyer_fee_audit_drift_is_a_typed_fatal_before_auction_receipts_escape() {
    let config = GameConfig::proposed_defaults();
    let limit = Money::from_cents(1_100);
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(1),
        side: Side::Buy,
    };
    let audit = EnvelopeAudit {
        limit,
        remaining_qty: 100,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents {
            commission: Money::from_cents(1),
            ..FeeComponents::ZERO
        },
        charged: FeeComponents {
            commission: Money::from_cents(1),
            ..FeeComponents::ZERO
        },
    };
    let buy = AuctionOrder {
        envelope: Envelope::tick_start_existing(
            key,
            crate::session::buy_order_reservation(&config, limit, 100, Money::ZERO).unwrap(),
            0,
            audit,
        ),
        arrival_seq: 1,
    };
    let sell = live_order(2, 2, Side::Sell, 900, 100);

    let error = complete(auction_state([buy, sell]), opening_phase())
        .expect_err("buyer cumulative fee audit drift must poison the shadow auction");

    assert!(matches!(
        error,
        crate::session::StepFatal::InvariantViolation { description, location }
            if description.contains("buyer fee audit")
                && location == "pipeline::stock_auction"
    ));
}

fn complete(
    state: StockAuctionState,
    phase: AuctionPhase,
) -> Result<super::stock_auction::AuctionCompletionOutput, crate::session::StepFatal> {
    complete_stock_auction(AuctionCompletionInput {
        state,
        phase,
        previous_close: Money::from_cents(1_000),
        exchange: StockExchange::Shanghai,
        price_tick: Money::from_cents(1),
        config: GameConfig::proposed_defaults(),
        day_end_envelopes: Vec::new(),
    })
}

fn opening_phase() -> AuctionPhase {
    AuctionPhase::Opening {
        elapsed_ticks: 5,
        cancelable_ticks: 5,
    }
}

fn auction_state(orders: impl IntoIterator<Item = AuctionOrder>) -> StockAuctionState {
    let mut state = StockAuctionState::new(StockCode("600001".to_owned()));
    for order in orders {
        let result = state
            .apply_operation(
                AuctionPhase::Opening {
                    elapsed_ticks: 0,
                    cancelable_ticks: 5,
                },
                AuctionOperation::Place(order),
            )
            .unwrap();
        assert!(matches!(result.fact, AuctionOperationFact::Placed { .. }));
    }
    state
}

fn live_order(
    account: u64,
    order_id: u64,
    side: Side,
    limit_cents: i64,
    remaining_qty: u32,
) -> AuctionOrder {
    let config = GameConfig::proposed_defaults();
    let limit = Money::from_cents(limit_cents);
    let key = EnvelopeKey {
        account: AccountId(account),
        stock: StockCode("600001".to_owned()),
        order: OrderId(order_id),
        side,
    };
    let audit = EnvelopeAudit {
        limit,
        remaining_qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    };
    let envelope = match side {
        Side::Buy => Envelope::tick_start_existing(
            key,
            crate::session::buy_order_reservation(&config, limit, remaining_qty, Money::ZERO)
                .unwrap(),
            0,
            audit,
        ),
        Side::Sell => Envelope::tick_start_existing(key, Money::ZERO, remaining_qty, audit),
    };
    AuctionOrder {
        envelope,
        arrival_seq: order_id,
    }
}

fn p3_live_order(
    account: u64,
    order_id: u64,
    side: Side,
    limit_cents: i64,
    remaining_qty: u32,
) -> AuctionOrder {
    let existing = live_order(account, order_id, side, limit_cents, remaining_qty);
    let key = existing.envelope.key().clone();
    let live = existing.envelope.live();
    let audit = existing.envelope.audit();
    AuctionOrder {
        envelope: Envelope::p3_created(key, live.cash, live.shares, audit),
        arrival_seq: existing.arrival_seq,
    }
}

fn receipt_key(source: ReceiptSource, envelope: EnvelopeKey, ordinal: u64) -> ReceiptLocalKey {
    ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        source,
        ReceiptTransition { envelope, ordinal },
    )
    .unwrap()
}

fn assert_receipt_exists(
    receipts: &[super::EnvelopeReceipt],
    kind: ReceiptKind,
    key: ReceiptLocalKey,
) {
    let receipt = receipt_by_key(receipts, &key);
    assert_eq!(receipt.kind, kind);
}

fn receipt_by_key<'a>(
    receipts: &'a [super::EnvelopeReceipt],
    key: &ReceiptLocalKey,
) -> &'a super::EnvelopeReceipt {
    receipts
        .iter()
        .find(|receipt| &receipt.local_key == key)
        .unwrap_or_else(|| panic!("missing receipt {key:?}"))
}
