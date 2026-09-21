use super::{Envelope, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, ResVec, StepFatal};
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
    let mut ledger = session.envelope_ledger.clone();
    let receipts = apply_receipt_transaction(
        &mut ledger,
        created_envelopes,
        worker_batches,
        terminal_keys,
    )?;
    let next_receipt_base = ledger.next_receipt_index();
    session.envelope_ledger = ledger;
    session.next_receipt_base = next_receipt_base;
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

/// Applies one complete P5 batch to a private ledger candidate.
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
    let mut candidate = ledger.clone();
    let cursor_before = candidate.next_receipt_index();

    candidate.insert_created(created_envelopes)?;
    let mut receipts = worker_batches.into_iter().flatten().collect::<Vec<_>>();
    #[cfg(any(test, feature = "verification-harness"))]
    {
        super::executor_perturbation::reorder(
            super::ExecutorBoundary::P5ReceiptResults,
            &mut receipts,
            |receipt| (format!("{:?}", receipt.local_key), 1),
        );
        if super::executor_perturbation::merge_enabled(super::CanonicalMerge::Completion) {
            candidate.apply(&mut receipts)?;
        } else {
            candidate.apply_in_delivery_order(&mut receipts)?;
        }
    }
    #[cfg(not(any(test, feature = "verification-harness")))]
    candidate.apply(&mut receipts)?;
    candidate.remove_terminal(&terminal_keys)?;
    candidate.validate_conservation()?;
    if candidate
        .iter()
        .any(|(_, envelope)| envelope.live() == ResVec::ZERO)
    {
        return Err(terminal_mismatch());
    }

    let receipt_count = u64::try_from(receipts.len()).map_err(|_| cursor_mismatch())?;
    let expected_cursor = cursor_before
        .checked_add(receipt_count)
        .ok_or_else(cursor_mismatch)?;
    if candidate.next_receipt_index() != expected_cursor {
        return Err(cursor_mismatch());
    }

    *ledger = candidate;
    Ok(receipts)
}

fn cursor_mismatch() -> StepFatal {
    StepFatal::InvariantViolation {
        description: "P5 receipt cursor does not match canonical receipt count".to_owned(),
        location: "pipeline::p5_receipts::apply_receipt_transaction".to_owned(),
    }
}

fn terminal_mismatch() -> StepFatal {
    StepFatal::InvariantViolation {
        description: "P5 terminal envelope remains in the live ledger".to_owned(),
        location: "pipeline::p5_receipts::apply_receipt_transaction".to_owned(),
    }
}
