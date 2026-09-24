use super::b1_continuous_transaction::{
    apply_initial_candidate_stream_for_test, apply_open_order_feedback_for_test,
    apply_tick_shadow_b1_continuous_transaction,
    apply_tick_shadow_b1_continuous_transaction_with_roots_for_test, prepare_b1_continuous_tick,
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
fn b1_player_rejections_and_acceptance_keep_queue_order_and_order_identity() {
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
    assert_eq!(
        committed
            .output
            .validation
            .results()
            .iter()
            .map(|result| result.key().clone())
            .collect::<Vec<_>>(),
        (0..3).map(P2CandidateKey::player).collect::<Vec<_>>()
    );
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
    assert!(matches!(
        player_events.as_slice(),
        [
            Event::IntentRejected { code: first, reason: RejectionReason::InvalidQuantity, .. },
            Event::IntentRejected { code: second, reason: RejectionReason::UnknownStock, .. },
            Event::OrderAccepted { code: third, id, remaining_qty: 100, .. },
        ] if first == &code
            && second == &StockCode("999999".to_owned())
            && third == &code
            && *id == first_order_id
    ));
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
    authority.route_intent(
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
fn npc_cancel_feedback_releases_slot_for_later_ready_request_without_cash_refund() {
    assert_cancel_releases_slots_only(Side::Buy);
    assert_cancel_releases_slots_only(Side::Sell);
}

#[test]
fn b1_feedback_rejects_foreign_swapped_and_duplicate_p4_fact_identities_atomically() {
    for corruption in [
        FactIdentityCorruption::Foreign,
        FactIdentityCorruption::Swapped,
        FactIdentityCorruption::Duplicate,
    ] {
        let (candidate, mut p3, outcomes, mut round) = feedback_identity_fixture();
        corrupt_fact_identities(&mut round, corruption);
        let candidate_business_before = candidate.business_state_hash().unwrap();
        let candidate_session_before = candidate.session_state_hash().unwrap();
        let p3_before = p3.checkpoint();
        let output_before = p3.output().clone();

        let error = apply_open_order_feedback_for_test(&mut p3, &outcomes, &round)
            .expect_err("malformed P4 identity must fail before candidate projection");

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(p3.checkpoint(), p3_before);
        assert_eq!(p3.output(), &output_before);
        assert_eq!(
            candidate.business_state_hash().unwrap(),
            candidate_business_before
        );
        assert_eq!(
            candidate.session_state_hash().unwrap(),
            candidate_session_before
        );
    }
}

fn assert_cancel_releases_slots_only(resting_side: Side) {
    let npc = AccountId(1);
    let player = AccountId(0);
    let mut candidate =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = candidate.markets.keys().next().unwrap().clone();
    if resting_side == Side::Sell {
        candidate
            .accounts
            .get_mut(&npc)
            .unwrap()
            .grant_position(code.clone(), 100, Money::from_cents(99_000))
            .unwrap();
    }
    let resting_order = OrderId(700);
    let placed = candidate
        .markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: resting_order,
            side: resting_side,
            price: Money::from_cents(990),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: npc,
            seq: 0,
        })
        .unwrap();
    assert!(placed.trades.is_empty());
    assert!(placed.resting.is_some());
    candidate.next_order_id = resting_order.0 + 1;
    candidate.hydrate_or_validate_envelope_ledger().unwrap();

    let plan = plan_tick(PhaseInput {
        session: &candidate,
    })
    .unwrap();
    let market = &candidate.markets[&code];
    let category = candidate
        .setup
        .stocks
        .iter()
        .find(|stock| stock.code == code)
        .unwrap()
        .category;
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                category,
                market.up_stop().unwrap(),
                market.down_stop().unwrap(),
            ),
        )],
        1,
        [(npc, 1), (player, 0)],
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    )
    .unwrap();
    let mut p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )
    .unwrap();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate).unwrap(),
    )
    .unwrap();

    let initial = P2CandidateBatch::new(vec![
        P2Candidate::new(
            P2CandidateKey::npc(npc, 0),
            npc,
            Intent::Cancel {
                code: code.clone(),
                id: resting_order,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(980),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let before_stream = p3.checkpoint();
    let rounds = apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).unwrap();

    assert_eq!(rounds.len(), 1);
    let after_stream = p3.checkpoint();
    assert_eq!(after_stream.global_open_orders(), 0);
    assert_eq!(after_stream.account_open_orders(npc), Some(0));
    assert_eq!(after_stream.account_open_orders(player), Some(0));
    assert_eq!(
        after_stream.remaining_cash(npc),
        before_stream.remaining_cash(npc),
        "cancel feedback must not return sealed cash budget"
    );
    assert_eq!(
        after_stream.remaining_sellable(npc, &code),
        before_stream.remaining_sellable(npc, &code),
        "cancel feedback must not return sealed share budget"
    );
    assert_eq!(p3.output().accepted().count(), 1);
    assert!(matches!(
        p3.output().results()[1],
        P3CandidateResult::Rejected {
            reason: RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    let later = p3
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            player,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(980),
                qty: 100,
            },
        ))
        .unwrap();
    assert!(matches!(later.result(), P3CandidateResult::Accepted { .. }));
    assert_eq!(p3.checkpoint().global_open_orders(), 1);
}

#[derive(Clone, Copy, Debug)]
enum FactIdentityCorruption {
    Foreign,
    Swapped,
    Duplicate,
}

fn feedback_identity_fixture() -> (
    GameSession,
    P3ValidatorDriver,
    Vec<P3ConsumeOutcome>,
    ContinuousExecutionRound,
) {
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
    (candidate, p3, outcomes, round)
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
