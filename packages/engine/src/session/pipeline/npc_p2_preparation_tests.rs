use super::npc_p2_preparation::prepare_npc_p2_source;
use super::*;
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountId, Event, Intent, Money, RejectionReason, Side};

fn due_retail(seed: u64, order_size: u32) -> (GameSession, AccountId) {
    let account = AccountId(1);
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        seed,
    )
    .unwrap();
    let mut strategy = crate::strategy::StrategyState::ZiNoise(
        ZiNoiseStrategy::new(1.0, order_size, 0.5, 1).unwrap(),
    );
    strategy.set_base_observation_probability(session.npc_attention[&account].base_probability);
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(strategy.into_strategy().unwrap());
    let tick = session.tick;
    // Preserve the real profile so this fixture can also be saved and restored.
    // Choose an attention-stream position that accepts even a quiet market.
    let attention = session.npc_attention.get_mut(&account).unwrap();
    attention.next_attention_candidate_tick = tick;
    attention.rng_state = 3;
    let mut quiet_probe = attention.clone();
    assert!(quiet_probe.evaluate_candidate_with_signal(crate::AccountKind::Retail, 0.0, tick));
    session
        .attention_queue
        .push(std::cmp::Reverse((tick, account)));
    (session, account)
}

#[test]
fn no_due_npc_queues_an_empty_batch_without_building_a_market_view() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    let future_tick = session.tick + 10;
    session.pending_npc = None;
    session.attention_queue.clear();
    let accounts = session.npc_attention.keys().copied().collect::<Vec<_>>();
    for account in accounts {
        session
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = future_tick;
        session
            .attention_queue
            .push(std::cmp::Reverse((future_tick, account)));
    }
    let code = session.markets.keys().next().unwrap().clone();
    session.market_minute_closes.remove(&code);

    super::queue_npc_for_next_tick(&mut session).unwrap();
    let ready = session.pending_npc.as_ref().unwrap();
    assert_eq!(ready.observed_tick, session.tick);
    assert!(ready.observed_accounts.is_empty());
    assert!(ready.intents.is_empty());
    assert!(ready.dependencies.is_empty());
}

#[test]
fn pending_npc_dependencies_validate_only_explicit_same_stock_replacements() {
    let a = crate::StockCode("600001".to_owned());
    let b = crate::StockCode("000001".to_owned());
    let place = |code| Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: Money::from_cents(1_000),
        qty: 100,
    };
    let mut queued = crate::session::PendingNpcBatch {
        observed_tick: 0,
        observed_accounts: vec![AccountId(1), AccountId(2)],
        intents: vec![
            (
                AccountId(1),
                Intent::Cancel {
                    code: a.clone(),
                    id: crate::OrderId(1),
                },
            ),
            (
                AccountId(1),
                Intent::Cancel {
                    code: b.clone(),
                    id: crate::OrderId(2),
                },
            ),
            (AccountId(1), place(a.clone())),
            (AccountId(2), place(a)),
            (AccountId(1), place(b)),
        ],
        dependencies: vec![(0, 2), (1, 4)],
    };
    queued.validate_dependencies().unwrap();
    for invalid in [
        vec![(0, 5)],
        vec![(2, 0)],
        vec![(0, 0)],
        vec![(2, 4)],
        vec![(0, 1)],
        vec![(0, 3)],
        vec![(1, 2)],
        vec![(0, 2), (0, 2)],
    ] {
        queued.dependencies = invalid;
        assert!(
            queued.validate_dependencies().is_err(),
            "{:?}",
            queued.dependencies
        );
    }
    queued.dependencies.clear();
    queued.validate_dependencies().unwrap();
}

#[test]
fn restore_and_queue_consumption_reject_the_same_invalid_dependency() {
    let (mut session, npc) = due_retail(41, 100);
    let code = session.setup.stocks[0].code.clone();
    let mut save = session.save().unwrap();
    save.pending_npc = Some(crate::session::PendingNpcBatch {
        observed_tick: session.tick,
        observed_accounts: vec![npc],
        intents: vec![
            (
                npc,
                Intent::Cancel {
                    code: code.clone(),
                    id: crate::OrderId(9),
                },
            ),
            (
                npc,
                Intent::PlaceLimit {
                    code,
                    side: Side::Buy,
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            ),
        ],
        dependencies: vec![(0, 1)],
    });
    // A cancellation may legitimately target an order that is already gone.
    GameSession::restore(&save).unwrap();
    save.pending_npc.as_mut().unwrap().dependencies = vec![(0, 2)];
    assert!(matches!(
        GameSession::restore(&save),
        Err(crate::SessionError::InvalidSave(message)) if message.contains("pending NPC dependency")
    ));
    session.pending_npc = save.pending_npc;
    assert!(matches!(
        super::npc_p2_preparation::take_ready_npc_batch(&mut session),
        Err(StepFatal::InvariantViolation { description, .. })
            if description.contains("pending NPC dependency")
    ));
}

#[test]
fn npc_preparation_captures_one_snapshot_and_returns_it_for_plan_roots() {
    let (authority, npc) = due_retail(8, 100);
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_p2_source(&mut prospective, None).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[npc]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[npc]);
    assert!(!prepared.candidates.candidates().is_empty());
}

#[test]
fn due_institution_plan_roots_are_ready_with_the_npc_source() {
    let mut authority =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 8).unwrap();
    let institution = AccountId(1);
    crate::session::npc_working_quote_tests::force_attention_candidate(
        &mut authority,
        institution,
        0,
    );
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_p2_source(&mut prospective, None).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[institution]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[institution]);
    assert!(!prepared.roots.is_empty());

    let mut injected = authority.clone_for_tick_shadow().unwrap();
    let prepared = prepare_npc_p2_source(
        &mut injected,
        Some(crate::session::plan_chain_candidates::PlanChainOperationBatch::empty()),
    )
    .unwrap();
    assert!(prepared.roots.is_empty());
}

#[test]
fn b1_npc_p3_quantity_rejection_reaches_one_final_event() {
    let (mut session, npc) = due_retail(8, 2_000_000);
    session.accounts.get_mut(&npc).unwrap().cash = Money::from_cents(10_000_000_000);
    session.pending_npc = None;
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let before_order_id = session.next_order_id;

    let committed = super::b1_continuous_transaction::prepare_b1_continuous_tick(&mut session)
        .unwrap()
        .commit();
    assert!(matches!(
        committed.output.validation.results(),
        [P3CandidateResult::Rejected {
            key: P2CandidateKey::Npc { account, npc_local_index: 0 },
            reason: RejectionReason::InvalidQuantity,
            ..
        }] if *account == npc
    ));
    assert_eq!(
        committed
            .commit
            .tick
            .events
            .iter()
            .filter(|event| matches!(
                event,
                Event::IntentRejected {
                    account,
                    reason: RejectionReason::InvalidQuantity,
                    ..
                } if *account == npc
            ))
            .count(),
        1
    );
    assert_eq!(session.next_order_id, before_order_id);
}

#[test]
fn b1_downstream_failure_discards_npc_decision_and_authority_state() {
    let (authority, npc) = due_retail(8, 100);
    let authority_before = authority.session_state_hash().unwrap();
    let mut decision_probe = authority.clone_for_tick_shadow().unwrap();
    let observation =
        super::decision_snapshot_capture::capture_decision_snapshot(&mut decision_probe).unwrap();
    assert_eq!(observation.snapshot.due_npc_ids(), &[npc]);
    assert_ne!(
        decision_probe.session_state_hash().unwrap(),
        authority_before
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
    let result =
        super::b1_continuous_transaction::apply_tick_shadow_b1_continuous_transaction(&mut plan);
    assert!(matches!(
        result,
        Err(super::b1_continuous_transaction::B1ContinuousTransactionError::Finalization(
            StepFatal::InvariantViolation { location, description }
        )) if location == "pipeline::b1_tick_finalizer"
            && description == "session and ledger receipt cursors disagree"
    ));
    assert_eq!(authority.session_state_hash().unwrap(), authority_before);
    assert!(plan.state.execute(|_| Ok(())).is_err());
}

#[test]
fn b1_p2_failure_discards_the_whole_tick_candidate() {
    let (mut authority, npc) = due_retail(8, 100);
    authority.accounts.get_mut(&npc).unwrap().strategy = None;
    let before = authority.business_state_hash().unwrap();

    let error = super::b1_continuous_transaction::prepare_b1_continuous_tick(&mut authority)
        .err()
        .expect("missing NPC strategy must abort preparation");

    assert!(matches!(
        error,
        super::b1_continuous_transaction::B1ContinuousTransactionError::Preparation(
            StepFatal::InvariantViolation { description, location }
        ) if location == "pipeline::npc_p2_preparation" && description.contains("strategy")
    ));
    assert_eq!(authority.business_state_hash().unwrap(), before);
}

#[test]
fn real_npc_working_quotes_cancel_before_one_replacement_with_contiguous_keys() {
    let (mut session, npc) = due_retail(41, 100);
    session.accounts.get_mut(&npc).unwrap().cash = Money::from_cents(10_000_000);
    // Seed two pre-existing quotes without reconciling the first one.
    session.accounts.get_mut(&npc).unwrap().kind = crate::AccountKind::Player;
    let code = session.setup.stocks[0].code.clone();
    for price in [900, 901] {
        session.seed_order_for_test(
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(price),
                qty: 1_000,
            },
            &mut Vec::new(),
        );
    }
    session.accounts.get_mut(&npc).unwrap().kind = crate::AccountKind::Retail;
    let mut old_ids: Vec<_> = session.markets[&code]
        .resting_orders_for(npc)
        .iter()
        .map(|order| order.id)
        .collect();
    old_ids.sort();
    assert_eq!(old_ids.len(), 2);
    let resources = plan_tick(PhaseInput { session: &session })
        .unwrap()
        .decision_resources()
        .unwrap()
        .clone();
    let mut queued_session = session.clone_for_tick_shadow().unwrap();
    queued_session.pending_npc = None;
    let prepared = prepare_npc_p2_source(&mut session, None).unwrap();
    assert!(prepared.projection.reconciliation_decisions().iter().all(|decision| matches!(
        decision,
        super::npc_p2_projection::NpcReconciliationDecision::Replace { account, .. } if *account == npc
    )));
    let candidates = prepared.candidates.candidates();
    assert_eq!(
        candidates.len(),
        3,
        "two old quotes share one residual replacement"
    );
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        (0..3)
            .map(|index| P2CandidateKey::npc(npc, index))
            .collect::<Vec<_>>()
    );
    for (candidate, old_id) in candidates.iter().zip(&old_ids) {
        assert!(matches!(candidate.intent(), Intent::Cancel { id, .. } if id == old_id));
    }
    assert!(matches!(
        candidates[2].intent(),
        Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }
    ));
    super::queue_npc_for_next_tick(&mut queued_session).unwrap();
    let expected = candidates
        .iter()
        .map(|candidate| (candidate.owner(), candidate.intent().clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(&queued_session.pending_npc.as_ref().unwrap().intents).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let encoded = serde_json::to_value(queued_session.save().unwrap()).unwrap();
    assert_eq!(
        encoded["pending_npc"]["dependencies"],
        serde_json::json!([[0, 2], [1, 2]]),
        "both reconciled cancellations must precede their one replacement after save"
    );
    let mut restored = GameSession::restore(&serde_json::from_value(encoded).unwrap()).unwrap();
    let (restored_candidates, _) =
        super::npc_p2_preparation::take_ready_npc_batch(&mut restored).unwrap();
    assert_eq!(
        serde_json::to_value(&restored_candidates.candidates()[2]).unwrap()["predecessors"],
        serde_json::to_value([P2CandidateKey::npc(npc, 0), P2CandidateKey::npc(npc, 1),]).unwrap(),
        "restored replacement must retain both real predecessor identities"
    );

    let mut p3 = P3ValidatorDriver::new(
        resources,
        session.envelope_ledger.clone(),
        session.next_order_id,
        session.setup.config.clone(),
        super::p3_context::build_p3_validation_context(&session).unwrap(),
    )
    .unwrap();
    let mut p4 = super::p4_continuous::IncrementalContinuousStockCoordinator::from_post_p0(
        super::p4_continuous_adapter::prepare_incremental_continuous_inputs(&session).unwrap(),
    )
    .unwrap();
    let rounds = super::b1_continuous_transaction::apply_initial_candidate_stream_for_test(
        &mut p3,
        &mut p4,
        &prepared.candidates,
    )
    .unwrap();
    assert_eq!(
        rounds.iter().map(|round| round.facts.len()).sum::<usize>(),
        3
    );
    let final_book = rounds.last().unwrap().projections[&code]
        .market
        .as_ref()
        .unwrap();
    assert!(final_book
        .resting_orders()
        .iter()
        .all(|order| !old_ids.contains(&order.id)));
    assert_eq!(final_book.resting_orders_for(npc).len(), 1);
}

#[test]
fn npc_replacement_keeps_the_requested_place_for_next_tick_validation() {
    let (mut session, npc) = due_retail(41, 100);
    let code = session.setup.stocks[0].code.clone();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 1_000,
        },
        &mut Vec::new(),
    );
    session.accounts.get_mut(&npc).unwrap().cash = session.reserved_cash_for_account(npc).unwrap();
    let prepared = prepare_npc_p2_source(&mut session, None).unwrap();
    assert_eq!(prepared.candidates.candidates().len(), 2);
    assert!(matches!(
        prepared.candidates.candidates()[0].intent(),
        Intent::Cancel { .. }
    ));
    assert_eq!(
        prepared.candidates.candidates()[0].key(),
        &P2CandidateKey::npc(npc, 0)
    );
    assert!(matches!(
        prepared.candidates.candidates()[1].intent(),
        Intent::PlaceLimit { qty: 100, .. }
    ));
    assert_eq!(prepared.projection.residual_intents().len(), 1);
}

#[test]
fn real_npc_review_cancels_working_quote_in_b1_and_cleans_lifecycle() {
    let (mut session, npc) = due_retail(8, 100);
    let code = session.setup.stocks[0].code.clone();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let old_id = session.npc_order_lifecycles[0].order_id;
    session
        .accounts
        .get_mut(&npc)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5, 1).unwrap()));
    session.pending_npc = None;
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let result = super::b1_continuous_transaction::prepare_b1_continuous_tick(&mut session)
        .unwrap()
        .commit();
    assert_eq!(result.output.candidates.candidates().len(), 1);
    assert!(
        matches!(result.output.candidates.candidates()[0].intent(), Intent::Cancel { id, .. } if *id == old_id)
    );
    assert!(result.output.events.iter().any(|event| matches!(event, Event::OrderCanceled { id, account, .. } if *id == old_id && *account == npc)));
    assert!(session.npc_order_lifecycles.is_empty());
    assert!(session.markets[&code].resting_orders_for(npc).is_empty());
}

#[test]
fn npc_reconciliation_local_indexes_restart_per_account_in_canonical_account_order() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 8).unwrap();
    let code = session.setup.stocks[0].code.clone();
    for npc in [AccountId(2), AccountId(1)] {
        session
            .accounts
            .get_mut(&npc)
            .unwrap()
            .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5, 1).unwrap()));
        session.seed_order_for_test(
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
            &mut Vec::new(),
        );
        crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, npc, 0);
    }
    let prepared = prepare_npc_p2_source(&mut session, None).unwrap();
    assert_eq!(
        prepared
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            P2CandidateKey::npc(AccountId(1), 0),
            P2CandidateKey::npc(AccountId(2), 0)
        ]
    );
    assert!(prepared
        .projection
        .reconciliation_decisions()
        .iter()
        .all(|decision| matches!(
            decision,
            super::npc_p2_projection::NpcReconciliationDecision::Cancel { .. }
        )));
}
