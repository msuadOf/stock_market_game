//! Atomic session-candidate installation for continuous P4 through P7.
//!
//! Callers must pass a prospective `GameSession` shadow. This seam computes every fallible P4-P7
//! result before replacing any session container, so a typed failure leaves the candidate exactly
//! unchanged. It does not commit the candidate into authoritative state.

use super::{
    p4_continuous::{ContinuousStockOutput, IncrementalContinuousStockFinish},
    p4_p5_p6_transaction::{
        apply_p4_p5_p6_transaction, P4P5P6TransactionError, P4P5P6TransactionOutput,
    },
    p6_transaction::P6TransactionOutput,
    p7_continuous_transaction::collect_continuous_transaction_events,
    p7_events::OwnedEventFact,
    p7_p4_producers::adapt_continuous_execution_facts,
    EnvelopeReceipt, StepFatal,
};
use crate::{Event, GameSession};

#[derive(Debug, thiserror::Error)]
pub(super) enum P4P7SessionTransactionError {
    #[error("P4-P7 session precondition failed: {0}")]
    Precondition(#[source] StepFatal),
    #[error("P4-P6 transaction failed: {0}")]
    P4P6(#[source] P4P5P6TransactionError),
    #[error("P7 event collection failed: {0}")]
    P7(#[source] StepFatal),
}

pub(super) struct P4P7SessionTransactionOutput {
    pub(super) events: Vec<Event>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: P6TransactionOutput,
}

/// Applies continuous P4-P7 to a prospective session shadow without committing authority.
pub(super) fn apply_session_p4_p7_transaction(
    session: &mut GameSession,
    workers: Vec<ContinuousStockOutput>,
    preceding_facts: Vec<OwnedEventFact>,
) -> Result<P4P7SessionTransactionOutput, P4P7SessionTransactionError> {
    validate_receipt_cursor(session)?;
    let transaction = apply_p4_p5_p6_transaction(
        &session.envelope_ledger,
        &session.accounts,
        &session.retail_experience,
        &session.retail_projection_seen,
        session.current_market_minute(),
        workers,
        session.setup.t1_enabled,
    )
    .map_err(P4P7SessionTransactionError::P4P6)?;
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    validate_stock_ownership(session, &transaction)?;
    let collected =
        collect_continuous_transaction_events(&transaction.stocks, session.seq, preceding_facts)
            .map_err(P4P7SessionTransactionError::P7)?;

    let P4P5P6TransactionOutput {
        ledger,
        account_patch,
        retail_patch,
        seen,
        stocks,
        receipts,
        p6,
    } = transaction;
    let next_receipt_base = ledger.next_receipt_index();
    for (code, stock) in stocks {
        session.markets.insert(code, stock.market);
    }
    session.envelope_ledger = ledger;
    session.next_receipt_base = next_receipt_base;
    session.accounts.extend(account_patch);
    session.retail_experience.extend(retail_patch);
    session.retail_projection_seen = seen;
    session.seq = collected.next_seq;

    Ok(P4P7SessionTransactionOutput {
        events: collected.events,
        receipts,
        p6,
    })
}

/// Consumes a fully drained incremental P4 coordinator and runs P5/P6/P7 exactly once.
///
/// Unknown-stock cancellation rejections are genuine P4 facts but own no stock worker. They are
/// adapted into the same P7 fact set before the accumulated worker outboxes enter the ordinary
/// atomic session transaction.
pub(super) fn apply_incremental_session_p4_p7_transaction(
    session: &mut GameSession,
    finish: IncrementalContinuousStockFinish,
    mut preceding_facts: Vec<OwnedEventFact>,
) -> Result<P4P7SessionTransactionOutput, P4P7SessionTransactionError> {
    preceding_facts.extend(
        adapt_continuous_execution_facts(&finish.detached_facts)
            .map_err(P4P7SessionTransactionError::P7)?,
    );
    apply_session_p4_p7_transaction(session, finish.workers, preceding_facts)
}

fn validate_receipt_cursor(session: &GameSession) -> Result<(), P4P7SessionTransactionError> {
    let ledger_cursor = session.envelope_ledger.next_receipt_index();
    if session.next_receipt_base != ledger_cursor {
        return Err(P4P7SessionTransactionError::Precondition(invariant(
            format!(
                "session receipt cursor {} does not match envelope ledger cursor {ledger_cursor}",
                session.next_receipt_base
            ),
        )));
    }
    Ok(())
}

fn validate_stock_ownership(
    session: &GameSession,
    transaction: &P4P5P6TransactionOutput,
) -> Result<(), P4P7SessionTransactionError> {
    if let Some(code) = transaction
        .stocks
        .keys()
        .find(|code| !session.markets.contains_key(*code))
    {
        return Err(P4P7SessionTransactionError::Precondition(invariant(
            format!("continuous worker returned unknown session stock {code:?}"),
        )));
    }
    Ok(())
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::p4_p7_session_transaction".to_owned(),
    }
}
