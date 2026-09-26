use super::{p6_transaction::P6TransactionError, StepFatal};
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
        if let Some(P6TransactionError::Settlement(fatal)) =
            source.downcast_ref::<P6TransactionError>()
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
        b1_continuous_transaction::B1ContinuousTransactionError,
        b2_auction_transaction::B2AuctionTransactionError,
        p4_p5_p6_transaction::P4P5P6TransactionError,
        p4_p7_session_transaction::P4P7SessionTransactionError,
        stock_auction::b2_auction_day_end::B2AuctionDayEndError,
    };

    #[test]
    fn nested_settlement_preserves_fatal_identity_for_b1_and_b2() {
        let fatal = StepFatal::InvariantViolation {
            description: "settlement failed".to_owned(),
            location: "p6::settlement".to_owned(),
        };
        assert_eq!(
            B1ContinuousTransactionError::P4P7(P4P7SessionTransactionError::P4P6(
                P4P5P6TransactionError::P6(P6TransactionError::Settlement(fatal.clone())),
            ))
            .into_fatal(),
            fatal,
        );
        assert_eq!(
            B2AuctionTransactionError::Finalization(B2AuctionDayEndError::P6(
                P6TransactionError::Settlement(fatal.clone()),
            ))
            .into_fatal(),
            fatal,
        );
    }
}
