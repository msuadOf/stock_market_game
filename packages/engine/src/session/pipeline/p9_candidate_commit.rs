//! Prepared single-point P9 commit for a fully computed tick candidate.

#[cfg(test)]
use crate::session::StateHash;
use crate::GameSession;

use super::{
    validate_receipt_keys, StepFatal, TickCommitEvidence, TickCommitResult, TickShadowPlan,
};

pub(super) struct PreparedP9CandidateCommit<'authority> {
    authority: &'authority mut GameSession,
    candidate: GameSession,
    receipt: P9CommitReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P9CommitReceipt {
    #[cfg(test)]
    business: StateHash,
    #[cfg(test)]
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
) -> Result<PreparedP9CandidateCommit<'authority>, StepFatal> {
    authority.require_healthy()?;
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
    #[cfg(test)]
    let business = candidate.business_state_hash()?;
    #[cfg(test)]
    let session = candidate.session_state_hash()?;
    let receipt = P9CommitReceipt {
        #[cfg(test)]
        business,
        #[cfg(test)]
        session,
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
    evidence: TickCommitEvidence,
}

pub(super) struct CandidateTickCommitResult {
    pub(super) tick: TickCommitResult,
    #[cfg(test)]
    pub(super) receipt: P9CommitReceipt,
    pub(crate) evidence: TickCommitEvidence,
}

/// Finalizes an isolated tick plan after every fallible check.
pub(super) fn prepare_tick_shadow_plan_commit<'authority>(
    authority: &'authority mut GameSession,
    plan: TickShadowPlan,
) -> Result<PreparedTickPlanCommit<'authority>, StepFatal> {
    validate_receipt_keys(&plan.receipt_keys)?;
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
    let prepared = prepare_p9_candidate_commit(authority, candidate)?;
    Ok(PreparedTickPlanCommit {
        prepared,
        events,
        evidence,
    })
}

impl PreparedTickPlanCommit<'_> {
    #[cfg(test)]
    pub(crate) fn evidence(&self) -> &TickCommitEvidence {
        &self.evidence
    }

    pub(super) fn commit(self) -> CandidateTickCommitResult {
        let _receipt = self.prepared.commit();
        CandidateTickCommitResult {
            tick: TickCommitResult {
                events: self.events,
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
