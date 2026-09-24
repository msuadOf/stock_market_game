use super::b1_continuous_transaction::apply_initial_candidate_stream_for_test;
use super::p3_context::build_p3_validation_context;
use super::p4_continuous::IncrementalContinuousStockCoordinator;
use super::p4_continuous_adapter::prepare_incremental_continuous_inputs;
use super::stock_auction::b2_auction_day_end::IncrementalAuctionStockCoordinator;
use super::stock_auction_adapter::prepare_incremental_auction_inputs;
use super::*;
use crate::{AccountId, Intent, Money, Side, StockCode};

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
    let initial = P2CandidateBatch::new(vec![
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
fn linked_parent_round_uses_account_threads_when_event_slots_cover_every_candidate() {
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let (session, _, initial) = fixture();
                let plan = plan_tick(PhaseInput { session: &session }).unwrap();
                let codes = session.markets.keys().cloned().collect::<Vec<_>>();
                let linked = [
                    (AccountId(1), codes[0].clone()),
                    (AccountId(0), codes[1].clone()),
                ];
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
                    P3OpenOrderLimits::PRODUCTION,
                )
                .unwrap()
                .with_pending_plan_event_budget(linked, 4);
                let mut p3 = P3ValidatorDriver::new(
                    plan.decision_resources().unwrap().clone(),
                    plan.envelope_ledger().unwrap(),
                    session.next_order_id,
                    session.setup.config.clone(),
                    context,
                )
                .unwrap();
                let outcomes = p3
                    .consume_round(initial.candidates().iter().cloned())
                    .unwrap();
                assert_eq!(p3.last_round_account_shards(), 2);
                assert_eq!(p3.checkpoint().pending_plan_event_slots_remaining(), 0);
                (outcomes, p3.checkpoint())
            })
    };

    let once = run(1);
    let twice = run(2);
    assert_eq!(once, twice);
    assert!(once
        .0
        .iter()
        .all(|outcome| matches!(outcome.result(), P3CandidateResult::Accepted { .. })));
}

#[test]
fn linked_parent_slot_competition_skips_rejection_and_remains_consumed_next_round() {
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let (session, _, _) = fixture();
                let plan = plan_tick(PhaseInput { session: &session }).unwrap();
                let codes = session.markets.keys().cloned().collect::<Vec<_>>();
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
                    P3OpenOrderLimits::PRODUCTION,
                )
                .unwrap()
                .with_pending_plan_event_budget(
                    [
                        (AccountId(1), codes[0].clone()),
                        (AccountId(0), codes[1].clone()),
                    ],
                    2,
                );
                let mut p3 = P3ValidatorDriver::new(
                    plan.decision_resources().unwrap().clone(),
                    plan.envelope_ledger().unwrap(),
                    session.next_order_id,
                    session.setup.config.clone(),
                    context,
                )
                .unwrap();
                let place = |key, account, code, qty| {
                    P2Candidate::new(
                        key,
                        account,
                        Intent::PlaceLimit {
                            code,
                            side: Side::Buy,
                            price: Money::from_cents(990),
                            qty,
                        },
                    )
                };
                let first = p3
                    .consume_round([
                        place(
                            P2CandidateKey::npc(AccountId(1), 0),
                            AccountId(1),
                            codes[0].clone(),
                            99,
                        ),
                        place(
                            P2CandidateKey::player(0),
                            AccountId(0),
                            codes[1].clone(),
                            100,
                        ),
                    ])
                    .unwrap();
                assert_eq!(p3.last_round_account_shards(), 2);
                let second = p3
                    .consume_round([place(
                        P2CandidateKey::plan_chain(0),
                        AccountId(1),
                        codes[0].clone(),
                        100,
                    )])
                    .unwrap();
                (first, second, p3.checkpoint())
            })
    };

    let once = run(1);
    let twice = run(2);
    assert_eq!(once, twice);
    assert!(matches!(
        once.0[0].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::InvalidQuantity,
            ..
        }
    ));
    assert!(matches!(
        once.0[1].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert!(matches!(
        once.1[0].result(),
        P3CandidateResult::PendingPlanEventsLimited { .. }
    ));
    assert_eq!(once.2.pending_plan_event_slots_remaining(), 0);
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
                let mut p3 = validator(&session);
                let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
                    prepare_incremental_continuous_inputs(&session).unwrap(),
                )
                .unwrap();

                let rounds = super::pre_open_transaction::apply_initial_candidate_stream(
                    &mut p3, &mut p4, &initial,
                )
                .unwrap();

                assert_eq!(rounds.len(), 1);
                assert_eq!(rounds[0].projections.len(), 2);
                assert_eq!(rounds[0].facts.len(), 2);
                assert_eq!(p3.last_round_account_shards(), 2);
                assert_eq!(p3.checkpoint().global_open_orders(), 0);
                assert_eq!(p3.checkpoint().feedback_count(), 2);
                assert!(rounds[0].facts.iter().all(|fact| matches!(
                    fact.outcome,
                    super::p4_continuous::ContinuousExecutionOutcome::Place {
                        fact: super::p4_continuous::ContinuousPlaceFact::Rejected {
                            reason: crate::RejectionReason::AuctionOrderEntryClosed,
                            ..
                        },
                        ..
                    }
                )));
                (rounds[0].facts.clone(), p3.checkpoint())
            })
    };

    assert_eq!(run(1), run(2));
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
fn independent_accounts_do_not_gain_global_capacity_priority_from_candidate_position() {
    let (session, _, initial) = fixture();
    let mut p3 = limited_validator(
        &session,
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    );

    let outcomes = p3
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();

    assert_eq!(p3.last_round_account_shards(), 2);
    assert!(outcomes.iter().all(|outcome| matches!(
        outcome.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    )));
    assert_eq!(p3.checkpoint().global_open_orders(), 0);
    assert_eq!(p3.output().next_order_id_after(), session.next_order_id);
}

#[test]
fn intersecting_operational_caps_keep_unaffected_limit_or_market_orders() {
    let (session, _, initial) = fixture();
    let plan = plan_tick(PhaseInput { session: &session }).unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let context = |global, event_slots, linked: Vec<(AccountId, StockCode)>| {
        P3ValidationContext::new(
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
            P3OpenOrderLimits {
                global,
                per_account: 2,
            },
        )
        .unwrap()
        .with_pending_plan_event_budget(linked, event_slots)
    };
    let driver = |context| {
        P3ValidatorDriver::new(
            plan.decision_resources().unwrap().clone(),
            plan.envelope_ledger().unwrap(),
            session.next_order_id,
            session.setup.config.clone(),
            context,
        )
        .unwrap()
    };

    let mut keep_ordinary_limit = driver(context(1, 0, vec![(AccountId(1), codes[0].clone())]));
    let limit_outcomes = keep_ordinary_limit
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();
    assert!(matches!(
        limit_outcomes[0].result(),
        P3CandidateResult::PendingPlanEventsLimited { .. }
    ));
    assert!(matches!(
        limit_outcomes[1].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert_eq!(keep_ordinary_limit.checkpoint().global_open_orders(), 1);

    let mut no_linked_priority = driver(context(1, 2, vec![(AccountId(1), codes[0].clone())]));
    let no_priority_outcomes = no_linked_priority
        .consume_round(initial.candidates().iter().cloned())
        .unwrap();
    assert!(no_priority_outcomes.iter().all(|outcome| matches!(
        outcome.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    )));
    assert_eq!(no_linked_priority.checkpoint().global_open_orders(), 0);

    let mut keep_linked_market = driver(context(
        0,
        2,
        vec![
            (AccountId(1), codes[0].clone()),
            (AccountId(0), codes[1].clone()),
        ],
    ));
    let market_outcomes = keep_linked_market
        .consume_round([
            initial.candidates()[0].clone(),
            P2Candidate::new(
                P2CandidateKey::player(0),
                AccountId(0),
                Intent::PlaceMarket {
                    code: codes[1].clone(),
                    side: Side::Buy,
                    qty: 100,
                },
            ),
        ])
        .unwrap();
    assert!(matches!(
        market_outcomes[0].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    assert!(matches!(
        market_outcomes[1].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert_eq!(keep_linked_market.checkpoint().global_open_orders(), 0);
    assert_eq!(
        keep_linked_market
            .checkpoint()
            .pending_plan_event_slots_remaining(),
        0
    );

    let mut equal_alternatives = driver(context(
        1,
        2,
        vec![
            (AccountId(1), codes[0].clone()),
            (AccountId(0), codes[1].clone()),
        ],
    ));
    let tie_outcomes = equal_alternatives
        .consume_round([
            initial.candidates()[0].clone(),
            initial.candidates()[1].clone(),
            P2Candidate::new(
                P2CandidateKey::plan_chain(0),
                AccountId(0),
                Intent::PlaceMarket {
                    code: codes[0].clone(),
                    side: Side::Buy,
                    qty: 100,
                },
            ),
        ])
        .unwrap();
    assert!(tie_outcomes[..2].iter().all(|outcome| matches!(
        outcome.result(),
        P3CandidateResult::PendingPlanEventsLimited { .. }
    )));
    assert!(matches!(
        tie_outcomes[2].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert_eq!(equal_alternatives.checkpoint().global_open_orders(), 0);
    assert_eq!(
        equal_alternatives
            .checkpoint()
            .pending_plan_event_slots_remaining(),
        2
    );
}

#[test]
fn global_capacity_ignores_invalid_places_without_favoring_a_source() {
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

    assert_eq!(p3.last_round_account_shards(), 2);

    assert!(matches!(
        outcomes[0].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::UnknownStock,
            ..
        }
    ));
    assert_eq!(outcomes[1].allocated_order_id(), None);
    assert!(matches!(
        outcomes[1].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    assert!(matches!(
        outcomes[2].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    assert_eq!(p3.checkpoint().global_open_orders(), 0);
    assert_eq!(
        p3.checkpoint().remaining_cash(AccountId(0)),
        before_player_cash
    );
}

#[test]
fn rejected_global_slot_does_not_reserve_cash_for_later_market_order() {
    let (session, _, initial) = fixture();
    let limits = P3OpenOrderLimits {
        global: 1,
        per_account: 3,
    };
    let first = initial.candidates()[0].clone();
    let competing = initial.candidates()[1].clone();
    let later = P2Candidate::new(
        P2CandidateKey::plan_chain(0),
        AccountId(0),
        Intent::PlaceMarket {
            code: session.markets.keys().next().unwrap().clone(),
            side: Side::Buy,
            qty: 100,
        },
    );
    let mut actual = limited_validator(&session, limits);
    let outcomes = actual
        .consume_round([first.clone(), competing, later.clone()])
        .unwrap();
    let mut expected = limited_validator(&session, limits);
    expected.consume_round([later]).unwrap();

    assert_eq!(actual.last_round_account_shards(), 2);
    assert!(matches!(
        outcomes[1].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    assert!(matches!(
        outcomes[2].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert_eq!(
        actual.checkpoint().remaining_cash(AccountId(0)),
        expected.checkpoint().remaining_cash(AccountId(0))
    );
    assert_eq!(actual.checkpoint().global_open_orders(), 0);
}

#[test]
fn rejected_global_slot_does_not_reserve_shares_for_later_market_sale() {
    let (mut session, _, initial) = fixture();
    let code = session.markets.keys().next().unwrap().clone();
    session
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let limits = P3OpenOrderLimits {
        global: 1,
        per_account: 3,
    };
    let first = initial.candidates()[0].clone();
    let competing = P2Candidate::new(
        P2CandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(990),
            qty: 100,
        },
    );
    let later = P2Candidate::new(
        P2CandidateKey::plan_chain(0),
        AccountId(0),
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Sell,
            qty: 100,
        },
    );
    let mut actual = limited_validator(&session, limits);
    let outcomes = actual
        .consume_round([first.clone(), competing, later.clone()])
        .unwrap();
    let mut expected = limited_validator(&session, limits);
    expected.consume_round([later]).unwrap();

    assert_eq!(actual.last_round_account_shards(), 2);
    assert!(matches!(
        outcomes[1].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
    assert!(matches!(
        outcomes[2].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert_eq!(
        actual.checkpoint().remaining_sellable(AccountId(0), &code),
        expected
            .checkpoint()
            .remaining_sellable(AccountId(0), &code)
    );
}

#[test]
fn linked_parent_slot_loser_keeps_cash_for_later_unlinked_market_order() {
    let (session, _, initial) = fixture();
    let plan = plan_tick(PhaseInput { session: &session }).unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let create = || {
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
            P3OpenOrderLimits::PRODUCTION,
        )
        .unwrap()
        .with_pending_plan_event_budget(
            [
                (AccountId(1), codes[0].clone()),
                (AccountId(0), codes[1].clone()),
            ],
            2,
        );
        P3ValidatorDriver::new(
            plan.decision_resources().unwrap().clone(),
            plan.envelope_ledger().unwrap(),
            session.next_order_id,
            session.setup.config.clone(),
            context,
        )
        .unwrap()
    };
    let first = initial.candidates()[0].clone();
    let competing = initial.candidates()[1].clone();
    let later = P2Candidate::new(
        P2CandidateKey::plan_chain(0),
        AccountId(0),
        Intent::PlaceMarket {
            code: codes[0].clone(),
            side: Side::Buy,
            qty: 100,
        },
    );
    let mut actual = create();
    let outcomes = actual
        .consume_round([first.clone(), competing, later.clone()])
        .unwrap();
    let mut expected = create();
    expected.consume_round([later]).unwrap();

    assert_eq!(actual.last_round_account_shards(), 2);
    assert!(matches!(
        outcomes[1].result(),
        P3CandidateResult::PendingPlanEventsLimited { .. }
    ));
    assert!(matches!(
        outcomes[2].result(),
        P3CandidateResult::Accepted { .. }
    ));
    assert_eq!(
        actual.checkpoint().remaining_cash(AccountId(0)),
        expected.checkpoint().remaining_cash(AccountId(0))
    );
    assert_eq!(actual.checkpoint().pending_plan_event_slots_remaining(), 2);
}

#[test]
fn shared_slot_results_ignore_account_worker_completion_order() {
    let run = |threads, permutation| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                with_executor_perturbation(
                    ExecutorPerturbation {
                        account_shards: permutation,
                        stock_shards: ExecutorPermutation::Canonical,
                        worker_results: permutation,
                        disable_merge: None,
                    },
                    || {
                        let (session, _, initial) = fixture();
                        let mut p3 = limited_validator(
                            &session,
                            P3OpenOrderLimits {
                                global: 1,
                                per_account: 3,
                            },
                        );
                        let later = P2Candidate::new(
                            P2CandidateKey::plan_chain(0),
                            AccountId(0),
                            Intent::PlaceMarket {
                                code: session.markets.keys().next().unwrap().clone(),
                                side: Side::Buy,
                                qty: 100,
                            },
                        );
                        let outcomes = p3
                            .consume_round([
                                initial.candidates()[0].clone(),
                                initial.candidates()[1].clone(),
                                later,
                            ])
                            .unwrap();
                        (outcomes, p3.checkpoint())
                    },
                )
                .unwrap()
                .0
            })
    };
    let expected = run(1, ExecutorPermutation::Canonical);
    for (threads, permutation) in [
        (1, ExecutorPermutation::Reverse),
        (2, ExecutorPermutation::Reverse),
        (4, ExecutorPermutation::RotateLeft),
    ] {
        assert_eq!(run(threads, permutation), expected);
    }
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
    let initial = P2CandidateBatch::new(vec![
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
    let mut p3 = validator(&session);
    let initial = P2CandidateBatch::new(vec![
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
fn same_account_ready_places_keep_request_order_without_waiting_for_p4_feedback() {
    for auction in [false, true] {
        let (mut session, _, _) = fixture();
        if auction {
            session.setup.auction_ticks = 900;
            session.setup.ticks_per_day = 15_300;
        }
        let codes = session.markets.keys().cloned().collect::<Vec<_>>();
        let candidates = P2CandidateBatch::new(vec![
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
        assert_eq!(rounds, 1);
        assert_eq!(p3.checkpoint().global_open_orders(), 0);
        assert_eq!(p3.checkpoint().account_open_orders(AccountId(0)), Some(0));
        assert_eq!(p3.output().next_order_id_after(), session.next_order_id + 1);
        assert_eq!(p3.output().accepted().count(), 1);
        assert!(matches!(
            p3.output().results()[1],
            P3CandidateResult::Rejected {
                reason: crate::RejectionReason::ResourceLimitExceeded,
                ..
            }
        ));
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
    let initial = P2CandidateBatch::new(vec![
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
fn continuous_feedback_accepts_independent_stock_facts_in_another_output_order() {
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
    assert_eq!(round.facts.len(), 2);
    round.facts.reverse();

    super::b1_continuous_transaction::apply_open_order_feedback_for_test(
        &mut p3, &outcomes, &round,
    )
    .unwrap();
    assert_eq!(p3.checkpoint().feedback_count(), 2);
}

#[test]
fn pre_open_feedback_rejects_a_late_unknown_delta_without_partial_updates() {
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
    round.open_order_deltas.last_mut().unwrap().candidate_key = P2CandidateKey::plan_chain(999);
    let before = p3.checkpoint();

    assert!(
        super::pre_open_transaction::apply_open_order_feedback(&mut p3, &outcomes, &round,)
            .is_err()
    );
    assert_eq!(p3.checkpoint(), before);
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
        assert!(plan.state.execute(|_| Ok(())).is_err());
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
