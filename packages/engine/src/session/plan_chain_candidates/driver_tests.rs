use super::driver::{PlanChainContinuationShadow, PlanChainYieldDriver, PlanChainYieldDriverError};
use super::*;
use crate::plans::PlanBook;
use crate::session::plan_execution::{PlanExecutionDisposition, PlanRouteOutcome};

fn driver_with_conflicting_orders() -> (
    GameSession,
    PlanBook,
    PlanChainContinuationShadow,
    PlanChainYieldDriver,
    StockCode,
    Vec<OrderId>,
) {
    let (mut session, request) = super::super::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    let code = request.allocation.code.clone();
    session
        .accounts
        .get_mut(&owner)
        .expect("fixture plan owner must exist")
        .kind = AccountKind::Player;
    let mut setup_events = Vec::new();
    for price in [901, 902] {
        session.route_intent(
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

    let mut plans = std::mem::take(&mut session.plans);
    let progress = session
        .prepare_plan_observation(&mut plans, request)
        .expect("fixture must prepare a route continuation");
    let shadow = PlanChainContinuationShadow::capture(&session, &plans)
        .expect("fixture must capture its continuation shadow");
    (
        session,
        plans,
        shadow,
        PlanChainYieldDriver::new(progress),
        code,
        order_ids,
    )
}

#[test]
fn yields_one_command_at_a_time_then_completes() {
    let (session, plans, mut shadow, mut driver, code, order_ids) =
        driver_with_conflicting_orders();
    let identities = (session.next_order_id, session.seq);
    let session_before = serde_json::to_value(session.save().unwrap()).unwrap();
    let plans_before = serde_json::to_value(&plans).unwrap();
    let shadow_before = shadow.serialized_state_for_test();

    let first = driver.yield_next().unwrap();
    assert_eq!(first.chain_generation_index, 0);
    assert!(matches!(
        first.intent,
        Intent::Cancel { code: actual, id } if actual == code && id == order_ids[0]
    ));
    assert!(matches!(
        driver.yield_next(),
        Err(PlanChainYieldDriverError::OutcomeRequired)
    ));

    driver
        .resume(&mut shadow, PlanRouteOutcome::Canceled(order_ids[0]))
        .unwrap();
    let second = driver.yield_next().unwrap();
    assert_eq!(second.chain_generation_index, 1);
    assert!(matches!(
        second.intent,
        Intent::Cancel { code: actual, id } if actual == code && id == order_ids[1]
    ));

    driver
        .resume(&mut shadow, PlanRouteOutcome::Canceled(order_ids[1]))
        .unwrap();
    let third = driver.yield_next().unwrap();
    assert_ne!(
        shadow.serialized_state_for_test(),
        shadow_before,
        "a successful continuation advances only its explicit shadow"
    );
    assert_eq!(third.chain_generation_index, 2);
    assert!(matches!(
        third.intent,
        Intent::PlaceLimit { code: actual, side: Side::Buy, price, qty }
            if actual == code && price == Money::from_cents(900) && qty == 100
    ));

    driver
        .resume(&mut shadow, PlanRouteOutcome::Accepted(OrderId(9_999)))
        .unwrap();
    assert!(matches!(
        driver.yield_next(),
        Err(PlanChainYieldDriverError::CompletionAvailable)
    ));
    assert!(matches!(
        driver.take_completion().unwrap().disposition,
        PlanExecutionDisposition::Submitted {
            order_id: OrderId(9_999),
            ..
        }
    ));
    assert!(matches!(
        driver.take_completion(),
        Err(PlanChainYieldDriverError::CompletionAlreadyTaken)
    ));
    assert_eq!(
        (session.next_order_id, session.seq),
        identities,
        "the yield/resume adapter must not allocate a global order id or event sequence"
    );
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        session_before
    );
    assert_eq!(serde_json::to_value(&plans).unwrap(), plans_before);
}

#[test]
fn ordinary_rejection_completes_without_yielding_a_follow_up_command() {
    let (_, _, mut shadow, mut driver, _, _) = driver_with_conflicting_orders();
    let _ = driver.yield_next().unwrap();

    driver
        .resume(
            &mut shadow,
            PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
        )
        .unwrap();

    assert!(matches!(
        driver.take_completion().unwrap().disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::InsufficientCash
        }
    ));
}

#[test]
fn missing_cancel_target_completes_as_an_explicit_route_rejection() {
    let (_, _, mut shadow, mut driver, _, _) = driver_with_conflicting_orders();
    let _ = driver.yield_next().unwrap();

    driver
        .resume(
            &mut shadow,
            PlanRouteOutcome::Rejected(RejectionReason::OrderNotFound),
        )
        .unwrap();

    assert!(matches!(
        driver.take_completion().unwrap().disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::OrderNotFound
        }
    ));
}

#[test]
fn mismatched_cancel_outcome_fails_without_mutating_session_or_plan_book() {
    let (session, plans, mut shadow, mut driver, _, order_ids) = driver_with_conflicting_orders();
    let _ = driver.yield_next().unwrap();
    let session_before = serde_json::to_value(session.save().unwrap()).unwrap();
    let plans_before = serde_json::to_value(&plans).unwrap();
    let shadow_before = shadow.serialized_state_for_test();

    assert!(matches!(
        driver.resume(&mut shadow, PlanRouteOutcome::Canceled(order_ids[1]),),
        Err(PlanChainYieldDriverError::Execution(
            PlanExecutionError::InvalidRouteOutcome
        ))
    ));
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        session_before
    );
    assert_eq!(serde_json::to_value(&plans).unwrap(), plans_before);
    assert_eq!(shadow.serialized_state_for_test(), shadow_before);
}

#[test]
fn exhausted_generation_index_fails_before_routing_or_mutating_state() {
    let (session, plans, shadow, mut driver, _, _) = driver_with_conflicting_orders();
    driver.set_next_generation_index_for_test(u64::MAX);
    let session_before = serde_json::to_value(session.save().unwrap()).unwrap();
    let plans_before = serde_json::to_value(&plans).unwrap();
    let shadow_before = shadow.serialized_state_for_test();
    let session = session;
    let plans = plans;

    assert!(matches!(
        driver.yield_next(),
        Err(PlanChainYieldDriverError::Execution(
            PlanExecutionError::CommandOrdinalOverflow
        ))
    ));
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        session_before
    );
    assert_eq!(serde_json::to_value(&plans).unwrap(), plans_before);
    assert_eq!(shadow.serialized_state_for_test(), shadow_before);
}

#[test]
fn next_generation_index_overflow_discards_the_resumed_shadow() {
    let (session, plans, mut shadow, mut driver, _, order_ids) = driver_with_conflicting_orders();
    driver.set_next_generation_index_for_test(u64::MAX - 1);
    let yielded = driver.yield_next().unwrap();
    assert_eq!(yielded.chain_generation_index, u64::MAX - 1);
    let session_before = serde_json::to_value(session.save().unwrap()).unwrap();
    let plans_before = serde_json::to_value(&plans).unwrap();
    let shadow_before = shadow.serialized_state_for_test();

    assert!(matches!(
        driver.resume(&mut shadow, PlanRouteOutcome::Canceled(order_ids[0]),),
        Err(PlanChainYieldDriverError::Execution(
            PlanExecutionError::CommandOrdinalOverflow
        ))
    ));
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        session_before
    );
    assert_eq!(serde_json::to_value(&plans).unwrap(), plans_before);
    assert_eq!(shadow.serialized_state_for_test(), shadow_before);
}
