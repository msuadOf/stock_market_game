use super::*;

fn expiring_npc_order(
    side: crate::Side,
) -> (
    GameSession,
    crate::StockCode,
    crate::AccountId,
    crate::OrderId,
) {
    let code = crate::StockCode("600888".to_owned());
    let account = crate::AccountId(1);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    if side == crate::Side::Sell {
        game.accounts
            .get_mut(&account)
            .unwrap()
            .grant_position(code.clone(), 100, crate::Money::from_cents(100_000))
            .unwrap();
    }
    let mut events = Vec::new();
    game.route_intent(
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side,
            price: crate::Money::from_cents(990),
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
    game.npc_order_lifecycles[0].expires_market_minute = game.current_market_minute();
    (game, code, account, order_id)
}

fn expiring_retail_order() -> (
    GameSession,
    crate::StockCode,
    crate::AccountId,
    crate::OrderId,
) {
    let code = crate::StockCode("600888".to_owned());
    let account = crate::AccountId(1);
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    let mut events = Vec::new();
    game.route_intent(
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(990),
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
    game.npc_order_lifecycles[0].expires_market_minute = game.current_market_minute();
    (game, code, account, order_id)
}

#[test]
fn p0_expiry_is_a_real_phase_that_preserves_authority_before_commit() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(plan.trace().first(), Some(&TickPhase::ExpiryShadow));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn p0_expiry_releases_buy_resources_and_defers_authority_until_p9() {
    let (mut game, code, account, order_id) = expiring_npc_order(crate::Side::Buy);
    let before = game.business_state_hash().unwrap();
    let cash_before = game.accounts[&account].cash;
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.markets[&code].resting_orders_for(account).len(), 1);
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

    let events = commit_tick(&mut game, plan).unwrap().events;
    assert!(matches!(
        events.as_slice(),
        [crate::Event::OrderCanceled { account: event_account, code: event_code, id, remaining_qty: 100, .. }, _]
            if *event_account == account && event_code == &code && *id == order_id
    ));
    assert!(game.markets[&code].resting_orders_for(account).is_empty());
    assert!(game.npc_order_lifecycles.is_empty());
    assert_eq!(game.accounts[&account].cash, cash_before);
    game.hydrate_or_validate_envelope_ledger().unwrap();
}

#[test]
fn p0_expiry_retail_cancellation_diagnostic_survives_p9_commit() {
    let (mut game, code, account, order_id) = expiring_retail_order();

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
fn p0_expiry_diagnostic_precedes_each_legacy_tick_diagnostic_exactly_once() {
    let (mut game, code, account, expired_order_id) = expiring_retail_order();
    game.pending_player.push((
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(980),
            qty: 100,
        },
    ));

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
fn discarded_or_failed_p0_shadow_preserves_authoritative_retail_diagnostics() {
    let (mut game, _code, _account, _order_id) = expiring_retail_order();
    let before = game.last_retail_order_events().to_vec();

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    drop(plan);
    assert_eq!(game.last_retail_order_events(), before);

    let fatal = StepFatal::InvariantViolation {
        description: "P0 retail diagnostic rollback".to_owned(),
        location: "p0_expiry_tests".to_owned(),
    };
    game.inject_post_shadow_failure(fatal.clone());
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.last_retail_order_events(), before);
}

#[test]
fn p0_expiry_leaves_auction_and_player_orders_unaffected() {
    let code = crate::StockCode("600888".to_owned());
    let player = crate::AccountId(0);
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(2);
    setup.ticks_per_day = 10;
    let mut game = GameSession::new(setup, 42).unwrap();
    let mut events = Vec::new();
    game.route_auction_intent(
        player,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(990),
            qty: 100,
        },
        &mut events,
    );

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert!(plan.expiry().releases.is_empty());
    assert_eq!(game.auction_orders[&code].len(), 1);
}

#[test]
fn p0_expiry_rejects_a_second_application() {
    let (game, _code, _account, _order_id) = expiring_npc_order(crate::Side::Buy);
    let input = PhaseInput { session: &game };
    let mut shadow = TickShadowPlan {
        state: TickShadow::capture(&game).unwrap(),
        tokens: Vec::new(),
        event_outbox: Vec::new(),
        receipt_keys: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
    };

    p0_expiry::plan_expiry(&input, TickStart, &mut shadow).unwrap();
    assert!(p0_expiry::plan_expiry(&input, TickStart, &mut shadow).is_err());
}

#[test]
fn p0_expiry_releases_sell_shares_with_zero_cash() {
    let (game, _code, account, _order_id) = expiring_npc_order(crate::Side::Sell);
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(plan.expiry().releases.len(), 1);
    assert_eq!(plan.expiry().releases[0].resources.cash, crate::Money::ZERO);
    assert_eq!(plan.expiry().releases[0].resources.shares, 100);
    assert_eq!(plan.expiry().released_by_account[&account].shares, 100);
}

#[test]
fn p0_expiry_assigns_deterministic_global_receipt_indices() {
    let first = crate::StockCode("600888".to_owned());
    let second = crate::StockCode("600889".to_owned());
    let account = crate::AccountId(1);
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    let mut second_spec = setup.stocks[0].clone();
    second_spec.code = second.clone();
    setup.stocks.push(second_spec);
    let mut game = GameSession::new(setup, 42).unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    let mut events = Vec::new();
    for (code, price) in [(&second, 980), (&first, 990)] {
        game.route_intent(
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::Money::from_cents(price),
                qty: 100,
            },
            &mut events,
        );
    }
    let expiry_minute = game.current_market_minute();
    for lifecycle in &mut game.npc_order_lifecycles {
        lifecycle.expires_market_minute = expiry_minute;
    }

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(plan.expiry().releases.len(), 2);
    assert_eq!(plan.expiry().releases[0].receipt_index, 0);
    assert_eq!(plan.expiry().releases[1].receipt_index, 1);
    assert_eq!(plan.expiry().releases[0].stock, first);
    assert_eq!(plan.expiry().releases[1].stock, second);
}

#[test]
fn p0_expiry_pairs_sorted_receipts_with_their_multi_account_lifecycles() {
    let code = crate::StockCode("600888".to_owned());
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    let first = crate::AccountId(1);
    let second = crate::AccountId(2);
    for account in [first, second] {
        game.accounts.get_mut(&account).unwrap().strategy = None;
    }
    let mut events = Vec::new();
    for (account, price) in [(second, 980), (first, 990)] {
        game.route_intent(
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: crate::Money::from_cents(price),
                qty: 100,
            },
            &mut events,
        );
    }
    let expiry_minute = game.current_market_minute();
    for lifecycle in &mut game.npc_order_lifecycles {
        lifecycle.expires_market_minute = expiry_minute;
    }

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

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
fn p0_expiry_post_shadow_failure_discards_order_receipt_and_event() {
    let (mut game, code, account, _order_id) = expiring_npc_order(crate::Side::Buy);
    let before_business = game.business_state_hash().unwrap();
    let before_seq = game.seq();
    let fatal = StepFatal::InvariantViolation {
        description: "P0 post-shadow failure".to_owned(),
        location: "p0_expiry_tests".to_owned(),
    };
    game.inject_post_shadow_failure(fatal.clone());

    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.business_state_hash().unwrap(), before_business);
    assert_eq!(game.seq(), before_seq);
    assert_eq!(game.markets[&code].resting_orders_for(account).len(), 1);
    assert_eq!(game.npc_order_lifecycles.len(), 1);
}
