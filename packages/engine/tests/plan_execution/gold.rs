use super::*;
use engine::plans::quote_policy::QuoteAction;
use engine::session::{Event, PlanExecutionDisposition};
use engine::PlanStatus;

#[test]
fn real_counterparty_partially_fills_a_four_hundred_share_plan() {
    // Given: an institution offers 100 real shares and a retail plan targets 400 shares.
    let mut session = session(setup(1_000, 8, 0, 0));
    let mut plans = PlanBook::default();
    let seller = create_plan(&mut plans, AccountId(2), Side::Sell, 100);
    let buyer = create_plan(&mut plans, AccountId(1), Side::Buy, 400);
    session
        .execute_plan_observation(
            &mut plans,
            request(
                seller,
                Side::Sell,
                QuoteAction::Submit {
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            ),
        )
        .expect("seller child is accepted");

    // When: the 400-share plan submits through the real continuous-auction router.
    let report = session
        .execute_plan_observation(
            &mut plans,
            request(
                buyer,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(1_000),
                    qty: 400,
                },
            ),
        )
        .expect("buyer child routes and settles");

    // Then: only the real 100-share trade advances the plan; 300 remains on the real book.
    assert!(report
        .events
        .iter()
        .any(|event| matches!(event, Event::Trade { qty: 100, .. })));
    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::Submitted { .. }
    ));
    let plan = plans.plan(buyer).expect("buyer plan remains active");
    assert_eq!(plan.filled_qty, 100);
    assert_eq!(plan.status, PlanStatus::Active);
    assert_eq!(
        session.save().expect("healthy save").parent_orders[&AccountId(1)][&code()]
            .active_child_remaining_qty,
        Some(300)
    );
}

#[test]
fn external_institution_plan_survives_save_restore_and_continues() {
    // Given: the public execution adapter receives an institution-owned PlanBook whose first
    // plan has PlanId(0). The session must take ownership before it persists a linked parent.
    let mut session = session(setup(1_000, 8, 0, 0));
    let mut plans = PlanBook::default();
    let plan_id = create_plan(&mut plans, AccountId(2), Side::Sell, 200);
    assert_eq!(plan_id, PlanId(0));
    let first = session
        .execute_plan_observation(
            &mut plans,
            request(
                plan_id,
                Side::Sell,
                QuoteAction::Submit {
                    price: Money::from_cents(1_100),
                    qty: 100,
                },
            ),
        )
        .expect("institution child is accepted");
    let first_order_id = match first.disposition {
        PlanExecutionDisposition::Submitted { order_id, .. } => order_id,
        other => panic!("expected submission, got {other:?}"),
    };

    // When: the linked parent and its PlanBook are saved and restored together.
    let saved = session.save().expect("linked external plan is saveable");
    assert_eq!(
        plans, saved.plans,
        "successful execution must persist the adopted PlanBook"
    );
    assert_eq!(saved.plans.plan_ids().collect::<Vec<_>>(), vec![PlanId(0)]);
    assert_eq!(
        saved.parent_orders[&AccountId(2)][&code()].linked_plan_id,
        Some(PlanId(0))
    );
    let mut restored = GameSession::restore(&saved).expect("linked external plan restores");
    let mut restored_plans = saved.plans.clone();

    // Then: execution continues through the restored authoritative plan, rather than leaving a
    // dead parent link or resetting the caller's detached PlanBook.
    let continued = restored
        .execute_plan_observation(
            &mut restored_plans,
            request(
                plan_id,
                Side::Sell,
                QuoteAction::Replace {
                    order_id: first_order_id,
                    price: Money::from_cents(1_090),
                    qty: 100,
                },
            ),
        )
        .expect("restored institution plan continues");
    assert!(matches!(
        continued.disposition,
        PlanExecutionDisposition::Replaced {
            canceled_order_id,
            order_id,
            ..
        } if canceled_order_id == first_order_id && order_id != first_order_id
    ));
    assert_eq!(
        restored_plans,
        restored.save().expect("continued save").plans
    );
}

#[test]
fn day_end_releases_child_and_next_observation_reissues_without_automatic_revival() {
    // Given: a player-owned plan has one unfilled real child on a one-tick trading day.
    let mut session = session(setup(0, 1, 0, 0));
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
    let first_id = match first.disposition {
        PlanExecutionDisposition::Submitted { order_id, .. } => order_id,
        other => panic!("expected submission, got {other:?}"),
    };

    // When: the real day-end lifecycle runs, then no plan observation occurs on the next day.
    session.step().expect("healthy step");
    session
        .synchronize_plan_execution(&mut plans)
        .expect("day-end event updates plan link");

    // Then: the old child and freeze are gone, the plan remains, and no order revives by itself.
    assert!(session.save().expect("healthy save").resting_orders[&code()].is_empty());
    assert!(session
        .save()
        .expect("healthy save")
        .parent_orders
        .is_empty());
    assert_eq!(
        plans.plan(plan_id).expect("plan persists").status,
        PlanStatus::Active
    );
    assert_eq!(
        plans
            .plan(plan_id)
            .expect("plan persists")
            .active_child_order_id,
        None
    );
    assert_eq!(
        session
            .step()
            .expect("healthy step")
            .iter()
            .filter(|event| matches!(event, Event::OrderAccepted { .. }))
            .count(),
        0
    );

    // When: that person makes the next explicit observation and confirms a fresh quote.
    let mut next = request(
        plan_id,
        Side::Buy,
        QuoteAction::Submit {
            price: Money::from_cents(900),
            qty: 100,
        },
    );
    next.trading_day = 2;
    let second = session
        .execute_plan_observation(&mut plans, next)
        .expect("next observation reissues");

    // Then: a new order id proves the stale queue priority was not retained.
    assert!(
        matches!(second.disposition, PlanExecutionDisposition::Submitted { order_id, .. } if order_id != first_id)
    );
}

#[test]
fn stale_external_book_can_append_without_reverting_session_lifecycle_facts() {
    // Given: the session has already applied day-end to its authoritative copy while the public
    // adapter still holds the previously returned PlanBook.
    let mut session = session(setup(0, 1, 0, 0));
    let mut plans = PlanBook::default();
    let first_plan = create_plan(&mut plans, AccountId(0), Side::Buy, 400);
    session
        .execute_plan_observation(
            &mut plans,
            request(
                first_plan,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(900),
                    qty: 100,
                },
            ),
        )
        .expect("first child is accepted");
    session.step().expect("healthy day-end step");
    assert!(plans
        .plan(first_plan)
        .expect("external plan remains available")
        .active_child_order_id
        .is_some());

    // When: that stale adapter legitimately appends a distinct plan before synchronizing.
    let appended_plan = create_plan(&mut plans, AccountId(1), Side::Sell, 100);
    session
        .synchronize_plan_execution(&mut plans)
        .expect("session history and appended external plan merge");

    // Then: the append survives, while the stale child link cannot revert authoritative day-end.
    assert!(plans
        .plan(first_plan)
        .expect("first plan persists")
        .active_child_order_id
        .is_none());
    assert_eq!(
        plans
            .plan(appended_plan)
            .expect("appended plan is adopted")
            .account,
        AccountId(1)
    );
    assert_eq!(
        plans,
        session.save().expect("merged book is saveable").plans
    );
}

#[test]
fn closing_fill_that_completes_a_plan_does_not_also_expire_it() {
    let mut session = session(setup(100, 2, 1, 0));
    let mut plans = PlanBook::default();
    let seller = create_plan(&mut plans, AccountId(2), Side::Sell, 100);
    let buyer = create_plan(&mut plans, AccountId(1), Side::Buy, 100);
    session
        .execute_plan_observation(
            &mut plans,
            request(
                seller,
                Side::Sell,
                QuoteAction::Submit {
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            ),
        )
        .expect("seller enters the auction");
    session
        .execute_plan_observation(
            &mut plans,
            request(
                buyer,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            ),
        )
        .expect("buyer enters the auction");

    let mut events = session.step().expect("healthy step");
    events.extend(session.step().expect("healthy step"));
    session
        .synchronize_plan_execution(&mut plans)
        .expect("a terminal fill supersedes the same boundary's day-end release");

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Trade { qty: 100, .. })));
    assert_eq!(
        plans
            .plan(seller)
            .expect("seller plan remains recorded")
            .status,
        PlanStatus::Completed
    );
    assert_eq!(
        plans
            .plan(buyer)
            .expect("buyer plan remains recorded")
            .status,
        PlanStatus::Completed
    );
}
