use super::*;
use engine::plans::quote_policy::QuoteAction;
use engine::session::PlanExecutionDisposition;

#[test]
fn opening_auction_after_0920_suppresses_a_conflicting_replacement() {
    let mut session = session(setup(0, 4, 2, 0));
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
        .expect("auction submission is accepted");
    let active_id = submitted_id(&accepted);

    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Replace {
                    order_id: active_id,
                    price: Money::from_cents(901),
                    qty: 100,
                },
            ),
        )
        .expect("noncancelable state is explicit");

    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::PendingReconsideration { order_id, .. }
            if order_id == active_id
    ));
    assert_eq!(session.save().auction_orders[&code()].len(), 1);
}

#[test]
fn opening_auction_remainder_keeps_its_link_when_carried_into_continuous() {
    let mut session = session(setup(0, 4, 3, 0));
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
        .expect("opening-auction submission is accepted");
    let active_id = submitted_id(&accepted);

    // Two steps finish the entry window; the unmatched remainder is carried into
    // the continuous book with its original id and queue order.
    session.step();
    session.step();
    let carried = &session.save().resting_orders[&code()];
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].id, active_id);

    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Keep {
                    order_id: active_id,
                },
            ),
        )
        .expect("carried child keeps its plan link");

    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::Kept { order_id, .. } if order_id == active_id
    ));
}

#[test]
fn closing_auction_suppresses_a_conflicting_replacement() {
    let mut session = session(setup(0, 2, 0, 1));
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
        .expect("continuous child is accepted");
    let active_id = submitted_id(&accepted);
    session.step();

    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Buy,
                QuoteAction::Replace {
                    order_id: active_id,
                    price: Money::from_cents(901),
                    qty: 100,
                },
            ),
        )
        .expect("closing conflict is explicit");

    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::PendingReconsideration { .. }
    ));
    assert!(session
        .save()
        .auction_orders
        .get(&code())
        .is_none_or(Vec::is_empty));
    assert_eq!(session.save().resting_orders[&code()].len(), 1);
}
