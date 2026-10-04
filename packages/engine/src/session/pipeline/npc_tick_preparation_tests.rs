use super::npc_tick_preparation::prepare_npc_decisions;
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
        ZiNoiseStrategy::new(1.0, order_size, 0.5).unwrap(),
    );
    strategy
        .set_base_observation_probability(session.state.npc_attention[&account].base_probability);
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(strategy.into_strategy().unwrap());
    let tick = session.state.tick;
    // Preserve the real profile so this fixture can also be saved and restored.
    // Choose an attention-stream position that accepts even a quiet market.
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.next_attention_candidate_tick = tick;
    attention.rng_state = 3;
    let mut quiet_probe = attention.clone();
    assert!(quiet_probe.evaluate_candidate_with_signal(crate::AccountKind::Retail, 0.0, tick));
    session.state.attention_scheduler.enqueue(tick, account);
    (session, account)
}

#[test]
fn no_due_npc_queues_an_empty_batch_without_building_a_market_view() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    let future_tick = session.state.tick + 10;
    session.state.pending_npc = None;
    session.state.attention_scheduler.clear();
    let accounts = session
        .state
        .npc_attention
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for account in accounts {
        session
            .state
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = future_tick;
        session
            .state
            .attention_scheduler
            .enqueue(future_tick, account);
    }
    let code = session.state.markets.keys().next().unwrap().clone();
    session.state.market_minute_closes.remove(&code);

    super::queue_npc_for_next_tick(&mut session).unwrap();
    let ready = session.state.pending_npc.as_ref().unwrap();
    assert_eq!(ready.observed_tick, session.state.tick);
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
        price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
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
    let code = session.state.setup.stocks[0].code.clone();
    let mut save = session.save().unwrap();
    save.pending_npc = Some(crate::session::PendingNpcBatch {
        observed_tick: session.state.tick,
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
                    price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
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
    session.state.pending_npc = save.pending_npc;
    assert!(matches!(
        super::npc_tick_preparation::take_ready_npc_batch(&mut session),
        Err(StepFatal::InvariantViolation { description, .. })
            if description.contains("pending NPC dependency")
    ));
}

#[test]
fn npc_preparation_captures_one_snapshot_and_returns_it_for_plan_roots() {
    let (mut authority, npc) = due_retail(8, 100);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut authority, npc);
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_decisions(&mut prospective, None).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[npc]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[npc]);
    assert!(!prepared.candidates.candidates().is_empty());
}

#[test]
fn next_tick_account_validation_rechecks_npc_orders_against_current_cash() {
    let (mut session, npc) = due_retail(8, 100);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut session, npc);

    assert!(session
        .state
        .pending_npc
        .as_ref()
        .unwrap()
        .intents
        .is_empty());
    session.state.pending_npc = None;
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let pending = session.state.pending_npc.as_ref().unwrap();
    assert!(pending.intents.iter().any(|(account, intent)| {
        *account == npc
            && matches!(
                intent,
                Intent::PlaceLimit {
                    side: Side::Buy,
                    ..
                }
            )
    }));
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_cash(Money::ZERO);

    let events = session.step().unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account,
            reason: RejectionReason::InsufficientCash,
            ..
        } if *account == npc
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::OrderAccepted { account, .. } if *account == npc
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::Trade { maker, taker, .. } if *maker == npc || *taker == npc
    )));
    assert_eq!(session.state.accounts[&npc].cash(), Money::ZERO);
    assert!(session
        .state
        .markets
        .values()
        .all(|market| { market.resting_orders_for(npc).is_empty() }));
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

    let prepared = prepare_npc_decisions(&mut prospective, None).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[institution]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[institution]);
    assert!(!prepared.roots.is_empty());

    let mut injected = authority.clone_for_tick_shadow().unwrap();
    let prepared = prepare_npc_decisions(
        &mut injected,
        Some(crate::session::plan_chain_candidates::PlanChainOperationBatch::empty()),
    )
    .unwrap();
    assert!(prepared.roots.is_empty());
}

#[test]
fn npc_account_validation_quantity_rejection_reaches_one_final_event() {
    let (mut session, npc) = due_retail(8, 2_000_000);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut session, npc);
    // 7% 的试买目标足以提出 200 万股，由下一 tick 的 AccountValidation 执行单笔数量规则。
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_cash(Money::from_cents(30_000_000_000));
    session.state.pending_npc = None;
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let before_order_id = session.state.next_order_id;

    let committed = super::continuous_tick_transaction::prepare_continuous_tick(&mut session)
        .unwrap()
        .commit();
    assert!(matches!(
        committed.output.validation.results(),
        [CandidateValidationResult::Rejected {
            key: IntentCandidateKey::Npc { account, npc_local_index: 0 },
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
    assert_eq!(session.state.next_order_id, before_order_id);
}

#[test]
fn downstream_failure_discards_npc_decision_and_authority_state() {
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
            candidate.state.next_receipt_base = 1;
            Ok(())
        })
        .unwrap();
    let result =
        super::continuous_tick_transaction::apply_tick_shadow_continuous_transaction(&mut plan);
    assert!(matches!(
        result,
        Err(super::continuous_tick_transaction::ContinuousTransactionError::Finalization(
            StepFatal::InvariantViolation { location, description }
        )) if location == "pipeline::continuous_tick_finalizer"
            && description == "session and ledger receipt cursors disagree"
    ));
    assert_eq!(authority.session_state_hash().unwrap(), authority_before);
    assert!(plan.state.execute(|_| Ok(())).is_err());
}

#[test]
fn intent_candidates_failure_discards_the_whole_tick_candidate() {
    let (mut authority, npc) = due_retail(8, 100);
    authority
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_strategy(None);
    let before = authority.business_state_hash().unwrap();

    let error = super::continuous_tick_transaction::prepare_continuous_tick(&mut authority)
        .err()
        .expect("missing NPC strategy must abort preparation");

    assert!(matches!(
        error,
        super::continuous_tick_transaction::ContinuousTransactionError::Preparation(
            StepFatal::InvariantViolation { description, location }
        ) if location == "pipeline::npc_tick_preparation" && description.contains("strategy")
    ));
    assert_eq!(authority.business_state_hash().unwrap(), before);
}

#[test]
fn real_npc_working_quotes_cancel_before_one_replacement_with_contiguous_keys() {
    let (mut session, npc) = due_retail(41, 100);
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_cash(Money::from_cents(10_000_000));
    // Seed two pre-existing quotes without reconciling the first one.
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_kind(crate::AccountKind::Player);
    let code = session.state.setup.stocks[0].code.clone();
    for price in [900, 901] {
        session.seed_order_for_test(
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(price)),
                qty: 1_000,
            },
            &mut Vec::new(),
        );
    }
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_kind(crate::AccountKind::Retail);
    let mut old_ids: Vec<_> = session.state.markets[&code]
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
    queued_session.state.pending_npc = None;
    let prepared = prepare_npc_decisions(&mut session, None).unwrap();
    assert!(prepared.projection.reconciliation_decisions().iter().all(|decision| matches!(
        decision,
        super::npc_state_projection::NpcReconciliationDecision::Replace { account, .. } if *account == npc
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
            .map(|index| IntentCandidateKey::npc(npc, index))
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
        serde_json::to_value(&queued_session.state.pending_npc.as_ref().unwrap().intents).unwrap(),
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
        super::npc_tick_preparation::take_ready_npc_batch(&mut restored).unwrap();
    assert_eq!(
        serde_json::to_value(&restored_candidates.candidates()[2]).unwrap()["predecessors"],
        serde_json::to_value([
            IntentCandidateKey::npc(npc, 0),
            IntentCandidateKey::npc(npc, 1),
        ])
        .unwrap(),
        "restored replacement must retain both real predecessor identities"
    );

    let mut validator = AccountValidatorDriver::new(
        resources,
        session.state.envelope_ledger.clone(),
        session.state.next_order_id,
        session.state.setup.config.clone(),
        super::account_validation_context::build_account_validation_context(&session).unwrap(),
    )
    .unwrap();
    let mut stock_execution =
        super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(
            super::continuous_matching_adapter::prepare_incremental_continuous_inputs(&session)
                .unwrap(),
        )
        .unwrap();
    let rounds = super::continuous_tick_transaction::apply_initial_candidate_stream_for_test(
        &mut validator,
        &mut stock_execution,
        &prepared.candidates,
    )
    .unwrap();
    assert_eq!(
        rounds.iter().map(|round| round.facts.len()).sum::<usize>(),
        3
    );
    let mut final_book = session.state.markets[&code].clone();
    for round in &rounds {
        if let Some(projection) = round.projections.get(&code) {
            final_book
                .apply_changed_orders(projection.market_delta.as_ref().unwrap().clone())
                .unwrap();
        }
    }
    assert!(final_book
        .resting_orders()
        .iter()
        .all(|order| !old_ids.contains(&order.id)));
    assert_eq!(final_book.resting_orders_for(npc).len(), 1);
}

#[test]
fn npc_replacement_keeps_the_requested_place_for_next_tick_validation() {
    let (mut session, npc) = due_retail(41, 100);
    let code = session.state.setup.stocks[0].code.clone();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(900)),
            // 仍将全部资金冻结；明确预算使当前策略的试买目标足够重新报一手。
            qty: 2_000,
        },
        &mut Vec::new(),
    );
    let reserved = session.reserved_cash_for_account(npc).unwrap();
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .fixture_set_cash(reserved);
    let prepared = prepare_npc_decisions(&mut session, None).unwrap();
    assert_eq!(prepared.candidates.candidates().len(), 2);
    assert!(matches!(
        prepared.candidates.candidates()[0].intent(),
        Intent::Cancel { .. }
    ));
    assert_eq!(
        prepared.candidates.candidates()[0].key(),
        &IntentCandidateKey::npc(npc, 0)
    );
    assert!(matches!(
        prepared.candidates.candidates()[1].intent(),
        Intent::PlaceLimit { qty: 100, .. }
    ));
    assert_eq!(prepared.projection.residual_intents().len(), 1);
}

#[test]
fn real_npc_review_cancels_working_quote_in_and_cleans_lifecycle() {
    let (mut session, npc) = due_retail(8, 100);
    let code = session.state.setup.stocks[0].code.clone();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let old_id = session.state.npc_order_lifecycles[0].order_id;
    session
        .state
        .accounts
        .get_mut(&npc)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5).unwrap()));
    session.state.pending_npc = None;
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let result = super::continuous_tick_transaction::prepare_continuous_tick(&mut session)
        .unwrap()
        .commit();
    assert_eq!(result.output.candidates.candidates().len(), 1);
    assert!(
        matches!(result.output.candidates.candidates()[0].intent(), Intent::Cancel { id, .. } if *id == old_id)
    );
    assert!(result.output.events.iter().any(|event| matches!(event, Event::OrderCanceled { id, account, .. } if *id == old_id && *account == npc)));
    assert!(session.state.npc_order_lifecycles.is_empty());
    assert!(session.state.markets[&code]
        .resting_orders_for(npc)
        .is_empty());
}

#[test]
fn npc_reconciliation_local_indexes_restart_per_account_in_canonical_account_order() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 8).unwrap();
    let code = session.state.setup.stocks[0].code.clone();
    for npc in [AccountId(2), AccountId(1)] {
        session
            .state
            .accounts
            .get_mut(&npc)
            .unwrap()
            .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5).unwrap()));
        session.seed_order_for_test(
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
            &mut Vec::new(),
        );
        crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, npc, 0);
    }
    let prepared = prepare_npc_decisions(&mut session, None).unwrap();
    assert_eq!(
        prepared
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            IntentCandidateKey::npc(AccountId(1), 0),
            IntentCandidateKey::npc(AccountId(2), 0)
        ]
    );
    assert!(prepared
        .projection
        .reconciliation_decisions()
        .iter()
        .all(|decision| matches!(
            decision,
            super::npc_state_projection::NpcReconciliationDecision::Cancel { .. }
        )));
}
