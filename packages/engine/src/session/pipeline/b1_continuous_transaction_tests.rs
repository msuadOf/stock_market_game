use super::b1_continuous_transaction::{
    apply_tick_shadow_b1_continuous_transaction,
    apply_tick_shadow_b1_continuous_transaction_with_roots_for_test, prepare_b1_continuous_tick,
    validate_execution_round_for_test,
};
use super::p3_context::build_p3_validation_context;
use super::p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator};
use super::p4_continuous_adapter::prepare_incremental_continuous_inputs;
use super::*;
use crate::session::{ParentOrderPlan, PlanExecutionDisposition, RetailOrderDiagnosticEvent};
use crate::{
    AccountId, Event, Intent, Money, Order, OrderId, RejectionReason, RetailExperienceState, Side,
    StockCode,
};

fn player_only_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn matching_working_order_is_adopted_by_plan_without_allocating_another_order_id() {
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    let owner = AccountId(1);
    let plan_id = request.plan_id;
    let mut setup_events = Vec::new();
    authority.seed_order_for_test(
        owner,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut setup_events,
    );
    let existing_id = match setup_events.as_slice() {
        [Event::OrderAccepted { id, .. }] => *id,
        other => panic!("fixture must have one accepted order: {other:?}"),
    };
    authority
        .npc_attention
        .get_mut(&owner)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    authority.attention_queue.clear();
    let next_order_id = authority.next_order_id;
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    assert!(matches!(
        output.plan_reports.as_slice(),
        [report] if matches!(
            report.disposition,
            PlanExecutionDisposition::Adopted { order_id, .. } if order_id == existing_id
        )
    ));
    assert!(!output.events.iter().any(|event| matches!(event, Event::OrderAccepted { account, code: accepted_code, .. } if *account == owner && accepted_code == &code)));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();

    assert_eq!(authority.next_order_id, next_order_id);
    assert_eq!(
        authority.plans.plan(plan_id).unwrap().active_child_order_id,
        Some(existing_id)
    );
    assert_eq!(
        authority.parent_orders[&owner][&code].active_child_order_id,
        Some(existing_id)
    );
    assert!(authority.pending_plan_events.is_empty());
}

#[test]
fn player_and_other_accounts_plan_join_one_ready_stock_batch() {
    use super::ready_ingress::ReadyIngress;
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    let plan_id = request.plan_id;
    authority.pending_player.push((
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            // Both arrival orders stay inside the price cage. This case checks
            // shared admission, not the effect of moving the best bid.
            price: Money::from_cents(900),
            qty: 100,
        },
    ));
    authority.attention_queue.clear();
    let roots = |request| {
        let mut roots = PlanChainOperationBatch::empty();
        roots.push_execution(request);
        roots
    };

    let mut preview = authority.clone_for_tick_shadow().unwrap();
    let mut ingress = ReadyIngress::capture_sources(&mut preview)
        .unwrap()
        .capture_roots(&preview, Some(roots(request.clone())))
        .unwrap();
    let ready = ingress.first_ready_batch(&mut preview).unwrap();
    assert!(ready
        .iter()
        .any(|candidate| matches!(candidate.key(), P2CandidateKey::Player { .. })));
    assert!(ready
        .iter()
        .any(|candidate| matches!(candidate.key(), P2CandidateKey::PlanChain { .. })));

    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots(request))
            .unwrap();
    assert!(output.events.iter().any(|event| matches!(event, Event::OrderAccepted { account, code: accepted_code, .. } if *account == AccountId(0) && accepted_code == &code)), "events: {:?}", output.events);
    let plan_order = output
        .events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                account,
                code: accepted_code,
                id,
                ..
            } if *account == AccountId(1) && accepted_code == &code => Some(*id),
            _ => None,
        })
        .expect("the independent plan order must be accepted");
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();
    assert_eq!(
        authority.plans.plan(plan_id).unwrap().active_child_order_id,
        Some(plan_order)
    );
    assert_eq!(
        authority.parent_orders[&AccountId(1)][&code].active_child_order_id,
        Some(plan_order)
    );
}

#[test]
fn queued_npc_cancel_reaches_the_book_before_plan_adopts_the_old_order() {
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;
    use crate::session::PendingNpcBatch;

    let (mut authority, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let plan_id = request.plan_id;
    let code = request.allocation.code.clone();
    let owner = AccountId(1);
    let mut setup_events = Vec::new();
    authority.seed_order_for_test(
        owner,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut setup_events,
    );
    let [Event::OrderAccepted { id: old_id, .. }] = setup_events.as_slice() else {
        panic!("fixture must have one accepted working order");
    };
    let old_id = *old_id;
    authority.pending_npc = Some(PendingNpcBatch {
        dependencies: Vec::new(),
        observed_tick: authority.tick,
        observed_accounts: Vec::new(),
        intents: vec![(
            owner,
            Intent::Cancel {
                code: code.clone(),
                id: old_id,
            },
        )],
    });
    authority
        .npc_attention
        .get_mut(&owner)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    authority.attention_queue.clear();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    assert!(output.events.iter().any(|event| {
        matches!(event, Event::OrderCanceled { account, id, .. } if *account == owner && *id == old_id)
    }));
    assert!(!output.plan_reports.iter().any(|report| {
        matches!(report.disposition, PlanExecutionDisposition::Adopted { order_id, .. } if order_id == old_id)
    }));
    let new_id = output
        .plan_reports
        .iter()
        .find_map(|report| match report.disposition {
            PlanExecutionDisposition::Submitted { order_id, .. } => Some(order_id),
            _ => None,
        })
        .expect("plan must submit a replacement after the queued cancellation");
    assert_ne!(new_id, old_id);
    assert!(output.events.iter().any(|event| {
        matches!(event, Event::OrderAccepted { account, code: accepted_code, id, .. }
            if *account == owner && accepted_code == &code && *id == new_id)
    }));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();
    assert_eq!(
        authority.plans.plan(plan_id).unwrap().active_child_order_id,
        Some(new_id)
    );
    assert_eq!(
        authority.parent_orders[&owner][&code].active_child_order_id,
        Some(new_id)
    );
    assert!(authority.markets[&code]
        .resting_orders()
        .into_iter()
        .any(|order| order.id == new_id && order.owner == owner));
}

#[test]
fn rejected_replace_cancel_keeps_child_parent_reservation_and_order_identity() {
    use crate::plans::quote_policy::QuoteAction;
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, mut request) =
        crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    let owner = AccountId(1);
    authority
        .npc_attention
        .get_mut(&owner)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    authority.attention_queue.clear();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request.clone());
    let mut first_tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut first_tick, roots)
        .unwrap();
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, first_tick)
        .unwrap()
        .commit();

    let old_id = authority
        .plans
        .plan(request.plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    let parent_before = authority.parent_orders[&owner][&code].clone();
    let reserved_before = authority.reserved_cash_for_account(owner).unwrap();
    let next_order_id = authority.next_order_id;
    request.decision.action = QuoteAction::Replace {
        order_id: OrderId(999_999),
        price: Money::from_cents(901),
        qty: 100,
    };
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut replacement = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut replacement, roots)
            .unwrap();
    assert!(matches!(
        output.plan_reports.as_slice(),
        [report] if matches!(
            report.disposition,
            PlanExecutionDisposition::RouteRejected {
                reason: RejectionReason::OrderNotFound
            }
        )
    ));
    assert!(!output
        .candidates
        .candidates()
        .iter()
        .any(
            |candidate| matches!(candidate.intent(), Intent::PlaceLimit { .. })
                && matches!(candidate.key(), P2CandidateKey::PlanChain { .. })
        ));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, replacement)
        .unwrap()
        .commit();

    assert_eq!(authority.next_order_id, next_order_id);
    assert_eq!(authority.parent_orders[&owner][&code], parent_before);
    assert_eq!(
        authority
            .plans
            .plan(parent_before.linked_plan_id.unwrap())
            .unwrap()
            .active_child_order_id,
        Some(old_id)
    );
    assert_eq!(
        authority.reserved_cash_for_account(owner).unwrap(),
        reserved_before
    );
}

fn live_buy_plan_case(
    seller_shares: u32,
    seller_holding: u32,
) -> (GameSession, crate::plans::PlanId, StockCode) {
    use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
    use crate::plans::{AllocationGrant, PlanBook, PlanOpen, PlanOpinion, PlanTarget, Urgency};
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;
    use crate::session::PlanExecutionRequest;

    let mut authority =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 47).unwrap();
    let code = StockCode("600888".to_owned());
    let seller = AccountId(0);
    authority
        .accounts
        .get_mut(&seller)
        .unwrap()
        .grant_position(code.clone(), seller_holding, Money::from_cents(1_000))
        .unwrap();
    authority
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    authority.attention_queue.clear();
    authority.plans = PlanBook::default();
    let plan_id = authority
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(400),
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
    // This fixture injects execution directly instead of opening the plan through
    // the lifecycle. Record that opening observation as the real lifecycle does,
    // so the next fill-only tick does not request an initial strategy review.
    let issuer = authority.company_registry.issuer_of(&code).unwrap();
    let acquired_count = authority.information[&AccountId(1)]
        .records_for_company(issuer)
        .len();
    authority
        .plans
        .record_review(
            plan_id,
            u64::from(authority.day),
            authority.markets[&code].last_price(),
            u32::try_from(acquired_count).unwrap(),
        )
        .unwrap();
    authority
        .enqueue_player_intent(
            seller,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1_000),
                qty: seller_shares,
            },
        )
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
                price: Money::from_cents(1_000),
                qty: 400,
            },
            reason: QuoteReason::OppositeQuoteProbe,
        },
        trading_day: 0,
    };
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();
    (authority, plan_id, code)
}

#[test]
fn fully_filled_first_plan_submit_makes_a_second_ready_submit_a_business_wait() {
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    let plan_id = request.plan_id;
    authority
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    authority
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    authority.attention_queue.clear();
    let mut setup_events = Vec::new();
    authority.seed_order_for_test(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut setup_events,
    );
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request.clone());
    roots.push_execution(request);
    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    assert!(output.plan_reports.iter().any(|report| matches!(
        report.disposition,
        PlanExecutionDisposition::Waiting {
            reason: crate::plans::QuoteReason::PendingReconsideration
        }
    )));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();
    let plan = authority.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty, 100);
    assert!(matches!(plan.status, crate::plans::PlanStatus::Completed));
    assert!(authority
        .parent_orders
        .get(&AccountId(1))
        .is_none_or(|orders| !orders.contains_key(&code)));
}

#[test]
fn live_plan_partial_fill_survives_restore_and_second_real_tick_fill() {
    let (mut uninterrupted, plan_id, code) = live_buy_plan_case(100, 200);
    let first = uninterrupted.save().unwrap();
    assert_eq!(first.plans.plan(plan_id).unwrap().filled_qty, 100);
    let child_id = first
        .plans
        .plan(plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    assert_eq!(
        first.parent_orders[&AccountId(1)][&code].active_child_remaining_qty,
        Some(300)
    );
    let first_reserved = first.snapshot.accounts[&AccountId(1)].reserved_cash;
    let mut restored = GameSession::restore(&first).unwrap();
    for session in [&mut uninterrupted, &mut restored] {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            )
            .unwrap();
        let events = session.step().unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Trade { code: traded, price, qty: 100, maker: AccountId(1), taker: AccountId(0), .. }
                if traded == &code && *price == Money::from_cents(1_000)
        )));
        let saved = session.save().unwrap();
        assert_eq!(saved.plans.plan(plan_id).unwrap().filled_qty, 200);
        assert_eq!(
            saved.plans.plan(plan_id).unwrap().active_child_order_id,
            Some(child_id),
            "the same partly filled child must survive restore and fill again"
        );
        assert_eq!(
            saved.parent_orders[&AccountId(1)][&code].active_child_remaining_qty,
            Some(200)
        );
        assert_eq!(
            saved.snapshot.accounts[&AccountId(1)].reserved_cash,
            session.reserved_cash_for_account(AccountId(1)).unwrap()
        );
        assert!(saved.snapshot.accounts[&AccountId(1)].reserved_cash < first_reserved);
        assert_eq!(
            saved.snapshot.accounts[&AccountId(1)].positions[&code].qty,
            200
        );
    }
    let original = uninterrupted.save().unwrap();
    let replay = restored.save().unwrap();
    let original_buyer = &original.snapshot.accounts[&AccountId(1)];
    let replay_buyer = &replay.snapshot.accounts[&AccountId(1)];
    assert_eq!(original_buyer.cash, replay_buyer.cash);
    assert_eq!(original_buyer.reserved_cash, replay_buyer.reserved_cash);
    assert_eq!(
        original_buyer.positions[&code].qty,
        replay_buyer.positions[&code].qty
    );
}

#[test]
fn live_buy_plan_does_not_round_a_fifty_share_remainder_into_a_new_lot() {
    use crate::plans::quote_policy::{QuoteAction, QuoteReason};
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, plan_id, code) = live_buy_plan_case(350, 350);
    let old_id = authority
        .plans
        .plan(plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    assert_eq!(authority.plans.plan(plan_id).unwrap().filled_qty, 350);
    let (_, mut request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    request.plan_id = plan_id;
    request.allocation.plan_id = plan_id;
    request.allocation.code = code.clone();
    request.decision.action = QuoteAction::Cancel { order_id: old_id };
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request.clone());
    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let canceled =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    assert!(matches!(
        canceled.plan_reports.as_slice(),
        [report] if matches!(report.disposition, PlanExecutionDisposition::Canceled { order_id, .. } if order_id == old_id)
    ));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();
    assert!(authority.markets[&code]
        .resting_orders_for(AccountId(1))
        .is_empty());

    request.decision.action = QuoteAction::Submit {
        price: Money::from_cents(1_001),
        qty: 100,
    };
    request.decision.reason = QuoteReason::OppositeQuoteProbe;
    let next_order_id = authority.next_order_id;
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut tick = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    assert!(matches!(
        output.plan_reports.as_slice(),
        [report] if matches!(report.disposition, PlanExecutionDisposition::RemainingBelowBoardLot { remaining_qty: 50 })
    ));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();
    assert_eq!(authority.plans.plan(plan_id).unwrap().filled_qty, 350);
    assert_eq!(authority.next_order_id, next_order_id);
    assert!(authority.markets[&code]
        .resting_orders_for(AccountId(1))
        .is_empty());
}

#[test]
fn b1_plan_chain_cash_rejection_reaches_final_event_and_report() {
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    authority.accounts.get_mut(&owner).unwrap().cash = Money::ZERO;
    let before_order_id = authority.next_order_id;
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);

    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut plan, roots).unwrap();
    assert!(matches!(
        output.validation.results(),
        [P3CandidateResult::Rejected {
            key: P2CandidateKey::PlanChain {
                chain_generation_index: 0
            },
            reason: RejectionReason::InsufficientCash,
            ..
        }]
    ));
    assert!(matches!(
        output.plan_reports.as_slice(),
        [report] if matches!(report.disposition, PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::InsufficientCash
        })
    ));
    let committed =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
            .unwrap()
            .commit();
    assert!(committed.tick.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account,
            reason: RejectionReason::InsufficientCash,
            ..
        } if *account == owner
    )));
    assert_eq!(authority.next_order_id, before_order_id);
}

#[test]
fn b1_player_rejections_and_acceptance_keep_cash_order_and_order_identity() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
    let first_order_id = OrderId(authority.next_order_id);
    for intent in [
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 1,
        },
        Intent::Cancel {
            code: StockCode("999999".to_owned()),
            id: OrderId(123),
        },
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    ] {
        authority
            .enqueue_player_intent(AccountId(0), intent)
            .unwrap();
    }

    let committed = prepare_b1_continuous_tick(&mut authority).unwrap().commit();
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
    // P3 forwards cancellations without allocating an order ID. The stock
    // worker supplies the UnknownStock business rejection checked below.
    assert!(matches!(result_for(1), P3CandidateResult::Accepted { .. }));
    assert!(matches!(
        committed
            .output
            .validation
            .operations()
            .iter()
            .find(|operation| operation.candidate_key() == &P2CandidateKey::player(1)),
        Some(P3ValidatedOperation::Cancel {
            account: AccountId(0),
            code: canceled_code,
            order_id: OrderId(123),
            ..
        }) if canceled_code == &StockCode("999999".to_owned())
    ));
    assert!(matches!(result_for(2), P3CandidateResult::Accepted { .. }));
    // The two buys share cash and retain receipt order. The independent cancel
    // does not need to sit between them in the result layout.
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
    for (index, expected_id) in [(0, None), (1, None), (2, Some(first_order_id))] {
        let identity = identities
            .iter()
            .find(|(key, _, _)| *key == &P2CandidateKey::player(index))
            .unwrap();
        assert_eq!(identity.2, expected_id);
    }
    let player_events = committed
        .commit
        .tick
        .events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::IntentRejected { .. } | Event::OrderAccepted { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(player_events.len(), 3);
    let quantity_rejection = player_events
        .iter()
        .position(|event| {
            matches!(event,
                Event::IntentRejected {
                    account: AccountId(0), code: rejected,
                    reason: RejectionReason::InvalidQuantity, ..
                } if rejected == &code
            )
        })
        .unwrap();
    assert!(player_events.iter().any(|event| matches!(event,
        Event::IntentRejected {
            account: AccountId(0), code: rejected,
            reason: RejectionReason::UnknownStock, ..
        } if rejected == &StockCode("999999".to_owned())
    )));
    let acceptance = player_events
        .iter()
        .position(|event| {
            matches!(event,
                Event::OrderAccepted {
                    account: AccountId(0), code: accepted, side: Side::Buy,
                    id, remaining_qty: 100, ..
                } if accepted == &code && *id == first_order_id
            )
        })
        .unwrap();
    assert!(quantity_rejection < acceptance);
    assert_eq!(authority.next_order_id, first_order_id.0 + 1);
    assert!(authority.pending_player.is_empty());
}

#[test]
fn joint_b1_player_batch_reaches_rebased_p9_without_legacy_bridge() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
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
    let before = authority.business_state_hash().unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output = apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();

    assert_eq!(authority.business_state_hash().unwrap(), before);
    assert_eq!(authority.pending_player.len(), 1);
    assert_eq!(
        output
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert!(output.plan_reports.is_empty());
    assert!(output.receipts.is_empty());
    assert_eq!(output.p6.settlement.applied_receipts, 0);
    assert_eq!(output.p6.settlement.applied_groups, 0);
    assert_eq!(
        output.validation.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );

    let committed =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
            .unwrap()
            .commit();

    assert!(authority.pending_player.is_empty());
    assert_eq!(authority.markets[&code].resting_orders().len(), 1);
    assert_eq!(authority.envelope_ledger.iter().count(), 1);
    assert!(matches!(
        committed.tick.events.as_slice(),
        [Event::PriceTick { seq: 1, tick: 1, code: first, .. },
        Event::PriceTick { seq: 2, tick: 1, code: second, .. },
        Event::OrderAccepted {
            seq: 3,
            account: AccountId(0),
            code: accepted,
            side: Side::Buy,
            remaining_qty: 100,
            ..
        }] if accepted == &code && first == &code && first < second
    ));
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.receipt.business_hash()
    );
}

#[test]
fn joint_b1_downstream_failure_discards_all_three_source_preparation() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    plan.state
        .execute(|candidate| {
            candidate.next_receipt_base = 1;
            Ok(())
        })
        .unwrap();
    let authority_before = authority.business_state_hash().unwrap();

    assert!(apply_tick_shadow_b1_continuous_transaction(&mut plan).is_err());

    assert_eq!(authority.business_state_hash().unwrap(), authority_before);
    assert!(plan.state.execute(|_| Ok(())).is_err());
}

#[test]
fn multi_account_plan_cancel_batch_rolls_back_after_late_continuation_overflow() {
    use crate::plans::quote_policy::QuoteAction;
    use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut authority, first) = crate::session::plan_chain_candidates_tests::execution_fixture();
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
    let mut initial = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(first.clone());
    roots.push_execution(second.clone());
    apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut initial, roots).unwrap();
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, initial)
        .unwrap()
        .commit();
    let first_old = authority
        .plans
        .plan(first.plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    let second_old = authority
        .plans
        .plan(second.plan_id)
        .unwrap()
        .active_child_order_id
        .unwrap();
    first_replace(&mut second, second_old);
    let mut first = first;
    first_replace(&mut first, first_old);

    let mut preview = authority.clone_for_tick_shadow().unwrap();
    preview.envelope_ledger = super::EnvelopeLedger::new(
        preview.next_receipt_base,
        preview.project_live_envelopes().unwrap(),
    )
    .unwrap();
    let resources = super::DecisionResourceSnapshot::seal(&preview).unwrap();
    let mut preview_p3 = super::P3ValidatorDriver::new(
        resources,
        preview.envelope_ledger.clone(),
        preview.next_order_id,
        preview.setup.config.clone(),
        build_p3_validation_context(&preview).unwrap(),
    )
    .unwrap();
    let mut preview_p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&preview).unwrap(),
    )
    .unwrap();
    let mut preview_roots = PlanChainOperationBatch::empty();
    preview_roots.push_execution(first.clone());
    preview_roots.push_execution(second.clone());
    preview_roots.set_adaptive_generation_for_test(u64::MAX - 2);
    let mut preview_chain =
        super::adaptive_plan_chain::AdaptivePlanChainCoordinator::capture_batch(
            &preview,
            preview_roots,
        )
        .unwrap();
    let ready = preview_chain.next_ready_batch(&mut preview).unwrap();
    assert_eq!(ready.len(), 2);
    assert!(ready
        .iter()
        .all(|candidate| matches!(candidate.intent(), Intent::Cancel { .. })));
    let outcomes = preview_p3.consume_round(ready).unwrap();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.operation().is_some())
            .count(),
        2
    );
    let round = preview_p4
        .apply_round(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.operation().cloned())
                .collect(),
        )
        .unwrap();
    assert_eq!(round.facts.len(), 2);

    let before = authority.business_state_hash().unwrap();
    let mut failed = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(first);
    roots.push_execution(second);
    roots.set_adaptive_generation_for_test(u64::MAX - 2);
    let error =
        match apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut failed, roots) {
            Ok(_) => panic!("late plan generation overflow must fail the tick"),
            Err(error) => error,
        };
    assert!(format!("{error:?}").contains("plan-chain command ordinal overflow"));
    assert!(failed.state.execute(|_| Ok(())).is_err());
    assert_eq!(authority.business_state_hash().unwrap(), before);
    assert_eq!(
        authority.markets[&StockCode("600888".to_owned())].resting_order_count(),
        2
    );

    fn first_replace(request: &mut crate::session::PlanExecutionRequest, old: OrderId) {
        request.decision.action = QuoteAction::Replace {
            order_id: old,
            price: Money::from_cents(901),
            qty: 100,
        };
    }
}

#[test]
fn one_account_two_stocks_replace_children_without_waiting_for_other_stock() {
    use crate::plans::quote_policy::QuoteAction;
    use crate::plans::{PlanOpen, PlanOpinion, PlanTarget, Urgency};
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let mut authority = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        47,
    )
    .unwrap();
    let (_, template) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let mut requests = Vec::new();
    for code in [
        StockCode("600888".to_owned()),
        StockCode("600889".to_owned()),
    ] {
        let plan_id = authority
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
        let mut request = template.clone();
        request.plan_id = plan_id;
        request.allocation.plan_id = plan_id;
        request.allocation.code = code;
        requests.push(request);
    }
    let mut initial = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    for request in &requests {
        roots.push_execution(request.clone());
    }
    apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut initial, roots).unwrap();
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, initial)
        .unwrap()
        .commit();

    let old_ids = requests
        .iter()
        .map(|request| {
            authority
                .plans
                .plan(request.plan_id)
                .unwrap()
                .active_child_order_id
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (request, old_id) in requests.iter_mut().zip(&old_ids) {
        request.decision.action = QuoteAction::Replace {
            order_id: *old_id,
            price: Money::from_cents(901),
            qty: 100,
        };
    }
    let mut replacement = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    for request in &requests {
        roots.push_execution(request.clone());
    }
    let output =
        apply_tick_shadow_b1_continuous_transaction_with_roots_for_test(&mut replacement, roots)
            .unwrap();
    let chain = output
        .candidates
        .candidates()
        .iter()
        .filter(|candidate| matches!(candidate.key(), P2CandidateKey::PlanChain { .. }))
        .collect::<Vec<_>>();
    assert_eq!(chain.len(), 4);
    assert!(chain[..2]
        .iter()
        .all(|candidate| matches!(candidate.intent(), Intent::Cancel { .. })));
    assert!(chain[2..]
        .iter()
        .all(|candidate| matches!(candidate.intent(), Intent::PlaceLimit { .. })));
    assert_eq!(output.plan_reports.len(), 2);
    assert!(output.plan_reports.iter().all(|report| matches!(
        report.disposition,
        PlanExecutionDisposition::Replaced { .. }
    )));
    super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, replacement)
        .unwrap()
        .commit();
    for (request, old_id) in requests.iter().zip(old_ids) {
        let active = authority
            .plans
            .plan(request.plan_id)
            .unwrap()
            .active_child_order_id
            .unwrap();
        assert_ne!(active, old_id);
        assert_eq!(
            authority.markets[&request.allocation.code].resting_order_count(),
            1
        );
    }
}

#[test]
fn prepared_joint_b1_entry_has_no_fallible_tail_after_p8() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
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

    let committed = prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert_eq!(
        committed.output.candidates.candidates()[0].key(),
        &P2CandidateKey::player(0)
    );
    assert_eq!(authority.markets[&code].resting_orders().len(), 1);
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.commit.receipt.business_hash()
    );
}

#[test]
fn b1_retail_diagnostics_cover_p3_and_p4_rejections_in_sealed_order() {
    let mut authority = player_only_session();
    authority.retail_experience.insert(
        AccountId(0),
        RetailExperienceState::without_equity_reference(),
    );
    let code = authority.markets.keys().next().unwrap().clone();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 0,
            },
        )
        .unwrap();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_100),
                qty: 100,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert!(matches!(
        authority.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(0),
                code: first_code,
                reason: crate::RejectionReason::InvalidQuantity,
            },
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(0),
                code: second_code,
                reason: crate::RejectionReason::PriceCageExceeded,
            },
        ] if first_code == &code && second_code == &code
    ));
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn b1_p4_rejection_preserves_its_allocated_causal_lifecycle() {
    use crate::diagnostics::causal::{CausalFactKind, Termination};

    let mut authority = player_only_session();
    let account = AccountId(0);
    let code = authority.markets.keys().next().unwrap().clone();
    let order_id = OrderId(authority.next_order_id);
    authority
        .enqueue_player_intent(
            account,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_100),
                qty: 100,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    let facts = authority.causal_facts();
    let submitted = facts
        .iter()
        .position(|fact| {
            matches!(
                &fact.kind,
                CausalFactKind::Submitted(origin)
                    if origin.order == order_id
                        && origin.account == account
                        && origin.code == code
                        && origin.side == Side::Buy
                        && origin.qty == 100
            )
        })
        .expect("P4 rejection must retain the allocated-ID submission");
    assert!(matches!(
        facts[submitted - 1].kind,
        CausalFactKind::Quote(_)
    ));
    assert!(matches!(
        facts[submitted + 1].kind,
        CausalFactKind::Terminated {
            order,
            account: terminated_account,
            qty: 100,
            reason: Termination::Aborted,
            ..
        } if order == order_id && terminated_account == account
    ));
    let report = authority.causal_diagnostics().unwrap();
    assert_eq!(report.submitted_qty, 100);
    assert_eq!(report.aborted_qty, 100);
    assert_eq!(report.open_qty, 0);
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn one_parallel_two_stock_round_keeps_each_causal_quote_boundary() {
    use crate::diagnostics::causal::CausalFactKind;

    let mut candidate = player_only_session();
    let codes = candidate.markets.keys().cloned().collect::<Vec<_>>();
    assert_eq!(codes.len(), 2);
    let plan = plan_tick(PhaseInput {
        session: &candidate,
    })
    .unwrap();
    let mut p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        build_p3_validation_context(&candidate).unwrap(),
    )
    .unwrap();
    let inputs = [
        (P2CandidateKey::player(0), codes[0].clone(), 990),
        (P2CandidateKey::player(1), codes[1].clone(), 980),
    ];
    let outcomes = p3
        .consume_round(
            inputs
                .iter()
                .map(|(key, code, price)| limit_candidate(key.clone(), code.clone(), *price)),
        )
        .unwrap();
    let operations = outcomes
        .iter()
        .map(|outcome| outcome.operation().unwrap().clone())
        .collect();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate).unwrap(),
    )
    .unwrap();

    let round = p4.apply_round(operations).unwrap();

    assert_eq!(round.facts.len(), 2);
    assert_eq!(round.projections.len(), 2);
    assert_eq!(round.operation_quotes.len(), 2);
    let mut chain = super::adaptive_plan_chain::AdaptivePlanChainCoordinator::capture_batch(
        &candidate,
        crate::session::plan_chain_candidates::PlanChainOperationBatch::empty(),
    )
    .unwrap();
    chain
        .project_execution_round(&mut candidate, &round)
        .unwrap();

    for ((_, code, bid_cents), outcome) in inputs.iter().zip(&outcomes) {
        let order_id = outcome.allocated_order_id().unwrap();
        let submitted = candidate
            .causal_facts()
            .iter()
            .position(|fact| {
                matches!(
                    &fact.kind,
                    CausalFactKind::Submitted(origin)
                        if origin.order == order_id && origin.code == *code
                )
            })
            .unwrap();
        assert!(matches!(
            &candidate.causal_facts()[submitted - 1].kind,
            CausalFactKind::Quote(quote)
                if quote.code == *code
                    && quote.bid_cents.is_none()
                    && quote.ask_cents.is_none()
                    && quote.bid_depth == 0
                    && quote.ask_depth == 0
        ));
        assert!(matches!(
            &candidate.causal_facts()[submitted + 1].kind,
            CausalFactKind::Quote(quote)
                if quote.code == *code
                    && quote.bid_cents == Some(*bid_cents)
                    && quote.ask_cents.is_none()
                    && quote.bid_depth == 100
                    && quote.ask_depth == 0
        ));
    }
}

#[test]
fn b1_retail_cancel_is_projected_once_and_preserves_p0_diagnostic_order() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut authority = GameSession::new(setup, 42).unwrap();
    let retail = AccountId(1);
    authority.accounts.get_mut(&retail).unwrap().strategy = None;
    let code = authority.markets.keys().next().unwrap().clone();
    let mut legacy_events = Vec::new();
    authority.seed_order_for_test(
        retail,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(980),
            qty: 100,
        },
        &mut legacy_events,
    );
    let expired = authority.markets[&code].resting_orders_for(retail)[0].id;
    authority.npc_order_lifecycles[0].expires_market_minute = authority.current_market_minute();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(970),
                qty: 100,
            },
        )
        .unwrap();
    authority.retail_experience.insert(
        AccountId(0),
        RetailExperienceState::without_equity_reference(),
    );

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert!(matches!(
        authority.last_retail_order_events(),
        [
            RetailOrderDiagnosticEvent::Canceled {
                account,
                order_id,
                remaining_qty: 100,
                ..
            },
            RetailOrderDiagnosticEvent::Submitted {
                account: AccountId(0),
                qty: 100,
                ..
            },
        ] if *account == retail && *order_id == expired
    ));
}

#[test]
fn b1_successful_retail_cancel_is_projected_from_the_p4_fact() {
    let mut authority = player_only_session();
    let account = AccountId(0);
    authority
        .retail_experience
        .insert(account, RetailExperienceState::without_equity_reference());
    let code = authority.markets.keys().next().unwrap().clone();
    let order_id = OrderId(700);
    authority
        .markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: order_id,
            side: Side::Buy,
            price: Money::from_cents(980),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: account,
            seq: 0,
        })
        .unwrap();
    authority.next_order_id = order_id.0 + 1;
    authority.hydrate_or_validate_envelope_ledger().unwrap();
    authority
        .enqueue_player_intent(
            account,
            Intent::Cancel {
                code: code.clone(),
                id: order_id,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert!(matches!(
        authority.last_retail_order_events(),
        [RetailOrderDiagnosticEvent::Canceled {
            account: AccountId(0),
            code: canceled_code,
            order_id: OrderId(700),
            remaining_qty: 100,
        }] if canceled_code == &code
    ));
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn b1_successful_cancel_records_termination_and_post_quote() {
    use crate::diagnostics::causal::{CausalFactKind, Termination};

    let mut authority = player_only_session();
    let account = AccountId(0);
    let code = authority.markets.keys().next().unwrap().clone();
    let order = Order {
        id: OrderId(700),
        side: Side::Buy,
        price: Money::from_cents(980),
        qty: 100,
        original_qty: 100,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: account,
        seq: 0,
    };
    authority
        .markets
        .get_mut(&code)
        .unwrap()
        .place(order.clone())
        .unwrap();
    authority.causal_submitted(&order, &code);
    authority.next_order_id = 701;
    authority.hydrate_or_validate_envelope_ledger().unwrap();
    authority
        .enqueue_player_intent(
            account,
            Intent::Cancel {
                code: code.clone(),
                id: order.id,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    let facts = authority.causal_facts();
    let terminated = facts
        .iter()
        .position(|fact| {
            matches!(
                fact.kind,
                CausalFactKind::Terminated {
                    order: OrderId(700),
                    qty: 100,
                    reason: Termination::Voluntary,
                    ..
                }
            )
        })
        .unwrap();
    assert!(matches!(
        facts[terminated + 1].kind,
        CausalFactKind::Quote(_)
    ));
    let report = authority.causal_diagnostics().unwrap();
    assert_eq!(report.canceled_qty, 100);
    assert_eq!(report.open_qty, 0);
}

#[test]
fn b1_consumed_parent_submission_is_not_applied_again_at_final_projection() {
    let mut authority = player_only_session();
    let account = AccountId(0);
    let code = authority.markets.keys().next().unwrap().clone();
    authority.parent_orders.entry(account).or_default().insert(
        code.clone(),
        ParentOrderPlan {
            code: code.clone(),
            side: Side::Buy,
            target_qty: 100,
            filled_qty: 0,
            child_qty: 100,
            active_child_order_id: None,
            active_child_remaining_qty: None,
            linked_plan_id: None,
            limit_price: Money::from_cents(980),
            expires_market_minute: 240,
        },
    );
    authority
        .enqueue_player_intent(
            account,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(980),
                qty: 100,
            },
        )
        .unwrap();

    prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    let parent = &authority.parent_orders[&account][&code];
    assert_eq!(parent.active_child_order_id, Some(OrderId(1)));
    assert_eq!(parent.active_child_remaining_qty, Some(100));
}

#[test]
fn b1_execution_round_rejects_foreign_swapped_and_duplicate_p4_fact_identities() {
    for corruption in [
        FactIdentityCorruption::Foreign,
        FactIdentityCorruption::Swapped,
        FactIdentityCorruption::Duplicate,
    ] {
        let (outcomes, mut round) = execution_identity_fixture();
        validate_execution_round_for_test(&outcomes, &round).unwrap();
        corrupt_fact_identities(&mut round, corruption);
        let error = validate_execution_round_for_test(&outcomes, &round)
            .expect_err("malformed P4 identity must fail before candidate projection");
        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
    }
}

#[derive(Clone, Copy, Debug)]
enum FactIdentityCorruption {
    Foreign,
    Swapped,
    Duplicate,
}

fn execution_identity_fixture() -> (Vec<P3ConsumeOutcome>, ContinuousExecutionRound) {
    let candidate = player_only_session();
    let code = candidate.markets.keys().next().unwrap().clone();
    let plan = plan_tick(PhaseInput {
        session: &candidate,
    })
    .unwrap();
    let mut p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        build_p3_validation_context(&candidate).unwrap(),
    )
    .unwrap();
    let outcomes = p3
        .consume_round([
            limit_candidate(P2CandidateKey::player(0), code.clone(), 980),
            limit_candidate(P2CandidateKey::player(1), code, 970),
        ])
        .unwrap();
    let operations = outcomes
        .iter()
        .map(|outcome| outcome.operation().unwrap().clone())
        .collect();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate).unwrap(),
    )
    .unwrap();
    let round = p4.apply_round(operations).unwrap();
    assert_eq!(round.facts.len(), 2);
    (outcomes, round)
}

fn limit_candidate(key: P2CandidateKey, code: StockCode, price_cents: i64) -> P2Candidate {
    P2Candidate::new(
        key,
        AccountId(0),
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(price_cents),
            qty: 100,
        },
    )
}

fn corrupt_fact_identities(
    round: &mut ContinuousExecutionRound,
    corruption: FactIdentityCorruption,
) {
    match corruption {
        FactIdentityCorruption::Foreign => {
            round.facts[0].candidate_key = P2CandidateKey::plan_chain(99);
        }
        FactIdentityCorruption::Swapped => {
            let first_key = round.facts[0].candidate_key.clone();
            let first_sealed = round.facts[0].sealed_index;
            round.facts[0].candidate_key = round.facts[1].candidate_key.clone();
            round.facts[0].sealed_index = round.facts[1].sealed_index;
            round.facts[1].candidate_key = first_key;
            round.facts[1].sealed_index = first_sealed;
        }
        FactIdentityCorruption::Duplicate => {
            round.facts[1] = round.facts[0].clone();
        }
    }
}
