use std::collections::BTreeSet;

use engine::calendar::CivilDate;
use engine::experience::{ExperienceMoment, PersonalWatchlist};
use engine::plans::{
    allocate_soft_budgets, eligible_candidates, read_allocation_experience,
    target_position_weight_bp, target_share_quantity, AllocationClass, AllocationFunds,
    AllocationPolicy, ExperienceHolding, ExperienceReadRequest, QuantityRounding, SignalScore,
};
use engine::{Money, RetailExperienceState, Side};

use super::super::{buy_request, code, money};

#[test]
fn target_weight_uses_k5a_integer_formula_and_clamps() {
    // Given / When / Then: 2000 + 5000/10000 * 6000/4 = 2750bp.
    assert_eq!(
        target_position_weight_bp(2_000, SignalScore::new(5_000).unwrap(), 6_000).unwrap(),
        2_750
    );
    assert_eq!(
        target_position_weight_bp(5_900, SignalScore::new(10_000).unwrap(), 6_000).unwrap(),
        6_000
    );
    assert_eq!(
        target_position_weight_bp(100, SignalScore::new(-10_000).unwrap(), 6_000).unwrap(),
        0
    );
}

#[test]
fn target_share_conversion_records_board_lot_rounding() {
    // Given: 2750bp of 100000 cents at 101 cents/share gives raw 272 shares.
    let target = target_share_quantity(2_750, money(100_000), money(101), 100).unwrap();

    // When / Then: buying rounds down, never expands the target to another board lot.
    assert_eq!(target.target_qty, 200);
    assert_eq!(
        target.rounding,
        QuantityRounding::BoardLotDown { raw_qty: 272 }
    );
}

#[test]
fn experience_reads_feed_failure_patience_and_risk_inputs() {
    // Given: a 20-day losing holding and account equity 30% below its observed peak.
    let stock = code("600101");
    let entry = ExperienceMoment {
        civil_date: CivilDate::from_ymd(2030, 1, 1).unwrap(),
        market_minute: 1,
        trading_day: 0,
    };
    let as_of = ExperienceMoment {
        civil_date: CivilDate::from_ymd(2030, 2, 1).unwrap(),
        market_minute: 2_000,
        trading_day: 20,
    };
    let mut experience = RetailExperienceState::new(money(100_000)).unwrap();
    experience
        .initialize_holding_dated(&stock, Some(money(1_000)), money(900), entry)
        .unwrap();

    // When: task-22 consumes all three task-20 read seams.
    let inputs = read_allocation_experience(&ExperienceReadRequest {
        experience: &experience,
        as_of: &as_of,
        equity: money(70_000),
        holding: Some(ExperienceHolding {
            code: &stock,
            cost: money(1_000),
        }),
    })
    .unwrap();

    // Then: integer inputs are ready for confidence, priority, and reserve policy.
    assert_eq!(inputs.failure_influence, 0);
    assert!(inputs.long_stuck);
    assert_eq!(inputs.drawdown_bp, Some(3_000));
}

#[test]
fn candidate_scope_is_exact_union_of_own_sources() {
    // Given: one holding, one watch entry, one discovery, and an unrelated market stock.
    let held = BTreeSet::from([code("600101")]);
    let mut watchlist = PersonalWatchlist::new();
    watchlist.record_attention(&code("002156"), 10, 10).unwrap();
    let discovered = BTreeSet::from([code("300260")]);

    // When: candidates are formed without a market-universe parameter.
    let candidates = eligible_candidates(&held, &watchlist, &discovered);

    // Then: only the three personally scoped sources are present.
    assert_eq!(
        candidates,
        BTreeSet::from([code("002156"), code("300260"), code("600101")])
    );
    assert!(!candidates.contains(&code("600610")));
}

#[test]
fn two_competing_stocks_cannot_overcommit_available_cash() {
    // Given: 100000 cash, 10000 reserve, two plans each request 80000 plus commission.
    let funds = AllocationFunds {
        cash: money(100_000),
        frozen_cash: Money::ZERO,
        equity: money(100_000),
    };
    let requests = [
        buy_request(2, "600101", AllocationClass::NewOpportunity, 8_000, 80_000),
        buy_request(1, "002156", AllocationClass::NewOpportunity, 8_000, 80_000),
    ];

    // When: same-confidence requests use stable PlanId ordering.
    let result = allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default()).unwrap();

    // Then: total is capped at 90000; PlanId(1) wins the first full request.
    assert_eq!(result.total_allocated.cents(), 90_000);
    assert_eq!(result.grants[0].plan_id.0, 1);
    assert_eq!(result.grants[0].allocated_cash.cents(), 80_500);
    assert_eq!(result.grants[1].allocated_cash.cents(), 9_500);
}

#[test]
fn allocation_is_invariant_to_request_order() {
    // Given: requests with distinct priorities, confidence, and stable plan ids.
    let funds = AllocationFunds {
        cash: money(100_000),
        frozen_cash: money(10_000),
        equity: money(100_000),
    };
    let requests = vec![
        buy_request(3, "600101", AllocationClass::NewOpportunity, 9_000, 40_000),
        buy_request(2, "002156", AllocationClass::ExistingPlan, 4_000, 40_000),
        buy_request(1, "300260", AllocationClass::ExistingPlan, 8_000, 40_000),
    ];
    let mut reversed = requests.clone();
    reversed.reverse();

    // When: the same account-local requests arrive in opposite collection order.
    let forward = allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default()).unwrap();
    let backward = allocate_soft_budgets(&funds, &reversed, &AllocationPolicy::default()).unwrap();

    // Then: deterministic priority sorting produces byte-for-byte equal grants.
    assert_eq!(forward, backward);
}

#[test]
fn risk_exit_fee_is_allocated_before_continuation_and_new_opportunity() {
    // Given: scarce cash and three classes; risk sell needs only its authoritative fee reserve.
    let funds = AllocationFunds {
        cash: money(10_000),
        frozen_cash: Money::ZERO,
        equity: money(50_000),
    };
    let mut risk = buy_request(9, "600101", AllocationClass::RiskReduction, 1_000, 0);
    risk.side = Side::Sell;
    risk.requested_sell_qty = 100;
    risk.sellable_qty = 100;
    let requests = [
        buy_request(1, "002156", AllocationClass::NewOpportunity, 9_000, 8_000),
        buy_request(2, "300260", AllocationClass::ExistingPlan, 2_000, 8_000),
        risk,
    ];

    // When / Then: risk, continuation, new is the allocation order.
    let result = allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default()).unwrap();
    assert_eq!(
        result
            .grants
            .iter()
            .map(|grant| grant.plan_id.0)
            .collect::<Vec<_>>(),
        vec![9, 2, 1]
    );
    assert_eq!(result.grants[0].allocated_cash.cents(), 500);
}

#[test]
fn cash_reserve_can_prevent_buy_despite_cheap_valuation() {
    // Given: cash equals the default 10% equity reserve.
    let funds = AllocationFunds {
        cash: money(10_000),
        frozen_cash: Money::ZERO,
        equity: money(100_000),
    };
    let requests = [buy_request(
        1,
        "600101",
        AllocationClass::NewOpportunity,
        10_000,
        5_000,
    )];

    // When / Then: valuation confidence cannot spend the cash reserve.
    let result = allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default()).unwrap();
    assert_eq!(result.total_allocated, Money::ZERO);
    assert_eq!(result.cash_reserve, money(10_000));
}

#[test]
fn cash_reserve_uses_half_even_at_exact_cent_ties() {
    // Given / When / Then: 10% of 5 cents is 0.5 -> 0; 10% of 15 cents is 1.5 -> 2.
    let policy = AllocationPolicy::default();
    let low = allocate_soft_budgets(
        &AllocationFunds {
            cash: money(5),
            frozen_cash: Money::ZERO,
            equity: money(5),
        },
        &[],
        &policy,
    )
    .unwrap();
    let high = allocate_soft_budgets(
        &AllocationFunds {
            cash: money(15),
            frozen_cash: Money::ZERO,
            equity: money(15),
        },
        &[],
        &policy,
    )
    .unwrap();
    assert_eq!(low.cash_reserve, Money::ZERO);
    assert_eq!(high.cash_reserve, money(2));
}

#[test]
fn partial_fill_is_reflected_only_by_next_authoritative_snapshot() {
    // Given: first allocation reserves an 80500 plan budget.
    let policy = AllocationPolicy::default();
    let first = allocate_soft_budgets(
        &AllocationFunds {
            cash: money(100_000),
            frozen_cash: Money::ZERO,
            equity: money(100_000),
        },
        &[buy_request(
            1,
            "600101",
            AllocationClass::ExistingPlan,
            8_000,
            80_000,
        )],
        &policy,
    )
    .unwrap();
    assert_eq!(first.total_allocated.cents(), 80_500);

    // When: a real partial fill reduced cash and the remaining live order freezes 30000.
    let second = allocate_soft_budgets(
        &AllocationFunds {
            cash: money(60_000),
            frozen_cash: money(30_000),
            equity: money(100_000),
        },
        &[buy_request(
            1,
            "600101",
            AllocationClass::ExistingPlan,
            8_000,
            40_000,
        )],
        &policy,
    )
    .unwrap();

    // Then: only 20000 beyond reserve remains; frozen cash is deducted once, not twice.
    assert_eq!(second.total_allocated.cents(), 20_000);
}
