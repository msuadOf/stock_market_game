use super::account_validation::{
    AccountValidationContext, PlaceKind, StockValidation, ValidatedOperation,
};
use super::account_validation_context::build_account_validation_context;
use super::*;
use crate::RejectionReason;

#[test]
fn symbolic_buy_reserves_its_account_validation_bound_and_keeps_the_requested_price() {
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        crate::AccountId(0),
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Lowest,
            qty: 100,
        },
    )])
    .unwrap();
    let output = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        build_account_validation_context(&game).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap();
    let draft = &output.drafts()[0];
    let bound = game.state.markets[&code].down_stop().unwrap();
    assert_eq!(draft.requested_price(), Some(crate::LimitPrice::Lowest));
    assert_eq!(draft.limit(), bound);
    assert_eq!(
        draft.required().cash,
        crate::session::buy_order_reservation(
            &game.state.setup.config,
            bound,
            100,
            crate::Money::ZERO
        )
        .unwrap()
    );
    let envelope = draft.materialize_envelope();
    assert_eq!(envelope.live(), draft.required());
    assert_eq!(envelope.pending_price(), Some(crate::LimitPrice::Lowest));
}

#[test]
fn highest_buy_needs_the_full_daily_limit_reservation_including_fees() {
    let code = crate::StockCode("600888".to_owned());
    let setup = crate::session::npc_working_quote_tests::quote_setup(0);
    let probe = GameSession::new(setup.clone(), 42).unwrap();
    let up = probe.state.markets[&code].up_stop().unwrap();
    let exact =
        crate::session::buy_order_reservation(&setup.config, up, 100, crate::Money::ZERO).unwrap();

    for (cash, accepted) in [
        (exact, true),
        (exact.sub(crate::Money::from_cents(1)).unwrap(), false),
    ] {
        let mut game = GameSession::new(setup.clone(), 42).unwrap();
        game.state
            .accounts
            .get_mut(&crate::AccountId(0))
            .unwrap()
            .fixture_set_cash(cash);
        let plan = plan_tick(PhaseInput { session: &game }).unwrap();
        let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
            IntentCandidateKey::player(0),
            crate::AccountId(0),
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Highest,
                qty: 100,
            },
        )])
        .unwrap();
        let output = CandidateValidationInput::new_with_context(
            batch,
            plan.decision_resources().unwrap().clone(),
            plan.envelope_ledger().unwrap(),
            game.state.next_order_id,
            game.state.setup.config.clone(),
            build_account_validation_context(&game).unwrap(),
        )
        .unwrap()
        .validate()
        .unwrap();
        if accepted {
            assert_eq!(output.accepted().count(), 1);
            assert_eq!(output.drafts()[0].qty(), 100);
            assert_eq!(output.drafts()[0].required().cash, exact);
        } else {
            assert_eq!(output.drafts().len(), 0);
            assert!(matches!(
                output.rejected().next(),
                Some((_, RejectionReason::InsufficientCash))
            ));
        }
    }
}

#[test]
fn account_validation_handoff_preserves_input_order_and_uses_sealed_budget_without_mutation() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_strategy(None);
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::plan_chain(0),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::npc(account, 0),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
                qty: 100,
            },
        ),
    ])
    .unwrap();

    let handoff = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        build_account_validation_context(&game).unwrap(),
    )
    .unwrap();
    let output = handoff.validate().unwrap();

    assert_eq!(output.accepted().count(), 2);
    assert_eq!(output.rejected().count(), 1);
    assert_eq!(
        output.drafts()[0].candidate_key(),
        &IntentCandidateKey::plan_chain(0)
    );
    assert_eq!(
        output.drafts()[1].candidate_key(),
        &IntentCandidateKey::npc(account, 0)
    );
    assert_eq!(
        output.drafts()[0].order_id(),
        crate::OrderId(game.state.next_order_id)
    );
    assert_eq!(
        output.drafts()[1].order_id(),
        crate::OrderId(game.state.next_order_id + 1)
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn account_validation_handoff_rejects_order_id_overflow_without_exposing_drafts() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        account,
        crate::Intent::PlaceLimit {
            code,
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
            qty: 100,
        },
    )])
    .unwrap();
    let handoff = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        u64::MAX,
        game.state.setup.config.clone(),
        build_account_validation_context(&game).unwrap(),
    )
    .unwrap();

    assert!(handoff.validate().is_err());
}

#[test]
fn account_validation_handoff_passes_cancel_without_allocating_an_order_id() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        account,
        crate::Intent::Cancel {
            code,
            id: crate::OrderId(99),
        },
    )])
    .unwrap();
    let handoff = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        build_account_validation_context(&game).unwrap(),
    )
    .unwrap();

    let output = handoff.validate().unwrap();

    assert_eq!(output.accepted().count(), 1);
    assert_eq!(output.rejected().count(), 0);
    assert_eq!(output.drafts().len(), 0);
}

#[test]
fn account_validation_handoff_passes_unknown_stock_cancel_to_the_stock_state_machine() {
    let account = crate::AccountId(1);
    let unknown = crate::StockCode("600999".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        account,
        crate::Intent::Cancel {
            code: unknown.clone(),
            id: crate::OrderId(99),
        },
    )])
    .unwrap();
    let handoff = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context(std::iter::empty()),
    )
    .unwrap();

    let output = handoff.validate().unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![IntentCandidateKey::player(0)]
    );
    assert_eq!(output.rejected().count(), 0);
    assert!(matches!(
        output.operations(),
        [ValidatedOperation::Cancel {
            account: owner,
            code,
            order_id: crate::OrderId(99),
            ..
        }] if *owner == account && code == &unknown
    ));
    assert_eq!(output.next_order_id_after(), game.state.next_order_id);
}

#[test]
fn account_validation_sealed_indices_keep_rejected_slots_while_order_ids_only_count_places() {
    let account = crate::AccountId(0);
    let known = crate::StockCode("600888".to_owned());
    let unknown = crate::StockCode("600999".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::player(4),
            account,
            crate::Intent::PlaceMarket {
                code: known.clone(),
                side: crate::Side::Buy,
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            account,
            crate::Intent::Cancel {
                code: unknown.clone(),
                id: crate::OrderId(90),
            },
        ),
        limit(2, account, known.clone(), crate::Side::Buy, 100),
        limit(1, account, unknown, crate::Side::Buy, 100),
        IntentCandidate::new(
            IntentCandidateKey::player(3),
            account,
            crate::Intent::Cancel {
                code: known.clone(),
                id: crate::OrderId(91),
            },
        ),
    ])
    .unwrap();
    let output = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context([(
            known,
            StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )]),
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(
        output
            .results()
            .iter()
            .map(CandidateValidationResult::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    assert_eq!(
        output
            .operations()
            .iter()
            .map(ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 4]
    );
    assert_eq!(
        output
            .drafts()
            .iter()
            .map(EnvelopeDraft::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(
        output
            .drafts()
            .iter()
            .map(EnvelopeDraft::order_id)
            .collect::<Vec<_>>(),
        vec![
            crate::OrderId(game.state.next_order_id),
            crate::OrderId(game.state.next_order_id + 1),
        ]
    );
    assert_eq!(output.next_order_id_after(), game.state.next_order_id + 2);
}

#[test]
fn account_validation_contract_passes_cancel_and_materializes_limit_and_market_envelopes() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::player(2),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
                qty: 50,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(0),
            account,
            crate::Intent::PlaceMarket {
                code: code.clone(),
                side: crate::Side::Buy,
                qty: 100,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(1),
            account,
            crate::Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(77),
            },
        ),
    ])
    .unwrap();
    let context = context([(
        code.clone(),
        StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )]);
    let output = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(output.accepted().count(), 3);
    assert_eq!(output.rejected().count(), 0);
    assert_eq!(output.operations().len(), 3);
    assert!(matches!(
        &output.operations()[2],
        ValidatedOperation::Cancel {
            candidate_key: IntentCandidateKey::Player { player_queue_index: 1 },
            sealed_index: 2,
            account: owner,
            code: cancel_code,
            order_id: crate::OrderId(77),
        } if *owner == account && cancel_code == &code
    ));
    assert_eq!(output.drafts().len(), 2);
    let market = &output.drafts()[1];
    assert_eq!(market.owner(), account);
    assert_eq!(market.code(), &code);
    assert_eq!(market.side(), crate::Side::Buy);
    assert_eq!(market.kind(), PlaceKind::Market);
    assert_eq!(market.limit(), crate::Money::from_cents(1_100));
    assert_eq!(market.qty(), 100);
    assert_eq!(
        market.order_id(),
        crate::OrderId(game.state.next_order_id + 1)
    );
    assert_eq!(market.order(), market.order_id());
    assert_eq!(market.key().account, account);
    assert_eq!(market.required().shares, 0);
    assert_eq!(
        market.required().cash,
        crate::session::buy_order_reservation(
            &game.state.setup.config,
            crate::Money::from_cents(1_100),
            100,
            crate::Money::ZERO,
        )
        .unwrap()
    );
    let sell = &output.drafts()[0];
    assert_eq!(sell.kind(), PlaceKind::Limit);
    assert_eq!(sell.order_id(), crate::OrderId(game.state.next_order_id));
    assert_eq!(sell.required(), ResVec::new(crate::Money::ZERO, 50));
    let envelope = sell.materialize_envelope();
    assert_eq!(envelope.origin(), EnvelopeOrigin::CreatedAtValidation);
    assert_eq!(envelope.key(), sell.key());
    assert_eq!(envelope.live(), sell.required());
    assert_eq!(envelope.audit().limit, sell.limit());
    assert_eq!(envelope.audit().remaining_qty, sell.qty());
    assert_eq!(
        output.next_order_id_after(),
        game.state.next_order_id.checked_add(2).unwrap()
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn account_validation_rejects_unknown_quantity_cash_and_t1_share_failures_with_typed_reasons() {
    let account = crate::AccountId(0);
    let main = crate::StockCode("600888".to_owned());
    let chinext = crate::StockCode("300888".to_owned());
    let unknown = crate::StockCode("600999".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(crate::Money::ZERO);
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(main.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    let holder = game.state.accounts.get_mut(&account).unwrap();
    let position = holder.position(&main).unwrap();
    let locked = crate::Position::from_restored_parts(
        position.qty(),
        100,
        position.invested_cents(),
        position.recovered_cents(),
    );
    holder.fixture_insert_position(main.clone(), locked);
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = vec![
        limit(0, account, unknown, crate::Side::Buy, 100),
        limit(1, account, main.clone(), crate::Side::Buy, 99),
        limit(2, account, main.clone(), crate::Side::Buy, 100),
        limit(3, account, main.clone(), crate::Side::Sell, 100),
        limit(4, account, main.clone(), crate::Side::Sell, 25),
        limit(5, account, main.clone(), crate::Side::Sell, 50),
        limit(6, account, chinext.clone(), crate::Side::Buy, 300_100),
        IntentCandidate::new(
            IntentCandidateKey::player(7),
            account,
            crate::Intent::PlaceMarket {
                code: chinext.clone(),
                side: crate::Side::Buy,
                qty: 150_100,
            },
        ),
        limit(8, account, main.clone(), crate::Side::Buy, 0),
    ];
    let context = context([
        (
            main.clone(),
            StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        ),
        (
            chinext,
            StockValidation::new(
                crate::SecurityCategory::ChiNext,
                crate::Money::from_cents(1_200),
                crate::Money::from_cents(800),
            ),
        ),
    ]);
    let output = CandidateValidationInput::new_with_context(
        IntentCandidateBatch::new(candidates).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();

    let reasons: Vec<_> = output
        .rejected()
        .map(|(key, reason)| (key.clone(), reason.clone()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            (IntentCandidateKey::player(0), RejectionReason::UnknownStock),
            (
                IntentCandidateKey::player(1),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(2),
                RejectionReason::InsufficientCash
            ),
            (
                IntentCandidateKey::player(3),
                RejectionReason::InsufficientShares
            ),
            (
                IntentCandidateKey::player(4),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(6),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(7),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(8),
                RejectionReason::InvalidQuantity
            ),
        ]
    );
    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![IntentCandidateKey::player(5)]
    );
    assert_eq!(
        output.drafts()[0].required(),
        ResVec::new(crate::Money::ZERO, 50)
    );
}

#[test]
fn account_validation_cash_budget_contends_across_stocks_in_account_receipt_order() {
    let account = crate::AccountId(0);
    let first = crate::StockCode("600888".to_owned());
    let second = crate::StockCode("600889".to_owned());
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let reservation = crate::session::buy_order_reservation(
        &game.state.setup.config,
        crate::Money::from_cents(900),
        100,
        crate::Money::ZERO,
    )
    .unwrap();
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(reservation);
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = IntentCandidateBatch::new(vec![
        limit(0, account, first.clone(), crate::Side::Buy, 100),
        limit(1, account, second.clone(), crate::Side::Buy, 100),
    ])
    .unwrap();
    let validation = StockValidation::new(
        crate::SecurityCategory::MainBoard,
        crate::Money::from_cents(1_100),
        crate::Money::from_cents(900),
    );
    let output = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context([(first, validation), (second, validation)]),
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![IntentCandidateKey::player(0)]
    );
    assert_eq!(
        output.rejected().collect::<Vec<_>>(),
        vec![(
            &IntentCandidateKey::player(1),
            &RejectionReason::InsufficientCash
        )]
    );
    assert_eq!(output.drafts()[0].required().cash, reservation);
}

#[test]
fn account_validation_near_order_id_overflow_is_fatal_before_any_output_is_observable() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let ledger = plan.envelope_ledger().unwrap();
    let batch = IntentCandidateBatch::new(vec![
        limit(0, account, code.clone(), crate::Side::Buy, 100),
        limit(1, account, code.clone(), crate::Side::Buy, 100),
    ])
    .unwrap();
    let handoff = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        ledger.clone(),
        u64::MAX - 1,
        game.state.setup.config.clone(),
        context([(
            code,
            StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )]),
    )
    .unwrap();

    let error = handoff.validate().unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, .. }
            if description == "P3 order ID allocation overflow"
    ));
    assert_eq!(handoff.ledger(), &ledger);
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn account_validation_quantity_caps_accept_exact_limits_and_reject_the_next_board_lot() {
    let account = crate::AccountId(0);
    let main = crate::StockCode("600888".to_owned());
    let chinext = crate::StockCode("300888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(crate::Money::from_cents(10_000_000_000));
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = vec![
        limit(0, account, main.clone(), crate::Side::Buy, 1_000_000),
        limit(1, account, main.clone(), crate::Side::Buy, 1_000_100),
        IntentCandidate::new(
            IntentCandidateKey::player(2),
            account,
            crate::Intent::PlaceMarket {
                code: main.clone(),
                side: crate::Side::Buy,
                qty: 1_000_000,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(3),
            account,
            crate::Intent::PlaceMarket {
                code: main.clone(),
                side: crate::Side::Buy,
                qty: 1_000_100,
            },
        ),
        limit(4, account, chinext.clone(), crate::Side::Buy, 300_000),
        limit(5, account, chinext.clone(), crate::Side::Buy, 300_100),
        IntentCandidate::new(
            IntentCandidateKey::player(6),
            account,
            crate::Intent::PlaceMarket {
                code: chinext.clone(),
                side: crate::Side::Buy,
                qty: 150_000,
            },
        ),
        IntentCandidate::new(
            IntentCandidateKey::player(7),
            account,
            crate::Intent::PlaceMarket {
                code: chinext.clone(),
                side: crate::Side::Buy,
                qty: 150_100,
            },
        ),
    ];
    let output = CandidateValidationInput::new_with_context(
        IntentCandidateBatch::new(candidates).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context([
            (
                main,
                StockValidation::new(
                    crate::SecurityCategory::MainBoard,
                    crate::Money::from_cents(1_100),
                    crate::Money::from_cents(900),
                ),
            ),
            (
                chinext,
                StockValidation::new(
                    crate::SecurityCategory::ChiNext,
                    crate::Money::from_cents(1_100),
                    crate::Money::from_cents(900),
                ),
            ),
        ]),
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![
            IntentCandidateKey::player(0),
            IntentCandidateKey::player(2),
            IntentCandidateKey::player(4),
            IntentCandidateKey::player(6),
        ]
    );
    assert_eq!(
        output
            .rejected()
            .map(|(key, reason)| (key.clone(), reason.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                IntentCandidateKey::player(1),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(3),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(5),
                RejectionReason::InvalidQuantity
            ),
            (
                IntentCandidateKey::player(7),
                RejectionReason::InvalidQuantity
            ),
        ]
    );
    assert_eq!(
        output
            .drafts()
            .iter()
            .map(|draft| (draft.kind(), draft.qty(), draft.sealed_index()))
            .collect::<Vec<_>>(),
        vec![
            (PlaceKind::Limit, 1_000_000, 0),
            (PlaceKind::Market, 1_000_000, 2),
            (PlaceKind::Limit, 300_000, 4),
            (PlaceKind::Market, 150_000, 6),
        ]
    );
}

#[test]
fn account_validation_sell_candidates_compete_for_one_private_same_batch_share_budget() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let output = CandidateValidationInput::new_with_context(
        IntentCandidateBatch::new(vec![
            limit(0, account, code.clone(), crate::Side::Sell, 100),
            limit(1, account, code.clone(), crate::Side::Sell, 50),
            limit(2, account, code.clone(), crate::Side::Sell, 100),
        ])
        .unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        context([(
            code,
            StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        )]),
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![IntentCandidateKey::player(0), IntentCandidateKey::player(1)]
    );
    assert_eq!(
        output.rejected().collect::<Vec<_>>(),
        vec![(
            &IntentCandidateKey::player(2),
            &RejectionReason::InsufficientShares,
        )]
    );
    assert_eq!(
        output
            .drafts()
            .iter()
            .map(EnvelopeDraft::required)
            .collect::<Vec<_>>(),
        vec![
            ResVec::new(crate::Money::ZERO, 100),
            ResVec::new(crate::Money::ZERO, 50),
        ]
    );
}

fn context(
    stocks: impl IntoIterator<Item = (crate::StockCode, StockValidation)>,
) -> AccountValidationContext {
    AccountValidationContext::new(stocks).unwrap()
}

fn limit(
    player_queue_index: u64,
    account: crate::AccountId,
    code: crate::StockCode,
    side: crate::Side,
    qty: u32,
) -> IntentCandidate {
    IntentCandidate::new(
        IntentCandidateKey::player(player_queue_index),
        account,
        crate::Intent::PlaceLimit {
            code,
            side,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
            qty,
        },
    )
}
