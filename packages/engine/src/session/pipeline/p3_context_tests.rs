use super::p3_context::collect_p3_context_facts;
use super::*;

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
fn production_p3_context_preserves_each_stocks_configured_category() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.stocks[1].category = crate::SecurityCategory::StMainBoard;
    let game = GameSession::new(setup, 42).unwrap();
    let first = crate::StockCode("600888".to_owned());
    let second = crate::StockCode("600889".to_owned());

    let facts = collect_p3_context_facts(&game).unwrap();

    assert_eq!(
        facts.stocks[&second].category,
        crate::SecurityCategory::StMainBoard
    );
    assert_eq!(
        facts.stocks[&first].category,
        crate::SecurityCategory::MainBoard
    );
}
