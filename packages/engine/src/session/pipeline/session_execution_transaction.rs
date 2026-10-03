//! Atomic session-candidate installation for continuous P4 through Projection.
//!
//! Callers must pass a prospective `GameSession` shadow. This seam computes every fallible P4-Projection
//! result before replacing any session container, so a typed failure leaves the candidate exactly
//! unchanged. It does not commit the candidate into authoritative state.

use super::{
    account_settlement::SettlementTransactionOutput,
    continuous_matching::{ContinuousStockOutput, IncrementalContinuousStockFinish},
    continuous_projection_transaction::collect_continuous_transaction_events,
    event_collection::OwnedEventFact,
    execution_fact_producers::adapt_continuous_execution_facts,
    stock_execution_transaction::{
        apply_stock_execution_transaction_with_beliefs, StockExecutionTransactionError,
        StockExecutionTransactionOutput,
    },
    EnvelopeReceipt, StepFatal,
};
use crate::{Event, GameSession};

#[derive(Debug, thiserror::Error)]
pub(super) enum SessionExecutionTransactionError {
    #[error("P4-P7 session precondition failed: {0}")]
    Precondition(#[source] StepFatal),
    #[error("P4-P6 transaction failed: {0}")]
    StockExecution(#[source] StockExecutionTransactionError),
    #[error("P7 event collection failed: {0}")]
    Projection(#[source] StepFatal),
}

pub(super) struct SessionExecutionTransactionOutput {
    pub(super) events: Vec<Event>,
    pub(super) event_keys: Vec<super::EventStableKey>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) settlement: SettlementTransactionOutput,
}

/// Applies continuous P4-Projection to a prospective session shadow without committing authority.
pub(super) fn apply_session_execution_transaction(
    session: &mut GameSession,
    workers: Vec<ContinuousStockOutput>,
    preceding_facts: Vec<OwnedEventFact>,
) -> Result<SessionExecutionTransactionOutput, SessionExecutionTransactionError> {
    validate_receipt_cursor(session)?;
    let transaction = apply_stock_execution_transaction_with_beliefs(
        &session.state.envelope_ledger,
        &session.state.accounts,
        &session.state.retail_experience,
        &session.state.belief_participants,
        &session.state.retail_projection_seen,
        workers,
        super::stock_execution_transaction::SettlementApplicationContext::new(
            crate::experience::ExperienceMoment {
                civil_date: session.civil_date(),
                market_minute: session.current_market_minute(),
                trading_day: u64::from(session.state.day),
            },
            &[],
            session.state.setup.t1_enabled,
        ),
    )
    .map_err(SessionExecutionTransactionError::StockExecution)?;
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    validate_stock_ownership(session, &transaction)?;
    let collected = collect_continuous_transaction_events(
        &transaction.stocks,
        session.state.seq,
        preceding_facts,
    )
    .map_err(SessionExecutionTransactionError::Projection)?;

    let StockExecutionTransactionOutput {
        ledger,
        account_patch,
        retail_patch,
        belief_patch,
        seen,
        stocks,
        receipts,
        settlement,
    } = transaction;
    let next_receipt_base = ledger.next_receipt_index();
    for (code, stock) in stocks {
        session.state.markets.insert(code, stock.market);
    }
    session.state.envelope_ledger = ledger;
    session.state.next_receipt_base = next_receipt_base;
    session.state.accounts.extend(account_patch);
    session.state.retail_experience.extend(retail_patch);
    for (id, book) in belief_patch {
        let mut participant = session
            .state
            .belief_participants
            .remove(&id)
            .expect("settlement belief patch must belong to a validated participant");
        *participant.belief_mut() = book;
        session.state.belief_participants.insert(id, participant);
    }
    session.state.retail_projection_seen = seen;
    session.state.seq = collected.next_seq;

    Ok(SessionExecutionTransactionOutput {
        events: collected.events,
        event_keys: collected.keys,
        receipts,
        settlement,
    })
}

/// Consumes a fully drained incremental P4 coordinator and runs ReceiptAggregation/Settlement/Projection exactly once.
///
/// Unknown-stock cancellation rejections are genuine P4 facts but own no stock worker. They are
/// adapted into the same Projection fact set before the accumulated worker outboxes enter the ordinary
/// atomic session transaction.
pub(super) fn apply_incremental_session_execution_transaction(
    session: &mut GameSession,
    finish: IncrementalContinuousStockFinish,
    mut preceding_facts: Vec<OwnedEventFact>,
) -> Result<SessionExecutionTransactionOutput, SessionExecutionTransactionError> {
    preceding_facts.extend(
        adapt_continuous_execution_facts(&finish.detached_facts)
            .map_err(SessionExecutionTransactionError::Projection)?,
    );
    apply_session_execution_transaction(session, finish.workers, preceding_facts)
}

fn validate_receipt_cursor(session: &GameSession) -> Result<(), SessionExecutionTransactionError> {
    let ledger_cursor = session.state.envelope_ledger.next_receipt_index();
    if session.state.next_receipt_base != ledger_cursor {
        return Err(SessionExecutionTransactionError::Precondition(invariant(
            format!(
                "session receipt cursor {} does not match envelope ledger cursor {ledger_cursor}",
                session.state.next_receipt_base
            ),
        )));
    }
    Ok(())
}

fn validate_stock_ownership(
    session: &GameSession,
    transaction: &StockExecutionTransactionOutput,
) -> Result<(), SessionExecutionTransactionError> {
    if let Some(code) = transaction
        .stocks
        .keys()
        .find(|code| !session.state.markets.contains_key(*code))
    {
        return Err(SessionExecutionTransactionError::Precondition(invariant(
            format!("continuous worker returned unknown session stock {code:?}"),
        )));
    }
    Ok(())
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::session_execution_transaction".to_owned(),
    }
}
