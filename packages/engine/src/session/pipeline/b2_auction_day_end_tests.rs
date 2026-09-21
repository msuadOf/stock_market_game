use super::super::*;
use super::b2_auction_day_end::*;
use crate::plans::{
    OpinionSource, PlanEvent, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, Urgency,
};
use crate::session::pipeline::p3_context::build_p3_validation_context;
use crate::session::{ParentOrderPlan, PendingPlanEvent, RetailOrderDiagnosticEvent};
use crate::{
    AccountId, Event, Intent, Money, Order, OrderId, PlanId, Side, StockCode, TradingPhase,
};

#[test]
fn nonfinal_opening_tick_drains_limit_order_before_one_indicative_tail() {
    let mut session = opening_session(0);
    let code = only_code(&session);
    let order_id = OrderId(session.next_order_id);
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        )],
    );

    let output =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .unwrap();

    assert_eq!(session.tick(), 1);
    assert_eq!(output.finalizer, audit(1, 0, 0));
    assert_eq!(count_events(&output.events, is_auction_tick), 1);
    assert_eq!(count_events(&output.events, is_auction_completed), 0);
    assert_eq!(count_events(&output.events, is_day_boundary), 0);
    assert!(output.receipts.is_empty());
    assert_eq!(output.p6.settlement.applied_receipts, 0);
    assert_eq!(session.markets[&code].resting_order_count(), 0);
    let queued = &session.auction_orders[&code];
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].arrival_seq, order_id.0);
    assert_eq!(queued[0].qty, 100);
    assert_eq!(
        session
            .envelope_ledger
            .iter()
            .map(|(key, _)| key.order)
            .collect::<Vec<_>>(),
        vec![order_id]
    );
}

#[test]
fn auction_market_order_is_rejected_after_p3_and_consumes_its_order_id() {
    let mut session = opening_session(0);
    let code = only_code(&session);
    let rejected_id = OrderId(session.next_order_id);
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(0),
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        )],
    );

    let output =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .unwrap();

    assert_eq!(session.next_order_id, rejected_id.0 + 1);
    assert!(session.auction_orders.is_empty());
    assert_eq!(session.envelope_ledger.iter().count(), 0);
    assert_eq!(output.receipts.len(), 1);
    assert_eq!(output.receipts[0].index, 0);
    assert_eq!(output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(output.receipts[0].envelope.order, rejected_id);
    assert_eq!(
        output.receipts[0].local_key,
        receipt_key(
            ReceiptSource::SealedIntent(0),
            output.receipts[0].envelope.clone(),
            0,
        )
    );
    assert!(output.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account: AccountId(0),
            code: rejected_code,
            reason: crate::RejectionReason::AuctionLimitOrderRequired,
            ..
        } if rejected_code == &code
    )));
    assert_eq!(output.p6.settlement.applied_receipts, 0);
}

#[test]
fn opening_0920_boundary_and_every_closing_tick_reject_cancellation() {
    let code = StockCode("600888".to_owned());
    for (mut session, expected_phase) in [
        (opening_session(300), TradingPhase::CallAuction),
        (closing_session(90), TradingPhase::ClosingAuction),
    ] {
        assert_eq!(session.phase(), expected_phase);
        install_auction_orders(
            &mut session,
            code.clone(),
            vec![auction_order(0, 10, Side::Buy, 990, 100)],
        );
        session.next_order_id = 11;
        session.hydrate_or_validate_envelope_ledger().unwrap();
        let (candidates, validation) = prepare(
            &session,
            vec![(
                AccountId(0),
                Intent::Cancel {
                    code: code.clone(),
                    id: OrderId(10),
                },
            )],
        );

        let output =
            apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
                .unwrap();

        assert!(output.receipts.is_empty());
        assert_eq!(session.auction_orders[&code].len(), 1);
        assert!(output.events.iter().any(|event| matches!(
            event,
            Event::IntentRejected {
                reason: crate::RejectionReason::AuctionOrderNotCancelable,
                ..
            }
        )));
    }
}

#[test]
fn opening_before_0920_cancels_existing_envelope_through_sealed_receipt() {
    let mut session = opening_session(299);
    let code = only_code(&session);
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![auction_order(0, 10, Side::Buy, 990, 100)],
    );
    session.next_order_id = 11;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let key = EnvelopeKey {
        account: AccountId(0),
        stock: code.clone(),
        order: OrderId(10),
        side: Side::Buy,
    };
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(10),
            },
        )],
    );

    let output =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .unwrap();

    assert!(session.auction_orders.is_empty());
    assert_eq!(session.envelope_ledger.iter().count(), 0);
    assert_eq!(output.receipts.len(), 1);
    assert_eq!(output.receipts[0].kind, ReceiptKind::Release);
    assert_eq!(
        output.receipts[0].local_key,
        receipt_key(ReceiptSource::SealedIntent(0), key, 0)
    );
    assert!(output.events.iter().any(|event| matches!(
        event,
        Event::OrderCanceled {
            account: AccountId(0),
            code: canceled_code,
            id: OrderId(10),
            remaining_qty: 100,
            ..
        } if canceled_code == &code
    )));
}

#[test]
fn opening_completion_preserves_partial_remainder_identity_fifo_and_audit() {
    let mut session = opening_session(599);
    let code = only_code(&session);
    session
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![
            auction_order(0, 10, Side::Buy, 1_100, 300),
            auction_order(0, 11, Side::Buy, 1_100, 100),
            auction_order(1, 12, Side::Sell, 900, 100),
        ],
    );
    session.next_order_id = 13;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let original_key = EnvelopeKey {
        account: AccountId(0),
        stock: code.clone(),
        order: OrderId(10),
        side: Side::Buy,
    };
    let (candidates, validation) = prepare(&session, Vec::new());

    let output =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .unwrap();

    assert_eq!(output.finalizer, audit(1, 1, 0));
    assert_eq!(count_events(&output.events, is_auction_tick), 1);
    assert_eq!(count_events(&output.events, is_auction_completed), 1);
    assert_eq!(count_events(&output.events, is_trade), 1);
    assert!(session.auction_orders.is_empty());
    let resting = session.markets[&code].resting_orders();
    assert_eq!(resting.len(), 2);
    assert_eq!(resting[0].id, OrderId(10));
    assert_eq!(resting[1].id, OrderId(11));
    assert!(resting[0].seq < resting[1].seq);
    assert_eq!(resting[0].owner, AccountId(0));
    assert_eq!(resting[0].qty, 200);
    assert_eq!(resting[0].original_qty, 300);
    assert_eq!(resting[0].filled_qty, 100);
    assert_eq!(resting[0].filled_value, Money::from_cents(110_000));
    let live = session.envelope_ledger.get(&original_key).unwrap();
    assert_eq!(live.audit().remaining_qty, 200);
    assert_eq!(live.audit().filled_qty, 100);
    assert_eq!(live.audit().filled_value, Money::from_cents(110_000));
    assert!(output.receipts.iter().any(|receipt| {
        receipt.kind == ReceiptKind::Rollover
            && receipt.envelope == original_key
            && receipt.local_key.source() == ReceiptSource::Auction(0)
    }));
}

#[test]
fn opening_completion_applies_same_tick_accept_and_fill_to_linked_plan() {
    let mut session = opening_session(599);
    let code = only_code(&session);
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![auction_order(0, 30, Side::Sell, 900, 100)],
    );
    session.next_order_id = 31;
    let plan_id = session
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    install_parent_with_id(
        &mut session,
        AccountId(1),
        code.clone(),
        Side::Buy,
        100,
        None,
        0,
        plan_id,
    );
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(1),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_100),
                qty: 100,
            },
        )],
    );

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    let plan = session.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty, 100);
    assert_eq!(plan.status, PlanStatus::Completed);
    assert!(session.pending_plan_events.is_empty());
    assert!(session.parent_orders.is_empty());
}

#[test]
fn opening_accept_and_cancel_synchronize_the_linked_parent_before_commit() {
    let mut session = opening_session(0);
    let code = only_code(&session);
    let account = AccountId(1);
    let order_id = OrderId(session.next_order_id);
    install_parent(&mut session, account, code.clone(), Side::Buy, 200, None, 0);
    let (candidates, validation) = prepare(
        &session,
        vec![(
            account,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        )],
    );

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    let parent = &session.parent_orders[&account][&code];
    assert_eq!(parent.active_child_order_id, Some(order_id));
    assert_eq!(parent.active_child_remaining_qty, Some(100));
    assert!(matches!(
        session.pending_plan_events.as_slice(),
        [PendingPlanEvent::Accepted {
            plan_id: PlanId(700),
            order_id: accepted,
            trading_day: 0,
        }] if *accepted == order_id
    ));
    session.envelope_ledger.rebase_live_for_next_tick().unwrap();

    let (candidates, validation) = prepare(
        &session,
        vec![(
            account,
            Intent::Cancel {
                code: code.clone(),
                id: order_id,
            },
        )],
    );
    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    let parent = &session.parent_orders[&account][&code];
    assert_eq!(parent.active_child_order_id, None);
    assert_eq!(parent.active_child_remaining_qty, None);
}

#[test]
fn opening_accept_and_cancel_preserve_retail_order_lifecycle_identity() {
    let mut session = retail_opening_session(0);
    let code = only_code(&session);
    let retail = AccountId(1);
    let order_id = OrderId(session.next_order_id);
    let (candidates, validation) = prepare(
        &session,
        vec![(
            retail,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        )],
    );
    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();
    session.envelope_ledger.rebase_live_for_next_tick().unwrap();

    let (candidates, validation) = prepare(
        &session,
        vec![(
            retail,
            Intent::Cancel {
                code: code.clone(),
                id: order_id,
            },
        )],
    );
    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    assert!(matches!(
        session.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Submitted {
                account,
                code: submitted_code,
                order_id: submitted_id,
                qty: 100,
                ..
            },
            RetailOrderDiagnosticEvent::Canceled {
                account: canceled_account,
                code: canceled_code,
                order_id: canceled_id,
                remaining_qty: 100,
            }
        ] if *account == retail
            && submitted_code == &code
            && *submitted_id == order_id
            && *canceled_account == retail
            && canceled_code == &code
            && *canceled_id == order_id
    ));
}

#[test]
fn retail_rejections_and_successes_keep_exact_sealed_lifecycle_order() {
    let mut session = retail_opening_session(0);
    let code = only_code(&session);
    let retail = AccountId(1);
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![auction_order(1, 10, Side::Buy, 980, 100)],
    );
    session.next_order_id = 11;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(
        &session,
        vec![
            (
                retail,
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: Money::from_cents(990),
                    qty: 0,
                },
            ),
            (
                retail,
                Intent::PlaceMarket {
                    code: code.clone(),
                    side: Side::Buy,
                    qty: 100,
                },
            ),
            (
                retail,
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: Money::from_cents(990),
                    qty: 100,
                },
            ),
            (
                retail,
                Intent::Cancel {
                    code: code.clone(),
                    id: OrderId(10),
                },
            ),
        ],
    );

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    assert!(matches!(
        session.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(1),
                code: p3_code,
                reason: crate::RejectionReason::InvalidQuantity,
            },
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(1),
                code: p4_code,
                reason: crate::RejectionReason::AuctionLimitOrderRequired,
            },
            RetailOrderDiagnosticEvent::Submitted {
                account: AccountId(1),
                code: accepted_code,
                order_id: OrderId(12),
                qty: 100,
                ..
            },
            RetailOrderDiagnosticEvent::Canceled {
                account: AccountId(1),
                code: canceled_code,
                order_id: OrderId(10),
                remaining_qty: 100,
            }
        ] if p3_code == &code
            && p4_code == &code
            && accepted_code == &code
            && canceled_code == &code
    ));
}

#[test]
fn retail_0920_cancel_rejection_is_projected_from_the_typed_worker_fact() {
    let mut session = retail_opening_session(300);
    let code = only_code(&session);
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![auction_order(1, 10, Side::Buy, 980, 100)],
    );
    session.next_order_id = 11;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(1),
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(10),
            },
        )],
    );

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    assert!(matches!(
        session.last_retail_order_events(),
        [RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: rejected_code,
            reason: crate::RejectionReason::AuctionOrderNotCancelable,
        }] if rejected_code == &code
    ));
}

#[test]
fn closing_boundary_canonicalizes_sealed_auction_and_day_end_receipts() {
    let mut session = closing_session(99);
    let code = only_code(&session);
    session
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    session
        .markets
        .get_mut(&code)
        .unwrap()
        .place(order(20, 0, Side::Buy, 900, 100))
        .unwrap();
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![
            auction_order(0, 30, Side::Buy, 1_100, 200),
            auction_order(1, 40, Side::Sell, 900, 100),
        ],
    );
    session.next_order_id = 50;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let market_reject = EnvelopeKey {
        account: AccountId(0),
        stock: code.clone(),
        order: OrderId(50),
        side: Side::Buy,
    };
    let auction_buy = EnvelopeKey {
        account: AccountId(0),
        stock: code.clone(),
        order: OrderId(30),
        side: Side::Buy,
    };
    let auction_sell = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(40),
        side: Side::Sell,
    };
    let continuous = EnvelopeKey {
        account: AccountId(0),
        stock: code.clone(),
        order: OrderId(20),
        side: Side::Buy,
    };
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(0),
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        )],
    );

    let output =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .unwrap();

    let expected = vec![
        (
            ReceiptKind::Reject,
            ReceiptSource::SealedIntent(0),
            market_reject,
            0,
        ),
        (
            ReceiptKind::Fill,
            ReceiptSource::Auction(0),
            auction_buy.clone(),
            0,
        ),
        (
            ReceiptKind::Rollover,
            ReceiptSource::Auction(0),
            auction_buy.clone(),
            1,
        ),
        (
            ReceiptKind::Fill,
            ReceiptSource::Auction(1),
            auction_sell,
            0,
        ),
        (
            ReceiptKind::Release,
            ReceiptSource::DayEnd(0),
            continuous.clone(),
            0,
        ),
        (
            ReceiptKind::Release,
            ReceiptSource::DayEnd(1),
            auction_buy.clone(),
            0,
        ),
    ];
    assert_eq!(output.receipts.len(), expected.len());
    for (index, (receipt, (kind, source, envelope, ordinal))) in
        output.receipts.iter().zip(expected).enumerate()
    {
        assert_eq!(receipt.index, u64::try_from(index).unwrap());
        assert_eq!(receipt.kind, kind);
        assert_eq!(receipt.envelope, envelope);
        assert_eq!(receipt.local_key, receipt_key(source, envelope, ordinal));
    }
    let rollover = output
        .receipts
        .iter()
        .find(|receipt| receipt.kind == ReceiptKind::Rollover)
        .unwrap();
    let auction_day_end = output
        .receipts
        .iter()
        .find(|receipt| {
            receipt.envelope == auction_buy
                && matches!(receipt.local_key.source(), ReceiptSource::DayEnd(_))
        })
        .unwrap();
    assert_eq!(rollover.qty_after, auction_day_end.qty_before);
    assert_eq!(rollover.value_after, auction_day_end.value_before);
    assert_eq!(rollover.delta.live_after, auction_day_end.delta.released);
    assert_eq!(output.p6.settlement.applied_receipts, 2);
    assert_eq!(output.p6.settlement.applied_groups, 2);
    assert_eq!(output.finalizer, audit(1, 1, 1));
    assert_eq!(session.tick(), 100);
    assert_eq!(session.day(), 1);
    assert!(session.auction_orders.is_empty());
    assert_eq!(session.markets[&code].resting_order_count(), 0);
    assert_eq!(session.envelope_ledger.iter().count(), 0);
    assert_eq!(count_events(&output.events, is_auction_tick), 1);
    assert_eq!(count_events(&output.events, is_auction_completed), 1);
    assert_eq!(count_events(&output.events, is_trade), 1);
    assert_eq!(count_events(&output.events, is_day_boundary), 1);
    assert_eq!(count_events(&output.events, is_order_canceled), 2);
}

#[test]
fn closing_partial_fill_reaches_linked_plan_before_day_end_cleanup() {
    let mut session = closing_session(99);
    let code = only_code(&session);
    session
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![
            auction_order(0, 30, Side::Buy, 1_100, 200),
            auction_order(1, 40, Side::Sell, 900, 100),
        ],
    );
    install_parent(
        &mut session,
        AccountId(0),
        code.clone(),
        Side::Buy,
        200,
        Some((OrderId(30), 200)),
        0,
    );
    session.next_order_id = 41;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(&session, Vec::new());

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    assert!(session.parent_orders.is_empty());
    assert!(matches!(
        session.pending_plan_events.as_slice(),
        [
            PendingPlanEvent::Filled {
                plan_id: PlanId(700),
                order_id: OrderId(30),
                qty: 100,
                trading_day: 0,
            },
            PendingPlanEvent::DayEnded {
                plan_id: PlanId(700),
                trading_day: 0,
            }
        ]
    ));
}

#[test]
fn closing_partial_fill_is_applied_to_the_session_plan_before_checked_day_end() {
    let mut session = closing_session(99);
    let code = only_code(&session);
    session
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![
            auction_order(0, 30, Side::Buy, 1_100, 200),
            auction_order(1, 40, Side::Sell, 900, 100),
        ],
    );
    let plan_id = session
        .plans
        .create(PlanOpen {
            account: AccountId(0),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(200),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    session
        .plans
        .apply(
            plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id: OrderId(30),
                trading_day: 0,
            },
        )
        .unwrap();
    install_parent_with_id(
        &mut session,
        AccountId(0),
        code.clone(),
        Side::Buy,
        200,
        Some((OrderId(30), 200)),
        0,
        plan_id,
    );
    session.next_order_id = 41;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(&session, Vec::new());

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    let plan = session.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty, 100);
    assert_eq!(plan.active_child_order_id, None);
    assert_eq!(plan.status, PlanStatus::Active);
    assert!(session.pending_plan_events.is_empty());
    assert!(session.parent_orders.is_empty());
}

#[test]
fn closing_partial_fill_and_release_preserve_retail_order_lifecycle() {
    let mut session = retail_closing_session(99);
    let code = only_code(&session);
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![
            auction_order(1, 30, Side::Buy, 1_100, 200),
            auction_order(0, 40, Side::Sell, 900, 100),
        ],
    );
    session.next_order_id = 41;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(&session, Vec::new());

    apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation).unwrap();

    assert!(matches!(
        session.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Filled {
                account: AccountId(1),
                code: filled_code,
                side: Side::Buy,
                order_id: OrderId(30),
                qty: 100,
            },
            RetailOrderDiagnosticEvent::Canceled {
                account: AccountId(1),
                code: canceled_code,
                order_id: OrderId(30),
                remaining_qty: 100,
            }
        ] if filled_code == &code && canceled_code == &code
    ));
}

#[test]
fn linked_plan_day_end_capacity_reports_resource_limit_and_finishes_the_day() {
    let mut session = closing_session(99);
    let code = only_code(&session);
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![auction_order(1, 30, Side::Buy, 1_100, 100)],
    );
    install_parent(
        &mut session,
        AccountId(1),
        code,
        Side::Buy,
        100,
        Some((OrderId(30), 100)),
        0,
    );
    session.pending_plan_events = vec![
        PendingPlanEvent::DayEnded {
            plan_id: PlanId(999),
            trading_day: 0,
        };
        crate::session::MAX_SAVED_PLAN_EVENTS
    ];
    session.next_order_id = 31;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(&session, Vec::new());
    let result =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .expect("DayEnd capacity is a business resource limit");

    assert_eq!(session.day(), 1);
    assert_eq!(
        session.pending_plan_events.len(),
        crate::session::MAX_SAVED_PLAN_EVENTS
    );
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(
                event,
                Event::ResourceLimit {
                    resource: crate::session::RuntimeResource::PendingPlanEvents,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn closing_continuous_books_skip_auction_and_share_collision_free_day_end_events() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.closing_auction_ticks = 10;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.tick = 99;
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    for (offset, code) in codes.iter().enumerate() {
        session
            .markets
            .get_mut(code)
            .unwrap()
            .place(order(
                20 + u64::try_from(offset).unwrap(),
                0,
                Side::Buy,
                900,
                100,
            ))
            .unwrap();
    }
    session.next_order_id = 30;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let (candidates, validation) = prepare(&session, Vec::new());

    let output =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation)
            .unwrap();

    assert_eq!(count_events(&output.events, is_trade), 0);
    assert_eq!(count_events(&output.events, is_auction_tick), 2);
    assert_eq!(count_events(&output.events, is_auction_completed), 2);
    assert_eq!(count_events(&output.events, is_order_canceled), 2);
    assert_eq!(count_events(&output.events, is_day_boundary), 1);
    assert_eq!(output.receipts.len(), 2);
    assert_eq!(
        output
            .receipts
            .iter()
            .map(|receipt| receipt.index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(output.receipts[0].envelope < output.receipts[1].envelope);
    assert!(output.receipts.iter().all(|receipt| {
        receipt.kind == ReceiptKind::Release
            && receipt.local_key.source() == ReceiptSource::DayEnd(0)
    }));
    assert!(
        session
            .markets
            .values()
            .all(|market| market.resting_order_count() == 0)
    );
    assert_eq!(session.envelope_ledger.iter().count(), 0);
}

#[test]
fn worker_failure_keeps_the_authoritative_session_byte_identical() {
    let mut session = opening_session(0);
    let code = only_code(&session);
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        )],
    );
    session.setup.stocks[0].tick = Money::ZERO;
    let before = hashes(&session);

    let result =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation);

    assert!(matches!(result, Err(B2AuctionDayEndError::Worker { .. })));
    assert_eq!(hashes(&session), before);
}

#[test]
fn p5_cursor_failure_keeps_the_authoritative_session_byte_identical() {
    let mut session = opening_session(0);
    let code = only_code(&session);
    let (candidates, validation) = prepare(
        &session,
        vec![(
            AccountId(0),
            Intent::PlaceMarket {
                code,
                side: Side::Buy,
                qty: 100,
            },
        )],
    );
    session.next_receipt_base = 1;
    let before = hashes(&session);

    let result =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation);

    assert!(matches!(result, Err(B2AuctionDayEndError::P5(_))));
    assert_eq!(hashes(&session), before);
}

#[test]
fn p6_failure_keeps_the_authoritative_session_byte_identical() {
    let mut session = opening_session(599);
    let code = only_code(&session);
    session.next_order_id = 12;
    let (candidates, validation) = prepare(&session, Vec::new());
    install_auction_orders(
        &mut session,
        code.clone(),
        vec![
            auction_order(0, 10, Side::Buy, 1_100, 100),
            auction_order(99, 11, Side::Sell, 900, 100),
        ],
    );
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let before = hashes(&session);

    let result =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation);

    assert!(matches!(result, Err(B2AuctionDayEndError::P6(_))));
    assert_eq!(hashes(&session), before);
}

#[test]
fn p7_failure_keeps_the_authoritative_session_byte_identical() {
    let mut session = opening_session(0);
    let (candidates, validation) = prepare(&session, Vec::new());
    session.seq = u64::MAX;
    let before = hashes(&session);

    let result =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation);

    assert!(matches!(result, Err(B2AuctionDayEndError::P7(_))));
    assert_eq!(hashes(&session), before);
}

#[test]
fn late_day_finalizer_failure_rolls_back_workers_p5_p6_and_market_boundary() {
    let mut session = closing_session(99);
    let (candidates, validation) = prepare(&session, Vec::new());
    session.day = u32::MAX;
    let before = hashes(&session);

    let result =
        apply_session_b2_auction_day_end_transaction(&mut session, &candidates, &validation);

    assert!(matches!(result, Err(B2AuctionDayEndError::Precondition(_))));
    assert_eq!(hashes(&session), before);
}

fn prepare(
    session: &GameSession,
    intents: Vec<(AccountId, Intent)>,
) -> (P2CandidateBatch, P3ValidationOutput) {
    let plan = plan_tick(PhaseInput { session }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(index, (owner, intent))| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                owner,
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::from_unsorted(candidates).unwrap();
    let validation = P2P3Handoff::new_with_context(
        batch.clone(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(session).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap();
    (batch, validation)
}

fn opening_session(tick: u64) -> GameSession {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(900),
        42,
    )
    .unwrap();
    session.tick = tick;
    assert_eq!(session.phase(), TradingPhase::CallAuction);
    session
}

fn retail_opening_session(tick: u64) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.tick = tick;
    assert_eq!(session.phase(), TradingPhase::CallAuction);
    session
}

fn closing_session(tick: u64) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.closing_auction_ticks = 10;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.tick = tick;
    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
    session
}

fn retail_closing_session(tick: u64) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.closing_auction_ticks = 10;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.tick = tick;
    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
    session
}

fn only_code(session: &GameSession) -> StockCode {
    session.markets.keys().next().unwrap().clone()
}

fn install_auction_orders(
    session: &mut GameSession,
    code: StockCode,
    orders: Vec<crate::AuctionOrderSnap>,
) {
    for order in &orders {
        *session.auction_order_counts.entry(order.owner).or_default() += 1;
    }
    assert!(session.auction_orders.insert(code, orders).is_none());
}

fn install_parent(
    session: &mut GameSession,
    account: AccountId,
    code: StockCode,
    side: Side,
    target_qty: u32,
    active: Option<(OrderId, u32)>,
    filled_qty: u32,
) {
    install_parent_with_id(
        session,
        account,
        code,
        side,
        target_qty,
        active,
        filled_qty,
        PlanId(700),
    );
}

fn install_parent_with_id(
    session: &mut GameSession,
    account: AccountId,
    code: StockCode,
    side: Side,
    target_qty: u32,
    active: Option<(OrderId, u32)>,
    filled_qty: u32,
    linked_plan_id: PlanId,
) {
    let plan = ParentOrderPlan {
        code: code.clone(),
        side,
        target_qty,
        filled_qty,
        child_qty: target_qty,
        active_child_order_id: active.map(|value| value.0),
        active_child_remaining_qty: active.map(|value| value.1),
        linked_plan_id: Some(linked_plan_id),
        limit_price: Money::from_cents(1_100),
        expires_market_minute: 240,
    };
    assert!(
        session
            .parent_orders
            .entry(account)
            .or_default()
            .insert(code, plan)
            .is_none()
    );
}

fn auction_order(
    owner: u64,
    id: u64,
    side: Side,
    limit_cents: i64,
    qty: u32,
) -> crate::AuctionOrderSnap {
    crate::AuctionOrderSnap {
        owner: AccountId(owner),
        side,
        limit: Money::from_cents(limit_cents),
        qty,
        arrival_seq: id,
    }
}

fn order(id: u64, owner: u64, side: Side, price_cents: i64, qty: u32) -> Order {
    Order {
        id: OrderId(id),
        side,
        price: Money::from_cents(price_cents),
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: AccountId(owner),
        seq: id,
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

fn audit(tail: u8, completion: u8, day_end: u8) -> B2FinalizerAudit {
    B2FinalizerAudit {
        auction_tail_passes: tail,
        auction_completion_passes: completion,
        day_end_passes: day_end,
    }
}

fn hashes(session: &GameSession) -> (crate::session::StateHash, crate::session::StateHash) {
    (
        session.business_state_hash().unwrap(),
        session.session_state_hash().unwrap(),
    )
}

fn count_events(events: &[Event], predicate: fn(&Event) -> bool) -> usize {
    events.iter().filter(|event| predicate(event)).count()
}

fn is_auction_tick(event: &Event) -> bool {
    matches!(event, Event::AuctionTick { .. })
}

fn is_auction_completed(event: &Event) -> bool {
    matches!(event, Event::AuctionCompleted { .. })
}

fn is_trade(event: &Event) -> bool {
    matches!(event, Event::Trade { .. })
}

fn is_day_boundary(event: &Event) -> bool {
    matches!(event, Event::DayBoundary { .. })
}

fn is_order_canceled(event: &Event) -> bool {
    matches!(event, Event::OrderCanceled { .. })
}
