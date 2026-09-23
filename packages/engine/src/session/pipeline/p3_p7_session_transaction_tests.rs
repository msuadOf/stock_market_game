use super::p3_context::build_p3_validation_context;
use super::p3_p7_session_transaction::{
    apply_session_p3_p7_transaction, collect_canonical_worker_results, P3P7SessionTransactionError,
};
use super::*;
use crate::{AccountId, Event, Intent, Money, Side};

fn validation(game: &GameSession, intents: Vec<Intent>) -> P3ValidationOutput {
    let plan = plan_tick(PhaseInput { session: game }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(ordinal, intent)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(ordinal).unwrap()),
                AccountId(0),
                intent,
            )
        })
        .collect::<Vec<_>>();
    P2P3Handoff::new_with_context(
        P2CandidateBatch::from_unsorted(candidates).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        build_p3_validation_context(game).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap()
}

#[test]
fn validated_player_buy_runs_parallel_stock_workers_through_p7_atomically() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    let untouched_code = game
        .markets
        .keys()
        .find(|candidate| *candidate != &code)
        .unwrap()
        .clone();
    let untouched_before =
        serde_json::to_vec(&game.markets[&untouched_code].hash_projection()).unwrap();
    let validation = validation(
        &game,
        vec![Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        }],
    );
    let seq_before = game.seq;
    let next_order_id_before = game.next_order_id;

    let output = apply_session_p3_p7_transaction(&mut game, &validation).unwrap();

    assert_eq!(game.seq, seq_before + 1);
    assert_eq!(game.next_order_id, next_order_id_before + 1);
    assert_eq!(game.markets.len(), 2);
    assert_eq!(
        serde_json::to_vec(&game.markets[&untouched_code].hash_projection()).unwrap(),
        untouched_before
    );
    assert_eq!(game.envelope_ledger.iter().count(), 1);
    assert_eq!(game.markets[&code].resting_orders().len(), 1);
    assert!(matches!(
        output.events.as_slice(),
        [Event::OrderAccepted { code: accepted, .. }] if accepted == &code
    ));
}

#[test]
fn adapter_failure_before_workers_leaves_the_session_candidate_unchanged() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.auction_ticks = 1;
    let mut game = GameSession::new(setup, 42).unwrap();
    let validation = validation(&game, Vec::new());
    let before = game.business_state_hash().unwrap();
    let next_order_id_before = game.next_order_id;

    let result = apply_session_p3_p7_transaction(&mut game, &validation);

    assert!(matches!(
        result,
        Err(P3P7SessionTransactionError::Adapter(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::p4_continuous_adapter"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.next_order_id, next_order_id_before);
}

#[test]
fn transaction_failure_after_parallel_workers_leaves_the_session_candidate_unchanged() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    let validation = validation(
        &game,
        vec![Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        }],
    );
    let next_order_id_before = game.next_order_id;
    game.next_receipt_base = 1;
    let before = game.business_state_hash().unwrap();

    let result = apply_session_p3_p7_transaction(&mut game, &validation);

    assert!(matches!(
        result,
        Err(P3P7SessionTransactionError::Transaction(
            super::p4_p7_session_transaction::P4P7SessionTransactionError::Precondition(
                StepFatal::InvariantViolation { location, .. }
            )
        )) if location == "pipeline::p4_p7_session_transaction"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.next_order_id, next_order_id_before);
}

#[test]
fn stale_p3_order_cursor_is_rejected_before_workers_without_session_mutation() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = game.markets.keys().next().unwrap().clone();
    let validation = validation(
        &game,
        vec![Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        }],
    );
    game.next_order_id += 1;
    let before = game.business_state_hash().unwrap();

    let result = apply_session_p3_p7_transaction(&mut game, &validation);

    assert!(matches!(
        result,
        Err(P3P7SessionTransactionError::Precondition(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::p3_p7_session_transaction"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn multiple_worker_failures_choose_the_first_canonical_stock_error() {
    let first = crate::StockCode("000001".to_owned());
    let second = crate::StockCode("600888".to_owned());
    let first_error = StepFatal::InvariantViolation {
        description: "first canonical worker failed".to_owned(),
        location: "worker.first".to_owned(),
    };
    let second_error = StepFatal::InvariantViolation {
        description: "second canonical worker failed".to_owned(),
        location: "worker.second".to_owned(),
    };

    let result = collect_canonical_worker_results(vec![
        (first.clone(), Err(first_error.clone())),
        (second, Err(second_error)),
    ]);

    assert!(matches!(
        result,
        Err(P3P7SessionTransactionError::Worker { code, source })
            if code == first && source == first_error
    ));
}
