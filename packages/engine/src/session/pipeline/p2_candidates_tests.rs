use super::*;

fn place_limit(price_cents: i64) -> crate::Intent {
    crate::Intent::PlaceLimit {
        code: crate::StockCode("600888".to_owned()),
        side: crate::Side::Buy,
        price: crate::Money::from_cents(price_cents),
        qty: 100,
    }
}

#[test]
fn p2_candidate_batch_preserves_source_order_without_using_key_as_priority() {
    let npc_owner = crate::AccountId(2);
    let player_owner = crate::AccountId(0);
    let plan_owner = crate::AccountId(3);
    let batch = P2CandidateBatch::new(vec![
        P2Candidate::new(P2CandidateKey::plan_chain(4), plan_owner, place_limit(903)),
        P2Candidate::new(
            P2CandidateKey::npc(npc_owner, 1),
            npc_owner,
            place_limit(902),
        ),
        P2Candidate::new(P2CandidateKey::player(1), player_owner, place_limit(901)),
        P2Candidate::new(
            P2CandidateKey::npc(npc_owner, 0),
            npc_owner,
            place_limit(900),
        ),
        P2Candidate::new(P2CandidateKey::player(0), player_owner, place_limit(899)),
    ])
    .unwrap();

    let keys: Vec<_> = batch
        .candidates()
        .iter()
        .map(P2Candidate::key)
        .cloned()
        .collect();

    assert_eq!(
        keys,
        vec![
            P2CandidateKey::plan_chain(4),
            P2CandidateKey::npc(npc_owner, 1),
            P2CandidateKey::player(1),
            P2CandidateKey::npc(npc_owner, 0),
            P2CandidateKey::player(0),
        ]
    );
}

#[test]
fn p2_candidate_key_preserves_source_local_tie_breakers() {
    let first_owner = crate::AccountId(1);
    let second_owner = crate::AccountId(2);

    assert!(P2CandidateKey::npc(first_owner, 9) < P2CandidateKey::npc(second_owner, 0));
    assert!(P2CandidateKey::npc(second_owner, 0) < P2CandidateKey::npc(second_owner, 1));
    assert_eq!(P2CandidateKey::player(7).source(), CandidateSource::Player);
    assert_eq!(
        P2CandidateKey::plan_chain(8).source_local_key(),
        CandidateSourceLocalKey::PlanChain {
            chain_generation_index: 8
        }
    );
}

#[test]
fn p2_candidate_batch_rejects_duplicate_keys_but_keeps_reverse_key_order() {
    let owner = crate::AccountId(1);
    let first = P2Candidate::new(P2CandidateKey::npc(owner, 0), owner, place_limit(900));
    let duplicate = P2Candidate::new(P2CandidateKey::npc(owner, 0), owner, place_limit(901));
    let later = P2Candidate::new(P2CandidateKey::npc(owner, 1), owner, place_limit(902));

    assert!(matches!(
        P2CandidateBatch::new(vec![first.clone(), duplicate]),
        Err(P2CandidateError::DuplicateKey(_))
    ));
    let batch = P2CandidateBatch::new(vec![later, first]).unwrap();
    assert_eq!(batch.candidates()[0].key(), &P2CandidateKey::npc(owner, 1));
    assert_eq!(batch.candidates()[1].key(), &P2CandidateKey::npc(owner, 0));
}

#[test]
fn p2_candidate_batch_serializes_owner_payload_and_only_p2_identity() {
    let owner = crate::AccountId(1);
    let batch = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::npc(owner, 0),
        owner,
        place_limit(900),
    )])
    .unwrap();

    let encoded = String::from_utf8(serde_json::to_vec(&batch).unwrap()).unwrap();

    assert!(encoded.contains("Npc"));
    assert!(encoded.contains("owner"));
    assert!(encoded.contains("PlaceLimit"));
    assert!(!encoded.contains("receipt_index"));
    assert!(!encoded.contains("event_seq"));
}

#[test]
fn p2_candidate_batch_round_trips_serialized_transfer_equality() {
    let owner = crate::AccountId(1);
    let batch = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::npc(owner, 0),
        owner,
        place_limit(900),
    )])
    .unwrap();

    let decoded: P2CandidateBatch =
        serde_json::from_slice(&serde_json::to_vec(&batch).unwrap()).unwrap();

    assert_eq!(decoded, batch);
}

#[test]
fn p2_candidate_batch_deserializes_source_order_and_rejects_duplicate_identity() {
    let owner = crate::AccountId(1);
    let batch = P2CandidateBatch::new(vec![
        P2Candidate::new(P2CandidateKey::npc(owner, 1), owner, place_limit(901)),
        P2Candidate::new(P2CandidateKey::npc(owner, 0), owner, place_limit(900)),
    ])
    .unwrap();
    let mut value = serde_json::to_value(&batch).unwrap();
    value["candidates"].as_array_mut().unwrap().reverse();
    let reversed = serde_json::from_value::<P2CandidateBatch>(value.clone()).unwrap();
    assert_eq!(
        reversed.candidates()[0].key(),
        &P2CandidateKey::npc(owner, 0)
    );
    value["candidates"][1] = value["candidates"][0].clone();
    let error = serde_json::from_value::<P2CandidateBatch>(value).unwrap_err();
    assert!(error.to_string().contains("duplicate P2 candidate key"));
}
