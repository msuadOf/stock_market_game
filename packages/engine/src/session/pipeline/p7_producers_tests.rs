use super::p3_p4_normalizer::normalize_p3_p4_operations;
use super::p7_producers::{adapt_p3_p4_cancel_rejections, adapt_p3_rejection_facts};
use super::*;
use crate::{AccountId, Event, Intent, Money, OrderId, RejectionReason, Side, StockCode};

fn code(value: &str) -> StockCode {
    StockCode(value.to_owned())
}

fn candidate(key: P2CandidateKey, account: u64, intent: Intent) -> P2Candidate {
    P2Candidate::new(key, AccountId(account), intent)
}

#[test]
fn p3_rejection_adapter_uses_candidate_payload_and_explicit_sealed_identity() {
    let alpha = code("600001");
    let beta = code("600002");
    let candidates = P2CandidateBatch::from_unsorted(vec![
        candidate(
            P2CandidateKey::npc(AccountId(4), 0),
            4,
            Intent::Cancel {
                code: alpha.clone(),
                id: OrderId(8),
            },
        ),
        candidate(
            P2CandidateKey::player(5),
            7,
            Intent::PlaceLimit {
                code: beta.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        ),
        candidate(
            P2CandidateKey::plan_chain(9),
            11,
            Intent::PlaceMarket {
                code: alpha.clone(),
                side: Side::Sell,
                qty: 200,
            },
        ),
    ])
    .unwrap();
    let results = vec![
        P3CandidateResult::Accepted {
            key: P2CandidateKey::npc(AccountId(4), 0),
            sealed_index: 0,
        },
        P3CandidateResult::Rejected {
            key: P2CandidateKey::player(5),
            sealed_index: 1,
            reason: RejectionReason::InsufficientCash,
        },
        P3CandidateResult::Rejected {
            key: P2CandidateKey::plan_chain(9),
            sealed_index: 2,
            reason: RejectionReason::InsufficientShares,
        },
    ];

    let facts = adapt_p3_rejection_facts(&candidates, &results).unwrap();

    assert_eq!(facts.len(), 2);
    assert_eq!(
        facts[0].event,
        Event::IntentRejected {
            seq: 0,
            account: AccountId(7),
            code: beta,
            reason: RejectionReason::InsufficientCash,
        }
    );
    assert_eq!(facts[0].key, EventStableKey::for_event(&facts[0].event, 1));
    assert_eq!(
        facts[1].event,
        Event::IntentRejected {
            seq: 0,
            account: AccountId(11),
            code: alpha,
            reason: RejectionReason::InsufficientShares,
        }
    );
    assert_eq!(facts[1].key, EventStableKey::for_event(&facts[1].event, 2));
}

#[test]
fn p3_pending_plan_event_limits_collapse_to_one_phase_six_session_fact() {
    let alpha = code("600001");
    let candidates = P2CandidateBatch::from_canonical(vec![
        candidate(
            P2CandidateKey::player(0),
            7,
            Intent::PlaceLimit {
                code: alpha.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        ),
        candidate(
            P2CandidateKey::player(1),
            7,
            Intent::PlaceLimit {
                code: alpha,
                side: Side::Buy,
                price: Money::from_cents(1_001),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let results = vec![
        P3CandidateResult::PendingPlanEventsLimited {
            key: P2CandidateKey::player(0),
            sealed_index: 0,
        },
        P3CandidateResult::PendingPlanEventsLimited {
            key: P2CandidateKey::player(1),
            sealed_index: 1,
        },
    ];
    let mut session_cursor = 4;

    let facts = super::p7_producers::adapt_p3_rejection_facts_after(
        &candidates,
        &results,
        &mut session_cursor,
    )
    .unwrap();

    assert_eq!(facts.len(), 1);
    assert!(matches!(
        facts[0].event,
        Event::ResourceLimit {
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit,
            ..
        } if limit == crate::session::MAX_SAVED_PLAN_EVENTS as u32
    ));
    assert_eq!(facts[0].key.local_event_index(), 4);
    assert_eq!(session_cursor, 5);
}

#[test]
fn day_end_pending_plan_limit_reuses_the_existing_tick_wide_fact_and_cursor() {
    let event = Event::ResourceLimit {
        seq: 0,
        resource: crate::session::RuntimeResource::PendingPlanEvents,
        limit: crate::session::MAX_SAVED_PLAN_EVENTS as u32,
    };
    let mut facts = vec![super::p7_events::OwnedEventFact {
        key: EventStableKey::for_event(&event, 3),
        event,
    }];
    let mut session_cursor = 4;

    super::p7_producers::push_pending_plan_events_resource_limit_fact_after(
        &mut facts,
        &mut session_cursor,
    )
    .unwrap();

    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].key.local_event_index(), 3);
    assert_eq!(session_cursor, 4);
}

#[test]
fn p3_rejection_adapter_rejects_duplicate_or_missing_candidate_contracts_without_output() {
    let alpha = code("600001");
    let candidates = P2CandidateBatch::from_unsorted(vec![candidate(
        P2CandidateKey::player(0),
        7,
        Intent::Cancel {
            code: alpha.clone(),
            id: OrderId(8),
        },
    )])
    .unwrap();
    let duplicate = vec![
        P3CandidateResult::Rejected {
            key: P2CandidateKey::player(0),
            sealed_index: 0,
            reason: RejectionReason::OrderNotFound,
        },
        P3CandidateResult::Rejected {
            key: P2CandidateKey::player(0),
            sealed_index: 1,
            reason: RejectionReason::OrderNotFound,
        },
    ];
    assert!(matches!(
        adapt_p3_rejection_facts(&candidates, &duplicate),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_producers"
    ));

    let missing = Vec::new();
    assert!(matches!(
        adapt_p3_rejection_facts(&candidates, &missing),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p7_producers"
    ));
}

#[test]
fn p3_rejection_adapter_rejects_a_candidate_key_and_sealed_identity_swap() {
    let alpha = code("600001");
    let candidates = P2CandidateBatch::from_unsorted(vec![
        candidate(
            P2CandidateKey::player(0),
            7,
            Intent::Cancel {
                code: alpha.clone(),
                id: OrderId(8),
            },
        ),
        candidate(
            P2CandidateKey::player(1),
            9,
            Intent::Cancel {
                code: alpha,
                id: OrderId(9),
            },
        ),
    ])
    .unwrap();
    let swapped = vec![
        P3CandidateResult::Accepted {
            key: P2CandidateKey::player(0),
            sealed_index: 1,
        },
        P3CandidateResult::Rejected {
            key: P2CandidateKey::player(1),
            sealed_index: 0,
            reason: RejectionReason::OrderNotFound,
        },
    ];

    assert!(matches!(
        adapt_p3_rejection_facts(&candidates, &swapped),
        Err(StepFatal::InvariantViolation { description, location })
            if description.contains("canonical P2 batch sealed identity")
                && location == "pipeline::p7_producers"
    ));
}

#[test]
fn unknown_stock_cancel_normalizer_emits_intent_rejected_with_the_sealed_key() {
    let known = code("600001");
    let unknown = code("600999");
    let results = vec![
        P3CandidateResult::Accepted {
            key: P2CandidateKey::player(0),
            sealed_index: 0,
        },
        P3CandidateResult::Accepted {
            key: P2CandidateKey::player(1),
            sealed_index: 1,
        },
    ];
    let operations = vec![
        P3ValidatedOperation::Cancel {
            candidate_key: P2CandidateKey::player(0),
            sealed_index: 0,
            account: AccountId(7),
            code: known.clone(),
            order_id: OrderId(4),
        },
        P3ValidatedOperation::Cancel {
            candidate_key: P2CandidateKey::player(1),
            sealed_index: 1,
            account: AccountId(9),
            code: unknown.clone(),
            order_id: OrderId(12),
        },
    ];
    let normalized = normalize_p3_p4_operations(&results, &operations, [known]).unwrap();
    assert_eq!(normalized.rejections()[0].order_id(), OrderId(12));

    let facts = adapt_p3_p4_cancel_rejections(normalized.rejections()).unwrap();

    // The existing public IntentRejected payload has no target order ID. The normalizer
    // retains that audit provenance; this adapter must not invent a public field for it.
    assert_eq!(normalized.rejections()[0].order_id(), OrderId(12));

    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].event,
        Event::IntentRejected {
            seq: 0,
            account: AccountId(9),
            code: unknown,
            reason: RejectionReason::UnknownStock,
        }
    );
    assert_eq!(facts[0].key, EventStableKey::for_event(&facts[0].event, 1));
}
