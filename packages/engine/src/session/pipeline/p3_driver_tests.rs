use super::*;
use std::collections::BTreeMap;

#[test]
fn p3_driver_shares_cash_budget_and_advances_rejected_sealed_slots_without_ids() {
    let account = crate::AccountId(0);
    let first = crate::StockCode("600888".to_owned());
    let second = crate::StockCode("600889".to_owned());
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let reservation = crate::session::buy_order_reservation(
        &game.setup.config,
        crate::Money::from_cents(900),
        100,
        crate::Money::ZERO,
    )
    .unwrap();
    game.accounts.get_mut(&account).unwrap().cash = reservation;
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let validation = P3StockValidation::new(
        crate::SecurityCategory::MainBoard,
        crate::Money::from_cents(1_100),
        crate::Money::from_cents(900),
    );
    let context = P3ValidationContext::new(
        [(first.clone(), validation), (second.clone(), validation)],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context,
    )
    .unwrap();

    let first_outcome = driver
        .consume(limit_candidate(0, account, first, crate::Side::Buy, 100))
        .unwrap();
    let rejected = driver
        .consume(limit_candidate(1, account, second, crate::Side::Buy, 100))
        .unwrap();

    assert_eq!(first_outcome.sealed_index(), 0);
    assert!(matches!(
        first_outcome.operation(),
        Some(P3ValidatedOperation::Place(draft))
            if draft.order_id() == crate::OrderId(game.next_order_id)
    ));
    assert_eq!(rejected.sealed_index(), 1);
    assert!(matches!(
        rejected.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::InsufficientCash,
            ..
        }
    ));
    assert_eq!(rejected.operation(), None);
    assert_eq!(rejected.next_order_id_after(), game.next_order_id + 1);
}

#[test]
fn p3_driver_shares_per_account_and_global_limit_counters_across_yields() {
    let first_account = crate::AccountId(0);
    let second_account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let validation = P3StockValidation::new(
        crate::SecurityCategory::MainBoard,
        crate::Money::from_cents(1_100),
        crate::Money::from_cents(900),
    );
    let mut per_account = driver(
        &game,
        P3ValidationContext::new(
            [(code.clone(), validation)],
            0,
            [(first_account, 0), (second_account, 0)],
            P3OpenOrderLimits {
                global: 10,
                per_account: 1,
            },
        )
        .unwrap(),
        game.next_order_id,
    );

    per_account
        .consume(limit_candidate(
            0,
            first_account,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap();
    let per_account_rejection = per_account
        .consume(limit_candidate(
            1,
            first_account,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap();
    assert!(matches!(
        per_account_rejection.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));

    let mut global = driver(
        &game,
        P3ValidationContext::new(
            [(code.clone(), validation)],
            0,
            [(first_account, 0), (second_account, 0)],
            P3OpenOrderLimits {
                global: 1,
                per_account: 10,
            },
        )
        .unwrap(),
        game.next_order_id,
    );
    global
        .consume(limit_candidate(
            0,
            first_account,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap();
    let global_rejection = global
        .consume(limit_candidate(
            1,
            second_account,
            code,
            crate::Side::Buy,
            100,
        ))
        .unwrap();
    assert!(matches!(
        global_rejection.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::ResourceLimitExceeded,
            ..
        }
    ));
}

#[test]
fn p3_driver_rejections_do_not_consume_the_shared_sell_budget_or_order_id() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);

    let invalid = driver
        .consume(limit_candidate(
            0,
            account,
            code.clone(),
            crate::Side::Sell,
            25,
        ))
        .unwrap();
    let board_lot = driver
        .consume(limit_candidate(
            1,
            account,
            code.clone(),
            crate::Side::Sell,
            100,
        ))
        .unwrap();
    let odd_lot = driver
        .consume(limit_candidate(
            2,
            account,
            code.clone(),
            crate::Side::Sell,
            50,
        ))
        .unwrap();
    let exhausted = driver
        .consume(limit_candidate(3, account, code, crate::Side::Sell, 100))
        .unwrap();

    assert!(matches!(
        invalid.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::InvalidQuantity,
            ..
        }
    ));
    assert!(matches!(
        board_lot.operation(),
        Some(P3ValidatedOperation::Place(draft))
            if draft.order_id() == crate::OrderId(game.next_order_id)
                && draft.required() == ResVec::new(crate::Money::ZERO, 100)
    ));
    assert!(matches!(
        odd_lot.operation(),
        Some(P3ValidatedOperation::Place(draft))
            if draft.order_id() == crate::OrderId(game.next_order_id + 1)
                && draft.required() == ResVec::new(crate::Money::ZERO, 50)
    ));
    assert!(matches!(
        exhausted.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::InsufficientShares,
            ..
        }
    ));
    assert_eq!(driver.checkpoint().sealed_count(), 4);
    assert_eq!(
        driver.checkpoint().next_order_id_after(),
        game.next_order_id + 2
    );
}

#[test]
fn p3_driver_fatal_is_atomic_and_the_same_sealed_slot_can_be_retried() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, u64::MAX);
    let before_checkpoint = driver.checkpoint();
    let before_output = driver.output().clone();

    let error = driver
        .consume(limit_candidate(
            0,
            account,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, .. }
            if description == "P3 order ID allocation overflow"
    ));
    assert_eq!(driver.checkpoint(), before_checkpoint);
    assert_eq!(driver.output(), &before_output);

    let retried = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            account,
            crate::Intent::Cancel {
                code,
                id: crate::OrderId(77),
            },
        ))
        .unwrap();
    assert_eq!(retried.sealed_index(), 0);
    assert_eq!(retried.next_order_id_after(), u64::MAX);
    assert!(matches!(
        retried.operation(),
        Some(P3ValidatedOperation::Cancel {
            order_id: crate::OrderId(77),
            ..
        })
    ));
}

#[test]
fn p3_driver_preserves_key_regression_and_rejects_replayed_identity_atomically() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(1),
            account,
            crate::Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(77),
            },
        ))
        .unwrap();
    let reverse_key = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            account,
            crate::Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(78),
            },
        ))
        .unwrap();
    assert_eq!(reverse_key.sealed_index(), 1);
    let before_checkpoint = driver.checkpoint();
    let before_output = driver.output().clone();

    let error = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(1),
            account,
            crate::Intent::Cancel {
                code,
                id: crate::OrderId(79),
            },
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation {
            description,
            location,
        } if description.contains("identity was replayed")
            && location == "pipeline::p3_driver"
    ));
    assert_eq!(driver.checkpoint(), before_checkpoint);
    assert_eq!(driver.output(), &before_output);
}

#[test]
fn p3_driver_final_output_matches_one_shot_batch_validation() {
    let account = crate::AccountId(0);
    let known = crate::StockCode("600888".to_owned());
    let unknown = crate::StockCode("600999".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let context = P3ValidationContext::new(
        [(
            known.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let candidates = vec![
        limit_candidate(0, account, known.clone(), crate::Side::Buy, 100),
        limit_candidate(1, account, unknown, crate::Side::Buy, 100),
        P2Candidate::new(
            P2CandidateKey::plan_chain(2),
            account,
            crate::Intent::Cancel {
                code: known.clone(),
                id: crate::OrderId(77),
            },
        ),
        P2Candidate::new(
            P2CandidateKey::plan_chain(3),
            account,
            crate::Intent::PlaceMarket {
                code: known,
                side: crate::Side::Buy,
                qty: 100,
            },
        ),
    ];
    let expected = P2P3Handoff::new_with_context(
        P2CandidateBatch::new(candidates.clone()).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context.clone(),
    )
    .unwrap()
    .validate()
    .unwrap();
    let mut driver = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context,
    )
    .unwrap();

    for candidate in candidates {
        driver.consume(candidate).unwrap();
    }

    assert_eq!(driver.output(), &expected);
    assert_eq!(driver.checkpoint().operation_count(), 3);
    assert_eq!(driver.checkpoint().draft_count(), 2);
}

#[test]
fn p3_driver_round_uses_two_pass_identity_allocation() {
    let account = crate::AccountId(0);
    let known = crate::StockCode("600888".to_owned());
    let unknown = crate::StockCode("600999".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let validation = P3StockValidation::new(
        crate::SecurityCategory::MainBoard,
        crate::Money::from_cents(1_100),
        crate::Money::from_cents(900),
    );
    let context = P3ValidationContext::new(
        [(known.clone(), validation)],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let start = game.next_order_id;
    let mut driver = driver(&game, context, start);

    let outcomes = driver
        .consume_round([
            P2Candidate::new(
                P2CandidateKey::plan_chain(0),
                account,
                crate::Intent::Cancel {
                    code: known.clone(),
                    id: crate::OrderId(77),
                },
            ),
            limit_candidate(1, account, unknown, crate::Side::Buy, 100),
            limit_candidate(2, account, known.clone(), crate::Side::Buy, 100),
            P2Candidate::new(
                P2CandidateKey::plan_chain(3),
                account,
                crate::Intent::PlaceMarket {
                    code: known,
                    side: crate::Side::Buy,
                    qty: 100,
                },
            ),
        ])
        .unwrap();

    assert_eq!(outcomes.len(), 4);
    assert_eq!(outcomes[0].allocated_order_id(), None);
    assert!(matches!(
        outcomes[1].result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::UnknownStock,
            ..
        }
    ));
    assert_eq!(outcomes[1].allocated_order_id(), None);
    assert_eq!(
        outcomes[2].allocated_order_id(),
        Some(crate::OrderId(start))
    );
    assert_eq!(
        outcomes[3].allocated_order_id(),
        Some(crate::OrderId(start + 1))
    );
    assert_eq!(
        outcomes
            .iter()
            .map(P3ConsumeOutcome::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        driver
            .output()
            .identities()
            .map(|(key, sealed_index, order_id)| (key.clone(), sealed_index, order_id))
            .collect::<Vec<_>>(),
        vec![
            (P2CandidateKey::plan_chain(0), 0, None),
            (P2CandidateKey::plan_chain(1), 1, None),
            (
                P2CandidateKey::plan_chain(2),
                2,
                Some(crate::OrderId(start)),
            ),
            (
                P2CandidateKey::plan_chain(3),
                3,
                Some(crate::OrderId(start + 1)),
            ),
        ]
    );
    assert_eq!(driver.output().next_sealed_index_after(), 4);
    let expected = driver.output().clone();
    assert_eq!(driver.finish(), expected);
}

#[test]
fn p3_driver_later_round_order_id_overflow_discards_the_whole_round() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, u64::MAX - 1);
    driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            account,
            crate::Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(77),
            },
        ))
        .unwrap();
    let before_checkpoint = driver.checkpoint();
    let before_output = driver.output().clone();

    let error = driver
        .consume_round([
            limit_candidate(1, account, code.clone(), crate::Side::Buy, 100),
            limit_candidate(2, account, code, crate::Side::Buy, 100),
        ])
        .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, .. }
            if description == "P3 order ID allocation overflow"
    ));
    assert_eq!(driver.checkpoint(), before_checkpoint);
    assert_eq!(driver.output(), &before_output);
}

#[test]
fn p3_driver_later_round_sealed_overflow_preserves_the_prior_boundary() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = P3ValidatorDriver::new_with_cursors(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        u64::MAX - 1,
        game.setup.config.clone(),
        context,
    )
    .unwrap();
    let first = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            account,
            crate::Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(77),
            },
        ))
        .unwrap();
    assert_eq!(first.sealed_index(), u64::MAX - 1);
    assert_eq!(driver.checkpoint().next_sealed_index(), u64::MAX);
    let before_checkpoint = driver.checkpoint();
    let before_output = driver.output().clone();

    let error = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(1),
            account,
            crate::Intent::Cancel {
                code,
                id: crate::OrderId(78),
            },
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, .. }
            if description == "P3 sealed candidate index overflow"
    ));
    assert_eq!(driver.checkpoint(), before_checkpoint);
    assert_eq!(driver.output(), &before_output);
}

#[test]
fn p3_driver_p4_feedback_updates_only_open_order_constraints() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let reservation = crate::session::buy_order_reservation(
        &game.setup.config,
        crate::Money::from_cents(900),
        100,
        crate::Money::ZERO,
    )
    .unwrap();
    game.accounts.get_mut(&account).unwrap().cash = reservation;
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let first = driver
        .consume(limit_candidate(
            0,
            account,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap();
    let after_p3 = driver.checkpoint();
    assert_eq!(after_p3.remaining_cash(account), Some(crate::Money::ZERO));
    assert_eq!(after_p3.global_open_orders(), 1);

    driver
        .apply_open_order_feedback(first.candidate_key(), first.sealed_index(), [])
        .unwrap();
    let after_p4_reject = driver.checkpoint();
    assert_eq!(
        after_p4_reject.remaining_cash(account),
        Some(crate::Money::ZERO)
    );
    assert_eq!(after_p4_reject.global_open_orders(), 0);
    assert_eq!(after_p4_reject.account_open_orders(account), Some(0));
    assert_eq!(after_p4_reject.feedback_count(), 1);

    let second = driver
        .consume(limit_candidate(1, account, code, crate::Side::Buy, 100))
        .unwrap();
    assert!(matches!(
        second.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::InsufficientCash,
            ..
        }
    ));
    assert_eq!(second.allocated_order_id(), None);
}

#[test]
fn p3_driver_p4_rejection_releases_the_slot_but_retains_its_allocated_order_id() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let rejected_by_p4 = driver
        .consume(limit_candidate(
            0,
            account,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap();
    assert_eq!(
        rejected_by_p4.allocated_order_id(),
        Some(crate::OrderId(game.next_order_id))
    );

    driver
        .apply_open_order_feedback(
            rejected_by_p4.candidate_key(),
            rejected_by_p4.sealed_index(),
            [],
        )
        .unwrap();
    let replacement = driver
        .consume(limit_candidate(1, account, code, crate::Side::Buy, 100))
        .unwrap();

    assert_eq!(
        replacement.allocated_order_id(),
        Some(crate::OrderId(game.next_order_id + 1))
    );
    assert_eq!(
        driver.output().next_order_id_after(),
        game.next_order_id + 2
    );
}

#[test]
fn p3_driver_p4_feedback_does_not_replenish_sellable_share_budget() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, crate::Money::from_cents(90_000))
        .unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let first = driver
        .consume(limit_candidate(
            0,
            account,
            code.clone(),
            crate::Side::Sell,
            100,
        ))
        .unwrap();

    driver
        .apply_open_order_feedback(first.candidate_key(), first.sealed_index(), [])
        .unwrap();
    assert_eq!(
        driver.checkpoint().remaining_sellable(account, &code),
        Some(0)
    );
    let second = driver
        .consume(limit_candidate(1, account, code, crate::Side::Sell, 100))
        .unwrap();
    assert!(matches!(
        second.result(),
        P3CandidateResult::Rejected {
            reason: crate::RejectionReason::InsufficientShares,
            ..
        }
    ));
}

#[test]
fn p3_driver_feedback_accounts_for_multiple_terminal_makers() {
    let taker = crate::AccountId(0);
    let maker = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        2,
        [(taker, 0), (maker, 2)],
        P3OpenOrderLimits {
            global: 3,
            per_account: 2,
        },
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let triggering = driver
        .consume(limit_candidate(
            0,
            taker,
            code.clone(),
            crate::Side::Buy,
            100,
        ))
        .unwrap();

    driver
        .apply_open_order_feedback(
            triggering.candidate_key(),
            triggering.sealed_index(),
            [(taker, 1), (maker, -2)],
        )
        .unwrap();
    let checkpoint = driver.checkpoint();
    assert_eq!(checkpoint.global_open_orders(), 1);
    assert_eq!(checkpoint.account_open_orders(taker), Some(1));
    assert_eq!(checkpoint.account_open_orders(maker), Some(0));

    let before_duplicate = driver.checkpoint();
    let error = driver
        .apply_open_order_feedback(
            triggering.candidate_key(),
            triggering.sealed_index(),
            [(taker, 1), (maker, -2)],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, .. }
            if description.contains("more than once")
    ));
    assert_eq!(driver.checkpoint(), before_duplicate);

    let maker_place = driver
        .consume(limit_candidate(1, maker, code, crate::Side::Buy, 100))
        .unwrap();
    assert!(matches!(
        maker_place.operation(),
        Some(P3ValidatedOperation::Place(_))
    ));
}

#[test]
fn p3_driver_market_feedback_rejects_any_positive_open_order_delta_atomically() {
    let taker = crate::AccountId(0);
    let maker = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        1,
        [(taker, 0), (maker, 1)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let market = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            taker,
            crate::Intent::PlaceMarket {
                code,
                side: crate::Side::Buy,
                qty: 100,
            },
        ))
        .unwrap();
    let before_feedback = driver.checkpoint();

    let error = driver
        .apply_open_order_feedback(
            market.candidate_key(),
            market.sealed_index(),
            [(taker, 1), (maker, -1)],
        )
        .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, .. }
            if description.contains("impossible positive order delta")
    ));
    assert_eq!(driver.checkpoint(), before_feedback);
}

#[test]
fn p3_driver_successful_cancel_feedback_releases_only_the_order_slot() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        1,
        [(account, 1)],
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let cancel = driver
        .consume(P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            account,
            crate::Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(77),
            },
        ))
        .unwrap();
    assert_eq!(cancel.allocated_order_id(), None);
    driver
        .apply_open_order_feedback(
            cancel.candidate_key(),
            cancel.sealed_index(),
            [(account, -1)],
        )
        .unwrap();

    let replacement = driver
        .consume(limit_candidate(1, account, code, crate::Side::Buy, 100))
        .unwrap();
    assert_eq!(
        replacement.allocated_order_id(),
        Some(crate::OrderId(game.next_order_id))
    );
}

#[test]
fn p3_driver_feedback_round_keeps_all_counts_when_a_later_fact_is_invalid() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )],
        1,
        [(account, 1)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let outcomes = driver
        .consume_round((0..2).map(|index| {
            P2Candidate::new(
                P2CandidateKey::plan_chain(index),
                account,
                crate::Intent::Cancel {
                    code: code.clone(),
                    id: crate::OrderId(77 + index),
                },
            )
        }))
        .unwrap();
    let before = driver.checkpoint();

    let result = driver.apply_open_order_feedback_round([
        (
            outcomes[0].candidate_key().clone(),
            outcomes[0].sealed_index(),
            BTreeMap::from([(account, -1)]),
        ),
        (
            outcomes[1].candidate_key().clone(),
            outcomes[1].sealed_index(),
            BTreeMap::from([(account, -1)]),
        ),
    ]);
    assert!(result.is_err());
    assert_eq!(driver.checkpoint(), before);
    driver
        .apply_open_order_feedback(
            outcomes[0].candidate_key(),
            outcomes[0].sealed_index(),
            [(account, -1)],
        )
        .unwrap();
    assert_eq!(driver.checkpoint().global_open_orders(), 0);
}

fn driver(
    game: &GameSession,
    context: P3ValidationContext,
    next_order_id: u64,
) -> P3ValidatorDriver {
    let plan = plan_tick(PhaseInput { session: game }).unwrap();
    P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        next_order_id,
        game.setup.config.clone(),
        context,
    )
    .unwrap()
}

fn limit_candidate(
    chain_generation_index: u64,
    account: crate::AccountId,
    code: crate::StockCode,
    side: crate::Side,
    qty: u32,
) -> P2Candidate {
    P2Candidate::new(
        P2CandidateKey::plan_chain(chain_generation_index),
        account,
        crate::Intent::PlaceLimit {
            code,
            side,
            price: crate::Money::from_cents(900),
            qty,
        },
    )
}
