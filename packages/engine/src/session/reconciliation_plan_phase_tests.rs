use super::execution::reconcile_plan::WorkingOrderDecision;
use super::*;

fn buy(code: &StockCode, price: i64) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(price),
        qty: 100,
    }
}

fn resting_auction_buy() -> (GameSession, AccountId, StockCode, OrderId) {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(900), 93).unwrap();
    let mut events = Vec::new();
    session.route_auction_intent(account, buy(&code, 900), &mut events);
    let order_id = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture auction order accepted");
    (session, account, code, order_id)
}

#[test]
fn reconciliation_plan_preopen_and_closing_auction_preserve_residual_order() {
    let (session, account, code, _) = resting_auction_buy();
    let desired = vec![buy(&code, 901), buy(&code, 902)];

    for phase in [TradingPhase::PreOpen, TradingPhase::ClosingAuction] {
        let plan = session.plan_npc_working_orders(account, desired.clone(), phase);
        assert!(plan.decisions.is_empty());
        assert!(matches!(
            plan.residual_intents.as_slice(),
            [
                Intent::PlaceLimit { price: first, .. },
                Intent::PlaceLimit { price: second, .. },
            ] if *first == Money::from_cents(901) && *second == Money::from_cents(902)
        ));
    }
}

#[test]
fn reconciliation_plan_multiple_orders_preserves_decision_and_residual_order() {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 94).unwrap();
    let mut events = Vec::new();
    session.route_intent(account, buy(&code, 900), &mut events);
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    session.route_intent(
        account,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_100),
            qty: 100,
        },
        &mut events,
    );
    let ids: Vec<_> = session.markets[&code]
        .resting_orders_for(account)
        .iter()
        .map(|order| order.id)
        .collect();
    let plan = session.plan_npc_working_orders(
        account,
        vec![
            buy(&code, 900),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1_090),
                qty: 100,
            },
            buy(&code, 780),
        ],
        TradingPhase::Continuous,
    );

    assert!(matches!(
        plan.decisions.as_slice(),
        [
            WorkingOrderDecision::Keep { order_id: first },
            WorkingOrderDecision::Replace { old_order_id: second, .. },
        ] if *first == ids[0] && *second == ids[1]
    ));
    assert!(matches!(
        plan.residual_intents.as_slice(),
        [
            Intent::PlaceLimit { price: first, side: Side::Sell, .. },
            Intent::PlaceLimit { price: second, side: Side::Buy, .. },
        ] if *first == Money::from_cents(1_090) && *second == Money::from_cents(780)
    ));
}

#[test]
fn reconciliation_planner_preserves_all_authority_surfaces() {
    let (mut session, account, code, _) = resting_auction_buy();
    session
        .enqueue_player_intent(AccountId(0), buy(&code, 880))
        .expect("player fixture accepts pending intent");
    let business_before = session.business_state_hash().unwrap();
    let seq_before = session.seq();
    let auction_before = session.auction_orders.clone();
    let lifecycle_before = session.npc_order_lifecycles.clone();
    let pending_before = session.pending_player.len();

    let plan =
        session.plan_npc_working_orders(account, vec![buy(&code, 901)], TradingPhase::CallAuction);

    assert_eq!(plan.decisions.len(), 1);
    assert_eq!(session.business_state_hash().unwrap(), business_before);
    assert_eq!(session.seq(), seq_before);
    assert_eq!(session.auction_orders, auction_before);
    assert_eq!(session.npc_order_lifecycles, lifecycle_before);
    assert_eq!(session.pending_player.len(), pending_before);
}

#[test]
fn reconciliation_executor_cancelable_auction_preserves_event_seq_and_residual_routing() {
    let (mut session, account, code, order_id) = resting_auction_buy();
    let seq_before = session.seq();
    let mut events = Vec::new();

    let residual = session.reconcile_npc_working_orders(
        account,
        vec![buy(&code, 901)],
        TradingPhase::CallAuction,
        &mut events,
    );

    assert!(matches!(
        events.as_slice(),
        [Event::OrderCanceled { seq, id, .. }] if *seq == seq_before + 1 && *id == order_id
    ));
    assert_eq!(residual.len(), 1);
    for intent in residual {
        session.route_auction_intent(account, intent, &mut events);
    }
    assert!(matches!(
        session.auction_orders[&code].as_slice(),
        [order] if order.owner == account && order.limit == Money::from_cents(901)
    ));
}
