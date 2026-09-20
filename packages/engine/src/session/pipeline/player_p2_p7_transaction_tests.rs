use super::player_p2_p7_transaction::{
    apply_tick_shadow_player_p2_p7_transaction, PlayerP2P7TransactionError,
};
use super::*;
use crate::{AccountId, Event, Intent, Money, OrderId, RejectionReason, Side, StockCode};

fn player_plan(game: &GameSession) -> TickShadowPlan {
    plan_tick(PhaseInput { session: game }).unwrap()
}

#[test]
fn real_player_candidate_reaches_rebased_single_point_p9_commit() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )
    .unwrap();
    let guard = super::p9_candidate_commit::P8AuthorityGuard::capture(&game).unwrap();
    let mut plan = player_plan(&game);
    apply_tick_shadow_player_p2_p7_transaction(&mut plan).unwrap();

    let prepared =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut game, plan, guard)
            .unwrap();
    let committed = prepared.commit();

    assert!(game.pending_player.is_empty());
    assert_eq!(game.envelope_ledger.iter().count(), 1);
    assert_eq!(
        game.envelope_ledger.iter().next().unwrap().1.origin(),
        EnvelopeOrigin::TickStart
    );
    assert_eq!(game.markets[&code].resting_orders().len(), 1);
    assert!(matches!(
        committed.tick.events.as_slice(),
        [Event::OrderAccepted { seq: 1, code: accepted, .. }] if accepted == &code
    ));
    assert_eq!(
        game.business_state_hash().unwrap(),
        committed.receipt.business_hash()
    );
    assert_eq!(committed.tick.trace.last(), Some(&TickPhase::CommitTick));
}

#[test]
fn real_player_queue_runs_through_p7_inside_one_uncommitted_tick_shadow() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    game.setup.t1_enabled = false;
    let code = game.markets.keys().next().unwrap().clone();
    let untouched_code = game
        .markets
        .keys()
        .find(|candidate| *candidate != &code)
        .unwrap()
        .clone();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )
    .unwrap();
    let authority_before = game.business_state_hash().unwrap();
    let untouched_before =
        serde_json::to_vec(&game.markets[&untouched_code].hash_projection()).unwrap();
    let next_order_id_before = game.next_order_id;
    let mut plan = player_plan(&game);

    let output = apply_tick_shadow_player_p2_p7_transaction(&mut plan).unwrap();

    assert_eq!(game.business_state_hash().unwrap(), authority_before);
    assert_eq!(game.pending_player.len(), 1);
    plan.state
        .execute(|candidate| {
            assert!(candidate.pending_player.is_empty());
            assert!(!candidate.setup.t1_enabled);
            assert_eq!(candidate.next_order_id, next_order_id_before + 1);
            assert_eq!(
                serde_json::to_vec(&candidate.markets[&untouched_code].hash_projection()).unwrap(),
                untouched_before
            );
            let (key, _) = candidate.envelope_ledger.iter().next().unwrap();
            assert_eq!(
                (key.account, &key.stock, key.order, key.side),
                (
                    AccountId(0),
                    &code,
                    OrderId(next_order_id_before),
                    Side::Buy
                )
            );
            assert_eq!(candidate.markets[&code].resting_orders().len(), 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        output.validation.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert!(output.receipts.is_empty());
    assert_eq!(output.events.len(), 1);
    assert!(matches!(
        &output.events[0],
        Event::OrderAccepted {
            seq: 1,
            account: AccountId(0),
            code: accepted,
            id,
            side: Side::Buy,
            price,
            remaining_qty: 100,
        } if accepted == &code
            && *id == OrderId(next_order_id_before)
            && *price == Money::from_cents(1_000)
    ));
}

#[test]
fn stale_p1_resources_are_rejected_before_player_queue_capture() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )
    .unwrap();
    let mut plan = player_plan(&game);
    plan.state
        .execute(|candidate| {
            candidate.accounts.get_mut(&AccountId(0)).unwrap().cash = Money::ZERO;
            Ok(())
        })
        .unwrap();
    let before = plan
        .state
        .execute(|candidate| candidate.business_state_hash())
        .unwrap();

    let result = apply_tick_shadow_player_p2_p7_transaction(&mut plan);

    assert!(matches!(
        result,
        Err(PlayerP2P7TransactionError::Preparation(
            StepFatal::InvariantViolation { description, location }
        )) if location == "pipeline::decision_resources::validate_source_session"
            && description == "P1 decision resource snapshot does not match its post-P0 session"
    ));
    plan.state
        .execute(|candidate| {
            assert_eq!(candidate.business_state_hash().unwrap(), before);
            assert_eq!(candidate.pending_player.len(), 1);
            assert_eq!(candidate.envelope_ledger.iter().count(), 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn p3_rejections_and_unknown_stock_cancel_join_p4_facts_in_one_p7_sequence() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    let next_order_id_before = game.next_order_id;
    for intent in [
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 1,
        },
        Intent::Cancel {
            code: StockCode("999999".to_owned()),
            id: OrderId(123),
        },
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    ] {
        game.enqueue_player_intent(AccountId(0), intent).unwrap();
    }
    let mut plan = player_plan(&game);

    let output = apply_tick_shadow_player_p2_p7_transaction(&mut plan).unwrap();

    assert!(matches!(
        output.events.as_slice(),
        [
            Event::IntentRejected { seq: 1, code: invalid, reason: RejectionReason::InvalidQuantity, .. },
            Event::IntentRejected { seq: 2, code: unknown, reason: RejectionReason::UnknownStock, .. },
            Event::OrderAccepted { seq: 3, id, remaining_qty: 100, .. },
        ] if invalid == &code
            && unknown == &StockCode("999999".to_owned())
            && *id == OrderId(next_order_id_before)
    ));
    plan.state
        .execute(|candidate| {
            assert!(candidate.pending_player.is_empty());
            assert_eq!(candidate.next_order_id, next_order_id_before + 1);
            assert_eq!(candidate.envelope_ledger.iter().count(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn downstream_failure_preserves_the_tick_shadow_player_queue_and_candidate_authority() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )
    .unwrap();
    let mut plan = player_plan(&game);
    plan.state
        .execute(|candidate| {
            candidate.next_receipt_base = 1;
            Ok(())
        })
        .unwrap();
    let before = plan
        .state
        .execute(|candidate| candidate.business_state_hash())
        .unwrap();

    let result = apply_tick_shadow_player_p2_p7_transaction(&mut plan);

    assert!(matches!(
        result,
        Err(PlayerP2P7TransactionError::P3P7(
            super::p3_p7_session_transaction::P3P7SessionTransactionError::Transaction(
                super::p4_p7_session_transaction::P4P7SessionTransactionError::Precondition(
                    StepFatal::InvariantViolation { description, location }
                )
            )
        )) if location == "pipeline::p4_p7_session_transaction"
            && description == "session receipt cursor 1 does not match envelope ledger cursor 0"
    ));
    plan.state
        .execute(|candidate| {
            assert_eq!(candidate.business_state_hash().unwrap(), before);
            assert_eq!(candidate.pending_player.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn adapter_failure_after_private_player_capture_preserves_the_tick_shadow() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.auction_ticks = 1;
    let mut game = GameSession::new(setup, 42).unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )
    .unwrap();
    let mut plan = player_plan(&game);
    let before = plan
        .state
        .execute(|candidate| candidate.business_state_hash())
        .unwrap();

    let result = apply_tick_shadow_player_p2_p7_transaction(&mut plan);

    assert!(matches!(
        result,
        Err(PlayerP2P7TransactionError::P3P7(
            super::p3_p7_session_transaction::P3P7SessionTransactionError::Adapter(
                StepFatal::InvariantViolation { location, .. }
            )
        )) if location == "pipeline::p4_continuous_adapter"
    ));
    plan.state
        .execute(|candidate| {
            assert_eq!(candidate.business_state_hash().unwrap(), before);
            assert_eq!(candidate.pending_player.len(), 1);
            Ok(())
        })
        .unwrap();
}
