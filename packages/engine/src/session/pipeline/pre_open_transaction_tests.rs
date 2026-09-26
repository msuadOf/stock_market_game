use super::pre_open_transaction::{
    apply_tick_shadow_pre_open_transaction_with_roots_for_test, prepare_pre_open_tick,
    PreOpenTransactionError,
};
use super::*;
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::strategy::ZiNoiseStrategy;
use crate::{
    AccountId, Event, GameSession, Intent, Money, OrderId, RejectionReason, Side, TradingPhase,
};

fn player_only_pre_open_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.inst_count = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    complete_opening_auction(&mut session);
    session
}

fn complete_opening_auction(session: &mut GameSession) {
    session.tick = 599;
    session.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(session).unwrap();
    assert_eq!(session.phase(), TradingPhase::CallAuction);
    super::b2_auction_transaction::prepare_b2_auction_tick(session)
        .unwrap()
        .commit();
    assert_eq!(session.tick(), 600);
    assert_eq!(session.phase(), TradingPhase::PreOpen);
    assert!(session.auction_orders.values().all(Vec::is_empty));
}

fn due_retail_pre_open_session() -> (GameSession, AccountId) {
    let account = AccountId(1);
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let mut session = GameSession::new(setup, 8).unwrap();
    complete_opening_auction(&mut session);
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5, 1).unwrap()));
    crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, account, 600);
    session.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(&mut session).unwrap();
    assert_eq!(session.phase(), TradingPhase::PreOpen);
    (session, account)
}

#[test]
fn empty_pre_open_tick_commits_silently_and_is_immediately_saveable() {
    let mut authority = player_only_pre_open_session();
    let tick_before = authority.tick();
    let seq_before = authority.seq();
    let histories_before = serde_json::to_value((
        &authority.price_history,
        &authority.market_minute_closes,
        &authority.daily_candles,
        &authority.active_daily_candles,
    ))
    .unwrap();

    let committed = prepare_pre_open_tick(&mut authority).unwrap().commit();

    assert_eq!(authority.tick(), tick_before + 1);
    assert_eq!(authority.phase(), TradingPhase::PreOpen);
    assert_eq!(authority.seq(), seq_before);
    assert!(committed.commit.tick.events.is_empty());
    assert!(committed.output.candidates.candidates().is_empty());
    assert!(committed.output.receipts.is_empty());
    assert_eq!(committed.output.p6.settlement.applied_receipts, 0);
    assert_eq!(
        serde_json::to_value((
            &authority.price_history,
            &authority.market_minute_closes,
            &authority.daily_candles,
            &authority.active_daily_candles,
        ))
        .unwrap(),
        histories_before,
        "09:25-09:30 remains a market-data-silent window",
    );

    let save = authority.save().expect("P9 is a legal save quiet point");
    let restored = GameSession::restore(&save).expect("committed PreOpen save must restore");
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(save).unwrap(),
    );
}

#[test]
fn player_place_and_cancel_are_rejected_at_the_stock_boundary_with_one_reject_receipt() {
    let mut authority = player_only_pre_open_session();
    let player = AccountId(0);
    let code = authority.setup.stocks[0].code.clone();
    let next_order_before = authority.next_order_id;
    let seq_before = authority.seq();
    authority
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    authority
        .enqueue_player_intent(
            player,
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(77),
            },
        )
        .unwrap();

    let committed = prepare_pre_open_tick(&mut authority).unwrap().commit();

    assert_eq!(authority.tick(), 601);
    assert_eq!(authority.next_order_id, next_order_before + 1);
    assert!(authority.pending_player.is_empty());
    assert!(authority.markets[&code].resting_orders().is_empty());
    assert_eq!(committed.output.receipts.len(), 1);
    assert_eq!(committed.output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(committed.output.p6.settlement.applied_receipts, 0);
    assert!(matches!(
        committed.commit.tick.events.as_slice(),
        [
            Event::IntentRejected {
                seq: first_seq,
                account,
                code: placed,
                reason: RejectionReason::AuctionOrderEntryClosed,
            },
            Event::IntentRejected {
                seq: second_seq,
                account: canceled_account,
                code: canceled,
                reason: RejectionReason::AuctionOrderNotCancelable,
            },
        ] if *first_seq == seq_before + 1
            && *second_seq == seq_before + 2
            && *account == player
            && *canceled_account == player
            && *placed == code
            && *canceled == code
    ));
}

#[test]
fn unknown_stock_cancel_still_receives_the_pre_open_window_rejection() {
    let mut authority = player_only_pre_open_session();
    let player = AccountId(0);
    let unknown = crate::StockCode("999999".to_owned());
    let next_order_before = authority.next_order_id;
    authority
        .enqueue_player_intent(
            player,
            Intent::Cancel {
                code: unknown.clone(),
                id: OrderId(77),
            },
        )
        .unwrap();

    let committed = prepare_pre_open_tick(&mut authority).unwrap().commit();

    assert_eq!(authority.next_order_id, next_order_before);
    assert!(committed.output.receipts.is_empty());
    assert!(matches!(
        committed.commit.tick.events.as_slice(),
        [Event::IntentRejected {
            account,
            code,
            reason: RejectionReason::AuctionOrderNotCancelable,
            ..
        }] if *account == player && *code == unknown
    ));
}

#[test]
fn final_pre_open_tick_enters_continuous_without_publishing_market_data() {
    let mut authority = player_only_pre_open_session();
    authority.tick = 899;
    authority.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(&mut authority).unwrap();
    let history_before = serde_json::to_value((
        &authority.price_history,
        &authority.market_minute_closes,
        &authority.daily_candles,
        &authority.active_daily_candles,
    ))
    .unwrap();

    let events = prepare_pre_open_tick(&mut authority)
        .unwrap()
        .commit()
        .into_events();

    assert_eq!(authority.tick(), 900);
    assert_eq!(authority.phase(), TradingPhase::Continuous);
    assert!(events.is_empty());
    assert_eq!(
        serde_json::to_value((
            &authority.price_history,
            &authority.market_minute_closes,
            &authority.daily_candles,
            &authority.active_daily_candles,
        ))
        .unwrap(),
        history_before,
    );
}

#[test]
fn opening_rollover_order_and_reservation_survive_a_silent_pre_open_tick_and_restore() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.inst_count = 0;
    let mut authority = GameSession::new(setup, 43).unwrap();
    authority.tick = 599;
    authority.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(&mut authority).unwrap();
    let player = AccountId(0);
    let code = authority.setup.stocks[0].code.clone();
    authority
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        )
        .unwrap();

    super::b2_auction_transaction::prepare_b2_auction_tick(&mut authority)
        .unwrap()
        .commit();
    assert_eq!(authority.phase(), TradingPhase::PreOpen);
    assert!(authority.auction_orders.values().all(Vec::is_empty));
    assert_eq!(authority.markets[&code].resting_order_count(), 1);

    let orders_before = serde_json::to_value(authority.markets[&code].resting_orders()).unwrap();
    let ledger_before = serde_json::to_value(authority.envelope_ledger.hash_projection()).unwrap();
    let reserved_before = authority.reserved_cash_for_account(player).unwrap();
    let seq_before = authority.seq();
    let next_order_before = authority.next_order_id;

    let committed = prepare_pre_open_tick(&mut authority).unwrap().commit();

    assert!(committed.commit.tick.events.is_empty());
    assert_eq!(authority.seq(), seq_before);
    assert_eq!(authority.next_order_id, next_order_before);
    assert_eq!(
        serde_json::to_value(authority.markets[&code].resting_orders()).unwrap(),
        orders_before,
    );
    assert_eq!(
        serde_json::to_value(authority.envelope_ledger.hash_projection()).unwrap(),
        ledger_before,
    );
    assert_eq!(
        authority.reserved_cash_for_account(player).unwrap(),
        reserved_before,
    );
    assert!(reserved_before > Money::ZERO);

    let save = authority.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(save).unwrap(),
    );
}

#[test]
fn real_npc_and_player_candidates_share_the_pre_open_shadow_and_commit_strategy_state() {
    let (mut authority, npc) = due_retail_pre_open_session();
    let player = AccountId(0);
    let code = authority.setup.stocks[0].code.clone();
    authority
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    let strategy_before = authority.accounts[&npc]
        .strategy
        .as_ref()
        .unwrap()
        .production_state()
        .unwrap();
    let attention_before = authority.npc_attention[&npc].next_attention_candidate_tick;
    let rng_before = authority.rng.state;

    let committed = prepare_pre_open_tick(&mut authority).unwrap().commit();

    let keys = committed
        .output
        .candidates
        .candidates()
        .iter()
        .map(|candidate| candidate.key().clone())
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 2);
    assert!(keys
        .iter()
        .any(|key| matches!(key, P2CandidateKey::Npc { account, .. } if *account == npc)));
    assert!(keys.contains(&P2CandidateKey::player(0)));
    assert!(committed.commit.tick.events.iter().all(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::AuctionOrderEntryClosed,
            ..
        }
    )));
    assert_ne!(
        authority.npc_attention[&npc].next_attention_candidate_tick, attention_before,
        "the accepted NPC attention schedule must advance on the committed shadow",
    );
    assert_eq!(
        authority.rng.state, rng_before,
        "NPC decisions use their derived RNG stream"
    );
    assert_eq!(
        authority.accounts[&npc]
            .strategy
            .as_ref()
            .unwrap()
            .production_state()
            .unwrap(),
        strategy_before,
        "this deterministic ZI decision does not invent a second strategy authority",
    );
}

#[test]
fn plan_chain_candidate_receives_typed_entry_closed_outcome_without_installing_a_child() {
    let (mut authority, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    authority.setup.auction_ticks = 900;
    authority.setup.ticks_per_day = 15_300;
    authority.tick = 600;
    authority.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(&mut authority).unwrap();
    let plan_id = request.plan_id;
    let next_order_before = authority.next_order_id;
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output =
        apply_tick_shadow_pre_open_transaction_with_roots_for_test(&mut plan, roots).unwrap();
    let committed =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
            .unwrap()
            .commit();

    assert_eq!(output.plan_reports.len(), 1);
    assert!(matches!(
        output.plan_reports[0].disposition,
        crate::session::PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::AuctionOrderEntryClosed,
        }
    ));
    assert!(matches!(
        committed.tick.events.as_slice(),
        [Event::IntentRejected {
            account: AccountId(1),
            reason: RejectionReason::AuctionOrderEntryClosed,
            ..
        }]
    ));
    assert_eq!(authority.next_order_id, next_order_before + 1);
    assert_eq!(authority.tick(), 601);
    assert_eq!(
        authority.plans.plan(plan_id).unwrap().active_child_order_id,
        None,
    );
    assert!(authority.parent_orders.is_empty());
}

#[test]
fn late_pre_open_failure_discards_candidate_tick_rng_strategy_queue_receipts_and_events() {
    let (mut authority, npc) = due_retail_pre_open_session();
    let code = authority.setup.stocks[0].code.clone();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();
    let seq_before = authority.seq();
    let queue_before = serde_json::to_vec(&authority.pending_player).unwrap();
    let strategy_before = authority.accounts[&npc]
        .strategy
        .as_ref()
        .unwrap()
        .production_state()
        .unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    plan.state
        .execute(|candidate| {
            candidate.next_receipt_base = 1;
            Ok(())
        })
        .unwrap();

    let error = match super::pre_open_transaction::apply_tick_shadow_pre_open_transaction(&mut plan)
    {
        Ok(_) => panic!("malformed receipt cursor must fail the PreOpen transaction"),
        Err(error) => error.into_fatal(),
    };

    assert!(matches!(error, StepFatal::InvariantViolation { .. }));
    assert!(plan.state.execute(|_| Ok(())).is_err());
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
    assert_eq!(
        serde_json::to_vec(&authority.pending_player).unwrap(),
        queue_before
    );
    assert_eq!(authority.tick(), 600);
    assert_eq!(authority.seq(), seq_before);
    assert_eq!(authority.next_receipt_base, 0);
    assert_eq!(
        authority.accounts[&npc]
            .strategy
            .as_ref()
            .unwrap()
            .production_state()
            .unwrap(),
        strategy_before,
    );
}

#[test]
fn nested_fatal_keeps_its_original_location() {
    let fatal = StepFatal::InvariantViolation {
        description: "pre-open candidate failed".to_owned(),
        location: "pre_open_transaction_tests::nested".to_owned(),
    };
    let nested =
        super::p4_p7_session_transaction::P4P7SessionTransactionError::Precondition(fatal.clone());

    let observed = PreOpenTransactionError::from_source_for_test(nested).into_fatal();

    assert_eq!(observed, fatal);
}
