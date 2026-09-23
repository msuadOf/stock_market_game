use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{AllocationGrant, PlanOpen, PlanOpinion, PlanTarget, Urgency};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;

pub(super) fn execution_fixture() -> (GameSession, PlanExecutionRequest) {
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 47).unwrap();
    let code = StockCode("600888".to_owned());
    let plan_id = session
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    (
        session,
        PlanExecutionRequest {
            plan_id,
            allocation: AllocationGrant {
                plan_id,
                code,
                allocated_cash: Money::from_cents(1_000_000),
                constraint: None,
            },
            decision: QuoteDecision {
                action: QuoteAction::Submit {
                    price: Money::from_cents(900),
                    qty: 100,
                },
                reason: QuoteReason::OppositeQuoteProbe,
            },
            trading_day: 0,
        },
    )
}

#[test]
fn failed_replace_cancel_never_submits_or_changes_existing_child() {
    let (mut session, mut request) = execution_fixture();
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request.clone());
    session.consume_plan_chain_operation_batch(batch, &mut Vec::new());
    let before = serde_json::to_value(&session.parent_orders).unwrap();
    let next_order_id = session.next_order_id;
    let reservation = session.reserved_cash_for_account(AccountId(1)).unwrap();
    request.decision.action = QuoteAction::Replace {
        order_id: OrderId(999_999),
        price: Money::from_cents(901),
        qty: 100,
    };
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request);
    let mut events = Vec::new();

    session.consume_plan_chain_operation_batch(batch, &mut events);

    assert!(matches!(
        events.as_slice(),
        [Event::IntentRejected {
            reason: RejectionReason::OrderNotFound,
            ..
        }]
    ));
    assert_eq!(session.next_order_id, next_order_id);
    assert_eq!(
        serde_json::to_value(&session.parent_orders).unwrap(),
        before
    );
    assert_eq!(
        session.reserved_cash_for_account(AccountId(1)).unwrap(),
        reservation
    );
}

#[test]
fn replacement_success_consumes_cancel_before_submit() {
    let (mut session, mut request) = execution_fixture();
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request.clone());
    session.consume_plan_chain_operation_batch(batch, &mut Vec::new());
    let old_id = session
        .plans
        .plan(request.plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    let expected_id = OrderId(session.next_order_id);
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request);
    let mut events = Vec::new();

    session.consume_plan_chain_operation_batch(batch, &mut events);

    assert!(
        matches!(events.as_slice(), [Event::OrderCanceled { id: canceled, .. }, Event::OrderAccepted { id: submitted, .. }] if *canceled == old_id && *submitted == expected_id)
    );
}

#[test]
fn exact_adoption_syncs_without_allocating_a_new_order_identity() {
    let (mut session, request) = execution_fixture();
    let code = request.allocation.code.clone();
    let mut events = Vec::new();
    session.route_intent(
        AccountId(1),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut events,
    );
    let existing_id = match events.as_slice() {
        [Event::OrderAccepted { id, .. }] => *id,
        _ => panic!("fixture order missing"),
    };
    let identities = (session.next_order_id, session.seq);
    let plan_id = request.plan_id;
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request);
    events.clear();

    session.consume_plan_chain_operation_batch(batch, &mut events);

    assert!(events.is_empty());
    assert_eq!((session.next_order_id, session.seq), identities);
    assert_eq!(
        session.plans.plan(plan_id).unwrap().active_child_order_id,
        Some(existing_id)
    );
    assert!(session.pending_plan_events.is_empty());
    assert_eq!(
        session.parent_orders[&AccountId(1)][&code].active_child_order_id,
        Some(existing_id)
    );
}

#[test]
fn completed_consumption_round_trips_plan_parent_and_order_state() {
    let (mut session, request) = execution_fixture();
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request);
    session.consume_plan_chain_operation_batch(batch, &mut Vec::new());
    let saved = session.save().unwrap();

    let restored = GameSession::restore(&saved).unwrap();

    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
}

#[test]
fn empty_plan_chain_operation_batch_leaves_identity_and_books_unchanged() {
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 43).unwrap();
    let next_order_id = session.next_order_id;
    let seq = session.seq;
    let mut events = Vec::new();

    session.consume_plan_chain_operation_batch(PlanChainOperationBatch::empty(), &mut events);

    assert!(events.is_empty());
    assert_eq!(session.next_order_id, next_order_id);
    assert_eq!(session.seq, seq);
}

#[test]
fn operation_batch_submission_preserves_parent_link_and_pending_acceptance_event() {
    let code = StockCode("600888".to_owned());
    let owner = AccountId(1);
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 47).unwrap();
    let plan_id = session
        .plans
        .create(PlanOpen {
            account: owner,
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    let request = PlanExecutionRequest {
        plan_id,
        allocation: AllocationGrant {
            plan_id,
            code: code.clone(),
            allocated_cash: Money::from_cents(1_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action: QuoteAction::Submit {
                price: Money::from_cents(900),
                qty: 100,
            },
            reason: QuoteReason::OppositeQuoteProbe,
        },
        trading_day: 0,
    };
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request);
    let mut events = Vec::new();

    session.consume_plan_chain_operation_batch(batch, &mut events);

    let order_id = match events.as_slice() {
        [Event::OrderAccepted { id, .. }] => *id,
        other => panic!("expected one accepted order, got {other:?}"),
    };
    assert_eq!(
        session.parent_orders[&owner][&code].active_child_order_id,
        Some(order_id)
    );
    assert_eq!(
        session.plans.plan(plan_id).unwrap().active_child_order_id,
        Some(order_id)
    );
    assert!(session.pending_plan_events.is_empty());
}
