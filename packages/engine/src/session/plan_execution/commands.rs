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

pub(in crate::session) struct YieldedPlanCommand {
    pub command: PlanRouteCommand,
}

impl YieldedPlanCommand {
    pub(in crate::session) fn new(
        command: PlanRouteCommand,
        next_ordinal: &mut u64,
    ) -> Result<Self, PlanExecutionError> {
        *next_ordinal = next_ordinal
            .checked_add(1)
            .ok_or(PlanExecutionError::CommandOrdinalOverflow)?;
        Ok(Self { command })
    }
}

pub(in crate::session) enum PlanRouteOutcome {
    Accepted(OrderId),
    Canceled(OrderId),
    Rejected(RejectionReason),
    SettlementFailed(String),
    ResourceLimited,
}

impl PlanRouteOutcome {
    pub(super) fn failure(self) -> Result<PlanExecutionDisposition, PlanExecutionError> {
        match self {
            Self::Rejected(reason) => Ok(PlanExecutionDisposition::RouteRejected { reason }),
            Self::SettlementFailed(reason) => {
                Ok(PlanExecutionDisposition::SettlementFailed { reason })
            }
            Self::ResourceLimited => Ok(PlanExecutionDisposition::SettlementFailed {
                reason: "pending plan event capacity exhausted".to_string(),
            }),
            Self::Accepted(_) | Self::Canceled(_) => Err(PlanExecutionError::InvalidRouteOutcome),
        }
    }
}

impl GameSession {
    pub(in crate::session) fn consume_plan_route_command(
        &mut self,
        command: PlanRouteCommand,
        events: &mut Vec<Event>,
    ) -> Result<PlanRouteOutcome, PlanExecutionError> {
        let (account, code, expected_cancel, intent, reprice) = match command {
            PlanRouteCommand::Cancel {
                account,
                code,
                order_id,
                cause,
            } => {
                let reprice = match cause {
                    PlanCancelCause::Replace | PlanCancelCause::ConflictingWorkingOrder => true,
                    PlanCancelCause::Restructure | PlanCancelCause::Explicit => false,
                };
                (
                    account,
                    code.clone(),
                    Some(order_id),
                    Intent::Cancel { code, id: order_id },
                    reprice,
                )
            }
            PlanRouteCommand::SubmitLimit {
                account,
                code,
                side,
                price,
                qty,
            } => (
                account,
                code.clone(),
                None,
                Intent::PlaceLimit {
                    code,
                    side,
                    price,
                    qty,
                },
                false,
            ),
        };
        #[cfg(feature = "simulation-diagnostics")]
        if reprice {
            self.causal.termination = Some(crate::diagnostics::causal::Termination::Reprice);
        }
        #[cfg(not(feature = "simulation-diagnostics"))]
        let _ = reprice;
        let event_start = events.len();
        let pending_start = self.pending_plan_events.len();
        let submitted_id = OrderId(self.next_order_id);
        self.route_plan_intent(account, intent, events);
        #[cfg(feature = "simulation-diagnostics")]
        if reprice {
            self.causal.termination = None;
        }
        let outcome =
            Self::plan_route_outcome(&events[event_start..], account, &code, expected_cancel);
        if expected_cancel.is_none()
            && matches!(outcome, Err(PlanExecutionError::InvalidRouteOutcome))
            && !events[event_start..].is_empty()
            && events[event_start..].iter().all(|event| matches!(event, Event::Trade { code: traded, taker, .. } if traded == &code && *taker == account))
            && self.pending_plan_events[pending_start..].iter().filter(|event| matches!(event, PendingPlanEvent::Accepted { order_id, .. } if *order_id == submitted_id)).count() == 1
        {
            return Ok(PlanRouteOutcome::Accepted(submitted_id));
        }
        outcome
    }

    pub(super) fn plan_route_outcome(
        events: &[Event],
        owner: AccountId,
        stock: &StockCode,
        expected_cancel: Option<OrderId>,
    ) -> Result<PlanRouteOutcome, PlanExecutionError> {
        let mut terminal = None;
        for event in events {
            let outcome = match event {
                Event::OrderAccepted {
                    account, code, id, ..
                } if *account == owner && code == stock && expected_cancel.is_none() => {
                    Some(PlanRouteOutcome::Accepted(*id))
                }
                Event::OrderCanceled {
                    account, code, id, ..
                } if *account == owner && code == stock && expected_cancel == Some(*id) => {
                    Some(PlanRouteOutcome::Canceled(*id))
                }
                Event::IntentRejected {
                    account,
                    code,
                    reason,
                    ..
                } if *account == owner && code == stock => {
                    Some(PlanRouteOutcome::Rejected(reason.clone()))
                }
                Event::SettlementError { reason, .. } => {
                    Some(PlanRouteOutcome::SettlementFailed(reason.clone()))
                }
                Event::ResourceLimit { .. } => Some(PlanRouteOutcome::ResourceLimited),
                Event::OrderAccepted { .. }
                | Event::OrderCanceled { .. }
                | Event::IntentRejected { .. } => {
                    return Err(PlanExecutionError::InvalidRouteOutcome);
                }
                Event::Trade { .. }
                | Event::AuctionTick { .. }
                | Event::AuctionCompleted { .. }
                | Event::PriceTick { .. }
                | Event::DayBoundary { .. }
                | Event::CivilDateAdvanced { .. }
                | Event::CompanyDisclosurePublished { .. } => None,
            };
            if let Some(outcome) = outcome {
                if terminal.replace(outcome).is_some() {
                    return Err(PlanExecutionError::InvalidRouteOutcome);
                }
            }
        }
        terminal.ok_or(PlanExecutionError::InvalidRouteOutcome)
    }
}
