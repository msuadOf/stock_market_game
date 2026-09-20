use super::*;
use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, Urgency};
use std::collections::VecDeque;

fn fixture() -> (
    GameSession,
    PlanBook,
    crate::plans::TradingPlan,
    NewChildSpec,
) {
    let session =
        GameSession::new(super::super::npc_working_quote_tests::quote_setup(0), 47).unwrap();
    let mut plans = PlanBook::default();
    let plan_id = plans
        .create(PlanOpen {
            account: AccountId(0),
            code: StockCode("600888".into()),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    let plan = plans.plan(plan_id).unwrap().clone();
    (
        session,
        plans,
        plan,
        NewChildSpec {
            price: Money::from_cents(902),
            qty: 100,
            remaining: 100,
            reason: QuoteReason::OppositeQuoteProbe,
        },
    )
}

#[test]
fn conflicting_working_cancels_keep_order_then_submit_once() {
    let (mut session, mut plans, plan, child) = fixture();
    for price in [900, 901] {
        session.route_intent(
            plan.account,
            Intent::PlaceLimit {
                code: plan.code.clone(),
                side: Side::Buy,
                price: Money::from_cents(price),
                qty: 100,
            },
            &mut Vec::new(),
        );
    }
    let working = session.plan_working_orders(plan.account, &plan.code);
    let expected: Vec<_> = working.iter().map(|order| order.id).collect();
    let next_id = OrderId(session.next_order_id);
    let progress = session.submit_plan_child(&plan, child, &mut plans).unwrap();

    let report = session
        .consume_plan_execution(&mut plans, progress)
        .unwrap();

    let canceled: Vec<_> = report
        .events
        .iter()
        .filter_map(|event| match event {
            Event::OrderCanceled { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(canceled, expected);
    assert!(
        matches!(report.disposition, PlanExecutionDisposition::Submitted { order_id, .. } if order_id == next_id)
    );
    assert_eq!(session.next_order_id, next_id.0 + 1);
}

#[test]
fn conflicting_working_cancel_first_failure_stops_remaining_commands() {
    let (mut session, mut plans, plan, child) = fixture();
    session.route_intent(
        plan.account,
        Intent::PlaceLimit {
            code: plan.code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let existing = session.plan_working_orders(plan.account, &plan.code)[0].id;
    let next_id = session.next_order_id;
    let progress = session
        .prepare_working_cancels(
            plan.clone(),
            child,
            VecDeque::from([OrderId(999_999), existing]),
        )
        .unwrap();

    let report = session
        .consume_plan_execution(&mut plans, progress)
        .unwrap();

    assert!(matches!(
        report.events.as_slice(),
        [Event::IntentRejected {
            reason: RejectionReason::OrderNotFound,
            ..
        }]
    ));
    assert_eq!(session.next_order_id, next_id);
    assert_eq!(
        session.plan_working_orders(plan.account, &plan.code)[0].id,
        existing
    );
    assert!(session.parent_orders.is_empty());
}

#[test]
fn missing_and_contradictory_route_terminals_fail_closed() {
    let code = StockCode("600888".into());
    let canceled = Event::OrderCanceled {
        seq: 1,
        account: AccountId(0),
        code: code.clone(),
        id: OrderId(1),
        remaining_qty: 100,
    };
    for events in [Vec::new(), vec![canceled.clone(), canceled]] {
        assert!(matches!(
            GameSession::plan_route_outcome(&events, AccountId(0), &code, Some(OrderId(1))),
            Err(PlanExecutionError::InvalidRouteOutcome)
        ));
    }
}
