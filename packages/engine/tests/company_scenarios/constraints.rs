use super::*;
use engine::plans::{
    allocate_soft_budgets, AllocationClass, AllocationExperience, AllocationFunds,
    AllocationPolicy, AllocationRequest, PlanBook,
};
use engine::strategy::Intent;
use engine::{AccountId, PlanId, RejectionReason, Side};

fn request(
    plan: u64,
    stock: &str,
    side: Side,
    requested_cash: i64,
    sellable_qty: u32,
) -> AllocationRequest {
    AllocationRequest {
        plan_id: PlanId(plan),
        code: code(stock),
        side,
        class: if side == Side::Sell {
            AllocationClass::RiskReduction
        } else {
            AllocationClass::NewOpportunity
        },
        confidence_bp: 9_000,
        requested_cash: Money::from_cents(requested_cash),
        fee_reserve: Money::from_cents(500),
        requested_sell_qty: if side == Side::Sell { 100 } else { 0 },
        sellable_qty,
        experience: AllocationExperience::default(),
    }
}

#[test]
fn unfilled_cross_stock_sale_proceeds_never_finance_oversubscribed_buys() {
    // Given: a real unfilled sale rests in a session while two buys oversubscribe authoritative cash.
    let mut game = crate::matching::matching_session(1_000);
    let sale_stock = code("600101");
    let mut plans = PlanBook::default();
    let sale_plan = crate::matching::plan(
        &mut plans,
        AccountId(1),
        sale_stock.clone(),
        Side::Sell,
        100,
    );
    game.execute_plan_observation(
        &mut plans,
        engine::session::PlanExecutionRequest {
            plan_id: sale_plan,
            allocation: engine::plans::AllocationGrant {
                plan_id: sale_plan,
                code: sale_stock.clone(),
                allocated_cash: Money::from_cents(500),
                constraint: None,
            },
            decision: engine::plans::quote_policy::QuoteDecision {
                action: engine::plans::quote_policy::QuoteAction::Submit {
                    price: Money::from_cents(1_100),
                    qty: 100,
                },
                reason: engine::plans::quote_policy::QuoteReason::UrgentProtectedLimit,
            },
            trading_day: 0,
        },
    )
    .unwrap();
    let authoritative = game.save();
    assert!(authoritative.resting_orders[&sale_stock]
        .iter()
        .any(|order| order.owner == AccountId(1) && order.side == Side::Sell));
    assert_eq!(
        authoritative.snapshot.accounts[&AccountId(1)].reserved_sell_qty[&sale_stock],
        100
    );
    let funds = AllocationFunds {
        cash: authoritative.snapshot.accounts[&AccountId(1)].cash,
        frozen_cash: authoritative.snapshot.accounts[&AccountId(1)].reserved_cash,
        equity: authoritative.snapshot.accounts[&AccountId(1)].cash,
    };
    let requests = [
        request(1, "600101", Side::Sell, 0, 100),
        request(2, "002156", Side::Buy, funds.cash.cents(), 0),
        request(3, "300260", Side::Buy, funds.cash.cents(), 0),
    ];

    // When: soft budgets allocate only existing cash and frozen resources.
    let result = allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default()).unwrap();

    // Then: sale contributes only its fee reserve and total allocation never exceeds free cash.
    assert_eq!(result.available_cash, funds.cash);
    assert!(result.total_allocated <= result.available_cash);
    assert_eq!(result.grants[0].allocated_cash, Money::from_cents(500));
    assert!(result.grants.iter().any(|grant| grant.constraint.is_some()));
    println!(
        "{{\"scenario\":\"cross_stock_budget\",\"available_cash\":{},\"allocated\":{}}}",
        result.available_cash.cents(),
        result.total_allocated.cents()
    );
}

#[test]
fn same_day_acquired_position_is_rejected_by_t1_without_state_repair() {
    // Given: a player acquires shares today through a real matched buy.
    let mut game = crate::matching::matching_session(1_000);
    let player = AccountId(0);
    let stock = code("600101");
    let mut plans = PlanBook::default();
    let seller = crate::matching::plan(&mut plans, AccountId(1), stock.clone(), Side::Sell, 100);
    game.execute_plan_observation(
        &mut plans,
        crate::matching::request(seller, stock.clone(), 100),
    )
    .unwrap();
    game.enqueue_player_intent(
        player,
        Intent::PlaceLimit {
            code: stock.clone(),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
        },
    )
    .unwrap();
    let mut trade_id = None;
    for _ in 0..TICKS_PER_DAY {
        let events = game.step();
        trade_id = events.iter().find_map(|event| match event {
            engine::Event::Trade {
                seq,
                taker,
                qty: 100,
                ..
            } if *taker == player => Some(*seq),
            _ => None,
        });
        if trade_id.is_some() {
            break;
        }
    }
    let position = &game.account(player).unwrap().positions[&stock];
    assert_eq!(position.qty, 100);
    assert_eq!(position.t1_locked, 100);
    assert_eq!(game.account(player).unwrap().sellable_qty(&stock), 0);
    game.enqueue_player_intent(
        player,
        Intent::PlaceLimit {
            code: stock.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_100),
            qty: 100,
        },
    )
    .unwrap();

    // When: the queued sell routes with zero sellable inventory.
    let events = game.step();

    // Then: T+1/availability is explicit and no order is accepted.
    assert!(events.iter().any(|event| matches!(event, engine::Event::IntentRejected { account, reason: RejectionReason::InsufficientShares, .. } if *account == player)));
    assert!(!events.iter().any(
        |event| matches!(event, engine::Event::OrderAccepted { account, .. } if *account == player)
    ));
    assert_eq!(game.account(player).unwrap().positions[&stock].qty, 100);
    println!(
        "{{\"scenario\":\"t1_real_fill\",\"trade_seq\":{},\"qty\":100,\"t1_locked\":100,\"sellable\":0}}",
        trade_id.unwrap()
    );
}

#[test]
fn malformed_controlled_fixture_is_rejected() {
    // Given / When: the checked-in fixture is changed to an invalid scalar type.
    let mut fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/company-model/task-28-scenario.json"
    ))
    .unwrap();
    fixture["matching"]["seller_shares"] = serde_json::json!("100");

    // Then: typed extraction fails rather than defaulting or coercing.
    assert!(serde_json::from_value::<ScenarioFixture>(fixture).is_err());
}
