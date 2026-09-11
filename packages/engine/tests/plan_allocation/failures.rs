use engine::plans::{
    allocate_soft_budgets, blend_candidate, AllocationClass, AllocationError, AllocationFunds,
    AllocationPolicy, CandidateAssessment, CandidateError, CandidateSignals, SignalContribution,
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
    // Given: no free cash, a pending sell, and a buy opportunity.
    let funds = AllocationFunds {
        cash: money(500),
        frozen_cash: Money::ZERO,
        equity: money(100_000),
    };
    let mut pending_sell = buy_request(1, "600101", AllocationClass::RiskReduction, 9_000, 0);
    pending_sell.side = Side::Sell;
    pending_sell.requested_sell_qty = 100;
    pending_sell.sellable_qty = 100;
    let buy = buy_request(2, "002156", AllocationClass::NewOpportunity, 9_000, 20_000);

    // When: allocation sees no hypothetical sale proceeds field.
    let result =
        allocate_soft_budgets(&funds, &[pending_sell, buy], &AllocationPolicy::default()).unwrap();

    // Then: cash pays the sell fee only; the buy receives nothing.
    assert_eq!(result.total_allocated.cents(), 500);
    assert_eq!(result.grants[1].allocated_cash, Money::ZERO);
}

#[test]
fn inconsistent_frozen_cash_is_rejected() {
    // Given: reservations claim more cash than the authoritative account owns.
    let funds = AllocationFunds {
        cash: money(1_000),
        frozen_cash: money(1_001),
        equity: money(1_000),
    };

    // When / Then: the invariant breach is typed, never saturated to zero.
    assert!(matches!(
        allocate_soft_budgets(&funds, &[], &AllocationPolicy::default()),
        Err(AllocationError::FrozenCashExceedsCash {
            cash_cents: 1_000,
            frozen_cents: 1_001
        })
    ));
}

#[test]
fn fee_reserve_larger_than_authoritative_cash_is_rejected() {
    // Given: one request whose minimum commission alone exceeds cash.
    let funds = AllocationFunds {
        cash: money(400),
        frozen_cash: Money::ZERO,
        equity: money(10_000),
    };
    let request = buy_request(1, "600101", AllocationClass::NewOpportunity, 9_000, 10_000);

    // When / Then: fees cannot be silently dropped to make the order look affordable.
    assert!(matches!(
        allocate_soft_budgets(&funds, &[request], &AllocationPolicy::default()),
        Err(AllocationError::FeeReserveExceedsAvailableCash { plan_id, fee_cents: 500, available_cents: 400 }) if plan_id.0 == 1
    ));
}

#[test]
fn unsellable_inventory_receives_no_soft_fee_budget() {
    // Given: a risk exit requests T+1-locked inventory.
    let funds = AllocationFunds {
        cash: money(5_000),
        frozen_cash: Money::ZERO,
        equity: money(10_000),
    };
    let mut request = buy_request(1, "600101", AllocationClass::RiskReduction, 9_000, 0);
    request.side = Side::Sell;
    request.requested_sell_qty = 100;
    request.sellable_qty = 0;

    // When / Then: the reason remains risk, but executable allocation is explicitly zero.
    let result = allocate_soft_budgets(&funds, &[request], &AllocationPolicy::default()).unwrap();
    assert_eq!(result.total_allocated, Money::ZERO);
    assert_eq!(result.grants[0].allocated_cash, Money::ZERO);
    assert!(result.grants[0].constraint.is_some());
}

#[test]
fn invalid_signal_score_is_rejected_instead_of_clamped_at_boundary() {
    // Given / When / Then: only N(x,t) may mathematically clamp; public score construction validates.
    assert!(engine::plans::SignalScore::new(10_001).is_err());
    assert!(engine::plans::SignalScore::new(-10_001).is_err());
    assert_eq!(available(10_000).score().unwrap().value(), 10_000);
}

#[test]
fn unknown_allocation_policy_version_is_rejected_on_restore_and_use() {
    // Given: a persisted policy from an unsupported future schema.
    let json = r#"{"version":2,"normal_cash_reserve_bp":1000,"stressed_cash_reserve_bp":3000,"risk_drawdown_threshold_bp":2000}"#;
    let policy: AllocationPolicy = serde_json::from_str(json).unwrap();

    // When / Then: use revalidates serde data and rejects the unknown version.
    assert!(matches!(
        allocate_soft_budgets(
            &AllocationFunds {
                cash: money(1_000),
                frozen_cash: Money::ZERO,
                equity: money(1_000)
            },
            &[],
            &policy,
        ),
        Err(AllocationError::UnsupportedPolicyVersion { version: 2 })
    ));
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
        equity: money(100_000),
    };
    let requests = [
        buy_request(7, "600101", AllocationClass::ExistingPlan, 8_000, 80_000),
        buy_request(7, "002156", AllocationClass::ExistingPlan, 8_000, 20_000),
    ];

    // When / Then: invalid duplicate identity is explicit rather than caller-order-dependent.
    assert!(matches!(
        allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default()),
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
