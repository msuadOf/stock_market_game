use super::*;
use engine::plans::quote_policy::QuoteAction;
use engine::session::{Event, PlanExecutionDisposition};
use engine::Intent;

fn ordinary_quote(session: &mut GameSession) -> engine::OrderId {
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code(),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
        )
        .expect("player quote queues");
    session
        .step()
        .into_iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                id,
                account: AccountId(0),
                ..
            } => Some(id),
            _ => None,
        })
        .expect("ordinary quote rests")
}

#[test]
fn exact_legal_quote_is_adopted_without_losing_queue_priority() {
    let mut session = session(setup(0, 8, 0, 0));
    let old_id = ordinary_quote(&mut session);
    let mut plans = PlanBook::default();
    let plan_id = create_plan(&mut plans, AccountId(0), Side::Buy, 400);

    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(900),
                    qty: 100,
                },
            ),
        )
        .expect("exact quote is adopted");

    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::Adopted { order_id, .. } if order_id == old_id
    ));
    assert!(report.events.is_empty());
}

#[test]
fn second_submit_while_a_child_is_in_flight_is_rejected() {
    let mut session = session(setup(0, 8, 0, 0));
    let mut plans = PlanBook::default();
    let plan_id = create_plan(&mut plans, AccountId(0), Side::Buy, 400);
    let first = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(900),
                    qty: 100,
                },
            ),
        )
        .expect("first child is accepted");
    let active_id = submitted_id(&first);

    let error = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(901),
                    qty: 100,
                },
            ),
        )
        .expect_err("each account and stock may carry at most one in-flight child");

    assert!(matches!(
        error,
        engine::session::PlanExecutionError::IncompatibleExecutionState { plan_id: id }
            if id == plan_id
    ));
    let resting = &session.save().resting_orders[&code()];
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].id, active_id);
}

#[test]
fn changed_price_cancels_then_submits_with_a_new_order_id() {
    let mut session = session(setup(0, 8, 0, 0));
    let old_id = ordinary_quote(&mut session);
    let mut plans = PlanBook::default();
    let plan_id = create_plan(&mut plans, AccountId(0), Side::Buy, 400);
    session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(900),
                    qty: 100,
                },
            ),
        )
        .expect("old quote is adopted");

    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Replace {
                    order_id: old_id,
                    price: Money::from_cents(901),
                    qty: 100,
                },
            ),
        )
        .expect("changed price routes cancellation then submission");

    let new_id = match report.disposition {
        PlanExecutionDisposition::Replaced {
            canceled_order_id,
            order_id,
            ..
        } => {
            assert_eq!(canceled_order_id, old_id);
            order_id
        }
        other => panic!("expected replacement, got {other:?}"),
    };
    assert_ne!(new_id, old_id);
    assert!(matches!(
        report.events.as_slice(),
        [Event::OrderCanceled { id, .. }, Event::OrderAccepted { id: accepted, .. }]
            if *id == old_id && *accepted == new_id
    ));
}
