use super::execution::reconcile_plan::WorkingOrderDecision;
use super::*;

fn buy(code: &StockCode, price: i64) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(price)),
        qty: 100,
    }
}

fn working_orders_plan(
    session: &GameSession,
    account: AccountId,
    desired: Vec<Intent>,
    phase: TradingPhase,
) -> super::execution::reconcile_plan::ReconciliationPlan {
    let (continuous, auction) = session.working_orders_by_account();
    session.plan_npc_working_order_reconciliation(
        desired,
        phase,
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
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(900), 93).unwrap();
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
fn reconciliation_plan_preopen_and_closing_auction_preserve_residual_order() {
    let (session, account, code, _) = resting_auction_buy();
    let desired = vec![buy(&code, 901), buy(&code, 902)];

    for phase in [TradingPhase::PreOpen, TradingPhase::ClosingAuction] {
        let plan = working_orders_plan(&session, account, desired.clone(), phase);
        assert!(plan.decisions.is_empty());
        assert!(matches!(
            plan.residual_intents.as_slice(),
            [
                Intent::PlaceLimit { price: first, .. },
                Intent::PlaceLimit { price: second, .. },
            ] if *first == LimitPrice::Fixed(Money::from_cents(901)) && *second == LimitPrice::Fixed(Money::from_cents(902))
        ));
    }
}

#[test]
fn reconciliation_plan_multiple_orders_preserves_decision_and_residual_order() {
    let account = AccountId(1);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 94).unwrap();
    let mut events = Vec::new();
    session.seed_order_for_test(account, buy(&code, 900), &mut events);
    session
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(90_000))
        .unwrap();
    session.seed_order_for_test(
        account,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: LimitPrice::Fixed(Money::from_cents(1_100)),
            qty: 100,
        },
        &mut events,
    );
    let ids: Vec<_> = session.markets[&code]
        .resting_orders_for(account)
        .iter()
        .map(|order| order.id)
        .collect();
    let plan = working_orders_plan(
        &session,
        account,
        vec![
            buy(&code, 900),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1_090)),
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
        ] if *first == LimitPrice::Fixed(Money::from_cents(1_090)) && *second == LimitPrice::Fixed(Money::from_cents(780))
    ));
}

#[test]
fn locked_auction_symbolic_crossing_uses_both_daily_boundaries() {
    let cases = [
        (Side::Buy, 900, Side::Sell, LimitPrice::Lowest, true),
        (Side::Buy, 1_100, Side::Sell, LimitPrice::Highest, true),
        (Side::Sell, 900, Side::Buy, LimitPrice::Lowest, true),
        (Side::Sell, 1_100, Side::Buy, LimitPrice::Highest, true),
        (Side::Buy, 900, Side::Sell, LimitPrice::Highest, false),
        (Side::Sell, 1_100, Side::Buy, LimitPrice::Lowest, false),
    ];
    for (resting_side, resting_price, new_side, new_price, crosses) in cases {
        let account = AccountId(1);
        let code = StockCode("600888".to_owned());
        let mut session = GameSession::new(npc_working_quote_tests::quote_setup(900), 93).unwrap();
        session.tick = 300;
        session.seed_auction_order_for_test(
            account,
            Intent::PlaceLimit {
                code: code.clone(),
                side: resting_side,
                price: LimitPrice::Fixed(Money::from_cents(resting_price)),
                qty: 100,
            },
            &mut Vec::new(),
        );
        let plan = working_orders_plan(
            &session,
            account,
            vec![Intent::PlaceLimit {
                code: code.clone(),
                side: new_side,
                price: new_price,
                qty: 100,
            }],
            TradingPhase::CallAuction,
        );
        assert!(plan.decisions.is_empty());
        assert_eq!(
            plan.residual_intents.is_empty(),
            crosses,
            "resting {resting_side:?} {resting_price}, new {new_side:?} {new_price:?}"
        );
    }
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

    let plan = working_orders_plan(
        &session,
        account,
        vec![buy(&code, 901)],
        TradingPhase::CallAuction,
    );

    assert_eq!(plan.decisions.len(), 1);
    assert_eq!(session.business_state_hash().unwrap(), business_before);
    assert_eq!(session.seq(), seq_before);
    assert_eq!(session.auction_orders, auction_before);
    assert_eq!(session.npc_order_lifecycles, lifecycle_before);
    assert_eq!(session.pending_player.len(), pending_before);
}
