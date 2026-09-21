use super::adaptive_plan_chain::AdaptivePlanChainCoordinator;
use super::b2_auction_transaction::{
    apply_open_order_feedback, apply_tick_shadow_b2_auction_transaction_with_roots_for_test,
    prepare_b2_auction_tick,
};
use super::p3_context::build_p3_validation_context;
use super::stock_auction::b2_auction_day_end::{
    IncrementalAuctionStockCoordinator, apply_incremental_auction_finish,
    finish_incremental_auction_coordinator,
};
use super::stock_auction_adapter::prepare_incremental_auction_inputs;
use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{
    AllocationGrant, OpinionSource, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, Urgency,
};
use crate::session::pipeline::DecisionResourceSnapshot;
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::{
    ParentOrderPlan, PendingPlanEvent, PlanExecutionDisposition, PlanExecutionRequest,
    RuntimeResource, MAX_SAVED_PLAN_EVENTS,
};
use crate::{AccountId, Event, GameSession, Intent, Money, OrderId, PlanId, RejectionReason, Side};

fn fixture() -> (GameSession, PlanExecutionRequest) {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    session.setup.auction_ticks = 900;
    session.setup.ticks_per_day = 15_300;
    session.tick = 0;
    assert_eq!(session.phase(), crate::TradingPhase::CallAuction);
    (session, request)
}

fn player_only_auction_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.inst_count = 0;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn b2_prepared_closing_day_end_reports_pending_plan_capacity_without_aborting() {
    let mut authority =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    authority.setup.closing_auction_ticks = 10;
    authority.tick = authority.setup.ticks_per_day - 1;
    assert_eq!(authority.phase(), crate::TradingPhase::ClosingAuction);
    let institution = AccountId(1);
    let code = authority.markets.keys().next().unwrap().clone();
    authority
        .parent_orders
        .entry(institution)
        .or_default()
        .insert(
            code.clone(),
            ParentOrderPlan {
                code,
                side: Side::Buy,
                target_qty: 100,
                filled_qty: 0,
                child_qty: 100,
                active_child_order_id: None,
                active_child_remaining_qty: None,
                linked_plan_id: Some(PlanId(700)),
                limit_price: Money::from_cents(1_000),
                expires_market_minute: 480,
            },
        );
    authority.pending_plan_events = vec![
        PendingPlanEvent::DayEnded {
            plan_id: PlanId(999),
            trading_day: 0,
        };
        MAX_SAVED_PLAN_EVENTS
    ];
    authority.pending_player.push((
        institution,
        Intent::PlaceLimit {
            code: authority.markets.keys().next().unwrap().clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    ));

    let committed = prepare_b2_auction_tick(&mut authority)
        .expect("DayEnd capacity is a business resource limit")
        .commit();

    assert_eq!(authority.day(), 1);
    assert_eq!(authority.pending_plan_events.len(), MAX_SAVED_PLAN_EVENTS);
    assert!(matches!(
        committed.output.validation.results(),
        [P3CandidateResult::PendingPlanEventsLimited { .. }]
    ));
    assert_eq!(
        committed
            .commit
            .tick
            .events
            .iter()
            .filter(|event| matches!(
                event,
                Event::ResourceLimit {
                    resource: RuntimeResource::PendingPlanEvents,
                    limit,
                    ..
                } if *limit == MAX_SAVED_PLAN_EVENTS as u32
            ))
            .count(),
        1
    );
}

fn opening_completion_fixture(
    target_qty: u32,
    seller_qty: u32,
) -> (GameSession, PlanExecutionRequest) {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(900),
        47,
    )
    .unwrap();
    session.tick = 599;
    let code = request_code(&session);
    let plan_id = session
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code: code.clone(),
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
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), seller_qty, Money::from_cents(900))
        .unwrap();
    session.auction_orders.insert(
        code.clone(),
        vec![crate::AuctionOrderSnap {
            owner: AccountId(0),
            side: Side::Sell,
            limit: Money::from_cents(900),
            qty: seller_qty,
            arrival_seq: 30,
        }],
    );
    session.auction_order_counts.insert(AccountId(0), 1);
    session.next_order_id = 31;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    (
        session,
        PlanExecutionRequest {
            plan_id,
            allocation: AllocationGrant {
                plan_id,
                code,
                allocated_cash: Money::from_cents(10_000_000),
                constraint: None,
            },
            decision: QuoteDecision {
                action: QuoteAction::Submit {
                    price: Money::from_cents(1_100),
                    qty: target_qty,
                },
                reason: QuoteReason::OppositeQuoteProbe,
            },
            trading_day: 0,
        },
    )
}

fn commit_with_roots(authority: &mut GameSession, request: PlanExecutionRequest) {
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let guard = super::p9_candidate_commit::P8AuthorityGuard::capture(authority).unwrap();
    let mut plan = plan_tick(PhaseInput { session: authority }).unwrap();
    apply_tick_shadow_b2_auction_transaction_with_roots_for_test(&mut plan, roots).unwrap();
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(authority, plan, guard)
        .unwrap()
        .commit();
}

fn seal(session: &mut GameSession) -> (P3ValidatorDriver, IncrementalAuctionStockCoordinator) {
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let allocation = session.seal_allocation_snapshot().unwrap();
    let resources = DecisionResourceSnapshot::seal(session, allocation).unwrap();
    let p3 = P3ValidatorDriver::new(
        resources,
        session.envelope_ledger.clone(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(session).unwrap(),
    )
    .unwrap();
    let p4 = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(session).unwrap(),
    )
    .unwrap();
    (p3, p4)
}

fn coordinator(
    session: &GameSession,
    request: PlanExecutionRequest,
) -> AdaptivePlanChainCoordinator {
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    AdaptivePlanChainCoordinator::capture_batch(session, roots).unwrap()
}

fn execute(
    session: &mut GameSession,
    chain: &mut AdaptivePlanChainCoordinator,
    p3: &mut P3ValidatorDriver,
    p4: &mut IncrementalAuctionStockCoordinator,
) -> Option<(
    P2Candidate,
    P3ConsumeOutcome,
    Option<super::stock_auction::b2_auction_day_end::AuctionExecutionRound>,
)> {
    let candidate = chain.next_candidate(session).unwrap()?;
    let outcome = p3.consume(candidate.clone()).unwrap();
    let round = outcome
        .operation()
        .map(|operation| p4.apply_round(vec![operation.clone()]).unwrap());
    if let Some(round) = &round {
        apply_open_order_feedback(p3, std::slice::from_ref(&outcome), round).unwrap();
    }
    chain
        .advance_after_auction_outcome(session, &outcome, round.as_ref())
        .unwrap();
    Some((candidate, outcome, round))
}

fn install_original_child(session: &mut GameSession, request: &PlanExecutionRequest) -> OrderId {
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request.clone());
    let mut events = Vec::new();
    session.consume_plan_chain_operation_batch(batch, &mut events);
    match events.as_slice() {
        [Event::OrderAccepted { id, .. }] => *id,
        other => panic!("expected one accepted auction child, got {other:?}"),
    }
}

#[test]
fn incremental_auction_replace_cancels_old_then_places_new_and_finalizes_once() {
    let (mut session, mut request) = fixture();
    let old_id = install_original_child(&mut session, &request);
    session.envelope_ledger.rebase_live_for_next_tick().unwrap();
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    let mut candidates = Vec::new();
    while let Some((candidate, _, _)) = execute(&mut session, &mut chain, &mut p3, &mut p4) {
        candidates.push(candidate);
    }

    assert_eq!(candidates.len(), 2);
    assert!(matches!(candidates[0].intent(), Intent::Cancel { id, .. } if *id == old_id));
    assert!(matches!(
        candidates[1].intent(),
        Intent::PlaceLimit { price, .. } if *price == Money::from_cents(901)
    ));
    let mut completion = chain.finish().unwrap();
    assert!(matches!(
        completion.reports[0].disposition,
        PlanExecutionDisposition::Replaced { canceled_order_id, .. } if canceled_order_id == old_id
    ));
    let validation = p3.finish();
    let candidates = P2CandidateBatch::from_canonical(candidates).unwrap();
    let finish = finish_incremental_auction_coordinator(&session, p4).unwrap();
    assert!(finish.workers.values().all(|worker| {
        worker.finalizer.auction_tail_passes == 1
            && worker.finalizer.auction_completion_passes == 0
            && worker.finalizer.day_end_passes == 0
    }));
    let mut session_index = 0;
    let preceding = completion.take_event_facts(&mut session_index).unwrap();
    let output = apply_incremental_auction_finish(
        &mut session,
        &candidates,
        &validation,
        finish,
        preceding,
        &mut session_index,
        &completion.consumed,
    )
    .unwrap();

    assert_eq!(
        output
            .events
            .iter()
            .filter(|event| matches!(event, Event::AuctionTick { .. }))
            .count(),
        1
    );
    let active = session
        .plans
        .plan(request.plan_id)
        .unwrap()
        .active_child_order_id;
    assert!(active.is_some_and(|order_id| order_id != old_id));
    assert_eq!(session.auction_orders[&request.allocation.code].len(), 1);
    assert_eq!(
        session.auction_orders[&request.allocation.code][0].arrival_seq,
        active.unwrap().0
    );
    assert!(
        session.pending_plan_events.is_empty(),
        "continuation-consumed cancel/accept facts must not be projected again in P7"
    );
    let plan_account = session.plans.plan(request.plan_id).unwrap().account;
    assert_eq!(
        session.parent_orders[&plan_account][&request.allocation.code].active_child_order_id,
        active
    );
}

#[test]
fn auction_cancel_rejection_stops_replace_before_illegal_successor() {
    let (mut session, mut request) = fixture();
    let old_id = install_original_child(&mut session, &request);
    session.envelope_ledger.rebase_live_for_next_tick().unwrap();
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let next_order_id = session.next_order_id;
    let mut chain = coordinator(&session, request);
    session.auction_orders.clear();
    session.auction_order_counts.clear();
    session.envelope_ledger = EnvelopeLedger::new(session.next_receipt_base, Vec::new()).unwrap();
    let (mut p3, mut p4) = seal(&mut session);

    let (candidate, _, round) = execute(&mut session, &mut chain, &mut p3, &mut p4).unwrap();
    assert!(matches!(candidate.intent(), Intent::Cancel { id, .. } if *id == old_id));
    assert!(matches!(
        &round.unwrap().facts[0].outcome,
        super::stock_auction::b2_auction_day_end::AuctionLifecycleFact::Rejected {
            reason: RejectionReason::OrderNotFound,
            ..
        }
    ));
    assert!(chain.next_candidate(&mut session).unwrap().is_none());
    assert_eq!(p3.output().drafts().len(), 0);
    assert_eq!(p3.output().next_order_id_after(), next_order_id);
    assert!(matches!(
        chain.finish().unwrap().reports[0].disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::OrderNotFound
        }
    ));
}

#[test]
fn later_auction_round_typed_failure_discards_private_tick_progress() {
    let (mut authority, mut request) = fixture();
    let old_id = install_original_child(&mut authority, &request);
    authority
        .envelope_ledger
        .rebase_live_for_next_tick()
        .unwrap();
    authority.next_order_id = crate::orderbook::js_safe_u64::MAX;
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let before = authority.business_state_hash().unwrap();
    let mut candidate = authority.clone_for_tick_shadow().unwrap();
    let (mut p3, mut p4) = seal(&mut candidate);
    let mut chain = coordinator(&candidate, request);

    execute(&mut candidate, &mut chain, &mut p3, &mut p4).unwrap();
    let place = chain.next_candidate(&mut candidate).unwrap().unwrap();
    assert!(matches!(place.intent(), Intent::PlaceLimit { .. }));
    let outcome = p3.consume(place).unwrap();
    let error = p4
        .apply_round(vec![outcome.operation().unwrap().clone()])
        .unwrap_err();

    assert!(matches!(error, StepFatal::InvariantViolation { .. }));
    assert_eq!(authority.business_state_hash().unwrap(), before);
    assert_eq!(authority.auction_orders[&request_code(&authority)].len(), 1);
    assert!(candidate.auction_orders.is_empty());
}

#[test]
fn parent_transaction_seam_rolls_back_every_authoritative_family_on_later_round_failure() {
    let (mut authority, mut request) = fixture();
    let old_id = install_original_child(&mut authority, &request);
    authority
        .envelope_ledger
        .rebase_live_for_next_tick()
        .unwrap();
    authority.next_order_id = crate::orderbook::js_safe_u64::MAX;
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();
    let next_order_before = authority.next_order_id;
    let queue_before = serde_json::to_value(&authority.auction_orders).unwrap();
    let ledger_before = serde_json::to_value(&authority.envelope_ledger).unwrap();
    let plans_before = serde_json::to_value(&authority.plans).unwrap();
    let parent_before = serde_json::to_value(&authority.parent_orders).unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let result = apply_tick_shadow_b2_auction_transaction_with_roots_for_test(&mut plan, roots);

    assert!(matches!(
        result,
        Err(
            super::b2_auction_transaction::B2AuctionTransactionError::Preparation(
                StepFatal::InvariantViolation { .. }
            )
        )
    ));
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
    assert_eq!(authority.next_order_id, next_order_before);
    assert_eq!(
        serde_json::to_value(&authority.auction_orders).unwrap(),
        queue_before
    );
    assert_eq!(
        serde_json::to_value(&authority.envelope_ledger).unwrap(),
        ledger_before
    );
    assert_eq!(
        serde_json::to_value(&authority.plans).unwrap(),
        plans_before
    );
    assert_eq!(
        serde_json::to_value(&authority.parent_orders).unwrap(),
        parent_before
    );
}

#[test]
fn prepared_parent_auction_seam_commits_one_complete_tick() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();

    let committed = prepare_b2_auction_tick(&mut authority).unwrap().commit();

    assert_eq!(
        committed.output.candidates.candidates()[0].key(),
        &P2CandidateKey::player(0)
    );
    assert_eq!(
        committed
            .output
            .validation
            .accepted()
            .cloned()
            .collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert!(committed.output.plan_reports.is_empty());
    assert_eq!(committed.output.auction.finalizer.auction_tail_passes, 1);
    assert_eq!(
        committed
            .commit
            .tick
            .events
            .iter()
            .filter(|event| matches!(event, Event::AuctionTick { .. }))
            .count(),
        1
    );
    assert!(authority.pending_player.is_empty());
    assert_eq!(authority.auction_orders[&code].len(), 1);
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.commit.receipt.business_hash()
    );
}

#[test]
fn parent_transaction_applies_opening_completion_full_fill_once() {
    let (mut authority, request) = opening_completion_fixture(100, 100);
    let plan_id = request.plan_id;

    commit_with_roots(&mut authority, request);

    let plan = authority.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty, 100);
    assert_eq!(plan.status, PlanStatus::Completed);
    assert!(authority.pending_plan_events.is_empty());
    assert!(authority.parent_orders.is_empty());
}

#[test]
fn parent_transaction_applies_opening_completion_partial_fill_once() {
    let (mut authority, request) = opening_completion_fixture(200, 100);
    let plan_id = request.plan_id;
    let code = request.allocation.code.clone();

    commit_with_roots(&mut authority, request);

    let plan = authority.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty, 100);
    assert_eq!(plan.status, PlanStatus::Active);
    let child_id = plan
        .active_child_order_id
        .expect("partial child remains active");
    let parent = &authority.parent_orders[&AccountId(1)][&code];
    assert_eq!(parent.filled_qty, 100);
    assert_eq!(parent.active_child_order_id, Some(child_id));
    assert_eq!(parent.active_child_remaining_qty, Some(100));
    assert!(authority.pending_plan_events.is_empty());
}

fn request_code(session: &GameSession) -> crate::StockCode {
    session.markets.keys().next().unwrap().clone()
}
