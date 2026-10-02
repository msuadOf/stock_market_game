use super::session_fact_producers::adapt_account_validation_rejection_facts;
use super::*;
use crate::{AccountId, Event, Intent, Money, OrderId, RejectionReason, Side, StockCode};

fn code(value: &str) -> StockCode {
    StockCode(value.to_owned())
}

fn candidate(key: IntentCandidateKey, account: u64, intent: Intent) -> IntentCandidate {
    IntentCandidate::new(key, AccountId(account), intent)
}

#[test]
fn account_validation_rejection_adapter_uses_candidate_payload_and_explicit_sealed_identity() {
    let alpha = code("600001");
    let beta = code("600002");
    let candidates = IntentCandidateBatch::new(vec![
        candidate(
            IntentCandidateKey::npc(AccountId(4), 0),
            4,
            Intent::Cancel {
                code: alpha.clone(),
                id: OrderId(8),
            },
        ),
        candidate(
            IntentCandidateKey::player(5),
            7,
            Intent::PlaceLimit {
                code: beta.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        ),
        candidate(
            IntentCandidateKey::plan_chain(9),
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
        CandidateValidationResult::Accepted {
            key: IntentCandidateKey::npc(AccountId(4), 0),
            sealed_index: 0,
        },
        CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(5),
            sealed_index: 1,
            reason: RejectionReason::InsufficientCash,
        },
        CandidateValidationResult::Rejected {
            key: IntentCandidateKey::plan_chain(9),
            sealed_index: 2,
            reason: RejectionReason::InsufficientShares,
        },
    ];

    let facts = adapt_account_validation_rejection_facts(&candidates, &results).unwrap();

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
fn account_validation_rejection_adapter_rejects_duplicate_or_missing_candidate_contracts_without_output(
) {
    let alpha = code("600001");
    let candidates = IntentCandidateBatch::new(vec![candidate(
        IntentCandidateKey::player(0),
        7,
        Intent::Cancel {
            code: alpha.clone(),
            id: OrderId(8),
        },
    )])
    .unwrap();
    let duplicate = vec![
        CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(0),
            sealed_index: 0,
            reason: RejectionReason::OrderNotFound,
        },
        CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(0),
            sealed_index: 1,
            reason: RejectionReason::OrderNotFound,
        },
    ];
    assert!(matches!(
        adapt_account_validation_rejection_facts(&candidates, &duplicate),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::session_fact_producers"
    ));

    let missing = Vec::new();
    assert!(matches!(
        adapt_account_validation_rejection_facts(&candidates, &missing),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::session_fact_producers"
    ));
}

#[test]
fn account_validation_rejection_adapter_rejects_a_candidate_key_and_sealed_identity_swap() {
    let alpha = code("600001");
    let candidates = IntentCandidateBatch::new(vec![
        candidate(
            IntentCandidateKey::player(0),
            7,
            Intent::Cancel {
                code: alpha.clone(),
                id: OrderId(8),
            },
        ),
        candidate(
            IntentCandidateKey::player(1),
            9,
            Intent::Cancel {
                code: alpha,
                id: OrderId(9),
            },
        ),
    ])
    .unwrap();
    let swapped = vec![
        CandidateValidationResult::Accepted {
            key: IntentCandidateKey::player(0),
            sealed_index: 1,
        },
        CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(1),
            sealed_index: 0,
            reason: RejectionReason::OrderNotFound,
        },
    ];

    assert!(matches!(
        adapt_account_validation_rejection_facts(&candidates, &swapped),
        Err(StepFatal::InvariantViolation { description, location })
            if description.contains("canonical P2 batch sealed identity")
                && location == "pipeline::session_fact_producers"
    ));
}
