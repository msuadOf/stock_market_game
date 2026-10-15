use super::*;

fn expiring_npc_order(
    side: crate::Side,
) -> (
    GameSession,
    crate::StockId,
    crate::AccountId,
    crate::OrderId,
) {
    let code = crate::StockId(0);
    let account = crate::AccountId(1);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.state.attention_scheduler.clear();
    if side == crate::Side::Sell {
        game.fixture_account_mut(&account)
            .unwrap()
            .grant_position(code.clone(), 100, crate::Money::from_cents(100_000))
            .unwrap();
    }
    let mut events = Vec::new();
    game.seed_order_for_test(
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(990)),
            qty: 100,
        },
        &mut events,
    );
    let order_id = events
        .iter()
        .find_map(|event| match event {
            crate::Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture must accept NPC quote");
    game.state.npc_order_lifecycles[0].expires_market_minute = game.current_market_minute();
    (game, code, account, order_id)
}

fn expiring_retail_order() -> (
    GameSession,
    crate::StockId,
    crate::AccountId,
    crate::OrderId,
) {
    let code = crate::StockId(0);
    let account = crate::AccountId(1);
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    game.state.attention_scheduler.clear();
    let mut events = Vec::new();
    game.seed_order_for_test(
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(990)),
            qty: 100,
        },
        &mut events,
    );
    let order_id = events
        .iter()
        .find_map(|event| match event {
            crate::Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture must accept the retail quote");
    game.state.npc_order_lifecycles[0].expires_market_minute = game.current_market_minute();
    (game, code, account, order_id)
}

#[test]
fn quote_expiry_is_a_real_phase_that_preserves_authority_before_commit() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();

    assert!(plan.expiry_applied);
    assert!(plan.decision_resources.is_some());
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn quote_expiry_releases_buy_resources_and_defers_authority_until_commit() {
    let (mut game, code, account, order_id) = expiring_npc_order(crate::Side::Buy);
    let before = game.business_state_hash().unwrap();
    let cash_before = game.state.accounts[&account].cash();
    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();

    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(
        game.state.markets[&code].resting_orders_for(account).len(),
        1
    );
    assert_eq!(plan.expiry().releases.len(), 1);
    assert_eq!(plan.expiry().releases[0].receipt_index, 0);
    assert_eq!(plan.expiry().releases[0].stock, code);
    assert_eq!(plan.expiry().releases[0].order_id, order_id);
    assert_eq!(plan.expiry().releases[0].side, crate::Side::Buy);
    assert_eq!(plan.expiry().releases[0].resources.shares, 0);
    assert!(plan.expiry().releases[0].resources.cash > crate::Money::ZERO);
    assert_eq!(
        plan.expiry().released_by_account[&account],
        plan.expiry().releases[0].resources
    );

    drop(plan);
    // D2 登记：旧簿 fixture（session.markets 挂单）为旧管线专有注入面——
    // runtime 等价锚=desk orders_expire_at_ttl_with_entity_refund+翻转批撤单面。
    let events = game.step_pipeline_contract().unwrap();
    assert!(matches!(
        events.as_slice(),
        [crate::Event::OrderCanceled { account: event_account, code: event_code, id, remaining_qty: 100, .. }, _]
            if *event_account == account && event_code == &code && *id == order_id
    ));
    assert!(game.state.markets[&code]
        .resting_orders_for(account)
        .is_empty());
    assert!(game.state.npc_order_lifecycles.is_empty());
    assert_eq!(game.state.accounts[&account].cash(), cash_before);
    game.hydrate_or_validate_envelope_ledger().unwrap();
}

#[test]
fn quote_expiry_frame_keeps_the_cancellation_fact_with_a_unique_key() {
    let (mut game, _code, account, order_id) = expiring_npc_order(crate::Side::Buy);
    let frame = game.step_frame().unwrap();
    frame.validate().unwrap();
    let cancellation = frame
        .facts
        .iter()
        .find(|fact| {
            matches!(
                fact.event,
                crate::Event::OrderCanceled {
                    account: owner,
                    id,
                    ..
                } if owner == account && id == order_id
            )
        })
        .expect("expired order must have a published cancellation fact");
    assert_eq!(
        cancellation.key,
        super::EventStableKey::for_event(&cancellation.event, cancellation.key.local_event_index())
    );
}

#[test]
fn quote_expiry_retail_cancellation_diagnostic_survives_commit() {
    let (mut game, code, account, order_id) = expiring_retail_order();

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();
    drop(plan);
    game.step().unwrap();

    assert!(matches!(
        game.last_retail_order_events(),
        [crate::session::RetailOrderDiagnosticEvent::Canceled {
            account: event_account,
            code: event_code,
            order_id: event_order,
            remaining_qty: 100,
        }] if *event_account == account && event_code == &code && *event_order == order_id
    ));
}

#[test]
fn quote_expiry_diagnostic_precedes_each_same_tick_diagnostic_exactly_once() {
    let (mut game, code, account, expired_order_id) = expiring_retail_order();
    let received = game
        .state
        .ingress_receipt_cursors
        .receive(
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(980)),
                qty: 100,
            },
        )
        .unwrap();
    game.state.pending_player.push(received);

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();
    drop(plan);
    game.step().unwrap();

    assert!(matches!(
        game.last_retail_order_events(),
        [
            crate::session::RetailOrderDiagnosticEvent::Canceled {
                order_id: canceled_id,
                remaining_qty: 100,
                ..
            },
            crate::session::RetailOrderDiagnosticEvent::Submitted {
                order_id: submitted_id,
                qty: 100,
                ..
            },
        ] if *canceled_id == expired_order_id && *submitted_id != expired_order_id
    ));
}

#[test]
fn discarded_or_failed_quote_expiry_shadow_preserves_authoritative_retail_diagnostics() {
    let (mut game, _code, _account, _order_id) = expiring_retail_order();
    let before = game.last_retail_order_events().to_vec();

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();
    drop(plan);
    assert_eq!(game.last_retail_order_events(), before);

    let fatal = StepFatal::InvariantViolation {
        description: "ExpiryShadow 散户诊断回滚".to_owned(),
        location: "quote_expiry_tests".to_owned(),
    };
    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();
    game.inject_post_shadow_failure(fatal.clone());
    drop(plan);
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.last_retail_order_events(), before);
}

#[test]
fn quote_expiry_leaves_auction_and_player_orders_unaffected() {
    let code = crate::StockId(0);
    let player = crate::AccountId(0);
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(2);
    setup.ticks_per_day = 10;
    let mut game = GameSession::new(setup, 42).unwrap();
    let mut events = Vec::new();
    game.seed_auction_order_for_test(
        player,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(990)),
            qty: 100,
        },
        &mut events,
    );

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();

    assert!(plan.expiry().releases.is_empty());
    assert_eq!(game.auction_order_projection_for_test()[&code].len(), 1);
}

/// T2 收尾（Astra 节点审）：一份收据事实链——过期批的 receipt 集合与
/// 到期生命周期一一对应（数量相等）且 envelope 键无重复（同一事实恰一
/// 条收据，无中转重复载体）。
#[test]
fn quote_expiry_receipt_chain_is_single_sourced() {
    let (mut game, _code, _account, _order_id) = expiring_npc_order(crate::Side::Buy);
    let minute = game.current_market_minute();
    let due =
        super::quote_expiry::NpcOrderLifecycleBook::new(&mut game.state.npc_order_lifecycles)
            .due_at(minute)
            .count();
    let mut plan = super::TickShadowPlan {
        decision_output: None,
        session: Some(TickShadowPlan::capture_candidate(&game).unwrap()),
        event_outbox: Vec::new(),
        applied_receipts: Vec::new(),
        auction_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
        precomputed_stock_inputs: None,
        precomputed_stock_shards: None,
    };
    quote_expiry::plan_expiry(&mut plan).unwrap();
    let receipts = &plan.applied_receipts;
    assert!(
        due > 0,
        "fixture 前提自证：应存在到期生命周期（due={due}）"
    );
    assert_eq!(
        receipts.len(),
        due,
        "receipt 链条目数=到期生命周期数（一一生成，无中转重复）"
    );
    let mut envelopes: Vec<_> = receipts.iter().map(|r| r.envelope.clone()).collect();
    envelopes.sort_by_key(|e| (e.account.0, e.stock.0, e.order.0, e.side as u8));
    let unique = envelopes.iter().collect::<std::collections::BTreeSet<_>>().len();
    assert_eq!(
        envelopes.len(),
        unique,
        "envelope 键无重复（同一事实恰一条收据）"
    );
}

#[test]
fn quote_expiry_rejects_a_second_application() {
    let (game, _code, _account, _order_id) = expiring_npc_order(crate::Side::Buy);
    let mut shadow = TickShadowPlan {
        decision_output: None,
        session: Some(TickShadowPlan::capture_candidate(&game).unwrap()),
        event_outbox: Vec::new(),
        applied_receipts: Vec::new(),
        auction_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
        precomputed_stock_inputs: None,
        precomputed_stock_shards: None,
    };

    quote_expiry::plan_expiry(&mut shadow).unwrap();
    assert!(quote_expiry::plan_expiry(&mut shadow).is_err());
}

#[test]
fn quote_expiry_releases_sell_shares_with_zero_cash() {
    let (game, _code, account, _order_id) = expiring_npc_order(crate::Side::Sell);
    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();

    assert_eq!(plan.expiry().releases.len(), 1);
    assert_eq!(plan.expiry().releases[0].resources.cash, crate::Money::ZERO);
    assert_eq!(plan.expiry().releases[0].resources.shares, 100);
    assert_eq!(plan.expiry().released_by_account[&account].shares, 100);
}

#[test]
fn quote_expiry_assigns_deterministic_global_receipt_indices() {
    let first = crate::StockId(0);
    let second = crate::StockId(1);
    let account = crate::AccountId(1);
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    let mut second_spec = setup.stocks[0].clone();
    second_spec.code = crate::StockCode("600889".to_owned());
    setup.stocks.push(second_spec);
    let mut game = GameSession::new(setup, 42).unwrap();
    game.state.attention_scheduler.clear();
    let mut events = Vec::new();
    for (code, price) in [(&second, 980), (&first, 990)] {
        game.seed_order_for_test(
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(price)),
                qty: 100,
            },
            &mut events,
        );
    }
    let expiry_minute = game.current_market_minute();
    for lifecycle in &mut game.state.npc_order_lifecycles {
        lifecycle.expires_market_minute = expiry_minute;
    }

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();

    assert_eq!(plan.expiry().releases.len(), 2);
    assert_eq!(plan.expiry().releases[0].receipt_index, 0);
    assert_eq!(plan.expiry().releases[1].receipt_index, 1);
    assert_eq!(plan.expiry().releases[0].stock, first);
    assert_eq!(plan.expiry().releases[1].stock, second);
}

#[test]
fn quote_expiry_pairs_sorted_receipts_with_their_multi_account_lifecycles() {
    let code = crate::StockId(0);
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    let first = crate::AccountId(1);
    let second = crate::AccountId(2);
    game.state.attention_scheduler.clear();
    let mut events = Vec::new();
    for (account, price) in [(second, 980), (first, 990)] {
        game.seed_order_for_test(
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(price)),
                qty: 100,
            },
            &mut events,
        );
    }
    let expiry_minute = game.current_market_minute();
    for lifecycle in &mut game.state.npc_order_lifecycles {
        lifecycle.expires_market_minute = expiry_minute;
    }

    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();

    assert_eq!(plan.expiry().releases.len(), 2);
    let first_release = plan
        .expiry()
        .releases
        .iter()
        .find(|release| release.account == first)
        .expect("first account must retain its receipt identity");
    let second_release = plan
        .expiry()
        .releases
        .iter()
        .find(|release| release.account == second)
        .expect("second account must retain its receipt identity");
    assert_eq!(first_release.stock, code);
    assert_eq!(second_release.stock, code);
    assert!(first_release.resources.cash > second_release.resources.cash);
}

#[test]
fn quote_expiry_post_shadow_failure_discards_order_receipt_and_event() {
    let (mut game, code, account, _order_id) = expiring_npc_order(crate::Side::Buy);
    let before_business = game.business_state_hash().unwrap();
    let before_seq = game.seq();
    let fatal = StepFatal::InvariantViolation {
        description: "ExpiryShadow 在 shadow 后失败".to_owned(),
        location: "quote_expiry_tests".to_owned(),
    };
    let plan = plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();
    game.inject_post_shadow_failure(fatal.clone());

    drop(plan);
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.business_state_hash().unwrap(), before_business);
    assert_eq!(game.seq(), before_seq);
    assert_eq!(
        game.state.markets[&code].resting_orders_for(account).len(),
        1
    );
    assert_eq!(game.state.npc_order_lifecycles.len(), 1);
}
