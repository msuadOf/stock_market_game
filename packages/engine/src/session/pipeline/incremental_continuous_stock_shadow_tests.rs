use super::*;
use crate::session::pipeline::{
    plan_tick, Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents, P2Candidate,
    P2CandidateBatch, P2CandidateKey, P2P3Handoff, P3StockValidation, P3ValidationContext,
    PhaseInput, ReceiptKind, ResVec,
};
use crate::session::pipeline::{
    with_executor_perturbation, ExecutorBoundary, ExecutorPermutation, ExecutorPerturbation,
};
use crate::{
    AccountId, Event, GameConfig, GameSession, Intent, Money, Order, OrderId, SecurityCategory,
    Side,
};

#[test]
fn same_stock_cancellations_follow_supplied_order_even_when_identity_numbers_descend() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let maker = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        100,
    );
    let input = stock_input(market, vec![maker], GameConfig::proposed_defaults());
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![input]).unwrap();
    let cancel = |key, sealed_index| P3ValidatedOperation::Cancel {
        candidate_key: P2CandidateKey::player(key),
        sealed_index,
        account: AccountId(11),
        code: code.clone(),
        order_id: OrderId(100),
    };

    let round = coordinator
        .apply_round(vec![cancel(9, 9), cancel(2, 2)])
        .unwrap();
    let first = round
        .facts
        .iter()
        .find(|fact| fact.candidate_key == P2CandidateKey::player(9))
        .unwrap();
    let second = round
        .facts
        .iter()
        .find(|fact| fact.candidate_key == P2CandidateKey::player(2))
        .unwrap();
    assert!(matches!(
        first.outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled { .. })
    ));
    assert!(matches!(
        second.outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
            reason: ContinuousCancelRejection::OrderNotFound,
            ..
        })
    ));
    assert_eq!(round.projections[&code].market.resting_order_count(), 0);
}

#[test]
fn post_p0_stock_shadow_survives_routes_and_same_tick_cancel_sees_the_created_order() {
    let code = stock("600888");
    let (operations, config, first_order_id) = validated_operations(&code, |next_order_id| {
        vec![
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(990),
                qty: 100,
            },
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(next_order_id),
            },
        ]
    });
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        empty_market(&code),
        vec![],
        config,
    )])
    .unwrap();

    let first = coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    assert_eq!(first.facts.len(), 1);
    assert_eq!(first.facts[0].allocated_order_id(), Some(first_order_id));
    assert!(matches!(
        first.facts[0].outcome(),
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Resting {
                order_id,
                remaining_qty: 100,
                ..
            },
            original_qty: 100,
        } if *order_id == first_order_id
    ));
    assert_eq!(first.projections[&code].market.resting_order_count(), 1);

    let second = coordinator
        .apply_round(vec![operations[1].clone()])
        .unwrap();
    assert_eq!(second.facts.len(), 1);
    assert!(matches!(
        second.facts[0].outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
            order_id,
            ..
        }) if *order_id == first_order_id
    ));
    assert_eq!(second.projections[&code].market.resting_order_count(), 0);

    let finish = coordinator.finish().unwrap();
    assert!(finish.detached_facts.is_empty());
    assert_eq!(finish.workers.len(), 1);
    assert_eq!(finish.workers[0].created_envelopes.len(), 1);
    assert_eq!(finish.workers[0].place_facts.len(), 1);
    assert_eq!(finish.workers[0].cancel_facts.len(), 1);
}

#[test]
fn immediate_full_fill_has_a_typed_outcome_without_an_order_accepted_fact() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let maker = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        100,
    );
    let (operations, config, incoming_id) = validated_operations(&code, |_| {
        vec![Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 100,
        }]
    });
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        market,
        vec![maker],
        config,
    )])
    .unwrap();

    let round = coordinator.apply_round(operations).unwrap();

    assert_eq!(round.facts.len(), 1);
    assert_eq!(round.facts[0].allocated_order_id(), Some(incoming_id));
    assert!(matches!(
        round.facts[0].outcome(),
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Filled {
                order_id,
                filled_qty: 100,
                ..
            },
            original_qty: 100,
        } if *order_id == incoming_id
    ));
    assert_eq!(round.trades.len(), 1);
    assert_eq!(round.receipts.len(), 2);
    assert!(
        crate::session::pipeline::p7_p4_producers::adapt_continuous_execution_facts(&round.facts)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn one_incoming_order_fills_each_resting_maker_once() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let first = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        100,
    );
    let second = add_resting(
        &mut market,
        &code,
        AccountId(12),
        OrderId(101),
        Side::Sell,
        100,
    );
    let (operations, config, _) = validated_operations(&code, |_| {
        vec![Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 200,
        }]
    });
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        market,
        vec![first, second],
        config,
    )])
    .unwrap();

    let round = coordinator.apply_round(operations).unwrap();

    assert_eq!(round.trades.len(), 2);
    assert_eq!(round.receipts.len(), 4);
    assert_eq!(round.projections[&code].market.resting_order_count(), 0);
    assert_eq!(
        round
            .receipts
            .iter()
            .filter(|receipt| receipt.envelope.side == Side::Sell)
            .map(|receipt| (
                receipt.envelope.account,
                receipt.envelope.order,
                receipt.kind,
                receipt.qty_after,
            ))
            .collect::<Vec<_>>(),
        vec![
            (AccountId(11), OrderId(100), ReceiptKind::Fill, 0),
            (AccountId(12), OrderId(101), ReceiptKind::Fill, 0),
        ]
    );
}

#[test]
fn stock_local_trade_identity_continues_across_adaptive_routes() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let first = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        100,
    );
    let second = add_resting(
        &mut market,
        &code,
        AccountId(12),
        OrderId(101),
        Side::Sell,
        100,
    );
    let (operations, config, _) = validated_operations(&code, |_| {
        vec![
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        ]
    });
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        market,
        vec![first, second],
        config,
    )])
    .unwrap();

    let first_round = coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    let second_round = coordinator
        .apply_round(vec![operations[1].clone()])
        .unwrap();

    assert_eq!(first_round.trades[0].stock_local_trade_event_index, 0);
    assert_eq!(second_round.trades[0].stock_local_trade_event_index, 1);
    let finish = coordinator.finish().unwrap();
    assert_eq!(
        finish.workers[0]
            .trades
            .iter()
            .map(|fact| fact.stock_local_trade_event_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn sell_maker_conservation_survives_partial_fills_across_routes() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let maker = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        200,
    );
    let (operations, config, _) = validated_operations(&code, |_| {
        vec![
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        ]
    });
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        market,
        vec![maker],
        config,
    )])
    .unwrap();

    let first = coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    let second = coordinator
        .apply_round(vec![operations[1].clone()])
        .unwrap();

    assert_eq!(first.trades.len(), 1);
    assert_eq!(second.trades.len(), 1);
    assert_eq!(first.receipts.len(), 2);
    assert_eq!(second.receipts.len(), 2);
    assert_eq!(first.projections[&code].market.resting_order_count(), 1);
    assert_eq!(second.projections[&code].market.resting_order_count(), 0);

    let finish = coordinator.finish().unwrap();
    assert_eq!(finish.workers[0].receipts.len(), 4);
    assert_eq!(finish.workers[0].terminal_keys.len(), 3);
}

#[test]
fn buy_maker_conservation_survives_partial_fills_across_routes() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let maker = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Buy,
        200,
    );
    let (operations, config) = validated_sell_operations(&code);
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        market,
        vec![maker],
        config,
    )])
    .unwrap();

    let first = coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    let second = coordinator
        .apply_round(vec![operations[1].clone()])
        .unwrap();

    assert_eq!(first.trades.len(), 1);
    assert_eq!(second.trades.len(), 1);
    assert_eq!(first.projections[&code].market.resting_order_count(), 1);
    assert_eq!(second.projections[&code].market.resting_order_count(), 0);
    let finish = coordinator.finish().unwrap();
    assert_eq!(finish.workers[0].receipts.len(), 4);
}

#[test]
fn tick_start_maker_can_be_canceled_after_a_partial_fill_in_an_earlier_route() {
    let code = stock("600888");
    let mut market = empty_market(&code);
    let maker = add_resting(
        &mut market,
        &code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        200,
    );
    let (operations, config, _) = validated_operations(&code, |_| {
        vec![Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 100,
        }]
    });
    let cancel = P3ValidatedOperation::Cancel {
        candidate_key: P2CandidateKey::player(1),
        sealed_index: 1,
        account: AccountId(11),
        code: code.clone(),
        order_id: OrderId(100),
    };
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        market,
        vec![maker],
        config,
    )])
    .unwrap();

    coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    let canceled = coordinator.apply_round(vec![cancel]).unwrap();

    assert!(matches!(
        canceled.facts[0].outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
            order_id: OrderId(100),
            remaining_qty: 100,
            ..
        })
    ));
    assert_eq!(canceled.receipts.len(), 1);
    let finish = coordinator.finish().unwrap();
    assert_eq!(finish.workers[0].receipts.len(), 3);
}

#[test]
fn equal_stock_local_indices_are_isolated_by_full_candidate_and_stock_identity() {
    let first_code = stock("600888");
    let second_code = stock("000001");
    let mut first_market = empty_market(&first_code);
    let mut second_market = empty_market(&second_code);
    let first_maker = add_resting(
        &mut first_market,
        &first_code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        100,
    );
    let second_maker = add_resting(
        &mut second_market,
        &second_code,
        AccountId(12),
        OrderId(101),
        Side::Sell,
        100,
    );
    let (operations, config) = validated_operations_for_stocks(
        &[first_code.clone(), second_code.clone()],
        vec![
            Intent::PlaceMarket {
                code: first_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: second_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        ],
    );
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![
        stock_input(first_market, vec![first_maker], config.clone()),
        stock_input(second_market, vec![second_maker], config),
    ])
    .unwrap();
    let first_key = operations[0].candidate_key().clone();
    let second_key = operations[1].candidate_key().clone();

    let round = coordinator.apply_round(operations).unwrap();

    assert_eq!(round.facts.len(), 2);
    assert_eq!(
        round
            .facts
            .iter()
            .map(|fact| fact.candidate_key().clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first_key, second_key])
    );
    assert_eq!(round.trades.len(), 2);
    assert_eq!(round.trades[0].stock_local_trade_event_index, 0);
    assert_eq!(round.trades[1].stock_local_trade_event_index, 0);
    assert_ne!(round.trades[0].stock, round.trades[1].stock);
    assert_eq!(round.projections.len(), 2);
}

#[test]
fn unknown_stock_cancel_is_a_detached_typed_rejection_not_a_fatal_error() {
    let known = stock("600888");
    let unknown = stock("999999");
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        empty_market(&known),
        vec![],
        GameConfig::proposed_defaults(),
    )])
    .unwrap();
    let operation = cancel_operation(P2CandidateKey::player(0), 0, AccountId(9), &unknown);

    let round = coordinator.apply_round(vec![operation]).unwrap();

    assert_eq!(round.facts.len(), 1);
    assert!(round.projections.is_empty());
    assert!(round.receipts.is_empty());
    assert!(matches!(
        round.facts[0].outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
            code,
            reason: ContinuousCancelRejection::UnknownStock,
            ..
        }) if code == &unknown
    ));
    let finish = coordinator.finish().unwrap();
    assert_eq!(finish.detached_facts.len(), 1);
    assert_eq!(finish.workers.len(), 1);
    assert!(finish.workers[0].cancel_facts.is_empty());
}

#[test]
fn reverse_candidate_keys_are_accepted_but_replay_keeps_private_boundary() {
    let code = stock("600888");
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![stock_input(
        empty_market(&code),
        vec![],
        GameConfig::proposed_defaults(),
    )])
    .unwrap();
    let first = cancel_operation(P2CandidateKey::plan_chain(1), 0, AccountId(1), &code);
    coordinator.apply_round(vec![first.clone()]).unwrap();

    let second = cancel_operation(P2CandidateKey::player(0), 1, AccountId(1), &code);
    let round = coordinator.apply_round(vec![second]).unwrap();

    assert_eq!(round.facts.len(), 1);
    assert_eq!(round.facts[0].sealed_index(), 1);
    assert_eq!(coordinator.stocks[&code].cancel_facts.len(), 2);
    assert!(coordinator.apply_round(vec![first]).is_err());
    assert!(coordinator.finish().is_err());
}

#[test]
fn independent_stocks_accept_operations_without_a_global_sealed_order() {
    let first_code = stock("600888");
    let second_code = stock("000001");
    let config = GameConfig::proposed_defaults();
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![
        stock_input(empty_market(&first_code), vec![], config.clone()),
        stock_input(empty_market(&second_code), vec![], config),
    ])
    .unwrap();

    let later_identity = cancel_operation(P2CandidateKey::player(1), 9, AccountId(1), &first_code);
    let earlier_identity =
        cancel_operation(P2CandidateKey::player(0), 2, AccountId(2), &second_code);
    let first_round = coordinator
        .apply_round(vec![later_identity, earlier_identity])
        .unwrap();
    assert_eq!(first_round.facts.len(), 2);
    assert!(first_round.facts.iter().all(|fact| matches!(
        fact.outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
            reason: ContinuousCancelRejection::OrderNotFound,
            ..
        })
    )));

    let next_round = coordinator
        .apply_round(vec![cancel_operation(
            P2CandidateKey::player(2),
            3,
            AccountId(2),
            &second_code,
        )])
        .unwrap();
    assert_eq!(next_round.facts.len(), 1);
    assert_eq!(next_round.facts[0].sealed_index(), 3);

    let descending = coordinator
        .apply_round(vec![cancel_operation(
            P2CandidateKey::player(3),
            1,
            AccountId(2),
            &second_code,
        )])
        .unwrap();
    assert_eq!(descending.facts[0].sealed_index(), 1);
    assert!(matches!(
        descending.facts[0].outcome(),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
            reason: ContinuousCancelRejection::OrderNotFound,
            ..
        })
    ));
    let following = coordinator
        .apply_round(vec![cancel_operation(
            P2CandidateKey::player(4),
            4,
            AccountId(2),
            &second_code,
        )])
        .unwrap();
    assert_eq!(following.facts[0].sealed_index(), 4);
    assert!(coordinator
        .apply_round(vec![cancel_operation(
            P2CandidateKey::player(5),
            1,
            AccountId(2),
            &second_code,
        )])
        .is_err());
}

#[test]
fn one_stock_worker_failure_invalidates_the_discardable_tick_coordinator() {
    let first_code = stock("600888");
    let second_code = stock("000001");
    let mut first_market = empty_market(&first_code);
    let mut second_market = empty_market(&second_code);
    let first_maker = add_resting(
        &mut first_market,
        &first_code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        200,
    );
    let second_maker = add_resting(
        &mut second_market,
        &second_code,
        AccountId(12),
        OrderId(101),
        Side::Sell,
        100,
    );
    let (operations, config) = validated_operations_for_stocks(
        &[first_code.clone(), second_code.clone()],
        vec![
            Intent::PlaceMarket {
                code: first_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: first_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: second_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        ],
    );
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![
        stock_input(first_market, vec![first_maker], config.clone()),
        stock_input(second_market, vec![second_maker], config),
    ])
    .unwrap();

    coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    coordinator
        .stocks
        .get_mut(&second_code)
        .unwrap()
        .next_trade_event_index = u64::MAX;

    assert!(coordinator
        .apply_round(vec![operations[1].clone(), operations[2].clone()])
        .is_err());

    assert!(coordinator.failed);
    assert!(coordinator
        .apply_round(vec![operations[1].clone(), operations[2].clone()])
        .is_err());
    assert!(coordinator.finish().is_err());
}

#[test]
fn two_stock_worker_errors_select_first_stock_under_reversed_delivery() {
    let first_code = stock("000001");
    let second_code = stock("600888");
    let mut first_market = empty_market(&first_code);
    let mut second_market = empty_market(&second_code);
    let first_maker = add_resting(
        &mut first_market,
        &first_code,
        AccountId(11),
        OrderId(100),
        Side::Sell,
        100,
    );
    let second_maker = add_resting(
        &mut second_market,
        &second_code,
        AccountId(12),
        OrderId(101),
        Side::Sell,
        100,
    );
    let (operations, config) = validated_operations_for_stocks(
        &[first_code.clone(), second_code.clone()],
        vec![
            Intent::PlaceMarket {
                code: first_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: second_code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        ],
    );
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(vec![
        stock_input(first_market, vec![first_maker], config.clone()),
        stock_input(second_market, vec![second_maker], config),
    ])
    .unwrap();
    coordinator
        .stocks
        .get_mut(&first_code)
        .unwrap()
        .next_trade_event_index = u64::MAX;
    coordinator.stocks.get_mut(&second_code).unwrap().ledger =
        EnvelopeLedger::new(0, Vec::<Envelope>::new()).unwrap();

    let first_error = apply_stock_round(
        first_code.clone(),
        coordinator.stocks[&first_code].clone(),
        vec![operations[0].clone()],
    )
    .err()
    .expect("first stock must fail after trade index overflow");
    let second_error = apply_stock_round(
        second_code.clone(),
        coordinator.stocks[&second_code].clone(),
        vec![operations[1].clone()],
    )
    .err()
    .expect("second stock must fail with a missing envelope");
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
    assert!(records.iter().any(|record| {
        record.boundary == ExecutorBoundary::P4ContinuousWorkerResults
            && record.identities == [second_code.0.clone(), first_code.0.clone()]
    }));
    assert!(records.iter().any(|record| {
        record.boundary == ExecutorBoundary::P4ContinuousStockShards
            && record.identities == [second_code.0.clone(), first_code.0.clone()]
    }));
    assert!(coordinator.failed);
    assert_eq!(coordinator.applied_operation_count, 0);
    assert!(coordinator.finish().is_err());
}

#[test]
fn consuming_finish_settles_all_route_receipts_and_detached_facts_exactly_once() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    let codes = game.markets.keys().cloned().collect::<Vec<_>>();
    let account = AccountId(1);
    let mut makers = Vec::new();
    for (index, code) in codes.iter().enumerate() {
        game.accounts
            .get_mut(&account)
            .unwrap()
            .grant_position(code.clone(), 100, Money::from_cents(1_000))
            .unwrap();
        let order_id = OrderId(10_000 + u64::try_from(index).unwrap());
        makers.push(add_resting(
            game.markets.get_mut(code).unwrap(),
            code,
            account,
            order_id,
            Side::Sell,
            100,
        ));
    }
    game.envelope_ledger =
        EnvelopeLedger::new(0, makers.iter().map(|maker| maker.envelope.clone())).unwrap();
    game.next_receipt_base = 0;
    let operations = validated_operations_in_session(
        &game,
        account,
        codes
            .iter()
            .map(|code| Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            })
            .collect(),
    );
    let incoming_ids = operations
        .iter()
        .map(|operation| match operation {
            P3ValidatedOperation::Place(draft) => draft.order_id(),
            P3ValidatedOperation::Cancel { .. } => panic!("expected a place operation"),
        })
        .collect::<Vec<_>>();
    let inputs =
        crate::session::pipeline::p4_continuous_adapter::prepare_incremental_continuous_inputs(
            &game,
        )
        .unwrap();
    let mut coordinator = IncrementalContinuousStockCoordinator::from_post_p0(inputs).unwrap();

    let first_round = coordinator
        .apply_round(vec![operations[0].clone()])
        .unwrap();
    let second_round = coordinator
        .apply_round(vec![operations[1].clone()])
        .unwrap();
    assert_eq!(first_round.trades[0].stock_local_trade_event_index, 0);
    assert_eq!(second_round.trades[0].stock_local_trade_event_index, 0);
    assert_ne!(first_round.trades[0].stock, second_round.trades[0].stock);
    let unknown = stock("999999");
    coordinator
        .apply_round(vec![P3ValidatedOperation::Cancel {
            candidate_key: P2CandidateKey::player(2),
            sealed_index: 2,
            account,
            code: unknown.clone(),
            order_id: OrderId(77),
        }])
        .unwrap();
    let finish = coordinator.finish().unwrap();
    let cash_before = game.accounts[&account].cash;
    let experience_before = game.retail_experience[&account].clone();
    let seq_before = game.seq;

    let output = crate::session::pipeline::p4_p7_session_transaction::apply_incremental_session_p4_p7_transaction(
        &mut game,
        finish,
        vec![],
    )
    .unwrap();

    assert_eq!(
        output
            .receipts
            .iter()
            .map(|receipt| receipt.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(output
        .receipts
        .iter()
        .all(|receipt| receipt.kind == ReceiptKind::Fill));
    assert_eq!(output.p6.settlement.applied_receipts, 4);
    assert_eq!(output.p6.settlement.applied_groups, 4);
    assert_eq!(game.envelope_ledger.iter().count(), 0);
    assert_eq!(game.envelope_ledger.terminal_count(), 4);
    assert_eq!(game.next_receipt_base, 4);
    assert!(codes
        .iter()
        .all(|code| game.markets[code].resting_order_count() == 0));

    let total_fees = output
        .receipts
        .iter()
        .map(|receipt| receipt.charged.total().unwrap())
        .fold(Money::ZERO, |sum, fee| sum.add(fee).unwrap());
    assert_eq!(
        game.accounts[&account].cash,
        cash_before.sub(total_fees).unwrap()
    );
    for code in &codes {
        let position = &game.accounts[&account].positions[code];
        assert_eq!(position.qty, 100);
        assert_eq!(position.t1_locked, 100);
    }
    let experience = &game.retail_experience[&account];
    assert_ne!(experience, &experience_before);
    for (index, code) in codes.iter().enumerate() {
        assert_eq!(
            experience.stocks[code].last_buy_order_id,
            Some(incoming_ids[index].0)
        );
        assert_eq!(
            experience.stocks[code].last_sell_order_id,
            Some(makers[index].envelope.key().order.0)
        );
    }

    assert_eq!(
        output
            .events
            .iter()
            .filter(|event| matches!(
                event,
                Event::IntentRejected {
                    code: rejected,
                    reason: crate::RejectionReason::UnknownStock,
                    ..
                } if rejected == &unknown
            ))
            .count(),
        1
    );
    assert_eq!(
        output
            .events
            .iter()
            .filter(|event| matches!(event, Event::Trade { .. }))
            .count(),
        2
    );
    assert_eq!(game.seq, seq_before + 3);
}

fn validated_operations(
    code: &StockCode,
    build: impl FnOnce(u64) -> Vec<Intent>,
) -> (Vec<P3ValidatedOperation>, GameConfig, OrderId) {
    let account = AccountId(0);
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let first_order_id = OrderId(game.next_order_id);
    let candidates = build(game.next_order_id)
        .into_iter()
        .enumerate()
        .map(|(index, intent)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                account,
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::new(candidates).unwrap();
    let context = P3ValidationContext::new([(
        code.clone(),
        P3StockValidation::new(
            SecurityCategory::MainBoard,
            Money::from_cents(1_100),
            Money::from_cents(900),
        ),
    )])
    .unwrap();
    let config = game.setup.config.clone();
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();
    assert_eq!(output.rejected().count(), 0);
    (output.operations().to_vec(), config, first_order_id)
}

fn validated_operations_for_stocks(
    codes: &[StockCode],
    intents: Vec<Intent>,
) -> (Vec<P3ValidatedOperation>, GameConfig) {
    let account = AccountId(0);
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(index, intent)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                account,
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::new(candidates).unwrap();
    let context = P3ValidationContext::new(codes.iter().cloned().map(|code| {
        (
            code,
            P3StockValidation::new(
                SecurityCategory::MainBoard,
                Money::from_cents(1_100),
                Money::from_cents(900),
            ),
        )
    }))
    .unwrap();
    let config = game.setup.config.clone();
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();
    assert_eq!(output.rejected().count(), 0);
    (output.operations().to_vec(), config)
}

fn validated_sell_operations(code: &StockCode) -> (Vec<P3ValidatedOperation>, GameConfig) {
    let account = AccountId(0);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 200, Money::from_cents(1_000))
        .unwrap();
    let config = game.setup.config.clone();
    let operations = validated_operations_in_session(
        &game,
        account,
        vec![
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Sell,
                qty: 100,
            },
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Sell,
                qty: 100,
            },
        ],
    );
    (operations, config)
}

fn validated_operations_in_session(
    game: &GameSession,
    account: AccountId,
    intents: Vec<Intent>,
) -> Vec<P3ValidatedOperation> {
    let plan = plan_tick(PhaseInput { session: game }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(index, intent)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                account,
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::new(candidates).unwrap();
    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        crate::session::pipeline::p3_context::build_p3_validation_context(game).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap();
    assert_eq!(output.rejected().count(), 0);
    output.operations().to_vec()
}

fn stock_input(
    market: Market,
    envelopes: Vec<ContinuousEnvelopeSnapshot>,
    config: GameConfig,
) -> ContinuousStockInput {
    ContinuousStockInput {
        phase: TradingPhase::Continuous,
        market,
        envelopes,
        operations: vec![],
        config,
    }
}

fn cancel_operation(
    candidate_key: P2CandidateKey,
    sealed_index: u64,
    account: AccountId,
    code: &StockCode,
) -> P3ValidatedOperation {
    P3ValidatedOperation::Cancel {
        candidate_key,
        sealed_index,
        account,
        code: code.clone(),
        order_id: OrderId(77),
    }
}

fn add_resting(
    market: &mut Market,
    code: &StockCode,
    owner: AccountId,
    order_id: OrderId,
    side: Side,
    qty: u32,
) -> ContinuousEnvelopeSnapshot {
    let price = Money::from_cents(1_000);
    let order = Order {
        id: order_id,
        side,
        price,
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner,
        seq: 0,
    };
    let result = market.place(order).unwrap();
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    let key = EnvelopeKey {
        account: owner,
        stock: code.clone(),
        order: order_id,
        side,
    };
    let audit = EnvelopeAudit {
        limit: price,
        remaining_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    };
    let live = match side {
        Side::Buy => ResVec::new(
            crate::session::buy_order_reservation(
                &GameConfig::proposed_defaults(),
                price,
                qty,
                Money::ZERO,
            )
            .unwrap(),
            0,
        ),
        Side::Sell => ResVec::new(Money::ZERO, qty),
    };
    ContinuousEnvelopeSnapshot {
        envelope: Envelope::tick_start_existing(key, live.cash, live.shares, audit),
        audit,
    }
}

fn empty_market(code: &StockCode) -> Market {
    Market::new(
        code.clone(),
        Money::from_cents(1_000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap()
}

fn stock(value: &str) -> StockCode {
    StockCode(value.to_owned())
}
