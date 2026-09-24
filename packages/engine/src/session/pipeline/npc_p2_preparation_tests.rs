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
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(
            ZiNoiseStrategy::new(1.0, order_size, 0.5, 1).unwrap(),
        ));
    let tick = session.tick;
    crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, account, tick);
    (session, account)
}

fn sealed_resources(session: &GameSession) -> DecisionResourceSnapshot {
    plan_tick(PhaseInput { session })
        .unwrap()
        .decision_resources()
        .unwrap()
        .clone()
}

#[test]
fn npc_preparation_captures_one_snapshot_and_returns_it_for_plan_roots() {
    let (authority, npc) = due_retail(8, 100);
    let resources = sealed_resources(&authority);
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_p2_source(&mut prospective, &resources).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[npc]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[npc]);
    assert!(!prepared.candidates.candidates().is_empty());
}

#[test]
fn b1_npc_p3_quantity_rejection_reaches_one_final_event() {
    let (mut session, npc) = due_retail(8, 2_000_000);
    session.accounts.get_mut(&npc).unwrap().cash = Money::from_cents(10_000_000_000);
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
    assert_eq!(observation.due_npc_ids(), &[npc]);
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
fn real_npc_working_quotes_cancel_before_one_replacement_with_contiguous_keys() {
    let (mut session, npc) = due_retail(41, 100);
    session.accounts.get_mut(&npc).unwrap().cash = Money::from_cents(10_000_000);
    // Seed two pre-existing quotes without reconciling the first one.
    session.accounts.get_mut(&npc).unwrap().kind = crate::AccountKind::Player;
    let code = session.setup.stocks[0].code.clone();
    for price in [900, 901] {
        session.route_intent(
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
    let resources = sealed_resources(&session);
    let prepared = prepare_npc_p2_source(&mut session, &resources).unwrap();
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
    let final_book = &rounds.last().unwrap().projections[&code].market;
    assert!(final_book
        .resting_orders()
        .iter()
        .all(|order| !old_ids.contains(&order.id)));
    assert_eq!(final_book.resting_orders_for(npc).len(), 1);
}

#[test]
fn npc_replacement_does_not_reuse_the_canceled_quotes_sealed_cash() {
    let (mut session, npc) = due_retail(41, 100);
    let code = session.setup.stocks[0].code.clone();
    session.route_intent(
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
    let resources = sealed_resources(&session);
    let prepared = prepare_npc_p2_source(&mut session, &resources).unwrap();
    assert_eq!(prepared.candidates.candidates().len(), 1);
    assert!(matches!(
        prepared.candidates.candidates()[0].intent(),
        Intent::Cancel { .. }
    ));
    assert_eq!(
        prepared.candidates.candidates()[0].key(),
        &P2CandidateKey::npc(npc, 0)
    );
    assert!(prepared.projection.residual_intents().is_empty());
}

#[test]
fn real_npc_review_cancels_working_quote_in_b1_and_cleans_lifecycle() {
    let (mut session, npc) = due_retail(8, 100);
    let code = session.setup.stocks[0].code.clone();
    session.route_intent(
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
        session.route_intent(
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
    let resources = sealed_resources(&session);
    let prepared = prepare_npc_p2_source(&mut session, &resources).unwrap();
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
