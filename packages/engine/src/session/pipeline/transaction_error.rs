use super::{account_settlement::SettlementTransactionError, StepFatal};
use std::error::Error;

/// Preserve typed fatal identity across orchestration wrappers.
/// Domain errors without an existing fatal become an explicit, located invariant violation.
pub(super) fn into_fatal(error: impl Error + 'static, location: &str) -> StepFatal {
    let mut current: Option<&(dyn Error + 'static)> = Some(&error);
    while let Some(source) = current {
        if let Some(fatal) = source.downcast_ref::<StepFatal>() {
            return fatal.clone();
        }
        // thiserror's transparent wrapper delegates source() to the wrapped error, which
        // hides a leaf StepFatal. Preserve that explicit settlement variant as well.
        if let Some(SettlementTransactionError::Settlement(fatal)) =
            source.downcast_ref::<SettlementTransactionError>()
        {
            return fatal.clone();
        }
        current = source.source();
    }
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: location.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::pipeline::{
        auction_tick_transaction::AuctionTransactionError,
        continuous_tick_transaction::ContinuousTransactionError,
        session_execution_transaction::SessionExecutionTransactionError,
        stock_auction::auction_day_end::AuctionDayEndError,
        stock_execution_transaction::StockExecutionTransactionError,
    };

    #[test]
    fn nested_settlement_preserves_fatal_identity_through_continuous_execution_and_auction_finalization(
    ) {
        let fatal = StepFatal::InvariantViolation {
            description: "settlement failed".to_owned(),
            location: "session::pipeline::settlement".to_owned(),
        };
        assert_eq!(
            ContinuousTransactionError::SessionExecution(
                SessionExecutionTransactionError::StockExecution(
                    StockExecutionTransactionError::Settlement(
                        SettlementTransactionError::Settlement(fatal.clone())
                    ),
                )
            )
            .into_fatal(),
            fatal,
        );
        assert_eq!(
            AuctionTransactionError::Finalization(AuctionDayEndError::Settlement(
                SettlementTransactionError::Settlement(fatal.clone()),
            ))
            .into_fatal(),
            fatal,
        );
    }
}
