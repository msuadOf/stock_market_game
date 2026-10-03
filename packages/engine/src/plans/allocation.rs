//! 以权威可用现金为基础，按账户为全部计划分配软预算。

mod experience;
mod types;

pub use experience::{read_allocation_experience, ExperienceHolding, ExperienceReadRequest};
pub use types::{
    AllocationClass, AllocationConstraint, AllocationError, AllocationExperience, AllocationFunds,
    AllocationGrant, AllocationRequest, AllocationResult,
};

use std::{cmp::Reverse, collections::BTreeSet};

use crate::{Money, Side};

/// Divides actual free cash among the strategy's requests, including buy fees.
/// The strategy decides how much cash to keep; this layer adds no cash floor.
pub fn allocate_soft_budgets(
    funds: &AllocationFunds,
    requests: &[AllocationRequest],
) -> Result<AllocationResult, AllocationError> {
    allocate_budgets(funds, requests, None)
}

pub(crate) fn allocate_child_quote_budgets(
    funds: &AllocationFunds,
    requests: &[AllocationRequest],
    minimum_commission: Money,
) -> Result<AllocationResult, AllocationError> {
    if minimum_commission < Money::ZERO {
        return Err(AllocationError::NegativeMoney {
            field: "minimum commission",
            cents: minimum_commission.cents(),
        });
    }
    allocate_budgets(funds, requests, Some(minimum_commission))
}

fn allocate_budgets(
    funds: &AllocationFunds,
    requests: &[AllocationRequest],
    minimum_buy_fee: Option<Money>,
) -> Result<AllocationResult, AllocationError> {
    validate_funds(funds)?;
    let available_cents = funds.cash.cents() - funds.frozen_cash.cents();
    let mut seen_plan_ids = BTreeSet::new();
    for request in requests {
        if !seen_plan_ids.insert(request.plan_id) {
            return Err(AllocationError::DuplicatePlanRequest {
                plan_id: request.plan_id,
            });
        }
        validate_request(request)?;
    }
    let mut ordered: Vec<&AllocationRequest> = requests.iter().collect();
    ordered.sort_by_key(|request| {
        (
            priority(request),
            Reverse(effective_confidence(request)),
            request.plan_id,
        )
    });
    let mut remaining = available_cents;
    let mut grants = Vec::with_capacity(ordered.len());
    for request in ordered {
        let (allocated, constraint) = match request.side {
            Side::Sell if request.requested_sell_qty > request.sellable_qty => {
                (0, Some(AllocationConstraint::InventoryUnavailable))
            }
            Side::Sell => {
                let fee = request.fee_reserve.cents().min(remaining);
                let reason = (fee < request.fee_reserve.cents())
                    .then_some(AllocationConstraint::InsufficientAvailableCash);
                (fee, reason)
            }
            Side::Buy => {
                let requested = request
                    .requested_cash
                    .cents()
                    .checked_add(request.fee_reserve.cents())
                    .ok_or(AllocationError::ArithmeticOverflow {
                        step: "request total",
                    })?;
                let candidate = requested.min(remaining);
                let fee_floor = minimum_buy_fee.unwrap_or(request.fee_reserve);
                let allocated = if candidate > fee_floor.cents() {
                    candidate
                } else {
                    0
                };
                let reason = if allocated == requested {
                    None
                } else {
                    Some(AllocationConstraint::InsufficientAvailableCash)
                };
                (allocated, reason)
            }
        };
        remaining -= allocated;
        grants.push(AllocationGrant {
            plan_id: request.plan_id,
            code: request.code.clone(),
            allocated_cash: Money::from_cents(allocated),
            constraint,
        });
    }
    let total_allocated = available_cents - remaining;
    Ok(AllocationResult {
        grants,
        available_cash: Money::from_cents(available_cents),
        total_allocated: Money::from_cents(total_allocated),
    })
}

fn priority(request: &AllocationRequest) -> u8 {
    if request.class == AllocationClass::RiskReduction
        || (request.side == Side::Sell && request.experience.long_stuck)
    {
        0
    } else {
        match request.class {
            AllocationClass::RiskReduction => 0,
            AllocationClass::ExistingPlan => 1,
            AllocationClass::NewOpportunity => 2,
        }
    }
}

fn effective_confidence(request: &AllocationRequest) -> u32 {
    request
        .confidence_bp
        .saturating_sub(u32::from(request.experience.failure_influence) * 1_000)
}

fn validate_funds(funds: &AllocationFunds) -> Result<(), AllocationError> {
    for (field, value) in [("cash", funds.cash), ("frozen cash", funds.frozen_cash)] {
        if value.cents() < 0 {
            return Err(AllocationError::NegativeMoney {
                field,
                cents: value.cents(),
            });
        }
    }
    if funds.frozen_cash > funds.cash {
        return Err(AllocationError::FrozenCashExceedsCash {
            cash_cents: funds.cash.cents(),
            frozen_cents: funds.frozen_cash.cents(),
        });
    }
    Ok(())
}

fn validate_request(request: &AllocationRequest) -> Result<(), AllocationError> {
    if request.confidence_bp > 10_000 {
        return Err(AllocationError::InvalidConfidence {
            plan_id: request.plan_id,
            confidence_bp: request.confidence_bp,
        });
    }
    for (field, value) in [
        ("requested cash", request.requested_cash),
        ("fee reserve", request.fee_reserve),
    ] {
        if value.cents() < 0 {
            return Err(AllocationError::NegativeMoney {
                field,
                cents: value.cents(),
            });
        }
    }
    let valid = match request.side {
        Side::Buy => {
            request.requested_cash.cents() > 0
                && request.requested_sell_qty == 0
                && request.sellable_qty == 0
        }
        Side::Sell => request.requested_cash == Money::ZERO && request.requested_sell_qty > 0,
    };
    if !valid {
        return Err(AllocationError::InvalidRequestShape {
            plan_id: request.plan_id,
            side: request.side,
        });
    }
    Ok(())
}
