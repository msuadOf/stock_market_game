use super::execution::reconcile_plan::{ReconciliationPlan, WorkingOrderDecision};
use super::*;

fn buy(code: &StockCode, price: i64) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(price),
        qty: 100,
    }
}

fn resting_buy() -> (GameSession, AccountId, StockCode, OrderId) {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 91).unwrap();
    let mut events = Vec::new();
    session.route_intent(account, buy(&code, 900), &mut events);
    let order_id = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture order accepted");
    (session, account, code, order_id)
}

fn auction_plan(
    session: &GameSession,
    account: AccountId,
    desired: Vec<Intent>,
    scope: ReconcileScope,
) -> ReconciliationPlan {
    let (continuous, auction) = session.working_orders_by_account();
    session.plan_npc_working_orders_from_index(
        desired,
        TradingPhase::CallAuction,
        scope,
        WorkingOrderSlices {
            continuous: continuous.get(&account).map(Vec::as_slice).unwrap_or(&[]),
            auction: auction.get(&account).map(Vec::as_slice).unwrap_or(&[]),
        },
    )
}

fn resting_auction_buy() -> (GameSession, AccountId, StockCode, OrderId) {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(900), 92).unwrap();
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
fn reconciliation_plan_keeps_exact_quote_and_consumes_only_that_target() {
    let (session, account, code, order_id) = resting_buy();
    let plan =
        session.plan_npc_working_orders(account, vec![buy(&code, 900)], TradingPhase::Continuous);

    assert!(
        matches!(plan.decisions.as_slice(), [WorkingOrderDecision::Keep { order_id: id }] if *id == order_id)
    );
    assert!(plan.residual_intents.is_empty());
}

#[test]
fn reconciliation_plan_links_replace_without_consuming_residual_target() {
    let (session, account, code, order_id) = resting_buy();
    let target = buy(&code, 990);
    let business_before = session.business_state_hash().unwrap();
    let seq_before = session.seq();
    let plan =
        session.plan_npc_working_orders(account, vec![target.clone()], TradingPhase::Continuous);

    assert!(
        matches!(plan.decisions.as_slice(), [WorkingOrderDecision::Replace { old_order_id, new_intent: Intent::PlaceLimit { price, .. } }] if *old_order_id == order_id && *price == Money::from_cents(990))
    );
    assert!(
        matches!(plan.residual_intents.as_slice(), [Intent::PlaceLimit { price, .. }] if *price == Money::from_cents(990))
    );
    assert_eq!(session.business_state_hash().unwrap(), business_before);
    assert_eq!(session.seq(), seq_before);
}

#[test]
fn reconciliation_plan_cancels_cross_side_without_consuming_opposite_target() {
    let (mut session, account, code, order_id) = resting_buy();
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    let target = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Sell,
        price: Money::from_cents(910),
        qty: 100,
    };
    let plan = session.plan_npc_working_orders(account, vec![target], TradingPhase::Continuous);

    assert!(
        matches!(plan.decisions.as_slice(), [WorkingOrderDecision::Cancel { order_id: id, .. }] if *id == order_id)
    );
    assert!(matches!(
        plan.residual_intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            ..
        }]
    ));
}

#[test]
fn reconciliation_executor_cancels_replace_then_later_routes_one_residual_target() {
    let (mut session, account, code, order_id) = resting_buy();
    let mut events = Vec::new();
    let residual = session.reconcile_npc_working_orders(
        account,
        vec![buy(&code, 990)],
        TradingPhase::Continuous,
        &mut events,
    );
    assert!(matches!(events.as_slice(), [Event::OrderCanceled { id, .. }] if *id == order_id));
    assert_eq!(residual.len(), 1);
    for intent in residual {
        session.route_intent(account, intent, &mut events);
    }
    assert_eq!(session.markets[&code].resting_orders_for(account).len(), 1);
    assert!(
        matches!(session.markets[&code].resting_orders_for(account).as_slice(), [order] if order.price == Money::from_cents(990))
    );
}

#[test]
fn reconciliation_plan_cancelable_auction_cancels_and_retains_changed_target() {
    let (session, account, code, order_id) = resting_auction_buy();
    let plan = auction_plan(
        &session,
        account,
        vec![buy(&code, 901)],
        ReconcileScope::AllWorkingOrders,
    );

    assert!(
        matches!(plan.decisions.as_slice(), [WorkingOrderDecision::Cancel { order_id: id, .. }] if *id == order_id)
    );
    assert!(
        matches!(plan.residual_intents.as_slice(), [Intent::PlaceLimit { price, .. }] if *price == Money::from_cents(901))
    );
}

#[test]
fn reconciliation_plan_locked_auction_suppresses_same_side_and_crossing_targets() {
    let (mut session, account, code, _order_id) = resting_auction_buy();
    session.tick = 300;
    let crossing = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Sell,
        price: Money::from_cents(900),
        qty: 100,
    };
    let plan = auction_plan(
        &session,
        account,
        vec![buy(&code, 901), crossing],
        ReconcileScope::AllWorkingOrders,
    );

    assert!(plan.decisions.is_empty());
    assert!(plan.residual_intents.is_empty());
}

#[test]
fn reconciliation_plan_locked_reviewed_auction_suppresses_only_reviewed_stock() {
    let (mut session, account, code, _order_id) = resting_auction_buy();
    let other = StockCode("600889".to_owned());
    session.tick = 300;
    let plan = auction_plan(
        &session,
        account,
        vec![buy(&code, 901), buy(&other, 900)],
        ReconcileScope::ReviewedStocks([code.clone()].into()),
    );

    assert!(plan.decisions.is_empty());
    assert!(
        matches!(plan.residual_intents.as_slice(), [Intent::PlaceLimit { code: target, .. }] if *target == other)
    );
}
