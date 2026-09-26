use super::p3_context::build_p3_validation_context;
use super::p3_validation::{
    P3PlaceKind, P3StockValidation, P3ValidatedOperation, P3ValidationContext,
};
use super::*;
use crate::RejectionReason;

#[test]
fn p3_handoff_preserves_input_order_and_uses_sealed_budget_without_mutation() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(vec![
        P2Candidate::new(
            P2CandidateKey::plan_chain(0),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::Money::from_cents(900),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::npc(account, 0),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::Money::from_cents(900),
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Sell,
                price: crate::Money::from_cents(900),
                qty: 100,
            },
        ),
    ])
    .unwrap();

    let handoff = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        build_p3_validation_context(&game).unwrap(),
    )
    .unwrap();
    let output = handoff.validate().unwrap();

    assert_eq!(output.accepted().count(), 2);
    assert_eq!(output.rejected().count(), 1);
    assert_eq!(
        output.drafts()[0].candidate_key(),
        &P2CandidateKey::plan_chain(0)
    );
    assert_eq!(
        output.drafts()[1].candidate_key(),
        &P2CandidateKey::npc(account, 0)
    );
    assert_eq!(
        output.drafts()[0].order_id(),
        crate::OrderId(game.next_order_id)
    );
    assert_eq!(
        output.drafts()[1].order_id(),
        crate::OrderId(game.next_order_id + 1)
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn p3_handoff_rejects_order_id_overflow_without_exposing_drafts() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::player(0),
        account,
        crate::Intent::PlaceLimit {
            code,
            side: crate::Side::Buy,
            price: crate::Money::from_cents(900),
            qty: 100,
        },
    )])
    .unwrap();
    let handoff = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        u64::MAX,
        game.setup.config.clone(),
        build_p3_validation_context(&game).unwrap(),
    )
    .unwrap();

    assert!(handoff.validate().is_err());
}

#[test]
fn p3_handoff_passes_cancel_without_allocating_an_order_id() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::player(0),
        account,
        crate::Intent::Cancel {
            code,
            id: crate::OrderId(99),
        },
    )])
    .unwrap();
    let handoff = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        build_p3_validation_context(&game).unwrap(),
    )
    .unwrap();

    let output = handoff.validate().unwrap();

    assert_eq!(output.accepted().count(), 1);
    assert_eq!(output.rejected().count(), 0);
    assert_eq!(output.drafts().len(), 0);
}

#[test]
fn p3_handoff_passes_unknown_stock_cancel_to_the_stock_state_machine() {
    let account = crate::AccountId(1);
    let unknown = crate::StockCode("600999".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::player(0),
        account,
        crate::Intent::Cancel {
            code: unknown.clone(),
            id: crate::OrderId(99),
        },
    )])
    .unwrap();
    let handoff = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context(std::iter::empty()),
    )
    .unwrap();

    let output = handoff.validate().unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert_eq!(output.rejected().count(), 0);
    assert!(matches!(
        output.operations(),
        [P3ValidatedOperation::Cancel {
            account: owner,
            code,
            order_id: crate::OrderId(99),
            ..
        }] if *owner == account && code == &unknown
    ));
    assert_eq!(output.next_order_id_after(), game.next_order_id);
}

#[test]
fn p3_sealed_indices_keep_rejected_slots_while_order_ids_only_count_places() {
    let account = crate::AccountId(0);
    let known = crate::StockCode("600888".to_owned());
    let unknown = crate::StockCode("600999".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(vec![
        P2Candidate::new(
            P2CandidateKey::player(4),
            account,
            crate::Intent::PlaceMarket {
                code: known.clone(),
                side: crate::Side::Buy,
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            account,
            crate::Intent::Cancel {
                code: unknown.clone(),
                id: crate::OrderId(90),
            },
        ),
        limit(2, account, known.clone(), crate::Side::Buy, 100),
        limit(1, account, unknown, crate::Side::Buy, 100),
        P2Candidate::new(
            P2CandidateKey::player(3),
            account,
            crate::Intent::Cancel {
                code: known.clone(),
                id: crate::OrderId(91),
            },
        ),
    ])
    .unwrap();
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context([(
            known,
            P3StockValidation::new(
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
            .map(P3CandidateResult::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    assert_eq!(
        output
            .operations()
            .iter()
            .map(P3ValidatedOperation::sealed_index)
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
            crate::OrderId(game.next_order_id),
            crate::OrderId(game.next_order_id + 1),
        ]
    );
    assert_eq!(output.next_order_id_after(), game.next_order_id + 2);
}

#[test]
fn p3_contract_passes_cancel_and_materializes_limit_and_market_envelopes() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = P2CandidateBatch::new(vec![
        P2Candidate::new(
            P2CandidateKey::player(2),
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Sell,
                price: crate::Money::from_cents(900),
                qty: 50,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            account,
            crate::Intent::PlaceMarket {
                code: code.clone(),
                side: crate::Side::Buy,
                qty: 100,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(1),
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
        P3StockValidation::new(
            crate::SecurityCategory::MainBoard,
            crate::Money::from_cents(1_100),
            crate::Money::from_cents(900),
        ),
    )]);
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
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
        P3ValidatedOperation::Cancel {
            candidate_key: P2CandidateKey::Player { player_queue_index: 1 },
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
    assert_eq!(market.kind(), P3PlaceKind::Market);
    assert_eq!(market.limit(), crate::Money::from_cents(1_100));
    assert_eq!(market.qty(), 100);
    assert_eq!(market.order_id(), crate::OrderId(game.next_order_id + 1));
    assert_eq!(market.order(), market.order_id());
    assert_eq!(market.key().account, account);
    assert_eq!(market.required().shares, 0);
    assert_eq!(
        market.required().cash,
        crate::session::buy_order_reservation(
            &game.setup.config,
            crate::Money::from_cents(1_100),
            100,
            crate::Money::ZERO,
        )
        .unwrap()
    );
    let sell = &output.drafts()[0];
    assert_eq!(sell.kind(), P3PlaceKind::Limit);
    assert_eq!(sell.order_id(), crate::OrderId(game.next_order_id));
    assert_eq!(sell.required(), ResVec::new(crate::Money::ZERO, 50));
    let envelope = sell.materialize_envelope();
    assert_eq!(envelope.origin(), EnvelopeOrigin::P3Created);
    assert_eq!(envelope.key(), sell.key());
    assert_eq!(envelope.live(), sell.required());
    assert_eq!(envelope.audit().limit, sell.limit());
    assert_eq!(envelope.audit().remaining_qty, sell.qty());
    assert_eq!(
        output.next_order_id_after(),
        game.next_order_id.checked_add(2).unwrap()
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn p3_rejects_unknown_quantity_cash_and_t1_share_failures_with_typed_reasons() {
    let account = crate::AccountId(0);
    let main = crate::StockCode("600888".to_owned());
    let chinext = crate::StockCode("300888".to_owned());
    let unknown = crate::StockCode("600999".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&account).unwrap().cash = crate::Money::ZERO;
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(main.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .positions
        .get_mut(&main)
        .unwrap()
        .t1_locked = 100;
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = vec![
        limit(0, account, unknown, crate::Side::Buy, 100),
        limit(1, account, main.clone(), crate::Side::Buy, 99),
        limit(2, account, main.clone(), crate::Side::Buy, 100),
        limit(3, account, main.clone(), crate::Side::Sell, 100),
        limit(4, account, main.clone(), crate::Side::Sell, 25),
        limit(5, account, main.clone(), crate::Side::Sell, 50),
        limit(6, account, chinext.clone(), crate::Side::Buy, 300_100),
        P2Candidate::new(
            P2CandidateKey::player(7),
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
            P3StockValidation::new(
                crate::SecurityCategory::MainBoard,
                crate::Money::from_cents(1_100),
                crate::Money::from_cents(900),
            ),
        ),
        (
            chinext,
            P3StockValidation::new(
                crate::SecurityCategory::ChiNext,
                crate::Money::from_cents(1_200),
                crate::Money::from_cents(800),
            ),
        ),
    ]);
    let output = P2P3Handoff::new_with_context(
        P2CandidateBatch::new(candidates).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
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
            (P2CandidateKey::player(0), RejectionReason::UnknownStock),
            (P2CandidateKey::player(1), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(2), RejectionReason::InsufficientCash),
            (
                P2CandidateKey::player(3),
                RejectionReason::InsufficientShares
            ),
            (P2CandidateKey::player(4), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(6), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(7), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(8), RejectionReason::InvalidQuantity),
        ]
    );
    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(5)]
    );
    assert_eq!(
        output.drafts()[0].required(),
        ResVec::new(crate::Money::ZERO, 50)
    );
}

#[test]
fn p3_cash_budget_contends_across_stocks_in_account_receipt_order() {
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
    let batch = P2CandidateBatch::new(vec![
        limit(0, account, first.clone(), crate::Side::Buy, 100),
        limit(1, account, second.clone(), crate::Side::Buy, 100),
    ])
    .unwrap();
    let validation = P3StockValidation::new(
        crate::SecurityCategory::MainBoard,
        crate::Money::from_cents(1_100),
        crate::Money::from_cents(900),
    );
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context([(first, validation), (second, validation)]),
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert_eq!(
        output.rejected().collect::<Vec<_>>(),
        vec![(
            &P2CandidateKey::player(1),
            &RejectionReason::InsufficientCash
        )]
    );
    assert_eq!(output.drafts()[0].required().cash, reservation);
}

#[test]
fn p3_near_order_id_overflow_is_fatal_before_any_output_is_observable() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let ledger = plan.envelope_ledger().unwrap();
    let batch = P2CandidateBatch::new(vec![
        limit(0, account, code.clone(), crate::Side::Buy, 100),
        limit(1, account, code.clone(), crate::Side::Buy, 100),
    ])
    .unwrap();
    let handoff = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        ledger.clone(),
        u64::MAX - 1,
        game.setup.config.clone(),
        context([(
            code,
            P3StockValidation::new(
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
fn p3_quantity_caps_accept_exact_limits_and_reject_the_next_board_lot() {
    let account = crate::AccountId(0);
    let main = crate::StockCode("600888".to_owned());
    let chinext = crate::StockCode("300888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&account).unwrap().cash = crate::Money::from_cents(10_000_000_000);
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = vec![
        limit(0, account, main.clone(), crate::Side::Buy, 1_000_000),
        limit(1, account, main.clone(), crate::Side::Buy, 1_000_100),
        P2Candidate::new(
            P2CandidateKey::player(2),
            account,
            crate::Intent::PlaceMarket {
                code: main.clone(),
                side: crate::Side::Buy,
                qty: 1_000_000,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(3),
            account,
            crate::Intent::PlaceMarket {
                code: main.clone(),
                side: crate::Side::Buy,
                qty: 1_000_100,
            },
        ),
        limit(4, account, chinext.clone(), crate::Side::Buy, 300_000),
        limit(5, account, chinext.clone(), crate::Side::Buy, 300_100),
        P2Candidate::new(
            P2CandidateKey::player(6),
            account,
            crate::Intent::PlaceMarket {
                code: chinext.clone(),
                side: crate::Side::Buy,
                qty: 150_000,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(7),
            account,
            crate::Intent::PlaceMarket {
                code: chinext.clone(),
                side: crate::Side::Buy,
                qty: 150_100,
            },
        ),
    ];
    let output = P2P3Handoff::new_with_context(
        P2CandidateBatch::new(candidates).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context([
            (
                main,
                P3StockValidation::new(
                    crate::SecurityCategory::MainBoard,
                    crate::Money::from_cents(1_100),
                    crate::Money::from_cents(900),
                ),
            ),
            (
                chinext,
                P3StockValidation::new(
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
            P2CandidateKey::player(0),
            P2CandidateKey::player(2),
            P2CandidateKey::player(4),
            P2CandidateKey::player(6),
        ]
    );
    assert_eq!(
        output
            .rejected()
            .map(|(key, reason)| (key.clone(), reason.clone()))
            .collect::<Vec<_>>(),
        vec![
            (P2CandidateKey::player(1), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(3), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(5), RejectionReason::InvalidQuantity),
            (P2CandidateKey::player(7), RejectionReason::InvalidQuantity),
        ]
    );
    assert_eq!(
        output
            .drafts()
            .iter()
            .map(|draft| (draft.kind(), draft.qty(), draft.sealed_index()))
            .collect::<Vec<_>>(),
        vec![
            (P3PlaceKind::Limit, 1_000_000, 0),
            (P3PlaceKind::Market, 1_000_000, 2),
            (P3PlaceKind::Limit, 300_000, 4),
            (P3PlaceKind::Market, 150_000, 6),
        ]
    );
}

#[test]
fn p3_sell_candidates_compete_for_one_private_same_batch_share_budget() {
    let account = crate::AccountId(0);
    let code = crate::StockCode("600888".to_owned());
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 150, crate::Money::from_cents(150_000))
        .unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let output = P2P3Handoff::new_with_context(
        P2CandidateBatch::new(vec![
            limit(0, account, code.clone(), crate::Side::Sell, 100),
            limit(1, account, code.clone(), crate::Side::Sell, 50),
            limit(2, account, code.clone(), crate::Side::Sell, 100),
        ])
        .unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context([(
            code,
            P3StockValidation::new(
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
        vec![P2CandidateKey::player(0), P2CandidateKey::player(1)]
    );
    assert_eq!(
        output.rejected().collect::<Vec<_>>(),
        vec![(
            &P2CandidateKey::player(2),
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
    stocks: impl IntoIterator<Item = (crate::StockCode, P3StockValidation)>,
) -> P3ValidationContext {
    P3ValidationContext::new(stocks).unwrap()
}

fn limit(
    player_queue_index: u64,
    account: crate::AccountId,
    code: crate::StockCode,
    side: crate::Side,
    qty: u32,
) -> P2Candidate {
    P2Candidate::new(
        P2CandidateKey::player(player_queue_index),
        account,
        crate::Intent::PlaceLimit {
            code,
            side,
            price: crate::Money::from_cents(900),
            qty,
        },
    )
}
