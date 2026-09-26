use super::*;
use crate::session::plan_execution::{PlanExecutionProgress, PlanRouteOutcome};

fn candidate_observation(session: &GameSession) -> (StateHash, serde_json::Value) {
    (
        session.business_state_hash().unwrap(),
        serde_json::to_value(session.snapshot()).unwrap(),
    )
}

fn fixture_with_two_live_orders() -> (GameSession, PlanExecutionRequest, Vec<OrderId>) {
    let (mut session, request) = super::super::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    let code = request.allocation.code.clone();
    // An NPC's legacy normal-quote routing cancels its existing same-side quote before
    // placing the next one. This adapter fixture requires two simultaneously live
    // commands, so make its otherwise strategy-free account a player explicitly.
    session
        .accounts
        .get_mut(&owner)
        .expect("fixture plan owner must exist")
        .kind = AccountKind::Player;
    let mut setup_events = Vec::new();
    for price in [901, 902] {
        session.seed_order_for_test(
            owner,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(price),
                qty: 100,
            },
            &mut setup_events,
        );
    }
    let order_ids = setup_events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(order_ids.len(), 2, "fixture must create two live orders");
    assert!(
        setup_events
            .iter()
            .all(|event| matches!(event, Event::OrderAccepted { .. })),
        "fixture must not replace either working order"
    );
    assert_eq!(
        session.markets[&code].resting_orders_for(owner).len(),
        2,
        "fixture orders must both remain live"
    );
    (session, request, order_ids)
}

#[test]
fn source_enumeration_preserves_multi_continuation_order_and_payload() {
    let (mut session, request, order_ids) = fixture_with_two_live_orders();
    let code = request.allocation.code.clone();
    let mut plans = std::mem::take(&mut session.plans);
    let first_route = match session
        .prepare_plan_observation(&mut plans, request)
        .unwrap()
    {
        PlanExecutionProgress::Route(route) => route,
        _ => panic!("fixture must yield a route continuation"),
    };
    let next_order_id = session.next_order_id;
    let mut batch = PlanChainOperationBatch::empty();

    let before_first = candidate_observation(&session);
    let first_candidate = batch.enumerate_candidate(first_route.command()).unwrap();
    assert_eq!(candidate_observation(&session), before_first);
    let mut cancel_events = Vec::new();
    session.seed_order_for_test(
        AccountId(1),
        Intent::Cancel {
            code: code.clone(),
            id: order_ids[0],
        },
        &mut cancel_events,
    );
    assert!(matches!(
        cancel_events.as_slice(),
        [Event::OrderCanceled { .. }]
    ));
    let second_route = match first_route
        .resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Canceled(order_ids[0]),
        )
        .unwrap()
    {
        PlanExecutionProgress::Route(route) => route,
        _ => panic!("first cancel must resume the continuation"),
    };
    let before_second = candidate_observation(&session);
    let second_candidate = batch.enumerate_candidate(second_route.command()).unwrap();
    assert_eq!(candidate_observation(&session), before_second);
    cancel_events.clear();
    session.seed_order_for_test(
        AccountId(1),
        Intent::Cancel {
            code: code.clone(),
            id: order_ids[1],
        },
        &mut cancel_events,
    );
    assert!(matches!(
        cancel_events.as_slice(),
        [Event::OrderCanceled { .. }]
    ));
    let third_route = match second_route
        .resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Canceled(order_ids[1]),
        )
        .unwrap()
    {
        PlanExecutionProgress::Route(route) => route,
        _ => panic!("second cancel must resume the continuation"),
    };
    let before_third = candidate_observation(&session);
    let third_candidate = batch.enumerate_candidate(third_route.command()).unwrap();
    assert_eq!(candidate_observation(&session), before_third);

    assert_eq!(
        [
            first_candidate.chain_generation_index,
            second_candidate.chain_generation_index,
            third_candidate.chain_generation_index,
        ],
        [0, 1, 2]
    );
    assert_eq!(
        [
            first_candidate.owner,
            second_candidate.owner,
            third_candidate.owner,
        ],
        [AccountId(1), AccountId(1), AccountId(1)]
    );
    assert_eq!(
        serde_json::to_value([
            first_candidate.intent,
            second_candidate.intent,
            third_candidate.intent
        ])
        .unwrap(),
        serde_json::to_value([
            Intent::Cancel {
                code: code.clone(),
                id: order_ids[0],
            },
            Intent::Cancel {
                code: code.clone(),
                id: order_ids[1],
            },
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
        ])
        .unwrap()
    );
    assert_eq!(session.next_order_id, next_order_id);
}

#[test]
fn source_rejects_a_conflicting_cancel_outcome_for_the_wrong_order() {
    let (mut session, request, order_ids) = fixture_with_two_live_orders();
    let mut plans = std::mem::take(&mut session.plans);
    let route = match session
        .prepare_plan_observation(&mut plans, request)
        .expect("fixture must prepare a conflicting-order cancellation")
    {
        PlanExecutionProgress::Route(route) => route,
        _ => panic!("fixture must yield a route continuation"),
    };

    assert!(matches!(
        route.resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Canceled(order_ids[1]),
        ),
        Err(PlanExecutionError::InvalidRouteOutcome)
    ));
}
