use super::p3_validation::P3ValidationState;
use super::{
    DecisionResourceSnapshot, EnvelopeLedger, P2Candidate, P2CandidateKey, P3CandidateResult,
    P3ValidatedOperation, P3ValidationContext, P3ValidationOutput, StepFatal,
};
use crate::GameConfig;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P3DriverCheckpoint {
    sealed_count: u64,
    next_order_id_after: u64,
    operation_count: usize,
    draft_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct P3ConsumeOutcome {
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
        ledger.validate_conservation()?;
        Ok(Self {
            last_candidate_key: None,
            state: P3ValidationState::new(resources, tick_start_next_order_id, config, context),
        })
    }

    /// Validates one candidate against the same immutable P1 resource snapshot and tick-start
    /// counters as every earlier candidate. `P3ValidationState::consume` mutates its budgets,
    /// counters and cumulative output only after all fallible checks for this candidate succeed;
    /// therefore any `StepFatal` leaves `checkpoint()` and `output()` unchanged at the previous
    /// successful boundary.
    pub fn consume(&mut self, candidate: P2Candidate) -> Result<P3ConsumeOutcome, StepFatal> {
        if self
            .last_candidate_key
            .as_ref()
            .is_some_and(|previous| previous >= candidate.key())
        {
            return Err(invariant(
                "non-canonical P3 driver candidate: keys must be strictly increasing",
            ));
        }
        let candidate_key = candidate.key().clone();
        let step = self.state.consume(&candidate)?;
        let outcome = P3ConsumeOutcome {
            result: step.result,
            operation: step.operation,
            next_order_id_after: self.state.output().next_order_id_after(),
        };
        self.last_candidate_key = Some(candidate_key);
        Ok(outcome)
    }

    pub fn checkpoint(&self) -> P3DriverCheckpoint {
        P3DriverCheckpoint {
            sealed_count: self.state.sealed_count(),
            next_order_id_after: self.state.output().next_order_id_after(),
            operation_count: self.state.output().operations().len(),
            draft_count: self.state.output().drafts().len(),
        }
    }

    pub const fn output(&self) -> &P3ValidationOutput {
        self.state.output()
    }
}

impl P3ConsumeOutcome {
    pub const fn result(&self) -> &P3CandidateResult {
        &self.result
    }

    pub const fn operation(&self) -> Option<&P3ValidatedOperation> {
        self.operation.as_ref()
    }

    pub const fn sealed_index(&self) -> u64 {
        self.result.sealed_index()
    }

    pub const fn next_order_id_after(&self) -> u64 {
        self.next_order_id_after
    }
}

impl P3DriverCheckpoint {
    pub const fn sealed_count(&self) -> u64 {
        self.sealed_count
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
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_driver".to_owned(),
    }
}
