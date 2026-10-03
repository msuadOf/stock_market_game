use super::*;
use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};
use crate::session::plan_execution::PlanExecutionDisposition;

#[test]
fn adaptive_source_generation_is_continuous_across_distinct_root_accounts() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    let second = session
        .state
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
    let batch = roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap();
    assert_eq!(
        batch
            .iter()
            .map(|candidate| (candidate.owner, candidate.chain_generation_index))
            .collect::<Vec<_>>(),
        vec![(AccountId(1), 0), (AccountId(0), 1)]
    );
    roots
        .resume_adaptive_candidates(
            &mut session,
            [
                (
                    AccountId(1),
                    code.clone(),
                    0,
                    PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
                ),
                (
                    AccountId(0),
                    code,
                    1,
                    PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
                ),
            ],
        )
        .unwrap();
    assert!(roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap()
        .is_empty());
    assert_eq!(roots.finish_adaptive().unwrap().len(), 2);
}

#[test]
fn same_plan_cannot_yield_again_until_its_typed_result_arrives() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    assert_eq!(
        roots
            .yield_adaptive_candidates(&mut session, &mut observation)
            .unwrap()
            .len(),
        1
    );
    assert!(roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap()
        .is_empty());
}

#[test]
fn lifecycle_skips_a_pending_stock_and_observes_an_independent_stock() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        47,
    )
    .unwrap();
    let (_, template) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let mut ids = Vec::new();
    for code in [&first, &second] {
        ids.push(
            session
                .state
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
                .unwrap(),
        );
    }
    session.state.day = 1;
    let mut request = template;
    request.plan_id = ids[0];
    request.allocation.plan_id = ids[0];
    request.trading_day = 1;
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    roots.push_lifecycle(
        owner,
        BTreeMap::from([
            (
                first.clone(),
                CandidateAssessment::InsufficientInformation { excluded: vec![] },
            ),
            (
                second.clone(),
                CandidateAssessment::InsufficientInformation { excluded: vec![] },
            ),
        ]),
        session.build_market_view(),
    );
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();

    let batch = roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap();
    assert_eq!(batch.len(), 1);
    assert!(matches!(batch[0].intent, Intent::PlaceLimit { .. }));
    assert_eq!(
        session
            .state
            .plans
            .plan(ids[0])
            .unwrap()
            .last_event_trading_day(),
        0
    );
    assert_eq!(
        session
            .state
            .plans
            .plan(ids[1])
            .unwrap()
            .last_event_trading_day(),
        1
    );
    roots
        .resume_adaptive_candidates(
            &mut session,
            [(
                owner,
                first,
                batch[0].chain_generation_index,
                PlanRouteOutcome::Rejected(RejectionReason::InsufficientCash),
            )],
        )
        .unwrap();
    assert!(roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap()
        .is_empty());
    assert_eq!(
        session
            .state
            .plans
            .plan(ids[0])
            .unwrap()
            .last_event_trading_day(),
        1
    );
}

#[test]
fn adaptive_source_late_generation_overflow_is_typed_and_emits_no_candidate() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    roots.set_adaptive_generation_for_test(u64::MAX);
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    assert!(matches!(
        roots.yield_adaptive_candidates(&mut session, &mut observation),
        Err(StepFatal::InvariantViolation { .. })
    ));
}

#[test]
fn frozen_plan_observation_reuses_cash_market_and_reservation_after_private_cancel() {
    let (mut session, _) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    let code = StockCode("600888".to_owned());
    session.seed_order_for_test(
        owner,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(901)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let frozen_cash = session.state.accounts[&owner].cash();
    let frozen_reserved = session.reserved_cash_for_account(owner).unwrap();
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    let order_id = session.state.markets[&code].resting_orders()[0].id;
    session.seed_order_for_test(
        owner,
        Intent::Cancel {
            code: code.clone(),
            id: order_id,
        },
        &mut Vec::new(),
    );
    session
        .state
        .accounts
        .get_mut(&owner)
        .unwrap()
        .fixture_set_cash(Money::from_cents(7));
    let observed = observation.observe(&mut session, |session| {
        (
            session.state.accounts[&owner].cash(),
            session.reserved_cash_for_account(owner).unwrap(),
            session.state.markets[&code].resting_order_count(),
        )
    });
    assert_eq!(observed, (frozen_cash, frozen_reserved, 1));
    assert_eq!(session.state.accounts[&owner].cash(), Money::from_cents(7));
    assert_eq!(session.state.markets[&code].resting_order_count(), 0);
}

#[test]
fn adaptive_source_first_failed_conflict_cancel_stops_all_dependent_commands() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    session
        .state
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .fixture_set_kind(AccountKind::Player);
    for price in [901, 902] {
        session.seed_order_for_test(
            AccountId(1),
            Intent::PlaceLimit {
                code: request.allocation.code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(price)),
                qty: 100,
            },
            &mut Vec::new(),
        );
    }
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let candidate = roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap()
        .remove(0);
    assert!(matches!(candidate.intent, Intent::Cancel { .. }));
    roots
        .resume_adaptive_candidates(
            &mut session,
            [(
                AccountId(1),
                code,
                candidate.chain_generation_index,
                PlanRouteOutcome::Rejected(RejectionReason::OrderNotFound),
            )],
        )
        .unwrap();
    assert!(roots
        .yield_adaptive_candidates(&mut session, &mut observation)
        .unwrap()
        .is_empty());
    assert!(matches!(
        roots.finish_adaptive().unwrap()[0].disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::OrderNotFound
        }
    ));
}

#[test]
fn root_coordinator_empty_poll_does_not_take_personal_state() {
    let (mut session, _) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let before = session.business_state_hash().unwrap();
    let mut coordinator = PlanRootCoordinator::empty();
    assert!(coordinator
        .prepare_ready_accounts(&mut session, false)
        .unwrap()
        .is_none());
    assert!(coordinator.is_empty());
    assert_eq!(session.business_state_hash().unwrap(), before);
}

#[test]
fn root_coordinator_disconnected_worker_is_explicit_failure() {
    let (mut session, _) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let (sender, receiver) = std::sync::mpsc::channel();
    drop(sender);
    let mut coordinator = PlanRootCoordinator {
        source: AccountSource::InFlight {
            receiver,
            remaining: 1,
            snapshot: std::sync::Arc::new(RootReadContext::capture(&session).unwrap()),
        },
        completion_sender: None,
    };
    assert!(matches!(
        coordinator.prepare_ready_accounts(&mut session, false),
        Err(StepFatal::InvariantViolation { .. })
    ));
    assert!(coordinator
        .set_completion_sender(std::sync::mpsc::channel().0)
        .is_err());
}

#[test]
fn stock_route_followups_consume_reconsideration_before_retry_without_erasing_it() {
    let (session, _) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let resource = (AccountId(1), StockCode("600888".to_owned()));
    let market = session.build_market_view();
    let mut routes = StockRouteCoordination::default();
    routes.remember_retry(resource.clone(), market.clone());
    routes.remember_reconsideration(
        resource.clone(),
        CandidateAssessment::InsufficientInformation { excluded: vec![] },
        market,
    );
    assert!(matches!(
        routes.take_follow_up(resource.clone()),
        Some(PlanChainOperation::Lifecycle { .. })
    ));
    assert!(matches!(
        routes.take_follow_up(resource.clone()),
        Some(PlanChainOperation::AccountExecution { .. })
    ));
    assert!(routes.take_follow_up(resource).is_none());
}

#[test]
fn stock_route_wrong_generation_does_not_consume_pending_route() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let plans = std::mem::take(&mut session.state.plans);
    let route = match session.prepare_plan_observation(&plans, request).unwrap() {
        PlanExecutionProgress::Route(route) => route,
        _ => panic!("fixture must yield a route"),
    };
    let resource = route_resource(route.command());
    let mut routes = StockRouteCoordination::default();
    routes.install_pending(resource.clone(), 7, route);
    assert!(routes.take_pending(&resource, 8).is_err());
    assert!(routes.pending_for_outcome(&resource, 7).is_ok());
}

#[test]
fn lifecycle_uses_fixed_account_resources_and_current_plan_after_outcome() {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let account = AccountId(1);
    let code = request.allocation.code;
    let market = session.build_market_view();
    let assessments = BTreeMap::from([(
        code,
        CandidateAssessment::InsufficientInformation { excluded: vec![] },
    )]);
    let mut observation = FrozenPlanChainObservation::capture(&session).unwrap();
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(Money::ZERO);
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_clear_positions();
    assert!(session
        .collect_plan_lifecycle_actions(account, &assessments, &market, &session.state.plans)
        .is_empty());
    let actions = observation.observe(&mut session, |candidate| {
        candidate.collect_plan_lifecycle_actions(
            account,
            &assessments,
            &market,
            &candidate.state.plans,
        )
    });
    assert!(
        matches!(actions.as_slice(), [PlanLifecycleAction::Observe { plan_id, .. }] if *plan_id == request.plan_id)
    );
    session
        .state
        .plans
        .apply(
            request.plan_id,
            PlanEvent::Terminated {
                reason: crate::plans::TerminationReason::Cancelled,
                trading_day: 0,
            },
        )
        .unwrap();
    assert!(observation
        .observe(&mut session, |candidate| {
            candidate.collect_plan_lifecycle_actions(
                account,
                &assessments,
                &market,
                &candidate.state.plans,
            )
        })
        .is_empty());
    assert_eq!(session.state.accounts[&account].cash(), Money::ZERO);
    assert!(session.state.accounts[&account].positions().is_empty());
}
