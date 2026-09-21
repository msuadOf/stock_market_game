//! Continuous B1 lifecycle diagnostics projected from typed P3/P4/P5 facts.

use super::{
    adaptive_plan_chain::PlanChainFactConsumption,
    p4_continuous::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousExecutionFact,
        ContinuousExecutionOutcome, ContinuousPlaceFact,
    },
    EnvelopeReceipt, P2Candidate, P2CandidateBatch, P2CandidateKey, P3CandidateResult,
    P3ValidationOutput, ReceiptKind, ReceiptSource, StepFatal,
};
use crate::session::RetailOrderDiagnosticEvent;
use crate::{GameSession, Intent, RejectionReason, StockCode};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn project_continuous_retail_lifecycle(
    session: &mut GameSession,
    candidates: &P2CandidateBatch,
    validation: &P3ValidationOutput,
    execution_facts: &[ContinuousExecutionFact],
    receipts: &[EnvelopeReceipt],
    consumed: &PlanChainFactConsumption,
) -> Result<(), StepFatal> {
    let candidates = index_candidates(candidates)?;
    if candidates.len() != validation.results().len() {
        return Err(invariant(
            "P3 result count does not match the continuous P2 candidate batch",
        ));
    }
    let mut facts = BTreeMap::new();
    for fact in execution_facts {
        let identity = (fact.candidate_key.clone(), fact.sealed_index);
        if facts.insert(identity.clone(), fact).is_some() {
            return Err(invariant(
                "continuous P4 facts contain a duplicate identity",
            ));
        }
        if !consumed.operations.contains(&identity) {
            return Err(invariant(
                "continuous final projection received an operation not consumed by the adaptive chain",
            ));
        }
    }
    let accepted_count = validation
        .results()
        .iter()
        .filter(|result| matches!(result, P3CandidateResult::Accepted { .. }))
        .count();
    if facts.len() != accepted_count {
        return Err(invariant(
            "continuous P4 fact count does not match P3 accepted results",
        ));
    }

    let sealed_receipts = receipts
        .iter()
        .filter(|receipt| matches!(receipt.local_key.source(), ReceiptSource::SealedIntent(_)))
        .collect::<Vec<_>>();
    if sealed_receipts
        .iter()
        .any(|receipt| !consumed.receipts.contains(&receipt.local_key))
    {
        return Err(invariant(
            "continuous final projection received a receipt not consumed by the adaptive chain",
        ));
    }

    let mut events = Vec::new();
    let mut seen_results = BTreeSet::new();
    for (ordinal, result) in validation.results().iter().enumerate() {
        let expected_sealed = u64::try_from(ordinal)
            .map_err(|_| invariant("continuous P3 result count exceeds u64"))?;
        if result.sealed_index() != expected_sealed || !seen_results.insert(result.key()) {
            return Err(invariant(
                "continuous P3 results are not in canonical sealed order",
            ));
        }
        let candidate = candidates
            .get(result.key())
            .ok_or_else(|| invariant("continuous P3 result has no P2 candidate"))?;
        match result {
            P3CandidateResult::Rejected { reason, .. } => {
                push_rejected(session, &mut events, candidate, reason.clone())?;
            }
            P3CandidateResult::PendingPlanEventsLimited { .. } => {}
            P3CandidateResult::Accepted { key, sealed_index } => {
                let fact = facts
                    .remove(&(key.clone(), *sealed_index))
                    .ok_or_else(|| invariant("P3 acceptance has no continuous P4 fact"))?;
                validate_operation(candidate, fact)?;
                project_operation(session, &mut events, fact)?;
                let mut fills = sealed_receipts
                    .iter()
                    .copied()
                    .filter(|receipt| {
                        receipt.local_key.source() == ReceiptSource::SealedIntent(*sealed_index)
                            && receipt.kind == ReceiptKind::Fill
                    })
                    .collect::<Vec<_>>();
                fills.sort_by(|left, right| left.local_key.cmp(&right.local_key));
                for receipt in fills {
                    let qty = receipt
                        .qty_before
                        .checked_sub(receipt.qty_after)
                        .filter(|qty| *qty > 0)
                        .ok_or_else(|| {
                            invariant("continuous retail fill has no positive quantity")
                        })?;
                    if session
                        .retail_experience
                        .contains_key(&receipt.envelope.account)
                    {
                        events.push(RetailOrderDiagnosticEvent::Filled {
                            account: receipt.envelope.account,
                            code: receipt.envelope.stock.clone(),
                            side: receipt.envelope.side,
                            order_id: receipt.envelope.order,
                            qty,
                        });
                    }
                }
            }
        }
    }
    if !facts.is_empty() {
        return Err(invariant("continuous P4 fact has no P3 result"));
    }
    session.last_retail_order_events.extend(events);
    Ok(())
}

fn validate_operation(
    candidate: &P2Candidate,
    fact: &ContinuousExecutionFact,
) -> Result<(), StepFatal> {
    if fact.candidate_key() != candidate.key() {
        return Err(invariant(
            "continuous P4 fact belongs to a different P2 candidate",
        ));
    }
    let valid = match (candidate.intent(), fact.outcome()) {
        (
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            },
            ContinuousExecutionOutcome::Place {
                fact: place,
                original_qty,
            },
        ) => {
            let (account, actual_code, order_id) = place_identity(place);
            let outcome_matches = match place {
                ContinuousPlaceFact::Resting {
                    side: actual_side,
                    price: actual_price,
                    remaining_qty,
                    ..
                } => actual_side == side && actual_price == price && remaining_qty <= qty,
                ContinuousPlaceFact::Filled {
                    side: actual_side,
                    filled_qty,
                    ..
                } => actual_side == side && filled_qty == qty,
                ContinuousPlaceFact::Rejected { .. } => true,
            };
            account == candidate.owner()
                && actual_code == code
                && original_qty == qty
                && fact.allocated_order_id() == Some(order_id)
                && outcome_matches
        }
        (
            Intent::PlaceMarket { code, side, qty },
            ContinuousExecutionOutcome::Place {
                fact: place,
                original_qty,
            },
        ) => {
            let (account, actual_code, order_id) = place_identity(place);
            let outcome_matches = match place {
                ContinuousPlaceFact::Filled {
                    side: actual_side,
                    filled_qty,
                    ..
                } => actual_side == side && filled_qty <= qty,
                ContinuousPlaceFact::Rejected { .. } => true,
                ContinuousPlaceFact::Resting { .. } => false,
            };
            account == candidate.owner()
                && actual_code == code
                && original_qty == qty
                && fact.allocated_order_id() == Some(order_id)
                && outcome_matches
        }
        (Intent::Cancel { code, id }, ContinuousExecutionOutcome::Cancel(cancel)) => {
            let (account, actual_code, order_id) = cancel_identity(cancel);
            account == candidate.owner()
                && actual_code == code
                && order_id == *id
                && fact.allocated_order_id().is_none()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invariant(
            "continuous P4 payload does not match its P2 candidate",
        ))
    }
}

fn place_identity(fact: &ContinuousPlaceFact) -> (crate::AccountId, &StockCode, crate::OrderId) {
    match fact {
        ContinuousPlaceFact::Resting {
            account,
            code,
            order_id,
            ..
        }
        | ContinuousPlaceFact::Filled {
            account,
            code,
            order_id,
            ..
        }
        | ContinuousPlaceFact::Rejected {
            account,
            code,
            order_id,
            ..
        } => (*account, code, *order_id),
    }
}

fn cancel_identity(fact: &ContinuousCancelFact) -> (crate::AccountId, &StockCode, crate::OrderId) {
    match fact {
        ContinuousCancelFact::Canceled {
            account,
            code,
            order_id,
            ..
        }
        | ContinuousCancelFact::Rejected {
            account,
            code,
            order_id,
            ..
        } => (*account, code, *order_id),
    }
}

fn index_candidates(
    candidates: &P2CandidateBatch,
) -> Result<BTreeMap<&P2CandidateKey, &P2Candidate>, StepFatal> {
    let mut indexed = BTreeMap::new();
    for candidate in candidates.candidates() {
        if indexed.insert(candidate.key(), candidate).is_some() {
            return Err(invariant("continuous P2 batch contains a duplicate key"));
        }
    }
    Ok(indexed)
}

fn project_operation(
    session: &GameSession,
    events: &mut Vec<RetailOrderDiagnosticEvent>,
    fact: &ContinuousExecutionFact,
) -> Result<(), StepFatal> {
    match &fact.outcome {
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Resting {
                    account,
                    code,
                    order_id,
                    side,
                    ..
                }
                | ContinuousPlaceFact::Filled {
                    account,
                    code,
                    order_id,
                    side,
                    ..
                },
            original_qty,
        } => {
            if session.retail_experience.contains_key(account) {
                events.push(RetailOrderDiagnosticEvent::Submitted {
                    account: *account,
                    code: code.clone(),
                    side: *side,
                    order_id: *order_id,
                    qty: *original_qty,
                });
            }
        }
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Rejected {
                    account,
                    code,
                    reason,
                    ..
                },
            ..
        } => push_rejected_payload(session, events, *account, code, reason.clone()),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
            account,
            code,
            order_id,
            remaining_qty,
            ..
        }) => {
            if session.retail_experience.contains_key(account) {
                events.push(RetailOrderDiagnosticEvent::Canceled {
                    account: *account,
                    code: code.clone(),
                    order_id: *order_id,
                    remaining_qty: *remaining_qty,
                });
            }
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
            account,
            code,
            reason,
            ..
        }) => push_rejected_payload(session, events, *account, code, cancel_rejection(*reason)),
    }
    Ok(())
}

fn push_rejected(
    session: &GameSession,
    events: &mut Vec<RetailOrderDiagnosticEvent>,
    candidate: &P2Candidate,
    reason: RejectionReason,
) -> Result<(), StepFatal> {
    let code = match candidate.intent() {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => code,
    };
    push_rejected_payload(session, events, candidate.owner(), code, reason);
    Ok(())
}

fn push_rejected_payload(
    session: &GameSession,
    events: &mut Vec<RetailOrderDiagnosticEvent>,
    account: crate::AccountId,
    code: &StockCode,
    reason: RejectionReason,
) {
    if session.retail_experience.contains_key(&account) {
        events.push(RetailOrderDiagnosticEvent::Rejected {
            account,
            code: code.clone(),
            reason,
        });
    }
}

const fn cancel_rejection(reason: ContinuousCancelRejection) -> RejectionReason {
    match reason {
        ContinuousCancelRejection::UnknownStock => RejectionReason::UnknownStock,
        ContinuousCancelRejection::OrderNotFound => RejectionReason::OrderNotFound,
        ContinuousCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
        ContinuousCancelRejection::SameTickEnvelope => RejectionReason::SameTickOrderNotCancelable,
        ContinuousCancelRejection::AuctionOrderNotCancelable => {
            RejectionReason::AuctionOrderNotCancelable
        }
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_lifecycle_projection".to_owned(),
    }
}
