use super::*;
use engine::plans::PlanBook;
use engine::{AccountId, Side};

#[test]
fn restored_and_uninterrupted_sessions_keep_canonical_events_and_saves_identical() {
    // Given: two real twins split by a mid-scenario save after a trading day and civil settlement.
    let mut uninterrupted = fixture_session();
    run_trading_day(&mut uninterrupted);
    uninterrupted
        .end_civil_day()
        .expect("first civil day settles");
    let bytes = serde_json::to_vec(&uninterrupted.save()).expect("authoritative save serializes");
    let decoded =
        engine::session::decode_save_slot(&bytes, &Default::default()).expect("save decodes");
    let mut restored = GameSession::restore(&decoded).expect("save restores");
    assert_eq!(bytes, serde_json::to_vec(&restored.save()).unwrap());

    // When: both twins receive exactly the same market commands through one complete trading day.
    for tick in 0..TICKS_PER_DAY {
        let original_events = uninterrupted.step();
        let restored_events = restored.step();
        assert_eq!(
            serde_json::to_vec(&original_events).unwrap(),
            serde_json::to_vec(&restored_events).unwrap(),
            "tick {tick}: canonical events diverged"
        );
    }
    uninterrupted
        .end_civil_day()
        .expect("uninterrupted civil day settles");
    restored
        .end_civil_day()
        .expect("restored civil day settles");

    // Then: all authoritative K7 state remains byte-identical, not merely the visible snapshot.
    assert_eq!(
        serde_json::to_vec(&uninterrupted.save()).unwrap(),
        serde_json::to_vec(&restored.save()).unwrap()
    );
}

#[test]
fn live_partial_fill_restores_and_continues_identically() {
    // Given: an institution-owned 400-share plan with 100 filled and a live 300-share child.
    let mut uninterrupted = crate::matching::matching_session(1_000);
    uninterrupted.step();
    let stock = code("600101");
    let mut plans = PlanBook::default();
    let seller = crate::matching::plan(&mut plans, AccountId(1), stock.clone(), Side::Sell, 100);
    let buyer = crate::matching::plan(&mut plans, AccountId(2), stock.clone(), Side::Buy, 400);
    uninterrupted
        .execute_plan_observation(
            &mut plans,
            crate::matching::request(seller, stock.clone(), 100),
        )
        .unwrap();
    uninterrupted
        .execute_plan_observation(
            &mut plans,
            crate::matching::request(buyer, stock.clone(), 400),
        )
        .unwrap();
    uninterrupted
        .synchronize_plan_execution(&mut plans)
        .unwrap();
    let mut mid = uninterrupted.save();
    mid.plans = plans.clone();
    assert_eq!(
        mid.parent_orders[&AccountId(2)][&stock].active_child_remaining_qty,
        Some(300)
    );
    assert_eq!(
        mid.snapshot.accounts[&AccountId(2)].reserved_cash,
        Money::from_cents(300_003)
    );
    let mut uninterrupted = GameSession::restore(&mid).unwrap();
    let mut restored = GameSession::restore(&mid).unwrap();

    // When: both twins receive the same real sell-plan counterparty command through routing.
    let mut original_plans = mid.plans.clone();
    let mut restored_plans = mid.plans.clone();
    let original_seller = crate::matching::plan(
        &mut original_plans,
        AccountId(1),
        stock.clone(),
        Side::Sell,
        100,
    );
    let restored_seller = crate::matching::plan(
        &mut restored_plans,
        AccountId(1),
        stock.clone(),
        Side::Sell,
        100,
    );
    let original = uninterrupted
        .execute_plan_observation(
            &mut original_plans,
            crate::matching::request(original_seller, stock.clone(), 100),
        )
        .unwrap();
    let replay = restored
        .execute_plan_observation(
            &mut restored_plans,
            crate::matching::request(restored_seller, stock.clone(), 100),
        )
        .unwrap();
    let mut original_after = uninterrupted.save();
    original_after.plans = original_plans.clone();
    uninterrupted = GameSession::restore(&original_after).unwrap();
    let mut restored_after = restored.save();
    restored_after.plans = restored_plans.clone();
    restored = GameSession::restore(&restored_after).unwrap();

    // Then: events and authoritative states remain canonical across restore.
    assert_eq!(
        serde_json::to_vec(&original.events).unwrap(),
        serde_json::to_vec(&replay.events).unwrap()
    );
    assert!(original
        .events
        .iter()
        .any(|event| matches!(event, engine::Event::Trade { qty: 100, .. })));
    assert_eq!(original_plans.plan(buyer).unwrap().filled_qty, 200);
    assert_eq!(restored_plans.plan(buyer).unwrap().filled_qty, 200);
    assert_eq!(
        uninterrupted.save().plans.plan(buyer).unwrap().filled_qty,
        200
    );
    assert_eq!(restored.save().plans.plan(buyer).unwrap().filled_qty, 200);
    assert_eq!(
        uninterrupted.save().parent_orders[&AccountId(2)][&stock].active_child_remaining_qty,
        Some(200)
    );
    assert_eq!(
        restored.save().parent_orders[&AccountId(2)][&stock].active_child_remaining_qty,
        Some(200)
    );
    assert!(
        uninterrupted.save().snapshot.accounts[&AccountId(2)].reserved_cash
            < Money::from_cents(300_003)
    );
    assert_eq!(
        serde_json::to_vec(&uninterrupted.save()).unwrap(),
        serde_json::to_vec(&restored.save()).unwrap()
    );
    println!(
        "{{\"scenario\":\"partial_fill_restore\",\"buyer_plan\":{},\"remaining_before\":300,\"event_bytes\":{},\"save_bytes\":{}}}",
        buyer.0,
        serde_json::to_vec(&original.events).unwrap().len(),
        serde_json::to_vec(&restored.save()).unwrap().len()
    );
}
