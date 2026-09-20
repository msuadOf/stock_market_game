use super::*;
use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};
use crate::session::plan_execution::PlanExecutionDisposition;

#[test]
fn adaptive_source_generation_is_continuous_across_distinct_root_accounts() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let second = session
        .plans
        .create(PlanOpen {
            account: AccountId(0),
            code: request.allocation.code.clone(),
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
    let mut second_request = request.clone();
    second_request.plan_id = second;
    second_request.allocation.plan_id = second;
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    roots.push_execution(second_request);
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    let first = roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .unwrap()
        .unwrap();
    assert_eq!(
        (first.owner, first.chain_generation_index),
        (AccountId(1), 0)
    );
    roots
        .resume_adaptive_candidate(
            &mut session,
            PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
        )
        .unwrap();
    let second = roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .unwrap()
        .unwrap();
    assert_eq!(
        (second.owner, second.chain_generation_index),
        (AccountId(0), 1)
    );
    roots
        .resume_adaptive_candidate(
            &mut session,
            PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
        )
        .unwrap();
    assert!(roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .unwrap()
        .is_none());
    assert_eq!(roots.finish_adaptive().unwrap().len(), 2);
}

#[test]
fn adaptive_source_refuses_a_second_yield_until_the_result_arrives() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .unwrap()
        .unwrap();
    assert!(roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .is_err());
}

#[test]
fn adaptive_source_late_generation_overflow_is_typed_and_emits_no_candidate() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    roots.set_adaptive_generation_for_test(u64::MAX);
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    assert!(matches!(
        roots.yield_adaptive_candidate(&mut session, &mut observation),
        Err(StepFatal::InvariantViolation { .. })
    ));
}

#[test]
fn frozen_plan_observation_reuses_cash_market_and_reservation_after_private_cancel() {
    let (mut session, _) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    let code = StockCode("600888".to_owned());
    session.route_intent(
        owner,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(901),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let frozen_cash = session.accounts[&owner].cash;
    let frozen_reserved = session.reserved_cash_for_account(owner).unwrap();
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    let order_id = session.markets[&code].resting_orders()[0].id;
    session.route_intent(
        owner,
        Intent::Cancel {
            code: code.clone(),
            id: order_id,
        },
        &mut Vec::new(),
    );
    session.accounts.get_mut(&owner).unwrap().cash = Money::from_cents(7);
    let observed = observation.observe(&mut session, |session| {
        (
            session.accounts[&owner].cash,
            session.reserved_cash_for_account(owner).unwrap(),
            session.markets[&code].resting_order_count(),
        )
    });
    assert_eq!(observed, (frozen_cash, frozen_reserved, 1));
    assert_eq!(session.accounts[&owner].cash, Money::from_cents(7));
    assert_eq!(session.markets[&code].resting_order_count(), 0);
}

#[test]
fn adaptive_source_first_failed_conflict_cancel_stops_all_dependent_commands() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    session.accounts.get_mut(&AccountId(1)).unwrap().kind = AccountKind::Player;
    for price in [901, 902] {
        session.route_intent(
            AccountId(1),
            Intent::PlaceLimit {
                code: request.allocation.code.clone(),
                side: Side::Buy,
                price: Money::from_cents(price),
                qty: 100,
            },
            &mut Vec::new(),
        );
    }
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let candidate = roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .unwrap()
        .unwrap();
    assert!(matches!(candidate.intent, Intent::Cancel { .. }));
    roots
        .resume_adaptive_candidate(
            &mut session,
            PlanRouteOutcome::Rejected(RejectionReason::OrderNotFound),
        )
        .unwrap();
    assert!(roots
        .yield_adaptive_candidate(&mut session, &mut observation)
        .unwrap()
        .is_none());
    assert!(matches!(
        roots.finish_adaptive().unwrap()[0].disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::OrderNotFound
        }
    ));
}
