use super::npc_p2_source::run_npc_p2_source;
use super::p2_composition::{
    compose_p2_source_candidates, compose_projected_p2_candidates, p2_candidate_from_keyed_npc_raw,
    P2SourceCompositionError,
};
use super::p3_context::build_p3_validation_context;
use super::*;
use crate::session::{
    plan_chain_candidates::PlanChainCandidateBatch, player_candidates::PlayerCandidateBatch,
};
use crate::strategy::{MarketView, MomentumStrategy, SelfView, StockView, StrategyState};
use crate::{AccountId, AccountKind, Money, StockCode, TradingPhase};
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
fn p2_source_composition_preserves_real_npc_keys_before_player_and_plan_chain() {
    let snapshot = nonempty_npc_snapshot();
    let snapshot_before =
        serde_json::to_vec(snapshot.account(AccountId(9)).unwrap().strategy_state()).unwrap();
    let npc = run_npc_p2_source(snapshot.clone()).unwrap();
    assert!(
        !npc.intents().is_empty(),
        "fixture must produce real NPC intents"
    );
    let expected_npc = npc
        .intents()
        .iter()
        .map(|intent| {
            let owner = match intent.key() {
                P2CandidateKey::Npc { account, .. } => *account,
                key => panic!("real NPC source emitted a non-NPC key {key:?}"),
            };
            P2Candidate::new(intent.key().clone(), owner, intent.intent().clone())
        })
        .collect::<Vec<_>>();
    let code = StockCode("600888".to_owned());
    let player_owner = AccountId(0);
    let player_intent = place(code.clone(), 902);
    let plan_owner = AccountId(3);
    let plan_intent = place(code.clone(), 903);

    let batch = compose_p2_source_candidates(
        &npc,
        PlayerCandidateBatch {
            intents: vec![(player_owner, player_intent.clone())],
        },
        vec![PlanChainCandidateBatch {
            owner: plan_owner,
            intent: plan_intent.clone(),
            chain_generation_index: 0,
        }],
    )
    .unwrap();

    assert_eq!(
        batch.candidates(),
        expected_npc
            .into_iter()
            .chain([
                P2Candidate::new(P2CandidateKey::player(0), player_owner, player_intent),
                P2Candidate::new(P2CandidateKey::plan_chain(0), plan_owner, plan_intent),
            ])
            .collect::<Vec<_>>()
    );
    assert_eq!(
        serde_json::to_vec(snapshot.account(AccountId(9)).unwrap().strategy_state()).unwrap(),
        snapshot_before
    );
}

#[test]
fn projected_p2_composition_preserves_npc_then_player_then_plan_chain_class_order() {
    let npc_owner = AccountId(2);
    let player_owner = AccountId(0);
    let plan_owner = AccountId(1);
    let npc = P2CandidateBatch::new(vec![P2Candidate::new(
        P2CandidateKey::npc(npc_owner, 0),
        npc_owner,
        place(StockCode("000001".to_owned()), 900),
    )])
    .unwrap();

    let combined = compose_projected_p2_candidates(
        &npc,
        PlayerCandidateBatch {
            intents: vec![(player_owner, place(StockCode("000001".to_owned()), 901))],
        },
        [PlanChainCandidateBatch {
            owner: plan_owner,
            intent: place(StockCode("000001".to_owned()), 902),
            chain_generation_index: 0,
        }],
    )
    .unwrap();

    assert_eq!(
        combined
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            P2CandidateKey::npc(npc_owner, 0),
            P2CandidateKey::player(0),
            P2CandidateKey::plan_chain(0),
        ]
    );
}

#[test]
fn p2_source_composition_wraps_a_noncanonical_plan_chain_index() {
    let npc = run_npc_p2_source(nonempty_npc_snapshot()).unwrap();
    let code = StockCode("600888".to_owned());

    assert!(matches!(
        compose_p2_source_candidates(
            &npc,
            empty_player(),
            vec![PlanChainCandidateBatch {
                owner: AccountId(3),
                intent: place(code, 903),
                chain_generation_index: 1,
            }],
        ),
        Err(P2SourceCompositionError::Candidate(
            P2CandidateError::InvalidSourceSequence
        ))
    ));
}

#[test]
fn keyed_npc_raw_candidate_rejects_a_non_npc_key_or_owner_mismatch() {
    let code = StockCode("600888".to_owned());

    assert!(matches!(
        p2_candidate_from_keyed_npc_raw(
            P2CandidateKey::player(0),
            AccountId(1),
            place(code.clone(), 900),
        ),
        Err(P2SourceCompositionError::NonNpcKey(
            P2CandidateKey::Player { .. }
        ))
    ));
    assert!(matches!(
        p2_candidate_from_keyed_npc_raw(
            P2CandidateKey::npc(AccountId(1), 0),
            AccountId(2),
            place(code, 901),
        ),
        Err(P2SourceCompositionError::NpcOwnerMismatch { .. })
    ));
}

#[test]
fn p2_composition_preserves_owned_source_order_without_session_mutation() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    let npc = keyed_npc_batch([
        (account, 0, place(code.clone(), 900)),
        (account, 1, place(code.clone(), 901)),
    ]);
    let player = PlayerCandidateBatch {
        intents: vec![(account, place(code.clone(), 902))],
    };
    let plan_chain = vec![PlanChainCandidateBatch {
        owner: account,
        intent: place(code, 903),
        chain_generation_index: 0,
    }];

    let batch = compose_projected_p2_candidates(&npc, player, plan_chain).unwrap();
    let keys: Vec<_> = batch
        .candidates()
        .iter()
        .map(|candidate| candidate.key().clone())
        .collect();

    assert_eq!(
        keys,
        vec![
            P2CandidateKey::npc(account, 0),
            P2CandidateKey::npc(account, 1),
            P2CandidateKey::player(0),
            P2CandidateKey::plan_chain(0),
        ]
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn p2_composition_rejects_duplicate_or_unordered_plan_chain_identity() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let duplicate = vec![
        PlanChainCandidateBatch {
            owner: account,
            intent: place(code.clone(), 900),
            chain_generation_index: 0,
        },
        PlanChainCandidateBatch {
            owner: account,
            intent: place(code.clone(), 901),
            chain_generation_index: 0,
        },
    ];
    let unordered = vec![
        PlanChainCandidateBatch {
            owner: account,
            intent: place(code.clone(), 902),
            chain_generation_index: 1,
        },
        PlanChainCandidateBatch {
            owner: account,
            intent: place(code, 903),
            chain_generation_index: 0,
        },
    ];
    let gapped = vec![
        PlanChainCandidateBatch {
            owner: account,
            intent: place(crate::StockCode("600888".to_owned()), 904),
            chain_generation_index: 0,
        },
        PlanChainCandidateBatch {
            owner: account,
            intent: place(crate::StockCode("600888".to_owned()), 905),
            chain_generation_index: 2,
        },
    ];

    assert!(compose_projected_p2_candidates(&empty_npc(), empty_player(), duplicate).is_err());
    assert!(compose_projected_p2_candidates(&empty_npc(), empty_player(), unordered).is_err());
    assert!(compose_projected_p2_candidates(&empty_npc(), empty_player(), gapped).is_err());
}

#[test]
fn p2_composition_feeds_the_p3_handoff_without_candidate_reordering() {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let batch = compose_projected_p2_candidates(
        &keyed_npc_batch([(account, 0, place(code.clone(), 900))]),
        PlayerCandidateBatch {
            intents: vec![(account, place(code.clone(), 901))],
        },
        vec![PlanChainCandidateBatch {
            owner: account,
            intent: place(code, 902),
            chain_generation_index: 0,
        }],
    )
    .unwrap();

    let output = P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        build_p3_validation_context(&game).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap();

    assert_eq!(
        output.accepted().cloned().collect::<Vec<_>>(),
        vec![
            P2CandidateKey::npc(account, 0),
            P2CandidateKey::player(0),
            P2CandidateKey::plan_chain(0),
        ]
    );
}

fn keyed_npc_batch<const N: usize>(
    entries: [(AccountId, u64, crate::Intent); N],
) -> P2CandidateBatch {
    P2CandidateBatch::new(
        entries
            .into_iter()
            .map(|(owner, index, intent)| {
                P2Candidate::new(P2CandidateKey::npc(owner, index), owner, intent)
            })
            .collect(),
    )
    .unwrap()
}

fn empty_npc() -> P2CandidateBatch {
    keyed_npc_batch([])
}

fn empty_player() -> PlayerCandidateBatch {
    PlayerCandidateBatch {
        intents: Vec::new(),
    }
}

fn nonempty_npc_snapshot() -> Arc<DecisionSnapshot> {
    let market = MarketView {
        stocks: ["600001", "600002"]
            .into_iter()
            .map(|code| {
                (
                    StockCode(code.to_owned()),
                    StockView {
                        best_bid: Some(Money::from_cents(1_049)),
                        best_ask: Some(Money::from_cents(1_051)),
                        last_price: Money::from_cents(1_050),
                        recent_prices: vec![
                            Money::from_cents(1_000),
                            Money::from_cents(1_020),
                            Money::from_cents(1_050),
                        ],
                        recent_market_minute_prices: vec![
                            Money::from_cents(1_000),
                            Money::from_cents(1_020),
                            Money::from_cents(1_050),
                        ],
                        relative_volume: 1.0,
                        order_book_imbalance: 0.0,
                    },
                )
            })
            .collect(),
        tick: 7,
        market_minute: 11,
    };
    let account = AccountId(9);
    let accounts = [(
        account,
        DecisionAccountInput::new(
            AccountKind::Hot,
            SelfView {
                cash: Money::from_cents(10_000_000),
                positions: BTreeMap::new(),
            },
            StrategyState::Momentum(MomentumStrategy::new(3, 0.02, 100).unwrap()),
            None,
            None,
        ),
    )]
    .into();
    Arc::new(
        DecisionSnapshot::new(
            7,
            8,
            TradingPhase::Continuous,
            11,
            market,
            None,
            vec![account],
            accounts,
        )
        .unwrap(),
    )
}

fn place(code: crate::StockCode, price: i64) -> crate::Intent {
    crate::Intent::PlaceLimit {
        code,
        side: crate::Side::Buy,
        price: crate::Money::from_cents(price),
        qty: 100,
    }
}
