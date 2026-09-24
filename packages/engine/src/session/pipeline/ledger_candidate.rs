use super::{
    validate_indexed_receipt_keys, EnvelopeReceipt, IndexedReceiptKey, ReceiptIndex, StepFatal,
};
use std::cmp::Reverse;

pub(super) fn normalize(
    next_index: &mut u64,
    receipts: &[EnvelopeReceipt],
) -> Result<Vec<EnvelopeReceipt>, StepFatal> {
    let mut candidate = receipts.to_vec();
    // One envelope's cumulative fills must be applied in its actual quantity chain. The
    // source identity can decrease when independently accepted stock requests are delivered
    // in that stock's local order. Sorting by that identity would corrupt the audit.
    candidate.sort_by(|left, right| {
        (
            left.local_key.journal().rank(),
            left.local_key.source().rank(),
            &left.envelope,
            Reverse(left.qty_before),
            left.value_before,
            &left.local_key,
        )
            .cmp(&(
                right.local_key.journal().rank(),
                right.local_key.source().rank(),
                &right.envelope,
                Reverse(right.qty_before),
                right.value_before,
                &right.local_key,
            ))
    });
    index_in_order(next_index, candidate)
}

pub(super) fn index_in_order(
    next_index: &mut u64,
    mut candidate: Vec<EnvelopeReceipt>,
) -> Result<Vec<EnvelopeReceipt>, StepFatal> {
    let mut indexed = Vec::with_capacity(candidate.len());
    for receipt in &mut candidate {
        receipt.index = *next_index;
        indexed.push(IndexedReceiptKey {
            index: ReceiptIndex(receipt.index),
            envelope: receipt.envelope.clone(),
            local_key: receipt.local_key.clone(),
        });
        *next_index = next_index
            .checked_add(1)
            .ok_or_else(|| super::ledger_validation::invariant("receipt index overflow"))?;
    }
    validate_indexed_receipt_keys(&indexed)?;
    Ok(candidate)
}
