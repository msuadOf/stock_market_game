//! Prepared single-point P9 commit for a fully computed tick candidate.

#[cfg(test)]
use crate::session::StateHash;
use crate::GameSession;
use std::collections::BTreeSet;

use super::{
    validate_receipt_keys, EnvelopeLedger, EnvelopeReceipt, JournalRank, ReceiptLocalKey,
    ReceiptSource, StepFatal, TickCommitEvidence, TickCommitResult, TickShadowPlan,
};

pub(super) struct PreparedCandidateCommit<'authority> {
    authority: &'authority mut GameSession,
    candidate: GameSession,
    receipt: TickCommitReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TickCommitReceipt {
    #[cfg(test)]
    business: StateHash,
    #[cfg(test)]
    session: StateHash,
    next_receipt_base: u64,
}

impl TickCommitReceipt {
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
pub(super) fn prepare_candidate_commit<'authority>(
    authority: &'authority mut GameSession,
    mut candidate: GameSession,
) -> Result<PreparedCandidateCommit<'authority>, StepFatal> {
    authority.require_healthy()?;
    candidate.require_healthy()?;
    if candidate.tick
        == authority
            .tick
            .checked_add(1)
            .ok_or_else(|| invariant("market tick overflow".to_owned()))?
    {
        super::npc_tick_preparation::queue_npc_for_next_tick(&mut candidate)?;
    } else if candidate.tick != authority.tick {
        return Err(invariant(
            "P9 candidate advanced more than one market tick".to_owned(),
        ));
    }
    validate_receipt_cursor(&candidate)?;
    let receipt_cursor = candidate.next_receipt_base;

    candidate.envelope_ledger.rebase_private_for_tick_commit()?;
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
    let receipt = TickCommitReceipt {
        #[cfg(test)]
        business,
        #[cfg(test)]
        session,
        next_receipt_base: receipt_cursor,
    };
    Ok(PreparedCandidateCommit {
        authority,
        candidate,
        receipt,
    })
}

impl PreparedCandidateCommit<'_> {
    pub(super) fn commit(self) -> TickCommitReceipt {
        self.authority.commit_tick_shadow(self.candidate);
        self.receipt
    }
}

pub(super) struct PreparedTickPlanCommit<'authority> {
    prepared: PreparedCandidateCommit<'authority>,
    events: Vec<crate::Event>,
    event_keys: Vec<super::EventStableKey>,
    evidence: Option<TickCommitEvidence>,
}

pub(super) struct CandidateTickCommitResult {
    pub(super) tick: TickCommitResult,
    #[cfg(test)]
    pub(super) receipt: TickCommitReceipt,
    pub(crate) evidence: Option<TickCommitEvidence>,
}

/// Finalizes an isolated tick plan after every fallible check.
#[cfg(test)]
pub(super) fn prepare_tick_shadow_plan_commit<'authority>(
    authority: &'authority mut GameSession,
    plan: TickShadowPlan,
) -> Result<PreparedTickPlanCommit<'authority>, StepFatal> {
    prepare_tick_shadow_plan_commit_with_evidence(authority, plan, true)
}

pub(super) fn prepare_tick_shadow_plan_commit_with_evidence<'authority>(
    authority: &'authority mut GameSession,
    plan: TickShadowPlan,
    capture_commit_evidence: bool,
) -> Result<PreparedTickPlanCommit<'authority>, StepFatal> {
    validate_receipt_keys(&plan.receipt_keys)?;
    let events = plan.event_outbox;
    let event_keys = plan.event_keys;
    validate_event_keys(&events, &event_keys, plan.expiry.releases.len())?;
    let candidate = plan.state.into_session()?;
    validate_applied_receipt_journal(
        &candidate.envelope_ledger,
        &plan.applied_receipts,
        &plan.receipt_keys,
    )?;
    let evidence = if capture_commit_evidence {
        Some(TickCommitEvidence::capture(
            &candidate.envelope_ledger,
            &plan.applied_receipts,
            &plan.receipt_keys,
            plan.auction_finalizers,
        )?)
    } else {
        None
    };
    #[cfg(test)]
    authority.run_post_shadow_hook()?;
    let prepared = prepare_candidate_commit(authority, candidate)?;
    Ok(PreparedTickPlanCommit {
        prepared,
        events,
        event_keys,
        evidence,
    })
}

fn validate_event_keys(
    events: &[crate::Event],
    keys: &[super::EventStableKey],
    expiry_count: usize,
) -> Result<(), StepFatal> {
    if events.len() != keys.len() || expiry_count > events.len() {
        return Err(invariant(
            "P0 and P7 event identity counts do not match the P9 outbox".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    for (index, (event, key)) in events.iter().zip(keys).enumerate() {
        let is_quote_expiry = index < expiry_count;
        let has_quote_expiry_index =
            key.local_event_index() >= super::event_key::QUOTE_EXPIRY_EVENT_INDEX_BASE;
        let valid_phase_index = is_quote_expiry == has_quote_expiry_index;
        let valid_quote_expiry_event =
            !is_quote_expiry || matches!(event, crate::Event::OrderCanceled { .. });
        if !valid_phase_index
            || !valid_quote_expiry_event
            || key.local_event_index() > crate::orderbook::js_safe_u64::MAX
            || super::EventStableKey::for_event(event, key.local_event_index()) != *key
            || !seen.insert(key.clone())
        {
            return Err(invariant(
                "P0/P7 event identity is invalid or duplicated".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_applied_receipt_journal(
    ledger: &EnvelopeLedger,
    receipts: &[EnvelopeReceipt],
    keys: &[ReceiptLocalKey],
) -> Result<(), StepFatal> {
    if receipts.len() != keys.len() || ledger.seen_local_keys.len() != keys.len() {
        return Err(invariant(
            "applied receipt values, journal keys, and ledger identities have different lengths"
                .to_owned(),
        ));
    }
    let count = u64::try_from(receipts.len())
        .map_err(|_| invariant("applied receipt count exceeds index domain".to_owned()))?;
    let first = ledger
        .next_receipt_index()
        .checked_sub(count)
        .ok_or_else(|| invariant("applied receipt count exceeds ledger cursor".to_owned()))?;
    let mut reached_sealed = false;
    for (offset, (receipt, key)) in receipts.iter().zip(keys).enumerate() {
        let expected_index = first
            .checked_add(u64::try_from(offset).expect("offset fits receipt count"))
            .ok_or_else(|| invariant("applied receipt index overflow".to_owned()))?;
        if receipt.index != expected_index
            || receipt.local_key != *key
            || receipt.envelope != *key.transition_envelope()
            || !ledger.seen_local_keys.contains(key)
        {
            return Err(invariant(
                "applied receipt journal disagrees with ledger identity or cursor".to_owned(),
            ));
        }
        match key.journal() {
            JournalRank::PreSeal => {
                if reached_sealed || !matches!(key.source(), ReceiptSource::QuoteExpiry(_)) {
                    return Err(invariant(
                        "P0 receipts are not the pre-seal journal prefix".to_owned(),
                    ));
                }
            }
            JournalRank::SealedBatch => reached_sealed = true,
        }
    }
    Ok(())
}

impl PreparedTickPlanCommit<'_> {
    #[cfg(test)]
    pub(crate) fn evidence(&self) -> &TickCommitEvidence {
        self.evidence
            .as_ref()
            .expect("test prepared tick requested commit evidence")
    }

    pub(super) fn commit(self) -> CandidateTickCommitResult {
        let _receipt = self.prepared.commit();
        CandidateTickCommitResult {
            tick: TickCommitResult {
                events: self.events,
                event_keys: self.event_keys,
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
        location: "pipeline::candidate_commit".to_owned(),
    }
}

#[cfg(test)]
mod event_identity_tests {
    use super::*;
    use crate::{AccountId, OrderId, StockCode};

    #[test]
    fn quote_expiry_cancel_and_same_account_projection_cancel_get_distinct_local_keys() {
        let canceled = |seq| crate::Event::OrderCanceled {
            seq,
            account: AccountId(3),
            code: StockCode("600001".to_owned()),
            id: OrderId(seq),
            remaining_qty: 100,
        };
        let events = [canceled(1), canceled(2)];
        let keys = [
            super::super::EventStableKey::for_event(
                &events[0],
                super::super::event_key::QUOTE_EXPIRY_EVENT_INDEX_BASE,
            ),
            super::super::EventStableKey::for_event(&events[1], 0),
        ];
        validate_event_keys(&events, &keys, 1).unwrap();

        assert_ne!(keys[0], keys[1]);
        assert_eq!(
            keys[0].local_event_index(),
            super::super::event_key::QUOTE_EXPIRY_EVENT_INDEX_BASE
        );
        assert_eq!(keys[1].local_event_index(), 0);
    }
}
