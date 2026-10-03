use super::execution::reconcile_plan::{ReconciliationPlan, WorkingOrderDecision};
use super::*;

fn buy(code: &StockCode, price: i64) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(price)),
        qty: 100,
    }
}

fn resting_buy() -> (GameSession, AccountId, StockCode, OrderId) {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 91).unwrap();
    let mut events = Vec::new();
    session.seed_order_for_test(account, buy(&code, 900), &mut events);
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
    session.plan_npc_working_order_reconciliation(
        desired,
        TradingPhase::CallAuction,
        scope,
        WorkingOrderSlices {
            continuous: continuous.get(&account).map(Vec::as_slice).unwrap_or(&[]),
            auction: auction.get(&account).map(Vec::as_slice).unwrap_or(&[]),
        },
    )
}

fn continuous_plan(
    session: &GameSession,
    account: AccountId,
    desired: Vec<Intent>,
) -> ReconciliationPlan {
    let (continuous, auction) = session.working_orders_by_account();
    session.plan_npc_working_order_reconciliation(
        desired,
        TradingPhase::Continuous,
        ReconcileScope::AllWorkingOrders,
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
    session.seed_auction_order_for_test(account, buy(&code, 900), &mut events);
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
    let plan = continuous_plan(&session, account, vec![buy(&code, 900)]);

    assert!(
        matches!(plan.decisions.as_slice(), [WorkingOrderDecision::Keep { order_id: id }] if *id == order_id)
    );
    assert!(plan.residual_intents.is_empty());
}

#[test]
fn symbolic_buy_replaces_equal_fixed_quote_without_consuming_raw_intent() {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 91).unwrap();
    let mut events = Vec::new();
    session.seed_order_for_test(account, buy(&code, 1_100), &mut events);
    let order_id = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    let target = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: LimitPrice::Highest,
        qty: 100,
    };
    let plan = continuous_plan(&session, account, vec![target.clone()]);
    assert!(matches!(
        plan.decisions.as_slice(),
        [WorkingOrderDecision::Replace { old_order_id, new_intent }]
            if *old_order_id == order_id && serde_json::to_value(new_intent).unwrap() == serde_json::to_value(&target).unwrap()
    ));
    assert!(matches!(
        plan.residual_intents.as_slice(),
        [Intent::PlaceLimit {
            price: LimitPrice::Highest,
            ..
        }]
    ));
}

#[test]
fn symbolic_sell_replaces_equal_fixed_quote_without_consuming_raw_intent() {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 91).unwrap();
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    let mut events = Vec::new();
    session.seed_order_for_test(
        account,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: LimitPrice::Fixed(Money::from_cents(900)),
            qty: 100,
        },
        &mut events,
    );
    let order_id = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    let target = Intent::PlaceLimit {
        code,
        side: Side::Sell,
        price: LimitPrice::Lowest,
        qty: 100,
    };
    let plan = continuous_plan(&session, account, vec![target]);
    assert!(matches!(
        plan.decisions.as_slice(),
        [WorkingOrderDecision::Replace { old_order_id, new_intent: Intent::PlaceLimit { price: LimitPrice::Lowest, .. } }]
            if *old_order_id == order_id
    ));
    assert!(matches!(
        plan.residual_intents.as_slice(),
        [Intent::PlaceLimit {
            price: LimitPrice::Lowest,
            ..
        }]
    ));
}

#[test]
fn reconciliation_plan_links_replace_without_consuming_residual_target() {
    let (session, account, code, order_id) = resting_buy();
    let target = buy(&code, 990);
    let business_before = session.business_state_hash().unwrap();
    let seq_before = session.seq();
    let plan = continuous_plan(&session, account, vec![target.clone()]);

    assert!(
        matches!(plan.decisions.as_slice(), [WorkingOrderDecision::Replace { old_order_id, new_intent: Intent::PlaceLimit { price, .. } }] if *old_order_id == order_id && *price == LimitPrice::Fixed(Money::from_cents(990)))
    );
    assert!(
        matches!(plan.residual_intents.as_slice(), [Intent::PlaceLimit { price, .. }] if *price == LimitPrice::Fixed(Money::from_cents(990)))
    );
    assert_eq!(session.business_state_hash().unwrap(), business_before);
    assert_eq!(session.seq(), seq_before);
}

#[test]
fn reconciliation_plan_cancels_cross_side_without_consuming_opposite_target() {
    let (mut session, account, code, order_id) = resting_buy();
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    let target = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Sell,
        price: LimitPrice::Fixed(Money::from_cents(910)),
        qty: 100,
    };
    let plan = continuous_plan(&session, account, vec![target]);

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
        matches!(plan.residual_intents.as_slice(), [Intent::PlaceLimit { price, .. }] if *price == LimitPrice::Fixed(Money::from_cents(901)))
    );
}

#[test]
fn reconciliation_plan_keeps_identical_auction_quote() {
    let (session, account, code, order_id) = resting_auction_buy();
    let plan = auction_plan(
        &session,
        account,
        vec![buy(&code, 900)],
        ReconcileScope::AllWorkingOrders,
    );

    assert!(matches!(
        plan.decisions.as_slice(),
        [WorkingOrderDecision::Keep { order_id: id }] if *id == order_id
    ));
    assert!(plan.residual_intents.is_empty());
}

#[test]
fn reconciliation_plan_locked_reviewed_auction_suppresses_opposite_target() {
    let (mut session, account, code, _order_id) = resting_auction_buy();
    session.state.tick = 300;
    let plan = auction_plan(
        &session,
        account,
        vec![Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: LimitPrice::Fixed(Money::from_cents(1_100)),
            qty: 100,
        }],
        ReconcileScope::ReviewedStocks([code].into()),
    );

    assert!(plan.decisions.is_empty());
    assert!(plan.residual_intents.is_empty());
}

#[test]
fn reconciliation_plan_locked_auction_suppresses_same_side_and_crossing_targets() {
    let (mut session, account, code, _order_id) = resting_auction_buy();
    session.state.tick = 300;
    let crossing = Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Sell,
        price: LimitPrice::Fixed(Money::from_cents(900)),
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
    session.state.tick = 300;
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
