use crate::{Money, PlanId, Side, StockCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationFunds {
    pub cash: Money,
    pub frozen_cash: Money,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationExperience {
    pub failure_influence: u16,
    pub long_stuck: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationClass {
    RiskReduction,
    ExistingPlan,
    NewOpportunity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationRequest {
    pub plan_id: PlanId,
    pub code: StockCode,
    pub side: Side,
    pub class: AllocationClass,
    pub confidence_bp: u32,
    /// Unsubmitted buy gross only. Live-order reservations belong exclusively to session.
    pub requested_cash: Money,
    /// Authoritative incremental fees for this unsubmitted portion, including minimum commission.
    pub fee_reserve: Money,
    pub requested_sell_qty: u32,
    pub sellable_qty: u32,
    pub experience: AllocationExperience,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationConstraint {
    InsufficientAvailableCash,
    InventoryUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationGrant {
    pub plan_id: PlanId,
    pub code: StockCode,
    pub allocated_cash: Money,
    pub constraint: Option<AllocationConstraint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationResult {
    pub grants: Vec<AllocationGrant>,
    pub available_cash: Money,
    pub total_allocated: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AllocationError {
    #[error("{field} cannot be negative, got {cents} cents")]
    NegativeMoney { field: &'static str, cents: i64 },
    #[error("frozen cash {frozen_cents} exceeds authoritative cash {cash_cents}")]
    FrozenCashExceedsCash { cash_cents: i64, frozen_cents: i64 },
    #[error("plan {plan_id:?} confidence {confidence_bp} exceeds 10000bp")]
    InvalidConfidence { plan_id: PlanId, confidence_bp: u32 },
    #[error("plan {plan_id:?} appears more than once in one allocation batch")]
    DuplicatePlanRequest { plan_id: PlanId },
    #[error("plan {plan_id:?} has an invalid request shape for side {side:?}")]
    InvalidRequestShape { plan_id: PlanId, side: Side },
    #[error("checked arithmetic overflow while allocating {step}")]
    ArithmeticOverflow { step: &'static str },
}
