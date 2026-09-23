use super::player_p2_p7_transaction::{
    apply_session_player_p2_p7_transaction, PlayerP2P7TransactionError,
};
use super::*;
use crate::{AccountId, Event, Intent, Money, OrderId, Side};

fn decision_resources(game: &GameSession) -> DecisionResourceSnapshot {
    plan_tick(PhaseInput { session: game })
        .unwrap()
        .decision_resources()
        .unwrap()
        .clone()
}

#[test]
fn real_player_queue_runs_through_p7_on_one_atomic_session_candidate() {
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
    let untouched_before =
        serde_json::to_vec(&game.markets[&untouched_code].hash_projection()).unwrap();
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
    let resources = decision_resources(&game);
    let next_order_id_before = game.next_order_id;

    let output = apply_session_player_p2_p7_transaction(&mut game, resources).unwrap();

    assert!(game.pending_player.is_empty());
    assert!(!game.setup.t1_enabled);
    assert_eq!(game.next_order_id, next_order_id_before + 1);
    assert_eq!(game.markets.len(), 2);
    assert_eq!(
        serde_json::to_vec(&game.markets[&untouched_code].hash_projection()).unwrap(),
        untouched_before
    );
    assert_eq!(game.envelope_ledger.iter().count(), 1);
    let (envelope_key, _) = game.envelope_ledger.iter().next().unwrap();
    assert_eq!(envelope_key.account, AccountId(0));
    assert_eq!(envelope_key.stock, code);
    assert_eq!(envelope_key.order, OrderId(next_order_id_before));
    assert_eq!(envelope_key.side, Side::Buy);
    assert_eq!(game.markets[&code].resting_orders().len(), 1);
    assert_eq!(
        output.validation.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert!(output.receipts.is_empty());
    assert!(output.p6.events.is_empty());
    assert_eq!(output.p6.settlement.applied_groups, 0);
    assert_eq!(output.p6.settlement.applied_receipts, 0);
    assert_eq!(output.events.len(), 1);
    match &output.events[0] {
        Event::OrderAccepted {
            account,
            code: accepted,
            id,
            ..
        } => {
            assert_eq!(*account, AccountId(0));
            assert_eq!(accepted, &code);
            assert_eq!(*id, OrderId(next_order_id_before));
        }
        other => panic!("expected OrderAccepted, observed {other:?}"),
    }
}

#[test]
fn downstream_failure_preserves_the_real_player_queue_and_all_candidate_authority() {
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
    let resources = decision_resources(&game);
    game.next_receipt_base = 1;
    let before = game.business_state_hash().unwrap();
    let queued_before = serde_json::to_vec(&game.pending_player).unwrap();

    let result = apply_session_player_p2_p7_transaction(&mut game, resources);

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
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(
        serde_json::to_vec(&game.pending_player).unwrap(),
        queued_before
    );
}

#[test]
fn adapter_failure_after_private_player_capture_preserves_the_queue_and_authority() {
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
    let resources = decision_resources(&game);
    let before = game.business_state_hash().unwrap();
    let queued_before = serde_json::to_vec(&game.pending_player).unwrap();

    let result = apply_session_player_p2_p7_transaction(&mut game, resources);

    assert!(matches!(
        result,
        Err(PlayerP2P7TransactionError::P3P7(
            super::p3_p7_session_transaction::P3P7SessionTransactionError::Adapter(
                StepFatal::InvariantViolation { location, .. }
            )
        )) if location == "pipeline::p4_continuous_adapter"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(
        serde_json::to_vec(&game.pending_player).unwrap(),
        queued_before
    );
}
