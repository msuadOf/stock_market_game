use super::p3_validation::P3ValidationState;
use super::{
    DecisionResourceSnapshot, EnvelopeLedger, P2Candidate, P2CandidateKey, P3CandidateResult,
    P3ValidatedOperation, P3ValidationContext, P3ValidationOutput, StepFatal,
};
use crate::{AccountId, GameConfig, Money, OrderId, StockCode};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P3DriverCheckpoint {
    resources: Arc<DecisionResourceSnapshot>,
    sealed_count: u64,
    next_sealed_index: u64,
    next_order_id_after: u64,
    operation_count: usize,
    draft_count: usize,
    remaining_cash: BTreeMap<AccountId, Money>,
    remaining_sellable: BTreeMap<(AccountId, StockCode), u32>,
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
    seen_candidate_keys: BTreeSet<P2CandidateKey>,
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
            seen_candidate_keys: BTreeSet::new(),
            state: P3ValidationState::new(
                resources,
                tick_start_next_order_id,
                tick_start_next_sealed_index,
                config,
                context,
            ),
        })
    }

    /// Validates one candidate against the same immutable P1 resource snapshot and private
    /// account budget as every earlier candidate. It is the one-element form of `consume_round`, so a
    /// `StepFatal` leaves `checkpoint()` and `output()` unchanged at the previous boundary.
    pub fn consume(&mut self, candidate: P2Candidate) -> Result<P3ConsumeOutcome, StepFatal> {
        let mut outcomes = self.consume_round([candidate])?;
        outcomes
            .pop()
            .ok_or_else(|| invariant("single-candidate P3 round produced no outcome"))
    }

    /// Consumes one ready round atomically. Account validation/reservation is completed for the
    /// whole ready round before OrderIds are assigned to accepted Place operations.
    pub fn consume_round(
        &mut self,
        candidates: impl IntoIterator<Item = P2Candidate>,
    ) -> Result<Vec<P3ConsumeOutcome>, StepFatal> {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        validate_candidate_identities(&self.seen_candidate_keys, &candidates)?;

        let (round, steps) = self.state.prepare_round(&candidates)?;
        let mut next_order_id_after = self.state.output().next_order_id_after();
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
        if next_order_id_after != round.output().next_order_id_after() {
            return Err(invariant(
                "P3 outcome OrderId cursor disagrees with cumulative validation output",
            ));
        }

        self.seen_candidate_keys
            .extend(candidates.iter().map(|candidate| candidate.key().clone()));
        self.state.commit_round(round);
        Ok(outcomes)
    }

    pub fn checkpoint(&self) -> P3DriverCheckpoint {
        P3DriverCheckpoint {
            resources: self.state.resources(),
            sealed_count: self.state.sealed_count(),
            next_sealed_index: self.state.next_sealed_index(),
            next_order_id_after: self.state.output().next_order_id_after(),
            operation_count: self.state.output().operations().len(),
            draft_count: self.state.output().drafts().len(),
            remaining_cash: self.state.remaining_cash(),
            remaining_sellable: self.state.remaining_sellable(),
        }
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

    pub fn remaining_cash(&self, account: AccountId) -> Option<Money> {
        self.remaining_cash
            .get(&account)
            .copied()
            .or_else(|| self.resources.available_cash(account).ok())
    }

    pub fn remaining_sellable(&self, account: AccountId, code: &StockCode) -> Option<u32> {
        self.remaining_sellable
            .get(&(account, code.clone()))
            .copied()
    }
}

fn validate_candidate_identities(
    previous: &BTreeSet<P2CandidateKey>,
    candidates: &[P2Candidate],
) -> Result<(), StepFatal> {
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        if previous.contains(candidate.key()) || !seen.insert(candidate.key()) {
            return Err(invariant("P3 driver candidate identity was replayed"));
        }
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_driver".to_owned(),
    }
}
