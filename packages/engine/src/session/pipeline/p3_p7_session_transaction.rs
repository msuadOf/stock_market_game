//! Continuous P3-to-P7 orchestration on a prospective session candidate.

use super::{
    p3_p4_normalizer::normalize_p3_p4_operations,
    p4_continuous::process_continuous_stock,
    p4_continuous_adapter::adapt_continuous_stock_inputs_from_operations,
    p4_p7_session_transaction::{
        apply_session_p4_p7_transaction, P4P7SessionTransactionError, P4P7SessionTransactionOutput,
    },
    p7_producers::{adapt_p3_p4_cancel_rejections, adapt_p3_rejection_facts},
    P2CandidateBatch, P3ValidationOutput, StepFatal,
};
use crate::{GameSession, StockCode};
use rayon::prelude::*;

#[derive(Debug, thiserror::Error)]
pub(super) enum P3P7SessionTransactionError {
    #[error("P3-P7 session precondition failed: {0}")]
    Precondition(#[source] StepFatal),
    #[error("P3-P4 continuous adapter failed: {0}")]
    Adapter(#[source] StepFatal),
    #[error("continuous stock worker for {code:?} failed: {source}")]
    Worker {
        code: StockCode,
        #[source]
        source: StepFatal,
    },
    #[error("P4-P7 session transaction failed: {0}")]
    Transaction(#[source] P4P7SessionTransactionError),
}

/// Partitions a complete P3 output by stock, executes the independent stock workers in parallel,
/// then installs their P4-P7 result atomically into the prospective session candidate.
pub(super) fn apply_session_p3_p7_transaction(
    session: &mut GameSession,
    candidates: &P2CandidateBatch,
    validation: &P3ValidationOutput,
) -> Result<P4P7SessionTransactionOutput, P3P7SessionTransactionError> {
    validate_order_cursor(session, validation)?;
    let normalized = normalize_p3_p4_operations(
        validation.results(),
        validation.operations(),
        session.markets.keys().cloned(),
    )
    .map_err(P3P7SessionTransactionError::Adapter)?;
    let mut preceding_facts = adapt_p3_rejection_facts(candidates, validation.results())
        .map_err(P3P7SessionTransactionError::Adapter)?;
    preceding_facts.extend(
        adapt_p3_p4_cancel_rejections(normalized.rejections())
            .map_err(P3P7SessionTransactionError::Adapter)?,
    );
    let inputs = adapt_continuous_stock_inputs_from_operations(session, normalized.operations())
        .map_err(P3P7SessionTransactionError::Adapter)?;
    let worker_results = inputs
        .into_par_iter()
        .map(|input| {
            let code = input.market.code().clone();
            (code, process_continuous_stock(input))
        })
        .collect::<Vec<_>>();
    let workers = collect_canonical_worker_results(worker_results)?;
    let output = apply_session_p4_p7_transaction(session, workers, preceding_facts)
        .map_err(P3P7SessionTransactionError::Transaction)?;
    session.next_order_id = validation.next_order_id_after();
    Ok(output)
}

pub(super) fn collect_canonical_worker_results(
    results: Vec<(
        StockCode,
        Result<super::p4_continuous::ContinuousStockOutput, StepFatal>,
    )>,
) -> Result<Vec<super::p4_continuous::ContinuousStockOutput>, P3P7SessionTransactionError> {
    results
        .into_iter()
        .map(|(code, result)| {
            result.map_err(|source| P3P7SessionTransactionError::Worker { code, source })
        })
        .collect()
}

fn validate_order_cursor(
    session: &GameSession,
    validation: &P3ValidationOutput,
) -> Result<(), P3P7SessionTransactionError> {
    let draft_count = u64::try_from(validation.drafts().len())
        .map_err(|error| P3P7SessionTransactionError::Precondition(invariant(error.to_string())))?;
    let expected_after = session
        .next_order_id
        .checked_add(draft_count)
        .ok_or_else(|| {
            P3P7SessionTransactionError::Precondition(invariant(
                "P3 accepted-order count overflows the session order cursor".to_owned(),
            ))
        })?;
    if validation.next_order_id_after() != expected_after {
        return Err(P3P7SessionTransactionError::Precondition(invariant(
            format!(
                "P3 next order cursor {} does not match session cursor {} plus {} accepted orders",
                validation.next_order_id_after(),
                session.next_order_id,
                draft_count
            ),
        )));
    }
    for (ordinal, draft) in validation.drafts().iter().enumerate() {
        let ordinal = u64::try_from(ordinal).map_err(|error| {
            P3P7SessionTransactionError::Precondition(invariant(error.to_string()))
        })?;
        let expected = session.next_order_id.checked_add(ordinal).ok_or_else(|| {
            P3P7SessionTransactionError::Precondition(invariant(
                "P3 draft order identity overflows the session order cursor".to_owned(),
            ))
        })?;
        if draft.order_id().0 != expected {
            return Err(P3P7SessionTransactionError::Precondition(invariant(
                format!(
                    "P3 draft order ID {} does not match expected session order ID {expected}",
                    draft.order_id().0
                ),
            )));
        }
    }
    Ok(())
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::p3_p7_session_transaction".to_owned(),
    }
}
