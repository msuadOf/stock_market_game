use super::*;
use crate::session::pipeline::b1_continuous_transaction::prepare_b1_continuous_tick;
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountKind, Money};

fn npc_session() -> (GameSession, AccountId, StockCode) {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        8,
    )
    .unwrap();
    let account = AccountId(1);
    let code = session.setup.stocks[0].code.clone();
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5, 1).unwrap()));
    let tick = session.tick;
    crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, account, tick);
    (session, account, code)
}

fn restore_attention_profile(session: &mut GameSession, npc: AccountId) {
    // The forced-observation fixture must restore the seed-derived probability before save.
    let baseline = GameSession::new(session.setup.clone(), session.seed).unwrap();
    let probability = baseline.npc_attention[&npc].base_probability;
    session
        .npc_attention
        .get_mut(&npc)
        .unwrap()
        .base_probability = probability;
    let mut state = session.accounts[&npc]
        .strategy
        .as_ref()
        .unwrap()
        .production_state()
        .unwrap();
    state.set_base_observation_probability(probability);
    session
        .accounts
        .get_mut(&npc)
        .unwrap()
        .set_strategy(state.into_strategy().unwrap());
}

#[test]
fn npc_b1_resting_quote_registers_original_expiry_and_restores_without_resampling() {
    let (mut session, npc, code) = npc_session();
    session.pending_npc = None;
    crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
    prepare_b1_continuous_tick(&mut session).unwrap().commit();
    let resting = session.markets[&code].resting_orders_for(npc);
    assert_eq!(resting.len(), 1);
    assert_eq!(session.npc_order_lifecycles.len(), 1);
    let lifecycle = &session.npc_order_lifecycles[0];
    assert_eq!(lifecycle.order_id, resting[0].id);
    assert_eq!(
        lifecycle.expires_market_minute - lifecycle.placed_market_minute,
        session.npc_quote_lifetime_minutes(npc, &code, &resting[0])
    );
    restore_attention_profile(&mut session, npc);
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(restored.npc_order_lifecycles, session.npc_order_lifecycles);
    assert_eq!(
        serde_json::to_vec(&restored.save().unwrap()).unwrap(),
        serde_json::to_vec(&saved).unwrap()
    );
}

#[test]
fn npc_b1_passive_full_fill_removes_existing_quote_lifecycle_before_save() {
    let (mut session, npc, code) = npc_session();
    session
        .accounts
        .get_mut(&npc)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
        },
        &mut Vec::new(),
    );
    assert_eq!(session.npc_order_lifecycles.len(), 1);
    let old_id = session.npc_order_lifecycles[0].order_id;
    restore_attention_profile(&mut session, npc);
    session.attention_queue.clear();
    session
        .npc_attention
        .get_mut(&npc)
        .unwrap()
        .next_attention_candidate_tick = 100;
    session.attention_queue.push(std::cmp::Reverse((100, npc)));
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    let result = prepare_b1_continuous_tick(&mut session).unwrap().commit();
    assert!(result
        .output
        .receipts
        .iter()
        .any(|receipt| receipt.envelope.order == old_id
            && receipt.kind == ReceiptKind::Fill
            && receipt.qty_after == 0));
    assert!(session.npc_order_lifecycles.is_empty());
    assert!(session.markets[&code].resting_orders_for(npc).is_empty());
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert!(restored.npc_order_lifecycles.is_empty());
}

#[test]
fn npc_parent_child_uses_parent_horizon_instead_of_quote_lifecycle() {
    let (mut session, request) = fixture();
    let account = AccountId(1);
    session.accounts.get_mut(&account).unwrap().kind = AccountKind::Inst;
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    while execute(&mut session, &mut chain, &mut p3, &mut p4).is_some() {}
    assert!(session.parent_orders[&account][&request.allocation.code]
        .active_child_order_id
        .is_some());
    assert!(session.npc_order_lifecycles.is_empty());
}

fn project_npc_limit_against_player_ask(qty: u32) -> (GameSession, OrderId) {
    let (mut session, npc, code) = npc_session();
    let player = AccountId(0);
    session
        .accounts
        .get_mut(&player)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    session.seed_order_for_test(
        player,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let result = p3
        .consume(P2Candidate::new(
            P2CandidateKey::npc(npc, 0),
            npc,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty,
            },
        ))
        .unwrap();
    let operation = result.operation().unwrap().clone();
    let mut round = p4.apply_round(vec![operation]).unwrap();
    let order_id = round.facts[0].allocated_order_id.unwrap();
    chain
        .project_execution_round(&mut session, &mut round)
        .unwrap();
    (session, order_id)
}

#[test]
fn npc_partial_resting_fill_registers_once_and_immediate_full_fill_does_not_register() {
    let (partial, order_id) = project_npc_limit_against_player_ask(200);
    assert_eq!(partial.npc_order_lifecycles.len(), 1);
    assert_eq!(partial.npc_order_lifecycles[0].order_id, order_id);
    let code = &partial.setup.stocks[0].code;
    assert_eq!(partial.markets[code].resting_orders()[0].qty, 100);
    let (filled, _) = project_npc_limit_against_player_ask(100);
    assert!(filled.npc_order_lifecycles.is_empty());
    assert!(filled.markets[code].resting_orders().is_empty());
}

#[test]
fn npc_quote_expiry_uses_acceptance_quote_when_a_later_round_operation_moves_the_spread() {
    let (mut session, npc, code) = npc_session();
    let player = AccountId(0);
    session
        .accounts
        .get_mut(&player)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let mut legacy = session.clone_for_tick_shadow().unwrap();
    let buy = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(1_000),
        qty: 100,
    };
    legacy.seed_order_for_test(npc, buy.clone(), &mut Vec::new());
    let expected = legacy.npc_order_lifecycles.clone();
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let operations = [
        P2Candidate::new(P2CandidateKey::npc(npc, 0), npc, buy),
        P2Candidate::new(
            P2CandidateKey::player(0),
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1_008),
                qty: 100,
            },
        ),
    ]
    .into_iter()
    .map(|candidate| p3.consume(candidate).unwrap().operation().unwrap().clone())
    .collect();
    let mut round = p4.apply_round(operations).unwrap();
    chain
        .project_execution_round(&mut session, &mut round)
        .unwrap();
    assert_eq!(session.npc_order_lifecycles, expected);
    assert_eq!(
        session.markets[&code].best_ask(),
        Some(Money::from_cents(1_008))
    );
    let resting = &session.markets[&code].resting_orders_for(npc)[0];
    assert_ne!(
        expected[0].expires_market_minute - expected[0].placed_market_minute,
        session.npc_quote_lifetime_minutes(npc, &code, resting)
    );
}

#[test]
fn npc_new_quote_filled_later_in_the_same_round_has_no_stale_lifecycle() {
    let (mut session, npc, code) = npc_session();
    let player = AccountId(0);
    session
        .accounts
        .get_mut(&player)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let operations = [
        P2Candidate::new(
            P2CandidateKey::npc(npc, 0),
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        ),
    ]
    .into_iter()
    .map(|candidate| p3.consume(candidate).unwrap().operation().unwrap().clone())
    .collect();
    let mut round = p4.apply_round(operations).unwrap();
    assert!(matches!(
        round.facts[0].outcome,
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Resting { .. },
            ..
        }
    ));
    assert!(round.projections[&code]
        .market
        .as_ref()
        .unwrap()
        .resting_orders()
        .is_empty());
    chain
        .project_execution_round(&mut session, &mut round)
        .unwrap();
    assert!(session.npc_order_lifecycles.is_empty());
}

#[test]
fn npc_resting_fact_without_its_acceptance_quote_is_a_typed_failure() {
    let (mut session, npc, code) = npc_session();
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let step = p3
        .consume(P2Candidate::new(
            P2CandidateKey::npc(npc, 0),
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        ))
        .unwrap();
    let mut round = p4
        .apply_round(vec![step.operation().unwrap().clone()])
        .unwrap();
    round
        .projections
        .get_mut(&code)
        .unwrap()
        .acceptance_quotes
        .clear();
    let error = chain
        .project_execution_round(&mut session, &mut round)
        .unwrap_err();
    assert!(
        matches!(error, StepFatal::InvariantViolation { description, .. }
        if description.contains("no acceptance quote"))
    );
    assert!(chain.consumed.operations.is_empty());
    assert!(session.npc_order_lifecycles.is_empty());
}
