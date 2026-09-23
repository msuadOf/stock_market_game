use super::super::*;
use super::v2::*;
use crate::session::pipeline::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents, JournalRank,
    ReceiptLocalKey, ReceiptSource, ReceiptTransition,
};

fn v2_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    GameSession::new(setup, 42).expect("v2 fixture must be valid")
}

fn low_price_v2_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.commission_min = Money::ZERO;
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    GameSession::new(setup, 42).expect("low-price v2 fixture must be valid")
}

fn install_live_buy(session: &mut GameSession) -> EnvelopeKey {
    let code = session.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Buy,
    };
    let result = session
        .markets
        .get_mut(&code)
        .expect("fixture market must exist")
        .place(Order {
            id: key.order,
            side: key.side,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: key.account,
            seq: 1,
        })
        .expect("fixture order must be accepted");
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    session.next_order_id = 2;
    session
        .hydrate_or_validate_envelope_ledger()
        .expect("fixture ledger must hydrate");
    key
}

fn install_receipt_prefix(session: &mut GameSession, envelope: EnvelopeKey) {
    let local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(0),
        ReceiptTransition {
            envelope,
            ordinal: 0,
        },
    )
    .expect("fixture receipt identity must be valid");
    session
        .retail_projection_seen
        .insert_test_identity(0, local_key);
    session.next_receipt_base = 1;
    session.envelope_ledger = EnvelopeLedger::new(
        session.next_receipt_base,
        session
            .project_live_envelopes()
            .expect("fixture live envelope projection must succeed"),
    )
    .expect("fixture ledger cursor must be valid");
}

fn install_partially_filled_sell(session: &mut GameSession) -> EnvelopeKey {
    let code = session.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    let filled_value = Money::from_cents(50_001);
    session
        .accounts
        .get_mut(&key.account)
        .expect("fixture seller must exist")
        .grant_position(code.clone(), 1, Money::from_cents(1))
        .expect("fixture historical holding must be valid");
    let result = session
        .markets
        .get_mut(&code)
        .expect("fixture market must exist")
        .place(Order {
            id: key.order,
            side: key.side,
            price: Money::from_cents(1),
            qty: 1,
            original_qty: 50_002,
            filled_qty: 50_001,
            filled_value,
            owner: key.account,
            seq: 1,
        })
        .expect("fixture partially-filled order must rest");
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    session.next_order_id = 2;
    let nominal = FeeComponents {
        commission: session.setup.config.commission(filled_value).unwrap(),
        stamp_tax: session.setup.config.stamp_tax(filled_value).unwrap(),
        transfer_fee: session.setup.config.transfer_fee(filled_value).unwrap(),
    };
    let charged = FeeComponents {
        commission: Money::from_cents(13),
        stamp_tax: Money::from_cents(25),
        transfer_fee: Money::ZERO,
    };
    session.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            key.clone(),
            Money::ZERO,
            1,
            EnvelopeAudit {
                limit: Money::from_cents(1),
                remaining_qty: 1,
                filled_qty: 50_001,
                filled_value,
                nominal,
                charged,
            },
        )],
    )
    .expect("fixture cumulative fee ledger must be valid");
    key
}

fn assert_invalid_save(error: SessionError, message: &str) {
    match error {
        SessionError::InvalidSave(description) => assert!(
            description.contains(message),
            "expected {message:?} in {description:?}"
        ),
        other => panic!("corrupt persisted input must be InvalidSave, got {other}"),
    }
}

#[test]
fn schema_header_accepts_only_explicit_v2() {
    validate_schema_version_header(br#"{"schema_version":2}"#).unwrap();

    for (json, message) in [
        (br#"{}"#.as_slice(), "missing"),
        (br#"{"schema_version":1}"#.as_slice(), "legacy"),
        (br#"{"schema_version":3}"#.as_slice(), "newer"),
    ] {
        assert_invalid_save(validate_schema_version_header(json).unwrap_err(), message);
    }
}

#[test]
fn healthy_initial_quiet_point_roundtrips_complete_strategy_state() {
    let source = v2_session();
    let state = capture_runtime_v2(&source).expect("healthy quiet point must capture");

    assert!(!state.poisoned);
    assert_eq!(state.next_receipt_base, 0);
    assert!(state.live_envelopes.is_empty());
    assert!(state.retail_projection_seen.is_empty());
    assert_eq!(state.strategy_states.len(), 1);

    let mut restored = v2_session();
    restore_runtime_v2(&mut restored, &state).expect("valid v2 state must restore");
    assert_eq!(capture_runtime_v2(&restored).unwrap(), state);
}

#[test]
fn live_envelope_and_receipt_prefix_roundtrip_losslessly() {
    let mut source = v2_session();
    let envelope = install_live_buy(&mut source);
    install_receipt_prefix(&mut source, envelope);
    let state = capture_runtime_v2(&source).expect("non-empty quiet point must capture");

    assert_eq!(state.live_envelopes.len(), 1);
    assert_eq!(state.retail_projection_seen.len(), 1);
    assert_eq!(state.retail_projection_seen[0].index, 0);
    assert_eq!(state.next_receipt_base, 1);

    let mut restored = v2_session();
    install_live_buy(&mut restored);
    restore_runtime_v2(&mut restored, &state).expect("non-empty v2 state must restore");
    assert_eq!(capture_runtime_v2(&restored).unwrap(), state);
}

#[test]
fn seller_cumulative_nominal_and_charged_audit_survives_restore() {
    let mut source = low_price_v2_session();
    install_partially_filled_sell(&mut source);
    let state = capture_runtime_v2(&source).expect("seller fee debt must be persistable");
    let audit = state.live_envelopes[0].audit;

    assert_eq!(audit.filled_qty, 50_001);
    assert_eq!(audit.filled_value, Money::from_cents(50_001));
    assert_eq!(audit.nominal.commission, Money::from_cents(13));
    assert_eq!(audit.nominal.stamp_tax, Money::from_cents(25));
    assert_eq!(audit.nominal.transfer_fee, Money::from_cents(1));
    assert_eq!(audit.charged.commission, Money::from_cents(13));
    assert_eq!(audit.charged.stamp_tax, Money::from_cents(25));
    assert_eq!(audit.charged.transfer_fee, Money::ZERO);

    let mut restored = low_price_v2_session();
    install_partially_filled_sell(&mut restored);
    restore_runtime_v2(&mut restored, &state).expect("seller fee debt must restore");
    assert_eq!(capture_runtime_v2(&restored).unwrap(), state);
}

#[test]
fn receipt_gap_is_rejected_as_corrupt_save() {
    let session = v2_session();
    let mut state = capture_runtime_v2(&session).unwrap();
    state.next_receipt_base = 1;

    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "receipt prefix",
    );
}

#[test]
fn receipt_history_rejects_unknown_or_future_envelope_identity() {
    let mut session = v2_session();
    let envelope = install_live_buy(&mut session);
    install_receipt_prefix(&mut session, envelope);
    let state = capture_runtime_v2(&session).unwrap();

    let mut unknown_account = state.clone();
    unknown_account.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .account = AccountId(999);
    assert_invalid_save(
        validate_runtime_v2(&session, &unknown_account).unwrap_err(),
        "unknown account",
    );

    let mut unknown_stock = state.clone();
    unknown_stock.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .stock = StockCode("600999".to_owned());
    assert_invalid_save(
        validate_runtime_v2(&session, &unknown_stock).unwrap_err(),
        "unknown stock",
    );

    let mut zero_order = state.clone();
    zero_order.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .order = OrderId(0);
    assert_invalid_save(
        validate_runtime_v2(&session, &zero_order).unwrap_err(),
        "invalid order",
    );

    let mut future_order = state;
    future_order.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .order = OrderId(session.next_order_id);
    assert_invalid_save(
        validate_runtime_v2(&session, &future_order).unwrap_err(),
        "invalid order",
    );
}

#[test]
fn duplicate_live_envelope_key_is_rejected_as_noncanonical() {
    let mut session = v2_session();
    install_live_buy(&mut session);
    let mut state = capture_runtime_v2(&session).unwrap();
    state.live_envelopes.push(state.live_envelopes[0].clone());

    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "strict canonical key order",
    );
}

#[test]
fn live_envelope_tampering_is_rejected_without_partial_restore() {
    let mut source = v2_session();
    install_live_buy(&mut source);
    let mut state = capture_runtime_v2(&source).unwrap();
    state.live_envelopes[0].live.cash = state.live_envelopes[0]
        .live
        .cash
        .add(Money::from_cents(1))
        .unwrap();

    let mut target = v2_session();
    install_live_buy(&mut target);
    let before = capture_runtime_v2(&target).unwrap();
    assert_invalid_save(
        restore_runtime_v2(&mut target, &state).unwrap_err(),
        "disagrees with its live order",
    );
    assert_eq!(capture_runtime_v2(&target).unwrap(), before);
}

#[test]
fn poisoned_marker_and_unknown_runtime_fields_fail_closed() {
    let session = v2_session();
    let mut state = capture_runtime_v2(&session).unwrap();
    state.poisoned = true;
    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "poisoned",
    );

    let mut encoded = serde_json::to_value(capture_runtime_v2(&session).unwrap()).unwrap();
    encoded
        .as_object_mut()
        .expect("runtime DTO must encode as an object")
        .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<SaveRuntimeV2>(encoded).is_err());
}

#[test]
fn strategy_state_identity_tampering_is_rejected() {
    let session = v2_session();
    let mut state = capture_runtime_v2(&session).unwrap();
    state.strategy_states.insert(
        AccountId(1),
        crate::strategy::StrategyState::Momentum(
            crate::MomentumStrategy::new(5, 0.02, 100).unwrap(),
        ),
    );

    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "deterministic strategy identity",
    );
}

#[test]
fn capture_rejects_poisoned_session_without_emitting_a_dto() {
    let mut session = v2_session();
    let fatal = StepFatal::InvariantViolation {
        description: "fixture poison".to_owned(),
        location: "persistence::v2_tests".to_owned(),
    };
    session.poison = Some(fatal.clone());

    assert_eq!(capture_runtime_v2(&session).unwrap_err(), fatal);
}
