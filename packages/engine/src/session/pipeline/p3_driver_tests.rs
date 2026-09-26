use super::*;

#[test]
fn p3_independent_share_reservations_of_one_account_use_two_workers() {
    let account = crate::AccountId(0);
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let codes = game
        .setup
        .stocks
        .iter()
        .map(|stock| stock.code.clone())
        .collect::<Vec<_>>();
    for code in &codes {
        game.accounts
            .get_mut(&account)
            .unwrap()
            .grant_position(code.clone(), 100, crate::Money::from_cents(1_000))
            .unwrap();
    }
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let validation = P3StockValidation::new(
        crate::SecurityCategory::MainBoard,
        crate::Money::from_cents(1_100),
        crate::Money::from_cents(900),
    );
    let context =
        P3ValidationContext::new(codes.iter().cloned().map(|code| (code, validation))).unwrap();
    let mut driver = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context,
    )
    .unwrap();
    let results = driver
        .consume_round(codes.iter().cloned().enumerate().map(|(index, code)| {
            limit_candidate(index as u64, account, code, crate::Side::Sell, 100)
        }))
        .unwrap();
    assert_eq!(driver.last_round_account_shards(), 2);
    assert!(results
        .iter()
        .all(|result| matches!(result.result(), P3CandidateResult::Accepted { .. })));
    for code in &codes {
        assert_eq!(
            driver.checkpoint().remaining_sellable(account, code),
            Some(0)
        );
    }
}

#[test]
fn p3_unrelated_cancels_do_not_occupy_the_cash_lane() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
    .unwrap();
    let mut driver = driver(&game, context, game.next_order_id);
    let results = driver
        .consume_round([
            P2Candidate::new(
                P2CandidateKey::player(0),
                account,
                crate::Intent::Cancel {
                    code: code.clone(),
                    id: crate::OrderId(10),
                },
            ),
            limit_candidate(1, account, code.clone(), crate::Side::Buy, 100),
            P2Candidate::new(
                P2CandidateKey::player(2),
                account,
                crate::Intent::Cancel {
                    code,
                    id: crate::OrderId(11),
                },
            ),
        ])
        .unwrap();
    assert_eq!(driver.last_round_account_shards(), 3);
    assert!(results
        .iter()
        .all(|result| matches!(result.result(), P3CandidateResult::Accepted { .. })));
}

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
    let context =
        P3ValidationContext::new([(first.clone(), validation), (second.clone(), validation)])
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
    assert_eq!(
        driver.checkpoint().remaining_cash(account),
        Some(crate::Money::ZERO)
    );
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
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
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
        .consume(limit_candidate(
            3,
            account,
            code.clone(),
            crate::Side::Sell,
            100,
        ))
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
    assert_eq!(
        driver.checkpoint().remaining_sellable(account, &code),
        Some(0)
    );
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
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
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
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
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
    let context = P3ValidationContext::new([(
        known.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
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
    let context = P3ValidationContext::new([(known.clone(), validation)]).unwrap();
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
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
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
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )])
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
