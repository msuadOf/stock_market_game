use super::*;
use engine::AccountId;

#[test]
fn session_owned_positive_plan_with_real_child_is_canceled_by_real_decline_routing() {
    // Given: a session-owned positive institution plan and linked child created by real execution.
    let mut game = crate::matching::matching_session(1_000);
    for _ in 0..8 {
        game.step();
    }
    let stock = code("600101");
    let mut plans = engine::plans::PlanBook::default();
    let plan_id = crate::matching::plan(
        &mut plans,
        AccountId(2),
        stock.clone(),
        engine::Side::Buy,
        400,
    );
    game.execute_plan_observation(
        &mut plans,
        crate::matching::request(plan_id, stock.clone(), 400),
    )
    .unwrap();
    game.synchronize_plan_execution(&mut plans).unwrap();
    let child_id = game.save().parent_orders[&AccountId(2)][&stock]
        .active_child_order_id
        .unwrap();
    let mut save = game.save();
    save.plans = plans;
    let closes = save.market_minute_closes.get_mut(&stock).unwrap();
    closes.clear();
    for minute in 0..30_u64 {
        closes.push(engine::MarketMinuteClose {
            absolute_trading_minute: minute,
            close: Money::from_cents(10_000),
        });
    }
    closes.push(engine::MarketMinuteClose {
        absolute_trading_minute: 30,
        close: Money::from_cents(9_700),
    });
    closes.push(engine::MarketMinuteClose {
        absolute_trading_minute: 31,
        close: Money::from_cents(9_627),
    });
    let attention = save.npc_attention.get_mut(&AccountId(2)).unwrap();
    attention.next_attention_candidate_tick = save.snapshot.tick;
    attention.rng_state = 0_u64.wrapping_sub(0x9E37_79B9_7F4A_7C15);
    let opinion_before = save.plans.plan(plan_id).unwrap().opinion.signal_score_bp;
    let mut ready = GameSession::restore(&save).unwrap();

    // When: normal session stepping runs urgency, quote policy, and real cancel routing.
    let events = ready.step();
    let after = ready.save();

    // Then: the linked child is canceled, freeze released, and no contradictory replacement exists.
    assert!(opinion_before > 0);
    assert!(events
        .iter()
        .any(|event| matches!(event, engine::Event::OrderCanceled { id, .. } if *id == child_id)));
    assert_eq!(
        after.snapshot.accounts[&AccountId(2)].reserved_cash,
        Money::ZERO
    );
    assert!(after.resting_orders[&stock]
        .iter()
        .all(|order| order.owner != AccountId(2)));
    println!(
        "{{\"scenario\":\"session_cheap_withdraw\",\"plan_id\":{},\"child_id\":{},\"opinion_bp\":{},\"reserved_after\":0}}",
        plan_id.0, child_id.0, opinion_before
    );
}
