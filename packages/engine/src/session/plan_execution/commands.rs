use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::session) enum PlanCancelCause {
    Restructure,
    Explicit,
    Replace,
    ConflictingWorkingOrder,
}

#[cfg(feature = "simulation-diagnostics")]
impl PlanCancelCause {
    pub(in crate::session) const fn causal_termination(
        self,
    ) -> crate::diagnostics::causal::Termination {
        match self {
            Self::Replace | Self::ConflictingWorkingOrder => {
                crate::diagnostics::causal::Termination::Reprice
            }
            Self::Restructure | Self::Explicit => {
                crate::diagnostics::causal::Termination::Voluntary
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::session) enum PlanRouteCommand {
    Cancel {
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        cause: PlanCancelCause,
    },
    SubmitLimit {
        account: AccountId,
        code: StockCode,
        side: Side,
        price: Money,
        qty: u32,
    },
}

pub(in crate::session) enum PlanRouteOutcome {
    Accepted(OrderId),
    Canceled(OrderId),
    Rejected(RejectionReason),
}

impl PlanRouteOutcome {
    pub(super) fn failure(self) -> Result<PlanExecutionDisposition, PlanExecutionError> {
        match self {
            Self::Rejected(reason) => Ok(PlanExecutionDisposition::RouteRejected { reason }),
            Self::Accepted(_) | Self::Canceled(_) => Err(PlanExecutionError::InvalidRouteOutcome),
        }
    }
}
