//! A one-command-at-a-time adapter between plan continuations and P2 candidates.

use super::plan_execution::{PlanExecutionProgress, PlanExecutionRoute, PlanRouteOutcome};
use super::*;
use crate::plans::PlanBook;

/// Explicit failures from the one-at-a-time plan-chain adapter.
#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum PlanChainYieldDriverError {
    #[error(
        "the yielded plan-chain command needs an outcome before another command can be yielded"
    )]
    OutcomeRequired,
    #[error("the plan-chain continuation completed; take its completion before yielding again")]
    CompletionAvailable,
    #[error("the plan-chain completion was already taken")]
    CompletionAlreadyTaken,
    #[error("the plan-chain driver has no route awaiting an outcome")]
    NoPendingRoute,
    #[error("the plan-chain driver stopped after a typed continuation failure")]
    Failed,
    #[error("could not capture the plan-chain continuation shadow: {0}")]
    Shadow(#[from] StepFatal),
    #[error(transparent)]
    Execution(#[from] PlanExecutionError),
}

/// Holds exactly one continuation route until its typed P3/P4 outcome arrives.
///
/// It intentionally does not call the order router. A continuation is resumed on a shadow
/// session and shadow plan book, then committed only after all local checks succeed.
#[derive(Clone)]
pub(in crate::session) struct PlanChainYieldDriver {
    candidate_source: PlanChainCandidateSource,
    state: DriverState,
}

/// The non-authoritative continuation state owned by P2.
///
/// Capturing it copies the session and plan book. The driver may only mutate this copy; P9 owns
/// any later reconciliation into authority.
pub(in crate::session) struct PlanChainContinuationShadow {
    session: GameSession,
    plans: PlanBook,
}

impl PlanChainContinuationShadow {
    pub(in crate::session) fn capture(
        session: &GameSession,
        plans: &PlanBook,
    ) -> Result<Self, PlanChainYieldDriverError> {
        Ok(Self {
            session: session.clone_for_tick_shadow()?,
            plans: plans.clone(),
        })
    }

    fn fork(&self) -> Result<Self, PlanChainYieldDriverError> {
        Ok(Self {
            session: self.session.clone_for_tick_shadow()?,
            plans: self.plans.clone(),
        })
    }

    pub(in crate::session) fn into_parts(self) -> (GameSession, PlanBook) {
        (self.session, self.plans)
    }

    #[cfg(test)]
    pub(in crate::session) fn serialized_state_for_test(&self) -> serde_json::Value {
        serde_json::to_value((&self.session.save().unwrap(), &self.plans))
            .expect("continuation shadow must remain serializable")
    }
}

#[derive(Clone)]
enum DriverState {
    Ready(Box<PlanExecutionRoute>),
    AwaitingOutcome(Box<PlanExecutionRoute>),
    Complete(Option<PlanExecutionReport>),
    Failed,
}

impl PlanChainYieldDriver {
    pub(in crate::session) fn fork(&self) -> Self {
        self.clone()
    }

    pub(in crate::session) fn new(progress: PlanExecutionProgress) -> Self {
        let state = match progress {
            PlanExecutionProgress::Route(route) => DriverState::Ready(route),
            PlanExecutionProgress::Complete(report) => DriverState::Complete(Some(report)),
        };
        Self {
            candidate_source: PlanChainCandidateSource::default(),
            state,
        }
    }

    /// Produces the sole command currently available to P2.
    pub(in crate::session) fn yield_next(
        &mut self,
    ) -> Result<PlanChainCandidateBatch, PlanChainYieldDriverError> {
        let candidate = match &self.state {
            DriverState::Ready(route) => self.candidate_source.enumerate(route.command())?,
            DriverState::AwaitingOutcome(_) => {
                return Err(PlanChainYieldDriverError::OutcomeRequired)
            }
            DriverState::Complete(Some(_)) => {
                return Err(PlanChainYieldDriverError::CompletionAvailable)
            }
            DriverState::Complete(None) => {
                return Err(PlanChainYieldDriverError::CompletionAlreadyTaken)
            }
            DriverState::Failed => return Err(PlanChainYieldDriverError::Failed),
        };
        let state = std::mem::replace(&mut self.state, DriverState::Failed);
        let DriverState::Ready(route) = state else {
            unreachable!("candidate was only enumerated from a ready plan-chain route");
        };
        self.state = DriverState::AwaitingOutcome(route);
        Ok(candidate)
    }

    /// Resumes the previously yielded route with its authoritative typed outcome.
    ///
    /// No intent is routed here. A typed failure discards the local shadow rather than exposing
    /// a partial parent-plan or plan-book update.
    pub(in crate::session) fn resume(
        &mut self,
        shadow: &mut PlanChainContinuationShadow,
        outcome: PlanRouteOutcome,
    ) -> Result<(), PlanChainYieldDriverError> {
        if !matches!(self.state, DriverState::AwaitingOutcome(_)) {
            return match self.state {
                DriverState::Ready(_) => Err(PlanChainYieldDriverError::NoPendingRoute),
                DriverState::Complete(Some(_)) => {
                    Err(PlanChainYieldDriverError::CompletionAvailable)
                }
                DriverState::Complete(None) => {
                    Err(PlanChainYieldDriverError::CompletionAlreadyTaken)
                }
                DriverState::Failed => Err(PlanChainYieldDriverError::Failed),
                DriverState::AwaitingOutcome(_) => unreachable!(),
            };
        }

        let mut candidate_shadow = shadow.fork()?;
        let state = std::mem::replace(&mut self.state, DriverState::Failed);
        let DriverState::AwaitingOutcome(route) = state else {
            unreachable!("the checked driver state must hold a pending route");
        };

        let progress = route.resume(
            &mut candidate_shadow.session,
            &mut candidate_shadow.plans,
            outcome,
        )?;
        if matches!(progress, PlanExecutionProgress::Route(_)) {
            self.candidate_source.ensure_next_generation_index()?;
        }

        *shadow = candidate_shadow;
        self.state = match progress {
            PlanExecutionProgress::Route(route) => DriverState::Ready(route),
            PlanExecutionProgress::Complete(report) => DriverState::Complete(Some(report)),
        };
        Ok(())
    }

    /// Takes the terminal continuation result exactly once.
    pub(in crate::session) fn take_completion(
        &mut self,
    ) -> Result<PlanExecutionReport, PlanChainYieldDriverError> {
        let state = std::mem::replace(&mut self.state, DriverState::Failed);
        match state {
            DriverState::Complete(Some(report)) => {
                self.state = DriverState::Complete(None);
                Ok(report)
            }
            DriverState::Complete(None) => {
                self.state = DriverState::Complete(None);
                Err(PlanChainYieldDriverError::CompletionAlreadyTaken)
            }
            other => {
                self.state = other;
                Err(match self.state {
                    DriverState::Ready(_) | DriverState::AwaitingOutcome(_) => {
                        PlanChainYieldDriverError::NoPendingRoute
                    }
                    DriverState::Failed => PlanChainYieldDriverError::Failed,
                    DriverState::Complete(_) => unreachable!(),
                })
            }
        }
    }

    #[cfg(test)]
    pub(in crate::session) fn set_next_generation_index_for_test(&mut self, index: u64) {
        self.candidate_source.next_generation_index = index;
    }
}
