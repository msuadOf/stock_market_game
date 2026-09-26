use super::*;
use crate::plans::{
    OpinionSource, PlanEvent, PlanOpen, PlanOpinion, PlanRevision, RevisionReason, Urgency,
};

fn fixture() -> (
    GameSession,
    PlanBook,
    crate::plans::TradingPlan,
    NewChildSpec,
) {
    fixture_with_target(100)
}

fn fixture_with_target(
    target_qty: u32,
) -> (
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
            target: PlanTarget::ShareCount(target_qty),
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

fn smaller_target_revision(target_qty: u32) -> PlanRevision {
    PlanRevision {
        reason: RevisionReason::SignalShift,
        trading_day: 0,
        direction: Side::Buy,
        target: PlanTarget::ShareCount(target_qty),
        opinion: PlanOpinion {
            signal_score_bp: 4_000,
            source: OpinionSource::Blended,
        },
        confidence_bp: 8_000,
        urgency: Urgency::Normal,
        below_filled_rationale: None,
    }
}

#[test]
fn restructure_rechecks_partial_fill_before_applying_smaller_target() {
    let (mut session, mut plans, plan, child) = fixture();
    let order_id = OrderId(7);
    session
        .install_plan_parent(&plan, child, Some((order_id, 100)))
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    let observed = plans.plan(plan.plan_id).unwrap().clone();
    let PlanExecutionProgress::Route(route) =
        PlanExecutionProgress::restructure(&observed, order_id, smaller_target_revision(50), false)
    else {
        panic!("active child must yield a cancellation route");
    };
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty: 60,
                child_complete: false,
                trading_day: 0,
            },
        )
        .unwrap();
    // P4 has canceled the unfilled remainder before the continuation resumes.
    let parent = session
        .parent_orders
        .get_mut(&plan.account)
        .unwrap()
        .get_mut(&plan.code)
        .unwrap();
    parent.filled_qty = 60;
    parent.active_child_order_id = None;
    parent.active_child_remaining_qty = None;
    let progress = route
        .resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Canceled(order_id),
        )
        .unwrap();
    assert!(matches!(
        progress,
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::Waiting {
                reason: QuoteReason::PendingReconsideration
            }
        })
    ));
    let current = plans.plan(plan.plan_id).unwrap();
    assert_eq!(current.target, PlanTarget::ShareCount(100));
    assert_eq!(current.filled_qty, 60);
    assert_eq!(current.active_child_order_id, None);
    assert_eq!(
        session.parent_orders[&plan.account][&plan.code].active_child_order_id,
        None
    );
    let restored: PlanBook = serde_json::from_str(&serde_json::to_string(&plans).unwrap()).unwrap();
    assert_eq!(
        restored.plan(plan.plan_id).unwrap().active_child_order_id,
        None
    );
}

#[test]
fn replace_rechecks_partial_fill_after_canceling_the_old_child() {
    let (mut session, mut plans, plan, child) = fixture();
    let order_id = OrderId(8);
    session
        .install_plan_parent(&plan, child, Some((order_id, child.qty)))
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    let progress =
        PlanExecutionProgress::replace(plans.plan(plan.plan_id).unwrap().clone(), child, order_id);
    let PlanExecutionProgress::Route(route) = progress else {
        panic!("replacement must first cancel its live child");
    };
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty: 60,
                child_complete: false,
                trading_day: 0,
            },
        )
        .unwrap();
    let parent = session
        .parent_orders
        .get_mut(&plan.account)
        .unwrap()
        .get_mut(&plan.code)
        .unwrap();
    parent.filled_qty = 60;
    parent.active_child_order_id = None;
    parent.active_child_remaining_qty = None;

    let result = route
        .resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Canceled(order_id),
        )
        .unwrap();
    assert!(matches!(
        result,
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::Waiting {
                reason: QuoteReason::PendingReconsideration
            }
        })
    ));
    let current = plans.plan(plan.plan_id).unwrap();
    assert_eq!(current.filled_qty, 60);
    assert_eq!(current.active_child_order_id, None);
    assert_eq!(
        session.parent_orders[&plan.account][&plan.code].active_child_order_id,
        None
    );
}

#[test]
fn completed_child_makes_cancel_failure_a_business_report() {
    let (mut session, mut plans, plan, _) = fixture();
    let order_id = OrderId(7);
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    let observed = plans.plan(plan.plan_id).unwrap().clone();
    let PlanExecutionProgress::Route(route) =
        PlanExecutionProgress::restructure(&observed, order_id, smaller_target_revision(50), false)
    else {
        panic!("active child must yield a cancellation route");
    };
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty: 100,
                child_complete: true,
                trading_day: 0,
            },
        )
        .unwrap();
    let progress = route
        .resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Rejected(RejectionReason::OrderAlreadyFilled),
        )
        .unwrap();
    assert!(matches!(
        progress,
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::RouteRejected {
                reason: RejectionReason::OrderAlreadyFilled
            }
        })
    ));
    assert_eq!(
        plans.plan(plan.plan_id).unwrap().status,
        PlanStatus::Completed
    );
}

#[test]
fn filled_child_can_reject_cancellation_while_its_plan_remains_active() {
    let (mut session, mut plans, plan, _) = fixture();
    let order_id = OrderId(9);
    plans
        .apply(
            plan.plan_id,
            PlanEvent::Revised {
                revision: smaller_target_revision(200),
            },
        )
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    let observed = plans.plan(plan.plan_id).unwrap().clone();
    let PlanExecutionProgress::Route(route) = PlanExecutionProgress::restructure(
        &observed,
        order_id,
        smaller_target_revision(150),
        false,
    ) else {
        panic!("active child must yield a cancellation route");
    };
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty: 100,
                child_complete: true,
                trading_day: 0,
            },
        )
        .unwrap();
    let progress = route
        .resume(
            &mut session,
            &mut plans,
            PlanRouteOutcome::Rejected(RejectionReason::OrderAlreadyFilled),
        )
        .unwrap();
    assert!(matches!(
        progress,
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::RouteRejected {
                reason: RejectionReason::OrderAlreadyFilled
            }
        })
    ));
    let current = plans.plan(plan.plan_id).unwrap();
    assert_eq!(current.status, PlanStatus::Active);
    assert_eq!(current.filled_qty, 100);
    assert_eq!(current.target, PlanTarget::ShareCount(200));
    assert_eq!(current.active_child_order_id, None);
}

#[test]
fn a_new_plan_submit_keeps_parent_private_until_the_stock_accepts_it() {
    let (mut session, _plans, plan, child) = fixture();
    let plan_id = plan.plan_id;
    let account = plan.account;
    let code = plan.code.clone();
    let progress = session
        .prepare_working_cancels(plan, child, Vec::new())
        .expect("a new plan child must be routable");
    let PlanExecutionProgress::Route(route) = progress else {
        panic!("a new child must yield a submit route");
    };
    assert!(
        session.parent_orders.is_empty(),
        "candidate generation must not install an unaccepted parent"
    );
    route
        .install_accepted_submit_parent(
            &mut session,
            &PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
        )
        .unwrap();
    assert!(session.parent_orders.is_empty());
    route
        .install_accepted_submit_parent(&mut session, &PlanRouteOutcome::Accepted(OrderId(4)))
        .unwrap();
    let parent = &session.parent_orders[&account][&code];
    assert_eq!(parent.linked_plan_id, Some(plan_id));
    assert_eq!(parent.active_child_order_id, None);
}

#[test]
fn adopting_an_existing_order_is_decided_without_writing_plan_state() {
    let (mut session, mut plans, plan, child) = fixture();
    let order_id = OrderId(7);
    session
        .markets
        .get_mut(&plan.code)
        .unwrap()
        .place(crate::Order {
            id: order_id,
            side: plan.direction,
            price: child.price,
            qty: child.qty,
            original_qty: child.qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: plan.account,
            seq: 7,
        })
        .unwrap();
    let before_plans = plans.clone();
    let before_parents = session.parent_orders.clone();
    let before_pending = session.pending_plan_events.clone();

    let observation: &GameSession = &session;
    let progress = observation
        .submit_plan_child(&plan, child)
        .expect("the matching live order must be adoptable");
    assert!(matches!(progress, PlanExecutionProgress::Adoption { .. }));
    assert_eq!(plans, before_plans);
    assert_eq!(session.parent_orders, before_parents);
    assert_eq!(session.pending_plan_events, before_pending);

    let progress = session
        .materialize_plan_progress(&mut plans, progress)
        .expect("adoption must install the accepted order's parent link");
    assert!(matches!(
        progress,
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::Adopted { order_id: id, .. },
            ..
        }) if id == order_id
    ));
    assert_eq!(
        session.parent_orders[&plan.account][&plan.code].active_child_order_id,
        Some(order_id)
    );
    assert_eq!(
        plans.plan(plan.plan_id).unwrap().active_child_order_id,
        Some(order_id)
    );
}

#[test]
fn stale_submit_with_an_active_child_waits_for_reconsideration() {
    let (mut session, mut plans, plan, child) = fixture();
    let order_id = OrderId(77);
    session
        .install_plan_parent(&plan, child, Some((order_id, child.qty)))
        .unwrap();
    session
        .markets
        .get_mut(&plan.code)
        .unwrap()
        .place(crate::Order {
            id: order_id,
            side: plan.direction,
            price: child.price,
            qty: child.qty,
            original_qty: child.qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: plan.account,
            seq: 77,
        })
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    let plan = plans.plan(plan.plan_id).unwrap().clone();
    let before_parent = session.parent_orders.clone();
    let before_pending = session.pending_plan_events.clone();

    let progress = session.submit_plan_child(&plan, child).unwrap();

    assert!(matches!(
        progress,
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::PendingReconsideration {
                order_id: id,
                ..
            },
        }) if id == order_id
    ));
    assert_eq!(session.parent_orders, before_parent);
    assert_eq!(session.pending_plan_events, before_pending);
    assert!(plans.plan(plan.plan_id).is_ok());
    let mut mismatched_plan = plan.clone();
    mismatched_plan.active_child_order_id = Some(OrderId(78));
    assert!(matches!(
        session.submit_plan_child(&mismatched_plan, child),
        Err(PlanExecutionError::IncompatibleExecutionState { .. })
    ));
    session
        .parent_orders
        .get_mut(&plan.account)
        .unwrap()
        .get_mut(&plan.code)
        .unwrap()
        .active_child_remaining_qty = Some(50);
    assert!(matches!(
        session.submit_plan_child(&plan, child),
        Err(PlanExecutionError::IncompatibleExecutionState { .. })
    ));
    session
        .parent_orders
        .get_mut(&plan.account)
        .unwrap()
        .get_mut(&plan.code)
        .unwrap()
        .active_child_remaining_qty = Some(child.qty);
    session
        .markets
        .get_mut(&plan.code)
        .unwrap()
        .cancel(order_id)
        .unwrap();
    assert!(matches!(
        session.submit_plan_child(&plan, child),
        Err(PlanExecutionError::IncompatibleExecutionState { .. })
    ));
}

#[test]
fn partly_filled_child_supersedes_a_stale_submit_before_quantity_validation() {
    let (mut session, mut plans, plan, mut child) = fixture_with_target(200);
    child.qty = 200;
    child.remaining = 200;
    let order_id = OrderId(78);
    session
        .install_plan_parent(&plan, child, Some((order_id, 150)))
        .unwrap();
    let parent = session
        .parent_orders
        .get_mut(&plan.account)
        .unwrap()
        .get_mut(&plan.code)
        .unwrap();
    parent.filled_qty = 50;
    session
        .markets
        .get_mut(&plan.code)
        .unwrap()
        .place(crate::Order {
            id: order_id,
            side: plan.direction,
            price: child.price,
            qty: 150,
            original_qty: 200,
            filled_qty: 50,
            filled_value: child.price.mul_shares(50).unwrap(),
            owner: plan.account,
            seq: 78,
        })
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty: 50,
                child_complete: false,
                trading_day: 0,
            },
        )
        .unwrap();
    let request = PlanExecutionRequest {
        plan_id: plan.plan_id,
        allocation: AllocationGrant {
            plan_id: plan.plan_id,
            code: plan.code.clone(),
            allocated_cash: Money::from_cents(1_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action: QuoteAction::Submit {
                price: child.price,
                qty: 200,
            },
            reason: child.reason,
        },
        trading_day: 0,
    };

    assert!(matches!(
        session.prepare_plan_observation(&plans, request).unwrap(),
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::PendingReconsideration {
                order_id: id,
                ..
            },
        }) if id == order_id
    ));
}

#[test]
fn completed_plan_reports_a_stale_submit_without_reopening_it() {
    let (mut session, mut plans, plan, child) = fixture();
    let order_id = OrderId(79);
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day: 0,
            },
        )
        .unwrap();
    plans
        .apply(
            plan.plan_id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty: child.qty,
                child_complete: true,
                trading_day: 0,
            },
        )
        .unwrap();
    let before = plans.clone();
    let request = PlanExecutionRequest {
        plan_id: plan.plan_id,
        allocation: AllocationGrant {
            plan_id: plan.plan_id,
            code: plan.code.clone(),
            allocated_cash: Money::from_cents(1_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action: QuoteAction::Submit {
                price: child.price,
                qty: child.qty,
            },
            reason: child.reason,
        },
        trading_day: 0,
    };

    assert!(matches!(
        session
            .prepare_plan_observation(&plans, request.clone())
            .unwrap(),
        PlanExecutionProgress::Complete(PlanExecutionReport {
            disposition: PlanExecutionDisposition::Waiting {
                reason: QuoteReason::PendingReconsideration
            }
        })
    ));
    assert_eq!(plans, before);
    session
        .install_plan_parent(&plan, child, Some((order_id, child.qty)))
        .unwrap();
    assert!(matches!(
        session.prepare_plan_observation(&plans, request),
        Err(PlanExecutionError::IncompatibleExecutionState { .. })
    ));
}

#[test]
fn an_adoption_decision_cannot_install_a_parent_after_its_order_disappears() {
    let (mut session, mut plans, plan, child) = fixture();
    let order_id = OrderId(8);
    session
        .markets
        .get_mut(&plan.code)
        .unwrap()
        .place(crate::Order {
            id: order_id,
            side: plan.direction,
            price: child.price,
            qty: child.qty,
            original_qty: child.qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: plan.account,
            seq: 8,
        })
        .unwrap();
    let progress = session.submit_plan_child(&plan, child).unwrap();
    session
        .markets
        .get_mut(&plan.code)
        .unwrap()
        .cancel(order_id)
        .unwrap();
    let before = plans.clone();

    assert!(matches!(
        session.materialize_plan_progress(&mut plans, progress),
        Err(PlanExecutionError::IncompatibleExecutionState { .. })
    ));
    assert_eq!(plans, before);
    assert!(session.parent_orders.is_empty());
    assert!(session.pending_plan_events.is_empty());
}

#[test]
fn plan_working_orders_reads_only_the_target_stock_and_account() {
    let mut session = GameSession::new(
        super::super::npc_working_quote_tests::two_stock_quote_setup(),
        47,
    )
    .unwrap();
    let owner = AccountId(1);
    let other = AccountId(0);
    let target = StockCode("600888".into());
    let second = StockCode("600889".into());
    for (code, id, account, price, seq) in [
        (&target, 40, owner, 903, 4),
        (&target, 2, other, 901, 2),
        (&second, 3, owner, 902, 3),
        (&target, 10, owner, 900, 7),
    ] {
        session
            .markets
            .get_mut(code)
            .unwrap()
            .place(crate::Order {
                id: OrderId(id),
                side: Side::Buy,
                price: Money::from_cents(price),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: account,
                seq,
            })
            .unwrap();
    }
    session.auction_orders.insert(
        target.clone(),
        vec![
            crate::session::AuctionOrderSnap {
                owner,
                side: Side::Buy,
                limit: Money::from_cents(906),
                qty: 100,
                order_id: 99,
            },
            crate::session::AuctionOrderSnap {
                owner,
                side: Side::Buy,
                limit: Money::from_cents(905),
                qty: 100,
                order_id: 70,
            },
        ],
    );

    let mut orders = session
        .plan_working_orders(owner, &target)
        .into_iter()
        .map(|order| (order.id, order.price.cents()))
        .collect::<Vec<_>>();
    orders.sort_by_key(|(id, _)| *id);
    assert_eq!(
        orders,
        vec![
            (OrderId(10), 900),
            (OrderId(40), 903),
            (OrderId(70), 905),
            (OrderId(99), 906),
        ]
    );
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
            child_complete: true,
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
            child_complete: true,
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
