use super::*;
use engine::plans::quote_policy::QuoteAction;
use engine::session::PlanExecutionDisposition;
use engine::{OrderId, RejectionReason};

#[test]
fn failed_cancel_keeps_the_old_child_and_its_reservation() {
    let mut session = session(setup(0, 8, 0, 0));
    let mut plans = PlanBook::default();
    let plan_id = create_plan(&mut plans, AccountId(0), Side::Buy, 400);
    let accepted = session
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
        .expect("child is accepted");
    let active_id = submitted_id(&accepted);

    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Replace {
                    order_id: OrderId(active_id.0 + 99),
                    price: Money::from_cents(901),
                    qty: 100,
                },
            ),
        )
        .expect("route rejection remains observable");

    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::OrderNotFound
        }
    ));
    let resting = &session.save().resting_orders[&code()];
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].id, active_id);
}

#[test]
fn allocation_shortfall_prevents_submit_before_any_order_is_created() {
    let mut session = session(setup(0, 8, 0, 0));
    let mut plans = PlanBook::default();
    let plan_id = create_plan(&mut plans, AccountId(0), Side::Buy, 400);
    let mut denied = request(
        plan_id,
        Side::Buy,
        QuoteAction::Submit {
            price: Money::from_cents(900),
            qty: 100,
        },
    );
    denied.allocation.allocated_cash = Money::ZERO;

    let error = session
        .execute_plan_observation(&mut plans, denied)
        .expect_err("soft budget must constrain submission");

    assert!(matches!(
        error,
        engine::session::PlanExecutionError::AllocationInsufficient {
            plan_id: id,
            allocated_cents: 0,
            required_cents,
        } if id == plan_id && required_cents > 0
    ));
    assert!(session.save().resting_orders[&code()].is_empty());
}
