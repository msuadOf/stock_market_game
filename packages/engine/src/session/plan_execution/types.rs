//! Public adapter contract and private lifecycle facts.

use super::*;
use crate::plans::{PlanError, PlanStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanExecutionRequest {
    pub plan_id: PlanId,
    pub allocation: AllocationGrant,
    pub decision: QuoteDecision,
    pub trading_day: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlanExecutionDisposition {
    Waiting {
        reason: QuoteReason,
    },
    Kept {
        order_id: OrderId,
        reason: QuoteReason,
    },
    Adopted {
        order_id: OrderId,
        reason: QuoteReason,
    },
    Submitted {
        order_id: OrderId,
        reason: QuoteReason,
    },
    Canceled {
        order_id: OrderId,
        reason: QuoteReason,
    },
    Replaced {
        canceled_order_id: OrderId,
        order_id: OrderId,
        reason: QuoteReason,
    },
    PendingReconsideration {
        order_id: OrderId,
        reason: QuoteReason,
    },
    RemainingBelowBoardLot {
        remaining_qty: u32,
    },
    RouteRejected {
        reason: RejectionReason,
    },
    SettlementFailed {
        reason: String,
    },
}

#[derive(Clone, Debug)]
pub struct PlanExecutionReport {
    pub disposition: PlanExecutionDisposition,
    pub events: Vec<Event>,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanExecutionError {
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error("plan {plan_id:?} references unknown account {account:?}")]
    UnknownAccount { plan_id: PlanId, account: AccountId },
    #[error("plan {plan_id:?} references unknown stock {code:?}")]
    UnknownStock { plan_id: PlanId, code: StockCode },
    #[error(
        "plan {plan_id:?} cannot execute on session day {session_day} using day {request_day}"
    )]
    TradingDayMismatch {
        plan_id: PlanId,
        session_day: u64,
        request_day: u64,
    },
    #[error("plan {plan_id:?} allocation belongs to plan {allocation_plan_id:?} stock {allocation_code:?}")]
    AllocationMismatch {
        plan_id: PlanId,
        allocation_plan_id: PlanId,
        allocation_code: StockCode,
    },
    #[error("plan {plan_id:?} has a position-fraction target that was not converted to shares")]
    UnconvertedFractionTarget { plan_id: PlanId },
    #[error("plan {plan_id:?} is {status:?} and cannot submit or replace a child")]
    PlanCannotSubmit { plan_id: PlanId, status: PlanStatus },
    #[error(
        "plan {plan_id:?} requested {requested_qty} shares with only {remaining_qty} remaining"
    )]
    QuantityExceedsRemaining {
        plan_id: PlanId,
        requested_qty: u32,
        remaining_qty: u32,
    },
    #[error("plan {plan_id:?} requested an invalid zero-sized child")]
    InvalidChildQuantity { plan_id: PlanId },
    #[error(
        "plan {plan_id:?} allocation {allocated_cents} cents cannot reserve {required_cents} cents"
    )]
    AllocationInsufficient {
        plan_id: PlanId,
        allocated_cents: i64,
        required_cents: i64,
    },
    #[error(
        "plan {plan_id:?} action references child {requested:?}, but active child is {active:?}"
    )]
    ChildMismatch {
        plan_id: PlanId,
        requested: OrderId,
        active: Option<OrderId>,
    },
    #[error("plan {plan_id:?} already has incompatible execution state")]
    IncompatibleExecutionState { plan_id: PlanId },
}

#[derive(Clone, Copy, Debug)]
pub(in crate::session) enum PendingPlanEvent {
    Accepted {
        plan_id: PlanId,
        order_id: OrderId,
        trading_day: u64,
    },
    Filled {
        plan_id: PlanId,
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    },
    DayEnded {
        plan_id: PlanId,
        trading_day: u64,
    },
}

#[derive(Clone, Copy)]
pub(super) struct NewChildSpec {
    pub(super) price: Money,
    pub(super) qty: u32,
    pub(super) remaining: u32,
    pub(super) reason: QuoteReason,
}
