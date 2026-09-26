use super::*;

fn p1_fixture(
    side: crate::Side,
    expires: bool,
) -> (GameSession, crate::AccountId, crate::StockCode) {
    let account = crate::AccountId(1);
    let code = crate::StockCode("600888".to_owned());
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
    game.seed_order_for_test(
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side,
            price: crate::Money::from_cents(990),
            qty: 100,
        },
        &mut events,
    );
    if expires {
        game.npc_order_lifecycles[0].expires_market_minute = game.current_market_minute();
    }
    (game, account, code)
}

#[test]
fn p1_expired_buy_releases_exactly_one_zero_free_cash_reservation() {
    let (mut game, account, _code) = p1_fixture(crate::Side::Buy, true);
    let reservation = game.project_live_envelopes().unwrap()[0].live().cash;
    game.accounts.get_mut(&account).unwrap().cash = reservation;
    let (mut before_expiry, _, _) = p1_fixture(crate::Side::Buy, false);
    before_expiry.accounts.get_mut(&account).unwrap().cash = reservation;
    let before_reservation = before_expiry.project_live_envelopes().unwrap()[0]
        .live()
        .cash;
    let before_plan = plan_tick(PhaseInput {
        session: &before_expiry,
    })
    .unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(before_reservation, reservation);
    assert_eq!(
        before_plan
            .decision_resources()
            .unwrap()
            .available_cash(account)
            .unwrap(),
        crate::Money::ZERO
    );
    assert_eq!(
        plan.decision_resources()
            .unwrap()
            .available_cash(account)
            .unwrap(),
        reservation
    );
    assert_ne!(
        plan.decision_resources()
            .unwrap()
            .available_cash(account)
            .unwrap(),
        crate::Money::ZERO
    );
    assert_ne!(
        plan.decision_resources()
            .unwrap()
            .available_cash(account)
            .unwrap(),
        reservation.add(reservation).unwrap()
    );
    assert_eq!(game.accounts[&account].cash, reservation);
}

#[test]
fn p1_expired_sell_releases_exactly_one_fully_reserved_position() {
    let (game, account, code) = p1_fixture(crate::Side::Sell, true);
    let sellable_before = game.accounts[&account].sellable_qty(&code);
    let reserved = game.project_live_envelopes().unwrap()[0].live().shares;
    let before_expiry = p1_fixture(crate::Side::Sell, false).0;
    let before_plan = plan_tick(PhaseInput {
        session: &before_expiry,
    })
    .unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(reserved, sellable_before);
    assert_eq!(
        before_plan
            .decision_resources()
            .unwrap()
            .available_sell_qty(account, &code)
            .unwrap(),
        0
    );
    assert_eq!(
        plan.decision_resources()
            .unwrap()
            .available_sell_qty(account, &code)
            .unwrap(),
        sellable_before
    );
    assert_ne!(
        plan.decision_resources()
            .unwrap()
            .available_sell_qty(account, &code)
            .unwrap(),
        sellable_before.checked_add(reserved).unwrap()
    );
    assert_eq!(game.accounts[&account].sellable_qty(&code), sellable_before);
}

#[test]
fn p1_subtracts_live_buy_and_sell_reservations_and_rejects_unknown_keys() {
    let (buy_game, buy_account, buy_code) = p1_fixture(crate::Side::Buy, false);
    let buy_plan = plan_tick(PhaseInput { session: &buy_game }).unwrap();
    assert!(
        buy_plan
            .decision_resources()
            .unwrap()
            .available_cash(buy_account)
            .unwrap()
            < buy_game.accounts[&buy_account].cash
    );
    assert_eq!(
        buy_plan
            .decision_resources()
            .unwrap()
            .available_sell_qty(buy_account, &buy_code)
            .unwrap(),
        0
    );

    let (sell_game, sell_account, sell_code) = p1_fixture(crate::Side::Sell, false);
    let sell_plan = plan_tick(PhaseInput {
        session: &sell_game,
    })
    .unwrap();
    assert_eq!(
        sell_plan
            .decision_resources()
            .unwrap()
            .available_cash(sell_account)
            .unwrap(),
        sell_game.accounts[&sell_account].cash
    );
    assert_eq!(
        sell_plan
            .decision_resources()
            .unwrap()
            .available_sell_qty(sell_account, &sell_code)
            .unwrap(),
        0
    );
    assert!(sell_plan
        .decision_resources()
        .unwrap()
        .available_cash(crate::AccountId(999))
        .is_err());
    assert!(sell_plan
        .decision_resources()
        .unwrap()
        .available_sell_qty(sell_account, &crate::StockCode("999999".to_owned()))
        .is_err());
}

#[test]
fn p1_sell_reservation_uses_account_book_key() {
    let (mut game, account, code) = p1_fixture(crate::Side::Sell, false);
    let original = game.accounts.get(&account).unwrap().clone();
    let mut mismatched = original;
    mismatched.id = crate::AccountId(99);
    game.accounts.insert(account, mismatched);

    let resources = DecisionResourceSnapshot::seal(&game).unwrap();

    assert_eq!(resources.available_sell_qty(account, &code).unwrap(), 0);
}

#[test]
fn p1_mixed_books_ignore_pending_plan_events_and_keep_seller_cash_unreserved() {
    let code = crate::StockCode("600888".to_owned());
    let seller = crate::AccountId(1);
    let player = crate::AccountId(0);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(2), 42).unwrap();
    game.accounts.get_mut(&seller).unwrap().strategy = None;
    game.accounts
        .get_mut(&seller)
        .unwrap()
        .grant_position(code.clone(), 100, crate::Money::from_cents(100_000))
        .unwrap();
    let seller_cash = game.accounts[&seller].cash;
    let player_cash = game.accounts[&player].cash;
    let mut events = Vec::new();
    game.seed_order_for_test(
        seller,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Sell,
            price: crate::Money::from_cents(990),
            qty: 100,
        },
        &mut events,
    );
    game.seed_order_for_test(
        player,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(980),
            qty: 100,
        },
        &mut events,
    );
    let baseline = plan_tick(PhaseInput { session: &game }).unwrap();
    game.pending_plan_events
        .push(crate::session::plan_execution::PendingPlanEvent::DayEnded {
            plan_id: crate::PlanId(1),
            trading_day: 0,
        });

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let allocation = plan.decision_resources().unwrap();

    assert_eq!(allocation.available_cash(seller).unwrap(), seller_cash);
    assert_eq!(allocation.available_sell_qty(seller, &code).unwrap(), 0);
    assert!(allocation.available_cash(player).unwrap() < player_cash);
    assert_eq!(game.pending_plan_events.len(), 1);
    assert_eq!(
        allocation.available_cash(player).unwrap(),
        baseline
            .decision_resources()
            .unwrap()
            .available_cash(player)
            .unwrap()
    );
}

#[test]
fn p1_seals_complete_decision_resources_from_post_p0_shadow() {
    let held = crate::StockCode("600888".to_owned());
    let unheld = crate::StockCode("600889".to_owned());
    let account = crate::AccountId(1);
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(held.clone(), 200, crate::Money::from_cents(1_000))
        .unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .positions
        .get_mut(&held)
        .unwrap()
        .t1_locked = 100;
    game.accounts.get_mut(&account).unwrap().cash = crate::Money::from_cents(500_000);

    let mut events = Vec::new();
    game.seed_order_for_test(
        account,
        crate::Intent::PlaceLimit {
            code: held.clone(),
            side: crate::Side::Sell,
            price: crate::Money::from_cents(990),
            qty: 100,
        },
        &mut events,
    );
    game.seed_order_for_test(
        account,
        crate::Intent::PlaceLimit {
            code: unheld.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(980),
            qty: 100,
        },
        &mut events,
    );
    game.markets
        .get_mut(&held)
        .unwrap()
        .set_last_price(crate::Money::from_cents(1_250));
    let raw_cash = game.accounts[&account].cash;
    let expected_equity = raw_cash
        .add(crate::Money::from_cents(1_250).mul_shares(200).unwrap())
        .unwrap();
    let before = game.business_state_hash().unwrap();

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let resources = plan.decision_resources().unwrap();

    assert_eq!(resources.raw_cash(account).unwrap(), raw_cash);
    assert!(resources.available_cash(account).unwrap() < raw_cash);
    assert_eq!(
        resources
            .raw_cash(account)
            .unwrap()
            .sub(resources.available_cash(account).unwrap())
            .unwrap(),
        resources.reserved_cash(account).unwrap()
    );
    assert_eq!(resources.equity(account).unwrap(), expected_equity);
    assert_eq!(resources.total_held_qty(account, &held).unwrap(), 200);
    assert_eq!(
        resources
            .sellable_before_reservation(account, &held)
            .unwrap(),
        100
    );
    assert_eq!(resources.available_sell_qty(account, &held).unwrap(), 0);
    assert_eq!(resources.reserved_sell_qty(account, &held).unwrap(), 100);
    assert_eq!(
        resources.cost_price(account, &held).unwrap(),
        Some(crate::Money::from_cents(1_000))
    );
    assert_eq!(resources.total_held_qty(account, &unheld).unwrap(), 0);
    assert_eq!(
        resources
            .sellable_before_reservation(account, &unheld)
            .unwrap(),
        0
    );
    assert_eq!(resources.available_sell_qty(account, &unheld).unwrap(), 0);
    assert_eq!(resources.reserved_sell_qty(account, &unheld).unwrap(), 0);
    assert_eq!(resources.cost_price(account, &unheld).unwrap(), None);
    assert_eq!(
        resources
            .total_held_qty(crate::AccountId(0), &unheld)
            .unwrap(),
        0
    );
    assert!(resources.raw_cash(crate::AccountId(999)).is_err());
    assert!(resources
        .total_held_qty(account, &crate::StockCode("999999".to_owned()))
        .is_err());
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn p1_decision_resource_equity_overflow_fails_closed_without_authority_mutation() {
    let code = crate::StockCode("600888".to_owned());
    let account = crate::AccountId(1);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 2, crate::Money::from_cents(1))
        .unwrap();
    game.markets
        .get_mut(&code)
        .unwrap()
        .set_last_price(crate::Money::from_cents(i64::MAX));
    let before = game.business_state_hash().unwrap();

    let error = match plan_tick(PhaseInput { session: &game }) {
        Ok(_) => panic!("equity overflow must fail P1 sealing"),
        Err(error) => error,
    };

    assert!(matches!(error, StepFatal::InvariantViolation { .. }));
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn p1_account_resources_are_identical_with_one_or_four_workers() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 3;
    let mut game = GameSession::new(setup, 42).unwrap();
    let codes = game.markets.keys().cloned().collect::<Vec<_>>();
    for (index, account) in [
        crate::AccountId(1),
        crate::AccountId(2),
        crate::AccountId(3),
    ]
    .into_iter()
    .enumerate()
    {
        game.accounts
            .get_mut(&account)
            .unwrap()
            .grant_position(
                codes[index % codes.len()].clone(),
                100,
                crate::Money::from_cents(1_000),
            )
            .unwrap();
    }
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| DecisionResourceSnapshot::seal(&game).unwrap())
    };
    assert_eq!(run(1), run(4));
}

#[test]
fn p1_resource_snapshot_keeps_sealed_values_after_account_and_market_change() {
    let code = crate::StockCode("600888".to_owned());
    let account = crate::AccountId(1);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, crate::Money::from_cents(1_000))
        .unwrap();
    let sealed = DecisionResourceSnapshot::seal(&game).unwrap();
    let cash = sealed.available_cash(account).unwrap();
    let equity = sealed.equity(account).unwrap();

    game.accounts.get_mut(&account).unwrap().cash = crate::Money::ZERO;
    game.accounts
        .get_mut(&account)
        .unwrap()
        .positions
        .get_mut(&code)
        .unwrap()
        .qty = 0;
    game.markets
        .get_mut(&code)
        .unwrap()
        .set_last_price(crate::Money::from_cents(2_000));

    assert_eq!(sealed.available_cash(account).unwrap(), cash);
    assert_eq!(sealed.equity(account).unwrap(), equity);
    assert_eq!(sealed.total_held_qty(account, &code).unwrap(), 100);
}

#[test]
fn p1_parallel_account_failures_reject_corrupt_resources() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    let code = game.setup.stocks[0].code.clone();
    let seller = crate::AccountId(1);
    let buyer = crate::AccountId(2);
    game.accounts
        .get_mut(&seller)
        .unwrap()
        .grant_position(code.clone(), 100, crate::Money::from_cents(1_000))
        .unwrap();
    let mut events = Vec::new();
    game.seed_order_for_test(
        seller,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Sell,
            price: crate::Money::from_cents(1_100),
            qty: 100,
        },
        &mut events,
    );
    game.seed_order_for_test(
        buyer,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(900),
            qty: 100,
        },
        &mut events,
    );
    assert_eq!(game.project_live_envelopes().unwrap().len(), 2);
    game.accounts
        .get_mut(&seller)
        .unwrap()
        .positions
        .get_mut(&code)
        .unwrap()
        .qty = 0;
    game.accounts.get_mut(&buyer).unwrap().cash = crate::Money::ZERO;
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| DecisionResourceSnapshot::seal(&game).unwrap_err())
    };
    for threads in [1, 4] {
        assert!(matches!(
            run(threads),
            StepFatal::InvariantViolation { description, location }
                if (description == "live sell reservation exceeds sellable shares"
                    || description == "live buy reservation exceeds account cash")
                    && location == "pipeline::decision_resources::allocation"
        ));
    }
}

#[test]
fn p1_parallel_equity_failures_reject_overflow() {
    let code = crate::StockCode("600888".to_owned());
    let player = crate::AccountId(0);
    let npc = crate::AccountId(1);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts
        .get_mut(&player)
        .unwrap()
        .grant_position(code.clone(), 2, crate::Money::from_cents(1))
        .unwrap();
    game.accounts
        .get_mut(&npc)
        .unwrap()
        .grant_position(code.clone(), 1, crate::Money::from_cents(1))
        .unwrap();
    game.markets
        .get_mut(&code)
        .unwrap()
        .set_last_price(crate::Money::from_cents(i64::MAX));
    let run = |threads, session: &GameSession| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                DecisionResourceSnapshot::seal(session)
                    .err()
                    .expect("equity overflow must fail")
            })
    };
    for threads in [1, 4] {
        assert!(matches!(
            run(threads, &game),
            StepFatal::InvariantViolation { description, location }
                if (description.contains("mul_shares")
                    || description.contains("overflow in add"))
                    && location == "pipeline::decision_resources"
        ));
    }

    game.accounts
        .get_mut(&player)
        .unwrap()
        .positions
        .get_mut(&code)
        .unwrap()
        .qty = 0;
    let second = run(4, &game);
    assert!(matches!(
        &second,
        StepFatal::InvariantViolation { description, location }
            if description.contains("overflow in add")
                && location == "pipeline::decision_resources"
    ));
}
