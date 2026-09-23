use super::{P2CandidateKey, P3CandidateResult, P3ValidatedOperation, StepFatal};
use crate::{AccountId, OrderId, RejectionReason, StockCode};
use std::collections::BTreeSet;

/// Ordinary rejection produced at the P3 -> P4 boundary for a cancellation whose stock does not
/// exist in the authoritative market set. It retains the sealed identity needed by downstream
/// deterministic event derivation, but does not allocate an order ID or create an envelope.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct P3P4CancelRejection {
    candidate_key: P2CandidateKey,
    sealed_index: u64,
    owner: AccountId,
    code: StockCode,
    order_id: OrderId,
    reason: RejectionReason,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct P3P4NormalizedOperations {
    operations: Vec<P3ValidatedOperation>,
    rejections: Vec<P3P4CancelRejection>,
}

/// Normalizes the complete P3 result/operation contract before stock partitioning.
///
/// P3 deliberately accepts cancellations without consulting the stock map. P4 stock adapters,
/// however, can only partition operations for authoritative stocks. This boundary turns exactly
/// those unknown-stock cancellations into ordinary rejection facts. All other operations retain
/// their original order and identity. The function is pure and constructs its entire result
/// locally, so any typed failure exposes no partial output.
pub(super) fn normalize_p3_p4_operations(
    results: &[P3CandidateResult],
    operations: &[P3ValidatedOperation],
    known_stocks: impl IntoIterator<Item = StockCode>,
) -> Result<P3P4NormalizedOperations, StepFatal> {
    let mut known = BTreeSet::new();
    for code in known_stocks {
        if !known.insert(code) {
            return Err(invariant("duplicate known stock in P3 -> P4 normalizer"));
        }
    }

    validate_result_order(results)?;
    validate_operation_order(operations)?;
    validate_accepted_mapping(results, operations)?;

    let mut normalized = Vec::with_capacity(operations.len());
    let mut rejections = Vec::new();
    for operation in operations {
        match operation {
            P3ValidatedOperation::Cancel {
                candidate_key,
                sealed_index,
                account,
                code,
                order_id,
            } if !known.contains(code) => rejections.push(P3P4CancelRejection {
                candidate_key: candidate_key.clone(),
                sealed_index: *sealed_index,
                owner: *account,
                code: code.clone(),
                order_id: *order_id,
                reason: RejectionReason::UnknownStock,
            }),
            P3ValidatedOperation::Place(draft) if !known.contains(draft.code()) => {
                return Err(invariant("accepted place references unknown stock"));
            }
            _ => normalized.push(operation.clone()),
        }
    }

    Ok(P3P4NormalizedOperations {
        operations: normalized,
        rejections,
    })
}

fn validate_result_order(results: &[P3CandidateResult]) -> Result<(), StepFatal> {
    let mut keys = BTreeSet::new();
    for (expected, result) in results.iter().enumerate() {
        let expected = u64::try_from(expected)
            .map_err(|_| invariant("P3 result index exceeds the sealed index domain"))?;
        if result.sealed_index() != expected {
            return Err(invariant(
                "P3 results are not contiguous in global sealed_index order",
            ));
        }
        if !keys.insert(result.key()) {
            return Err(invariant("P3 results contain a duplicate candidate key"));
        }
    }
    Ok(())
}

fn validate_operation_order(operations: &[P3ValidatedOperation]) -> Result<(), StepFatal> {
    if operations
        .windows(2)
        .any(|pair| pair[0].sealed_index() >= pair[1].sealed_index())
    {
        return Err(invariant(
            "P3 operations are not in strict global sealed_index order",
        ));
    }
    Ok(())
}

fn validate_accepted_mapping(
    results: &[P3CandidateResult],
    operations: &[P3ValidatedOperation],
) -> Result<(), StepFatal> {
    let accepted = results.iter().filter_map(|result| match result {
        P3CandidateResult::Accepted { key, sealed_index } => Some((key, *sealed_index)),
        P3CandidateResult::Rejected { .. } => None,
    });
    if !accepted.eq(operations
        .iter()
        .map(|operation| (operation.candidate_key(), operation.sealed_index())))
    {
        return Err(invariant(
            "P3 accepted results and validated operations disagree",
        ));
    }
    Ok(())
}

impl P3P4NormalizedOperations {
    pub(super) fn operations(&self) -> &[P3ValidatedOperation] {
        &self.operations
    }

    pub(super) fn rejections(&self) -> &[P3P4CancelRejection] {
        &self.rejections
    }
}

impl P3P4CancelRejection {
    pub(super) const fn candidate_key(&self) -> &P2CandidateKey {
        &self.candidate_key
    }

    pub(super) const fn sealed_index(&self) -> u64 {
        self.sealed_index
    }

    pub(super) const fn owner(&self) -> AccountId {
        self.owner
    }

    pub(super) const fn code(&self) -> &StockCode {
        &self.code
    }

    pub(super) const fn order_id(&self) -> OrderId {
        self.order_id
    }

    pub(super) const fn reason(&self) -> &RejectionReason {
        &self.reason
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_p4_normalizer".to_owned(),
    }
}
