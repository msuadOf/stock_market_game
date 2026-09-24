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
    stock_auction::b2_auction_day_end::{
        AuctionExecutionFact, AuctionExecutionRound, AuctionLifecycleFact,
    },
    DecisionSnapshot, EnvelopeReceipt, P2Candidate, P2CandidateKey, P3CandidateResult,
    P3ConsumeOutcome, P3ValidatedOperation, ReceiptKind, ReceiptLocalKey, ReceiptSource, StepFatal,
};
#[cfg(feature = "simulation-diagnostics")]
use crate::session::plan_execution::PlanCancelCause;
use crate::session::{
    plan_chain_candidates::{FrozenPlanChainObservation, PlanChainOperationBatch},
    plan_execution::PlanRouteOutcome,
    OrderFillSettlement, PlanExecutionReport,
};
use crate::{AccountId, Event, GameSession, Intent, OrderId, RejectionReason, Side, StockCode};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct PlanChainFactConsumption {
    pub(super) operations: BTreeSet<(P2CandidateKey, u64)>,
    pub(super) receipts: BTreeSet<ReceiptLocalKey>,
    candidate_keys: BTreeSet<P2CandidateKey>,
    sealed_indices: BTreeSet<u64>,
}

pub(super) struct AdaptivePlanChainCompletion {
    pub(super) reports: Vec<PlanExecutionReport>,
    pub(super) events: Vec<Event>,
    pub(super) consumed: PlanChainFactConsumption,
}

pub(super) struct AdaptivePlanChainCoordinator {
    roots: PlanChainOperationBatch,
    observation: Option<FrozenPlanChainObservation>,
    pending: Vec<P2Candidate>,
    #[cfg(feature = "simulation-diagnostics")]
    pending_cancel_causes: BTreeMap<P2CandidateKey, PlanCancelCause>,
    consumed: PlanChainFactConsumption,
    projection_events: Vec<Event>,
    exhausted: bool,
    failed: bool,
}

fn intent_code(intent: &Intent) -> &StockCode {
    match intent {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => code,
    }
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

    pub(super) fn capture_batch(
        session: &GameSession,
        roots: PlanChainOperationBatch,
    ) -> Result<Self, StepFatal> {
        let observation = if roots.is_empty() {
            None
        } else {
            Some(FrozenPlanChainObservation::capture(session)?)
        };
        Ok(Self {
            roots,
            observation,
            pending: Vec::new(),
            #[cfg(feature = "simulation-diagnostics")]
            pending_cancel_causes: BTreeMap::new(),
            consumed: PlanChainFactConsumption::default(),
            projection_events: Vec::new(),
            exhausted: false,
            failed: false,
        })
    }

    pub(super) fn next_ready_batch(
        &mut self,
        session: &mut GameSession,
    ) -> Result<Vec<P2Candidate>, StepFatal> {
        self.ensure_active()?;
        if !self.pending.is_empty() {
            return self.fail(invariant(
                "cannot yield before delivering the pending command outcomes",
            ));
        }
        let Some(observation) = self.observation.as_mut() else {
            if !self.roots.is_empty() {
                return self.fail(invariant(
                    "plan-chain roots exist without a frozen observation",
                ));
            }
            self.exhausted = true;
            return Ok(Vec::new());
        };
        let generated = self.roots.yield_adaptive_candidates(session, observation);
        let candidates = match generated {
            Ok(candidates) => candidates
                .into_iter()
                .map(|candidate| {
                    P2Candidate::new(
                        P2CandidateKey::plan_chain(candidate.chain_generation_index),
                        candidate.owner,
                        candidate.intent,
                    )
                })
                .collect::<Vec<_>>(),
            Err(error) => return self.fail(error),
        };
        #[cfg(feature = "simulation-diagnostics")]
        {
            for candidate in &candidates {
                let P2CandidateKey::PlanChain {
                    chain_generation_index,
                } = candidate.key()
                else {
                    return self.fail(invariant("non-plan candidate yielded by plan chain"));
                };
                let cause = self.roots.pending_cancel_cause(*chain_generation_index);
                if matches!(candidate.intent(), Intent::Cancel { .. }) != cause.is_some() {
                    return self.fail(invariant(
                        "plan-chain cancellation lost its typed causal provenance",
                    ));
                }
                if let Some(cause) = cause {
                    self.pending_cancel_causes
                        .insert(candidate.key().clone(), cause);
                }
            }
        }
        self.exhausted = candidates.is_empty();
        self.pending = candidates.clone();
        Ok(candidates)
    }

    pub(super) fn advance_after_typed_outcomes(
        &mut self,
        session: &mut GameSession,
        steps: &[P3ConsumeOutcome],
        round: Option<&ContinuousExecutionRound>,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let result = self.advance_continuous_batch(session, steps, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(super) fn advance_after_auction_outcomes(
        &mut self,
        session: &mut GameSession,
        steps: &[P3ConsumeOutcome],
        round: Option<&AuctionExecutionRound>,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let result = self.advance_auction_batch(session, steps, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn advance_continuous_batch(
        &mut self,
        session: &mut GameSession,
        steps: &[P3ConsumeOutcome],
        round: Option<&ContinuousExecutionRound>,
    ) -> Result<(), StepFatal> {
        self.validate_pending_prefix(steps)?;
        let mut facts = BTreeMap::new();
        if let Some(round) = round {
            for fact in &round.facts {
                if facts
                    .insert((fact.candidate_key.clone(), fact.sealed_index), fact)
                    .is_some()
                {
                    return Err(invariant("P4 returned a duplicate plan command fact"));
                }
            }
        }
        let mut outcomes = Vec::with_capacity(steps.len());
        for (candidate, step) in self.pending.iter().zip(steps) {
            let outcome = match step.result() {
                P3CandidateResult::Rejected { reason, .. } => {
                    if step.operation().is_some() {
                        return Err(invariant("P3-rejected command carries an operation"));
                    }
                    PlanRouteOutcome::Rejected(reason.clone())
                }
                P3CandidateResult::PendingPlanEventsLimited { .. } => {
                    if step.operation().is_some() {
                        return Err(invariant("resource-limited command carries an operation"));
                    }
                    PlanRouteOutcome::ResourceLimited
                }
                P3CandidateResult::Accepted { sealed_index, .. } => {
                    let operation = step
                        .operation()
                        .ok_or_else(|| invariant("P3-accepted command has no operation"))?;
                    if operation.candidate_key() != candidate.key()
                        || operation.sealed_index() != *sealed_index
                    {
                        return Err(invariant(
                            "P3 operation identity disagrees with its candidate result",
                        ));
                    }
                    let fact = facts
                        .remove(&(candidate.key().clone(), *sealed_index))
                        .ok_or_else(|| invariant("P3-accepted command has no P4 fact"))?;
                    validate_command_fact(candidate, operation, fact)?;
                    route_outcome(&fact.outcome)
                }
            };
            let P2CandidateKey::PlanChain {
                chain_generation_index,
            } = candidate.key()
            else {
                return Err(invariant("pending candidate is not a plan command"));
            };
            outcomes.push((
                candidate.owner(),
                intent_code(candidate.intent()).clone(),
                *chain_generation_index,
                outcome,
            ));
        }
        if !facts.is_empty() {
            return Err(invariant("P4 returned an extra plan command fact"));
        }
        if let Some(round) = round {
            self.project_execution_round(session, round)?;
        }
        self.pending.drain(..steps.len());
        #[cfg(feature = "simulation-diagnostics")]
        for step in steps {
            self.pending_cancel_causes.remove(step.candidate_key());
        }
        self.roots.resume_adaptive_candidates(session, outcomes)
    }

    fn advance_auction_batch(
        &mut self,
        session: &mut GameSession,
        steps: &[P3ConsumeOutcome],
        round: Option<&AuctionExecutionRound>,
    ) -> Result<(), StepFatal> {
        self.validate_pending_prefix(steps)?;
        let mut facts = BTreeMap::new();
        if let Some(round) = round {
            for fact in &round.facts {
                if facts
                    .insert((fact.candidate_key.clone(), fact.sealed_index), fact)
                    .is_some()
                {
                    return Err(invariant(
                        "P4 returned a duplicate auction plan command fact",
                    ));
                }
            }
        }
        let mut outcomes = Vec::with_capacity(steps.len());
        for (candidate, step) in self.pending.iter().zip(steps) {
            let outcome = match step.result() {
                P3CandidateResult::Rejected { reason, .. } => {
                    if step.operation().is_some() {
                        return Err(invariant(
                            "P3-rejected auction command carries an operation",
                        ));
                    }
                    PlanRouteOutcome::Rejected(reason.clone())
                }
                P3CandidateResult::PendingPlanEventsLimited { .. } => {
                    if step.operation().is_some() {
                        return Err(invariant(
                            "resource-limited auction command carries an operation",
                        ));
                    }
                    PlanRouteOutcome::ResourceLimited
                }
                P3CandidateResult::Accepted { sealed_index, .. } => {
                    let operation = step
                        .operation()
                        .ok_or_else(|| invariant("P3-accepted auction command has no operation"))?;
                    if operation.candidate_key() != candidate.key()
                        || operation.sealed_index() != *sealed_index
                    {
                        return Err(invariant(
                            "auction P3 operation identity disagrees with its candidate result",
                        ));
                    }
                    let fact = facts
                        .remove(&(candidate.key().clone(), *sealed_index))
                        .ok_or_else(|| invariant("P3-accepted auction command has no P4 fact"))?;
                    validate_auction_command_fact(candidate, operation, fact)?;
                    auction_route_outcome(&fact.outcome)
                }
            };
            let P2CandidateKey::PlanChain {
                chain_generation_index,
            } = candidate.key()
            else {
                return Err(invariant("pending candidate is not a plan command"));
            };
            outcomes.push((
                candidate.owner(),
                intent_code(candidate.intent()).clone(),
                *chain_generation_index,
                outcome,
            ));
        }
        if !facts.is_empty() {
            return Err(invariant("P4 returned an extra auction plan command fact"));
        }
        if let Some(round) = round {
            self.project_auction_execution_round(session, round)?;
        }
        self.pending.drain(..steps.len());
        #[cfg(feature = "simulation-diagnostics")]
        for step in steps {
            self.pending_cancel_causes.remove(step.candidate_key());
        }
        self.roots.resume_adaptive_candidates(session, outcomes)
    }

    fn validate_pending_prefix(&self, steps: &[P3ConsumeOutcome]) -> Result<(), StepFatal> {
        if steps.is_empty() || steps.len() > self.pending.len() {
            return Err(invariant("typed result batch is not a pending plan prefix"));
        }
        for (candidate, step) in self.pending.iter().zip(steps) {
            if candidate.key() != step.result().key() {
                return Err(invariant(
                    "P3 result belongs to a different plan-chain candidate",
                ));
            }
        }
        Ok(())
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

    pub(super) fn project_auction_execution_round(
        &mut self,
        session: &mut GameSession,
        round: &AuctionExecutionRound,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let result = self.project_auction_round(session, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn project_auction_round(
        &mut self,
        session: &mut GameSession,
        round: &AuctionExecutionRound,
    ) -> Result<(), StepFatal> {
        let mut operation_ids = BTreeSet::new();
        let mut receipt_ids = BTreeSet::new();
        let mut sealed = BTreeSet::new();
        let mut candidates = BTreeSet::new();
        for fact in &round.facts {
            if self.consumed.candidate_keys.contains(&fact.candidate_key)
                || self.consumed.sealed_indices.contains(&fact.sealed_index)
                || !operation_ids.insert((fact.candidate_key.clone(), fact.sealed_index))
                || !sealed.insert(fact.sealed_index)
                || !candidates.insert(fact.candidate_key.clone())
            {
                return Err(invariant(
                    "duplicate candidate or sealed identity in auction plan projection",
                ));
            }
            validate_auction_fact_identity(fact)?;
        }
        for receipt in &round.receipts {
            if self.consumed.receipts.contains(&receipt.local_key)
                || !receipt_ids.insert(receipt.local_key.clone())
            {
                return Err(invariant(
                    "duplicate receipt identity in auction plan projection",
                ));
            }
        }
        for (code, projection) in &round.projections {
            if !session.markets.contains_key(code) {
                return Err(invariant("auction projection returned an unknown stock"));
            }
            if projection.orders.is_empty() {
                session.auction_orders.remove(code);
            } else {
                session
                    .auction_orders
                    .insert(code.clone(), projection.orders.clone());
            }
        }
        rebuild_auction_order_counts(session)?;

        let mut ordered = round.facts.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|fact| fact.sealed_index);
        for fact in ordered {
            #[cfg(feature = "simulation-diagnostics")]
            project_auction_causal_operation(
                session,
                fact,
                auction_causal_cancel_termination(
                    &self.pending,
                    &self.pending_cancel_causes,
                    fact,
                )?,
            );
            match &fact.outcome {
                AuctionLifecycleFact::Accepted {
                    account,
                    code,
                    order_id,
                    side,
                    qty,
                    ..
                } => {
                    validate_parent_acceptance(session, *account, code, *side, *qty)?;
                    session.record_parent_order_submission(
                        *account,
                        code,
                        *side,
                        *order_id,
                        *qty,
                        &mut self.projection_events,
                    );
                }
                AuctionLifecycleFact::Canceled {
                    account,
                    code,
                    order_id,
                    ..
                } => {
                    session.record_parent_order_canceled(*account, code, *order_id);
                    session.remove_npc_order_lifecycle(*account, code, *order_id);
                }
                AuctionLifecycleFact::Rejected { .. } => {}
            }
            let mut receipts = round
                .receipts
                .iter()
                .filter(|receipt| {
                    receipt.local_key.source() == ReceiptSource::SealedIntent(fact.sealed_index)
                })
                .collect::<Vec<_>>();
            receipts.sort_by(|left, right| left.local_key.cmp(&right.local_key));
            for receipt in receipts {
                project_receipt(session, receipt, &mut self.projection_events)?;
            }
            synchronize_projected_plans(session)?;
        }
        if round
            .receipts
            .iter()
            .any(|receipt| !matches!(receipt.local_key.source(), ReceiptSource::SealedIntent(_)))
        {
            return Err(invariant(
                "auction operation round exposed a finalizer receipt before finish",
            ));
        }
        if round.facts.is_empty() {
            synchronize_projected_plans(session)?;
        }
        self.consumed.operations.extend(operation_ids);
        self.consumed.receipts.extend(receipt_ids);
        self.consumed.candidate_keys.extend(candidates);
        self.consumed.sealed_indices.extend(sealed);
        Ok(())
    }

    fn project_round(
        &mut self,
        session: &mut GameSession,
        round: &ContinuousExecutionRound,
    ) -> Result<(), StepFatal> {
        let mut operation_ids = BTreeSet::new();
        let mut receipt_ids = BTreeSet::new();
        let mut sealed = BTreeSet::new();
        let mut candidates = BTreeSet::new();
        for fact in &round.facts {
            if self.consumed.candidate_keys.contains(&fact.candidate_key)
                || self.consumed.sealed_indices.contains(&fact.sealed_index)
                || !operation_ids.insert((fact.candidate_key.clone(), fact.sealed_index))
                || !sealed.insert(fact.sealed_index)
                || !candidates.insert(fact.candidate_key.clone())
            {
                return Err(invariant(
                    "duplicate candidate or sealed identity in plan projection",
                ));
            }
            validate_fact_identity(fact)?;
        }
        for receipt in &round.receipts {
            if self.consumed.receipts.contains(&receipt.local_key)
                || !receipt_ids.insert(receipt.local_key.clone())
            {
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
            #[cfg(feature = "simulation-diagnostics")]
            project_continuous_causal_start(
                session,
                round,
                fact,
                causal_cancel_termination(&self.pending, &self.pending_cancel_causes, fact)?,
            )?;
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
                        if let ContinuousPlaceFact::Resting {
                            sealed_index,
                            price,
                            remaining_qty,
                            ..
                        } = fact
                        {
                            let quote = round
                                .projections
                                .get(code)
                                .and_then(|projection| {
                                    projection.acceptance_quotes.get(sealed_index)
                                })
                                .ok_or_else(|| {
                                    invariant("resting NPC projection has no acceptance quote")
                                })?;
                            if quote.order.id != *order_id
                                || quote.order.owner != *account
                                || quote.order.side != *side
                                || quote.order.price != *price
                                || quote.order.qty != *remaining_qty
                            {
                                return Err(invariant(
                                    "resting acceptance quote disagrees with typed order fact",
                                ));
                            }
                            // Use the actual acceptance-time quote, even when a later operation
                            // in this round changes the market. Parent submission precedes this
                            // check so parent children retain their own execution horizon.
                            session.register_npc_order_lifecycle_at_quote(
                                *account,
                                code,
                                &quote.order,
                                quote.last_price,
                                quote.best_bid,
                                quote.best_ask,
                            );
                        }
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
            #[cfg(feature = "simulation-diagnostics")]
            project_continuous_causal_end(session, round, fact)?;
            // A completed linked parent must disappear before a later independent operation
            // can be accepted on that account/stock/side, just as in single-operation rounds.
            synchronize_projected_plans(session)?;
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
        if round.facts.is_empty() || consumed_receipts.len() < round.receipts.len() {
            synchronize_projected_plans(session)?;
        }
        // Scan only changed stocks once per round; unrelated books cannot invalidate a quote.
        let live_ids: BTreeSet<_> = round
            .projections
            .values()
            .flat_map(|projection| {
                projection
                    .market
                    .resting_orders()
                    .into_iter()
                    .map(|order| order.id)
            })
            .collect();
        session.npc_order_lifecycles.retain(|lifecycle| {
            !round.projections.contains_key(&lifecycle.code)
                || live_ids.contains(&lifecycle.order_id)
        });
        self.consumed.operations.extend(operation_ids);
        self.consumed.receipts.extend(receipt_ids);
        self.consumed.candidate_keys.extend(candidates);
        self.consumed.sealed_indices.extend(sealed);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<AdaptivePlanChainCompletion, StepFatal> {
        self.ensure_active()?;
        if !self.exhausted || !self.pending.is_empty() {
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

#[cfg(feature = "simulation-diagnostics")]
fn project_auction_causal_operation(
    session: &mut GameSession,
    fact: &AuctionExecutionFact,
    cancel_termination: crate::diagnostics::causal::Termination,
) {
    match &fact.outcome {
        AuctionLifecycleFact::Accepted {
            account,
            code,
            order_id,
            side,
            qty,
            ..
        } => {
            let quote = session.causal_quote(code);
            session.causal_submitted_with_quote(*account, *order_id, code, *side, *qty, quote);
        }
        AuctionLifecycleFact::Canceled {
            account,
            code,
            order_id,
            remaining_qty,
            ..
        } => session.causal_terminated(
            (*account, *order_id, *remaining_qty),
            code,
            cancel_termination,
        ),
        AuctionLifecycleFact::Rejected { .. } => {}
    }
}

#[cfg(feature = "simulation-diagnostics")]
fn auction_causal_cancel_termination(
    pending: &[P2Candidate],
    pending_causes: &BTreeMap<P2CandidateKey, PlanCancelCause>,
    fact: &AuctionExecutionFact,
) -> Result<crate::diagnostics::causal::Termination, StepFatal> {
    use crate::diagnostics::causal::Termination;

    if !matches!(fact.outcome, AuctionLifecycleFact::Canceled { .. }) {
        return Ok(Termination::Voluntary);
    }
    let is_pending = pending
        .iter()
        .any(|candidate| candidate.key() == &fact.candidate_key);
    match (is_pending, pending_causes.get(&fact.candidate_key).copied()) {
        (false, None) => Ok(Termination::Voluntary),
        (true, Some(cause)) => Ok(cause.causal_termination()),
        (true, None) => Err(invariant(
            "pending auction plan-chain cancellation has no typed causal provenance",
        )),
        (false, Some(_)) => Err(invariant(
            "auction plan-chain causal provenance belongs to a different operation",
        )),
    }
}

#[cfg(feature = "simulation-diagnostics")]
fn project_continuous_causal_start(
    session: &mut GameSession,
    round: &ContinuousExecutionRound,
    fact: &ContinuousExecutionFact,
    cancel_termination: crate::diagnostics::causal::Termination,
) -> Result<(), StepFatal> {
    let quotes = round.operation_quotes.get(&fact.sealed_index);
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
            let quotes = quotes.ok_or_else(|| {
                invariant("successful continuous place has no operation quote projection")
            })?;
            session.causal_submitted_with_quote(
                *account,
                *order_id,
                code,
                *side,
                *original_qty,
                causal_quote(code, &quotes.before),
            );
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
            account,
            code,
            order_id,
            remaining_qty,
            ..
        }) => {
            if quotes.is_none() {
                return Err(invariant(
                    "successful continuous cancellation has no operation quote projection",
                ));
            }
            session.causal_terminated(
                (*account, *order_id, *remaining_qty),
                code,
                cancel_termination,
            );
        }
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Rejected {
                    account,
                    code,
                    order_id,
                    ..
                },
            original_qty,
        } => {
            let quotes = quotes.ok_or_else(|| {
                invariant("P4-rejected continuous place has no pre-operation quote projection")
            })?;
            let mut rejection_receipts = round.receipts.iter().filter(|receipt| {
                receipt.local_key.source() == ReceiptSource::SealedIntent(fact.sealed_index)
                    && receipt.envelope.account == *account
                    && receipt.envelope.stock == *code
                    && receipt.envelope.order == *order_id
                    && receipt.kind == ReceiptKind::Reject
            });
            let receipt = rejection_receipts.next().ok_or_else(|| {
                invariant("P4-rejected continuous place has no matching reject receipt")
            })?;
            if rejection_receipts.next().is_some()
                || receipt.qty_before != *original_qty
                || receipt.qty_after != *original_qty
            {
                return Err(invariant(
                    "P4-rejected continuous place has ambiguous lifecycle provenance",
                ));
            }
            session.causal_submitted_with_quote(
                *account,
                *order_id,
                code,
                receipt.envelope.side,
                *original_qty,
                causal_quote(code, &quotes.before),
            );
            session.causal_terminated(
                (*account, *order_id, *original_qty),
                code,
                crate::diagnostics::causal::Termination::Aborted,
            );
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { .. }) => {
            if quotes.is_some() {
                return Err(invariant(
                    "rejected continuous operation exposed an operation quote projection",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "simulation-diagnostics")]
fn causal_cancel_termination(
    pending: &[P2Candidate],
    pending_causes: &BTreeMap<P2CandidateKey, PlanCancelCause>,
    fact: &ContinuousExecutionFact,
) -> Result<crate::diagnostics::causal::Termination, StepFatal> {
    use crate::diagnostics::causal::Termination;

    if !matches!(fact.outcome, ContinuousExecutionOutcome::Cancel(_)) {
        return Ok(Termination::Voluntary);
    }
    let is_pending = pending
        .iter()
        .any(|candidate| candidate.key() == fact.candidate_key());
    match (
        is_pending,
        pending_causes.get(fact.candidate_key()).copied(),
    ) {
        (false, None) => Ok(Termination::Voluntary),
        (true, Some(cause)) => Ok(cause.causal_termination()),
        (true, None) => Err(invariant(
            "pending plan-chain cancellation has no typed causal provenance",
        )),
        (false, Some(_)) => Err(invariant(
            "plan-chain causal provenance belongs to a different operation",
        )),
    }
}

#[cfg(feature = "simulation-diagnostics")]
fn project_continuous_causal_end(
    session: &mut GameSession,
    round: &ContinuousExecutionRound,
    fact: &ContinuousExecutionFact,
) -> Result<(), StepFatal> {
    use crate::diagnostics::causal::{CausalFactKind, Termination};

    let (code, side, incoming_order) = match &fact.outcome {
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Resting {
                    code,
                    order_id,
                    side,
                    ..
                }
                | ContinuousPlaceFact::Filled {
                    code,
                    order_id,
                    side,
                    ..
                },
            ..
        } => (code, Some(*side), Some(*order_id)),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled { code, .. }) => {
            (code, None, None)
        }
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Rejected { .. },
            ..
        }
        | ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { .. }) => {
            return Ok(())
        }
    };
    let quotes = round
        .operation_quotes
        .get(&fact.sealed_index)
        .ok_or_else(|| {
            invariant("successful continuous operation lost its operation quote projection")
        })?;
    for trade in round
        .trades
        .iter()
        .filter(|trade| trade.triggering_sealed_index == fact.sealed_index)
    {
        session.causal_record(CausalFactKind::Execution {
            code: trade.stock.clone(),
            maker: trade.trade.maker_order_id,
            taker: trade.trade.taker_order_id,
            side,
            qty: trade.trade.qty,
            price_cents: trade.trade.price.cents(),
            before: causal_quote(code, &quotes.before),
        });
    }
    if let Some(order_id) = incoming_order {
        for receipt in round.receipts.iter().filter(|receipt| {
            receipt.local_key.source() == ReceiptSource::SealedIntent(fact.sealed_index)
                && receipt.envelope.order == order_id
                && receipt.kind == ReceiptKind::Release
        }) {
            if receipt.qty_before == 0 {
                return Err(invariant(
                    "continuous market remainder release has zero quantity",
                ));
            }
            session.causal_terminated(
                (receipt.envelope.account, order_id, receipt.qty_before),
                code,
                Termination::MarketRemainder,
            );
        }
    }
    session.causal_record(CausalFactKind::Quote(causal_quote(code, &quotes.after)));
    Ok(())
}

#[cfg(feature = "simulation-diagnostics")]
fn causal_quote(
    code: &StockCode,
    snapshot: &super::p4_continuous::ContinuousQuoteSnapshot,
) -> crate::diagnostics::causal::Quote {
    crate::diagnostics::causal::Quote {
        code: code.clone(),
        bid_cents: snapshot.bid_cents,
        ask_cents: snapshot.ask_cents,
        bid_depth: snapshot.bid_depth,
        ask_depth: snapshot.ask_depth,
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

fn validate_auction_command_fact(
    candidate: &P2Candidate,
    operation: &P3ValidatedOperation,
    fact: &AuctionExecutionFact,
) -> Result<(), StepFatal> {
    validate_auction_fact_identity(fact)?;
    if fact.candidate_key != *candidate.key() || fact.sealed_index != operation.sealed_index() {
        return Err(invariant(
            "auction P4 fact does not match the yielded candidate/sealed identity",
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
            AuctionLifecycleFact::Accepted {
                account,
                code: actual_code,
                order_id,
                side: actual_side,
                qty: actual_qty,
                ..
            },
        ) => {
            *account == candidate.owner()
                && actual_code == code
                && actual_side == side
                && actual_qty == qty
                && draft.owner() == *account
                && draft.code() == code
                && draft.side() == *side
                && draft.limit() == *price
                && draft.qty() == *qty
                && draft.order_id() == *order_id
                && fact.allocated_order_id == Some(*order_id)
        }
        (
            Intent::PlaceLimit { code, .. },
            P3ValidatedOperation::Place(draft),
            AuctionLifecycleFact::Rejected {
                account,
                code: actual_code,
                order_id,
                ..
            },
        ) => {
            *account == candidate.owner()
                && actual_code == code
                && draft.owner() == *account
                && draft.code() == code
                && *order_id == Some(draft.order_id())
                && fact.allocated_order_id == Some(draft.order_id())
        }
        (
            Intent::Cancel { code, id },
            P3ValidatedOperation::Cancel {
                account,
                code: actual_code,
                order_id,
                ..
            },
            AuctionLifecycleFact::Canceled {
                account: fact_account,
                code: fact_code,
                order_id: fact_order,
                ..
            },
        ) => {
            *account == candidate.owner()
                && fact_account == account
                && actual_code == code
                && fact_code == code
                && order_id == id
                && fact_order == id
                && fact.allocated_order_id.is_none()
        }
        (
            Intent::Cancel { code, id },
            P3ValidatedOperation::Cancel {
                account,
                code: actual_code,
                order_id,
                ..
            },
            AuctionLifecycleFact::Rejected {
                account: fact_account,
                code: fact_code,
                order_id: fact_order,
                ..
            },
        ) => {
            *account == candidate.owner()
                && fact_account == account
                && actual_code == code
                && fact_code == code
                && order_id == id
                && *fact_order == Some(*id)
                && fact.allocated_order_id.is_none()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invariant(
            "auction P4 payload does not match the yielded command",
        ))
    }
}

fn validate_auction_fact_identity(fact: &AuctionExecutionFact) -> Result<(), StepFatal> {
    let (candidate_key, sealed_index, allocated) = match &fact.outcome {
        AuctionLifecycleFact::Accepted {
            candidate_key,
            sealed_index,
            order_id,
            ..
        } => (candidate_key, *sealed_index, Some(*order_id)),
        AuctionLifecycleFact::Canceled {
            candidate_key,
            sealed_index,
            ..
        } => (candidate_key, *sealed_index, None),
        AuctionLifecycleFact::Rejected {
            candidate_key,
            sealed_index,
            ..
        } => (candidate_key, *sealed_index, fact.allocated_order_id),
    };
    if candidate_key != &fact.candidate_key
        || sealed_index != fact.sealed_index
        || allocated != fact.allocated_order_id
    {
        return Err(invariant(
            "auction P4 fact has inconsistent candidate/sealed/order identity",
        ));
    }
    Ok(())
}

fn auction_route_outcome(outcome: &AuctionLifecycleFact) -> PlanRouteOutcome {
    match outcome {
        AuctionLifecycleFact::Accepted { order_id, .. } => PlanRouteOutcome::Accepted(*order_id),
        AuctionLifecycleFact::Canceled { order_id, .. } => PlanRouteOutcome::Canceled(*order_id),
        AuctionLifecycleFact::Rejected { reason, .. } => PlanRouteOutcome::Rejected(reason.clone()),
    }
}

fn rebuild_auction_order_counts(session: &mut GameSession) -> Result<(), StepFatal> {
    let mut counts = std::collections::BTreeMap::<AccountId, usize>::new();
    for order in session.auction_orders.values().flatten() {
        let count = counts.entry(order.owner).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| invariant("auction projection order count overflow"))?;
    }
    session.auction_order_counts = counts;
    Ok(())
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
                ContinuousCancelRejection::AuctionOrderNotCancelable => {
                    RejectionReason::AuctionOrderNotCancelable
                }
            })
        }
    }
}

fn synchronize_projected_plans(session: &mut GameSession) -> Result<(), StepFatal> {
    let mut plans = std::mem::take(&mut session.plans);
    let synchronized = session.synchronize_owned_plan_execution(&mut plans);
    session.plans = plans;
    synchronized.map_err(|error| invariant(&error.to_string()))
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
        if parent.side == key.side
            && parent.active_child_order_id == Some(key.order)
            && (parent
                .active_child_remaining_qty
                .is_none_or(|remaining| qty > remaining)
                || parent
                    .filled_qty
                    .checked_add(qty)
                    .is_none_or(|filled| filled > parent.target_qty))
        {
            return Err(invariant(
                "typed fill exceeds parent child or target quantity",
            ));
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
    if receipt.qty_after == 0 {
        session.remove_npc_order_lifecycle(key.account, &key.stock, key.order);
    }
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
