use crate::{Money, PlanId, Side, StockCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationPolicy {
    pub version: u16,
    pub normal_cash_reserve_bp: u32,
    pub stressed_cash_reserve_bp: u32,
    pub risk_drawdown_threshold_bp: u32,
}

impl AllocationPolicy {
    pub fn new(
        version: u16,
        normal_cash_reserve_bp: u32,
        stressed_cash_reserve_bp: u32,
        risk_drawdown_threshold_bp: u32,
    ) -> Result<Self, AllocationError> {
        let policy = Self {
            version,
            normal_cash_reserve_bp,
            stressed_cash_reserve_bp,
            risk_drawdown_threshold_bp,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub(super) fn validate(self) -> Result<(), AllocationError> {
        if self.version != 1 {
            return Err(AllocationError::UnsupportedPolicyVersion {
                version: self.version,
            });
        }
        if self.normal_cash_reserve_bp > 10_000
            || self.stressed_cash_reserve_bp > 10_000
            || self.stressed_cash_reserve_bp < self.normal_cash_reserve_bp
            || self.risk_drawdown_threshold_bp > 10_000
        {
            return Err(AllocationError::InvalidPolicy);
        }
        Ok(())
    }
}

impl Default for AllocationPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            normal_cash_reserve_bp: 1_000,
            stressed_cash_reserve_bp: 3_000,
            risk_drawdown_threshold_bp: 2_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationFunds {
    pub cash: Money,
    pub frozen_cash: Money,
    pub equity: Money,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationExperience {
    pub failure_influence: u16,
    pub long_stuck: bool,
    pub drawdown_bp: Option<u32>,
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
    CashReserve,
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
    pub cash_reserve: Money,
    pub total_allocated: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AllocationError {
    #[error("allocation policy version {version} is unsupported")]
    UnsupportedPolicyVersion { version: u16 },
    #[error("allocation policy basis-point fields are inconsistent")]
    InvalidPolicy,
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
    #[error("plan {plan_id:?} fee reserve {fee_cents} exceeds available cash {available_cents}")]
    FeeReserveExceedsAvailableCash {
        plan_id: PlanId,
        fee_cents: i64,
        available_cents: i64,
    },
    #[error("checked arithmetic overflow while allocating {step}")]
    ArithmeticOverflow { step: &'static str },
}
