use super::candidate_composition::{compose_projected_candidates, CandidateCompositionError};
use super::*;
use crate::session::player_candidates::PlayerCandidateBatch;
use crate::session::ReceiptBearingIntent;
use crate::{AccountId, Intent, Money, Side, StockId};

fn place(code: &StockId) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(900)),
        qty: 100,
    }
}

#[test]
fn projected_npc_and_queued_player_keep_their_request_identities() {
    // 数值身份后无字符串可断言：纯 fixture 单股按目录 rank 取 StockId(0)。
    let code = StockId(0);
    let npc = IntentCandidateBatch::new(vec![
        IntentCandidate::new(
            IntentCandidateKey::npc(AccountId(9), 0),
            AccountId(9),
            place(&code),
        )
        .with_ingress_order(0, 0),
        IntentCandidate::new(
            IntentCandidateKey::npc(AccountId(9), 1),
            AccountId(9),
            place(&code),
        )
        .with_ingress_order(1, 1),
    ])
    .unwrap();
    let batch = compose_projected_candidates(
        npc,
        PlayerCandidateBatch {
            intents: vec![ReceiptBearingIntent {
                owner: AccountId(0),
                intent: place(&code),
                account_ordinal: 0,
                stock_ordinal: 2,
            }],
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
            (AccountId(9), IntentCandidateKey::npc(AccountId(9), 0)),
            (AccountId(9), IntentCandidateKey::npc(AccountId(9), 1)),
            (AccountId(0), IntentCandidateKey::player(0)),
        ]
    );
}

#[test]
fn projected_npc_rejects_wrong_source_or_owner() {
    // 数值身份后无字符串可断言：纯 fixture 单股按目录 rank 取 StockId(0)。
    let code = StockId(0);
    let wrong_source = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        AccountId(1),
        place(&code),
    )])
    .unwrap();
    assert!(matches!(
        compose_projected_candidates(wrong_source, PlayerCandidateBatch { intents: vec![] }),
        Err(CandidateCompositionError::NonNpcKey(
            IntentCandidateKey::Player { .. }
        ))
    ));
    let wrong_owner = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::npc(AccountId(1), 0),
        AccountId(2),
        place(&code),
    )])
    .unwrap();
    assert!(matches!(
        compose_projected_candidates(wrong_owner, PlayerCandidateBatch { intents: vec![] }),
        Err(CandidateCompositionError::NpcOwnerMismatch { .. })
    ));
}
