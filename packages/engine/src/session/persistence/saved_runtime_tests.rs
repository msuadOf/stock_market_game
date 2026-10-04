use super::super::*;
use super::saved_runtime::*;
use crate::session::pipeline::{
    transition::{FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents, JournalRank,
    ReceiptLocalKey, ReceiptSource, ReceiptTransition,
};

fn session_with_runtime_state() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    GameSession::new(setup, 42).expect("runtime_state fixture 必须有效")
}

#[test]
fn restore_keeps_cognitive_profiles_independent_from_execution_styles() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 1;
    let mut session = GameSession::new(setup, 42).expect("mixed NPC fixture must initialize");
    let retail = AccountId(1);
    let institution = AccountId(2);

    let retail_execution = session.state.accounts[&retail]
        .strategy()
        .expect("retail execution strategy exists")
        .profile();
    let retail_cognition = match retail_execution {
        crate::strategy::StrategyProfile::Retail(crate::strategy::RetailStyle::LongTerm) => {
            crate::strategy::RetailStyle::Noise
        }
        _ => crate::strategy::RetailStyle::LongTerm,
    };
    let retail_cognition = crate::strategy::StrategyProfile::Retail(retail_cognition);
    let mut retail_rng = crate::session::SplitMix64::new(1201);
    let retail_analysis =
        crate::strategy::derive_analysis_profile(&retail_cognition, retail, &mut retail_rng)
            .expect("retail cognition profile must derive");
    *session
        .state
        .belief_participants
        .get_mut(&retail)
        .expect("retail participant exists")
        .belief_mut() = crate::strategy::BeliefBook::new(
        retail,
        retail_cognition.clone(),
        retail_analysis,
        &mut retail_rng,
    );

    assert_eq!(
        session.state.accounts[&institution]
            .strategy()
            .expect("institution execution strategy exists")
            .institution_style(),
        Some(crate::strategy::InstitutionStyle::DeepValue)
    );
    let institution_cognition =
        crate::strategy::StrategyProfile::Institution(crate::strategy::InstitutionStyle::Growth);
    let mut institution_rng = crate::session::SplitMix64::new(1202);
    let institution_analysis = crate::strategy::derive_analysis_profile(
        &institution_cognition,
        institution,
        &mut institution_rng,
    )
    .expect("institution cognition profile must derive");
    *session
        .state
        .belief_participants
        .get_mut(&institution)
        .expect("institution participant exists")
        .belief_mut() = crate::strategy::BeliefBook::new(
        institution,
        institution_cognition.clone(),
        institution_analysis,
        &mut institution_rng,
    );

    let mut restored = GameSession::restore(&session.save().expect("separated profiles save"))
        .expect("cognition and execution styles may differ within account kind");
    assert_eq!(
        restored.state.belief_participants[&institution]
            .belief()
            .profile(),
        &institution_cognition
    );
    assert_eq!(
        restored.state.accounts[&institution]
            .strategy()
            .unwrap()
            .profile(),
        crate::strategy::StrategyProfile::Institution(crate::strategy::InstitutionStyle::DeepValue)
    );
    assert_eq!(
        restored.state.belief_participants[&retail]
            .belief()
            .profile(),
        &retail_cognition
    );
    assert!(restored.state.belief_participants[&retail]
        .belief()
        .analysis()
        .fundamental_method()
        .is_some());
    let market = restored.build_market_view();
    let code = market.stocks.keys().next().unwrap().clone();
    let assessments = restored
        .capture_retail_analysis(&[retail], &market)
        .expect("explicit retail analysis must consume its cognition profile");
    assert!(assessments[&retail].contains_key(&code));
    restored
        .step()
        .expect("separated profiles must support a tick");

    let resaved = restored.save().expect("separated profiles resave");
    let rebuilt = GameSession::restore(&resaved).expect("resaved profiles restore");
    assert_eq!(
        rebuilt.state.belief_participants[&institution]
            .belief()
            .profile(),
        &institution_cognition
    );
    assert_eq!(
        resaved.runtime_state.strategy_states[&institution].profile(),
        crate::strategy::StrategyProfile::Institution(crate::strategy::InstitutionStyle::DeepValue)
    );
    assert_eq!(
        rebuilt.state.belief_participants[&retail]
            .belief()
            .profile(),
        &retail_cognition
    );

    let mut wrong_owner = serde_json::to_value(&resaved).expect("save serializes");
    let institution_key = institution.0.to_string();
    wrong_owner["belief_books"][institution_key]["npc"] = serde_json::json!(retail.0);
    let wrong_owner: SaveSlot = serde_json::from_value(wrong_owner).expect("typed edited save");
    assert!(matches!(
        GameSession::restore(&wrong_owner),
        Err(SessionError::InvalidSave(message)) if message.contains("belief book owner")
    ));

    let mut wrong_kind = serde_json::to_value(&resaved).expect("save serializes");
    wrong_kind["belief_books"][institution.0.to_string()]["profile"] = serde_json::to_value(
        crate::strategy::StrategyProfile::Retail(crate::strategy::RetailStyle::LongTerm),
    )
    .expect("retail profile serializes");
    let wrong_kind: SaveSlot = serde_json::from_value(wrong_kind).expect("typed edited save");
    assert!(matches!(
        GameSession::restore(&wrong_kind),
        Err(SessionError::InvalidSave(message)) if message.contains("belief profile conflicts with account kind")
    ));
}

fn saved_fee_components(fees: FeeComponents) -> SavedFeeComponents {
    SavedFeeComponents {
        commission: fees.commission,
        stamp_tax: fees.stamp_tax,
        transfer_fee: fees.transfer_fee,
    }
}

#[test]
fn restore_rejects_parent_child_that_does_not_match_a_live_order() {
    let mut session = session_with_runtime_state();
    let code = session.state.setup.stocks[0].code.clone();
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
        .state
        .parent_orders
        .entry(institution)
        .or_default()
        .insert(
            code.clone(),
            ParentOrderPlan::from_saved_facts(
                code.clone(),
                Side::Buy,
                100,
                0,
                100,
                None,
                None,
                None,
                Money::from_cents(1_000),
                480,
            ),
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

fn low_price_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.commission_min = Money::ZERO;
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    GameSession::new(setup, 42).expect("low-price runtime_state fixture 必须有效")
}

fn gross_capped_fee_session() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    GameSession::new(setup, 42).expect("gross-capped runtime_state fixture 必须有效")
}

fn install_live_buy(session: &mut GameSession) -> EnvelopeKey {
    let code = session.state.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Buy,
    };
    let result = session
        .state
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
    session.state.next_order_id = 2;
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
        .state
        .retail_projection_seen
        .insert_test_identity(0, local_key);
    session.state.next_receipt_base = 1;
    session.state.envelope_ledger = EnvelopeLedger::new(
        session.state.next_receipt_base,
        session
            .project_live_envelopes()
            .expect("fixture live envelope projection must succeed"),
    )
    .expect("fixture ledger cursor must be valid");
}

fn install_partially_filled_sell(session: &mut GameSession) -> EnvelopeKey {
    let code = session.state.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    let filled_value = Money::from_cents(50_001);
    session
        .state
        .accounts
        .get_mut(&key.account)
        .expect("fixture seller must exist")
        .grant_position(code.clone(), 1, Money::from_cents(1))
        .expect("fixture historical holding must be valid");
    let result = session
        .state
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
    session.state.next_order_id = 2;
    let nominal = FeeComponents {
        commission: session.state.setup.config.commission(filled_value).unwrap(),
        stamp_tax: session.state.setup.config.stamp_tax(filled_value).unwrap(),
        transfer_fee: session
            .state
            .setup
            .config
            .transfer_fee(filled_value)
            .unwrap(),
    };
    let charged = FeeComponents {
        commission: Money::from_cents(13),
        stamp_tax: Money::from_cents(25),
        transfer_fee: Money::ZERO,
    };
    session.state.envelope_ledger = EnvelopeLedger::new(
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
    let code = session.state.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    let filled_value = Money::from_cents(100);
    session
        .state
        .accounts
        .get_mut(&key.account)
        .expect("fixture seller must exist")
        .grant_position(code.clone(), 1, Money::from_cents(1))
        .expect("fixture historical holding must be valid");
    let result = session
        .state
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
    session.state.next_order_id = 2;
    let nominal = FeeComponents {
        commission: session.state.setup.config.commission(filled_value).unwrap(),
        stamp_tax: session.state.setup.config.stamp_tax(filled_value).unwrap(),
        transfer_fee: session
            .state
            .setup
            .config
            .transfer_fee(filled_value)
            .unwrap(),
    };
    assert!(nominal.commission > filled_value);
    session.state.envelope_ledger = EnvelopeLedger::new(
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
    let code = session.state.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    session
        .state
        .accounts
        .get_mut(&key.account)
        .expect("fixture seller must exist")
        .grant_position(code.clone(), 2, Money::from_cents(1))
        .expect("fixture historical holding must be valid");
    let result = session
        .state
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
    session.state.next_order_id = 2;
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
fn save_decoder_accepts_current_structure_and_rejects_version_markers() {
    let session = session_with_runtime_state();
    let mut value = serde_json::to_value(session.save().unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("schema_version");
    let json = serde_json::to_vec(&value).unwrap();
    decode_save_slot(&json, &SaveDecodeLimits::default()).unwrap();
    for marker in [
        serde_json::json!(1),
        serde_json::json!(2),
        serde_json::json!(3),
        serde_json::json!(4),
        serde_json::Value::Null,
        serde_json::json!("3"),
    ] {
        value
            .as_object_mut()
            .unwrap()
            .insert("schema_version".to_owned(), marker);
        let json = serde_json::to_vec(&value).unwrap();
        assert_invalid_save(
            decode_save_slot(&json, &SaveDecodeLimits::default()).unwrap_err(),
            "schema_version",
        );
    }
}

#[test]
fn new_session_rejects_legacy_and_unknown_simulation_policies() {
    for policy in [
        "a-share-simulation-v1",
        "a-share-simulation-v2",
        "a-share-simulation-unknown",
    ] {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.simulation_policy_id = policy.to_owned();
        let error = match GameSession::new(setup, 42) {
            Ok(_) => panic!("new session must reject unsupported policy {policy:?}"),
            Err(error) => error,
        };
        match error {
            SessionError::InvalidSetup(description) => assert!(
                description.contains(SIMULATION_POLICY_ID),
                "policy rejection must name the supported policy: {description}"
            ),
            other => panic!("unsupported policy must be InvalidSetup, got {other}"),
        }
    }
}

#[test]
fn fresh_session_strategy_and_attention_share_exact_canonical_probability() {
    let session = GameSession::new(super::super::npc_working_quote_tests::quote_setup(0), 994)
        .expect("runtime_state session 必须有效");
    let state = capture_runtime_state(&session).expect("新局 runtime_state 权威状态必须一致");
    for (account, strategy) in state.strategy_states {
        assert_eq!(
            strategy.base_observation_probability().to_bits(),
            session.state.npc_attention[&account]
                .base_probability
                .to_bits(),
        );
    }
}

#[test]
fn healthy_initial_quiet_point_roundtrips_complete_strategy_state() {
    let source = session_with_runtime_state();
    let state = capture_runtime_state(&source).expect("healthy quiet point must capture");

    assert!(!state.poisoned);
    assert_eq!(state.next_receipt_base, 0);
    assert!(state.live_envelopes.is_empty());
    assert!(state.retail_projection_seen.is_empty());
    assert_eq!(state.strategy_states.len(), 1);

    let mut restored = session_with_runtime_state();
    restore_runtime_state(&mut restored, &state).expect("有效 runtime_state 必须可恢复");
    assert_eq!(capture_runtime_state(&restored).unwrap(), state);
}

#[test]
fn live_envelope_and_receipt_prefix_roundtrip_losslessly() {
    let mut source = session_with_runtime_state();
    let envelope = install_live_buy(&mut source);
    install_receipt_prefix(&mut source, envelope);
    let state = capture_runtime_state(&source).expect("non-empty quiet point must capture");

    assert_eq!(state.live_envelopes.len(), 1);
    assert_eq!(state.retail_projection_seen.len(), 1);
    assert_eq!(state.retail_projection_seen[0].index, 0);
    assert_eq!(state.next_receipt_base, 1);

    let mut restored = session_with_runtime_state();
    install_live_buy(&mut restored);
    restore_runtime_state(&mut restored, &state).expect("非空 runtime_state 必须可恢复");
    assert_eq!(restored.state.next_receipt_base, 1);
    assert_eq!(restored.state.envelope_ledger.next_receipt_index(), 1);
    assert_eq!(capture_runtime_state(&restored).unwrap(), state);
}

#[test]
fn save_omits_snapshot_and_envelope_mirrors_but_keeps_charged_fees() {
    let mut source = low_price_session();
    install_partially_filled_sell(&mut source);
    let save = source.save().expect("path-dependent fee history must save");
    let encoded = serde_json::to_value(&save).expect("save must encode");
    let snapshot = encoded["snapshot"].as_object().unwrap();
    assert!(!snapshot.contains_key("day"));
    assert!(!snapshot.contains_key("phase"));
    let envelope = encoded["runtime_state"]["live_envelopes"][0]
        .as_object()
        .unwrap();
    assert_eq!(
        envelope.keys().cloned().collect::<Vec<_>>(),
        ["charged", "key"]
    );
    assert_eq!(
        envelope["charged"]["commission"],
        serde_json::json!(source
            .state
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
            "charged": { "commission": "0", "stamp_tax": "0", "transfer_fee": "0" }
        });
        serde_json::from_value::<SavedLiveEnvelope>(encoded.clone())
            .expect("完整当前 envelope 在注入派生字段前应可解码");
        encoded[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<SavedLiveEnvelope>(encoded).is_err(),
            "legacy derived field {field} must be rejected"
        );
    }
}

#[test]
fn partial_parent_child_quantity_is_rebuilt_without_losing_fill_or_fee_history() {
    let mut source = low_price_session();
    let key = install_partially_filled_sell(&mut source);
    source
        .state
        .parent_orders
        .entry(key.account)
        .or_default()
        .insert(
            key.stock.clone(),
            ParentOrderPlan::from_saved_facts(
                key.stock.clone(),
                key.side,
                50_100,
                50_001,
                50_100,
                Some(key.order),
                Some(1),
                None,
                Money::from_cents(1),
                480,
            ),
        );
    let saved = source.save().expect("partially filled parent must save");
    let encoded = serde_json::to_value(&saved).unwrap();
    assert!(
        encoded["parent_orders"][key.account.0.to_string()][&key.stock.0]
            .get("active_child_remaining_qty")
            .is_none()
    );
    let restored = GameSession::restore(&saved).expect("parent must rebuild from its live child");
    let parent = &restored.state.parent_orders[&key.account][&key.stock];
    assert_eq!(parent.active_child_remaining_qty(), Some(1));
    assert_eq!(parent.target_qty(), 50_100);
    assert_eq!(parent.filled_qty(), 50_001);
    assert_eq!(
        restored.state.markets[&key.stock].resting_orders_for(key.account)[0].qty,
        1
    );
    assert_eq!(
        restored.state.envelope_ledger.get(&key).unwrap().audit(),
        source.state.envelope_ledger.get(&key).unwrap().audit()
    );
}

#[test]
fn seller_cumulative_nominal_and_charged_audit_survives_restore() {
    let mut source = low_price_session();
    install_partially_filled_sell(&mut source);
    let state = capture_runtime_state(&source).expect("seller fee debt must be persistable");
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

    let mut restored = low_price_session();
    install_partially_filled_sell(&mut restored);
    restore_runtime_state(&mut restored, &state).expect("seller fee debt must restore");
    assert_eq!(capture_runtime_state(&restored).unwrap(), state);
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
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    let mut source = GameSession::new(setup, 43).expect("fee-history fixture must be valid");
    let code = source.state.setup.stocks[0].code.clone();
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(1),
        side: Side::Sell,
    };
    let before_value = Money::from_cents(50_999);
    let nominal_before = FeeComponents {
        commission: source.state.setup.config.commission(before_value).unwrap(),
        stamp_tax: source.state.setup.config.stamp_tax(before_value).unwrap(),
        transfer_fee: source
            .state
            .setup
            .config
            .transfer_fee(before_value)
            .unwrap(),
    };
    let transition = FillTransition::sell(SellFillInput {
        config: &source.state.setup.config,
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
        .state
        .accounts
        .get_mut(&key.account)
        .unwrap()
        .grant_position(code.clone(), 1, Money::from_cents(1))
        .unwrap();
    source
        .state
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
    source.state.next_order_id = 2;
    source.state.envelope_ledger = EnvelopeLedger::new(
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
fn complete_restore_accepts_zero_cash_seller_with_live_envelope() {
    let mut source = gross_capped_fee_session();
    let key = install_unfilled_gross_capped_sell(&mut source);
    let mut save = source.save().expect("有效卖方费用欠额必须可存档");
    assert_eq!(
        save.runtime_state.live_envelopes[0].charged,
        SavedFeeComponents::default(),
        "an unfilled seller has no actual charges",
    );
    save.snapshot
        .accounts
        .get_mut(&key.account)
        .expect("saved seller must exist")
        .cash = Money::ZERO;

    let restored = GameSession::restore(&save)
        .expect("卖方 runtime_state envelope 只预留股份，不要求卖方现金");
    assert_eq!(
        restored
            .save()
            .expect("restored zero-cash seller must remain saveable")
            .runtime_state
            .live_envelopes[0]
            .charged,
        SavedFeeComponents::default(),
    );
}

#[test]
fn complete_restore_rejects_live_sell_when_edited_assets_cannot_cover_it() {
    let mut source = gross_capped_fee_session();
    let key = install_unfilled_gross_capped_sell(&mut source);
    let save = source.save().expect("有效卖方 runtime_state 必须可存档");

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
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 42).expect("runtime_state fixture 必须有效");
    session
        .state
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

    let cash_before_fill = session.state.accounts[&AccountId(0)].cash();
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
        .add(session.state.setup.config.commission(gross).unwrap())
        .unwrap()
        .add(session.state.setup.config.transfer_fee(gross).unwrap())
        .unwrap();
    assert_eq!(
        session.state.accounts[&AccountId(0)].cash(),
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
        .runtime_state
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
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 43).expect("runtime_state fixture 必须有效");
    session
        .state
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
        .runtime_state
        .live_envelopes
        .iter()
        .find(|envelope| envelope.key.side == Side::Sell)
        .expect("the seller must remain partially live");
    let first_gross = Money::from_cents(100);
    let first_nominal = session.state.setup.config.commission(first_gross).unwrap();
    assert!(
        first_nominal > first_gross,
        "the first leg must create minimum-commission debt",
    );
    assert_eq!(
        seller.charged,
        saved_fee_components(session.project_live_envelopes().unwrap()[0].audit().nominal),
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
    setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 44).expect("runtime_state fixture 必须有效");
    session
        .state
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
        .runtime_state
        .live_envelopes
        .iter()
        .find(|envelope| envelope.key.side == Side::Sell)
        .expect("the new seller must remain partially live");
    assert_eq!(
        seller.charged,
        saved_fee_components(session.project_live_envelopes().unwrap()[0].audit().nominal)
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
    let mut session = low_price_session();
    install_partially_filled_sell(&mut session);
    let mut state = capture_runtime_state(&session).unwrap();
    state.live_envelopes[0].charged.transfer_fee = Money::from_cents(-1);

    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "negative component",
    );
}

#[test]
fn charged_fee_component_above_nominal_is_rejected_as_corrupt_save() {
    let mut session = low_price_session();
    install_partially_filled_sell(&mut session);
    let mut state = capture_runtime_state(&session).unwrap();
    state.live_envelopes[0].charged.transfer_fee = Money::from_cents(2);

    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "inconsistent cumulative charged fee audit",
    );
}

#[test]
fn cumulative_charged_components_do_not_reconstruct_per_fill_priority() {
    let mut session = low_price_session();
    install_partially_filled_sell(&mut session);
    let mut state = capture_runtime_state(&session).unwrap();
    let charged = &mut state.live_envelopes[0].charged;
    charged.commission = Money::from_cents(12);
    charged.transfer_fee = Money::from_cents(1);

    validate_runtime_state(&session, &state)
        .expect("cumulative fee components cannot reveal historical per-fill priority");
}

#[test]
fn charged_fee_total_above_gross_is_rejected_when_components_are_within_nominal() {
    let mut session = gross_capped_fee_session();
    install_gross_capped_sell(&mut session);
    let mut state = capture_runtime_state(&session).unwrap();
    state.live_envelopes[0].charged.commission = Money::from_cents(101);

    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "inconsistent cumulative charged fee audit",
    );
}

#[test]
fn receipt_gap_is_rejected_as_corrupt_save() {
    let session = session_with_runtime_state();
    let mut state = capture_runtime_state(&session).unwrap();
    state.next_receipt_base = 1;

    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "receipt prefix",
    );
}

#[test]
fn receipt_history_rejects_unknown_or_future_envelope_identity() {
    let mut session = session_with_runtime_state();
    let envelope = install_live_buy(&mut session);
    install_receipt_prefix(&mut session, envelope);
    let state = capture_runtime_state(&session).unwrap();

    let mut unknown_account = state.clone();
    unknown_account.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .account = AccountId(999);
    assert_invalid_save(
        validate_runtime_state(&session, &unknown_account).unwrap_err(),
        "unknown account",
    );

    let mut unknown_stock = state.clone();
    unknown_stock.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .stock = StockCode("600999".to_owned());
    assert_invalid_save(
        validate_runtime_state(&session, &unknown_stock).unwrap_err(),
        "unknown stock",
    );

    let mut zero_order = state.clone();
    zero_order.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .order = OrderId(0);
    assert_invalid_save(
        validate_runtime_state(&session, &zero_order).unwrap_err(),
        "invalid order",
    );

    let mut future_order = state;
    future_order.retail_projection_seen[0]
        .local_key
        .transition
        .envelope
        .order = OrderId(session.state.next_order_id);
    assert_invalid_save(
        validate_runtime_state(&session, &future_order).unwrap_err(),
        "invalid order",
    );
}

#[test]
fn duplicate_live_envelope_key_is_rejected_as_noncanonical() {
    let mut session = session_with_runtime_state();
    install_live_buy(&mut session);
    let mut state = capture_runtime_state(&session).unwrap();
    state.live_envelopes.push(state.live_envelopes[0].clone());

    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "strict canonical key order",
    );
}

#[test]
fn live_envelope_tampering_is_rejected_without_partial_restore() {
    let mut source = session_with_runtime_state();
    install_live_buy(&mut source);
    let mut state = capture_runtime_state(&source).unwrap();
    state.live_envelopes[0].charged.commission = Money::from_cents(1);

    let mut target = session_with_runtime_state();
    install_live_buy(&mut target);
    let before = capture_runtime_state(&target).unwrap();
    assert_invalid_save(
        restore_runtime_state(&mut target, &state).unwrap_err(),
        "inconsistent cumulative charged fee audit",
    );
    assert_eq!(capture_runtime_state(&target).unwrap(), before);
}

#[test]
fn poisoned_marker_and_unknown_runtime_fields_fail_closed() {
    let session = session_with_runtime_state();
    let mut state = capture_runtime_state(&session).unwrap();
    state.poisoned = true;
    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "poisoned",
    );

    let mut encoded = serde_json::to_value(capture_runtime_state(&session).unwrap()).unwrap();
    encoded
        .as_object_mut()
        .expect("runtime DTO must encode as an object")
        .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<SavedRuntimeState>(encoded).is_err());
}

#[test]
fn decode_rejects_unknown_nested_strategy_state_fields() {
    let session = session_with_runtime_state();
    let mut encoded =
        serde_json::to_value(session.save().expect("健康 runtime_state 存档")).unwrap();
    let states = encoded["runtime_state"]["strategy_states"]
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
    let session = session_with_runtime_state();
    let pristine = serde_json::to_value(session.save().expect("健康 runtime_state 存档")).unwrap();
    for malformed in ["0", "3FF0000000000000"] {
        let mut encoded = pristine.clone();
        let states = encoded["runtime_state"]["strategy_states"]
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
    hot_setup.simulation_policy_id = SIMULATION_POLICY_ID.to_owned();
    hot_setup.npcs.inst_count = 0;
    hot_setup.npcs.hot_count = 1;
    let hot_session = GameSession::new(hot_setup, 42).expect("hot runtime_state fixture 必须有效");
    let mut save_json =
        serde_json::to_value(hot_session.save().expect("healthy hot save")).unwrap();
    let saved_strategy = save_json["runtime_state"]["strategy_states"]
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
    let session = session_with_runtime_state();
    let mut encoded =
        serde_json::to_value(session.save().expect("健康 runtime_state 存档")).unwrap();
    let states = encoded["runtime_state"]["strategy_states"]
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
    let session = session_with_runtime_state();
    let mut state = capture_runtime_state(&session).unwrap();
    state.strategy_states.insert(
        AccountId(1),
        crate::strategy::StrategyState::Momentum(
            crate::MomentumStrategy::new(5, 0.02, 100).unwrap(),
        ),
    );

    assert_invalid_save(
        validate_runtime_state(&session, &state).unwrap_err(),
        "deterministic strategy identity",
    );
}

#[test]
fn capture_rejects_poisoned_session_without_emitting_a_dto() {
    let mut session = session_with_runtime_state();
    let fatal = StepFatal::InvariantViolation {
        description: "fixture poison".to_owned(),
        location: "persistence::saved_runtime_tests".to_owned(),
    };
    session.poison = Some(fatal.clone());

    assert_eq!(capture_runtime_state(&session).unwrap_err(), fatal);
}

#[test]
fn cumulative_fee_audit_buyer_requires_every_nominal_component() {
    let config = GameConfig::proposed_defaults();
    let gross = Money::from_cents(100_000);
    let nominal = CumulativeFeeAudit {
        side: Side::Buy,
        filled_value: gross,
        charged: SavedFeeComponents::default(),
    }
    .nominal(&config)
    .unwrap();
    assert!(CumulativeFeeAudit {
        side: Side::Buy,
        filled_value: gross,
        charged: nominal
    }
    .validate(&config)
    .unwrap());
    for charged in [
        SavedFeeComponents {
            commission: nominal.commission.sub(Money::from_cents(1)).unwrap(),
            ..nominal
        },
        SavedFeeComponents {
            transfer_fee: Money::ZERO,
            ..nominal
        },
        SavedFeeComponents {
            stamp_tax: Money::from_cents(1),
            ..nominal
        },
    ] {
        assert!(!CumulativeFeeAudit {
            side: Side::Buy,
            filled_value: gross,
            charged
        }
        .validate(&config)
        .unwrap());
    }
}

#[test]
fn cumulative_fee_audit_zero_and_negative_gross_keep_existing_boundaries() {
    let config = GameConfig::proposed_defaults();
    for side in [Side::Buy, Side::Sell] {
        let zero = CumulativeFeeAudit {
            side,
            filled_value: Money::ZERO,
            charged: SavedFeeComponents::default(),
        };
        assert_eq!(
            zero.nominal(&config).unwrap(),
            SavedFeeComponents::default()
        );
        assert!(zero.validate(&config).unwrap());
        let negative = CumulativeFeeAudit {
            side,
            filled_value: Money::from_cents(-1),
            charged: SavedFeeComponents::default(),
        };
        assert_invalid_save(
            negative.nominal(&config).unwrap_err(),
            "saved filled value cannot be negative",
        );
    }
}

#[test]
fn saved_receipt_source_accepts_quote_expiry_and_rejects_old_tag() {
    let source = SavedReceiptSource::QuoteExpiry(7);
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        serde_json::json!({ "QuoteExpiry": 7 })
    );
    assert!(
        serde_json::from_value::<SavedReceiptSource>(serde_json::json!({ "P0Expiry": 7 })).is_err()
    );
}

#[test]
fn save_slot_uses_current_structure_and_rejects_legacy_runtime_key() {
    let session = session_with_runtime_state();
    let encoded = serde_json::to_value(session.save().unwrap()).unwrap();
    assert!(encoded.get("schema_version").is_none());
    assert!(encoded.get("runtime_state").is_some());
    assert!(encoded.get("runtime_v2").is_none());
    let mut legacy = encoded;
    let runtime = legacy
        .as_object_mut()
        .unwrap()
        .remove("runtime_state")
        .unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .insert("runtime_v2".to_owned(), runtime);
    assert!(serde_json::from_value::<SaveSlot>(legacy).is_err());
}
