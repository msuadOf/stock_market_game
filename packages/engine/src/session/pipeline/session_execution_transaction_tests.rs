use super::continuous_matching::process_continuous_stock;
use super::session_execution_transaction::{
    apply_session_execution_transaction, SessionExecutionTransactionError,
};
use super::*;
use crate::{AccountId, Event, Intent, Market, Money, SecurityCategory, Side, StockCode};

fn accepted_buy_worker(
    game: &GameSession,
    buyer: AccountId,
) -> super::continuous_matching::ContinuousStockOutput {
    let code = game.state.markets.keys().next().unwrap().clone();
    let plan = plan_tick(PhaseInput { session: game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        buyer,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
    )])
    .unwrap();
    let context = AccountValidationContext::new([(
        code.clone(),
        StockValidation::new(
            SecurityCategory::MainBoard,
            Money::from_cents(1_100),
            Money::from_cents(900),
        ),
    )])
    .unwrap();
    let validation = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();

    let mut input = super::continuous_matching_adapter::prepare_incremental_continuous_inputs(game)
        .unwrap()
        .into_iter()
        .find(|input| input.market.code() == &code)
        .unwrap();
    input.operations = validation.operations().to_vec();
    process_continuous_stock(input).unwrap()
}

#[test]
fn successful_candidate_installs_matching_through_projection_state_and_cursors_together() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let worker = accepted_buy_worker(&game, AccountId(0));
    let code = worker.market.code().clone();
    let seq_before = game.state.seq;

    let output = apply_session_execution_transaction(&mut game, vec![worker], Vec::new()).unwrap();

    assert_eq!(game.state.seq, seq_before + 1);
    assert_eq!(
        game.state.next_receipt_base,
        game.state.envelope_ledger.next_receipt_index()
    );
    assert_eq!(game.state.envelope_ledger.iter().count(), 1);
    assert_eq!(game.state.markets[&code].resting_orders().len(), 1);
    assert!(output.receipts.is_empty());
    assert_eq!(output.settlement.settlement.applied_receipts, 0);
    assert_eq!(output.events.len(), 1);
    match output.events[0].clone() {
        Event::OrderAccepted {
            seq,
            code: event_code,
            ..
        } => {
            assert_eq!(seq, game.state.seq);
            assert_eq!(event_code, code);
        }
        event => panic!("expected OrderAccepted, got {event:?}"),
    }
}

#[test]
fn institutional_fill_updates_belief_and_preserves_participant_members_and_other_account() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    let buyer = AccountId(1);
    let other = AccountId(2);
    let code = game.state.markets.keys().next().unwrap().clone();
    let market_minute = game.current_market_minute();
    let observed_at = game.observation_civil_instant();
    let publication = game.state.library.publication_ids()[0];
    let participant = game.state.belief_participants.get_mut(&buyer).unwrap();
    participant
        .watchlist_mut()
        .record_attention(&code, market_minute, market_minute)
        .unwrap();
    participant
        .price_memory_mut()
        .observe_price(
            &code,
            Money::from_cents(1_011),
            market_minute,
            market_minute,
        )
        .unwrap();
    *participant.information_mut() = crate::information::NpcInformationState::new(buyer);
    participant
        .information_mut()
        .record_acquisition(buyer, &game.state.library, publication, observed_at)
        .unwrap();
    let buyer_before = participant.clone();
    let other_before = game.state.belief_participants[&other].clone();
    let other_account_before = game.state.accounts[&other].clone();
    game.state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(1_000))
        .unwrap();
    game.seed_order_for_test(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let buy_order_id = game.state.next_order_id;
    let worker = accepted_buy_worker(&game, buyer);

    let output = apply_session_execution_transaction(&mut game, vec![worker], Vec::new()).unwrap();

    assert_eq!(output.settlement.settlement.applied_receipts, 2);
    assert!(output.events.iter().any(|event| matches!(
        event,
        Event::Trade { maker: AccountId(0), taker, qty: 100, .. } if *taker == buyer
    )));
    let participant = &game.state.belief_participants[&buyer];
    let experience = participant.belief().experience();
    assert_ne!(experience, buyer_before.belief().experience());
    let traded = &experience.stocks[&code];
    assert_eq!(traded.last_buy_order_id, Some(buy_order_id));
    assert_eq!(traded.last_buy_price, Some(Money::from_cents(1_000)));
    assert_eq!(traded.last_trade_market_minute, market_minute);
    assert_eq!(
        game.state.accounts[&buyer].positions()[&code].t1_locked(),
        100
    );
    assert_eq!(participant.information(), buyer_before.information());
    assert_eq!(participant.watchlist(), buyer_before.watchlist());
    assert_eq!(participant.price_memory(), buyer_before.price_memory());
    let unchanged = &game.state.belief_participants[&other];
    assert_eq!(unchanged.belief(), other_before.belief());
    assert_eq!(unchanged.information(), other_before.information());
    assert_eq!(unchanged.watchlist(), other_before.watchlist());
    assert_eq!(unchanged.price_memory(), other_before.price_memory());
    let unchanged_account = &game.state.accounts[&other];
    assert_eq!(unchanged_account.kind(), other_account_before.kind());
    assert_eq!(unchanged_account.cash(), other_account_before.cash());
    let position_facts = |account: &crate::Account| {
        account
            .positions()
            .iter()
            .map(|(code, position)| {
                (
                    code.clone(),
                    (
                        position.qty(),
                        position.t1_locked(),
                        position.invested_cents(),
                        position.recovered_cents(),
                    ),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(
        position_facts(unchanged_account),
        position_facts(&other_account_before)
    );
    assert_eq!(
        unchanged_account
            .strategy()
            .unwrap()
            .production_state()
            .unwrap(),
        other_account_before
            .strategy()
            .unwrap()
            .production_state()
            .unwrap()
    );
}

#[test]
fn projection_sequence_failure_leaves_the_whole_session_candidate_unchanged() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let worker = accepted_buy_worker(&game, AccountId(0));
    game.state.seq = u64::MAX;
    let before = game.business_state_hash().unwrap();

    let result = apply_session_execution_transaction(&mut game, vec![worker], Vec::new());

    assert!(matches!(
        result,
        Err(SessionExecutionTransactionError::Projection(StepFatal::InvariantViolation {
            location,
            ..
        })) if location == "pipeline::event_collection::collect_events"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn split_receipt_cursor_is_rejected_before_any_candidate_work() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let worker = accepted_buy_worker(&game, AccountId(0));
    game.state.next_receipt_base = 1;
    let before = game.business_state_hash().unwrap();

    let result = apply_session_execution_transaction(&mut game, vec![worker], Vec::new());

    assert!(matches!(
        result,
        Err(SessionExecutionTransactionError::Precondition(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::session_execution_transaction"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn unknown_worker_stock_is_rejected_without_extending_session_markets() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600999".to_owned());
    let worker = super::continuous_matching::ContinuousStockOutput {
        market: Market::new(code, Money::from_cents(1_000), 0.10, Money::from_cents(1)).unwrap(),
        created_envelopes: Vec::new(),
        receipts: Vec::new(),
        terminal_keys: Vec::new(),
        trades: Vec::new(),
        place_facts: Vec::new(),
        cancel_facts: Vec::new(),
    };
    let before = game.business_state_hash().unwrap();
    let market_count = game.state.markets.len();

    let result = apply_session_execution_transaction(&mut game, vec![worker], Vec::new());

    assert!(matches!(
        result,
        Err(SessionExecutionTransactionError::Precondition(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::session_execution_transaction"
    ));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.state.markets.len(), market_count);
}
