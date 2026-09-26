use super::adaptive_plan_chain::AdaptivePlanChainCoordinator;
use super::b2_auction_transaction::{
    apply_tick_shadow_b2_auction_transaction_with_roots_for_test, prepare_b2_auction_tick,
    validate_execution_round,
};
use super::p3_context::build_p3_validation_context;
use super::stock_auction::b2_auction_day_end::{
    apply_incremental_auction_finish, finish_incremental_auction_coordinator,
    IncrementalAuctionStockCoordinator,
};
use super::stock_auction_adapter::prepare_incremental_auction_inputs;
use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{
    AllocationGrant, OpinionSource, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, Urgency,
};
use crate::session::pipeline::DecisionResourceSnapshot;
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::{PlanExecutionDisposition, PlanExecutionRequest};
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountId, Event, GameSession, Intent, Money, OrderId, RejectionReason, Side};

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
fn real_retail_auction_review_cancels_old_quote_before_accepting_new_quote() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let mut authority = GameSession::new(setup, 41).unwrap();
    let retail = AccountId(1);
    let code = authority.setup.stocks[0].code.clone();
    authority
        .accounts
        .get_mut(&retail)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5, 1).unwrap()));
    let mut setup_events = Vec::new();
    authority.seed_auction_order_for_test(
        retail,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut setup_events,
    );
    let old_id = setup_events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture old auction quote must be accepted");
    authority.hydrate_or_validate_envelope_ledger().unwrap();
    let tick = authority.tick;
    crate::session::npc_working_quote_tests::force_attention_candidate(
        &mut authority,
        retail,
        tick,
    );
    authority.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(&mut authority).unwrap();

    let committed = prepare_b2_auction_tick(&mut authority)
        .expect("real retail auction review must complete")
        .commit();
    let cancel_seq = committed
        .commit
        .tick
        .events
        .iter()
        .find_map(|event| match event {
            Event::OrderCanceled {
                seq, account, id, ..
            } if *account == retail && *id == old_id => Some(*seq),
            _ => None,
        })
        .expect("review must cancel the old quote");
    let (accept_seq, new_id, new_price) = committed
        .commit
        .tick
        .events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                seq,
                account,
                id,
                price,
                ..
            } if *account == retail && *id != old_id => Some((*seq, *id, *price)),
            _ => None,
        })
        .expect("review must accept one new quote");

    assert_eq!(accept_seq, cancel_seq + 1);
    assert_ne!(new_price, Money::from_cents(900));
    let own_orders = authority.auction_orders[&code]
        .iter()
        .filter(|order| order.owner == retail)
        .collect::<Vec<_>>();
    assert!(
        matches!(own_orders.as_slice(), [order] if order.order_id == new_id.0 && order.limit == new_price)
    );
}

#[test]
fn production_b2_market_rejection_consumes_id_and_does_not_refund_p3_budget() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    let protective_price = authority.markets[&code].up_stop().unwrap();
    let one_order_cash = crate::session::buy_order_reservation(
        &authority.setup.config,
        protective_price,
        100,
        Money::ZERO,
    )
    .unwrap();
    authority.accounts.get_mut(&AccountId(0)).unwrap().cash = one_order_cash;
    for _ in 0..2 {
        authority
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceMarket {
                    code: code.clone(),
                    side: Side::Buy,
                    qty: 100,
                },
            )
            .unwrap();
    }
    let rejected_id = OrderId(authority.next_order_id);

    let committed = prepare_b2_auction_tick(&mut authority)
        .expect("market rejection is an ordinary P4 outcome")
        .commit();

    assert!(matches!(
        committed.output.validation.results(),
        [
            P3CandidateResult::Accepted { .. },
            P3CandidateResult::Rejected {
                reason: RejectionReason::InsufficientCash,
                ..
            }
        ]
    ));
    assert_eq!(authority.next_order_id, rejected_id.0 + 1);
    assert_eq!(committed.output.auction.receipts.len(), 1);
    assert_eq!(
        committed.output.auction.receipts[0].kind,
        ReceiptKind::Reject
    );
    assert_eq!(
        committed.output.auction.receipts[0].envelope.order,
        rejected_id
    );
    assert!(committed.commit.tick.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::AuctionLimitOrderRequired,
            ..
        }
    )));
    assert!(committed.commit.tick.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));
    assert_eq!(authority.accounts[&AccountId(0)].cash, one_order_cash);
    assert_eq!(
        authority.snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO
    );
}

#[test]
fn production_b2_market_rejection_consumes_the_shared_p3_budget_before_a_later_limit() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    let protective_price = authority.markets[&code].up_stop().unwrap();
    let one_order_cash = crate::session::buy_order_reservation(
        &authority.setup.config,
        protective_price,
        100,
        Money::ZERO,
    )
    .unwrap();
    authority.accounts.get_mut(&AccountId(0)).unwrap().cash = one_order_cash;
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        )
        .unwrap();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: protective_price,
                qty: 100,
            },
        )
        .unwrap();
    let rejected_market_id = OrderId(authority.next_order_id);

    let committed = prepare_b2_auction_tick(&mut authority)
        .expect("auction market rejection is an ordinary P4 outcome")
        .commit();

    // P3 accepts the market request using its protective-price budget. P4 then rejects it
    // because an A-share call auction accepts limit orders only. That P4 rejection consumes the
    // preallocated ID, and neither its ID nor its P3 budget is refunded to the later limit.
    assert!(matches!(
        committed.output.validation.results(),
        [
            P3CandidateResult::Accepted { .. },
            P3CandidateResult::Rejected {
                reason: RejectionReason::InsufficientCash,
                ..
            }
        ]
    ));
    assert_eq!(authority.next_order_id, rejected_market_id.0 + 1);
    assert_eq!(committed.output.auction.receipts.len(), 1);
    assert_eq!(
        committed.output.auction.receipts[0].envelope.order,
        rejected_market_id
    );
    assert!(committed.commit.tick.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::AuctionLimitOrderRequired,
            ..
        }
    )));
    assert!(committed.commit.tick.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));
}

#[test]
fn production_b2_market_validation_rejects_quantity_and_shares_before_allocating_id() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    for intent in [
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 1,
        },
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Sell,
            qty: 100,
        },
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 100,
        },
    ] {
        authority
            .enqueue_player_intent(AccountId(0), intent)
            .unwrap();
    }
    let rejected_id = OrderId(authority.next_order_id);

    let committed = prepare_b2_auction_tick(&mut authority)
        .expect("P3 business rejections and P4 market rejection commit atomically")
        .commit();

    let results = committed.output.validation.results();
    assert_eq!(results.len(), 3);
    let result_for = |index| {
        results
            .iter()
            .find(|result| result.key() == &P2CandidateKey::player(index))
            .unwrap()
    };
    assert!(matches!(
        result_for(0),
        P3CandidateResult::Rejected {
            reason: RejectionReason::InvalidQuantity,
            ..
        }
    ));
    assert!(matches!(
        result_for(1),
        P3CandidateResult::Rejected {
            reason: RejectionReason::InsufficientShares,
            ..
        }
    ));
    assert!(matches!(result_for(2), P3CandidateResult::Accepted { .. }));
    // Cash orders retain receipt order; the sell request uses an independent
    // shares budget and can appear before or after either buy.
    let result_position = |index| {
        results
            .iter()
            .position(|result| result.key() == &P2CandidateKey::player(index))
            .unwrap()
    };
    assert!(result_position(0) < result_position(2));
    assert!(result_for(0).sealed_index() < result_for(2).sealed_index());
    let identities = committed.output.validation.identities().collect::<Vec<_>>();
    assert_eq!(identities.len(), 3);
    for (index, expected_id) in [(0, None), (1, None), (2, Some(rejected_id))] {
        let identity = identities
            .iter()
            .find(|(key, _, _)| *key == &P2CandidateKey::player(index))
            .unwrap();
        assert_eq!(identity.2, expected_id);
    }
    assert_eq!(authority.next_order_id, rejected_id.0 + 1);
    assert_eq!(committed.output.auction.receipts.len(), 1);
    assert_eq!(
        committed.output.auction.receipts[0].kind,
        ReceiptKind::Reject
    );
    assert_eq!(
        committed.output.auction.receipts[0].envelope.order,
        rejected_id
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
            order_id: 30,
        }],
    );
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
    authority.pending_npc = None;
    super::npc_p2_preparation::queue_npc_for_next_tick(authority).unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut plan = plan_tick(PhaseInput { session: authority }).unwrap();
    apply_tick_shadow_b2_auction_transaction_with_roots_for_test(&mut plan, roots).unwrap();
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(authority, plan)
        .unwrap()
        .commit();
}

fn seal(session: &mut GameSession) -> (P3ValidatorDriver, IncrementalAuctionStockCoordinator) {
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let resources = DecisionResourceSnapshot::seal(session).unwrap();
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
    let mut batch = chain.next_ready_batch(session).unwrap();
    if batch.is_empty() {
        return None;
    }
    assert_eq!(batch.len(), 1);
    let candidate = batch.remove(0);
    let outcome = p3.consume(candidate.clone()).unwrap();
    let round = outcome
        .operation()
        .map(|operation| p4.apply_round(vec![operation.clone()]).unwrap());
    if let Some(round) = &round {
        validate_execution_round(std::slice::from_ref(&outcome), round).unwrap();
    }
    chain
        .advance_after_auction_outcomes(session, std::slice::from_ref(&outcome), round.as_ref())
        .unwrap();
    Some((candidate, outcome, round))
}

#[test]
fn auction_round_validation_accepts_cross_stock_fact_reordering() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let mut session = GameSession::new(setup, 42).unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let (mut p3, mut p4) = seal(&mut session);
    let candidates = codes
        .iter()
        .enumerate()
        .map(|(index, code)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                AccountId(0),
                Intent::Cancel {
                    code: code.clone(),
                    id: OrderId(77),
                },
            )
        })
        .collect::<Vec<_>>();
    let outcomes = p3.consume_round(candidates).unwrap();
    let operations = outcomes
        .iter()
        .filter_map(|outcome| outcome.operation().cloned())
        .collect::<Vec<_>>();
    assert_eq!(operations.len(), 2);
    let mut round = p4.apply_round(operations).unwrap();
    round.facts.reverse();
    validate_execution_round(&outcomes, &round).unwrap();
}

#[test]
fn auction_plan_chain_accepts_cross_stock_fact_reordering() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let mut session = GameSession::new(setup, 47).unwrap();
    let (_, template) = fixture();
    let mut roots = PlanChainOperationBatch::empty();
    for (account, code) in [
        (AccountId(1), crate::StockCode("600888".to_owned())),
        (AccountId(0), crate::StockCode("600889".to_owned())),
    ] {
        let plan_id = session
            .plans
            .create(PlanOpen {
                account,
                code: code.clone(),
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
        let mut request = template.clone();
        request.plan_id = plan_id;
        request.allocation.plan_id = plan_id;
        request.allocation.code = code;
        roots.push_execution(request);
    }
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = AdaptivePlanChainCoordinator::capture_batch(&session, roots).unwrap();
    let batch = chain.next_ready_batch(&mut session).unwrap();
    assert_eq!(batch.len(), 2);
    let outcomes = p3.consume_round(batch).unwrap();
    let operations = outcomes
        .iter()
        .filter_map(|outcome| outcome.operation().cloned())
        .collect();
    let mut round = p4.apply_round(operations).unwrap();
    assert_eq!(round.facts.len(), 2);
    round.facts.reverse();
    validate_execution_round(&outcomes, &round).unwrap();
    chain
        .advance_after_auction_outcomes(&mut session, &outcomes, Some(&round))
        .unwrap();
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    assert_eq!(chain.finish().unwrap().reports.len(), 2);
}

#[test]
fn auction_plan_projection_keeps_cancel_before_replace_with_reverse_sealed_ids() {
    let (mut session, request) = fixture();
    let old_id = install_original_child(&mut session, &request);
    session.envelope_ledger.rebase_live_for_next_tick().unwrap();
    let code = request.allocation.code.clone();
    let account = session.plans.plan(request.plan_id).unwrap().account;
    let (mut p3, mut p4) = seal(&mut session);
    let candidates = [
        P2Candidate::new(
            P2CandidateKey::player(0),
            account,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(901),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(1),
            account,
            Intent::Cancel {
                code: code.clone(),
                id: old_id,
            },
        ),
    ];
    let outcomes = p3.consume_round(candidates).unwrap();
    let new_id = outcomes[0]
        .operation()
        .and_then(|operation| match operation {
            P3ValidatedOperation::Place(draft) => Some(draft.order_id()),
            P3ValidatedOperation::Cancel { .. } => None,
        })
        .unwrap();
    let operations = outcomes
        .iter()
        .rev()
        .filter_map(|outcome| outcome.operation().cloned())
        .collect::<Vec<_>>();
    let round = p4.apply_round(operations).unwrap();
    assert_eq!(
        round
            .facts
            .iter()
            .map(|fact| fact.sealed_index)
            .collect::<Vec<_>>(),
        vec![1, 0]
    );
    validate_execution_round(&outcomes, &round).unwrap();

    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    chain
        .project_auction_execution_round(&mut session, &round)
        .unwrap();
    assert_eq!(
        session.parent_orders[&account][&code].active_child_order_id,
        Some(new_id)
    );
    assert_eq!(session.auction_orders[&code].len(), 1);
    assert_eq!(session.auction_orders[&code][0].order_id, new_id.0);
}

fn install_original_child(session: &mut GameSession, request: &PlanExecutionRequest) -> OrderId {
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request.clone());
    let events = commit_injected_plan_roots_for_test(session, batch);
    let accepted = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1, "expected one accepted auction child");
    accepted[0]
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
    let completion = chain.finish().unwrap();
    assert!(matches!(
        completion.reports[0].disposition,
        PlanExecutionDisposition::Replaced { canceled_order_id, .. } if canceled_order_id == old_id
    ));
    let validation = p3.finish();
    let candidates = P2CandidateBatch::new(candidates).unwrap();
    let finish = finish_incremental_auction_coordinator(&session, p4).unwrap();
    assert!(finish.workers.values().all(|worker| {
        worker.finalizer.auction_tail_passes == 1
            && worker.finalizer.auction_completion_passes == 0
            && worker.finalizer.day_end_passes == 0
    }));
    let output = apply_incremental_auction_finish(
        &mut session,
        &candidates,
        &validation,
        finish,
        Vec::new(),
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
        session.auction_orders[&request.allocation.code][0].order_id,
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
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
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
    let place = chain.next_ready_batch(&mut candidate).unwrap().remove(0);
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
    let ledger_before = authority.envelope_ledger.clone();
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
    assert!(plan.state.execute(|_| Ok(())).is_err());
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
    assert_eq!(authority.next_order_id, next_order_before);
    assert_eq!(
        serde_json::to_value(&authority.auction_orders).unwrap(),
        queue_before
    );
    assert_eq!(authority.envelope_ledger, ledger_before);
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
