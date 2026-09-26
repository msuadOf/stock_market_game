use super::{Envelope, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, StepFatal};
use crate::GameSession;

/// Applies P5 to a private ledger candidate and advances both authoritative
/// receipt cursors together. A split cursor is an invariant failure, never a
/// condition to repair by choosing one side.
pub(super) fn apply_session_receipt_transaction(
    session: &mut GameSession,
    created_envelopes: Vec<Envelope>,
    worker_batches: Vec<Vec<EnvelopeReceipt>>,
    terminal_keys: Vec<EnvelopeKey>,
) -> Result<Vec<EnvelopeReceipt>, StepFatal> {
    if session.next_receipt_base != session.envelope_ledger.next_receipt_index() {
        return Err(session_cursor_mismatch(
            session.next_receipt_base,
            session.envelope_ledger.next_receipt_index(),
        ));
    }
    let receipts = apply_receipt_transaction(
        &mut session.envelope_ledger,
        created_envelopes,
        worker_batches,
        terminal_keys,
    )?;
    session.next_receipt_base = session.envelope_ledger.next_receipt_index();
    Ok(receipts)
}

fn session_cursor_mismatch(session_cursor: u64, ledger_cursor: u64) -> StepFatal {
    StepFatal::InvariantViolation {
        description: format!(
            "session receipt cursor {session_cursor} does not match envelope ledger cursor {ledger_cursor}"
        ),
        location: "pipeline::p5_receipts::apply_session_receipt_transaction".to_owned(),
    }
}

/// Applies one complete P5 batch without changing the caller's ledger on failure.
///
/// Newly created envelopes are installed before their receipts, including envelopes
/// which become terminal in the same batch. Worker indices remain untrusted
/// placeholders. The caller's ledger is replaced only after every ledger check and
/// the receipt-cursor check succeeds.
pub(super) fn apply_receipt_transaction(
    ledger: &mut EnvelopeLedger,
    created_envelopes: Vec<Envelope>,
    worker_batches: Vec<Vec<EnvelopeReceipt>>,
    terminal_keys: Vec<EnvelopeKey>,
) -> Result<Vec<EnvelopeReceipt>, StepFatal> {
    let (candidate, receipts) = apply_owned_receipt_transaction(
        ledger.clone(),
        created_envelopes,
        worker_batches,
        terminal_keys,
    )?;
    *ledger = candidate;
    Ok(receipts)
}

/// The enclosing tick owns and discards this ledger on failure. All rows are
/// checked at the input and output boundaries; the batch itself applies each
/// transition in place instead of repeatedly cloning the entire order ledger.
pub(super) fn apply_owned_receipt_transaction(
    mut candidate: EnvelopeLedger,
    created_envelopes: Vec<Envelope>,
    worker_batches: Vec<Vec<EnvelopeReceipt>>,
    terminal_keys: Vec<EnvelopeKey>,
) -> Result<(EnvelopeLedger, Vec<EnvelopeReceipt>), StepFatal> {
    candidate.validate_complete_evidence()?;
    let cursor_before = candidate.next_receipt_index();

    candidate.insert_created_for_stock_round(created_envelopes)?;
    let mut receipts = worker_batches.into_iter().flatten().collect::<Vec<_>>();
    #[cfg(any(test, feature = "verification-harness"))]
    {
        super::executor_perturbation::reorder(
            super::ExecutorBoundary::P5ReceiptResults,
            &mut receipts,
            |receipt| (format!("{:?}", receipt.local_key), 1),
        );
        if super::executor_perturbation::merge_enabled(super::CanonicalMerge::Completion) {
            candidate.apply_private_for_stock_round(&mut receipts, &terminal_keys)?;
        } else {
            candidate.apply_private_in_delivery_order_for_p5(&mut receipts, &terminal_keys)?;
        }
    }
    #[cfg(not(any(test, feature = "verification-harness")))]
    candidate.apply_private_for_stock_round(&mut receipts, &terminal_keys)?;
    candidate.validate_complete_evidence()?;

    let receipt_count = u64::try_from(receipts.len()).map_err(|_| cursor_mismatch())?;
    let expected_cursor = cursor_before
        .checked_add(receipt_count)
        .ok_or_else(cursor_mismatch)?;
    if candidate.next_receipt_index() != expected_cursor {
        return Err(cursor_mismatch());
    }

    Ok((candidate, receipts))
}

fn cursor_mismatch() -> StepFatal {
    StepFatal::InvariantViolation {
        description: "P5 receipt cursor does not match canonical receipt count".to_owned(),
        location: "pipeline::p5_receipts::apply_receipt_transaction".to_owned(),
    }
}
