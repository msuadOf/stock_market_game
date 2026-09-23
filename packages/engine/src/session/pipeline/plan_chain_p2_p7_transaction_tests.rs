use super::plan_chain_p2_p7_transaction::{
    apply_plan_chain_p2_p7_transaction, PlanChainP2P7TransactionError,
};
use super::*;
use crate::plans::PlanBook;
use crate::session::{
    npc_generation::NpcDecisionBatch, plan_chain_candidates::PlanChainYieldDriver,
    plan_execution::PlanExecutionDisposition,
};
use crate::{
    AccountId, AccountKind, Event, Intent, Money, OrderId, RejectionReason, Side, StockCode,
};

fn continuation_fixture() -> (
    GameSession,
    PlanBook,
    PlanChainYieldDriver,
    StockCode,
    Vec<OrderId>,
) {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let owner = AccountId(1);
    let code = request.allocation.code.clone();
    session.accounts.get_mut(&owner).unwrap().kind = AccountKind::Player;
    let mut setup_events = Vec::new();
    for price in [901, 902] {
        session.route_intent(
            owner,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(price),
                qty: 100,
            },
            &mut setup_events,
        );
    }
    session.hydrate_or_validate_envelope_ledger().unwrap();
    let order_ids = setup_events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(order_ids.len(), 2);
    let mut plans = std::mem::take(&mut session.plans);
    let progress = session
        .prepare_plan_observation(&mut plans, request)
        .unwrap();
    (
        session,
        plans,
        PlanChainYieldDriver::new(progress),
        code,
        order_ids,
    )
}

fn resources(session: &GameSession) -> DecisionResourceSnapshot {
    plan_tick(PhaseInput { session })
        .unwrap()
        .decision_resources()
        .unwrap()
        .clone()
}

fn submit_continuation_fixture() -> (GameSession, PlanBook, PlanChainYieldDriver, StockCode) {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    let code = request.allocation.code.clone();
    session.accounts.get_mut(&AccountId(1)).unwrap().kind = AccountKind::Player;
    let mut plans = std::mem::take(&mut session.plans);
    let progress = session
        .prepare_plan_observation(&mut plans, request)
        .unwrap();
    (session, plans, PlanChainYieldDriver::new(progress), code)
}

fn empty_npc() -> NpcDecisionBatch {
    NpcDecisionBatch {
        accepted_due_npc_ids: Vec::new(),
        intents: Vec::new(),
    }
}

#[test]
fn real_plan_cancel_reaches_p7_and_only_advances_owned_prospective_state() {
    let (session, plans, mut driver, code, order_ids) = continuation_fixture();
    let plans_before = serde_json::to_vec(&plans).unwrap();
    let queue_before = serde_json::to_vec(&session.pending_player).unwrap();
    let cursors_before = (
        session.next_order_id,
        session.next_receipt_base,
        session.envelope_ledger.next_receipt_index(),
        session.seq,
    );

    let mut output = apply_plan_chain_p2_p7_transaction(
        &session,
        &plans,
        empty_npc(),
        resources(&session),
        &driver,
    )
    .unwrap();

    assert_eq!(serde_json::to_vec(&plans).unwrap(), plans_before);
    assert_eq!(
        serde_json::to_vec(&session.pending_player).unwrap(),
        queue_before
    );
    assert_eq!(
        (
            session.next_order_id,
            session.next_receipt_base,
            session.envelope_ledger.next_receipt_index(),
            session.seq,
        ),
        cursors_before
    );
    assert!(output.events.iter().any(|event| matches!(
        event,
        Event::OrderCanceled { account, code: actual, id, .. }
            if *account == AccountId(1) && actual == &code && *id == order_ids[0]
    )));
    assert_eq!(
        output
            .validation
            .results()
            .iter()
            .map(|result| (result.key().clone(), result.sealed_index()))
            .collect::<Vec<_>>(),
        vec![(P2CandidateKey::plan_chain(0), 0)]
    );
    let next = output.driver.yield_next().unwrap();
    assert_eq!(next.chain_generation_index, 1);
    assert_eq!(driver.yield_next().unwrap().chain_generation_index, 0);
}

#[test]
fn npc_player_plan_chain_classes_keep_their_sealed_order_and_generation_identity() {
    let (mut session, plans, driver, code, _) = continuation_fixture();
    let other_code = session
        .markets
        .keys()
        .find(|candidate| *candidate != &code)
        .cloned()
        .unwrap_or_else(|| code.clone());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: other_code.clone(),
                id: OrderId(9_999_998),
            },
        )
        .unwrap();
    let npc = NpcDecisionBatch {
        accepted_due_npc_ids: vec![AccountId(0)],
        intents: vec![(
            AccountId(0),
            Intent::Cancel {
                code: other_code,
                id: OrderId(9_999_999),
            },
        )],
    };

    let output =
        apply_plan_chain_p2_p7_transaction(&session, &plans, npc, resources(&session), &driver)
            .unwrap();

    assert_eq!(
        output
            .validation
            .results()
            .iter()
            .map(|result| (result.key().clone(), result.sealed_index()))
            .collect::<Vec<_>>(),
        vec![
            (P2CandidateKey::npc(AccountId(0), 0), 0),
            (P2CandidateKey::player(0), 1),
            (P2CandidateKey::plan_chain(0), 2),
        ]
    );
    assert_eq!(session.pending_player.len(), 1);
}

#[test]
fn downstream_failure_preserves_authority_queue_cursors_ledger_seen_and_sequence() {
    let (mut session, plans, mut driver, _, _) = continuation_fixture();
    let code = session.markets.keys().next().unwrap().clone();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code,
                id: OrderId(9_999_999),
            },
        )
        .unwrap();
    let decision_resources = resources(&session);
    session.next_receipt_base = session.next_receipt_base.checked_add(1).unwrap();
    let queue_before = serde_json::to_vec(&session.pending_player).unwrap();
    let ledger_before = session.envelope_ledger.clone();
    let plans_before = serde_json::to_vec(&plans).unwrap();
    let cursors_before = (
        session.next_order_id,
        session.next_receipt_base,
        session.seq,
    );

    let result = apply_plan_chain_p2_p7_transaction(
        &session,
        &plans,
        empty_npc(),
        decision_resources,
        &driver,
    );

    assert!(matches!(
        result,
        Err(PlanChainP2P7TransactionError::P3P7(
            super::p3_p7_session_transaction::P3P7SessionTransactionError::Transaction(
                super::p4_p7_session_transaction::P4P7SessionTransactionError::Precondition(_)
            )
        ))
    ));
    assert_eq!(
        serde_json::to_vec(&session.pending_player).unwrap(),
        queue_before
    );
    assert_eq!(session.envelope_ledger, ledger_before);
    assert_eq!(serde_json::to_vec(&plans).unwrap(), plans_before);
    assert_eq!(
        (
            session.next_order_id,
            session.next_receipt_base,
            session.seq
        ),
        cursors_before
    );
    assert_eq!(driver.yield_next().unwrap().chain_generation_index, 0);
}

#[test]
fn p3_plan_rejection_survives_final_event_collection_and_resumes_the_fork_only() {
    let (mut session, plans, mut driver, code) = submit_continuation_fixture();
    session.accounts.get_mut(&AccountId(1)).unwrap().cash = Money::ZERO;
    let decision_resources = resources(&session);

    let mut output = apply_plan_chain_p2_p7_transaction(
        &session,
        &plans,
        empty_npc(),
        decision_resources,
        &driver,
    )
    .unwrap();

    assert!(matches!(
        output.validation.results(),
        [P3CandidateResult::Rejected {
            key: P2CandidateKey::PlanChain {
                chain_generation_index: 0
            },
            sealed_index: 0,
            reason: RejectionReason::InsufficientCash,
        }]
    ));
    assert!(output.events.iter().any(|event| matches!(
        event,
        Event::IntentRejected { account, code: actual, reason, .. }
            if *account == AccountId(1)
                && actual == &code
                && *reason == RejectionReason::InsufficientCash
    )));
    assert!(matches!(
        output.driver.take_completion().unwrap().disposition,
        PlanExecutionDisposition::RouteRejected {
            reason: RejectionReason::InsufficientCash
        }
    ));
    assert_eq!(driver.yield_next().unwrap().chain_generation_index, 0);
}
