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

#[test]
fn owned_plan_sync_late_failure_keeps_pending_and_parent_unchanged() {
    let (mut session, mut plans, first, child) = fixture();
    let second_id = plans
        .create(PlanOpen {
            account: first.account,
            code: StockCode("600889".into()),
            direction: first.direction,
            target: first.target,
            opinion: first.opinion,
            confidence_bp: first.confidence_bp,
            urgency: first.urgency,
            horizon_trading_days: first.horizon_trading_days,
            created_trading_day: first.created_trading_day,
        })
        .unwrap();
    session
        .install_plan_parent(&first, child, Some((OrderId(1), 100)))
        .unwrap();
    session.pending_plan_events = vec![
        PendingPlanEvent::Accepted {
            plan_id: first.plan_id,
            order_id: OrderId(1),
            trading_day: 0,
        },
        PendingPlanEvent::Filled {
            plan_id: second_id,
            order_id: OrderId(2),
            qty: 100,
            trading_day: 0,
        },
    ];
    let plans_before = plans.clone();
    let pending_before = session.pending_plan_events.clone();
    let parents_before = session.parent_orders.clone();

    assert!(session
        .synchronize_owned_plan_execution(&mut plans)
        .is_err());
    assert_eq!(plans, plans_before);
    assert_eq!(session.pending_plan_events, pending_before);
    assert_eq!(session.parent_orders, parents_before);
}

#[test]
fn owned_plan_sync_consumes_facts_after_completion_and_removes_linked_parent() {
    let (mut session, mut plans, plan, child) = fixture();
    session
        .install_plan_parent(&plan, child, Some((OrderId(1), 100)))
        .unwrap();
    let late = PendingPlanEvent::DayEnded {
        plan_id: plan.plan_id,
        trading_day: 0,
    };
    session.pending_plan_events = vec![
        PendingPlanEvent::Accepted {
            plan_id: plan.plan_id,
            order_id: OrderId(1),
            trading_day: 0,
        },
        PendingPlanEvent::Filled {
            plan_id: plan.plan_id,
            order_id: OrderId(1),
            qty: 100,
            trading_day: 0,
        },
        late,
    ];

    session
        .synchronize_owned_plan_execution(&mut plans)
        .unwrap();

    assert_eq!(
        plans.plan(plan.plan_id).unwrap().status,
        PlanStatus::Completed
    );
    assert!(session.pending_plan_events.is_empty());
    assert!(session.parent_orders.is_empty());
    session.plans = plans;
    session
        .save()
        .expect("completed plan has no stale pending fact");
}

#[test]
fn owned_plan_sync_rejects_pending_fact_for_an_unknown_plan() {
    let (mut session, mut plans, _, _) = fixture();
    let fact = PendingPlanEvent::DayEnded {
        plan_id: PlanId(999),
        trading_day: 0,
    };
    session.pending_plan_events.push(fact);
    let before = plans.clone();

    assert!(matches!(
        session.synchronize_owned_plan_execution(&mut plans),
        Err(PlanExecutionError::Plan(
            crate::plans::PlanError::UnknownPlan {
                plan_id: PlanId(999)
            }
        ))
    ));
    assert_eq!(plans, before);
    assert_eq!(session.pending_plan_events, vec![fact]);
}
