use super::p2_composition::{compose_projected_p2_candidates, P2SourceCompositionError};
use super::*;
use crate::session::player_candidates::PlayerCandidateBatch;
use crate::{AccountId, Intent, Money, Side, StockCode};

fn place(code: &StockCode) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(900),
        qty: 100,
    }
}

#[test]
fn projected_npc_and_queued_player_keep_their_request_identities() {
    let code = StockCode("600888".to_owned());
    let npc = P2CandidateBatch::new(vec![
        P2Candidate::new(
            P2CandidateKey::npc(AccountId(9), 0),
            AccountId(9),
            place(&code),
        ),
        P2Candidate::new(
            P2CandidateKey::npc(AccountId(9), 1),
            AccountId(9),
            place(&code),
        ),
    ])
    .unwrap();
    let batch = compose_projected_p2_candidates(
        npc,
        PlayerCandidateBatch {
            intents: vec![(AccountId(0), place(&code))],
        },
    )
    .unwrap();
    assert_eq!(
        batch
            .candidates()
            .iter()
            .map(|candidate| (candidate.owner(), candidate.key().clone()))
            .collect::<Vec<_>>(),
        vec![
            (AccountId(9), P2CandidateKey::npc(AccountId(9), 0)),
            (AccountId(9), P2CandidateKey::npc(AccountId(9), 1)),
            (AccountId(0), P2CandidateKey::player(0)),
        ]
    );
}

#[test]
fn projected_npc_rejects_wrong_source_or_owner() {
    let code = StockCode("600888".to_owned());
    let wrong_source = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::player(0),
        AccountId(1),
        place(&code),
    )])
    .unwrap();
    assert!(matches!(
        compose_projected_p2_candidates(wrong_source, PlayerCandidateBatch { intents: vec![] }),
        Err(P2SourceCompositionError::NonNpcKey(
            P2CandidateKey::Player { .. }
        ))
    ));
    let wrong_owner = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::npc(AccountId(1), 0),
        AccountId(2),
        place(&code),
    )])
    .unwrap();
    assert!(matches!(
        compose_projected_p2_candidates(wrong_owner, PlayerCandidateBatch { intents: vec![] }),
        Err(P2SourceCompositionError::NpcOwnerMismatch { .. })
    ));
}
