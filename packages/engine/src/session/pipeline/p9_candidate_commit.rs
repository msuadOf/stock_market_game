//! Prepared single-point P9 commit for a fully computed tick candidate.

#[cfg(test)]
use crate::session::hash::RollbackHashes;
use crate::{session::StateHash, GameSession};

use super::{
    validate_receipt_keys, PhaseOutput, StepFatal, TickCommitEvidence, TickCommitResult, TickPhase,
    TickShadowPlan,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P8AuthorityGuard {
    business: StateHash,
    session: StateHash,
}

impl P8AuthorityGuard {
    #[cfg(test)]
    pub(super) const fn from_rollback_hashes(hashes: RollbackHashes) -> Self {
        Self {
            business: hashes.business,
            session: hashes.session,
        }
    }

    pub(super) fn capture(authority: &GameSession) -> Result<Self, StepFatal> {
        authority.require_healthy()?;
        let hashes = authority.rollback_hashes()?;
        Ok(Self {
            business: hashes.business,
            session: hashes.session,
        })
    }

    fn validate(self, authority: &GameSession) -> Result<(), StepFatal> {
        authority.require_healthy()?;
        let hashes = authority.rollback_hashes()?;
        if hashes.business != self.business {
            return Err(StepFatal::Internal {
                expected: self.business,
                observed: hashes.business,
            });
        }
        if hashes.session != self.session {
            return Err(StepFatal::Internal {
                expected: self.session,
                observed: hashes.session,
            });
        }
        Ok(())
    }
}

pub(super) struct PreparedP9CandidateCommit<'authority> {
    authority: &'authority mut GameSession,
    candidate: GameSession,
    receipt: P9CommitReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P9CommitReceipt {
    business: StateHash,
    session: StateHash,
    next_receipt_base: u64,
}

impl P9CommitReceipt {
    #[cfg(test)]
    pub(super) const fn business_hash(self) -> StateHash {
        self.business
    }

    #[cfg(test)]
    pub(super) const fn session_hash(self) -> StateHash {
        self.session
    }

    #[cfg(test)]
    pub(super) const fn next_receipt_base(self) -> u64 {
        self.next_receipt_base
    }
}

/// Completes every fallible P8/P9 precondition and returns an infallible commit token.
pub(super) fn prepare_p9_candidate_commit<'authority>(
    authority: &'authority mut GameSession,
    mut candidate: GameSession,
    guard: P8AuthorityGuard,
) -> Result<PreparedP9CandidateCommit<'authority>, StepFatal> {
    guard.validate(authority)?;
    candidate.require_healthy()?;
    validate_receipt_cursor(&candidate)?;
    let receipt_cursor = candidate.next_receipt_base;

    candidate.envelope_ledger.rebase_live_for_next_tick()?;

    validate_receipt_cursor(&candidate)?;
    if candidate.next_receipt_base != receipt_cursor {
        return Err(invariant(
            "live-ledger rebase changed the global receipt cursor".to_owned(),
        ));
    }
    let hashes = candidate.rollback_hashes()?;
    let receipt = P9CommitReceipt {
        business: hashes.business,
        session: hashes.session,
        next_receipt_base: receipt_cursor,
    };
    Ok(PreparedP9CandidateCommit {
        authority,
        candidate,
        receipt,
    })
}

impl PreparedP9CandidateCommit<'_> {
    pub(super) fn commit(self) -> P9CommitReceipt {
        self.authority.commit_tick_shadow(self.candidate);
        self.receipt
    }
}

pub(super) struct PreparedTickPlanCommit<'authority> {
    prepared: PreparedP9CandidateCommit<'authority>,
    events: Vec<crate::Event>,
    trace: Vec<TickPhase>,
    evidence: TickCommitEvidence,
}

pub(super) struct CandidateTickCommitResult {
    pub(super) tick: TickCommitResult,
    #[cfg(test)]
    pub(super) receipt: P9CommitReceipt,
    pub(crate) evidence: TickCommitEvidence,
}

/// Finalizes an isolated new-path plan without invoking the compatibility bridge.
pub(super) fn prepare_tick_shadow_plan_commit<'authority>(
    authority: &'authority mut GameSession,
    mut plan: TickShadowPlan,
    guard: P8AuthorityGuard,
) -> Result<PreparedTickPlanCommit<'authority>, StepFatal> {
    validate_receipt_keys(&plan.receipt_keys)?;
    plan.tokens.push(PhaseOutput {
        phase: TickPhase::CommitTick,
    });
    let trace = plan.trace();
    let events = plan.event_outbox;
    let candidate = plan.state.into_session()?;
    let evidence = TickCommitEvidence::capture(
        &candidate.envelope_ledger,
        &plan.applied_receipts,
        &plan.receipt_keys,
        plan.b2_finalizers,
    )?;
    #[cfg(test)]
    authority.run_post_shadow_hook()?;
    let prepared = prepare_p9_candidate_commit(authority, candidate, guard)?;
    Ok(PreparedTickPlanCommit {
        prepared,
        events,
        trace,
        evidence,
    })
}

impl PreparedTickPlanCommit<'_> {
    #[cfg(test)]
    pub(crate) const fn evidence(&self) -> &TickCommitEvidence {
        &self.evidence
    }

    pub(super) fn commit(self) -> CandidateTickCommitResult {
        #[cfg(test)]
        super::COMMIT_TRACES.with_borrow_mut(|traces| traces.push(self.trace.clone()));
        let _receipt = self.prepared.commit();
        CandidateTickCommitResult {
            tick: TickCommitResult {
                events: self.events,
                trace: self.trace,
            },
            #[cfg(test)]
            receipt: _receipt,
            evidence: self.evidence,
        }
    }
}

fn validate_receipt_cursor(candidate: &GameSession) -> Result<(), StepFatal> {
    let ledger_cursor = candidate.envelope_ledger.next_receipt_index();
    if candidate.next_receipt_base != ledger_cursor {
        return Err(invariant(format!(
            "candidate receipt cursor {} does not match envelope ledger cursor {ledger_cursor}",
            candidate.next_receipt_base
        )));
    }
    Ok(())
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::p9_candidate_commit".to_owned(),
    }
}
