//! 根据 typed P3/P4/ReceiptAggregation 事实投影 Continuous 生命周期诊断。

use super::{
    adaptive_plan_chain::PlanChainFactConsumption,
    continuous_matching::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousExecutionFact,
        ContinuousExecutionOutcome, ContinuousPlaceFact,
    },
    AccountValidationOutput, CandidateValidationResult, EnvelopeReceipt, IntentCandidate,
    IntentCandidateBatch, IntentCandidateKey, ReceiptKind, ReceiptSource, StepFatal,
};
use crate::session::RetailOrderDiagnosticEvent;
use crate::{GameSession, Intent, OrderId, RejectionReason, StockCode};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn project_continuous_retail_lifecycle(
    session: &mut GameSession,
    candidates: &IntentCandidateBatch,
    validation: &AccountValidationOutput,
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
        if !consumed.contains_operation(&identity.0, identity.1) {
            return Err(invariant(
                "continuous final projection received an operation not consumed by the adaptive chain",
            ));
        }
    }
    let accepted_count = validation
        .results()
        .iter()
        .filter(|result| matches!(result, CandidateValidationResult::Accepted { .. }))
        .count();
    if facts.len() != accepted_count {
        return Err(invariant(
            "continuous P4 fact count does not match P3 accepted results",
        ));
    }

    let mut fills_by_request = BTreeMap::<u64, Vec<&EnvelopeReceipt>>::new();
    for receipt in receipts {
        if let ReceiptSource::SealedIntent(sealed_index) = receipt.local_key.source() {
            if !consumed.contains_receipt(&receipt.local_key) {
                return Err(invariant(
                    "continuous final projection received a receipt not consumed by the adaptive chain",
                ));
            }
            if receipt.kind == ReceiptKind::Fill {
                fills_by_request
                    .entry(sealed_index)
                    .or_default()
                    .push(receipt);
            }
        }
    }
    for fills in fills_by_request.values_mut() {
        fills.sort_by(|left, right| left.local_key.cmp(&right.local_key));
    }

    validate_result_identities(validation.results())?;
    let mut events = Vec::new();
    let mut fill_quantities = BTreeMap::new();
    for result in validation.results() {
        let candidate = candidates
            .get(result.key())
            .ok_or_else(|| invariant("continuous P3 result has no P2 candidate"))?;
        match result {
            CandidateValidationResult::Rejected { reason, .. } => {
                push_rejected(session, &mut events, candidate, reason.clone())?;
            }
            CandidateValidationResult::Accepted { key, sealed_index } => {
                let fact = facts
                    .remove(&(key.clone(), *sealed_index))
                    .ok_or_else(|| invariant("P3 acceptance has no continuous P4 fact"))?;
                validate_operation(candidate, fact)?;
                project_operation(session, &mut events, fact)?;
                for receipt in fills_by_request.remove(sealed_index).unwrap_or_default() {
                    let qty = receipt
                        .qty_before
                        .checked_sub(receipt.qty_after)
                        .filter(|qty| *qty > 0)
                        .ok_or_else(|| {
                            invariant("continuous retail fill has no positive quantity")
                        })?;
                    if session
                        .state
                        .retail_experience
                        .contains_key(&receipt.envelope.account)
                    {
                        fill_quantities
                            .insert(events.len(), (receipt.qty_before, receipt.qty_after));
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
    session
        .state
        .last_retail_order_events
        .extend(order_lifecycle_events(events, fill_quantities)?);
    Ok(())
}

fn order_lifecycle_events(
    events: Vec<RetailOrderDiagnosticEvent>,
    fill_quantities: BTreeMap<usize, (u32, u32)>,
) -> Result<Vec<RetailOrderDiagnosticEvent>, StepFatal> {
    ContinuousLifecycleEventBatch::from_events(events, fill_quantities)?.finish()
}

/// 仅拥有本批诊断的同订单依赖，全部校验成功后才交给 Session。
struct ContinuousLifecycleEventBatch {
    submitted_this_tick: BTreeSet<OrderId>,
    published: BTreeSet<OrderId>,
    remaining_fills: BTreeMap<OrderId, usize>,
    pending_fills: BTreeMap<OrderId, Vec<RetailOrderDiagnosticEvent>>,
    pending_terminal: BTreeMap<OrderId, RetailOrderDiagnosticEvent>,
    ordered: Vec<RetailOrderDiagnosticEvent>,
}

impl ContinuousLifecycleEventBatch {
    fn from_events(
        mut events: Vec<RetailOrderDiagnosticEvent>,
        mut fill_quantities: BTreeMap<usize, (u32, u32)>,
    ) -> Result<Self, StepFatal> {
        let mut fills_by_order =
            BTreeMap::<OrderId, Vec<(u32, u32, RetailOrderDiagnosticEvent)>>::new();
        for (position, event) in events.iter().enumerate() {
            if let RetailOrderDiagnosticEvent::Filled { order_id, .. } = event {
                let (before, after) = fill_quantities.remove(&position).ok_or_else(|| {
                    invariant("continuous retail fill has no source quantity transition")
                })?;
                fills_by_order
                    .entry(*order_id)
                    .or_default()
                    .push((before, after, event.clone()));
            }
        }
        if !fill_quantities.is_empty() {
            return Err(invariant(
                "continuous retail quantity transition has no fill event",
            ));
        }
        let remaining_fills = fills_by_order
            .iter()
            .map(|(order_id, fills)| (*order_id, fills.len()))
            .collect::<BTreeMap<_, _>>();
        let mut sorted_fills = fills_by_order
            .into_iter()
            .map(|(order_id, mut fills)| -> Result<_, StepFatal> {
                fills.sort_by_key(|fill| std::cmp::Reverse(fill.0));
                if fills.windows(2).any(|pair| pair[0].1 != pair[1].0) {
                    return Err(invariant("continuous retail fill quantity chain is broken"));
                }
                Ok((
                    order_id,
                    fills
                        .into_iter()
                        .map(|(_, _, event)| event)
                        .collect::<Vec<_>>()
                        .into_iter(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        for event in &mut events {
            if let RetailOrderDiagnosticEvent::Filled { order_id, .. } = event {
                *event = sorted_fills
                    .get_mut(order_id)
                    .and_then(Iterator::next)
                    .ok_or_else(|| invariant("continuous retail fill ordering lost a receipt"))?;
            }
        }

        let mut submitted_this_tick = BTreeSet::<OrderId>::new();
        for event in &events {
            if let RetailOrderDiagnosticEvent::Submitted { order_id, .. } = event {
                if !submitted_this_tick.insert(*order_id) {
                    return Err(invariant(
                        "continuous retail lifecycle submits an order twice",
                    ));
                }
            }
        }

        let mut batch = Self {
            submitted_this_tick,
            published: BTreeSet::new(),
            remaining_fills,
            pending_fills: BTreeMap::new(),
            pending_terminal: BTreeMap::new(),
            ordered: Vec::with_capacity(events.len()),
        };
        for event in events {
            batch.emit_event(event)?;
        }
        Ok(batch)
    }

    fn emit_event(&mut self, event: RetailOrderDiagnosticEvent) -> Result<(), StepFatal> {
        match &event {
            RetailOrderDiagnosticEvent::Filled { order_id, .. }
                if self.submitted_this_tick.contains(order_id)
                    && !self.published.contains(order_id) =>
            {
                self.pending_fills.entry(*order_id).or_default().push(event);
            }
            RetailOrderDiagnosticEvent::Submitted { order_id, .. } => {
                let order_id = *order_id;
                self.published.insert(order_id);
                self.ordered.push(event);
                if let Some(fills) = self.pending_fills.remove(&order_id) {
                    for fill in fills {
                        self.emit_fill(order_id, fill)?;
                    }
                }
                if self.remaining_fills.get(&order_id).copied().unwrap_or(0) == 0 {
                    if let Some(terminal) = self.pending_terminal.remove(&order_id) {
                        self.ordered.push(terminal);
                    }
                }
            }
            RetailOrderDiagnosticEvent::Filled { order_id, .. } => {
                self.emit_fill(*order_id, event)?
            }
            RetailOrderDiagnosticEvent::Canceled { order_id, .. }
            | RetailOrderDiagnosticEvent::Aborted { order_id, .. }
                if self.remaining_fills.get(order_id).copied().unwrap_or(0) > 0
                    || (self.submitted_this_tick.contains(order_id)
                        && !self.published.contains(order_id)) =>
            {
                if self.pending_terminal.insert(*order_id, event).is_some() {
                    return Err(invariant("continuous retail order has two terminal events"));
                }
            }
            _ => self.ordered.push(event),
        }

        Ok(())
    }

    fn emit_fill(
        &mut self,
        order_id: OrderId,
        fill: RetailOrderDiagnosticEvent,
    ) -> Result<(), StepFatal> {
        let remaining = self
            .remaining_fills
            .get_mut(&order_id)
            .ok_or_else(|| invariant("continuous retail fill has no local quantity chain"))?;
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| invariant("continuous retail fill was emitted twice"))?;
        self.ordered.push(fill);
        if *remaining == 0 {
            if let Some(terminal) = self.pending_terminal.remove(&order_id) {
                self.ordered.push(terminal);
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<RetailOrderDiagnosticEvent>, StepFatal> {
        if !self.pending_fills.is_empty() || !self.pending_terminal.is_empty() {
            return Err(invariant(
                "continuous retail order lifecycle has unresolved local dependencies",
            ));
        }
        Ok(self.ordered)
    }
}

fn validate_result_identities(results: &[CandidateValidationResult]) -> Result<(), StepFatal> {
    let mut keys = BTreeSet::new();
    let mut sealed_indices = BTreeSet::new();
    for result in results {
        if !keys.insert(result.key()) || !sealed_indices.insert(result.sealed_index()) {
            return Err(invariant(
                "continuous P3 results contain a duplicate identity",
            ));
        }
    }
    Ok(())
}

fn validate_operation(
    candidate: &IntentCandidate,
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
                } => {
                    actual_side == side
                        && (match price {
                            crate::LimitPrice::Fixed(requested) => actual_price == requested,
                            crate::LimitPrice::Highest | crate::LimitPrice::Lowest => true,
                        })
                        && remaining_qty <= qty
                }
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
    candidates: &IntentCandidateBatch,
) -> Result<BTreeMap<&IntentCandidateKey, &IntentCandidate>, StepFatal> {
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
            if session.state.retail_experience.contains_key(account) {
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
            if session.state.retail_experience.contains_key(account) {
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
    candidate: &IntentCandidate,
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
    if session.state.retail_experience.contains_key(&account) {
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
        ContinuousCancelRejection::OrderAlreadyFilled => RejectionReason::OrderAlreadyFilled,
        ContinuousCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountId, Side};

    #[test]
    fn independent_results_need_unique_identities_but_no_global_position_order() {
        let later = CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(1),
            sealed_index: 1,
            reason: RejectionReason::InsufficientCash,
        };
        let earlier = CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(0),
            sealed_index: 0,
            reason: RejectionReason::InsufficientCash,
        };
        assert!(validate_result_identities(&[later.clone(), earlier.clone()]).is_ok());
        let repeated_sealed = CandidateValidationResult::Rejected {
            key: IntentCandidateKey::player(2),
            sealed_index: 1,
            reason: RejectionReason::InsufficientCash,
        };
        assert!(validate_result_identities(&[later, repeated_sealed]).is_err());
        assert!(validate_result_identities(&[earlier.clone(), earlier]).is_err());
    }

    #[test]
    fn fill_waits_for_its_own_new_submission_without_sorting_unrelated_orders() {
        let code = StockCode("600001".to_owned());
        let a = OrderId(10);
        let b = OrderId(11);
        let submitted = |order_id| RetailOrderDiagnosticEvent::Submitted {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty: 100,
        };
        let filled = |order_id| RetailOrderDiagnosticEvent::Filled {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty: 100,
        };
        assert_eq!(
            order_lifecycle_events(
                vec![submitted(b), filled(a), submitted(a), filled(b)],
                BTreeMap::from([(1, (100, 0)), (3, (100, 0))]),
            )
            .unwrap(),
            vec![submitted(b), submitted(a), filled(a), filled(b)]
        );
        assert_eq!(
            order_lifecycle_events(
                vec![filled(OrderId(9)), submitted(a)],
                BTreeMap::from([(0, (100, 0))]),
            )
            .unwrap(),
            vec![filled(OrderId(9)), submitted(a)]
        );
    }

    #[test]
    fn one_order_keeps_receipt_fill_order_when_results_arrive_reversed() {
        let code = StockCode("600001".to_owned());
        let order_id = OrderId(10);
        let submitted = RetailOrderDiagnosticEvent::Submitted {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty: 100,
        };
        let filled = |qty| RetailOrderDiagnosticEvent::Filled {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty,
        };
        assert_eq!(
            order_lifecycle_events(
                vec![filled(60), submitted.clone(), filled(40)],
                BTreeMap::from([(0, (60, 0)), (2, (100, 60))]),
            )
            .unwrap(),
            vec![submitted, filled(40), filled(60)]
        );
    }

    #[test]
    fn old_order_fill_precedes_its_cancel_even_when_results_are_reversed() {
        let code = StockCode("600001".to_owned());
        let order_id = OrderId(10);
        let filled = RetailOrderDiagnosticEvent::Filled {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty: 40,
        };
        let canceled = RetailOrderDiagnosticEvent::Canceled {
            account: AccountId(1),
            code,
            order_id,
            remaining_qty: 60,
        };
        assert_eq!(
            order_lifecycle_events(
                vec![canceled.clone(), filled.clone()],
                BTreeMap::from([(1, (100, 60))]),
            )
            .unwrap(),
            vec![filled, canceled]
        );
    }
    #[test]
    fn lifecycle_batch_preserves_quantity_and_dependency_first_errors() {
        let code = StockCode("600001".to_owned());
        let order_id = OrderId(10);
        let submitted = RetailOrderDiagnosticEvent::Submitted {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty: 100,
        };
        let filled = |qty| RetailOrderDiagnosticEvent::Filled {
            account: AccountId(1),
            code: code.clone(),
            side: Side::Buy,
            order_id,
            qty,
        };
        let canceled = RetailOrderDiagnosticEvent::Canceled {
            account: AccountId(1),
            code: code.clone(),
            order_id,
            remaining_qty: 60,
        };
        let cases = [
            (
                vec![filled(40)],
                BTreeMap::new(),
                "continuous retail fill has no source quantity transition",
            ),
            (
                vec![],
                BTreeMap::from([(0, (100, 60))]),
                "continuous retail quantity transition has no fill event",
            ),
            (
                vec![filled(40), filled(30)],
                BTreeMap::from([(0, (100, 60)), (1, (50, 20))]),
                "continuous retail fill quantity chain is broken",
            ),
            (
                vec![submitted.clone(), submitted.clone()],
                BTreeMap::new(),
                "continuous retail lifecycle submits an order twice",
            ),
            (
                vec![canceled.clone(), canceled.clone(), filled(40)],
                BTreeMap::from([(2, (100, 60))]),
                "continuous retail order has two terminal events",
            ),
            (
                vec![submitted.clone(), submitted, filled(40)],
                BTreeMap::new(),
                "continuous retail fill has no source quantity transition",
            ),
        ];
        for (events, quantities, expected) in cases {
            assert_eq!(
                order_lifecycle_events(events, quantities).unwrap_err(),
                invariant(expected)
            );
        }
    }
    #[test]
    fn lifecycle_batch_rejects_unresolved_dependencies_without_publishing_output() {
        let order_id = OrderId(10);
        let fill = RetailOrderDiagnosticEvent::Filled {
            account: AccountId(1),
            code: StockCode("600001".to_owned()),
            side: Side::Buy,
            order_id,
            qty: 100,
        };
        let mut batch =
            ContinuousLifecycleEventBatch::from_events(vec![], BTreeMap::new()).unwrap();
        assert_eq!(
            batch.emit_fill(order_id, fill.clone()).unwrap_err(),
            invariant("continuous retail fill has no local quantity chain")
        );
        batch.remaining_fills.insert(order_id, 0);
        assert_eq!(
            batch.emit_fill(order_id, fill.clone()).unwrap_err(),
            invariant("continuous retail fill was emitted twice")
        );
        // 模拟尚未发布 Submitted 的缓冲状态，finish 必须拒绝交出 partial outbox。
        batch.submitted_this_tick.insert(order_id);
        batch.emit_event(fill).unwrap();
        assert_eq!(
            batch.finish().unwrap_err(),
            invariant("continuous retail order lifecycle has unresolved local dependencies")
        );
    }
}
