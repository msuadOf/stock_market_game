use super::{p6_transaction::P6TransactionError, StepFatal};
use std::error::Error;

/// Preserve typed fatal identity (including rollback hashes) across orchestration wrappers.
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
        npc_p2_p7_transaction::NpcP2P7TransactionError,
        p4_p5_p6_transaction::P4P5P6TransactionError,
        p4_p7_session_transaction::P4P7SessionTransactionError,
        stock_auction::b2_auction_day_end::B2AuctionDayEndError,
    };

    #[test]
    fn nested_settlement_internal_preserves_both_hashes_for_b1_and_b2() {
        let session =
            crate::GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42)
                .unwrap();
        let fatal = StepFatal::Internal {
            expected: session.business_state_hash().unwrap(),
            observed: session.session_state_hash().unwrap(),
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

    #[test]
    fn nested_invariant_keeps_original_location_and_domain_errors_gain_transaction_location() {
        let fatal = StepFatal::InvariantViolation {
            description: "broken receipt chain".into(),
            location: "p5::receipt".into(),
        };
        assert_eq!(
            B1ContinuousTransactionError::Npc(NpcP2P7TransactionError::Preparation(fatal.clone()))
                .into_fatal(),
            fatal
        );
        assert_eq!(
            B2AuctionTransactionError::Finalization(B2AuctionDayEndError::P5(fatal.clone()))
                .into_fatal(),
            fatal
        );
        let b1 =
            B1ContinuousTransactionError::Npc(NpcP2P7TransactionError::UnkeyedProjectedIntent {
                account: crate::AccountId(7),
            });
        let description = b1.to_string();
        assert_eq!(
            b1.into_fatal(),
            StepFatal::InvariantViolation {
                description,
                location: "pipeline::b1_continuous_transaction".into()
            }
        );
        let b2 = B2AuctionTransactionError::Npc(NpcP2P7TransactionError::UnkeyedProjectedIntent {
            account: crate::AccountId(7),
        });
        let description = b2.to_string();
        assert_eq!(
            b2.into_fatal(),
            StepFatal::InvariantViolation {
                description,
                location: "pipeline::b2_auction_transaction".into()
            }
        );
    }
}
