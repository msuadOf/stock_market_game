use super::p3_p4_normalizer::normalize_p3_p4_operations;
use super::*;
use crate::{
    AccountId, Intent, Money, OrderId, RejectionReason, SecurityCategory, Side, StockCode,
};

#[test]
fn unknown_stock_cancel_becomes_an_ordinary_rejection_without_touching_places_or_known_cancels() {
    let account = AccountId(0);
    let known = StockCode("600888".to_owned());
    let unknown = StockCode("600999".to_owned());
    let validation = validate(
        vec![known.clone()],
        vec![
            Intent::Cancel {
                code: known.clone(),
                id: OrderId(41),
            },
            Intent::Cancel {
                code: unknown.clone(),
                id: OrderId(42),
            },
            Intent::PlaceLimit {
                code: known.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
        ],
    );
    let next_order_id_after = validation.next_order_id_after();
    let draft_count = validation.drafts().len();

    let normalized = normalize_p3_p4_operations(
        validation.results(),
        validation.operations(),
        [known.clone()],
    )
    .unwrap();

    assert_eq!(
        normalized
            .operations()
            .iter()
            .map(P3ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert!(matches!(
        &normalized.operations()[0],
        P3ValidatedOperation::Cancel {
            candidate_key,
            sealed_index: 0,
            account: actual_account,
            code,
            order_id: OrderId(41),
        } if candidate_key == &P2CandidateKey::player(0)
            && *actual_account == account
            && code == &known
    ));
    assert!(matches!(
        &normalized.operations()[1],
        P3ValidatedOperation::Place(draft)
            if draft.candidate_key() == &P2CandidateKey::player(2)
                && draft.sealed_index() == 2
    ));
    assert_eq!(normalized.rejections().len(), 1);
    let rejection = &normalized.rejections()[0];
    assert_eq!(rejection.candidate_key(), &P2CandidateKey::player(1));
    assert_eq!(rejection.sealed_index(), 1);
    assert_eq!(rejection.owner(), account);
    assert_eq!(rejection.code(), &unknown);
    assert_eq!(rejection.order_id(), OrderId(42));
    assert_eq!(rejection.reason(), &RejectionReason::UnknownStock);

    // The adapter is pure: it neither allocates a new ID nor materializes an envelope.
    assert_eq!(validation.next_order_id_after(), next_order_id_after);
    assert_eq!(validation.drafts().len(), draft_count);
}

#[test]
fn unknown_stock_place_is_a_typed_contract_failure_instead_of_being_silently_filtered() {
    let code = StockCode("600888".to_owned());
    let validation = validate(
        vec![code],
        vec![Intent::PlaceLimit {
            code: StockCode("600888".to_owned()),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        }],
    );

    let error = normalize_p3_p4_operations(
        validation.results(),
        validation.operations(),
        Vec::<StockCode>::new(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("accepted place references unknown stock")
                && location == "pipeline::p3_p4_normalizer"
    ));
}

#[test]
fn inconsistent_results_and_operations_fail_atomically() {
    let code = StockCode("600888".to_owned());
    let one = validate(
        vec![code.clone()],
        vec![Intent::Cancel {
            code: code.clone(),
            id: OrderId(1),
        }],
    );
    let two = validate(
        vec![code.clone()],
        vec![
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(1),
            },
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(2),
            },
        ],
    );

    let error = normalize_p3_p4_operations(one.results(), two.operations(), [code]).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("accepted results and validated operations disagree")
                && location == "pipeline::p3_p4_normalizer"
    ));
}

#[test]
fn duplicate_known_stock_identity_is_a_typed_contract_failure() {
    let code = StockCode("600888".to_owned());
    let validation = validate(vec![code.clone()], Vec::new());

    let error = normalize_p3_p4_operations(
        validation.results(),
        validation.operations(),
        [code.clone(), code],
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("duplicate known stock")
                && location == "pipeline::p3_p4_normalizer"
    ));
}

#[test]
fn malformed_result_identity_and_operation_order_each_fail_explicitly() {
    let code = StockCode("600888".to_owned());
    let noncontiguous = [P3CandidateResult::Rejected {
        key: P2CandidateKey::player(0),
        sealed_index: 1,
        reason: RejectionReason::UnknownStock,
    }];
    let error = normalize_p3_p4_operations(&noncontiguous, &[], [code.clone()]).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("not contiguous")
                && location == "pipeline::p3_p4_normalizer"
    ));

    let duplicate_key = P2CandidateKey::player(0);
    let duplicates = [
        P3CandidateResult::Rejected {
            key: duplicate_key.clone(),
            sealed_index: 0,
            reason: RejectionReason::UnknownStock,
        },
        P3CandidateResult::Rejected {
            key: duplicate_key,
            sealed_index: 1,
            reason: RejectionReason::UnknownStock,
        },
    ];
    let error = normalize_p3_p4_operations(&duplicates, &[], [code.clone()]).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("duplicate candidate key")
                && location == "pipeline::p3_p4_normalizer"
    ));

    let validation = validate(
        vec![code.clone()],
        vec![
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(1),
            },
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(2),
            },
        ],
    );
    let mut reversed = validation.operations().to_vec();
    reversed.reverse();
    let error = normalize_p3_p4_operations(validation.results(), &reversed, [code]).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("strict global sealed_index order")
                && location == "pipeline::p3_p4_normalizer"
    ));
}

fn validate(known_stocks: Vec<StockCode>, intents: Vec<Intent>) -> P3ValidationOutput {
    let account = AccountId(0);
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = intents.into_iter().enumerate().map(|(index, intent)| {
        P2Candidate::new(
            P2CandidateKey::player(u64::try_from(index).unwrap()),
            account,
            intent,
        )
    });
    let context = P3ValidationContext::new(
        known_stocks.into_iter().map(|code| {
            (
                code,
                P3StockValidation::new(
                    SecurityCategory::MainBoard,
                    Money::from_cents(1_100),
                    Money::from_cents(900),
                ),
            )
        }),
        0,
        [(account, 0)],
        P3OpenOrderLimits::PRODUCTION,
    )
    .unwrap();
    P2P3Handoff::new_with_context(
        P2CandidateBatch::from_unsorted(candidates.collect()).unwrap(),
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap()
}
