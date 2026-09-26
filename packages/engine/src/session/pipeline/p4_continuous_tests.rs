use super::p4_continuous::*;
use super::*;
use crate::{
    AccountId, GameConfig, Intent, Market, Money, Order, OrderId, RejectionReason,
    SecurityCategory, Side, StockCode, Trade, TradingPhase,
};

#[test]
fn tick_start_owner_cancel_releases_the_full_envelope_and_returns_a_terminal_fact() {
    let fixture = CancelFixture::tick_start(7, 100, 0, Money::ZERO, Money::from_cents(99_500));
    let ledger_envelope = fixture.snapshot.envelope.clone();
    let key = ledger_envelope.key().clone();

    let output = cancel_continuous_order(fixture.input(42, AccountId(1), OrderId(7)))
        .expect("a tick-start order owned by the caller must be cancellable");

    assert!(output.market.resting_orders().is_empty());
    assert_eq!(
        output.fact,
        ContinuousCancelFact::Canceled {
            sealed_index: 42,
            account: AccountId(1),
            code: fixture.code.clone(),
            order_id: OrderId(7),
            side: Side::Buy,
            remaining_qty: 100,
        }
    );
    assert_eq!(output.terminal_key.as_ref(), Some(&key));
    let receipt = output
        .receipt
        .expect("successful cancellation emits a release");
    assert_eq!(receipt.kind, ReceiptKind::Release);
    assert_eq!(receipt.envelope, key);
    assert_eq!(receipt.qty_before, 100);
    assert_eq!(receipt.qty_after, 100);
    assert_eq!(receipt.value_before, Money::ZERO);
    assert_eq!(receipt.value_after, Money::ZERO);
    assert_eq!(receipt.delta.spent, ResVec::ZERO);
    assert_eq!(
        receipt.delta.released,
        ResVec::new(Money::from_cents(99_500), 0)
    );
    assert_eq!(receipt.delta.live_after, ResVec::ZERO);
    assert_eq!(receipt.nominal, FeeComponents::ZERO);
    assert_eq!(receipt.charged, FeeComponents::ZERO);
    assert_eq!(receipt.charged_before, FeeComponents::ZERO);
    assert_eq!(receipt.charged_after, FeeComponents::ZERO);
    assert_eq!(receipt.deliver_qty, 0);
    assert_eq!(receipt.deliver_cash, Money::ZERO);
    assert_eq!(
        receipt.local_key,
        ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(42),
            ReceiptTransition {
                envelope: receipt.envelope.clone(),
                ordinal: 0,
            },
        )
        .unwrap()
    );

    let mut ledger = EnvelopeLedger::new(9, [ledger_envelope]).unwrap();
    let mut receipts = [receipt];
    ledger.apply(&mut receipts).unwrap();
    ledger.remove_terminal(&[key]).unwrap();
    assert_eq!(ledger.next_receipt_index(), 10);
    assert_eq!(ledger.terminal_count(), 1);
}

#[test]
fn wrong_owner_cancel_is_a_typed_rejection_and_leaves_the_market_unchanged() {
    let fixture = CancelFixture::tick_start(7, 100, 0, Money::ZERO, Money::from_cents(99_500));
    let key = fixture.snapshot.envelope.key().clone();

    let output = cancel_continuous_order(fixture.input(8, AccountId(2), OrderId(7)))
        .expect("wrong ownership is a business rejection, not a fatal error");

    assert_unchanged_resting_order(&output.market, OrderId(7), AccountId(1), 100);
    assert!(output.receipt.is_none());
    assert_eq!(output.terminal_key, None);
    assert_eq!(
        output.fact,
        ContinuousCancelFact::Rejected {
            sealed_index: 8,
            account: AccountId(2),
            code: fixture.code,
            order_id: OrderId(7),
            reason: ContinuousCancelRejection::NotOrderOwner,
        }
    );

    let ledger = EnvelopeLedger::new(3, [fixture.snapshot.envelope]).unwrap();
    assert_eq!(
        ledger.get(&key).unwrap().live(),
        ResVec::new(Money::from_cents(99_500), 0)
    );
    assert_eq!(ledger.next_receipt_index(), 3);
}

#[test]
fn missing_order_cancel_is_a_typed_rejection_and_leaves_the_market_unchanged() {
    let fixture = CancelFixture::tick_start(7, 100, 0, Money::ZERO, Money::from_cents(99_500));

    let output = cancel_continuous_order(fixture.input(9, AccountId(1), OrderId(999)))
        .expect("a missing order is a business rejection, not a fatal error");

    assert_unchanged_resting_order(&output.market, OrderId(7), AccountId(1), 100);
    assert!(output.receipt.is_none());
    assert_eq!(output.terminal_key, None);
    assert_eq!(
        output.fact,
        ContinuousCancelFact::Rejected {
            sealed_index: 9,
            account: AccountId(1),
            code: fixture.code,
            order_id: OrderId(999),
            reason: ContinuousCancelRejection::OrderNotFound,
        }
    );
}

#[test]
fn fully_filled_cancel_has_a_distinct_business_failure_for_its_owner() {
    let fixture = CancelFixture::tick_start(7, 100, 0, Money::ZERO, Money::from_cents(99_500));
    let mut market = fixture.market.clone();
    market
        .place(Order {
            id: OrderId(8),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(2),
            seq: 0,
        })
        .unwrap();
    assert!(market.resting_orders().is_empty());
    for (account, expected) in [
        (AccountId(1), ContinuousCancelRejection::OrderAlreadyFilled),
        (AccountId(3), ContinuousCancelRejection::NotOrderOwner),
    ] {
        let output = cancel_continuous_order(ContinuousCancelInput {
            market: market.clone(),
            envelopes: Vec::new(),
            operation: ContinuousCancelOperation {
                sealed_index: 9,
                account,
                code: fixture.code.clone(),
                order_id: OrderId(7),
            },
        })
        .unwrap();
        assert!(output.receipt.is_none());
        assert_eq!(
            output.fact,
            ContinuousCancelFact::Rejected {
                sealed_index: 9,
                account,
                code: fixture.code.clone(),
                order_id: OrderId(7),
                reason: expected,
            }
        );
    }
}

#[test]
fn same_tick_p3_envelope_can_be_canceled_and_released() {
    let fixture = CancelFixture::p3_created(8, 100, Money::from_cents(99_500));
    let key = fixture.snapshot.envelope.key().clone();

    let output = cancel_continuous_order(fixture.input(10, AccountId(1), OrderId(8)))
        .expect("the stock book accepts a live same-tick cancellation");

    assert_eq!(output.market.resting_order_count(), 0);
    assert_eq!(output.terminal_key, Some(key.clone()));
    assert_eq!(
        output.fact,
        ContinuousCancelFact::Canceled {
            sealed_index: 10,
            account: AccountId(1),
            code: fixture.code,
            order_id: OrderId(8),
            side: Side::Buy,
            remaining_qty: 100,
        }
    );

    let mut ledger = EnvelopeLedger::new(4, [fixture.snapshot.envelope]).unwrap();
    let mut receipt = output.receipt.into_iter().collect::<Vec<_>>();
    ledger.apply(&mut receipt).unwrap();
    ledger.remove_terminal(&[key]).unwrap();
    assert_eq!(ledger.next_receipt_index(), 5);
}

#[test]
fn cancellation_after_an_earlier_fill_continues_the_receipt_audit_chain() {
    let code = StockCode("600888".to_owned());
    let key = envelope_key(&code, OrderId(7));
    let start_audit = audit(100, 0, Money::ZERO);
    let start =
        Envelope::tick_start_existing(key.clone(), Money::from_cents(100_000), 0, start_audit);
    let after_fill_audit = audit(90, 10, Money::from_cents(10_000));
    let after_fill =
        Envelope::tick_start_existing(key.clone(), Money::from_cents(90_000), 0, after_fill_audit);
    let market = resting_market(
        &code,
        Order {
            id: OrderId(7),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 90,
            original_qty: 100,
            filled_qty: 10,
            filled_value: Money::from_cents(10_000),
            owner: AccountId(1),
            seq: 0,
        },
    );
    let input = ContinuousCancelInput {
        market,
        envelopes: vec![ContinuousEnvelopeSnapshot {
            envelope: after_fill,
            audit: after_fill_audit,
        }],
        operation: ContinuousCancelOperation {
            sealed_index: 41,
            account: AccountId(1),
            code: code.clone(),
            order_id: OrderId(7),
        },
    };

    let output = cancel_continuous_order(input).unwrap();
    let cancel = output.receipt.expect("cancel after fill emits a release");
    assert_eq!(cancel.qty_before, 90);
    assert_eq!(cancel.qty_after, 90);
    assert_eq!(cancel.value_before, Money::from_cents(10_000));
    assert_eq!(cancel.value_after, Money::from_cents(10_000));
    assert_eq!(
        cancel.delta.released,
        ResVec::new(Money::from_cents(90_000), 0)
    );
    assert_eq!(
        cancel.local_key,
        ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(41),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap()
    );

    let fill = EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(40),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key.clone(),
        kind: ReceiptKind::Fill,
        qty_before: 100,
        qty_after: 90,
        value_before: Money::ZERO,
        value_after: Money::from_cents(10_000),
        delta: ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(10_000), 0),
            ResVec::ZERO,
            ResVec::new(Money::from_cents(90_000), 0),
        ),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty: 10,
        deliver_cash: Money::ZERO,
    };
    let mut ledger = EnvelopeLedger::new(11, [start]).unwrap();
    let mut receipts = [fill, cancel];
    ledger.apply(&mut receipts).unwrap();
    ledger.remove_terminal(&[key]).unwrap();
    assert_eq!(ledger.next_receipt_index(), 13);
    assert_eq!(ledger.terminal_count(), 1);
}

#[test]
fn seller_cancel_after_partial_fill_preserves_nonzero_nominal_and_charged_audit() {
    let code = StockCode("600888".to_owned());
    let config = GameConfig::proposed_defaults();
    let transition = transition::FillTransition::sell(transition::SellFillInput {
        config: &config,
        fill_qty: 50,
        remaining_qty_after: 50,
        filled_value_before: Money::ZERO,
        gross_delta: Money::from_cents(50_000),
        nominal_before: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
    })
    .unwrap();
    assert!(transition.nominal_after.total().unwrap() > Money::ZERO);
    assert!(transition.charged_after.total().unwrap() > Money::ZERO);
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(17),
        side: Side::Sell,
    };
    let partial_audit = EnvelopeAudit {
        limit: Money::from_cents(1_000),
        remaining_qty: 50,
        filled_qty: 50,
        filled_value: Money::from_cents(50_000),
        nominal: transition.nominal_after,
        charged: transition.charged_after,
    };
    let envelope = Envelope::tick_start_existing(key.clone(), Money::ZERO, 50, partial_audit);
    let market = resting_market(
        &code,
        Order {
            id: OrderId(17),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 50,
            original_qty: 100,
            filled_qty: 50,
            filled_value: Money::from_cents(50_000),
            owner: AccountId(1),
            seq: 0,
        },
    );

    let output = cancel_continuous_order(ContinuousCancelInput {
        market,
        envelopes: vec![ContinuousEnvelopeSnapshot {
            envelope: envelope.clone(),
            audit: partial_audit,
        }],
        operation: ContinuousCancelOperation {
            sealed_index: 88,
            account: AccountId(1),
            code,
            order_id: OrderId(17),
        },
    })
    .unwrap();
    let receipt = output.receipt.unwrap();
    assert_eq!(receipt.charged_before, transition.charged_after);
    assert_eq!(receipt.charged_after, transition.charged_after);
    assert_eq!(receipt.nominal, FeeComponents::ZERO);
    assert_eq!(receipt.charged, FeeComponents::ZERO);
    assert_eq!(receipt.delta.released, ResVec::new(Money::ZERO, 50));

    let mut ledger = EnvelopeLedger::new(20, [envelope]).unwrap();
    let mut receipts = [receipt];
    ledger.apply(&mut receipts).unwrap();
    ledger.remove_terminal(&[key]).unwrap();
}

#[test]
fn cancellation_fails_closed_for_missing_duplicate_or_drifted_snapshots() {
    let fixture = CancelFixture::tick_start(7, 100, 0, Money::ZERO, Money::from_cents(99_500));
    let operation = ContinuousCancelOperation {
        sealed_index: 1,
        account: AccountId(1),
        code: fixture.code.clone(),
        order_id: OrderId(7),
    };
    let missing = cancel_continuous_order(ContinuousCancelInput {
        market: fixture.market.clone(),
        envelopes: Vec::new(),
        operation: operation.clone(),
    });
    assert!(missing.is_err());

    let duplicate = cancel_continuous_order(ContinuousCancelInput {
        market: fixture.market.clone(),
        envelopes: vec![fixture.snapshot.clone(), fixture.snapshot.clone()],
        operation: operation.clone(),
    });
    assert!(duplicate.is_err());

    let mut drifted = fixture.snapshot.clone();
    drifted.audit.remaining_qty = 99;
    let drift = cancel_continuous_order(ContinuousCancelInput {
        market: fixture.market,
        envelopes: vec![drifted],
        operation,
    });
    assert!(drift.is_err());
}

#[test]
fn cancellation_with_another_stock_code_is_an_explicit_unknown_stock_rejection() {
    let fixture = CancelFixture::tick_start(7, 100, 0, Money::ZERO, Money::from_cents(99_500));
    let unknown = StockCode("600999".to_owned());
    let output = cancel_continuous_order(ContinuousCancelInput {
        market: fixture.market,
        envelopes: vec![fixture.snapshot],
        operation: ContinuousCancelOperation {
            sealed_index: 2,
            account: AccountId(1),
            code: unknown.clone(),
            order_id: OrderId(7),
        },
    })
    .unwrap();

    assert_eq!(
        output.fact,
        ContinuousCancelFact::Rejected {
            sealed_index: 2,
            account: AccountId(1),
            code: unknown,
            order_id: OrderId(7),
            reason: ContinuousCancelRejection::UnknownStock,
        }
    );
    assert!(output.receipt.is_none());
    assert_unchanged_resting_order(&output.market, OrderId(7), AccountId(1), 100);
}

#[test]
fn stock_worker_assigns_time_priority_from_supplied_order_not_sealed_identity() {
    let code = StockCode("600888".to_owned());
    let (mut operations, config) = validated_operations(vec![
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(990),
            qty: 100,
        },
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(990),
            qty: 100,
        },
    ]);
    operations.swap(0, 1);
    let expected = operations
        .iter()
        .map(|operation| place_draft(operation).order_id())
        .collect::<Vec<_>>();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations,
        config,
    })
    .unwrap();

    assert_eq!(
        output
            .market
            .resting_orders()
            .iter()
            .map(|order| order.id)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn continuous_limit_place_rests_with_its_preallocated_id_and_live_draft() {
    let code = StockCode("600888".to_owned());
    let (operations, config) = validated_operations(vec![Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(990),
        qty: 100,
    }]);
    let draft = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations,
        config,
    })
    .unwrap();

    assert_eq!(output.created_envelopes.len(), 1);
    assert_eq!(output.created_envelopes[0].key(), draft.key());
    assert_eq!(output.created_envelopes[0].live(), draft.required());
    assert!(output.receipts.is_empty());
    assert!(output.terminal_keys.is_empty());
    assert!(output.trades.is_empty());
    assert!(output.cancel_facts.is_empty());
    assert_eq!(
        output.place_facts,
        vec![ContinuousPlaceFact::Resting {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code: code.clone(),
            order_id: draft.order_id(),
            side: Side::Buy,
            price: Money::from_cents(990),
            remaining_qty: 100,
        }]
    );
    assert_unchanged_resting_order(&output.market, draft.order_id(), draft.owner(), 100);
}

#[test]
fn price_cage_reject_consumes_the_preallocated_id_and_releases_the_draft() {
    let code = StockCode("600888".to_owned());
    let (operations, config) = validated_operations(vec![Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(1_021),
        qty: 100,
    }]);
    let draft = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations,
        config,
    })
    .unwrap();

    assert!(output.market.resting_orders().is_empty());
    assert_created_draft(&output, &draft);
    assert_eq!(output.receipts.len(), 1);
    assert_eq!(output.receipts[0].index, 0);
    assert_eq!(output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(output.receipts[0].envelope, *draft.key());
    assert_eq!(output.receipts[0].delta.released, draft.required());
    assert_eq!(output.receipts[0].delta.live_after, ResVec::ZERO);
    assert_eq!(output.terminal_keys, vec![draft.key().clone()]);
    assert_eq!(
        output.place_facts,
        vec![ContinuousPlaceFact::Rejected {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code,
            order_id: draft.order_id(),
            reason: RejectionReason::PriceCageExceeded,
        }]
    );
    validate_worker_receipts(&[], &output);
}

#[test]
fn limit_reject_after_dynamic_cage_still_terminates_the_preallocated_draft() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let maker = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(2),
        OrderId(100),
        Side::Sell,
        Money::from_cents(1_100),
        100,
    );
    let (operations, config) = validated_operations(vec![Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(1_101),
        qty: 100,
    }]);
    let draft = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations,
        config,
    })
    .unwrap();

    assert_unchanged_resting_order(&output.market, OrderId(100), AccountId(2), 100);
    assert_created_draft(&output, &draft);
    assert_eq!(output.receipts.len(), 1);
    assert_eq!(output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(output.terminal_keys, vec![draft.key().clone()]);
    assert_eq!(
        output.place_facts,
        vec![ContinuousPlaceFact::Rejected {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code,
            order_id: draft.order_id(),
            reason: RejectionReason::LimitExceeded,
        }]
    );
    validate_worker_receipts(&[maker], &output);
}

#[test]
fn auction_market_reject_releases_the_draft_without_routing_to_the_book() {
    let code = StockCode("600888".to_owned());
    let (operations, config) = validated_operations(vec![Intent::PlaceMarket {
        code: code.clone(),
        side: Side::Buy,
        qty: 100,
    }]);
    let draft = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::CallAuction,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations,
        config,
    })
    .unwrap();

    assert!(output.market.resting_orders().is_empty());
    assert_created_draft(&output, &draft);
    assert_eq!(output.receipts.len(), 1);
    assert_eq!(output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(output.terminal_keys, vec![draft.key().clone()]);
    assert_eq!(
        output.place_facts,
        vec![ContinuousPlaceFact::Rejected {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code,
            order_id: draft.order_id(),
            reason: RejectionReason::AuctionLimitOrderRequired,
        }]
    );
    validate_worker_receipts(&[], &output);
}

#[test]
fn continuous_market_without_liquidity_releases_the_draft_and_reports_zero_filled() {
    let code = StockCode("600888".to_owned());
    let (operations, config) = validated_operations(vec![Intent::PlaceMarket {
        code: code.clone(),
        side: Side::Buy,
        qty: 100,
    }]);
    let draft = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: empty_market(&code),
        envelopes: Vec::new(),
        operations,
        config,
    })
    .unwrap();

    assert!(output.market.resting_orders().is_empty());
    assert!(output.trades.is_empty());
    assert_created_draft(&output, &draft);
    assert_eq!(output.receipts.len(), 1);
    let release = &output.receipts[0];
    assert_eq!(release.kind, ReceiptKind::Release);
    assert_eq!(release.envelope, *draft.key());
    assert_eq!(
        release.local_key,
        sealed_key(draft.sealed_index(), draft.key().clone(), 0)
    );
    assert_eq!(release.delta.spent, ResVec::ZERO);
    assert_eq!(release.delta.released, draft.required());
    assert_eq!(release.delta.live_after, ResVec::ZERO);
    assert_eq!(output.terminal_keys, vec![draft.key().clone()]);
    assert_eq!(
        output.place_facts,
        vec![ContinuousPlaceFact::Filled {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code,
            order_id: draft.order_id(),
            side: Side::Buy,
            // `Filled` describes market-order execution quantity; zero is an explicit accepted,
            // unfilled result whose escrow is released, not a rejection or a resting order.
            filled_qty: 0,
        }]
    );
    validate_worker_receipts(&[], &output);
}

#[test]
fn continuous_market_partial_fill_releases_remainder_with_the_next_ordinal() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let maker = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(1),
        OrderId(100),
        Side::Sell,
        Money::from_cents(1_000),
        100,
    );
    let (operations, config) = validated_operations(vec![Intent::PlaceMarket {
        code: code.clone(),
        side: Side::Buy,
        qty: 200,
    }]);
    let draft = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations,
        config,
    })
    .unwrap();

    assert!(output.market.resting_orders().is_empty());
    assert_eq!(output.trades.len(), 1);
    assert_created_draft(&output, &draft);
    assert_eq!(output.receipts.len(), 3);
    let incoming: Vec<_> = output
        .receipts
        .iter()
        .filter(|receipt| receipt.envelope == *draft.key())
        .collect();
    assert_eq!(incoming.len(), 2);
    assert_eq!(incoming[0].kind, ReceiptKind::Fill);
    assert_eq!(incoming[0].qty_before, 200);
    assert_eq!(incoming[0].qty_after, 100);
    assert_eq!(
        incoming[0].local_key,
        sealed_key(draft.sealed_index(), draft.key().clone(), 0)
    );
    assert_eq!(incoming[1].kind, ReceiptKind::Release);
    assert_eq!(incoming[1].qty_before, 100);
    assert_eq!(incoming[1].qty_after, 100);
    assert_eq!(
        incoming[1].local_key,
        sealed_key(draft.sealed_index(), draft.key().clone(), 1)
    );
    assert_eq!(incoming[1].delta.spent, ResVec::ZERO);
    assert_ne!(incoming[1].delta.released, ResVec::ZERO);
    assert_eq!(incoming[1].delta.live_after, ResVec::ZERO);
    assert!(output.terminal_keys.contains(draft.key()));
    assert!(output.terminal_keys.contains(maker.envelope.key()));
    assert_eq!(
        output.place_facts,
        vec![ContinuousPlaceFact::Filled {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code,
            order_id: draft.order_id(),
            side: Side::Buy,
            filled_qty: 100,
        }]
    );
    validate_worker_receipts(&[maker], &output);
}

#[test]
fn continuous_match_emits_per_envelope_ordinals_and_unnumbered_valid_receipts() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let first = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(1),
        OrderId(100),
        Side::Sell,
        Money::from_cents(1_000),
        100,
    );
    let second = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(2),
        OrderId(101),
        Side::Sell,
        Money::from_cents(1_000),
        100,
    );
    let (operations, config) = validated_operations(vec![Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(1_000),
        qty: 200,
    }]);
    let draft = place_draft(&operations[0]).clone();
    let sealed_index = draft.sealed_index();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![first.clone(), second.clone()],
        operations,
        config,
    })
    .unwrap();

    assert_created_draft(&output, &draft);
    assert_eq!(output.trades.len(), 2);
    assert_eq!(output.trades[0].stock, code);
    assert_eq!(output.trades[1].stock, code);
    assert_eq!(output.trades[0].triggering_sealed_index, sealed_index);
    assert_eq!(output.trades[1].triggering_sealed_index, sealed_index);
    assert_eq!(output.trades[0].stock_local_trade_event_index, 0);
    assert_eq!(output.trades[1].stock_local_trade_event_index, 1);
    assert_eq!(output.trades[0].trade.maker_order_id, OrderId(100));
    assert_eq!(output.trades[1].trade.maker_order_id, OrderId(101));
    assert_eq!(output.receipts.len(), 4);
    assert!(output.receipts.iter().all(|receipt| receipt.index == 0));
    let mut actual_keys: Vec<_> = output
        .receipts
        .iter()
        .map(|receipt| receipt.local_key.clone())
        .collect();
    actual_keys.sort();
    let mut expected_keys = vec![
        sealed_key(sealed_index, first.envelope.key().clone(), 0),
        sealed_key(sealed_index, second.envelope.key().clone(), 0),
        sealed_key(sealed_index, draft.key().clone(), 0),
        sealed_key(sealed_index, draft.key().clone(), 1),
    ];
    expected_keys.sort();
    assert_eq!(actual_keys, expected_keys);
    assert_eq!(output.terminal_keys.len(), 3);
    assert!(output.terminal_keys.contains(first.envelope.key()));
    assert!(output.terminal_keys.contains(second.envelope.key()));
    assert!(output.terminal_keys.contains(draft.key()));
    let seller_receipts: Vec<_> = output
        .receipts
        .iter()
        .filter(|receipt| receipt.envelope.side == Side::Sell)
        .collect();
    assert_eq!(seller_receipts.len(), 2);
    assert!(seller_receipts
        .iter()
        .all(|receipt| receipt.nominal.total().unwrap() > Money::ZERO));
    assert!(seller_receipts
        .iter()
        .all(|receipt| receipt.charged.total().unwrap() > Money::ZERO));
    assert!(output.market.resting_orders().is_empty());
    for (trade_fact, receipt_pair) in output.trades.iter().zip(output.receipts.chunks_exact(2)) {
        assert_eq!(
            receipt_pair[0].qty_before - receipt_pair[0].qty_after,
            trade_fact.trade.qty
        );
        assert_eq!(
            receipt_pair[1].qty_before - receipt_pair[1].qty_after,
            trade_fact.trade.qty
        );
        let gross = trade_fact
            .trade
            .price
            .mul_shares(trade_fact.trade.qty)
            .unwrap();
        assert_eq!(
            receipt_pair[0]
                .value_after
                .sub(receipt_pair[0].value_before)
                .unwrap(),
            gross
        );
        assert_eq!(
            receipt_pair[1]
                .value_after
                .sub(receipt_pair[1].value_before)
                .unwrap(),
            gross
        );
    }
    validate_worker_receipts(&[first, second], &output);
}

#[test]
fn same_stock_competing_limits_fill_in_received_order_with_descending_identities() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let maker = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(1),
        OrderId(100),
        Side::Sell,
        Money::from_cents(1_000),
        100,
    );
    let (mut operations, config) = validated_operations(vec![
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    ]);
    let first_received = place_draft(&operations[1]).order_id();
    let later = place_draft(&operations[0]).order_id();
    operations.reverse();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![maker.clone()],
        operations,
        config,
    })
    .unwrap();

    assert_eq!(output.trades.len(), 1);
    assert_eq!(output.trades[0].triggering_sealed_index, 1);
    assert_eq!(output.trades[0].trade.maker_order_id, OrderId(100));
    assert_eq!(output.trades[0].trade.taker_order_id, first_received);
    assert_eq!(output.trades[0].trade.qty, 100);
    assert_eq!(output.market.resting_orders().len(), 1);
    assert_eq!(output.market.resting_orders()[0].id, later);
    assert_eq!(output.receipts.len(), 2);
    assert!(output
        .receipts
        .iter()
        .all(|receipt| { receipt.local_key.source() == ReceiptSource::SealedIntent(1) }));
    validate_worker_receipts(&[maker], &output);
}

#[test]
fn trade_fact_identity_spans_multiple_sealed_operations_legs_and_makers() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let makers = [
        add_resting_snapshot(
            &mut market,
            &code,
            AccountId(1),
            OrderId(100),
            Side::Sell,
            Money::from_cents(1_000),
            100,
        ),
        add_resting_snapshot(
            &mut market,
            &code,
            AccountId(2),
            OrderId(101),
            Side::Sell,
            Money::from_cents(1_000),
            100,
        ),
        add_resting_snapshot(
            &mut market,
            &code,
            AccountId(3),
            OrderId(102),
            Side::Sell,
            Money::from_cents(1_000),
            100,
        ),
    ];
    let (operations, config) = validated_operations(vec![
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 200,
        },
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 100,
        },
    ]);

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: makers.to_vec(),
        operations,
        config,
    })
    .unwrap();

    assert_eq!(output.trades.len(), 3);
    assert_eq!(
        output
            .trades
            .iter()
            .map(|fact| (
                fact.triggering_sealed_index,
                fact.stock_local_trade_event_index,
                fact.trade.maker_order_id,
                fact.trade.qty,
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 0, OrderId(100), 100),
            (0, 1, OrderId(101), 100),
            (1, 2, OrderId(102), 100),
        ]
    );
    assert!(output.trades.iter().all(|fact| fact.stock == code));
    assert_eq!(output.place_facts.len(), 2);
    assert!(matches!(
        output.place_facts[0],
        ContinuousPlaceFact::Filled {
            sealed_index: 0,
            filled_qty: 200,
            ..
        }
    ));
    assert!(matches!(
        output.place_facts[1],
        ContinuousPlaceFact::Filled {
            sealed_index: 1,
            filled_qty: 100,
            ..
        }
    ));
    assert_eq!(output.receipts.len(), 6);
    for (trade_fact, receipt_pair) in output.trades.iter().zip(output.receipts.chunks_exact(2)) {
        assert!(receipt_pair.iter().all(|receipt| receipt.local_key.source()
            == ReceiptSource::SealedIntent(trade_fact.triggering_sealed_index)));
        assert!(receipt_pair
            .iter()
            .all(|receipt| receipt.qty_before - receipt.qty_after == trade_fact.trade.qty));
    }
    validate_worker_receipts(&makers, &output);
}

#[test]
fn account_facts_use_the_triggering_sealed_operation_as_identity() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let existing = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(0),
        OrderId(100),
        Side::Buy,
        Money::from_cents(980),
        100,
    );
    let (operations, config) = validated_operations(vec![
        Intent::Cancel {
            code: code.clone(),
            id: OrderId(100),
        },
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(990),
            qty: 100,
        },
    ]);
    let draft = place_draft(&operations[1]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![existing.clone()],
        operations,
        config,
    })
    .unwrap();

    assert_eq!(output.cancel_facts.len(), 1);
    assert_eq!(output.place_facts.len(), 1);
    assert_eq!(
        output.cancel_facts[0],
        ContinuousCancelFact::Canceled {
            sealed_index: 0,
            account: AccountId(0),
            code: code.clone(),
            order_id: OrderId(100),
            side: Side::Buy,
            remaining_qty: 100,
        }
    );
    assert_eq!(
        output.place_facts[0],
        ContinuousPlaceFact::Resting {
            sealed_index: 1,
            account: AccountId(0),
            code,
            order_id: draft.order_id(),
            side: Side::Buy,
            price: Money::from_cents(990),
            remaining_qty: 100,
        }
    );
    validate_worker_receipts(&[existing], &output);
}

#[test]
fn each_stock_worker_numbers_trade_facts_from_zero() {
    for code in [
        StockCode("600888".to_owned()),
        StockCode("000001".to_owned()),
    ] {
        let mut market = empty_market(&code);
        let maker = add_resting_snapshot(
            &mut market,
            &code,
            AccountId(1),
            OrderId(100),
            Side::Sell,
            Money::from_cents(1_000),
            100,
        );
        let (operations, config) = validated_operations_for_stock(
            &code,
            vec![Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            }],
        );
        let sealed_index = operations[0].sealed_index();

        let output = process_continuous_stock(ContinuousStockInput {
            phase: TradingPhase::Continuous,
            market,
            envelopes: vec![maker.clone()],
            operations,
            config,
        })
        .unwrap();

        assert_eq!(output.trades.len(), 1);
        assert_eq!(output.trades[0].stock, code);
        assert_eq!(output.trades[0].triggering_sealed_index, sealed_index);
        assert_eq!(output.trades[0].stock_local_trade_event_index, 0);
        validate_worker_receipts(&[maker], &output);
    }
}

#[test]
fn trade_fact_index_overflow_is_atomic() {
    let code = StockCode("600888".to_owned());
    let trade = Trade {
        price: Money::from_cents(1_000),
        qty: 100,
        maker: AccountId(1),
        taker: AccountId(2),
        maker_order_id: OrderId(10),
        taker_order_id: OrderId(11),
        maker_filled_value_before: Money::ZERO,
        taker_filled_value_before: Money::ZERO,
    };
    let sentinel = ContinuousTradeFact {
        stock: code.clone(),
        triggering_sealed_index: 6,
        stock_local_trade_event_index: 9,
        trade: trade.clone(),
    };
    let mut facts = vec![sentinel.clone()];
    let mut next = u64::MAX;

    let result = append_trade_facts(&code, 7, &[trade], &mut next, &mut facts);

    assert!(result.is_err());
    assert_eq!(next, u64::MAX);
    assert_eq!(facts, vec![sentinel]);
}

#[test]
fn partial_seller_fill_keeps_live_shares_and_nonzero_fee_audit() {
    let code = StockCode("600888".to_owned());
    let mut market = empty_market(&code);
    let seller = add_resting_snapshot(
        &mut market,
        &code,
        AccountId(1),
        OrderId(100),
        Side::Sell,
        Money::from_cents(1_000),
        200,
    );
    let (operations, config) = validated_operations(vec![Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(1_000),
        qty: 100,
    }]);
    let incoming = place_draft(&operations[0]).clone();

    let output = process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes: vec![seller.clone()],
        operations,
        config,
    })
    .unwrap();

    assert_created_draft(&output, &incoming);
    let seller_receipt = output
        .receipts
        .iter()
        .find(|receipt| receipt.envelope == *seller.envelope.key())
        .unwrap();
    assert_eq!(seller_receipt.qty_before, 200);
    assert_eq!(seller_receipt.qty_after, 100);
    assert_eq!(seller_receipt.delta.spent, ResVec::new(Money::ZERO, 100));
    assert_eq!(
        seller_receipt.delta.live_after,
        ResVec::new(Money::ZERO, 100)
    );
    assert!(seller_receipt.nominal.total().unwrap() > Money::ZERO);
    assert!(seller_receipt.charged.total().unwrap() > Money::ZERO);
    assert!(!output.terminal_keys.contains(seller.envelope.key()));
    assert!(output.terminal_keys.contains(incoming.key()));
    assert_unchanged_resting_order(&output.market, OrderId(100), AccountId(1), 100);
    validate_worker_receipts(&[seller], &output);
}

struct CancelFixture {
    code: StockCode,
    market: Market,
    snapshot: ContinuousEnvelopeSnapshot,
}

impl CancelFixture {
    fn tick_start(
        order_id: u64,
        remaining_qty: u32,
        filled_qty: u32,
        filled_value: Money,
        live_cash: Money,
    ) -> Self {
        Self::new(
            order_id,
            remaining_qty,
            filled_qty,
            filled_value,
            live_cash,
            false,
        )
    }

    fn p3_created(order_id: u64, remaining_qty: u32, live_cash: Money) -> Self {
        Self::new(order_id, remaining_qty, 0, Money::ZERO, live_cash, true)
    }

    fn new(
        order_id: u64,
        remaining_qty: u32,
        filled_qty: u32,
        filled_value: Money,
        live_cash: Money,
        p3_created: bool,
    ) -> Self {
        let code = StockCode("600888".to_owned());
        let order = Order {
            id: OrderId(order_id),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: remaining_qty,
            original_qty: remaining_qty + filled_qty,
            filled_qty,
            filled_value,
            owner: AccountId(1),
            seq: 0,
        };
        let market = resting_market(&code, order);
        let envelope_audit = audit(remaining_qty, filled_qty, filled_value);
        let key = envelope_key(&code, OrderId(order_id));
        let envelope = if p3_created {
            Envelope::p3_created(key, live_cash, 0, envelope_audit)
        } else {
            Envelope::tick_start_existing(key, live_cash, 0, envelope_audit)
        };
        Self {
            code,
            market,
            snapshot: ContinuousEnvelopeSnapshot {
                envelope,
                audit: envelope_audit,
            },
        }
    }

    fn input(
        &self,
        sealed_index: u64,
        account: AccountId,
        order_id: OrderId,
    ) -> ContinuousCancelInput {
        ContinuousCancelInput {
            market: self.market.clone(),
            envelopes: vec![self.snapshot.clone()],
            operation: ContinuousCancelOperation {
                sealed_index,
                account,
                code: self.code.clone(),
                order_id,
            },
        }
    }
}

fn resting_market(code: &StockCode, order: Order) -> Market {
    let mut market = Market::new(
        code.clone(),
        Money::from_cents(1_000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap();
    let result = market.place(order).unwrap();
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    market
}

fn envelope_key(code: &StockCode, order: OrderId) -> EnvelopeKey {
    EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order,
        side: Side::Buy,
    }
}

fn audit(remaining_qty: u32, filled_qty: u32, filled_value: Money) -> EnvelopeAudit {
    EnvelopeAudit {
        limit: Money::from_cents(1_000),
        remaining_qty,
        filled_qty,
        filled_value,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    }
}

fn assert_unchanged_resting_order(market: &Market, id: OrderId, owner: AccountId, qty: u32) {
    let orders = market.resting_orders();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].id, id);
    assert_eq!(orders[0].owner, owner);
    assert_eq!(orders[0].qty, qty);
}

fn validated_operations(intents: Vec<Intent>) -> (Vec<P3ValidatedOperation>, GameConfig) {
    let code = StockCode("600888".to_owned());
    validated_operations_for_stock(&code, intents)
}

fn validated_operations_for_stock(
    code: &StockCode,
    intents: Vec<Intent>,
) -> (Vec<P3ValidatedOperation>, GameConfig) {
    let account = AccountId(0);
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(index, intent)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                account,
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::new(candidates).unwrap();
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            SecurityCategory::MainBoard,
            Money::from_cents(1_100),
            Money::from_cents(900),
        ),
    )])
    .unwrap();
    let config = game.setup.config.clone();
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();
    assert_eq!(output.rejected().count(), 0);
    (output.operations().to_vec(), config)
}

fn place_draft(operation: &P3ValidatedOperation) -> &EnvelopeDraft {
    match operation {
        P3ValidatedOperation::Place(draft) => draft,
        P3ValidatedOperation::Cancel { .. } => panic!("expected a P3 place operation"),
    }
}

fn empty_market(code: &StockCode) -> Market {
    Market::new(
        code.clone(),
        Money::from_cents(1_000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap()
}

fn add_resting_snapshot(
    market: &mut Market,
    code: &StockCode,
    owner: AccountId,
    order_id: OrderId,
    side: Side,
    price: Money,
    qty: u32,
) -> ContinuousEnvelopeSnapshot {
    let order = Order {
        id: order_id,
        side,
        price,
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner,
        seq: 0,
    };
    let result = market.place(order).unwrap();
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    let key = EnvelopeKey {
        account: owner,
        stock: code.clone(),
        order: order_id,
        side,
    };
    let envelope_audit = EnvelopeAudit {
        limit: price,
        remaining_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    };
    let (cash, shares) = match side {
        Side::Buy => (
            crate::session::buy_order_reservation(
                &GameConfig::proposed_defaults(),
                price,
                qty,
                Money::ZERO,
            )
            .unwrap(),
            0,
        ),
        Side::Sell => (Money::ZERO, qty),
    };
    ContinuousEnvelopeSnapshot {
        envelope: Envelope::tick_start_existing(key, cash, shares, envelope_audit),
        audit: envelope_audit,
    }
}

fn sealed_key(sealed_index: u64, envelope: EnvelopeKey, ordinal: u64) -> ReceiptLocalKey {
    ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(sealed_index),
        ReceiptTransition { envelope, ordinal },
    )
    .unwrap()
}

fn assert_created_draft(output: &ContinuousStockOutput, draft: &EnvelopeDraft) {
    assert_eq!(output.created_envelopes.len(), 1);
    assert_eq!(output.created_envelopes[0].key(), draft.key());
    assert_eq!(output.created_envelopes[0].live(), draft.required());
}

fn validate_worker_receipts(
    initial: &[ContinuousEnvelopeSnapshot],
    output: &ContinuousStockOutput,
) {
    let envelopes = initial
        .iter()
        .map(|snapshot| snapshot.envelope.clone())
        .chain(output.created_envelopes.iter().cloned());
    let mut ledger = EnvelopeLedger::new(100, envelopes).unwrap();
    let mut receipts = output.receipts.clone();
    ledger.apply(&mut receipts).unwrap();
    ledger.remove_terminal(&output.terminal_keys).unwrap();
    assert_eq!(ledger.next_receipt_index(), 100 + receipts.len() as u64);
}
