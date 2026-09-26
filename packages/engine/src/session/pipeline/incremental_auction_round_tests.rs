use super::super::super::p3_context::build_p3_validation_context;
use super::super::super::{plan_tick, P2Candidate, P2P3Handoff, PhaseInput};
use super::super::super::{
    with_executor_perturbation, ExecutorBoundary, ExecutorPermutation, ExecutorPerturbation,
};
use super::*;
use crate::{AccountId, Intent, OrderId, Side};

fn prepare(session: &GameSession, intents: Vec<(AccountId, Intent)>) -> P3ValidationOutput {
    let plan = plan_tick(PhaseInput { session }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(index, (owner, intent))| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                owner,
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::new(candidates).unwrap();
    P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        session.next_order_id,
        session.setup.config.clone(),
        build_p3_validation_context(session).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap()
}

#[test]
fn incremental_auction_accepts_reverse_keys_and_rejects_replayed_identity() {
    let session = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(900),
        42,
    )
    .unwrap();
    let code = session.markets.keys().next().unwrap().clone();
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();
    let cancel = |candidate_key, sealed_index| P3ValidatedOperation::Cancel {
        candidate_key,
        sealed_index,
        account: AccountId(0),
        code: code.clone(),
        order_id: OrderId(77),
    };

    let round = coordinator
        .apply_round(vec![
            cancel(P2CandidateKey::player(1), 0),
            cancel(P2CandidateKey::player(0), 1),
        ])
        .unwrap();
    assert_eq!(round.facts.len(), 2);
    assert_eq!(coordinator.seen_candidate_keys.len(), 2);
    assert_eq!(coordinator.seen_sealed_indices.len(), 2);
    assert_eq!(coordinator.applied_operation_count, 2);
    assert!(coordinator
        .apply_round(vec![cancel(P2CandidateKey::player(1), 2)])
        .is_err());
    assert_eq!(coordinator.applied_operation_count, 2);
}

#[test]
fn incremental_auction_does_not_order_stocks_by_sealed_identity() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let session = GameSession::new(setup, 42).unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();
    let cancel = |code: StockCode, key, index| P3ValidatedOperation::Cancel {
        candidate_key: P2CandidateKey::player(key),
        sealed_index: index,
        account: AccountId(0),
        code,
        order_id: OrderId(77),
    };

    coordinator
        .apply_round(vec![cancel(codes[1].clone(), 0, 2)])
        .unwrap();
    coordinator
        .apply_round(vec![cancel(codes[0].clone(), 1, 1)])
        .unwrap();
    assert_eq!(coordinator.applied_operation_count, 2);
    coordinator
        .apply_round(vec![cancel(codes[1].clone(), 2, 0)])
        .unwrap();
    assert_eq!(coordinator.applied_operation_count, 3);
    assert!(coordinator
        .apply_round(vec![cancel(codes[1].clone(), 2, 3)])
        .is_err());
    assert_eq!(coordinator.applied_operation_count, 3);
}

#[test]
fn incremental_auction_unknown_stock_cancels_do_not_create_stock_order() {
    let session = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(900),
        42,
    )
    .unwrap();
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();
    let unknown = StockCode("999999".to_owned());
    for (key, index) in [(0, 2), (1, 1)] {
        let round = coordinator
            .apply_round(vec![P3ValidatedOperation::Cancel {
                candidate_key: P2CandidateKey::player(key),
                sealed_index: index,
                account: AccountId(0),
                code: unknown.clone(),
                order_id: OrderId(77),
            }])
            .unwrap();
        assert!(matches!(
            &round.facts[0].outcome,
            AuctionLifecycleFact::Rejected {
                reason: RejectionReason::UnknownStock,
                ..
            }
        ));
    }
    assert_eq!(coordinator.applied_operation_count, 2);
}

#[test]
fn incremental_auction_worker_failure_keeps_stock_and_detached_facts_retryable() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let session = GameSession::new(setup, 42).unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    let validation = prepare(
        &session,
        codes
            .iter()
            .cloned()
            .map(|code| {
                (
                    AccountId(0),
                    Intent::PlaceLimit {
                        code,
                        side: Side::Buy,
                        price: Money::from_cents(990),
                        qty: 100,
                    },
                )
            })
            .collect(),
    );
    let mut operations = validation.operations().to_vec();
    assert_eq!(operations.len(), 2);
    operations.push(P3ValidatedOperation::Cancel {
        candidate_key: P2CandidateKey::player(2),
        sealed_index: 2,
        account: AccountId(0),
        code: StockCode("unknown".to_owned()),
        order_id: OrderId(77),
    });
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();
    let failing_envelope = match &operations[1] {
        P3ValidatedOperation::Place(draft) => draft.materialize_envelope(),
        _ => panic!("expected a placement"),
    };
    let failing_stock = coordinator.stocks.get_mut(&codes[1]).unwrap();
    let clean_ledger = failing_stock.ledger.clone();
    failing_stock
        .ledger
        .insert_created([failing_envelope])
        .unwrap();

    assert!(coordinator.apply_round(operations.clone()).is_err());
    for code in &codes {
        let stock = &coordinator.stocks[code];
        assert!(stock.completion.state.orders().is_empty());
        assert!(stock.created_envelopes.is_empty());
        assert!(stock.receipts.is_empty());
        assert!(stock.lifecycle_facts.is_empty());
        assert!(stock.event_facts.is_empty());
    }
    assert!(coordinator.detached_event_facts.is_empty());
    assert!(coordinator.detached_lifecycle_facts.is_empty());
    assert!(coordinator.seen_candidate_keys.is_empty());
    assert!(coordinator.seen_sealed_indices.is_empty());
    assert_eq!(coordinator.applied_operation_count, 0);

    coordinator.stocks.get_mut(&codes[1]).unwrap().ledger = clean_ledger;
    let recovered = coordinator.apply_round(operations).unwrap();
    assert_eq!(recovered.facts.len(), 3);
    for code in &codes {
        assert!(recovered.facts.iter().any(|fact| matches!(
            &fact.outcome,
            AuctionLifecycleFact::Accepted { code: accepted, .. } if accepted == code
        )));
    }
    assert!(recovered.facts.iter().any(|fact| matches!(
        &fact.outcome,
        AuctionLifecycleFact::Rejected {
            code,
            reason: RejectionReason::UnknownStock,
            ..
        } if code.0 == "unknown"
    )));
    assert_eq!(coordinator.detached_event_facts.len(), 1);
    assert_eq!(coordinator.detached_lifecycle_facts.len(), 1);
    assert_eq!(coordinator.seen_candidate_keys.len(), 3);
    assert_eq!(coordinator.seen_sealed_indices.len(), 3);
}

#[test]
fn two_auction_worker_errors_select_first_stock_under_reversed_delivery() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let session = GameSession::new(setup, 42).unwrap();
    let codes = session.markets.keys().cloned().collect::<Vec<_>>();
    assert_eq!(codes.len(), 2);
    let validation = prepare(
        &session,
        codes
            .iter()
            .cloned()
            .map(|code| {
                (
                    AccountId(0),
                    Intent::PlaceLimit {
                        code,
                        side: Side::Buy,
                        price: Money::from_cents(990),
                        qty: 100,
                    },
                )
            })
            .collect(),
    );
    let operations = validation.operations().to_vec();
    assert_eq!(operations.len(), 2);
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&session).unwrap(),
    )
    .unwrap();
    let duplicate = match &operations[0] {
        P3ValidatedOperation::Place(draft) => draft.materialize_envelope(),
        _ => panic!("expected first placement"),
    };
    coordinator
        .stocks
        .get_mut(&codes[0])
        .unwrap()
        .ledger
        .insert_created([duplicate])
        .unwrap();
    coordinator
        .stocks
        .get_mut(&codes[1])
        .unwrap()
        .completion
        .price_tick = Money::ZERO;

    let first_error = apply_auction_stock_round(
        codes[0].clone(),
        coordinator.stocks[&codes[0]].clone(),
        vec![operations[0].clone()],
    )
    .err()
    .expect("first stock must reject a duplicate envelope");
    let second_error = apply_auction_stock_round(
        codes[1].clone(),
        coordinator.stocks[&codes[1]].clone(),
        vec![operations[1].clone()],
    )
    .err()
    .expect("second stock must reject an invalid price tick");
    assert_ne!(first_error, second_error);

    let perturbation = ExecutorPerturbation {
        account_shards: ExecutorPermutation::Canonical,
        stock_shards: ExecutorPermutation::Reverse,
        worker_results: ExecutorPermutation::Reverse,
        disable_merge: None,
    };
    let (result, records) =
        with_executor_perturbation(perturbation, || coordinator.apply_round(operations)).unwrap();
    assert_eq!(result.unwrap_err(), first_error);
    for boundary in [
        ExecutorBoundary::P4AuctionStockShards,
        ExecutorBoundary::P4AuctionWorkerResults,
    ] {
        assert!(records.iter().any(|record| {
            record.boundary == boundary
                && record.identities == [codes[1].0.clone(), codes[0].0.clone()]
        }));
    }
    assert_eq!(coordinator.applied_operation_count, 0);
    assert!(coordinator.seen_candidate_keys.is_empty());
    assert!(coordinator.stocks[&codes[0]]
        .completion
        .state
        .orders()
        .is_empty());
    assert!(coordinator.stocks[&codes[1]]
        .completion
        .state
        .orders()
        .is_empty());
}
