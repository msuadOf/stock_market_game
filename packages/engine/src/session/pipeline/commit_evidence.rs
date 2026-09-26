//! Read-only execution facts captured at the prepared P9 boundary.
//!
//! The evidence owns clones of the receipts and ledger rows that the pipeline
//! actually validated. It is deliberately not part of `GameSession` or
//! `SaveSlot`, and it cannot mutate the prepared candidate or committed
//! authority.

use super::{
    validate_indexed_receipt_keys, Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger,
    EnvelopeOrigin, EnvelopeReceipt, FeeComponents, IndexedReceiptKey, JournalRank, ReceiptIndex,
    ReceiptLocalKey, ReceiptSource, ResVec, StepFatal,
};
use crate::{Money, StockCode};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    static CAPTURE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn capture_count_for_test() -> usize {
    CAPTURE_COUNT.with(std::cell::Cell::get)
}

#[derive(Clone, Debug)]
pub struct CommitEnvelopeChain {
    envelope: Envelope,
    receipts: Vec<EnvelopeReceipt>,
    terminal: bool,
}

impl CommitEnvelopeChain {
    pub const fn envelope(&self) -> &Envelope {
        &self.envelope
    }

    pub fn receipts(&self) -> &[EnvelopeReceipt] {
        &self.receipts
    }

    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }
}

/// Counts emitted by one real B2 stock-worker tail execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B2FinalizerExecution {
    stock: StockCode,
    auction_tail_passes: u64,
    auction_completion_passes: u64,
    day_end_passes: u64,
}

impl B2FinalizerExecution {
    pub(super) const fn new(
        stock: StockCode,
        auction_tail_passes: u64,
        auction_completion_passes: u64,
        day_end_passes: u64,
    ) -> Self {
        Self {
            stock,
            auction_tail_passes,
            auction_completion_passes,
            day_end_passes,
        }
    }

    pub const fn stock(&self) -> &StockCode {
        &self.stock
    }

    pub const fn auction_tail_passes(&self) -> u64 {
        self.auction_tail_passes
    }

    pub const fn auction_completion_passes(&self) -> u64 {
        self.auction_completion_passes
    }

    pub const fn day_end_passes(&self) -> u64 {
        self.day_end_passes
    }
}

/// Immutable Task 9 seam captured at the prepared-P9 boundary before P9
/// rebases tick-local ledger evidence for the next tick.
#[derive(Clone, Debug)]
pub struct TickCommitEvidence {
    envelope_chains: Vec<CommitEnvelopeChain>,
    receipts: Vec<EnvelopeReceipt>,
    p0_receipt_count: usize,
    b2_finalizers: Vec<B2FinalizerExecution>,
    next_receipt_index: u64,
}

impl TickCommitEvidence {
    pub fn envelope_chains(&self) -> &[CommitEnvelopeChain] {
        &self.envelope_chains
    }

    /// Every applied receipt in its real global-index order.
    pub fn receipts(&self) -> &[EnvelopeReceipt] {
        &self.receipts
    }

    /// The actual normalized P0 prefix, never reconstructed from expiry summaries.
    pub fn p0_receipts(&self) -> &[EnvelopeReceipt] {
        &self.receipts[..self.p0_receipt_count]
    }

    pub fn b2_finalizers(&self) -> &[B2FinalizerExecution] {
        &self.b2_finalizers
    }

    pub const fn next_receipt_index(&self) -> u64 {
        self.next_receipt_index
    }

    pub(super) fn capture(
        ledger: &EnvelopeLedger,
        receipts: &[EnvelopeReceipt],
        expected_keys: &[ReceiptLocalKey],
        b2_finalizers: Vec<B2FinalizerExecution>,
    ) -> Result<Self, StepFatal> {
        #[cfg(test)]
        CAPTURE_COUNT.with(|count| count.set(count.get() + 1));
        ledger.validate_complete_evidence()?;

        let actual_keys = receipts
            .iter()
            .map(|receipt| receipt.local_key.clone())
            .collect::<Vec<_>>();
        if actual_keys != expected_keys {
            return Err(invariant(
                "applied receipt values do not match the prepared receipt-key journal",
            ));
        }
        let indexed = receipts
            .iter()
            .map(|receipt| IndexedReceiptKey {
                index: ReceiptIndex(receipt.index),
                envelope: receipt.envelope.clone(),
                local_key: receipt.local_key.clone(),
            })
            .collect::<Vec<_>>();
        validate_indexed_receipt_keys(&indexed)?;

        let actual_seen = actual_keys.iter().cloned().collect::<BTreeSet<_>>();
        if actual_seen != ledger.seen_local_keys {
            return Err(invariant(
                "applied receipt values do not cover the ledger receipt identities",
            ));
        }
        if let Some(last) = receipts.last() {
            let expected_cursor = last
                .index
                .checked_add(1)
                .ok_or_else(|| invariant("prepared receipt cursor overflow"))?;
            if expected_cursor != ledger.next_receipt_index() {
                return Err(invariant(
                    "prepared receipt values do not reach the ledger cursor",
                ));
            }
        }

        let mut reached_sealed = false;
        let mut p0_receipt_count = 0usize;
        for receipt in receipts {
            match receipt.local_key.journal() {
                JournalRank::PreSeal => {
                    if reached_sealed
                        || !matches!(receipt.local_key.source(), ReceiptSource::P0Expiry(_))
                    {
                        return Err(invariant(
                            "prepared P0 receipts are not the canonical PreSeal prefix",
                        ));
                    }
                    p0_receipt_count = p0_receipt_count
                        .checked_add(1)
                        .ok_or_else(|| invariant("prepared P0 receipt count overflow"))?;
                }
                JournalRank::SealedBatch => reached_sealed = true,
            }
        }

        let mut envelopes = BTreeMap::<EnvelopeKey, (Envelope, bool)>::new();
        for (key, envelope) in &ledger.envelopes {
            if envelopes
                .insert(key.clone(), (envelope.clone(), false))
                .is_some()
            {
                return Err(invariant("duplicate live envelope in commit evidence"));
            }
        }
        for (key, envelope) in &ledger.terminal_envelopes {
            if envelopes
                .insert(key.clone(), (envelope.clone(), true))
                .is_some()
            {
                return Err(invariant(
                    "live and terminal commit-evidence envelopes overlap",
                ));
            }
        }

        let mut receipts_by_envelope = BTreeMap::<EnvelopeKey, Vec<EnvelopeReceipt>>::new();
        for receipt in receipts {
            if !envelopes.contains_key(&receipt.envelope) {
                return Err(invariant(
                    "applied receipt has no pre-commit live or terminal envelope",
                ));
            }
            receipts_by_envelope
                .entry(receipt.envelope.clone())
                .or_default()
                .push(receipt.clone());
        }
        validate_receipt_replay(ledger, receipts, &envelopes, &receipts_by_envelope)?;

        let envelope_chains = envelopes
            .into_iter()
            .map(|(key, (envelope, terminal))| CommitEnvelopeChain {
                envelope,
                receipts: receipts_by_envelope.remove(&key).unwrap_or_default(),
                terminal,
            })
            .collect::<Vec<_>>();
        if !receipts_by_envelope.is_empty() {
            return Err(invariant(
                "commit evidence retained an unbound receipt chain",
            ));
        }

        Ok(Self {
            envelope_chains,
            receipts: receipts.to_vec(),
            p0_receipt_count,
            b2_finalizers,
            next_receipt_index: ledger.next_receipt_index(),
        })
    }
}

fn validate_receipt_replay(
    expected: &EnvelopeLedger,
    receipts: &[EnvelopeReceipt],
    envelopes: &BTreeMap<EnvelopeKey, (Envelope, bool)>,
    receipts_by_envelope: &BTreeMap<EnvelopeKey, Vec<EnvelopeReceipt>>,
) -> Result<(), StepFatal> {
    let mut initial = Vec::with_capacity(envelopes.len());
    for (key, (envelope, _)) in envelopes {
        let chain = receipts_by_envelope.get(key).map_or(&[][..], Vec::as_slice);
        let initial_envelope = if let Some(first) = chain.first() {
            let filled_this_tick = chain.iter().try_fold(0_u32, |total, receipt| {
                let filled = receipt
                    .qty_before
                    .checked_sub(receipt.qty_after)
                    .ok_or_else(|| invariant("receipt quantity chain regressed during replay"))?;
                total
                    .checked_add(filled)
                    .ok_or_else(|| invariant("receipt filled quantity overflow during replay"))
            })?;
            let nominal_this_tick = chain
                .iter()
                .try_fold(FeeComponents::ZERO, |total, receipt| {
                    checked_fee_add(total, receipt.nominal)
                })?;
            let final_audit = envelope.audit();
            let initial_audit = EnvelopeAudit {
                limit: final_audit.limit,
                remaining_qty: first.qty_before,
                filled_qty: final_audit
                    .filled_qty
                    .checked_sub(filled_this_tick)
                    .ok_or_else(|| {
                        invariant("receipt filled quantity exceeds final envelope audit")
                    })?,
                filled_value: first.value_before,
                nominal: checked_fee_sub(final_audit.nominal, nominal_this_tick)?,
                charged: first.charged_before,
            };
            envelope_at_tick_start(envelope, initial_audit)
        } else {
            if envelope.spent() != ResVec::ZERO || envelope.released() != ResVec::ZERO {
                return Err(invariant(
                    "pre-commit envelope changed without an applied receipt value",
                ));
            }
            envelope.clone()
        };
        initial.push(initial_envelope);
    }

    let first_index = receipts
        .first()
        .map_or(expected.next_receipt_index(), |receipt| receipt.index);
    let mut replay = EnvelopeLedger::new(first_index, initial).map_err(replay_error)?;
    let mut replayed_receipts = receipts.to_vec();
    let terminal_keys = envelopes
        .iter()
        .filter_map(|(key, (_, terminal))| terminal.then_some(key.clone()))
        .collect::<Vec<_>>();
    replay
        .replay_private_for_commit_evidence(&mut replayed_receipts, &terminal_keys)
        .map_err(replay_error)?;
    if replay != *expected {
        return Err(invariant(
            "applied receipt values do not replay to the prepared envelope ledger",
        ));
    }
    Ok(())
}

fn envelope_at_tick_start(envelope: &Envelope, audit: EnvelopeAudit) -> Envelope {
    let basis = envelope.basis();
    match envelope.origin() {
        EnvelopeOrigin::TickStart => {
            Envelope::tick_start_existing(envelope.key().clone(), basis.cash, basis.shares, audit)
        }
        EnvelopeOrigin::P3Created => {
            Envelope::p3_created(envelope.key().clone(), basis.cash, basis.shares, audit)
        }
    }
}

fn checked_fee_add(left: FeeComponents, right: FeeComponents) -> Result<FeeComponents, StepFatal> {
    Ok(FeeComponents {
        commission: checked_money_add(left.commission, right.commission)?,
        stamp_tax: checked_money_add(left.stamp_tax, right.stamp_tax)?,
        transfer_fee: checked_money_add(left.transfer_fee, right.transfer_fee)?,
    })
}

fn checked_fee_sub(left: FeeComponents, right: FeeComponents) -> Result<FeeComponents, StepFatal> {
    Ok(FeeComponents {
        commission: checked_money_sub(left.commission, right.commission)?,
        stamp_tax: checked_money_sub(left.stamp_tax, right.stamp_tax)?,
        transfer_fee: checked_money_sub(left.transfer_fee, right.transfer_fee)?,
    })
}

fn checked_money_add(left: Money, right: Money) -> Result<Money, StepFatal> {
    left.add(right)
        .map_err(|error| invariant(&format!("receipt fee replay overflow: {error}")))
}

fn checked_money_sub(left: Money, right: Money) -> Result<Money, StepFatal> {
    let difference = left
        .sub(right)
        .map_err(|error| invariant(&format!("receipt fee replay underflow: {error}")))?;
    if difference < Money::ZERO {
        return Err(invariant("receipt fee replay became negative"));
    }
    Ok(difference)
}

fn replay_error(error: StepFatal) -> StepFatal {
    invariant(&format!(
        "applied receipt values failed prepared-ledger replay: {error}"
    ))
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::commit_evidence".to_owned(),
    }
}
