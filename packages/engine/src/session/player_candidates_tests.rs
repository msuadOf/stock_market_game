use super::*;

#[test]
fn player_candidate_capture_returns_empty_batch_without_touching_session_state() {
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 41).unwrap();
    let next_order_id_before = session.next_order_id;
    let seq_before = session.seq;

    let batch = session.capture_player_candidate_batch();

    assert!(batch.intents.is_empty());
    assert!(session.pending_player.is_empty());
    assert_eq!(session.next_order_id, next_order_id_before);
    assert_eq!(session.seq, seq_before);
}

#[test]
fn player_candidate_capture_drains_interleaved_queue_once_in_global_fifo_order() {
    let first_player = AccountId(0);
    let second_player = AccountId(3);
    let first_code = StockCode("600888".to_owned());
    let second_code = StockCode("000001".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let queued = vec![
        (
            first_player,
            Intent::PlaceLimit {
                code: first_code.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
        ),
        (
            second_player,
            Intent::Cancel {
                code: second_code.clone(),
                id: OrderId(7),
            },
        ),
        (
            first_player,
            Intent::PlaceMarket {
                code: first_code.clone(),
                side: Side::Sell,
                qty: 200,
            },
        ),
    ];
    session.pending_player = queued.clone();

    let batch = session.capture_player_candidate_batch();

    assert_eq!(
        serde_json::to_vec(&batch.intents).unwrap(),
        serde_json::to_vec(&queued).unwrap()
    );
    assert!(session.pending_player.is_empty());
    assert!(session.capture_player_candidate_batch().intents.is_empty());
}

#[test]
fn player_candidate_capture_preserves_same_account_payloads_without_normalization() {
    let player = AccountId(0);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 43).unwrap();
    let queued = vec![
        (
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(901),
                qty: 100,
            },
        ),
        (
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(899),
                qty: 200,
            },
        ),
        (
            player,
            Intent::Cancel {
                code,
                id: OrderId(11),
            },
        ),
    ];
    session.pending_player = queued.clone();

    let batch = session.capture_player_candidate_batch();

    assert_eq!(
        serde_json::to_vec(&batch.intents).unwrap(),
        serde_json::to_vec(&queued).unwrap()
    );
}

#[test]
fn player_candidate_capture_transfers_queued_intents_without_routing_side_effects() {
    let player = AccountId(0);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 44).unwrap();
    session.pending_player = vec![(
        player,
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
    )];
    let before_snapshot = serde_json::to_vec(&session.snapshot()).unwrap();
    let identities = (session.next_order_id, session.seq);

    let batch = session.capture_player_candidate_batch();

    assert_eq!(batch.intents.len(), 1);
    assert!(session.pending_player.is_empty());
    assert_eq!((session.next_order_id, session.seq), identities);
    assert_eq!(
        serde_json::to_vec(&session.snapshot()).unwrap(),
        before_snapshot
    );
}
