//! Task 22 QA: account-local candidate scoring and soft-budget allocation.

mod failures;
mod gold;

use engine::plans::{
    AllocationClass, AllocationExperience, AllocationRequest, SignalContribution, SignalScore,
};
use engine::{Money, PlanId, Side, StockCode};

pub(crate) fn money(cents: i64) -> Money {
    Money::from_cents(cents)
}

pub(crate) fn code(value: &str) -> StockCode {
    StockCode(value.into())
}

pub(crate) fn available(score: i32) -> SignalContribution {
    SignalContribution::available(SignalScore::new(score).unwrap())
}

pub(crate) fn buy_request(
    plan: u64,
    stock: &str,
    class: AllocationClass,
    confidence_bp: u32,
    requested_cents: i64,
) -> AllocationRequest {
    AllocationRequest {
        plan_id: PlanId(plan),
        code: code(stock),
        side: Side::Buy,
        class,
        confidence_bp,
        requested_cash: money(requested_cents),
        fee_reserve: money(500),
        requested_sell_qty: 0,
        sellable_qty: 0,
        experience: AllocationExperience::default(),
    }
}
