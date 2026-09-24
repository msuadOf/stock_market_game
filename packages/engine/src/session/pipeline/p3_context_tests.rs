use super::p3_context::{checked_increment, collect_p3_context_facts};
use super::*;
use crate::{Order, OrderId, Side};

#[test]
fn production_p3_context_uses_configured_category_and_authoritative_stops() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = crate::StockCode("600888".to_owned());
    game.setup.stocks[0].category = crate::SecurityCategory::StMainBoard;
    let market = &game.markets[&code];
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();

    let facts = collect_p3_context_facts(&game).unwrap();
    let stock = &facts.stocks[&code];

    assert!(facts.account_open_orders.is_empty());
    assert_eq!(stock.category, crate::SecurityCategory::StMainBoard);
    assert_eq!(stock.market_buy_protective_price, market.up_stop().unwrap());
    assert_eq!(
        stock.market_sell_protective_price,
        market.down_stop().unwrap()
    );
    assert!(stock.market_buy_protective_price > crate::Money::ZERO);
    assert!(stock.market_sell_protective_price > crate::Money::ZERO);
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_eq!(game.session_state_hash().unwrap(), session_before);
}

#[test]
fn production_p3_context_counts_continuous_and_auction_orders_per_account() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = crate::StockCode("600888".to_owned());
    let owner = crate::AccountId(1);
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: OrderId(800),
            side: Side::Buy,
            price: crate::Money::from_cents(900),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: crate::Money::ZERO,
            owner,
            seq: 0,
        })
        .unwrap();
    game.auction_orders
        .entry(code.clone())
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner,
            side: Side::Buy,
            limit: crate::Money::from_cents(900),
            qty: 100,
            arrival_seq: 801,
        });
    game.auction_orders
        .entry(code)
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner,
            side: Side::Sell,
            limit: crate::Money::from_cents(1_100),
            qty: 100,
            arrival_seq: 802,
        });
    game.auction_order_counts.insert(owner, 2);

    let facts = collect_p3_context_facts(&game).unwrap();

    assert_eq!(facts.global_open_orders, 3);
    assert_eq!(facts.account_open_orders[&owner], 3);
}

#[test]
fn production_p3_context_merges_two_stock_counts_without_order_snapshots() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.stocks[1].category = crate::SecurityCategory::StMainBoard;
    let mut game = GameSession::new(setup, 42).unwrap();
    let first = crate::StockCode("600888".to_owned());
    let second = crate::StockCode("600889".to_owned());
    let player = crate::AccountId(0);
    let npc = crate::AccountId(1);
    for (code, owner, id, price) in [
        (first.clone(), player, 800, 900),
        (first.clone(), npc, 801, 900),
        (second.clone(), npc, 802, 900),
    ] {
        game.markets
            .get_mut(&code)
            .unwrap()
            .place(Order {
                id: OrderId(id),
                side: Side::Buy,
                price: crate::Money::from_cents(price),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: crate::Money::ZERO,
                owner,
                seq: 0,
            })
            .unwrap();
    }
    game.auction_orders
        .entry(second.clone())
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner: player,
            side: Side::Sell,
            limit: crate::Money::from_cents(1_100),
            qty: 100,
            arrival_seq: 803,
        });
    game.auction_order_counts.insert(player, 1);

    let facts = collect_p3_context_facts(&game).unwrap();

    assert_eq!(facts.global_open_orders, 4);
    assert_eq!(facts.account_open_orders[&player], 2);
    assert_eq!(facts.account_open_orders[&npc], 2);
    assert_eq!(
        facts.stocks[&second].category,
        crate::SecurityCategory::StMainBoard
    );
    assert_eq!(
        facts.stocks[&first].category,
        crate::SecurityCategory::MainBoard
    );
}

#[test]
fn production_p3_context_rejects_a_stale_auction_count_cache() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.auction_order_counts.insert(crate::AccountId(1), 1);

    let error = collect_p3_context_facts(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("auction order count cache")
                && location == "pipeline::p3_context"
    ));
}

#[test]
fn production_p3_context_rejects_a_continuous_order_with_a_missing_owner() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = crate::StockCode("600888".to_owned());
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: OrderId(900),
            side: Side::Buy,
            price: crate::Money::from_cents(900),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: crate::Money::ZERO,
            owner: crate::AccountId(999),
            seq: 0,
        })
        .unwrap();

    let error = collect_p3_context_facts(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("continuous order owner")
                && location == "pipeline::p3_context"
    ));
}

#[test]
fn production_p3_context_rejects_an_auction_order_with_a_missing_owner() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = crate::StockCode("600888".to_owned());
    let missing = crate::AccountId(999);
    game.auction_orders
        .entry(code)
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner: missing,
            side: Side::Buy,
            limit: crate::Money::from_cents(900),
            qty: 100,
            arrival_seq: 901,
        });
    game.auction_order_counts.insert(missing, 1);

    let error = collect_p3_context_facts(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("auction order owner")
                && location == "pipeline::p3_context"
    ));
}

#[test]
fn production_p3_context_count_overflow_is_explicit() {
    let error = checked_increment(usize::MAX, "test count").unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description == "test count overflow" && location == "pipeline::p3_context"
    ));
}
