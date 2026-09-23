use super::*;

#[test]
fn p0_expiry_hydrates_complete_live_books_without_expiring_a_lifecycle() {
    let code = crate::StockCode("600888".to_owned());
    let npc = crate::AccountId(1);
    let player = crate::AccountId(0);
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(2), 42).unwrap();
    game.accounts.get_mut(&npc).unwrap().strategy = None;
    let mut events = Vec::new();
    game.route_intent(
        npc,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(990),
            qty: 100,
        },
        &mut events,
    );
    game.route_auction_intent(
        player,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::Money::from_cents(980),
            qty: 100,
        },
        &mut events,
    );
    let projected = game.project_live_envelopes().unwrap();
    let cash_before = game.accounts[&npc].cash;
    let seq_before = game.seq();
    let cursor_before = game.next_receipt_base;

    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    let ledger = plan.envelope_ledger().unwrap();

    assert!(plan.expiry().releases.is_empty());
    assert_eq!(ledger.iter().count(), projected.len());
    for envelope in projected {
        assert_eq!(ledger.get(envelope.key()).unwrap(), &envelope);
    }
    assert_eq!(game.markets[&code].resting_orders_for(npc).len(), 1);
    assert_eq!(game.auction_orders[&code].len(), 1);
    assert_eq!(game.accounts[&npc].cash, cash_before);
    assert_eq!(game.seq(), seq_before);
    assert_eq!(game.next_receipt_base, cursor_before);
}
