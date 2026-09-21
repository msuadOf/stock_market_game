use super::p3_validation::P3ValidationState;
use super::{
    DecisionResourceSnapshot, EnvelopeLedger, P2Candidate, P2CandidateKey, P3CandidateResult,
    P3ValidatedOperation, P3ValidationContext, P3ValidationOutput, StepFatal,
};
use crate::{AccountId, GameConfig, Money, OrderId, StockCode};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P3DriverCheckpoint {
    sealed_count: u64,
    next_sealed_index: u64,
    next_order_id_after: u64,
    operation_count: usize,
    draft_count: usize,
    feedback_count: usize,
    remaining_cash: BTreeMap<AccountId, Money>,
    remaining_sellable: BTreeMap<(AccountId, StockCode), u32>,
    global_open_orders: usize,
    account_open_orders: BTreeMap<AccountId, usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct P3ConsumeOutcome {
    candidate_key: P2CandidateKey,
    sealed_index: u64,
    allocated_order_id: Option<OrderId>,
    result: P3CandidateResult,
    operation: Option<P3ValidatedOperation>,
    next_order_id_after: u64,
}

#[derive(Clone, Debug)]
pub struct P3ValidatorDriver {
    last_candidate_key: Option<P2CandidateKey>,
    state: P3ValidationState,
}

impl P3ValidatorDriver {
    pub fn new(
        resources: DecisionResourceSnapshot,
        ledger: EnvelopeLedger,
        tick_start_next_order_id: u64,
        config: GameConfig,
        context: P3ValidationContext,
    ) -> Result<Self, StepFatal> {
        Self::new_with_cursors(
            resources,
            ledger,
            tick_start_next_order_id,
            0,
            config,
            context,
        )
    }

    pub fn new_with_cursors(
        resources: DecisionResourceSnapshot,
        ledger: EnvelopeLedger,
        tick_start_next_order_id: u64,
        tick_start_next_sealed_index: u64,
        config: GameConfig,
        context: P3ValidationContext,
    ) -> Result<Self, StepFatal> {
        ledger.validate_conservation()?;
        Ok(Self {
            last_candidate_key: None,
            state: P3ValidationState::new(
                resources,
                tick_start_next_order_id,
                tick_start_next_sealed_index,
                config,
                context,
            )?,
        })
    }

    /// Validates one candidate against the same immutable P1 resource snapshot and tick-start
    /// counters as every earlier candidate. It is the one-element form of `consume_round`, so a
    /// `StepFatal` leaves `checkpoint()` and `output()` unchanged at the previous boundary.
    pub fn consume(&mut self, candidate: P2Candidate) -> Result<P3ConsumeOutcome, StepFatal> {
        let mut outcomes = self.consume_round([candidate])?;
        outcomes
            .pop()
            .ok_or_else(|| invariant("single-candidate P3 round produced no outcome"))
    }

    /// Consumes one ready round atomically. Account validation/reservation is completed for the
    /// whole canonical round before OrderIds are assigned to accepted Place operations.
    pub fn consume_round(
        &mut self,
        candidates: impl IntoIterator<Item = P2Candidate>,
    ) -> Result<Vec<P3ConsumeOutcome>, StepFatal> {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        validate_candidate_order(self.last_candidate_key.as_ref(), &candidates)?;

        let mut next_state = self.state.clone();
        let mut next_order_id_after = next_state.output().next_order_id_after();
        let steps = next_state.consume_round(&candidates)?;
        let mut outcomes = Vec::with_capacity(steps.len());
        for step in steps {
            let candidate_key = step.candidate_key().clone();
            let sealed_index = step.sealed_index();
            let allocated_order_id = step.allocated_order_id();
            if allocated_order_id.is_some() {
                next_order_id_after = next_order_id_after
                    .checked_add(1)
                    .ok_or_else(|| invariant("P3 outcome OrderId cursor overflow"))?;
            }
            outcomes.push(P3ConsumeOutcome {
                candidate_key,
                sealed_index,
                allocated_order_id,
                result: step.result,
                operation: step.operation,
                next_order_id_after,
            });
        }
        if next_order_id_after != next_state.output().next_order_id_after() {
            return Err(invariant(
                "P3 outcome OrderId cursor disagrees with cumulative validation output",
            ));
        }

        if let Some(last) = candidates.last() {
            self.last_candidate_key = Some(last.key().clone());
        }
        self.state = next_state;
        Ok(outcomes)
    }

    /// Applies the actual P4 change in open-order slots for one accepted operation. The deltas
    /// may include terminal resting makers. This never replenishes cash or sellable-share budget.
    pub fn apply_open_order_feedback(
        &mut self,
        candidate_key: &P2CandidateKey,
        sealed_index: u64,
        actual_deltas: impl IntoIterator<Item = (AccountId, i64)>,
    ) -> Result<(), StepFatal> {
        let mut next_state = self.state.clone();
        next_state.apply_open_order_feedback(candidate_key, sealed_index, actual_deltas)?;
        self.state = next_state;
        Ok(())
    }

    /// Installs all stock-worker slot changes atomically; a malformed later fact must not
    /// leave feedback from an earlier operation applied to the next continuation round.
    pub(super) fn apply_open_order_feedback_round(
        &mut self,
        feedback: impl IntoIterator<Item = (P2CandidateKey, u64, BTreeMap<AccountId, i64>)>,
    ) -> Result<(), StepFatal> {
        let mut next_state = self.state.clone();
        for (candidate_key, sealed_index, deltas) in feedback {
            next_state.apply_open_order_feedback(&candidate_key, sealed_index, deltas)?;
        }
        self.state = next_state;
        Ok(())
    }

    pub fn checkpoint(&self) -> P3DriverCheckpoint {
        P3DriverCheckpoint {
            sealed_count: self.state.sealed_count(),
            next_sealed_index: self.state.next_sealed_index(),
            next_order_id_after: self.state.output().next_order_id_after(),
            operation_count: self.state.output().operations().len(),
            draft_count: self.state.output().drafts().len(),
            feedback_count: self.state.feedback_count(),
            remaining_cash: self.state.remaining_cash(),
            remaining_sellable: self.state.remaining_sellable(),
            global_open_orders: self.state.global_open_orders(),
            account_open_orders: self.state.account_open_orders(),
        }
    }

    pub(super) fn ready_round_len(&self, candidates: &[P2Candidate]) -> Result<usize, StepFatal> {
        self.state.ready_round_len(candidates)
    }

    #[cfg(test)]
    pub(super) fn last_round_account_shards(&self) -> usize {
        self.state.last_round_account_shards
    }

    pub const fn output(&self) -> &P3ValidationOutput {
        self.state.output()
    }

    pub fn finish(self) -> P3ValidationOutput {
        self.state.into_output()
    }
}

impl P3ConsumeOutcome {
    pub const fn candidate_key(&self) -> &P2CandidateKey {
        &self.candidate_key
    }

    pub const fn result(&self) -> &P3CandidateResult {
        &self.result
    }

    pub const fn operation(&self) -> Option<&P3ValidatedOperation> {
        self.operation.as_ref()
    }

    pub const fn sealed_index(&self) -> u64 {
        self.sealed_index
    }

    pub const fn allocated_order_id(&self) -> Option<OrderId> {
        self.allocated_order_id
    }

    pub const fn next_order_id_after(&self) -> u64 {
        self.next_order_id_after
    }

    pub fn into_operation(self) -> Option<P3ValidatedOperation> {
        self.operation
    }
}

impl P3DriverCheckpoint {
    pub const fn sealed_count(&self) -> u64 {
        self.sealed_count
    }

    pub const fn next_sealed_index(&self) -> u64 {
        self.next_sealed_index
    }

    pub const fn next_order_id_after(&self) -> u64 {
        self.next_order_id_after
    }

    pub const fn operation_count(&self) -> usize {
        self.operation_count
    }

    pub const fn draft_count(&self) -> usize {
        self.draft_count
    }

    pub const fn feedback_count(&self) -> usize {
        self.feedback_count
    }

    pub fn remaining_cash(&self, account: AccountId) -> Option<Money> {
        self.remaining_cash.get(&account).copied()
    }

    pub fn remaining_sellable(&self, account: AccountId, code: &StockCode) -> Option<u32> {
        self.remaining_sellable
            .get(&(account, code.clone()))
            .copied()
    }

    pub const fn global_open_orders(&self) -> usize {
        self.global_open_orders
    }

    pub fn account_open_orders(&self, account: AccountId) -> Option<usize> {
        self.account_open_orders.get(&account).copied()
    }
}

fn validate_candidate_order(
    previous: Option<&P2CandidateKey>,
    candidates: &[P2Candidate],
) -> Result<(), StepFatal> {
    if let (Some(previous), Some(first)) = (previous, candidates.first()) {
        if previous >= first.key() {
            return Err(invariant(
                "non-canonical P3 driver candidate: keys must be strictly increasing",
            ));
        }
    }
    if candidates
        .windows(2)
        .any(|pair| pair[0].key() >= pair[1].key())
    {
        return Err(invariant(
            "non-canonical P3 driver round: keys must be strictly increasing",
        ));
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_driver".to_owned(),
    }
}
