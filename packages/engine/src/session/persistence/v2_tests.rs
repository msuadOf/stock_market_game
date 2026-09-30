use super::super::*;
use super::v2::*;
use crate::session::pipeline::{
    transition::{FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents, JournalRank,
    ReceiptLocalKey, ReceiptSource, ReceiptTransition,
};

fn v2_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    GameSession::new(setup, 42).expect("v2 fixture must be valid")
}

fn fee_v2(fees: FeeComponents) -> FeeComponentsV2 {
    FeeComponentsV2 {
        commission: fees.commission,
        stamp_tax: fees.stamp_tax,
        transfer_fee: fees.transfer_fee,
    }
}

#[test]
fn restore_rejects_parent_child_that_does_not_match_a_live_order() {
    let mut session = v2_session();
    let code = session.setup.stocks[0].code.clone();
    let institution = AccountId(1);
    let mut events = Vec::new();
    session.seed_order_for_test(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut events,
    );
    let player_order = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture player order must be accepted");
    session
        .parent_orders
        .entry(institution)
        .or_default()
        .insert(
            code.clone(),
            ParentOrderPlan {
                code: code.clone(),
                side: Side::Buy,
                target_qty: 100,
                filled_qty: 0,
                child_qty: 100,
                active_child_order_id: None,
                active_child_remaining_qty: None,
                linked_plan_id: None,
                limit_price: Money::from_cents(1_000),
                expires_market_minute: 480,
            },
        );
    let mut forged = session
        .save()
        .expect("unlinked parent without a child is valid");
    let plan = forged
        .parent_orders
        .get_mut(&institution)
        .unwrap()
        .get_mut(&code)
        .unwrap();
    plan.active_child_order_id = Some(player_order);

    assert!(matches!(
        GameSession::restore(&forged),
        Err(SessionError::InvalidSave(reason)) if reason.contains("active child does not match a live order")
    ));
}

fn low_price_v2_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.commission_min = Money::ZERO;
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    GameSession::new(setup, 42).expect("low-price v2 fixture must be valid")
}

fn gross_capped_v2_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    GameSession::new(setup, 42).expect("gross-capped v2 fixture must be valid")
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

fn install_gross_capped_sell(session: &mut GameSession) -> EnvelopeKey {
    let code = session.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    let filled_value = Money::from_cents(100);
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
            original_qty: 101,
            filled_qty: 100,
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
    assert!(nominal.commission > filled_value);
    session.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            key.clone(),
            Money::ZERO,
            1,
            EnvelopeAudit {
                limit: Money::from_cents(1),
                remaining_qty: 1,
                filled_qty: 100,
                filled_value,
                nominal,
                charged: FeeComponents {
                    commission: filled_value,
                    stamp_tax: Money::ZERO,
                    transfer_fee: Money::ZERO,
                },
            },
        )],
    )
    .expect("fixture capped-fee ledger must be valid");
    key
}

fn install_unfilled_gross_capped_sell(session: &mut GameSession) -> EnvelopeKey {
    let code = session.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    session
        .accounts
        .get_mut(&key.account)
        .expect("fixture seller must exist")
        .grant_position(code.clone(), 2, Money::from_cents(1))
        .expect("fixture historical holding must be valid");
    let result = session
        .markets
        .get_mut(&code)
        .expect("fixture market must exist")
        .place(Order {
            id: key.order,
            side: key.side,
            price: Money::from_cents(1),
            qty: 2,
            original_qty: 2,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: key.account,
            seq: 1,
        })
        .expect("fixture sell must rest");
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    session.next_order_id = 2;
    session
        .hydrate_or_validate_envelope_ledger()
        .expect("fixture ledger must hydrate");
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

fn restore_error(save: &SaveSlot) -> SessionError {
    match GameSession::restore(save) {
        Ok(_) => panic!("corrupt save must be rejected"),
        Err(error) => error,
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
fn new_session_rejects_legacy_and_unknown_simulation_policies() {
    for policy in ["a-share-simulation-v1", "a-share-simulation-unknown"] {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.simulation_policy_id = policy.to_owned();
        let error = match GameSession::new(setup, 42) {
            Ok(_) => panic!("new session must reject unsupported policy {policy:?}"),
            Err(error) => error,
        };
        match error {
            SessionError::InvalidSetup(description) => assert!(
                description.contains(SIMULATION_POLICY_ID_V2),
                "policy rejection must name the supported policy: {description}"
            ),
            other => panic!("unsupported policy must be InvalidSetup, got {other}"),
        }
    }
}

#[test]
fn fresh_session_strategy_and_attention_share_exact_canonical_probability() {
    let session = GameSession::new(super::super::npc_working_quote_tests::quote_setup(0), 994)
        .expect("v2 session must be valid");
    let state = capture_runtime_v2(&session).expect("fresh v2 authority must be consistent");
    for (account, strategy) in state.strategy_states {
        assert_eq!(
            strategy.base_observation_probability().to_bits(),
            session.npc_attention[&account].base_probability.to_bits(),
        );
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
    assert_eq!(restored.next_receipt_base, 1);
    assert_eq!(restored.envelope_ledger.next_receipt_index(), 1);
    assert_eq!(capture_runtime_v2(&restored).unwrap(), state);
}

#[test]
fn save_omits_snapshot_and_envelope_mirrors_but_keeps_charged_fees() {
    let mut source = low_price_v2_session();
    install_partially_filled_sell(&mut source);
    let save = source.save().expect("path-dependent fee history must save");
    let encoded = serde_json::to_value(&save).expect("save must encode");
    let snapshot = encoded["snapshot"].as_object().unwrap();
    assert!(!snapshot.contains_key("day"));
    assert!(!snapshot.contains_key("phase"));
    let envelope = encoded["runtime_v2"]["live_envelopes"][0]
        .as_object()
        .unwrap();
    assert_eq!(
        envelope.keys().cloned().collect::<Vec<_>>(),
        ["charged", "key"]
    );
    assert_eq!(
        envelope["charged"]["commission"],
        serde_json::json!(source
            .setup
            .config
            .commission(Money::from_cents(50_001))
            .unwrap())
    );
    let restored = GameSession::restore(&save).expect("tick/setup must reconstruct time state");
    assert_eq!(restored.day(), source.day());
    assert_eq!(restored.phase(), source.phase());
}

#[test]
fn parent_plan_save_omits_rebuilt_child_quantity_and_rejects_injected_legacy_field() {
    let plan = crate::session::SaveParentOrderPlan {
        code: StockCode("600888".to_owned()),
        side: Side::Buy,
        target_qty: 200,
        filled_qty: 0,
        child_qty: 100,
        active_child_order_id: Some(OrderId(1)),
        linked_plan_id: None,
        limit_price: Money::from_cents(1_000),
        expires_market_minute: 500,
    };
    let mut encoded = serde_json::to_value(&plan).expect("parent plan must encode");
    assert!(!encoded
        .as_object()
        .unwrap()
        .contains_key("active_child_remaining_qty"));
    encoded["active_child_remaining_qty"] = serde_json::json!(99);
    assert!(serde_json::from_value::<crate::session::SaveParentOrderPlan>(encoded).is_err());
}

#[test]
fn runtime_envelope_rejects_injected_derived_fields() {
    for field in [
        "live",
        "limit",
        "remaining_qty",
        "filled_qty",
        "filled_value",
        "nominal",
    ] {
        let mut encoded = serde_json::json!({
            "key": { "account": 1, "stock": "600888", "order": 1, "side": "Sell" },
            "charged": { "commission": 0, "stamp_tax": 0, "transfer_fee": 0 }
        });
        encoded[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<LiveEnvelopeV2>(encoded).is_err(),
            "legacy derived field {field} must be rejected"
        );
    }
}

#[test]
fn partial_parent_child_quantity_is_rebuilt_without_losing_fill_or_fee_history() {
    let mut source = low_price_v2_session();
    let key = install_partially_filled_sell(&mut source);
    source.parent_orders.entry(key.account).or_default().insert(
        key.stock.clone(),
        ParentOrderPlan {
            code: key.stock.clone(),
            side: key.side,
            target_qty: 50_100,
            filled_qty: 50_001,
            child_qty: 50_100,
            active_child_order_id: Some(key.order),
            active_child_remaining_qty: Some(1),
            linked_plan_id: None,
            limit_price: Money::from_cents(1),
            expires_market_minute: 480,
        },
    );
    let saved = source.save().expect("partially filled parent must save");
    let encoded = serde_json::to_value(&saved).unwrap();
    assert!(
        encoded["parent_orders"][key.account.0.to_string()][&key.stock.0]
            .get("active_child_remaining_qty")
            .is_none()
    );
    let restored = GameSession::restore(&saved).expect("parent must rebuild from its live child");
    let parent = &restored.parent_orders[&key.account][&key.stock];
    assert_eq!(parent.active_child_remaining_qty, Some(1));
    assert_eq!(parent.target_qty, 50_100);
    assert_eq!(parent.filled_qty, 50_001);
    assert_eq!(
        restored.markets[&key.stock].resting_orders_for(key.account)[0].qty,
        1
    );
    assert_eq!(
        restored.envelope_ledger.get(&key).unwrap().audit(),
        source.envelope_ledger.get(&key).unwrap().audit()
    );
}

#[test]
fn seller_cumulative_nominal_and_charged_audit_survives_restore() {
    let mut source = low_price_v2_session();
    install_partially_filled_sell(&mut source);
    let state = capture_runtime_v2(&source).expect("seller fee debt must be persistable");
    let audit = source.project_live_envelopes().unwrap()[0].audit();
    let charged = state.live_envelopes[0].charged;

    assert_eq!(audit.filled_qty, 50_001);
    assert_eq!(audit.filled_value, Money::from_cents(50_001));
    assert_eq!(audit.nominal.commission, Money::from_cents(13));
    assert_eq!(audit.nominal.stamp_tax, Money::from_cents(25));
    assert_eq!(audit.nominal.transfer_fee, Money::from_cents(1));
    assert_eq!(charged.commission, Money::from_cents(13));
    assert_eq!(charged.stamp_tax, Money::from_cents(25));
    assert_eq!(charged.transfer_fee, Money::ZERO);

    let mut restored = low_price_v2_session();
    install_partially_filled_sell(&mut restored);
    restore_runtime_v2(&mut restored, &state).expect("seller fee debt must restore");
    assert_eq!(capture_runtime_v2(&restored).unwrap(), state);
    restored
        .step()
        .expect("a restored path-dependent seller fee debt must survive the next tick");
}

#[test]
fn per_fill_priority_history_survives_restore_and_next_tick() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.commission_rate = 0.0005;
    setup.config.commission_min = Money::ZERO;
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    let mut source = GameSession::new(setup, 43).expect("fee-history fixture must be valid");
    let code = source.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    let before_value = Money::from_cents(50_999);
    let nominal_before = FeeComponents {
        commission: source.setup.config.commission(before_value).unwrap(),
        stamp_tax: source.setup.config.stamp_tax(before_value).unwrap(),
        transfer_fee: source.setup.config.transfer_fee(before_value).unwrap(),
    };
    let transition = FillTransition::sell(SellFillInput {
        config: &source.setup.config,
        fill_qty: 1,
        remaining_qty_after: 1,
        filled_value_before: before_value,
        gross_delta: Money::from_cents(1),
        nominal_before,
        charged_before: nominal_before,
    })
    .expect("one-cent leg must use the real seller transition");
    assert_eq!(transition.nominal_after.commission, Money::from_cents(26));
    assert_eq!(transition.nominal_after.stamp_tax, Money::from_cents(26));
    assert_eq!(transition.nominal_after.transfer_fee, Money::from_cents(1));
    assert_eq!(transition.charged_after.commission, Money::from_cents(26));
    assert_eq!(transition.charged_after.stamp_tax, Money::from_cents(25));
    assert_eq!(transition.charged_after.transfer_fee, Money::from_cents(1));

    source
        .accounts
        .get_mut(&key.account)
        .unwrap()
        .grant_position(code.clone(), 1, Money::from_cents(1))
        .unwrap();
    source
        .markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: key.order,
            side: key.side,
            price: Money::from_cents(1),
            qty: 1,
            original_qty: 51_001,
            filled_qty: 51_000,
            filled_value: Money::from_cents(51_000),
            owner: key.account,
            seq: 1,
        })
        .unwrap();
    source.next_order_id = 2;
    source.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            key,
            Money::ZERO,
            1,
            EnvelopeAudit {
                limit: Money::from_cents(1),
                remaining_qty: 1,
                filled_qty: 51_000,
                filled_value: Money::from_cents(51_000),
                nominal: transition.nominal_after,
                charged: transition.charged_after,
            },
        )],
    )
    .unwrap();

    let save = source.save().expect("path-dependent fee history must save");
    let mut restored =
        GameSession::restore(&save).expect("path-dependent fee history must restore");
    restored
        .step()
        .expect("path-dependent fee history must survive the next public tick");
}

#[test]
fn complete_restore_accepts_zero_cash_seller_with_v2_live_envelope() {
    let mut source = gross_capped_v2_session();
    let key = install_unfilled_gross_capped_sell(&mut source);
    let mut save = source.save().expect("valid v2 seller debt must save");
    assert_eq!(
        save.runtime_v2.live_envelopes[0].charged,
        FeeComponentsV2::default(),
        "an unfilled seller has no actual charges",
    );
    save.snapshot
        .accounts
        .get_mut(&key.account)
        .expect("saved seller must exist")
        .cash = Money::ZERO;

    let restored = GameSession::restore(&save)
        .expect("v2 seller envelope reserves shares but never requires seller cash");
    assert_eq!(
        restored
            .save()
            .expect("restored zero-cash seller must remain saveable")
            .runtime_v2
            .live_envelopes[0]
            .charged,
        FeeComponentsV2::default(),
    );
}

#[test]
fn complete_restore_rejects_live_sell_when_edited_assets_cannot_cover_it() {
    let mut source = gross_capped_v2_session();
    let key = install_unfilled_gross_capped_sell(&mut source);
    let save = source.save().expect("valid v2 seller must save");

    let mut assets_tampered = save;
    assets_tampered
        .snapshot
        .accounts
        .get_mut(&key.account)
        .expect("saved seller must exist")
        .positions
        .get_mut(&key.stock)
        .expect("saved seller must hold the sold stock")
        .qty = 1;
    assert_invalid_save(restore_error(&assets_tampered), "over-reserve shares");
}

#[test]
fn real_step_settles_and_persists_gross_capped_seller_without_cash_reservation() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.starting_cash = Money::from_cents(10_000);
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 42).expect("v2 fixture must be valid");
    session
        .accounts
        .get_mut(&AccountId(0))
        .expect("player must exist")
        .grant_position(code.clone(), 200, Money::from_cents(1))
        .expect("fixture holding must be valid");

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1)),
                qty: 100,
            },
        )
        .unwrap();
    session.step().expect("buyer order tick must commit");

    let cash_before_fill = session.accounts[&AccountId(0)].cash;
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1)),
                qty: 200,
            },
        )
        .unwrap();
    session.step().expect("partial-fill tick must commit");
    let gross = Money::from_cents(100);
    let expected_buy_cost = gross
        .add(session.setup.config.commission(gross).unwrap())
        .unwrap()
        .add(session.setup.config.transfer_fee(gross).unwrap())
        .unwrap();
    assert_eq!(
        session.accounts[&AccountId(0)].cash,
        cash_before_fill.sub(expected_buy_cost).unwrap(),
        "seller receives gross minus the capped charge, which is zero for this one-yuan leg",
    );

    let partial = session
        .save()
        .expect("real partial-fill quiet point must save");
    assert_eq!(
        session.snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO,
    );
    let seller = partial
        .runtime_v2
        .live_envelopes
        .iter()
        .find(|envelope| envelope.key.side == Side::Sell)
        .expect("partially-filled seller envelope must remain live");
    assert_eq!(seller.charged.commission, gross);
    let audit = session.project_live_envelopes().unwrap()[0].audit();
    assert_eq!(audit.filled_value, gross);
    assert!(audit.nominal.commission > seller.charged.commission);

    let mut restored = GameSession::restore(&partial).expect("real partial-fill save must restore");
    for candidate in [&mut session, &mut restored] {
        candidate
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: LimitPrice::Fixed(Money::from_cents(1)),
                    qty: 100,
                },
            )
            .unwrap();
    }
    let uninterrupted_events = session
        .step()
        .expect("uninterrupted continuation must commit");
    let restored_events = restored.step().expect("restored continuation must commit");
    assert_eq!(
        serde_json::to_vec(&uninterrupted_events).unwrap(),
        serde_json::to_vec(&restored_events).unwrap(),
    );
    assert_eq!(
        serde_json::to_vec(&session.save().expect("uninterrupted save")).unwrap(),
        serde_json::to_vec(&restored.save().expect("restored save")).unwrap(),
    );
}

#[test]
fn same_tick_routes_advance_one_seller_fee_debt_without_reusing_tick_start_audit() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.starting_cash = Money::from_cents(20_000);
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 43).expect("v2 fixture must be valid");
    session
        .accounts
        .get_mut(&AccountId(0))
        .expect("player must exist")
        .grant_position(code.clone(), 1_300, Money::from_cents(1))
        .expect("fixture holding must be valid");

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1)),
                qty: 1_300,
            },
        )
        .unwrap();
    session.step().expect("resting seller tick must commit");

    for qty in [100, 1_000] {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: LimitPrice::Fixed(Money::from_cents(1)),
                    qty,
                },
            )
            .unwrap();
    }
    let events = session
        .step()
        .expect("both incoming routes must share one cumulative seller audit");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Trade { .. }))
            .count(),
        2,
    );
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::SettlementError { .. })));

    let save = session
        .save()
        .expect("same-tick debt recovery must end at a save quiet point");
    let seller = save
        .runtime_v2
        .live_envelopes
        .iter()
        .find(|envelope| envelope.key.side == Side::Sell)
        .expect("the seller must remain partially live");
    let first_gross = Money::from_cents(100);
    let first_nominal = session.setup.config.commission(first_gross).unwrap();
    assert!(
        first_nominal > first_gross,
        "the first leg must create minimum-commission debt",
    );
    assert_eq!(
        seller.charged,
        fee_v2(session.project_live_envelopes().unwrap()[0].audit().nominal),
        "the larger second route must recover the first route's unpaid nominal fee",
    );
    assert_eq!(
        session.project_live_envelopes().unwrap()[0]
            .audit()
            .remaining_qty,
        200
    );
}

#[test]
fn same_tick_new_seller_route_creates_and_advances_cumulative_fee_audit() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.starting_cash = Money::from_cents(20_000);
    setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 44).expect("v2 fixture must be valid");
    session
        .accounts
        .get_mut(&AccountId(0))
        .expect("player must exist")
        .grant_position(code.clone(), 1_300, Money::from_cents(1))
        .expect("fixture holding must be valid");

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1)),
                qty: 1_300,
            },
        )
        .unwrap();
    for qty in [100, 1_000] {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: LimitPrice::Fixed(Money::from_cents(1)),
                    qty,
                },
            )
            .unwrap();
    }

    let events = session
        .step()
        .expect("new seller and both incoming routes must commit in one tick");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Trade { .. }))
            .count(),
        2,
    );
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::SettlementError { .. })));

    let save = session
        .save()
        .expect("same-tick new seller audit must end at a save quiet point");
    let seller = save
        .runtime_v2
        .live_envelopes
        .iter()
        .find(|envelope| envelope.key.side == Side::Sell)
        .expect("the new seller must remain partially live");
    assert_eq!(
        seller.charged,
        fee_v2(session.project_live_envelopes().unwrap()[0].audit().nominal)
    );
    assert_eq!(
        session.project_live_envelopes().unwrap()[0]
            .audit()
            .remaining_qty,
        200
    );
}

#[test]
fn negative_charged_fee_component_is_rejected_as_corrupt_save() {
    let mut session = low_price_v2_session();
    install_partially_filled_sell(&mut session);
    let mut state = capture_runtime_v2(&session).unwrap();
    state.live_envelopes[0].charged.transfer_fee = Money::from_cents(-1);

    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "negative component",
    );
}

#[test]
fn charged_fee_component_above_nominal_is_rejected_as_corrupt_save() {
    let mut session = low_price_v2_session();
    install_partially_filled_sell(&mut session);
    let mut state = capture_runtime_v2(&session).unwrap();
    state.live_envelopes[0].charged.transfer_fee = Money::from_cents(2);

    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "inconsistent cumulative charged fee audit",
    );
}

#[test]
fn cumulative_charged_components_do_not_reconstruct_per_fill_priority() {
    let mut session = low_price_v2_session();
    install_partially_filled_sell(&mut session);
    let mut state = capture_runtime_v2(&session).unwrap();
    let charged = &mut state.live_envelopes[0].charged;
    charged.commission = Money::from_cents(12);
    charged.transfer_fee = Money::from_cents(1);

    validate_runtime_v2(&session, &state)
        .expect("cumulative fee components cannot reveal historical per-fill priority");
}

#[test]
fn charged_fee_total_above_gross_is_rejected_when_components_are_within_nominal() {
    let mut session = gross_capped_v2_session();
    install_gross_capped_sell(&mut session);
    let mut state = capture_runtime_v2(&session).unwrap();
    state.live_envelopes[0].charged.commission = Money::from_cents(101);

    assert_invalid_save(
        validate_runtime_v2(&session, &state).unwrap_err(),
        "inconsistent cumulative charged fee audit",
    );
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
    state.live_envelopes[0].charged.commission = Money::from_cents(1);

    let mut target = v2_session();
    install_live_buy(&mut target);
    let before = capture_runtime_v2(&target).unwrap();
    assert_invalid_save(
        restore_runtime_v2(&mut target, &state).unwrap_err(),
        "inconsistent cumulative charged fee audit",
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
fn decode_rejects_unknown_nested_strategy_state_fields() {
    let session = v2_session();
    let mut encoded = serde_json::to_value(session.save().expect("healthy v2 save")).unwrap();
    let states = encoded["runtime_v2"]["strategy_states"]
        .as_object_mut()
        .expect("strategy states must encode as an object");
    let state = states
        .values_mut()
        .next()
        .expect("fixture must contain a production strategy")
        .as_object_mut()
        .expect("StrategyState must encode as a tagged object");
    let payload = state
        .values_mut()
        .next()
        .expect("StrategyState must contain one payload")
        .as_object_mut()
        .expect("strategy payload must encode as an object");
    payload.insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    let bytes = serde_json::to_vec(&encoded).unwrap();

    let error = decode_save_slot(&bytes, &SaveDecodeLimits::default())
        .expect_err("unknown nested strategy state fields must fail closed");
    assert_invalid_save(error, "unknown field");
}

#[test]
fn decode_rejects_noncanonical_strategy_float_bit_strings() {
    let session = v2_session();
    let pristine = serde_json::to_value(session.save().expect("healthy v2 save")).unwrap();
    for malformed in ["0", "3FF0000000000000"] {
        let mut encoded = pristine.clone();
        let states = encoded["runtime_v2"]["strategy_states"]
            .as_object_mut()
            .expect("strategy states must encode as an object");
        let payload = states
            .values_mut()
            .next()
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|state| state.values_mut().next())
            .and_then(serde_json::Value::as_object_mut)
            .expect("fixture strategy must encode an object payload");
        payload.insert(
            "base_observation_probability".to_owned(),
            serde_json::Value::String(malformed.to_owned()),
        );
        let bytes = serde_json::to_vec(&encoded).unwrap();

        let error = decode_save_slot(&bytes, &SaveDecodeLimits::default())
            .expect_err("noncanonical exact-float strings must fail closed");
        assert_invalid_save(error, "16 lowercase hexadecimal digits");
    }
}

#[test]
fn decode_rejects_strategy_integers_outside_javascript_safe_range() {
    let mut hot_setup = super::super::npc_working_quote_tests::quote_setup(0);
    hot_setup.simulation_policy_id = SIMULATION_POLICY_ID_V2.to_owned();
    hot_setup.npcs.inst_count = 0;
    hot_setup.npcs.hot_count = 1;
    let hot_session = GameSession::new(hot_setup, 42).expect("hot v2 fixture must be valid");
    let mut save_json =
        serde_json::to_value(hot_session.save().expect("healthy hot save")).unwrap();
    let saved_strategy = save_json["runtime_v2"]["strategy_states"]
        .as_object_mut()
        .and_then(|states| states.values_mut().next())
        .and_then(|state| state.get_mut("Momentum"))
        .and_then(serde_json::Value::as_object_mut)
        .expect("hot fixture must contain Momentum state");
    saved_strategy.insert(
        "lookback".to_owned(),
        serde_json::Value::Number(9_007_199_254_740_992_u64.into()),
    );

    let hot = crate::strategy::StrategyState::Momentum(
        crate::MomentumStrategy::new(2, 0.02, 100).unwrap(),
    );
    let mut hot_json = serde_json::to_value(hot).unwrap();
    hot_json["Momentum"]["lookback"] = serde_json::Value::Number(9_007_199_254_740_991_u64.into());
    serde_json::from_value::<crate::strategy::StrategyState>(hot_json.clone())
        .expect("maximum JavaScript-safe strategy integer must decode");
    hot_json["Momentum"]["lookback"] = serde_json::Value::Number(9_007_199_254_740_992_u64.into());

    let save_error = decode_save_slot(
        &serde_json::to_vec(&save_json).unwrap(),
        &SaveDecodeLimits::default(),
    )
    .expect_err("unsafe Momentum integer in a save must be rejected");
    assert_invalid_save(save_error, "JavaScript safe integer range");

    let state_error = serde_json::from_slice::<crate::strategy::StrategyState>(
        &serde_json::to_vec(&hot_json).unwrap(),
    )
    .expect_err("unsafe Momentum integer must be rejected");
    assert!(
        state_error
            .to_string()
            .contains("JavaScript safe integer range"),
        "unexpected unsafe-integer error: {state_error}"
    );
}

#[test]
fn restore_rejects_strategy_attention_probability_drift() {
    let session = v2_session();
    let mut encoded = serde_json::to_value(session.save().expect("healthy v2 save")).unwrap();
    let states = encoded["runtime_v2"]["strategy_states"]
        .as_object_mut()
        .expect("strategy states must encode as an object");
    let payload = states
        .values_mut()
        .next()
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|state| state.values_mut().next())
        .and_then(serde_json::Value::as_object_mut)
        .expect("fixture strategy must encode an object payload");
    payload.insert(
        "base_observation_probability".to_owned(),
        serde_json::Value::String("3fe0000000000000".to_owned()),
    );
    let bytes = serde_json::to_vec(&encoded).unwrap();
    let decoded = decode_save_slot(&bytes, &SaveDecodeLimits::default())
        .expect("canonical same-profile StrategyState must decode");

    let error = restore_error(&decoded);
    assert_invalid_save(error, "disagrees with NPC attention probability");
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
