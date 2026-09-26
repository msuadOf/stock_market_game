use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteReason};
use crate::session::pipeline::{
    commit_injected_plan_roots_for_test, decision_snapshot_capture::capture_decision_snapshot,
    p3_context::build_p3_validation_context, p4_continuous::IncrementalContinuousStockCoordinator,
    p4_continuous_adapter::prepare_incremental_continuous_inputs, DecisionResourceSnapshot,
    EnvelopeLedger, P3ValidatorDriver,
};
use crate::session::{PlanExecutionDisposition, PlanExecutionRequest};
use crate::{AccountKind, Event, Money};
use std::collections::BTreeMap;

#[path = "npc_lifecycle_projection_tests.rs"]
mod npc_lifecycle_projection_tests;

fn fixture() -> (GameSession, PlanExecutionRequest) {
    crate::session::plan_chain_candidates_tests::execution_fixture()
}

fn seal(session: &mut GameSession) -> (P3ValidatorDriver, IncrementalContinuousStockCoordinator) {
    session.envelope_ledger = EnvelopeLedger::new(
        session.next_receipt_base,
        session.project_live_envelopes().unwrap(),
    )
    .unwrap();
    let resources = DecisionResourceSnapshot::seal(session).unwrap();
    let p3 = P3ValidatorDriver::new(
        resources,
        session.envelope_ledger.clone(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(session).unwrap(),
    )
    .unwrap();
    let p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(session).unwrap(),
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

#[test]
fn empty_account_source_finishes_without_a_frozen_observation() {
    let (mut session, _) = fixture();
    let roots = PlanChainOperationBatch::accounts(
        Vec::new(),
        session.build_market_view(),
        BTreeMap::new(),
        BTreeMap::new(),
        session.observation_civil_instant(),
        Default::default(),
    );
    let mut chain = AdaptivePlanChainCoordinator::capture_batch(&session, roots).unwrap();

    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    assert!(chain.finish().unwrap().reports.is_empty());
}

#[test]
fn plan_observation_waits_for_an_earlier_request_from_its_account_and_stock() {
    let (mut session, request) = fixture();
    let code = request.allocation.code.clone();
    let mut chain = coordinator(&session, request);
    chain.block_unfinished_routes(&[P2Candidate::new(
        P2CandidateKey::npc(AccountId(1), 0),
        AccountId(1),
        Intent::Cancel {
            code,
            id: OrderId(123),
        },
    )]);

    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    assert!(chain.pending.is_empty());
    chain.clear_unfinished_routes();
    assert_eq!(chain.next_ready_batch(&mut session).unwrap().len(), 1);
}

#[test]
fn unrelated_account_request_on_the_same_stock_does_not_block_plan_root() {
    let (mut session, request) = fixture();
    let code = request.allocation.code.clone();
    let mut chain = coordinator(&session, request);
    chain.block_unfinished_routes(&[P2Candidate::new(
        P2CandidateKey::npc(AccountId(2), 0),
        AccountId(2),
        Intent::Cancel {
            code,
            id: OrderId(123),
        },
    )]);

    assert_eq!(chain.next_ready_batch(&mut session).unwrap().len(), 1);
}

fn execute(
    session: &mut GameSession,
    chain: &mut AdaptivePlanChainCoordinator,
    p3: &mut P3ValidatorDriver,
    p4: &mut IncrementalContinuousStockCoordinator,
) -> Option<(
    P2Candidate,
    P3ConsumeOutcome,
    Option<ContinuousExecutionRound>,
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
    chain
        .advance_after_typed_outcomes(session, std::slice::from_ref(&outcome), round.as_ref())
        .unwrap();
    Some((candidate, outcome, round))
}

fn working_orders(session: &mut GameSession, prices: &[i64]) -> Vec<OrderId> {
    let code = StockCode("600888".to_owned());
    session.accounts.get_mut(&AccountId(1)).unwrap().kind = AccountKind::Player;
    let mut events = Vec::new();
    for price in prices {
        session.seed_order_for_test(
            AccountId(1),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(*price),
                qty: 100,
            },
            &mut events,
        );
    }
    let ids: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), prices.len());
    ids
}

#[test]
fn adaptive_real_p3_p4_two_cancels_then_new_order_complete_in_one_tick() {
    let (mut session, request) = fixture();
    let ids = working_orders(&mut session, &[901, 902]);
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    let cash = session.accounts[&AccountId(1)].cash;
    let mut candidates = Vec::new();
    while let Some((candidate, _, _)) = execute(&mut session, &mut chain, &mut p3, &mut p4) {
        candidates.push(candidate);
    }
    assert_eq!(candidates.len(), 3);
    assert!(matches!(candidates[0].intent(), Intent::Cancel { id, .. } if *id == ids[0]));
    assert!(matches!(candidates[1].intent(), Intent::Cancel { id, .. } if *id == ids[1]));
    assert!(
        matches!(candidates[2].intent(), Intent::PlaceLimit { price, .. } if *price == Money::from_cents(900))
    );
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        (0..3).map(P2CandidateKey::plan_chain).collect::<Vec<_>>()
    );
    let complete = chain.finish().unwrap();
    assert_eq!(complete.reports.len(), 1);
    assert!(matches!(
        complete.reports[0].disposition,
        PlanExecutionDisposition::Submitted { .. }
    ));
    assert_eq!(complete.consumed.operations.len(), 3);
    assert_eq!(
        session.markets[&request.allocation.code].resting_order_count(),
        1
    );
    assert_eq!(session.accounts[&AccountId(1)].cash, cash, "P6 has not run");
    assert!(session.pending_plan_events.is_empty());
}

#[test]
fn adaptive_real_replace_cancels_old_child_then_installs_new_child() {
    let (mut session, mut request) = fixture();
    let mut old = PlanChainOperationBatch::empty();
    old.push_execution(request.clone());
    commit_injected_plan_roots_for_test(&mut session, old);
    let old_id = session
        .plans
        .plan(request.plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    assert!(execute(&mut session, &mut chain, &mut p3, &mut p4).is_some());
    #[cfg(feature = "simulation-diagnostics")]
    assert!(session.causal_facts().iter().any(|fact| matches!(
        fact.kind,
        crate::diagnostics::causal::CausalFactKind::Terminated {
            order,
            reason: crate::diagnostics::causal::Termination::Reprice,
            ..
        } if order == old_id
    )));
    assert!(execute(&mut session, &mut chain, &mut p3, &mut p4).is_some());
    assert!(execute(&mut session, &mut chain, &mut p3, &mut p4).is_none());
    let complete = chain.finish().unwrap();
    let PlanExecutionDisposition::Replaced {
        canceled_order_id,
        order_id,
        ..
    } = complete.reports[0].disposition
    else {
        panic!("replace must finish both commands in the same tick");
    };
    assert_eq!(canceled_order_id, old_id);
    assert_ne!(order_id, old_id);
    assert_eq!(
        session
            .plans
            .plan(request.plan_id)
            .unwrap()
            .active_child_order_id,
        Some(order_id)
    );
    assert_eq!(
        session.parent_orders[&AccountId(1)][&request.allocation.code].active_child_order_id,
        Some(order_id)
    );
}

#[test]
fn replace_rechecks_plan_remaining_after_another_account_partially_fills_old_child() {
    let (mut session, mut request) = fixture();
    let code = request.allocation.code.clone();
    let mut old = PlanChainOperationBatch::empty();
    old.push_execution(request.clone());
    commit_injected_plan_roots_for_test(&mut session, old);
    let old_id = session
        .plans
        .plan(request.plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            crate::account::Position {
                qty: 50,
                t1_locked: 0,
                invested_cents: 0,
                recovered_cents: 0,
            },
        );
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    let pending_cancel = chain.next_ready_batch(&mut session).unwrap();
    assert_eq!(pending_cancel.len(), 1);
    assert!(matches!(pending_cancel[0].intent(), Intent::Cancel { id, .. } if *id == old_id));

    let seller = P2Candidate::new(
        P2CandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(900),
            qty: 50,
        },
    );
    let seller_step = p3.consume(seller.clone()).unwrap();
    let seller_round = p4
        .apply_round(vec![seller_step.operation().unwrap().clone()])
        .unwrap();
    chain
        .project_execution_round(&mut session, &seller_round)
        .unwrap();
    assert_eq!(session.plans.plan(request.plan_id).unwrap().filled_qty, 50);

    let cancel_step = p3.consume(pending_cancel[0].clone()).unwrap();
    let cancel_round = p4
        .apply_round(vec![cancel_step.operation().unwrap().clone()])
        .unwrap();
    chain
        .advance_after_typed_outcomes(&mut session, &[cancel_step], Some(&cancel_round))
        .unwrap();
    assert!(
        chain.next_ready_batch(&mut session).unwrap().is_empty(),
        "the remaining 50 shares cannot produce another 100-share buy request"
    );
    assert_eq!(session.plans.plan(request.plan_id).unwrap().filled_qty, 50);
    assert!(session.markets[&code]
        .resting_orders_for(AccountId(1))
        .is_empty());
    assert!(matches!(
        chain.finish().unwrap().reports[0].disposition,
        PlanExecutionDisposition::Waiting {
            reason: QuoteReason::PendingReconsideration
        }
    ));
}

#[test]
fn conflicting_cancels_recheck_live_orders_after_another_account_fills_one() {
    let (mut session, request) = fixture();
    let plan_id = request.plan_id;
    let code = request.allocation.code.clone();
    let ids = working_orders(&mut session, &[901, 902]);
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            crate::account::Position {
                qty: 100,
                t1_locked: 0,
                invested_cents: 0,
                recovered_cents: 0,
            },
        );
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request);
    let first = chain.next_ready_batch(&mut session).unwrap().remove(0);
    assert!(matches!(first.intent(), Intent::Cancel { id, .. } if *id == ids[0]));

    let seller = P2Candidate::new(
        P2CandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(902),
            qty: 100,
        },
    );
    let seller_step = p3.consume(seller.clone()).unwrap();
    let seller_round = p4
        .apply_round(vec![seller_step.operation().unwrap().clone()])
        .unwrap();
    assert!(seller_round.facts.iter().any(|fact| matches!(
        &fact.outcome,
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Filled {
                account: AccountId(0),
                filled_qty: 100,
                ..
            },
            ..
        }
    )));
    chain
        .project_execution_round(&mut session, &seller_round)
        .unwrap();
    assert!(session.markets[&code]
        .resting_orders_for(AccountId(1))
        .iter()
        .all(|order| order.id != ids[1]));

    let first_step = p3.consume(first.clone()).unwrap();
    let first_round = p4
        .apply_round(vec![first_step.operation().unwrap().clone()])
        .unwrap();
    chain
        .advance_after_typed_outcomes(&mut session, &[first_step], Some(&first_round))
        .unwrap();

    let next = chain.next_ready_batch(&mut session).unwrap();
    assert_eq!(next.len(), 1);
    assert!(matches!(next[0].intent(), Intent::PlaceLimit { .. }));
    assert!(session.markets[&code]
        .resting_orders_for(AccountId(1))
        .is_empty());
    let submit_step = p3.consume(next[0].clone()).unwrap();
    let submit_round = p4
        .apply_round(vec![submit_step.operation().unwrap().clone()])
        .unwrap();
    chain
        .advance_after_typed_outcomes(&mut session, &[submit_step], Some(&submit_round))
        .unwrap();
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    let complete = chain.finish().unwrap();
    let PlanExecutionDisposition::Submitted { order_id, .. } = complete.reports[0].disposition
    else {
        panic!("the replacement child must be submitted");
    };
    let working = session.markets[&code].resting_orders_for(AccountId(1));
    assert_eq!(working.len(), 1);
    assert_eq!(working[0].id, order_id);
    assert_eq!(
        session.plans.plan(plan_id).unwrap().active_child_order_id,
        Some(order_id)
    );
    assert_eq!(
        session.parent_orders[&AccountId(1)][&code].active_child_order_id,
        Some(order_id)
    );
    assert_eq!(
        session
            .plans
            .active_plan(AccountId(1), &code)
            .unwrap()
            .filled_qty,
        0
    );
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn every_typed_plan_cancel_cause_keeps_its_causal_classification() {
    use crate::diagnostics::causal::Termination;
    use crate::session::plan_execution::PlanCancelCause;

    for cause in [
        PlanCancelCause::Replace,
        PlanCancelCause::ConflictingWorkingOrder,
    ] {
        assert_eq!(cause.causal_termination(), Termination::Reprice);
    }
    for cause in [PlanCancelCause::Restructure, PlanCancelCause::Explicit] {
        assert_eq!(cause.causal_termination(), Termination::Voluntary);
    }
}

#[test]
fn adaptive_cancel_release_never_refills_p3_cash_for_the_followup_place() {
    let (mut session, request) = fixture();
    working_orders(&mut session, &[901, 902]);
    let cash = session.reserved_cash_for_account(AccountId(1)).unwrap();
    session.accounts.get_mut(&AccountId(1)).unwrap().cash = cash;
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request);
    execute(&mut session, &mut chain, &mut p3, &mut p4).unwrap();
    execute(&mut session, &mut chain, &mut p3, &mut p4).unwrap();
    let (_, outcome, round) = execute(&mut session, &mut chain, &mut p3, &mut p4).unwrap();
    assert!(matches!(
        outcome.result(),
        P3CandidateResult::Rejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    ));
    assert!(round.is_none());
    assert!(execute(&mut session, &mut chain, &mut p3, &mut p4).is_none());
    assert!(matches!(
        chain.finish().unwrap().reports[0].disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::InsufficientCash
        }
    ));
    assert_eq!(session.accounts[&AccountId(1)].cash, cash);
}

fn fill_case(resting_sell_qty: u32) {
    let (mut session, mut request) = fixture();
    let code = request.allocation.code.clone();
    request.decision.action = QuoteAction::Submit {
        price: Money::from_cents(1_000),
        qty: 100,
    };
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), resting_sell_qty, Money::from_cents(1_000))
        .unwrap();
    session.seed_order_for_test(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: resting_sell_qty,
        },
        &mut Vec::new(),
    );
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = coordinator(&session, request.clone());
    let (_, _, round) = execute(&mut session, &mut chain, &mut p3, &mut p4).unwrap();
    let round = round.unwrap();
    assert_eq!(
        session.plans.plan(request.plan_id).unwrap().filled_qty,
        resting_sell_qty
    );
    assert!(session.pending_plan_events.is_empty());
    if resting_sell_qty == 100 {
        assert!(matches!(
            round.facts[0].outcome,
            ContinuousExecutionOutcome::Place {
                fact: ContinuousPlaceFact::Filled { .. },
                ..
            }
        ));
        assert!(!session
            .parent_orders
            .get(&AccountId(1))
            .is_some_and(|parents| parents.contains_key(&code)));
    } else {
        assert_eq!(
            session.parent_orders[&AccountId(1)][&code].filled_qty,
            resting_sell_qty
        );
        assert_eq!(
            session.parent_orders[&AccountId(1)][&code].active_child_remaining_qty,
            Some(100 - resting_sell_qty)
        );
    }
    let before = serde_json::to_value(&session.plans).unwrap();
    assert!(
        chain.project_execution_round(&mut session, &round).is_err(),
        "same facts must not be applied twice"
    );
    assert_eq!(serde_json::to_value(&session.plans).unwrap(), before);
}

#[test]
fn adaptive_immediate_full_fill_advances_plan_once_without_order_accepted_event() {
    fill_case(100);
}

#[test]
fn adaptive_immediate_partial_fill_advances_parent_and_plan_once() {
    fill_case(50);
}

#[test]
fn adaptive_rejected_first_or_second_cancel_never_emits_dependent_place() {
    for reject_index in [0, 1] {
        let (mut session, request) = fixture();
        working_orders(&mut session, &[901, 902]);
        let (mut p3, mut p4) = seal(&mut session);
        let mut chain = coordinator(&session, request);
        if reject_index == 1 {
            execute(&mut session, &mut chain, &mut p3, &mut p4).unwrap();
        }
        let candidate = chain.next_ready_batch(&mut session).unwrap().remove(0);
        let outcome = p3.consume(candidate.clone()).unwrap();
        let Intent::Cancel { code, id } = candidate.intent() else {
            panic!("expected conflict cancel");
        };
        // This unit fixture supplies an explicit negative P4 result at each continuation edge.
        // The success cases above exercise real P4 cancellation; no public events are involved.
        let rejected = ContinuousExecutionRound {
            facts: vec![ContinuousExecutionFact {
                candidate_key: candidate.key().clone(),
                sealed_index: outcome.sealed_index(),
                allocated_order_id: None,
                outcome: ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
                    sealed_index: outcome.sealed_index(),
                    account: candidate.owner(),
                    code: code.clone(),
                    order_id: *id,
                    reason: ContinuousCancelRejection::OrderNotFound,
                }),
            }],
            receipts: Vec::new(),
            trades: Vec::new(),
            projections: BTreeMap::new(),
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes: BTreeMap::new(),
        };
        chain
            .advance_after_typed_outcomes(
                &mut session,
                std::slice::from_ref(&outcome),
                Some(&rejected),
            )
            .unwrap();
        assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
        assert!(matches!(
            chain.finish().unwrap().reports[0].disposition,
            PlanExecutionDisposition::RouteRejected {
                reason: RejectionReason::OrderNotFound
            }
        ));
        assert_eq!(p3.output().drafts().len(), 0);
    }
}

#[test]
fn adaptive_wrong_candidate_or_sealed_identity_stops_the_coordinator() {
    for swap_candidate in [false, true] {
        let (mut session, request) = fixture();
        let (mut p3, mut p4) = seal(&mut session);
        let mut chain = coordinator(&session, request);
        let candidate = chain.next_ready_batch(&mut session).unwrap().remove(0);
        let outcome = p3.consume(candidate).unwrap();
        let mut round = p4
            .apply_round(vec![outcome.operation().unwrap().clone()])
            .unwrap();
        if swap_candidate {
            round.facts[0].candidate_key = P2CandidateKey::plan_chain(88);
        } else {
            round.facts[0].sealed_index += 1;
        }
        assert!(chain
            .advance_after_typed_outcomes(
                &mut session,
                std::slice::from_ref(&outcome),
                Some(&round)
            )
            .is_err());
        assert!(chain.next_ready_batch(&mut session).is_err());
        assert!(chain.finish().is_err());
    }
}

#[test]
fn independent_accounts_share_one_round_and_late_bad_fact_cannot_commit() {
    use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};

    let (mut authority, first) = fixture();
    let second_id = authority
        .plans
        .create(PlanOpen {
            account: AccountId(0),
            code: first.allocation.code.clone(),
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
    let mut second = first.clone();
    second.plan_id = second_id;
    second.allocation.plan_id = second_id;
    let before = authority.business_state_hash().unwrap();
    let mut session = authority.clone_for_tick_shadow().unwrap();
    let (mut p3, mut p4) = seal(&mut session);
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(first);
    roots.push_execution(second);
    let mut chain = AdaptivePlanChainCoordinator::capture_batch(&session, roots).unwrap();
    let batch = chain.next_ready_batch(&mut session).unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(
        batch.iter().map(P2Candidate::owner).collect::<Vec<_>>(),
        vec![AccountId(1), AccountId(0)]
    );
    let outcomes = p3.consume_round(batch).unwrap();
    let operations = outcomes
        .iter()
        .filter_map(|outcome| outcome.operation().cloned())
        .collect();
    let mut round = p4.apply_round(operations).unwrap();
    assert_eq!(round.facts.len(), 2);
    round.facts[1].sealed_index += 1;
    assert!(chain
        .advance_after_typed_outcomes(&mut session, &outcomes, Some(&round))
        .is_err());
    assert!(chain.finish().is_err());
    assert_eq!(authority.business_state_hash().unwrap(), before);
}

#[test]
fn independent_stocks_finish_in_one_plan_round_for_same_or_different_accounts() {
    for second_account in [AccountId(0), AccountId(1)] {
        for reverse_facts in [false, true] {
            run_independent_stock_round(second_account, reverse_facts, false);
        }
    }
}

#[test]
fn independent_stock_feedback_can_arrive_outside_the_plan_pending_prefix() {
    for second_account in [AccountId(0), AccountId(1)] {
        run_independent_stock_round(second_account, false, true);
    }
}

fn run_independent_stock_round(
    second_account: AccountId,
    reverse_facts: bool,
    split_feedback: bool,
) {
    use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};

    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        47,
    )
    .unwrap();
    let (_, template) = fixture();
    let mut roots = PlanChainOperationBatch::empty();
    let mut plan_ids = Vec::new();
    for (account, code) in [
        (AccountId(1), StockCode("600888".to_owned())),
        (second_account, StockCode("600889".to_owned())),
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
                    source: crate::plans::OpinionSource::Blended,
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
        plan_ids.push(plan_id);
    }
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain = AdaptivePlanChainCoordinator::capture_batch(&session, roots).unwrap();
    let batch = chain.next_ready_batch(&mut session).unwrap();
    assert_eq!(batch.len(), 2);
    let outcomes = p3.consume_round(batch).unwrap();
    if split_feedback {
        for index in [1, 0] {
            let outcome = &outcomes[index];
            let round = p4
                .apply_round(vec![outcome.operation().unwrap().clone()])
                .unwrap();
            chain
                .advance_after_typed_outcomes(
                    &mut session,
                    std::slice::from_ref(outcome),
                    Some(&round),
                )
                .unwrap();
        }
    } else {
        let mut round = p4
            .apply_round(
                outcomes
                    .iter()
                    .filter_map(|outcome| outcome.operation().cloned())
                    .collect(),
            )
            .unwrap();
        assert_eq!(round.projections.len(), 2);
        if reverse_facts {
            round.facts.reverse();
        }
        chain
            .advance_after_typed_outcomes(&mut session, &outcomes, Some(&round))
            .unwrap();
    }
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    assert_eq!(chain.finish().unwrap().reports.len(), plan_ids.len());
    for plan_id in plan_ids {
        assert!(session
            .plans
            .plan(plan_id)
            .unwrap()
            .active_child_order_id
            .is_some());
    }
}

#[test]
fn account_execution_quotes_two_existing_stocks_before_either_p4_result() {
    use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};

    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        47,
    )
    .unwrap();
    let owner = AccountId(1);
    session.accounts.get_mut(&owner).unwrap().cash = Money::from_cents(10_000_000);
    let codes = [
        StockCode("600888".to_owned()),
        StockCode("600889".to_owned()),
    ];
    let plan_ids = codes
        .iter()
        .map(|code| {
            session
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
                    horizon_trading_days: 1,
                    created_trading_day: 0,
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_account_execution(owner, session.build_market_view());
    let mut chain = AdaptivePlanChainCoordinator::capture_batch(&session, roots).unwrap();
    let (mut p3, mut p4) = seal(&mut session);

    let batch = chain.next_ready_batch(&mut session).unwrap();
    assert_eq!(batch.len(), 2, "both natural plan quotes must be ready");
    assert_eq!(
        batch
            .iter()
            .map(|candidate| match candidate.intent() {
                Intent::PlaceLimit { code, .. } => code.clone(),
                other => panic!("expected plan quote, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        codes
    );
    let outcomes = p3.consume_round(batch).unwrap();
    let round = p4
        .apply_round(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.operation().cloned())
                .collect(),
        )
        .unwrap();
    assert_eq!(round.projections.len(), 2);
    chain
        .advance_after_typed_outcomes(&mut session, &outcomes, Some(&round))
        .unwrap();
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    assert_eq!(chain.finish().unwrap().reports.len(), 2);
    for plan_id in plan_ids {
        assert!(session
            .plans
            .plan(plan_id)
            .unwrap()
            .active_child_order_id
            .is_some());
    }
}

#[test]
fn projected_rounds_reject_reused_candidate_or_sealed_identity() {
    let rejected_round = |candidate_key, sealed_index| ContinuousExecutionRound {
        facts: vec![ContinuousExecutionFact {
            candidate_key,
            sealed_index,
            allocated_order_id: None,
            outcome: ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
                sealed_index,
                account: AccountId(0),
                code: StockCode("600888".to_owned()),
                order_id: OrderId(1),
                reason: ContinuousCancelRejection::OrderNotFound,
            }),
        }],
        receipts: Vec::new(),
        trades: Vec::new(),
        projections: BTreeMap::new(),
        #[cfg(feature = "simulation-diagnostics")]
        operation_quotes: BTreeMap::new(),
    };
    for (key, sealed) in [
        (P2CandidateKey::player(0), 1),
        (P2CandidateKey::player(1), 0),
    ] {
        let (mut session, _) = fixture();
        let mut chain =
            AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
                .unwrap();
        chain
            .project_execution_round(&mut session, &rejected_round(P2CandidateKey::player(0), 0))
            .unwrap();
        assert!(chain
            .project_execution_round(&mut session, &rejected_round(key, sealed))
            .is_err());
        assert!(chain.next_ready_batch(&mut session).is_err());
    }
}

#[test]
fn projected_auction_rounds_reject_reused_candidate_or_sealed_identity() {
    let rejected_round = |candidate_key: P2CandidateKey, sealed_index| AuctionExecutionRound {
        facts: vec![AuctionExecutionFact {
            candidate_key: candidate_key.clone(),
            sealed_index,
            allocated_order_id: None,
            outcome: AuctionLifecycleFact::Rejected {
                candidate_key,
                sealed_index,
                account: AccountId(0),
                code: StockCode("600888".to_owned()),
                order_id: None,
                reason: RejectionReason::OrderNotFound,
            },
        }],
        receipts: Vec::new(),
        projections: BTreeMap::new(),
    };
    for (key, sealed) in [
        (P2CandidateKey::player(0), 1),
        (P2CandidateKey::player(1), 0),
    ] {
        let (mut session, _) = fixture();
        let mut chain =
            AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
                .unwrap();
        chain
            .project_auction_execution_round(
                &mut session,
                &rejected_round(P2CandidateKey::player(0), 0),
            )
            .unwrap();
        assert!(chain
            .project_auction_execution_round(&mut session, &rejected_round(key, sealed))
            .is_err());
        assert!(chain.next_ready_batch(&mut session).is_err());
    }
}

#[test]
fn adaptive_late_chain_generation_overflow_discards_only_private_progress() {
    let (mut authority, request) = fixture();
    working_orders(&mut authority, &[901, 902]);
    let before = authority.business_state_hash().unwrap();
    let mut candidate_session = authority.clone_for_tick_shadow().unwrap();
    let (mut p3, mut p4) = seal(&mut candidate_session);
    let mut chain = coordinator(&candidate_session, request);
    execute(&mut candidate_session, &mut chain, &mut p3, &mut p4).unwrap();
    chain.roots.set_adaptive_generation_for_test(u64::MAX);
    assert!(matches!(
        chain.next_ready_batch(&mut candidate_session),
        Err(StepFatal::InvariantViolation { .. })
    ));
    assert!(chain.finish().is_err());
    assert_eq!(authority.business_state_hash().unwrap(), before);
    assert_eq!(
        authority
            .markets
            .values()
            .map(|market| market.resting_order_count())
            .sum::<usize>(),
        2
    );
}

#[test]
fn adaptive_initial_player_partial_market_fill_is_projected_without_a_false_full_fill_requirement()
{
    let (mut session, _) = fixture();
    let code = StockCode("600888".to_owned());
    session
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(code.clone(), 50, Money::from_cents(1_000))
        .unwrap();
    session.seed_order_for_test(
        AccountId(1),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 50,
        },
        &mut Vec::new(),
    );
    let (mut p3, mut p4) = seal(&mut session);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let outcome = p3
        .consume(P2Candidate::new(
            P2CandidateKey::player(0),
            AccountId(0),
            Intent::PlaceMarket {
                code,
                side: Side::Buy,
                qty: 100,
            },
        ))
        .unwrap();
    let round = p4
        .apply_round(vec![outcome.operation().unwrap().clone()])
        .unwrap();
    assert!(matches!(
        round.facts[0].outcome,
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Filled { filled_qty: 50, .. },
            original_qty: 100,
        }
    ));
    chain.project_execution_round(&mut session, &round).unwrap();
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    assert_eq!(chain.finish().unwrap().consumed.operations.len(), 1);
}

#[test]
fn adaptive_real_multi_account_lifecycle_quote_and_execution_roots_share_one_stream() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].code = StockCode("000812".to_owned());
    setup.stocks[0].exchange = crate::StockExchange::Shenzhen;
    setup.stocks[0].initial_price = Money::from_cents(285);
    setup.stocks[0].category = crate::SecurityCategory::StMainBoard;
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 400_000;
    setup.npcs.retail_cash_median = Money::from_cents(60_000);
    setup.strategy_params.inst.order_size = 1_000;
    setup.closing_auction_ticks = 10;
    setup.history_len = 5;
    setup.start_date = crate::CivilDate::from_iso("2030-01-07").unwrap();
    let template = GameSession::new(setup.clone(), 42).unwrap();
    setup.npcs.inst_count = 2;
    let mut session = GameSession::new(setup, 42).unwrap();
    for id in [AccountId(1), AccountId(2)] {
        let strategy = template.accounts[&AccountId(1)]
            .strategy
            .as_ref()
            .unwrap()
            .production_state()
            .unwrap()
            .into_strategy()
            .unwrap();
        session.accounts.get_mut(&id).unwrap().strategy =
            Some(crate::account::StoredStrategy::production(strategy));
        session
            .belief_books
            .insert(id, template.belief_books[&AccountId(1)].clone());
        session.accounts.get_mut(&id).unwrap().cash = template.accounts[&AccountId(1)].cash;
        session.accounts.get_mut(&id).unwrap().positions =
            template.accounts[&AccountId(1)].positions.clone();
        crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, id, 0);
        assert!(session.accounts[&id]
            .strategy
            .as_ref()
            .unwrap()
            .belief_chain_params()
            .is_some());
    }
    session.seed_order_for_test(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockCode("000812".to_owned()),
            side: Side::Buy,
            price: Money::from_cents(280),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let snapshot = capture_decision_snapshot(&mut session).unwrap().snapshot;
    assert_eq!(snapshot.due_npc_ids(), &[AccountId(1), AccountId(2)]);
    let mut chain = AdaptivePlanChainCoordinator::capture_roots_before_p4(
        &session,
        snapshot.due_npc_ids(),
        &snapshot,
    )
    .unwrap();
    let (mut p3, mut p4) = seal(&mut session);
    let mut owners = Vec::new();
    let mut keys = Vec::new();
    loop {
        let batch = chain.next_ready_batch(&mut session).unwrap();
        if batch.is_empty() {
            break;
        }
        owners.extend(batch.iter().map(P2Candidate::owner));
        keys.extend(batch.iter().map(|candidate| candidate.key().clone()));
        let outcomes = p3.consume_round(batch).unwrap();
        let operations = outcomes
            .iter()
            .filter_map(|outcome| outcome.operation().cloned())
            .collect();
        let round = p4.apply_round(operations).unwrap();
        chain
            .advance_after_typed_outcomes(&mut session, &outcomes, Some(&round))
            .unwrap();
    }
    owners.sort();
    assert_eq!(owners, vec![AccountId(1), AccountId(2)]);
    keys.sort();
    assert_eq!(
        keys,
        vec![P2CandidateKey::plan_chain(0), P2CandidateKey::plan_chain(1)]
    );
    let completion = chain.finish().unwrap();
    assert_eq!(completion.reports.len(), 2);
    assert_eq!(completion.consumed.operations.len(), 2);
    assert_eq!(session.plans.plan_ids().count(), 2);
}
