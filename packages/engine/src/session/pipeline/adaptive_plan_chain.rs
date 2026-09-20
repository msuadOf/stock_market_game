//! Complete root-plan scheduling against one P1 observation and incremental typed P3/P4 results.
//!
//! This module owns plan-fact consumption. P5/P6 still receive every receipt once, and P7 must
//! not repeat parent/PlanBook projection for identities returned in `consumed`. No public event
//! is inspected to determine a command outcome, including immediately fully filled orders.

use super::{
    p4_continuous::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousExecutionFact,
        ContinuousExecutionOutcome, ContinuousExecutionRound, ContinuousPlaceFact,
    },
    DecisionSnapshot, EnvelopeReceipt, P2Candidate, P2CandidateKey, P3CandidateResult,
    P3ConsumeOutcome, P3ValidatedOperation, ReceiptKind, ReceiptLocalKey, ReceiptSource, StepFatal,
};
use crate::session::{
    plan_chain_candidates::{FrozenPlanChainObservation, PlanChainOperationBatch},
    plan_execution::PlanRouteOutcome,
    OrderFillSettlement, PlanExecutionReport,
};
use crate::{AccountId, Event, GameSession, Intent, OrderId, RejectionReason, Side, StockCode};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct PlanChainFactConsumption {
    pub(super) operations: BTreeSet<(P2CandidateKey, u64)>,
    pub(super) receipts: BTreeSet<ReceiptLocalKey>,
}

pub(super) struct AdaptivePlanChainCompletion {
    pub(super) reports: Vec<PlanExecutionReport>,
    pub(super) events: Vec<Event>,
    pub(super) consumed: PlanChainFactConsumption,
}

pub(super) struct AdaptivePlanChainCoordinator {
    roots: PlanChainOperationBatch,
    observation: FrozenPlanChainObservation,
    pending: Option<P2Candidate>,
    consumed: PlanChainFactConsumption,
    projection_events: Vec<Event>,
    exhausted: bool,
    failed: bool,
}

impl AdaptivePlanChainCoordinator {
    pub(super) fn capture_roots_before_p4(
        session: &GameSession,
        accepted_due_npc_ids: &[AccountId],
        snapshot: &DecisionSnapshot,
    ) -> Result<Self, StepFatal> {
        let roots = session.capture_decision_chain_roots(accepted_due_npc_ids, snapshot)?;
        Self::capture_batch(session, roots)
    }

    fn capture_batch(
        session: &GameSession,
        roots: PlanChainOperationBatch,
    ) -> Result<Self, StepFatal> {
        Ok(Self {
            roots,
            observation: FrozenPlanChainObservation::capture(session)?,
            pending: None,
            consumed: PlanChainFactConsumption::default(),
            projection_events: Vec::new(),
            exhausted: false,
            failed: false,
        })
    }

    pub(super) fn next_candidate(
        &mut self,
        session: &mut GameSession,
    ) -> Result<Option<P2Candidate>, StepFatal> {
        self.ensure_active()?;
        if self.pending.is_some() {
            return self.fail(invariant(
                "cannot yield before delivering the pending command outcome",
            ));
        }
        let generated = self
            .roots
            .yield_adaptive_candidate(session, &mut self.observation);
        let candidate = match generated {
            Ok(candidate) => candidate.map(|candidate| {
                P2Candidate::new(
                    P2CandidateKey::plan_chain(candidate.chain_generation_index),
                    candidate.owner,
                    candidate.intent,
                )
            }),
            Err(error) => return self.fail(error),
        };
        self.exhausted = candidate.is_none();
        self.pending = candidate.clone();
        Ok(candidate)
    }

    pub(super) fn advance_after_typed_outcome(
        &mut self,
        session: &mut GameSession,
        step: &P3ConsumeOutcome,
        round: Option<&ContinuousExecutionRound>,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let result = self.advance(session, step, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn advance(
        &mut self,
        session: &mut GameSession,
        step: &P3ConsumeOutcome,
        round: Option<&ContinuousExecutionRound>,
    ) -> Result<(), StepFatal> {
        let candidate = self
            .pending
            .as_ref()
            .ok_or_else(|| invariant("typed result has no pending plan-chain command"))?;
        if candidate.key() != step.result().key() {
            return Err(invariant(
                "P3 result belongs to a different plan-chain candidate",
            ));
        }
        let outcome = match step.result() {
            P3CandidateResult::Rejected { reason, .. } => {
                if step.operation().is_some() || round.is_some() {
                    return Err(invariant(
                        "P3-rejected plan command cannot carry a P4 execution",
                    ));
                }
                PlanRouteOutcome::Rejected(reason.clone())
            }
            P3CandidateResult::Accepted { sealed_index, .. } => {
                let operation = step
                    .operation()
                    .ok_or_else(|| invariant("P3-accepted plan command has no operation"))?;
                if operation.candidate_key() != candidate.key()
                    || operation.sealed_index() != *sealed_index
                {
                    return Err(invariant(
                        "P3 operation identity disagrees with its candidate result",
                    ));
                }
                let round =
                    round.ok_or_else(|| invariant("P3-accepted plan command has no P4 result"))?;
                if round.facts.len() != 1 {
                    return Err(invariant(
                        "one yielded command must receive exactly one P4 operation fact",
                    ));
                }
                let fact = &round.facts[0];
                validate_command_fact(candidate, operation, fact)?;
                let outcome = route_outcome(&fact.outcome);
                self.project_execution_round(session, round)?;
                outcome
            }
        };
        self.pending = None;
        self.roots.resume_adaptive_candidate(session, outcome)
    }

    /// Also consumes NPC/player results before visiting the first plan root. The market and
    /// working-order view advances, while the frozen account/market decision view does not.
    pub(super) fn project_execution_round(
        &mut self,
        session: &mut GameSession,
        round: &ContinuousExecutionRound,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let result = self.project_round(session, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn project_round(
        &mut self,
        session: &mut GameSession,
        round: &ContinuousExecutionRound,
    ) -> Result<(), StepFatal> {
        let mut operation_ids = self.consumed.operations.clone();
        let mut receipt_ids = self.consumed.receipts.clone();
        let mut sealed = BTreeSet::new();
        let mut candidates = BTreeSet::new();
        for fact in &round.facts {
            if !operation_ids.insert((fact.candidate_key.clone(), fact.sealed_index))
                || !sealed.insert(fact.sealed_index)
                || !candidates.insert(fact.candidate_key.clone())
                || self
                    .consumed
                    .operations
                    .iter()
                    .any(|(key, index)| key == &fact.candidate_key || *index == fact.sealed_index)
            {
                return Err(invariant(
                    "duplicate candidate or sealed identity in plan projection",
                ));
            }
            validate_fact_identity(fact)?;
        }
        for receipt in &round.receipts {
            if !receipt_ids.insert(receipt.local_key.clone()) {
                return Err(invariant("duplicate receipt identity in plan projection"));
            }
        }
        for (code, projection) in &round.projections {
            if !session.markets.contains_key(code) {
                return Err(invariant("plan projection returned an unknown stock"));
            }
            session
                .markets
                .insert(code.clone(), projection.market.clone());
        }
        let mut ordered: Vec<_> = round.facts.iter().collect();
        ordered.sort_by_key(|fact| fact.sealed_index);
        let mut consumed_receipts = BTreeSet::new();
        for fact in ordered {
            match &fact.outcome {
                ContinuousExecutionOutcome::Place { fact, original_qty } => match fact {
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
                    } => {
                        validate_parent_acceptance(session, *account, code, *side, *original_qty)?;
                        session.record_parent_order_submission(
                            *account,
                            code,
                            *side,
                            *order_id,
                            *original_qty,
                            &mut self.projection_events,
                        );
                    }
                    ContinuousPlaceFact::Rejected { .. } => {}
                },
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
                    account,
                    code,
                    order_id,
                    ..
                }) => {
                    session.record_parent_order_canceled(*account, code, *order_id);
                    session.remove_npc_order_lifecycle(*account, code, *order_id);
                }
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { .. }) => {}
            }
            let mut receipts: Vec<_> = round
                .receipts
                .iter()
                .filter(|receipt| {
                    receipt.local_key.source() == ReceiptSource::SealedIntent(fact.sealed_index)
                })
                .collect();
            receipts.sort_by(|left, right| left.local_key.cmp(&right.local_key));
            for receipt in receipts {
                project_receipt(session, receipt, &mut self.projection_events)?;
                consumed_receipts.insert(receipt.local_key.clone());
            }
        }
        // Auction/day-end finalizer receipts have no command outcome and are consumed only
        // after that finalizer actually runs. A future command source cannot appear here.
        for receipt in &round.receipts {
            if !consumed_receipts.contains(&receipt.local_key) {
                if matches!(receipt.local_key.source(), ReceiptSource::SealedIntent(_)) {
                    return Err(invariant(
                        "sealed receipt has no operation in this projection round",
                    ));
                }
                project_receipt(session, receipt, &mut self.projection_events)?;
            }
        }
        let mut plans = std::mem::take(&mut session.plans);
        let synchronized = session.synchronize_plan_execution(&mut plans);
        session.plans = plans;
        synchronized.map_err(|error| invariant(&error.to_string()))?;
        self.consumed.operations = operation_ids;
        self.consumed.receipts = receipt_ids;
        Ok(())
    }

    pub(super) fn finish(self) -> Result<AdaptivePlanChainCompletion, StepFatal> {
        self.ensure_active()?;
        if !self.exhausted || self.pending.is_some() {
            return Err(invariant(
                "plan-chain coordinator finished before exhausting its roots",
            ));
        }
        let reports = self.roots.finish_adaptive()?;
        let mut events = self.projection_events;
        events.extend(
            reports
                .iter()
                .flat_map(|report| report.events.iter().cloned()),
        );
        Ok(AdaptivePlanChainCompletion {
            reports,
            events,
            consumed: self.consumed,
        })
    }

    fn ensure_active(&self) -> Result<(), StepFatal> {
        if self.failed {
            Err(invariant(
                "plan-chain coordinator stopped after a typed failure",
            ))
        } else {
            Ok(())
        }
    }

    fn fail<T>(&mut self, error: StepFatal) -> Result<T, StepFatal> {
        self.failed = true;
        Err(error)
    }
}

fn validate_command_fact(
    candidate: &P2Candidate,
    operation: &P3ValidatedOperation,
    fact: &ContinuousExecutionFact,
) -> Result<(), StepFatal> {
    validate_fact_identity(fact)?;
    if fact.candidate_key != *candidate.key() || fact.sealed_index != operation.sealed_index() {
        return Err(invariant(
            "P4 fact does not match the yielded candidate/sealed identity",
        ));
    }
    let valid = match (candidate.intent(), operation, &fact.outcome) {
        (
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            },
            P3ValidatedOperation::Place(draft),
            ContinuousExecutionOutcome::Place {
                fact: place,
                original_qty,
            },
        ) => {
            let (owner, stock, order) = place_identity(place);
            let details_match = match place {
                ContinuousPlaceFact::Resting {
                    side: actual_side,
                    price: actual_price,
                    ..
                } => actual_side == side && actual_price == price,
                ContinuousPlaceFact::Filled {
                    side: actual_side,
                    filled_qty,
                    ..
                } => actual_side == side && filled_qty == qty,
                ContinuousPlaceFact::Rejected { .. } => true,
            };
            owner == candidate.owner()
                && stock == code
                && draft.order_id() == order
                && draft.owner() == owner
                && draft.code() == code
                && draft.side() == *side
                && draft.limit() == *price
                && draft.qty() == *qty
                && original_qty == qty
                && fact.allocated_order_id == Some(order)
                && details_match
        }
        (
            Intent::Cancel { code, id },
            P3ValidatedOperation::Cancel {
                account,
                code: actual,
                order_id,
                ..
            },
            ContinuousExecutionOutcome::Cancel(cancel),
        ) => {
            let (owner, stock, order) = cancel_identity(cancel);
            owner == candidate.owner()
                && owner == *account
                && stock == code
                && actual == code
                && order == *id
                && order == *order_id
                && fact.allocated_order_id.is_none()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invariant("P4 payload does not match the yielded command"))
    }
}

impl AdaptivePlanChainCompletion {
    /// Appends to the caller's shared phase-6 Session ordinal domain. This allocates no global
    /// event sequence; the final P7 collector assigns it with every other producer exactly once.
    pub(super) fn take_event_facts(
        &mut self,
        next_session_local_index: &mut u64,
    ) -> Result<Vec<super::p7_events::OwnedEventFact>, StepFatal> {
        let mut cursor = *next_session_local_index;
        let mut facts = Vec::with_capacity(self.events.len());
        for event in &self.events {
            if !matches!(event, Event::ResourceLimit { .. }) {
                return Err(invariant(
                    "plan continuation emitted an unsupported presentation event",
                ));
            }
            let index = cursor;
            cursor = cursor
                .checked_add(1)
                .ok_or_else(|| invariant("plan continuation Session event ordinal overflow"))?;
            facts.push(super::p7_events::OwnedEventFact {
                key: super::EventStableKey::for_event(event, index),
                event: event.clone(),
            });
        }
        self.events.clear();
        *next_session_local_index = cursor;
        Ok(facts)
    }
}

fn validate_fact_identity(fact: &ContinuousExecutionFact) -> Result<(), StepFatal> {
    let valid = match &fact.outcome {
        ContinuousExecutionOutcome::Place {
            fact: place,
            original_qty,
        } => {
            let inner_index = match place {
                ContinuousPlaceFact::Resting {
                    sealed_index,
                    remaining_qty,
                    ..
                } => {
                    if *remaining_qty == 0 || remaining_qty > original_qty {
                        return Err(invariant("invalid resting quantity in plan feedback"));
                    }
                    *sealed_index
                }
                ContinuousPlaceFact::Filled {
                    sealed_index,
                    filled_qty,
                    ..
                } => {
                    // A market order can terminate after a partial fill; its remaining
                    // reservation has a separate Release receipt. Plan commands are limits
                    // and get the stronger equality check in validate_command_fact.
                    if filled_qty > original_qty {
                        return Err(invariant(
                            "terminal fill feedback exceeds original order quantity",
                        ));
                    }
                    *sealed_index
                }
                ContinuousPlaceFact::Rejected { sealed_index, .. } => *sealed_index,
            };
            inner_index == fact.sealed_index
                && fact.allocated_order_id == Some(place_identity(place).2)
                && *original_qty > 0
        }
        ContinuousExecutionOutcome::Cancel(cancel) => {
            let inner_index = match cancel {
                ContinuousCancelFact::Canceled { sealed_index, .. }
                | ContinuousCancelFact::Rejected { sealed_index, .. } => *sealed_index,
            };
            inner_index == fact.sealed_index && fact.allocated_order_id.is_none()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(invariant("P4 fact has inconsistent sealed/order identity"))
    }
}

fn place_identity(fact: &ContinuousPlaceFact) -> (AccountId, &StockCode, OrderId) {
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

fn cancel_identity(fact: &ContinuousCancelFact) -> (AccountId, &StockCode, OrderId) {
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

fn route_outcome(outcome: &ContinuousExecutionOutcome) -> PlanRouteOutcome {
    match outcome {
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Resting { order_id, .. }
                | ContinuousPlaceFact::Filled { order_id, .. },
            ..
        } => PlanRouteOutcome::Accepted(*order_id),
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Rejected { reason, .. },
            ..
        } => PlanRouteOutcome::Rejected(reason.clone()),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled { order_id, .. }) => {
            PlanRouteOutcome::Canceled(*order_id)
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { reason, .. }) => {
            PlanRouteOutcome::Rejected(match reason {
                ContinuousCancelRejection::UnknownStock => RejectionReason::UnknownStock,
                ContinuousCancelRejection::OrderNotFound => RejectionReason::OrderNotFound,
                ContinuousCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
                ContinuousCancelRejection::SameTickEnvelope => {
                    RejectionReason::SameTickOrderNotCancelable
                }
            })
        }
    }
}

fn validate_parent_acceptance(
    session: &GameSession,
    account: AccountId,
    code: &StockCode,
    side: Side,
    qty: u32,
) -> Result<(), StepFatal> {
    if let Some(parent) = session
        .parent_orders
        .get(&account)
        .and_then(|parents| parents.get(code))
    {
        if parent.side == side {
            if parent.active_child_order_id.is_some() {
                return Err(invariant(
                    "typed acceptance would install a second active parent child",
                ));
            }
            if qty == 0 || qty > parent.target_qty.saturating_sub(parent.filled_qty) {
                return Err(invariant(
                    "typed acceptance exceeds parent remaining quantity",
                ));
            }
            if parent.linked_plan_id.is_some() && !session.has_pending_plan_event_capacity(1) {
                return Err(invariant(
                    "parent acceptance exceeds pending plan fact capacity",
                ));
            }
        }
    }
    Ok(())
}

fn project_receipt(
    session: &mut GameSession,
    receipt: &EnvelopeReceipt,
    events: &mut Vec<Event>,
) -> Result<(), StepFatal> {
    let key = &receipt.envelope;
    if matches!(receipt.kind, ReceiptKind::Release | ReceiptKind::Reject) {
        session.record_parent_order_canceled(key.account, &key.stock, key.order);
        session.remove_npc_order_lifecycle(key.account, &key.stock, key.order);
        return Ok(());
    }
    if receipt.kind != ReceiptKind::Fill {
        return Ok(());
    }
    let qty = receipt
        .qty_before
        .checked_sub(receipt.qty_after)
        .filter(|qty| *qty > 0)
        .ok_or_else(|| invariant("plan fill receipt has no positive quantity"))?;
    let gross = receipt
        .value_after
        .sub(receipt.value_before)
        .map_err(|error| invariant(&error.to_string()))?;
    if let Some(parent) = session
        .parent_orders
        .get(&key.account)
        .and_then(|parents| parents.get(&key.stock))
    {
        if parent.side == key.side && parent.active_child_order_id == Some(key.order) {
            if !parent
                .active_child_remaining_qty
                .is_some_and(|remaining| qty <= remaining)
                || parent
                    .filled_qty
                    .checked_add(qty)
                    .is_none_or(|filled| filled > parent.target_qty)
            {
                return Err(invariant(
                    "typed fill exceeds parent child or target quantity",
                ));
            }
        }
    }
    let fill = OrderFillSettlement {
        account: key.account,
        side: key.side,
        order_id: key.order,
        filled_value_before: receipt.value_before,
        gross,
        qty,
    };
    if !session.can_record_parent_order_fills(&key.stock, &[fill], None) {
        return Err(invariant("typed fills exceed pending plan fact capacity"));
    }
    session.record_parent_order_fills(&key.stock, &[fill], events);
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::adaptive_plan_chain".to_owned(),
    }
}

#[cfg(test)]
#[path = "adaptive_plan_chain_tests.rs"]
mod tests;
