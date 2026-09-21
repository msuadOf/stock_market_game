use super::b1_continuous_transaction::apply_initial_candidate_stream_for_test;
use super::p3_context::build_p3_validation_context;
use super::p4_continuous::IncrementalContinuousStockCoordinator;
use super::p4_continuous_adapter::prepare_incremental_continuous_inputs;
use super::stock_auction::b2_auction_day_end::IncrementalAuctionStockCoordinator;
use super::stock_auction_adapter::prepare_incremental_auction_inputs;
use super::*;
use crate::{AccountId, Intent, Money, Side};

fn fixture() -> (GameSession, P3ValidatorDriver, P2CandidateBatch) {
    let session = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let plan = plan_tick(PhaseInput { session: &session }).unwrap();
    let p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(&session).unwrap(),
    )
    .unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let initial = P2CandidateBatch::from_canonical(vec![
        P2Candidate::new(
            P2CandidateKey::npc(AccountId(1), 0),
            AccountId(1),
            Intent::PlaceLimit {
                code: codes[0].clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: codes[1].clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    (session, p3, initial)
}

#[test]
fn continuous_initial_round_batches_independent_accounts_and_stocks() {
    let (session, mut p3, initial) = fixture();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds = apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).unwrap();

    assert_eq!(rounds.len(), 1, "independent stocks must reach P4 together");
    assert_eq!(rounds[0].projections.len(), 2);
    assert_eq!(rounds[0].facts.len(), 2);
    assert_eq!(p3.last_round_account_shards(), 2);
    assert_eq!(p3.checkpoint().feedback_count(), 2);
    assert_eq!(p3.checkpoint().global_open_orders(), 2);
    assert_eq!(
        p3.output()
            .identities()
            .map(|(key, sealed, id)| (key.clone(), sealed, id))
            .collect::<Vec<_>>(),
        vec![
            (
                P2CandidateKey::npc(AccountId(1), 0),
                0,
                Some(crate::OrderId(session.next_order_id))
            ),
            (
                P2CandidateKey::player(0),
                1,
                Some(crate::OrderId(session.next_order_id + 1))
            ),
        ]
    );
}

#[test]
fn initial_order_id_overflow_does_not_leave_partial_p3_or_p4_work() {
    let (mut session, _, initial) = fixture();
    session.next_order_id = u64::MAX - 1;
    let plan = plan_tick(PhaseInput { session: &session }).unwrap();
    let mut p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(&session).unwrap(),
    )
    .unwrap();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let before = p3.checkpoint();

    assert!(apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).is_err());

    assert_eq!(p3.checkpoint(), before);
    let finish = p4.finish().unwrap();
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
    let mut p3 = validator(&session);
    let mut p4 = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds =
        super::b2_auction_transaction::apply_initial_candidate_stream(&mut p3, &mut p4, &initial)
            .unwrap();

    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0].projections.len(), 2);
    assert_eq!(rounds[0].facts.len(), 2);
    assert!(
        rounds[0].receipts.is_empty(),
        "the initial batch does not clear the auction"
    );
    assert_eq!(p3.last_round_account_shards(), 2);
    assert_eq!(p3.checkpoint().global_open_orders(), 2);
    assert_eq!(p3.checkpoint().feedback_count(), 2);
}

fn validator(session: &GameSession) -> P3ValidatorDriver {
    let plan = plan_tick(PhaseInput { session }).unwrap();
    P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(session).unwrap(),
    )
    .unwrap()
}

fn limited_validator(session: &GameSession, limits: P3OpenOrderLimits) -> P3ValidatorDriver {
    let plan = plan_tick(PhaseInput { session }).unwrap();
    let context = P3ValidationContext::new(
        session.setup.stocks.iter().map(|stock| {
            (
                stock.code.clone(),
                P3StockValidation::new(
                    stock.category,
                    Money::from_cents(1_100),
                    Money::from_cents(900),
                ),
            )
        }),
        0,
        session.accounts.keys().map(|account| (*account, 0)),
        limits,
    )
    .unwrap();
    P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        context,
    )
    .unwrap()
}

#[test]
fn global_slot_competition_uses_canonical_order_and_skips_prior_p3_rejections() {
    let (session, _, initial) = fixture();
    let mut p3 = limited_validator(
        &session,
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    );
    let candidates = vec![
        P2Candidate::new(
            P2CandidateKey::npc(AccountId(1), 0),
            AccountId(1),
            Intent::PlaceLimit {
                code: crate::StockCode("unknown".into()),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::npc(AccountId(1), 1),
            AccountId(1),
            initial.candidates()[0].intent().clone(),
        ),
        initial.candidates()[1].clone(),
    ];
    let before_player_cash = p3.checkpoint().remaining_cash(AccountId(0));

    let outcomes = p3.consume_round(candidates).unwrap();

    assert!(matches!(
        outcomes[0].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::UnknownStock,
            ..
        }
    ));
    assert_eq!(
        outcomes[1].allocated_order_id(),
        Some(crate::OrderId(session.next_order_id))
    );
    assert!(matches!(
        outcomes[2].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    assert_eq!(p3.checkpoint().global_open_orders(), 1);
    assert_eq!(
        p3.checkpoint().remaining_cash(AccountId(0)),
        before_player_cash
    );
}

#[test]
fn parallel_account_errors_report_first_canonical_candidate_without_mutation() {
    let (session, mut p3, initial) = fixture();
    let first = P2Candidate::new(
        P2CandidateKey::npc(AccountId(9), 0),
        AccountId(9),
        initial.candidates()[0].intent().clone(),
    );
    let second = P2Candidate::new(
        P2CandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: session.markets.keys().next().unwrap().clone(),
            side: Side::Buy,
            price: Money::from_cents(i64::MAX),
            qty: 100,
        },
    );
    let expected = validator(&session).consume(first.clone()).unwrap_err();
    let second_error = validator(&session).consume(second.clone()).unwrap_err();
    assert_ne!(
        expected, second_error,
        "the two failures must be distinguishable"
    );
    let checkpoint = p3.checkpoint();

    // Account 0 is visited before 9 by the shard map, but NPC 9 is earlier canonically.
    assert_eq!(p3.consume_round([first, second]).unwrap_err(), expected);
    assert_eq!(p3.checkpoint(), checkpoint);
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
    session.consume_plan_chain_operation_batch(setup, &mut Vec::new());
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let mut p3 = validator(&session);
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, PlanChainOperationBatch::empty())
            .unwrap();
    let initial = P2CandidateBatch::from_canonical(vec![
        P2Candidate::new(
            P2CandidateKey::player(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(900),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(1),
            AccountId(1),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let cash_before = session.accounts[&AccountId(1)].cash;

    let rounds = apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).unwrap();
    assert_eq!(rounds.len(), 1);
    chain
        .project_execution_round(&mut session, &rounds[0])
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
    assert!(chain.next_candidate(&mut session).unwrap().is_none());
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
    let mut p3 = validator(&session);
    let initial = P2CandidateBatch::from_canonical(vec![
        P2Candidate::new(
            P2CandidateKey::npc(AccountId(1), 0),
            AccountId(1),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(990),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(1),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(980),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds = apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).unwrap();

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
    assert_eq!(projection.market.resting_orders()[0].id, last.order.id);
    assert_eq!(projection.market.resting_order_count(), 1);
    assert_eq!(projection.market.best_bid(), last.best_bid);
    assert_ne!(projection.market.last_price(), first.last_price);
}

#[test]
fn p4_rejection_releases_account_slot_for_next_initial_round_without_cash_refund() {
    for auction in [false, true] {
        let (mut session, _, _) = fixture();
        if auction {
            session.setup.auction_ticks = 900;
            session.setup.ticks_per_day = 15_300;
        }
        let codes = session.markets.keys().cloned().collect::<Vec<_>>();
        let candidates = P2CandidateBatch::from_canonical(vec![
            P2Candidate::new(
                P2CandidateKey::player(0),
                AccountId(0),
                Intent::PlaceLimit {
                    code: codes[0].clone(),
                    side: Side::Buy,
                    price: Money::from_cents(2_000),
                    qty: 100,
                },
            ),
            P2Candidate::new(
                P2CandidateKey::player(1),
                AccountId(0),
                Intent::PlaceLimit {
                    code: codes[1].clone(),
                    side: Side::Buy,
                    price: Money::from_cents(990),
                    qty: 100,
                },
            ),
        ])
        .unwrap();
        let mut p3 = limited_validator(
            &session,
            P3OpenOrderLimits {
                global: 4,
                per_account: 1,
            },
        );
        let cash_before = p3.checkpoint().remaining_cash(AccountId(0)).unwrap();
        let rounds = if auction {
            let mut p4 = IncrementalAuctionStockCoordinator::from_post_p0(
                prepare_incremental_auction_inputs(&session).unwrap(),
            )
            .unwrap();
            super::b2_auction_transaction::apply_initial_candidate_stream(
                &mut p3,
                &mut p4,
                &candidates,
            )
            .unwrap()
            .len()
        } else {
            let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
                prepare_incremental_continuous_inputs(&session).unwrap(),
            )
            .unwrap();
            apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &candidates)
                .unwrap()
                .len()
        };
        assert_eq!(rounds, 2);
        assert_eq!(p3.checkpoint().global_open_orders(), 1);
        assert_eq!(p3.checkpoint().account_open_orders(AccountId(0)), Some(1));
        assert_eq!(p3.output().next_order_id_after(), session.next_order_id + 2);
        let reserved = p3
            .output()
            .drafts()
            .iter()
            .try_fold(Money::ZERO, |total, draft| total.add(draft.required().cash))
            .unwrap();
        assert_eq!(
            p3.checkpoint().remaining_cash(AccountId(0)).unwrap(),
            cash_before.sub(reserved).unwrap()
        );
    }
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
    let npc_second = P2Candidate::new(
        P2CandidateKey::npc(AccountId(1), 1),
        AccountId(1),
        Intent::PlaceLimit {
            code: codes[1].clone(),
            side: Side::Buy,
            price: Money::from_cents(990),
            qty: 100,
        },
    );
    let mut p3 = validator(&session);
    let initial = P2CandidateBatch::from_canonical(vec![
        initial.candidates()[0].clone(),
        npc_second,
        initial.candidates()[1].clone(),
    ])
    .unwrap();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();

    let rounds = apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).unwrap();

    assert_eq!(rounds.len(), 1);
    assert_eq!(p3.last_round_account_shards(), 2);
    assert_eq!(p3.output().accepted().count(), 2);
    assert_eq!(
        p3.output().rejected().collect::<Vec<_>>(),
        vec![(
            &P2CandidateKey::npc(AccountId(1), 1),
            &crate::RejectionReason::InsufficientCash
        ),]
    );
    assert_eq!(
        p3.checkpoint().remaining_cash(AccountId(1)),
        Some(Money::ZERO)
    );
    let continuation = p3
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            AccountId(0),
            Intent::PlaceLimit {
                code: codes[0].clone(),
                side: Side::Buy,
                price: Money::from_cents(980),
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
fn batch_feedback_failure_on_a_later_operation_discards_earlier_feedback() {
    let (session, mut p3, initial) = fixture();
    let outcomes = p3
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let mut round = p4
        .apply_round(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.operation().cloned())
                .collect(),
        )
        .unwrap();
    let last_delta = round.open_order_deltas.last_mut().unwrap();
    last_delta.account = AccountId(999);
    let before = p3.checkpoint();
    let output = p3.output().clone();

    assert!(
        super::b1_continuous_transaction::apply_open_order_feedback_for_test(
            &mut p3, &outcomes, &round,
        )
        .is_err()
    );

    assert_eq!(p3.checkpoint(), before);
    assert_eq!(p3.output(), &output);
}

#[test]
fn auction_batch_feedback_failure_discards_earlier_feedback() {
    let (mut session, _, initial) = fixture();
    session.setup.auction_ticks = 900;
    session.setup.ticks_per_day = 15_300;
    let mut p3 = validator(&session);
    let outcomes = p3
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();
    let mut p4 = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();
    let mut round = p4
        .apply_round(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.operation().cloned())
                .collect(),
        )
        .unwrap();
    round.open_order_deltas.last_mut().unwrap().account = AccountId(999);
    let before = p3.checkpoint();

    assert!(
        super::b2_auction_transaction::apply_open_order_feedback(&mut p3, &outcomes, &round)
            .is_err()
    );

    assert_eq!(p3.checkpoint(), before);
}

#[test]
fn multi_stock_post_worker_failure_keeps_authority_and_tick_shadow_unchanged() {
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
                        price: Money::from_cents(990),
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
        let shadow_before = plan
            .state
            .execute(|candidate| {
                Ok((
                    candidate.business_state_hash()?,
                    candidate.session_state_hash()?,
                ))
            })
            .unwrap();

        if auction {
            assert!(
                super::b2_auction_transaction::apply_tick_shadow_b2_auction_transaction(&mut plan)
                    .is_err()
            );
        } else {
            assert!(
                super::b1_continuous_transaction::apply_tick_shadow_b1_continuous_transaction(
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
        assert_eq!(
            plan.state
                .execute(|candidate| Ok((
                    candidate.business_state_hash()?,
                    candidate.session_state_hash()?
                )))
                .unwrap(),
            shadow_before
        );
        assert!(plan.event_outbox.is_empty());
        assert!(plan.receipt_keys.is_empty());
    }
}

#[test]
fn multi_stock_transaction_bytes_are_identical_across_thread_budgets() {
    for auction in [false, true] {
        let reference = transaction_fingerprint(1, auction);
        for threads in [2, 4] {
            assert_eq!(transaction_fingerprint(threads, auction), reference);
        }
    }
}

fn transaction_fingerprint(
    threads: usize,
    auction: bool,
) -> (Vec<u8>, String, crate::session::StateHash) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
            setup.npcs.inst_count = 0;
            if auction {
                setup.auction_ticks = 900;
                setup.ticks_per_day = 15_300;
            }
            let mut authority = GameSession::new(setup, 42).unwrap();
            authority.accounts.insert(
                AccountId(1),
                crate::Account::new(
                    AccountId(1),
                    crate::AccountKind::Player,
                    Money::from_cents(10_000_000),
                ),
            );
            if auction {
                authority.tick = 599;
            }
            let codes = authority.markets.keys().cloned().collect::<Vec<_>>();
            for code in &codes {
                authority
                    .accounts
                    .get_mut(&AccountId(0))
                    .unwrap()
                    .grant_position(code.clone(), 100, Money::from_cents(100_000))
                    .unwrap();
            }
            for code in codes {
                authority
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
                authority
                    .enqueue_player_intent(
                        AccountId(1),
                        Intent::PlaceLimit {
                            code,
                            side: Side::Buy,
                            price: Money::from_cents(1_000),
                            qty: 100,
                        },
                    )
                    .unwrap();
            }
            let (events, receipts) = if auction {
                let committed =
                    super::b2_auction_transaction::prepare_b2_auction_tick(&mut authority)
                        .unwrap()
                        .commit();
                (
                    committed.output.auction.events,
                    committed.output.auction.receipts,
                )
            } else {
                let committed =
                    super::b1_continuous_transaction::prepare_b1_continuous_tick(&mut authority)
                        .unwrap()
                        .commit();
                (committed.output.events, committed.output.receipts)
            };
            assert!(receipts.len() >= 4);
            assert!(authority
                .markets
                .values()
                .all(|market| market.resting_orders().is_empty()));
            (
                serde_json::to_vec(&events).unwrap(),
                format!("{receipts:?}"),
                authority.business_state_hash().unwrap(),
            )
        })
}
