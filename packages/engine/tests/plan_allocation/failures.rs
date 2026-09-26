use engine::plans::{
    allocate_soft_budgets, blend_candidate, AllocationClass, AllocationConstraint, AllocationError,
    AllocationFunds, CandidateAssessment, CandidateError, CandidateSignals, SignalContribution,
    SignalUnavailableReason,
};
use engine::strategy::AnalysisWeights;
use engine::{Money, Side};

use super::{available, buy_request, money};

#[test]
fn all_unavailable_is_insufficient_information_not_a_zero_sell_signal() {
    // Given: every non-zero-weight dimension is unavailable.
    let weights = AnalysisWeights::new(2_000, 2_000, 2_000, 2_000, 2_000).unwrap();
    let unavailable =
        || SignalContribution::unavailable(SignalUnavailableReason::MissingObservation);
    let signals = CandidateSignals {
        fundamental: unavailable(),
        trend: unavailable(),
        price_volume: unavailable(),
        technical: unavailable(),
        experience: unavailable(),
    };

    // When / Then: no direction score is fabricated.
    assert!(matches!(
        blend_candidate(&weights, &signals),
        CandidateAssessment::InsufficientInformation { excluded } if excluded.len() == 5
    ));
}

#[test]
fn pending_sell_proceeds_never_fund_a_buy() {
    // Given: cash only covers a buy's fees, a pending sell, and a buy opportunity.
    let funds = AllocationFunds {
        cash: money(500),
        frozen_cash: Money::ZERO,
    };
    let mut pending_sell = buy_request(1, "600101", AllocationClass::RiskReduction, 9_000, 0);
    pending_sell.side = Side::Sell;
    pending_sell.fee_reserve = Money::ZERO;
    pending_sell.requested_sell_qty = 100;
    pending_sell.sellable_qty = 100;
    let buy = buy_request(2, "002156", AllocationClass::NewOpportunity, 9_000, 20_000);

    // When: allocation sees no hypothetical sale proceeds field.
    let result = allocate_soft_budgets(&funds, &[pending_sell, buy]).unwrap();

    // Then: the sell needs no cash reserve and its future proceeds cannot fund the buy.
    assert_eq!(result.total_allocated, Money::ZERO);
    assert_eq!(result.grants[0].constraint, None);
    assert_eq!(result.grants[1].allocated_cash, Money::ZERO);
    assert_eq!(
        result.grants[1].constraint,
        Some(AllocationConstraint::InsufficientAvailableCash)
    );
}

#[test]
fn inconsistent_frozen_cash_is_rejected() {
    // Given: reservations claim more cash than the authoritative account owns.
    let funds = AllocationFunds {
        cash: money(1_000),
        frozen_cash: money(1_001),
    };

    // When / Then: the invariant breach is typed, never saturated to zero.
    assert!(matches!(
        allocate_soft_budgets(&funds, &[]),
        Err(AllocationError::FrozenCashExceedsCash {
            cash_cents: 1_000,
            frozen_cents: 1_001
        })
    ));
}

#[test]
fn unaffordable_buy_fee_leaves_zero_budget_without_aborting_other_plans() {
    // Given: the buy's minimum commission exceeds cash; a sell needs no cash reserve.
    let funds = AllocationFunds {
        cash: money(400),
        frozen_cash: Money::ZERO,
    };
    let request = buy_request(1, "600101", AllocationClass::NewOpportunity, 9_000, 10_000);
    let mut sell = buy_request(2, "002156", AllocationClass::RiskReduction, 9_000, 0);
    sell.side = Side::Sell;
    sell.fee_reserve = Money::ZERO;
    sell.requested_sell_qty = 100;
    sell.sellable_qty = 100;

    let result = allocate_soft_budgets(&funds, &[request, sell])
        .expect("insufficient cash is a normal budget constraint");

    assert_eq!(result.total_allocated, Money::ZERO);
    assert_eq!(result.grants[0].plan_id.0, 2);
    assert_eq!(result.grants[0].constraint, None);
    assert_eq!(result.grants[1].plan_id.0, 1);
    assert_eq!(result.grants[1].allocated_cash, Money::ZERO);
    assert_eq!(
        result.grants[1].constraint,
        Some(AllocationConstraint::InsufficientAvailableCash)
    );
}

#[test]
fn unsellable_inventory_is_rejected_even_without_a_cash_reserve() {
    // Given: a risk exit requests T+1-locked inventory.
    let funds = AllocationFunds {
        cash: money(5_000),
        frozen_cash: Money::ZERO,
    };
    let mut request = buy_request(1, "600101", AllocationClass::RiskReduction, 9_000, 0);
    request.side = Side::Sell;
    request.fee_reserve = Money::ZERO;
    request.requested_sell_qty = 100;
    request.sellable_qty = 0;

    // When / Then: the reason remains risk, but executable allocation is explicitly zero.
    let result = allocate_soft_budgets(&funds, &[request]).unwrap();
    assert_eq!(result.total_allocated, Money::ZERO);
    assert_eq!(result.grants[0].allocated_cash, Money::ZERO);
    assert_eq!(
        result.grants[0].constraint,
        Some(AllocationConstraint::InventoryUnavailable)
    );
}

#[test]
fn invalid_signal_score_is_rejected_instead_of_clamped_at_boundary() {
    // Given / When / Then: only N(x,t) may mathematically clamp; public score construction validates.
    assert!(engine::plans::SignalScore::new(10_001).is_err());
    assert!(engine::plans::SignalScore::new(-10_001).is_err());
    assert_eq!(available(10_000).score().unwrap().value(), 10_000);
}

#[test]
fn target_share_quantity_overflow_is_rejected_instead_of_capped() {
    // Given: a valid monetary input whose one-cent share count exceeds u32.
    let equity = money(i64::MAX);

    // When / Then: quantity conversion reports overflow rather than silently truncating the target.
    assert!(matches!(
        engine::plans::target_share_quantity(10_000, equity, money(1), 100),
        Err(CandidateError::ArithmeticOverflow {
            step: "target share quantity"
        })
    ));
}

#[test]
fn duplicate_plan_requests_are_rejected_before_order_can_affect_allocation() {
    // Given: two differently sized requests that claim the same stable plan identity.
    let funds = AllocationFunds {
        cash: money(100_000),
        frozen_cash: Money::ZERO,
    };
    let requests = [
        buy_request(7, "600101", AllocationClass::ExistingPlan, 8_000, 80_000),
        buy_request(7, "002156", AllocationClass::ExistingPlan, 8_000, 20_000),
    ];

    // When / Then: invalid duplicate identity is explicit rather than caller-order-dependent.
    assert!(matches!(
        allocate_soft_budgets(&funds, &requests),
        Err(AllocationError::DuplicatePlanRequest { plan_id }) if plan_id.0 == 7
    ));
}

#[test]
fn non_a_share_board_lot_is_rejected() {
    // Given / When / Then: task 22's public target conversion cannot opt out of the A-share lot.
    assert!(matches!(
        engine::plans::target_share_quantity(2_000, money(100_000), money(100), 1),
        Err(CandidateError::InvalidBoardLotSize { lot_size: 1 })
    ));
}
