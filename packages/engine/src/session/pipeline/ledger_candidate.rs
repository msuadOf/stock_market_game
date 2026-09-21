use super::{
    validate_indexed_receipt_keys, EnvelopeReceipt, IndexedReceiptKey, ReceiptIndex, StepFatal,
};

pub(super) fn normalize(
    next_index: &mut u64,
    receipts: &[EnvelopeReceipt],
) -> Result<Vec<EnvelopeReceipt>, StepFatal> {
    let mut candidate = receipts.to_vec();
    candidate.sort_by(|left, right| left.local_key.cmp(&right.local_key));
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
