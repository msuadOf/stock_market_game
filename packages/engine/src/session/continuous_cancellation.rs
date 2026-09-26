use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContinuousCancellationCause {
    Voluntary,
    #[cfg(feature = "simulation-diagnostics")]
    Reprice,
    Expired,
    #[cfg(feature = "simulation-diagnostics")]
    DayEnd,
    #[cfg(feature = "simulation-diagnostics")]
    MarketRemainder,
    #[cfg(feature = "simulation-diagnostics")]
    Aborted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ContinuousCancellationFact {
    pub(super) account: AccountId,
    pub(super) code: StockCode,
    pub(super) order_id: OrderId,
    pub(super) side: Side,
    pub(super) remaining_qty: u32,
}

#[derive(Debug)]
pub(super) enum ContinuousCancellationError {
    UnknownStock,
    OrderNotFound,
    OrderAlreadyFilled,
    NotOrderOwner,
    Market(MarketError),
}

impl GameSession {
    pub(super) fn cancel_continuous_order_state_only(
        &mut self,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        cause: ContinuousCancellationCause,
    ) -> Result<ContinuousCancellationFact, ContinuousCancellationError> {
        #[cfg(not(feature = "simulation-diagnostics"))]
        let _ = cause;
        let market = self
            .markets
            .get(&code)
            .ok_or(ContinuousCancellationError::UnknownStock)?;
        let mut candidate_market = market.clone();
        let order = match candidate_market.cancel(order_id) {
            Ok(order) => order,
            Err(MarketError::OrderBook(OrderError::OrderAlreadyFilled(_))) => {
                return Err(if market.filled_order_owner(order_id) == Some(account) {
                    ContinuousCancellationError::OrderAlreadyFilled
                } else {
                    ContinuousCancellationError::NotOrderOwner
                });
            }
            Err(MarketError::OrderBook(OrderError::OrderNotFound(_))) => {
                return Err(ContinuousCancellationError::OrderNotFound);
            }
            Err(error) => return Err(ContinuousCancellationError::Market(error)),
        };
        if order.owner != account {
            return Err(ContinuousCancellationError::NotOrderOwner);
        }

        #[cfg(feature = "simulation-diagnostics")]
        self.causal_terminated((account, order_id, order.qty), &code, cause.termination());
        self.markets.insert(code.clone(), candidate_market);
        #[cfg(feature = "simulation-diagnostics")]
        self.causal_snapshot(&code);
        self.remove_npc_order_lifecycle(account, &code, order_id);
        self.record_parent_order_canceled(account, &code, order_id);
        self.record_retail_order_canceled(account, code.clone(), order_id, order.qty);

        Ok(ContinuousCancellationFact {
            account,
            code,
            order_id,
            side: order.side,
            remaining_qty: order.qty,
        })
    }

    pub(super) fn continuous_cancellation_cause(&self) -> ContinuousCancellationCause {
        #[cfg(feature = "simulation-diagnostics")]
        {
            match self.causal.termination {
                Some(crate::diagnostics::causal::Termination::Voluntary) | None => {
                    ContinuousCancellationCause::Voluntary
                }
                Some(crate::diagnostics::causal::Termination::Reprice) => {
                    ContinuousCancellationCause::Reprice
                }
                Some(crate::diagnostics::causal::Termination::Expired) => {
                    ContinuousCancellationCause::Expired
                }
                Some(crate::diagnostics::causal::Termination::DayEnd) => {
                    ContinuousCancellationCause::DayEnd
                }
                Some(crate::diagnostics::causal::Termination::MarketRemainder) => {
                    ContinuousCancellationCause::MarketRemainder
                }
                Some(crate::diagnostics::causal::Termination::Aborted) => {
                    ContinuousCancellationCause::Aborted
                }
            }
        }
        #[cfg(not(feature = "simulation-diagnostics"))]
        ContinuousCancellationCause::Voluntary
    }
}

#[cfg(feature = "simulation-diagnostics")]
impl ContinuousCancellationCause {
    fn termination(self) -> crate::diagnostics::causal::Termination {
        match self {
            Self::Voluntary => crate::diagnostics::causal::Termination::Voluntary,
            Self::Reprice => crate::diagnostics::causal::Termination::Reprice,
            Self::Expired => crate::diagnostics::causal::Termination::Expired,
            Self::DayEnd => crate::diagnostics::causal::Termination::DayEnd,
            Self::MarketRemainder => crate::diagnostics::causal::Termination::MarketRemainder,
            Self::Aborted => crate::diagnostics::causal::Termination::Aborted,
        }
    }
}
