use super::p4_continuous::{process_continuous_stock, ContinuousStockInput};
use super::p4_p7_session_transaction::{
    apply_session_p4_p7_transaction, P4P7SessionTransactionError,
};
use super::*;
use crate::{
    AccountId, Event, Intent, Market, Money, SecurityCategory, Side, StockCode, TradingPhase,
};

fn accepted_buy_worker(game: &GameSession) -> super::p4_continuous::ContinuousStockOutput {
    let code = game.markets.keys().next().unwrap().clone();
    let plan = plan_tick(PhaseInput { session: game }).unwrap();
    let batch = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )])
    .unwrap();
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            SecurityCategory::MainBoard,
            Money::from_cents(1_100),
            Money::from_cents(900),
        ),
    )])
    .unwrap();
    let validation = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();

    process_continuous_stock(ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market: game.markets[&code].clone(),
        envelopes: Vec::new(),
        operations: validation.operations().to_vec(),
        config: game.setup.config.clone(),
    })
    .unwrap()
}

#[test]
fn successful_candidate_installs_p4_through_p7_state_and_cursors_together() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let worker = accepted_buy_worker(&game);
    let code = worker.market.code().clone();
    let seq_before = game.seq;

    let output = apply_session_p4_p7_transaction(&mut game, vec![worker], Vec::new()).unwrap();

    assert_eq!(game.seq, seq_before + 1);
    assert_eq!(
        game.next_receipt_base,
        game.envelope_ledger.next_receipt_index()
    );
    assert_eq!(game.envelope_ledger.iter().count(), 1);
    assert_eq!(game.markets[&code].resting_orders().len(), 1);
    assert!(output.receipts.is_empty());
    assert_eq!(output.p6.settlement.applied_receipts, 0);
    assert_eq!(output.events.len(), 1);
    match output.events[0].clone() {
        Event::OrderAccepted {
            seq,
            code: event_code,
            ..
        } => {
            assert_eq!(seq, game.seq);
            assert_eq!(event_code, code);
        }
        event => panic!("expected OrderAccepted, got {event:?}"),
    }
}

#[test]
fn p7_sequence_failure_leaves_the_whole_session_candidate_unchanged() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let worker = accepted_buy_worker(&game);
    game.seq = u64::MAX;
    let before = game.business_state_hash().unwrap();

    let result = apply_session_p4_p7_transaction(&mut game, vec![worker], Vec::new());

    assert!(matches!(
        result,
        Err(P4P7SessionTransactionError::P7(StepFatal::InvariantViolation {
            location,
            ..
        })) if location == "pipeline::p7_events::collect_events"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn split_receipt_cursor_is_rejected_before_any_candidate_work() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let worker = accepted_buy_worker(&game);
    game.next_receipt_base = 1;
    let before = game.business_state_hash().unwrap();

    let result = apply_session_p4_p7_transaction(&mut game, vec![worker], Vec::new());

    assert!(matches!(
        result,
        Err(P4P7SessionTransactionError::Precondition(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::p4_p7_session_transaction"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn unknown_worker_stock_is_rejected_without_extending_session_markets() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600999".to_owned());
    let worker = super::p4_continuous::ContinuousStockOutput {
        market: Market::new(code, Money::from_cents(1_000), 0.10, Money::from_cents(1)).unwrap(),
        created_envelopes: Vec::new(),
        receipts: Vec::new(),
        terminal_keys: Vec::new(),
        trades: Vec::new(),
        place_facts: Vec::new(),
        cancel_facts: Vec::new(),
    };
    let before = game.business_state_hash().unwrap();
    let market_count = game.markets.len();

    let result = apply_session_p4_p7_transaction(&mut game, vec![worker], Vec::new());

    assert!(matches!(
        result,
        Err(P4P7SessionTransactionError::Precondition(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::p4_p7_session_transaction"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.markets.len(), market_count);
}
