use super::npc_p2_p7_transaction::{
    apply_session_npc_p2_p7_transaction, prepare_npc_p2_source,
    prepare_npc_p2_source_from_snapshot, NpcP2P7TransactionError,
};
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
fn externally_captured_snapshot_is_consumed_without_a_second_attention_capture() {
    let (authority, npc) = due_retail(8, 100);
    let resources = sealed_resources(&authority);
    let mut prospective = authority.clone_for_tick_shadow().unwrap();
    let snapshot =
        super::decision_snapshot_capture::capture_decision_snapshot(&mut prospective).unwrap();

    let prepared =
        prepare_npc_p2_source_from_snapshot(&mut prospective, &resources, snapshot).unwrap();

    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[npc]);
    assert!(!prepared.candidates.candidates().is_empty());
}

#[test]
fn captured_snapshot_entry_is_equivalent_to_the_capture_and_prepare_entry() {
    let (authority, _) = due_retail(8, 100);
    let resources = sealed_resources(&authority);
    let mut legacy = authority.clone_for_tick_shadow().unwrap();
    let mut shared_snapshot = authority.clone_for_tick_shadow().unwrap();

    let legacy_prepared = prepare_npc_p2_source(&mut legacy, &resources).unwrap();
    let snapshot =
        super::decision_snapshot_capture::capture_decision_snapshot(&mut shared_snapshot).unwrap();
    let shared_prepared =
        prepare_npc_p2_source_from_snapshot(&mut shared_snapshot, &resources, snapshot).unwrap();

    assert_eq!(
        shared_prepared.projection.accepted_due_npc_ids(),
        legacy_prepared.projection.accepted_due_npc_ids()
    );
    assert_eq!(shared_prepared.candidates, legacy_prepared.candidates);
    assert_eq!(
        shared_snapshot.session_state_hash().unwrap(),
        legacy.session_state_hash().unwrap()
    );
}

#[test]
fn snapshot_from_same_clock_but_different_strategy_source_is_rejected_before_projection() {
    let (authority, _) = due_retail(8, 100);
    let resources = sealed_resources(&authority);
    let mut prospective = authority.clone_for_tick_shadow().unwrap();
    let (foreign, _) = due_retail(8, 200);
    let mut foreign_prospective = foreign.clone_for_tick_shadow().unwrap();
    let foreign_snapshot =
        super::decision_snapshot_capture::capture_decision_snapshot(&mut foreign_prospective)
            .unwrap();
    let before = prospective.session_state_hash().unwrap();

    let result =
        prepare_npc_p2_source_from_snapshot(&mut prospective, &resources, foreign_snapshot);

    assert!(matches!(
        result,
        Err(NpcP2P7TransactionError::Preparation(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::npc_p2_p7_transaction::validate_snapshot_source_session"
    ));
    assert_eq!(prospective.session_state_hash().unwrap(), before);
}

#[test]
fn real_npc_source_runs_through_p7_without_consuming_the_player_class() {
    let (mut session, npc) = due_retail(8, 100);
    let player = AccountId(0);
    let code = session.setup.stocks[0].code.clone();
    session
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
        )
        .unwrap();
    let player_queue_before = serde_json::to_vec(&session.pending_player).unwrap();
    let resources = sealed_resources(&session);
    let resources_before = resources.clone();
    let next_order_id_before = session.next_order_id;

    let output = apply_session_npc_p2_p7_transaction(&mut session, &resources).unwrap();

    assert_eq!(
        resources, resources_before,
        "P1 snapshot must remain immutable"
    );
    assert_eq!(
        serde_json::to_vec(&session.pending_player).unwrap(),
        player_queue_before,
        "the NPC seam must not consume or reorder the later player class"
    );
    assert_eq!(output.projection.accepted_due_npc_ids(), &[npc]);
    assert!(!output.validation.results().is_empty());
    assert!(output.validation.results().iter().all(
        |result| matches!(result.key(), P2CandidateKey::Npc { account, .. } if account == &npc)
    ));
    assert_eq!(
        session.next_order_id,
        next_order_id_before + u64::try_from(output.validation.drafts().len()).unwrap()
    );
    assert_eq!(session.seq, u64::try_from(output.events.len()).unwrap());
    assert_eq!(output.events.len(), output.validation.results().len());
    assert!(output.receipts.is_empty());
    assert!(output.p6.events.is_empty());
}

#[test]
fn p3_rejection_is_merged_once_by_the_final_event_collector() {
    let (mut session, npc) = due_retail(8, 2_000_000);
    session.accounts.get_mut(&npc).unwrap().cash = Money::from_cents(10_000_000_000);
    let resources = sealed_resources(&session);
    let seq_before = session.seq;

    let output = apply_session_npc_p2_p7_transaction(&mut session, &resources).unwrap();

    assert_eq!(output.validation.rejected().count(), 1);
    assert_eq!(session.seq, seq_before + 1);
    assert!(matches!(
        output.events.as_slice(),
        [Event::IntentRejected {
            seq,
            account,
            reason: RejectionReason::InvalidQuantity,
            ..
        }] if *seq == seq_before + 1 && *account == npc
    ));
}

#[test]
fn mismatched_p1_snapshot_is_rejected_before_npc_state_can_leak() {
    let (mut session, npc) = due_retail(8, 100);
    let resources = sealed_resources(&session);
    let stale_cash = session.accounts[&npc]
        .cash
        .add(Money::from_cents(1))
        .unwrap();
    session.accounts.get_mut(&npc).unwrap().cash = stale_cash;
    let before = session.session_state_hash().unwrap();

    let result = apply_session_npc_p2_p7_transaction(&mut session, &resources);

    assert!(matches!(
        result,
        Err(NpcP2P7TransactionError::Preparation(
            StepFatal::InvariantViolation { location, .. }
        )) if location == "pipeline::decision_resources::validate_source_session"
    ));
    assert_eq!(session.session_state_hash().unwrap(), before);
}

#[test]
fn prepared_npc_source_is_discardable_when_the_unified_downstream_fails() {
    let (authority, _) = due_retail(8, 100);
    let resources = sealed_resources(&authority);
    let authority_before = authority.session_state_hash().unwrap();
    let next_order_id_before = authority.next_order_id;
    let ledger_before = authority.envelope_ledger.clone();
    let seq_before = authority.seq;
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_p2_source(&mut prospective, &resources).unwrap();

    assert!(!prepared.candidates.candidates().is_empty());
    assert_eq!(prospective.next_order_id, next_order_id_before);
    assert_eq!(prospective.envelope_ledger, ledger_before);
    assert_eq!(prospective.seq, seq_before);
    assert_ne!(prospective.session_state_hash().unwrap(), authority_before);

    let context = super::p3_context::build_p3_validation_context(&prospective).unwrap();
    let validation = P2P3Handoff::new_with_context(
        prepared.candidates.clone(),
        resources,
        prospective.envelope_ledger.clone(),
        prospective.next_order_id,
        prospective.setup.config.clone(),
        context,
    )
    .unwrap()
    .validate()
    .unwrap();
    prospective.next_receipt_base = 1;
    let result = super::p3_p7_session_transaction::apply_session_p3_p7_transaction(
        &mut prospective,
        &prepared.candidates,
        &validation,
    );

    assert!(result.is_err());
    drop(prospective);
    assert_eq!(authority.session_state_hash().unwrap(), authority_before);
}

#[test]
fn downstream_failure_discards_npc_strategy_rng_queue_cursor_ledger_seen_and_seq_changes() {
    let (mut session, _) = due_retail(8, 100);
    let resources = sealed_resources(&session);
    session.next_receipt_base = 1;
    let business_before = session.business_state_hash().unwrap();
    let session_before = session.session_state_hash().unwrap();
    let order_cursor_before = session.next_order_id;
    let receipt_cursor_before = session.next_receipt_base;
    let seq_before = session.seq;
    let ledger_before = session.envelope_ledger.clone();
    let queue_before = serde_json::to_vec(&session.pending_player).unwrap();

    let result = apply_session_npc_p2_p7_transaction(&mut session, &resources);

    assert!(matches!(
        result,
        Err(NpcP2P7TransactionError::P3P7(
            super::p3_p7_session_transaction::P3P7SessionTransactionError::Transaction(
                super::p4_p7_session_transaction::P4P7SessionTransactionError::Precondition(
                    StepFatal::InvariantViolation { location, .. }
                )
            )
        )) if location == "pipeline::p4_p7_session_transaction"
    ));
    assert_eq!(session.business_state_hash().unwrap(), business_before);
    assert_eq!(session.session_state_hash().unwrap(), session_before);
    assert_eq!(session.next_order_id, order_cursor_before);
    assert_eq!(session.next_receipt_base, receipt_cursor_before);
    assert_eq!(session.seq, seq_before);
    assert_eq!(session.envelope_ledger, ledger_before);
    assert_eq!(
        serde_json::to_vec(&session.pending_player).unwrap(),
        queue_before
    );
}

#[test]
fn real_npc_working_quotes_cancel_before_one_replacement_with_contiguous_keys() {
    let (mut session, npc) = due_retail(41, 100);
    session.accounts.get_mut(&npc).unwrap().cash = Money::from_cents(10_000_000);
    // Seed two pre-existing quotes without the legacy NPC router replacing the first.
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
