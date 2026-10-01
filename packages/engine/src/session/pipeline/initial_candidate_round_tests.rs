use super::account_validation_context::build_account_validation_context;
use super::continuous_matching::IncrementalContinuousStockCoordinator;
use super::continuous_matching_adapter::prepare_incremental_continuous_inputs;
use super::continuous_tick_transaction::apply_initial_candidate_stream_for_test;
use super::stock_auction::auction_day_end::IncrementalAuctionStockCoordinator;
use super::stock_auction_adapter::prepare_incremental_auction_inputs;
use super::*;
use crate::{AccountId, Intent, Money, Side};

fn fixture() -> (GameSession, AccountValidatorDriver, IntentCandidateBatch) {
    let session = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let plan = plan_tick(PhaseInput { session: &session }).unwrap();
    let validator = AccountValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_account_validation_context(&session).unwrap(),
    )
    .unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let initial = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::npc(AccountId(1), 0),
            AccountId(1),
            Intent::PlaceLimit {
                code: codes[0].clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: codes[1].clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    (session, validator, initial)
}

#[test]
fn continuous_initial_round_batches_independent_accounts_and_stocks() {
    let (session, mut validator, initial) = fixture();
    let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds =
        apply_initial_candidate_stream_for_test(&mut validator, &mut stock_execution, &initial)
            .unwrap();

    assert_eq!(rounds.len(), 1, "independent stocks must reach P4 together");
    assert_eq!(rounds[0].projections.len(), 2);
    assert_eq!(rounds[0].facts.len(), 2);
    assert_eq!(validator.last_round_account_shards(), 2);

    assert_eq!(
        validator
            .output()
            .identities()
            .map(|(key, sealed, id)| (key.clone(), sealed, id))
            .collect::<Vec<_>>(),
        vec![
            (
                IntentCandidateKey::npc(AccountId(1), 0),
                0,
                Some(crate::OrderId(session.next_order_id))
            ),
            (
                IntentCandidateKey::player(0),
                1,
                Some(crate::OrderId(session.next_order_id + 1))
            ),
        ]
    );
}

#[test]
fn pre_open_initial_round_batches_independent_accounts_and_stocks_before_phase_rejection() {
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let (mut session, _, initial) = fixture();
                session.setup.auction_ticks = 900;
                session.setup.ticks_per_day = 15_300;
                session.tick = 600;
                assert_eq!(session.phase(), crate::TradingPhase::PreOpen);
                let mut validator = build_validator(&session);
                let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
                    prepare_incremental_continuous_inputs(&session).unwrap(),
                )
                .unwrap();

                let rounds = super::pre_open_transaction::apply_initial_candidate_stream(
                    &mut validator,
                    &mut stock_execution,
                    &initial,
                )
                .unwrap();

                assert_eq!(rounds.len(), 1);
                assert_eq!(rounds[0].projections.len(), 2);
                assert_eq!(rounds[0].facts.len(), 2);
                assert_eq!(validator.last_round_account_shards(), 2);

                assert!(rounds[0].facts.iter().all(|fact| matches!(
                    fact.outcome,
                    super::continuous_matching::ContinuousExecutionOutcome::Place {
                        fact: super::continuous_matching::ContinuousPlaceFact::Rejected {
                            reason: crate::RejectionReason::AuctionOrderEntryClosed,
                            ..
                        },
                        ..
                    }
                )));
                (rounds[0].facts.clone(), validator.checkpoint())
            })
    };

    assert_eq!(run(1), run(2));
}

#[test]
fn initial_order_id_overflow_does_not_leave_partial_account_validation_or_matching_work() {
    let (mut session, _, initial) = fixture();
    session.next_order_id = u64::MAX - 1;
    let plan = plan_tick(PhaseInput { session: &session }).unwrap();
    let mut validator = AccountValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_account_validation_context(&session).unwrap(),
    )
    .unwrap();
    let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let before = validator.checkpoint();

    assert!(apply_initial_candidate_stream_for_test(
        &mut validator,
        &mut stock_execution,
        &initial
    )
    .is_err());

    assert_eq!(validator.checkpoint(), before);
    let finish = stock_execution.finish().unwrap();
    assert!(finish
        .workers
        .iter()
        .all(|stock| stock.market.resting_orders().is_empty()));
}

#[test]
fn auction_initial_round_batches_accounts_and_stocks_without_clearing_early() {
    let (mut session, _, initial) = fixture();
    session.setup.auction_ticks = 900;
    session.setup.ticks_per_day = 15_300;
    let mut validator = build_validator(&session);
    let mut stock_execution = IncrementalAuctionStockCoordinator::from_post_expiry(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds = super::auction_tick_transaction::apply_initial_candidate_stream(
        &mut validator,
        &mut stock_execution,
        &initial,
    )
    .unwrap();

    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0].projections.len(), 2);
    assert_eq!(rounds[0].facts.len(), 2);
    assert!(
        rounds[0].receipts.is_empty(),
        "the initial batch does not clear the auction"
    );
    assert_eq!(validator.last_round_account_shards(), 2);
}

fn build_validator(session: &GameSession) -> AccountValidatorDriver {
    let plan = plan_tick(PhaseInput { session }).unwrap();
    AccountValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_account_validation_context(session).unwrap(),
    )
    .unwrap()
}

#[test]
fn independent_funded_accounts_accept_orders_without_count_policy() {
    let (session, mut validator, initial) = fixture();
    let outcomes = validator
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();
    assert_eq!(validator.last_round_account_shards(), 2);
    assert!(outcomes
        .iter()
        .all(|outcome| matches!(outcome.result(), CandidateValidationResult::Accepted { .. })));
    assert_eq!(
        validator.output().next_order_id_after(),
        session.next_order_id + 2
    );

    let mut split = build_validator(&session);
    let split_outcomes = initial
        .candidates()
        .iter()
        .cloned()
        .map(|candidate| split.consume(candidate).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes, split_outcomes);
    assert_eq!(validator.checkpoint(), split.checkpoint());
}

#[test]
fn parallel_account_errors_report_first_canonical_candidate_without_mutation() {
    let (session, mut validator, initial) = fixture();
    let first = IntentCandidate::new(
        IntentCandidateKey::npc(AccountId(9), 0),
        AccountId(9),
        initial.candidates()[0].intent().clone(),
    );
    let second = IntentCandidate::new(
        IntentCandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: session.markets.keys().next().unwrap().clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(i64::MAX)),
            qty: 100,
        },
    );
    let expected = build_validator(&session)
        .consume(first.clone())
        .unwrap_err();
    let second_error = build_validator(&session)
        .consume(second.clone())
        .unwrap_err();
    assert_ne!(
        expected, second_error,
        "the two failures must be distinguishable"
    );
    let checkpoint = validator.checkpoint();

    // Account 0 is visited before 9 by the shard map, but NPC 9 is earlier canonically.
    assert_eq!(
        validator.consume_round([first, second]).unwrap_err(),
        expected
    );
    assert_eq!(validator.checkpoint(), checkpoint);
}

#[test]
fn initial_batch_full_fill_completes_linked_parent_before_later_manual_acceptance() {
    use super::adaptive_plan_chain::AdaptivePlanChainCoordinator;
    use crate::session::plan_chain_candidates::PlanChainOperationBatch;

    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    let plan_id = request.plan_id;
    let mut setup = PlanChainOperationBatch::empty();
    setup.push_execution(request);
    commit_injected_plan_roots_for_test(&mut session, setup);
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let mut validator = build_validator(&session);
    let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let initial = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(1),
            AccountId(1),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let cash_before = session.accounts[&AccountId(1)].cash;

    let mut rounds =
        apply_initial_candidate_stream_for_test(&mut validator, &mut stock_execution, &initial)
            .unwrap();
    assert_eq!(rounds.len(), 1);
    chain
        .project_execution_round(&mut session, &mut rounds[0])
        .unwrap();

    assert!(session.plans.plan(plan_id).unwrap().is_terminal());
    assert!(!session.parent_orders.contains_key(&AccountId(1)));
    assert!(session.pending_plan_events.is_empty());
    assert_eq!(session.markets[&code].resting_order_count(), 1);
    assert_eq!(
        session.accounts[&AccountId(1)].cash,
        cash_before,
        "projection does not settle cash"
    );
    assert!(chain.next_ready_batch(&mut session).unwrap().is_empty());
    let completion = chain.finish().unwrap();
    assert_eq!(completion.consumed.operations.len(), 2);
    assert_eq!(completion.consumed.receipts.len(), 2);
}

#[test]
fn batched_resting_acceptances_keep_each_operations_market_quote_and_order() {
    let (mut session, _, _) = fixture();
    let code = session.markets.keys().next().unwrap().clone();
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(99_000))
        .unwrap();
    let mut validator = build_validator(&session);
    let initial = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::npc(AccountId(1), 0),
            AccountId(1),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(1),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(980)),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds =
        apply_initial_candidate_stream_for_test(&mut validator, &mut stock_execution, &initial)
            .unwrap();

    assert_eq!(rounds.len(), 1);
    let projection = &rounds[0].projections[&code];
    assert_eq!(
        projection
            .acceptance_quotes
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    let first = &projection.acceptance_quotes[&0];
    assert_eq!(first.order.id, crate::OrderId(session.next_order_id));
    assert_eq!(first.order.qty, 100);
    assert_eq!(first.last_price, Money::from_cents(1_000));
    assert_eq!(first.best_bid, Some(Money::from_cents(990)));
    assert_eq!(first.best_ask, None);
    let last = &projection.acceptance_quotes[&2];
    assert_eq!(last.order.id, crate::OrderId(session.next_order_id + 2));
    assert_eq!(last.last_price, Money::from_cents(990));
    assert_eq!(last.best_bid, Some(Money::from_cents(980)));
    assert_eq!(last.best_ask, None);
    let mut projected_market = session.markets[&code].clone();
    projected_market
        .apply_changed_orders(projection.market_delta.as_ref().unwrap().clone())
        .unwrap();
    assert_eq!(projected_market.resting_orders()[0].id, last.order.id);
    assert_eq!(projected_market.resting_order_count(), 1);
    assert_eq!(projected_market.best_bid(), last.best_bid);
    assert_ne!(projected_market.last_price(), first.last_price);
}

#[test]
fn account_shards_preserve_cross_stock_budget_competition_and_continuation_ids() {
    let (mut session, _, initial) = fixture();
    let required = crate::session::buy_order_reservation(
        &session.setup.config,
        Money::from_cents(990),
        100,
        Money::ZERO,
    )
    .unwrap();
    session.accounts.get_mut(&AccountId(1)).unwrap().cash = required;
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let npc_second = IntentCandidate::new(
        IntentCandidateKey::npc(AccountId(1), 1),
        AccountId(1),
        Intent::PlaceLimit {
            code: codes[1].clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(990)),
            qty: 100,
        },
    );
    let mut validator = build_validator(&session);
    let initial = IntentCandidateBatch::new(vec![
        initial.candidates()[0].clone(),
        npc_second,
        initial.candidates()[1].clone(),
    ])
    .unwrap();
    let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds =
        apply_initial_candidate_stream_for_test(&mut validator, &mut stock_execution, &initial)
            .unwrap();

    assert_eq!(rounds.len(), 1);
    assert_eq!(validator.last_round_account_shards(), 2);
    assert_eq!(validator.output().accepted().count(), 2);
    assert_eq!(
        validator.output().rejected().collect::<Vec<_>>(),
        vec![(
            &IntentCandidateKey::npc(AccountId(1), 1),
            &crate::RejectionReason::InsufficientCash
        ),]
    );
    assert_eq!(
        validator.checkpoint().remaining_cash(AccountId(1)),
        Some(Money::ZERO)
    );
    let continuation = validator
        .consume(IntentCandidate::new(
            IntentCandidateKey::plan_chain(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: codes[0].clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(980)),
                qty: 100,
            },
        ))
        .unwrap();
    assert_eq!(continuation.sealed_index(), 3);
    assert_eq!(
        continuation.allocated_order_id(),
        Some(crate::OrderId(session.next_order_id + 2))
    );
}

#[test]
fn continuous_execution_accepts_independent_stock_facts_in_another_output_order() {
    let (session, mut validator, initial) = fixture();
    let outcomes = validator
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();
    let mut stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let mut round = stock_execution
        .apply_round(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.operation().cloned())
                .collect(),
        )
        .unwrap();
    assert_eq!(round.facts.len(), 2);
    round.facts.reverse();

    super::continuous_tick_transaction::validate_execution_round_for_test(&outcomes, &round)
        .unwrap();
}

#[test]
fn multi_stock_post_worker_failure_keeps_authority_and_discards_tick_shadow() {
    for auction in [false, true] {
        let (mut authority, _, _) = fixture();
        authority.attention_queue.clear();
        if auction {
            authority.setup.auction_ticks = 900;
            authority.setup.ticks_per_day = 15_300;
        }
        let codes = authority.markets.keys().cloned().collect::<Vec<_>>();
        for code in codes {
            authority
                .enqueue_player_intent(
                    AccountId(0),
                    Intent::PlaceLimit {
                        code,
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                        qty: 100,
                    },
                )
                .unwrap();
        }
        let authority_before = (
            authority.business_state_hash().unwrap(),
            authority.session_state_hash().unwrap(),
        );
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
        if auction {
            assert!(
                super::auction_tick_transaction::apply_tick_shadow_auction_transaction(&mut plan)
                    .is_err()
            );
        } else {
            assert!(
                super::continuous_tick_transaction::apply_tick_shadow_continuous_transaction(
                    &mut plan
                )
                .is_err()
            );
        }

        assert_eq!(
            (
                authority.business_state_hash().unwrap(),
                authority.session_state_hash().unwrap()
            ),
            authority_before
        );
        assert!(plan.state.execute(|_| Ok(())).is_err());
        assert!(plan.event_outbox.is_empty());
        assert!(plan.receipt_keys.is_empty());
    }
}
