use super::*;
use crate::session::pipeline::continuous_tick_transaction::prepare_continuous_tick;
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountKind, Money};

fn npc_session() -> (GameSession, AccountId, StockCode) {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        8,
    )
    .unwrap();
    let account = AccountId(1);
    let code = session.state.setup.stocks[0].code.clone();
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap()));
    let tick = session.state.tick;
    crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, account, tick);
    (session, account, code)
}

fn restore_attention_profile(session: &mut GameSession, npc: AccountId) {
    // 强制观察 fixture 保存前必须恢复由 seed 推导的 probability。
    let baseline = GameSession::new(session.state.setup.clone(), session.state.seed).unwrap();
    let probability = baseline.state.npc_attention[&npc].base_probability;
    session
        .state
        .npc_attention
        .get_mut(&npc)
        .unwrap()
        .base_probability = probability;
    let mut state = session.state.accounts[&npc]
        .strategy()
        .unwrap()
        .production_state()
        .unwrap();
    state.set_base_observation_probability(probability);
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .set_strategy(state.into_strategy().unwrap());
}

#[test]
fn npc_resting_quote_registers_original_expiry_and_restores_without_resampling() {
    let (mut session, npc, code) = npc_session();
    crate::session::pipeline::npc_state_projection_tests::use_buy_imbalance_analysis(
        &mut session,
        npc,
    );
    session.state.pending_npc = None;
    crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
    prepare_continuous_tick(&mut session).unwrap().commit();
    let resting = session.state.markets[&code].resting_orders_for(npc);
    assert_eq!(resting.len(), 1);
    assert_eq!(session.state.npc_order_lifecycles.len(), 1);
    let lifecycle = &session.state.npc_order_lifecycles[0];
    assert_eq!(lifecycle.order_id, resting[0].id);
    assert_eq!(
        lifecycle.expires_market_minute - lifecycle.placed_market_minute,
        session.npc_quote_lifetime_minutes(npc, &code, &resting[0])
    );
    restore_attention_profile(&mut session, npc);
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        restored.state.npc_order_lifecycles,
        session.state.npc_order_lifecycles
    );
    assert_eq!(
        serde_json::to_vec(&restored.save().unwrap()).unwrap(),
        serde_json::to_vec(&saved).unwrap()
    );
}

#[test]
fn npc_passive_full_fill_removes_existing_quote_lifecycle_before_save() {
    let (mut session, npc, code) = npc_session();
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let moment = crate::experience::ExperienceMoment {
        civil_date: session.civil_date(),
        market_minute: session.current_market_minute(),
        trading_day: u64::from(session.day()),
    };
    session
        .state
        .retail_experience
        .get_mut(&npc)
        .unwrap()
        .initialize_holding_dated(
            &code,
            Some(Money::from_cents(100_000)),
            Money::from_cents(1_000),
            moment,
        )
        .unwrap();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    assert_eq!(session.state.npc_order_lifecycles.len(), 1);
    let old_id = session.state.npc_order_lifecycles[0].order_id;
    restore_attention_profile(&mut session, npc);
    session.state.attention_scheduler.clear();
    session
        .state
        .npc_attention
        .get_mut(&npc)
        .unwrap()
        .next_attention_candidate_tick = 100;
    session.state.attention_scheduler.enqueue(100, npc);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    let result = prepare_continuous_tick(&mut session).unwrap().commit();
    assert!(result
        .output
        .receipts
        .iter()
        .any(|receipt| receipt.envelope.order == old_id
            && receipt.kind == ReceiptKind::Fill
            && receipt.qty_after == 0));
    assert!(session.state.npc_order_lifecycles.is_empty());
    assert!(session.state.markets[&code]
        .resting_orders_for(npc)
        .is_empty());
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert!(restored.state.npc_order_lifecycles.is_empty());
}

#[test]
fn npc_parent_child_uses_parent_horizon_instead_of_quote_lifecycle() {
    let (mut session, request) = fixture();
    let account = AccountId(1);
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_kind(AccountKind::Inst);
    let (mut validator, mut stock_execution) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    while execute(
        &mut session,
        &mut chain,
        &mut validator,
        &mut stock_execution,
    )
    .is_some()
    {}
    assert!(
        session.state.parent_orders[&account][&request.allocation.code]
            .active_child_order_id()
            .is_some()
    );
    assert!(session.state.npc_order_lifecycles.is_empty());
}

fn project_npc_limit_against_player_ask(qty: u32) -> (GameSession, OrderId) {
    let (mut session, npc, code) = npc_session();
    let player = AccountId(0);
    session
        .state
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
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let (mut validator, mut stock_execution) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let result = validator
        .consume(IntentCandidate::new(
            IntentCandidateKey::npc(npc, 0),
            npc,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty,
            },
        ))
        .unwrap();
    let operation = result.operation().unwrap().clone();
    let mut round = stock_execution.apply_round(vec![operation]).unwrap();
    let order_id = round.facts[0].allocated_order_id.unwrap();
    chain
        .project_execution_round(&mut session, &mut round)
        .unwrap();
    (session, order_id)
}

#[test]
fn npc_partial_resting_fill_registers_once_and_immediate_full_fill_does_not_register() {
    let (partial, order_id) = project_npc_limit_against_player_ask(200);
    assert_eq!(partial.state.npc_order_lifecycles.len(), 1);
    assert_eq!(partial.state.npc_order_lifecycles[0].order_id, order_id);
    let code = &partial.state.setup.stocks[0].code;
    assert_eq!(partial.state.markets[code].resting_orders()[0].qty, 100);
    let (filled, _) = project_npc_limit_against_player_ask(100);
    assert!(filled.state.npc_order_lifecycles.is_empty());
    assert!(filled.state.markets[code].resting_orders().is_empty());
}

#[test]
fn npc_quote_expiry_uses_acceptance_quote_when_a_later_round_operation_moves_the_spread() {
    let (mut session, npc, code) = npc_session();
    let player = AccountId(0);
    session
        .state
        .accounts
        .get_mut(&player)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let buy = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
        qty: 100,
    };
    let (mut validator, mut stock_execution) = seal(&mut session);
    // 受理时报价：NPC 买单是本轮首个操作，受理瞬间的盘口即 sealing 后、玩家卖单进入前。
    let (last_at_acceptance, bid_at_acceptance, ask_at_acceptance) = {
        let market = session.state.markets.get(&code).unwrap();
        (market.last_price(), market.best_bid(), market.best_ask())
    };
    let placed = session.current_market_minute();
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let operations = [
        IntentCandidate::new(IntentCandidateKey::npc(npc, 0), npc, buy),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_008)),
                qty: 100,
            },
        ),
    ]
    .into_iter()
    .map(|candidate| {
        validator
            .consume(candidate)
            .unwrap()
            .operation()
            .unwrap()
            .clone()
    })
    .collect();
    let mut round = stock_execution.apply_round(operations).unwrap();
    chain
        .project_execution_round(&mut session, &mut round)
        .unwrap();
    assert_eq!(session.state.npc_order_lifecycles.len(), 1);
    let lifecycle = &session.state.npc_order_lifecycles[0];
    assert_eq!(lifecycle.account, npc);
    assert_eq!(lifecycle.code, code);
    let resting = &session.state.markets[&code].resting_orders_for(npc)[0];
    assert_eq!(lifecycle.order_id, resting.id);
    assert_eq!(lifecycle.placed_market_minute, placed);
    // 期望期限由测试独立提供的受理时报价输入推导，不复制生产输出。
    let acceptance_lifetime = session.npc_quote_lifetime_minutes_at_quote(
        npc,
        &code,
        resting,
        last_at_acceptance,
        bid_at_acceptance,
        ask_at_acceptance,
    );
    assert_eq!(
        lifecycle.expires_market_minute,
        placed + acceptance_lifetime
    );
    assert_eq!(
        session.state.markets[&code].best_ask(),
        Some(Money::from_cents(1_008))
    );
    // 盘口移动后按当前簿重算得到不同期限：到期必须锚定受理时报价，而非投影后盘口。
    assert_ne!(
        acceptance_lifetime,
        session.npc_quote_lifetime_minutes(npc, &code, resting)
    );
}

#[test]
fn npc_new_quote_filled_later_in_the_same_round_has_no_stale_lifecycle() {
    let (mut session, npc, code) = npc_session();
    let player = AccountId(0);
    session
        .state
        .accounts
        .get_mut(&player)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let (mut validator, mut stock_execution) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let operations = [
        IntentCandidate::new(
            IntentCandidateKey::npc(npc, 0),
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        ),
    ]
    .into_iter()
    .map(|candidate| {
        validator
            .consume(candidate)
            .unwrap()
            .operation()
            .unwrap()
            .clone()
    })
    .collect();
    let mut round = stock_execution.apply_round(operations).unwrap();
    assert!(matches!(
        round.facts[0].outcome,
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Resting { .. },
            ..
        }
    ));
    let new_id = round.facts[0].allocated_order_id.unwrap();
    let mut projected_market = session.state.markets[&code].clone();
    projected_market
        .apply_changed_orders(
            round.projections[&code]
                .market_delta
                .as_ref()
                .unwrap()
                .clone(),
        )
        .unwrap();
    assert!(projected_market.resting_orders().is_empty());
    let finish = stock_execution.finish().unwrap();
    assert_eq!(
        finish.workers[0].market.filled_order_owner(new_id),
        Some(npc)
    );
    assert_eq!(projected_market.filled_order_owner(new_id), Some(npc));
    assert!(matches!(
        projected_market.cancel(new_id),
        Err(crate::market::MarketError::OrderBook(
            crate::orderbook::OrderError::OrderAlreadyFilled(id)
        )) if id == new_id
    ));
    chain
        .project_execution_round(&mut session, &mut round)
        .unwrap();
    assert!(session.state.npc_order_lifecycles.is_empty());
}

#[test]
fn npc_resting_fact_without_its_acceptance_quote_is_a_typed_failure() {
    let (mut session, npc, code) = npc_session();
    let (mut validator, mut stock_execution) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let step = validator
        .consume(IntentCandidate::new(
            IntentCandidateKey::npc(npc, 0),
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        ))
        .unwrap();
    let mut round = stock_execution
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
    assert_eq!(chain.consumed.operation_count(), 0);
    assert!(session.state.npc_order_lifecycles.is_empty());
}
