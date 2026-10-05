use super::*;

fn received_intent(
    session: &mut GameSession,
    owner: AccountId,
    intent: Intent,
) -> ReceiptBearingIntent {
    session
        .state
        .ingress_receipt_cursors
        .receive(owner, intent)
        .unwrap()
}

#[test]
fn player_candidate_capture_returns_empty_batch_without_touching_session_state() {
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 41).unwrap();
    let next_order_id_before = session.state.next_order_id;
    let seq_before = session.state.seq;

    let batch = session.capture_player_candidate_batch();

    assert!(batch.intents.is_empty());
    assert!(session.state.pending_player.is_empty());
    assert_eq!(session.state.next_order_id, next_order_id_before);
    assert_eq!(session.state.seq, seq_before);
}

#[test]
fn player_candidate_capture_drains_interleaved_queue_once_in_global_fifo_order() {
    let first_player = AccountId(0);
    let second_player = AccountId(3);
    let first_code = StockCode("600888".to_owned());
    let second_code = StockCode("000001".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let queued = vec![
        received_intent(
            &mut session,
            first_player,
            Intent::PlaceLimit {
                code: first_code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        ),
        received_intent(
            &mut session,
            second_player,
            Intent::Cancel {
                code: second_code.clone(),
                id: OrderId(7),
            },
        ),
        received_intent(
            &mut session,
            first_player,
            Intent::PlaceMarket {
                code: first_code.clone(),
                side: Side::Sell,
                qty: 200,
            },
        ),
    ];
    session.state.pending_player = queued.clone();

    let batch = session.capture_player_candidate_batch();

    assert_eq!(
        serde_json::to_vec(&batch.intents).unwrap(),
        serde_json::to_vec(&queued).unwrap()
    );
    assert!(session.state.pending_player.is_empty());
    assert!(session.capture_player_candidate_batch().intents.is_empty());
}

#[test]
fn player_candidate_capture_preserves_same_account_payloads_without_normalization() {
    let player = AccountId(0);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 43).unwrap();
    let queued = vec![
        received_intent(
            &mut session,
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(901)),
                qty: 100,
            },
        ),
        received_intent(
            &mut session,
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(899)),
                qty: 200,
            },
        ),
        received_intent(
            &mut session,
            player,
            Intent::Cancel {
                code,
                id: OrderId(11),
            },
        ),
    ];
    session.state.pending_player = queued.clone();

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
    let queued = received_intent(
        &mut session,
        player,
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(900)),
            qty: 100,
        },
    );
    session.state.pending_player = vec![queued];
    let before_snapshot = serde_json::to_vec(&session.snapshot()).unwrap();
    let identities = (session.state.next_order_id, session.state.seq);

    let batch = session.capture_player_candidate_batch();

    assert_eq!(batch.intents.len(), 1);
    assert!(session.state.pending_player.is_empty());
    assert_eq!((session.state.next_order_id, session.state.seq), identities);
    assert_eq!(
        serde_json::to_vec(&session.snapshot()).unwrap(),
        before_snapshot
    );
}

#[test]
fn full_u64_receipt_ordinals_roundtrip_and_overflow_atomically() {
    let player = AccountId(0);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 45).unwrap();
    session
        .state
        .ingress_receipt_cursors
        .next_account_ordinal
        .insert(player, u64::MAX - 1);
    session
        .state
        .ingress_receipt_cursors
        .next_stock_ordinal
        .insert(code.clone(), u64::MAX - 1);
    let intent = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(900)),
        qty: 100,
    };

    session.enqueue_player_intent(player, intent).unwrap();

    let save = session.save().unwrap();
    let encoded = serde_json::to_value(&save).unwrap();
    assert_eq!(
        encoded["pending_player"][0]["account_ordinal"],
        (u64::MAX - 1).to_string()
    );
    assert_eq!(
        encoded["pending_player"][0]["stock_ordinal"],
        (u64::MAX - 1).to_string()
    );
    let mut restored = GameSession::restore(&save).unwrap();
    let pending_before = serde_json::to_value(&restored.state.pending_player).unwrap();
    let cursors_before = serde_json::to_value(&restored.state.ingress_receipt_cursors).unwrap();
    let error = restored
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap_err();
    assert!(matches!(error, SessionError::ResourceLimit(_)));
    assert_eq!(
        serde_json::to_value(&restored.state.pending_player).unwrap(),
        pending_before
    );
    assert_eq!(
        serde_json::to_value(&restored.state.ingress_receipt_cursors).unwrap(),
        cursors_before
    );
    restored
        .state
        .ingress_receipt_cursors
        .next_account_ordinal
        .insert(player, 0);
    let account_cursors_before = restored
        .state
        .ingress_receipt_cursors
        .next_account_ordinal
        .clone();
    let error = restored
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap_err();
    assert!(matches!(error, SessionError::ResourceLimit(_)));
    assert_eq!(
        restored.state.ingress_receipt_cursors.next_account_ordinal,
        account_cursors_before
    );
    assert_eq!(
        serde_json::to_value(&restored.state.pending_player).unwrap(),
        pending_before
    );
}

#[test]
fn receipt_and_cursor_wire_values_reject_noncanonical_decimal_strings() {
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 47).unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap();
    let encoded = serde_json::to_value(session.save().unwrap()).unwrap();

    for value in ["01", "+1"] {
        let mut malformed = encoded.clone();
        malformed["pending_player"][0]["account_ordinal"] = value.into();
        assert!(serde_json::from_value::<crate::session::SaveSlot>(malformed).is_err());
    }
    for value in ["01", "+1"] {
        let mut malformed = encoded.clone();
        malformed["ingress_receipt_cursors"]["next_account_ordinal"]["0"] = value.into();
        assert!(serde_json::from_value::<crate::session::SaveSlot>(malformed).is_err());
    }
    for value in ["01", "+1"] {
        let mut malformed = encoded.clone();
        malformed["ingress_receipt_cursors"]["next_stock_ordinal"][code.0.as_str()] = value.into();
        assert!(serde_json::from_value::<crate::session::SaveSlot>(malformed).is_err());
    }
}

#[test]
fn restore_rejects_cross_lane_receipt_cycle() {
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 46).unwrap();
    for price in [900, 901] {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: LimitPrice::Fixed(Money::from_cents(price)),
                    qty: 100,
                },
            )
            .unwrap();
    }
    let mut save = session.save().unwrap();
    save.pending_player[0].stock_ordinal = 1;
    save.pending_player[1].stock_ordinal = 0;

    assert!(matches!(
        GameSession::restore(&save),
        Err(SessionError::InvalidSave(message)) if message.contains("precedence contains a cycle")
    ));
}
