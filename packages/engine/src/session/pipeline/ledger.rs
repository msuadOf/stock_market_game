use super::{
    ledger_candidate, ledger_conservation, ledger_validation, Envelope, EnvelopeAudit, EnvelopeKey,
    EnvelopeOrigin, FeeComponents, ReceiptDelta, ReceiptLocalKey, ResVec, StepFatal,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptKind {
    Fill,
    Release,
    Reject,
    Rollover,
}

#[derive(Clone, Debug)]
pub struct EnvelopeReceipt {
    pub index: u64,
    pub local_key: ReceiptLocalKey,
    pub envelope: EnvelopeKey,
    pub kind: ReceiptKind,
    pub qty_before: u32,
    pub qty_after: u32,
    pub value_before: crate::Money,
    pub value_after: crate::Money,
    pub delta: ReceiptDelta,
    pub nominal: FeeComponents,
    pub charged: FeeComponents,
    pub charged_before: FeeComponents,
    pub charged_after: FeeComponents,
    pub deliver_qty: u32,
    pub deliver_cash: crate::Money,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(super) struct ConservationState {
    pub(super) p0_released: ResVec,
    pub(super) sealed_spent: ResVec,
    pub(super) sealed_released: ResVec,
}

impl ConservationState {
    pub(super) const EMPTY: Self = Self {
        p0_released: ResVec::ZERO,
        sealed_spent: ResVec::ZERO,
        sealed_released: ResVec::ZERO,
    };
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct EnvelopeLedger {
    pub(super) envelopes: BTreeMap<EnvelopeKey, Envelope>,
    pub(super) terminal_envelopes: BTreeMap<EnvelopeKey, Envelope>,
    pub(super) audits: BTreeMap<EnvelopeKey, EnvelopeAudit>,
    pub(super) conservation: BTreeMap<EnvelopeKey, ConservationState>,
    pub(super) seen_local_keys: BTreeSet<ReceiptLocalKey>,
    pub(super) next_receipt_index: u64,
}

/// JSON-safe canonical projection used by the session business hash.
#[derive(serde::Serialize)]
pub(in crate::session) struct EnvelopeLedgerHashProjection<'a> {
    envelopes: Vec<(&'a EnvelopeKey, &'a Envelope)>,
    terminal_envelopes: Vec<(&'a EnvelopeKey, &'a Envelope)>,
    audits: Vec<(&'a EnvelopeKey, &'a EnvelopeAudit)>,
    conservation: Vec<(&'a EnvelopeKey, &'a ConservationState)>,
    seen_local_keys: Vec<&'a ReceiptLocalKey>,
    next_receipt_index: u64,
}

impl EnvelopeLedger {
    pub(in crate::session) fn hash_projection(&self) -> EnvelopeLedgerHashProjection<'_> {
        EnvelopeLedgerHashProjection {
            envelopes: self.envelopes.iter().collect(),
            terminal_envelopes: self.terminal_envelopes.iter().collect(),
            audits: self.audits.iter().collect(),
            conservation: self.conservation.iter().collect(),
            seen_local_keys: self.seen_local_keys.iter().collect(),
            next_receipt_index: self.next_receipt_index,
        }
    }

    pub fn new(
        next_receipt_index: u64,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<Self, StepFatal> {
        let mut values = BTreeMap::new();
        let mut audits = BTreeMap::new();
        let mut conservation = BTreeMap::new();
        for envelope in envelopes {
            let key = envelope.key().clone();
            if values.insert(key.clone(), envelope.clone()).is_some() {
                return Err(ledger_validation::invariant("duplicate envelope"));
            }
            audits.insert(key.clone(), envelope.audit());
            conservation.insert(key, ConservationState::EMPTY);
        }
        Ok(Self {
            envelopes: values,
            terminal_envelopes: BTreeMap::new(),
            audits,
            conservation,
            seen_local_keys: BTreeSet::new(),
            next_receipt_index,
        })
    }

    pub fn apply(&mut self, receipts: &mut [EnvelopeReceipt]) -> Result<(), StepFatal> {
        self.apply_with_normalizer(receipts, ledger_candidate::normalize)
    }

    fn apply_with_normalizer(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        normalize: impl FnOnce(&mut u64, &[EnvelopeReceipt]) -> Result<Vec<EnvelopeReceipt>, StepFatal>,
    ) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.apply_private_with_normalizer(receipts, normalize, true)?;
        *self = shadow;
        Ok(())
    }

    fn apply_private_with_normalizer(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        normalize: impl FnOnce(&mut u64, &[EnvelopeReceipt]) -> Result<Vec<EnvelopeReceipt>, StepFatal>,
        validate_aggregate: bool,
    ) -> Result<(), StepFatal> {
        let candidate = normalize(&mut self.next_receipt_index, receipts)?;
        for receipt in &candidate {
            if !self.seen_local_keys.insert(receipt.local_key.clone()) {
                return Err(ledger_validation::invariant("duplicate receipt local key"));
            }
            self.apply_one(receipt)?;
        }
        if validate_aggregate {
            ledger_conservation::validate(self)?;
        }
        receipts.clone_from_slice(&candidate);
        Ok(())
    }

    fn apply_one(&mut self, receipt: &EnvelopeReceipt) -> Result<(), StepFatal> {
        let audit = *self
            .audits
            .get(&receipt.envelope)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope audit"))?;
        ledger_validation::validate(receipt, audit, self.envelopes.get(&receipt.envelope))?;
        let audit_after = ledger_validation::next_audit(receipt, audit)?;
        self.envelopes
            .get_mut(&receipt.envelope)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope"))?
            .apply(receipt.delta, audit_after)?;
        ledger_conservation::record(self, receipt)?;
        self.audits.insert(receipt.envelope.clone(), audit_after);
        Ok(())
    }

    pub fn remove_terminal(&mut self, keys: &[EnvelopeKey]) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.remove_terminal_private(keys, true)?;
        *self = shadow;
        Ok(())
    }

    fn remove_terminal_private(
        &mut self,
        keys: &[EnvelopeKey],
        validate_aggregate: bool,
    ) -> Result<(), StepFatal> {
        for key in keys {
            let envelope = self
                .envelopes
                .remove(key)
                .ok_or_else(|| ledger_validation::invariant("unknown terminal envelope"))?;
            if envelope.live() != ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "terminal envelope still has live resources",
                ));
            }
            self.terminal_envelopes.insert(key.clone(), envelope);
        }
        if validate_aggregate {
            ledger_conservation::validate(self)?;
        }
        Ok(())
    }

    /// Commit evidence owns this newly built ledger and discards it on error.
    /// Reuse the ordinary transition checks without cloning that private value twice.
    pub(super) fn replay_private_for_commit_evidence(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        self.apply_private_with_normalizer(receipts, ledger_candidate::normalize, true)?;
        self.remove_terminal_private(terminal_keys, true)
    }

    /// P4 owns a discardable stock-round ledger. Validate each receipt transition
    /// immediately, then check aggregate conservation once before publishing the
    /// round. An error discards the whole stock round and ultimately the tick.
    pub(super) fn apply_private_for_stock_round(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        self.apply_private_with_normalizer(receipts, ledger_candidate::normalize, false)?;
        self.remove_terminal_private(terminal_keys, false)
    }

    /// P5 owns this ledger for the rest of the private tick. The caller checks
    /// complete evidence after the whole receipt batch, so each intermediate
    /// transition can use the same checked, in-place path as a stock round.
    #[cfg(any(test, feature = "verification-harness"))]
    pub(super) fn apply_private_in_delivery_order_for_p5(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        self.apply_private_with_normalizer(
            receipts,
            |next_index, receipts| ledger_candidate::index_in_order(next_index, receipts.to_vec()),
            false,
        )?;
        self.remove_terminal_private(terminal_keys, false)
    }

    /// Atomically installs the P3-created envelopes that P5 will validate.
    ///
    /// This operation only creates ledger rows. It does not allocate receipt
    /// indices, consume local receipt identities, or apply any transition.
    pub fn insert_created(
        &mut self,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.insert_created_for_stock_round(envelopes)?;
        validate_ledger_evidence(self)?;
        validate_ledger_evidence(&shadow)?;
        *self = shadow;
        Ok(())
    }

    /// The stock round and its enclosing tick are discarded together on error.
    /// Validate every new row before writing, then use the owned ledger directly.
    pub(super) fn insert_created_for_stock_round(
        &mut self,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let mut batch_keys = BTreeSet::new();
        let mut prepared = Vec::new();
        for envelope in envelopes {
            envelope.validate()?;
            if envelope.origin() != EnvelopeOrigin::P3Created {
                return Err(ledger_validation::invariant(
                    "inserted envelope was not created by P3",
                ));
            }
            if envelope.live() == ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "inserted P3 envelope has no live resources",
                ));
            }
            let key = envelope.key().clone();
            if !batch_keys.insert(key.clone())
                || self.envelopes.contains_key(&key)
                || self.terminal_envelopes.contains_key(&key)
                || self.audits.contains_key(&key)
                || self.conservation.contains_key(&key)
            {
                return Err(ledger_validation::invariant(
                    "inserted P3 envelope key conflicts with ledger evidence",
                ));
            }
            prepared.push((key, envelope));
        }
        for (key, envelope) in prepared {
            self.audits.insert(key.clone(), envelope.audit());
            self.conservation
                .insert(key.clone(), ConservationState::EMPTY);
            self.envelopes.insert(key, envelope);
        }
        Ok(())
    }

    pub fn reset_tick_state(&mut self) {
        self.envelopes.clear();
        self.terminal_envelopes.clear();
        self.audits.clear();
        self.conservation.clear();
        self.seen_local_keys.clear();
    }

    /// Rolls every still-live envelope into the next tick without discarding its
    /// cumulative fill and fee audit.
    ///
    /// Receipt identities, terminal evidence, and conservation counters are
    /// tick-local. The global receipt cursor and every live envelope's cumulative
    /// audit are cross-tick state. Work is performed on a private candidate so an
    /// invalid source ledger leaves `self` unchanged.
    pub fn rebase_live_for_next_tick(&mut self) -> Result<(), StepFatal> {
        let mut candidate = self.clone();
        candidate.rebase_private_for_tick_commit()?;
        *self = candidate;
        Ok(())
    }

    /// The P9 owner already discards its whole tick candidate on failure.
    /// Move live rows out of that candidate instead of cloning the ledger again.
    pub(super) fn rebase_private_for_tick_commit(&mut self) -> Result<(), StepFatal> {
        validate_ledger_evidence(self)?;

        let mut envelopes = BTreeMap::new();
        for (key, envelope) in std::mem::take(&mut self.envelopes) {
            let live = envelope.live();
            let rebased = Envelope::tick_start_existing(
                key.clone(),
                live.cash,
                live.shares,
                envelope.audit(),
            );
            rebased.validate()?;
            envelopes.insert(key, rebased);
        }

        // Source evidence was checked above. Each new basis equals its live
        // balance, so its empty conservation row is valid by construction.
        // Keep the already owned audit rows instead of cloning every live key.
        self.audits.retain(|key, _| envelopes.contains_key(key));
        self.conservation.retain(|key, state| {
            if envelopes.contains_key(key) {
                *state = ConservationState::EMPTY;
                true
            } else {
                false
            }
        });
        self.envelopes = envelopes;
        self.terminal_envelopes.clear();
        self.seen_local_keys.clear();
        Ok(())
    }

    pub const fn next_receipt_index(&self) -> u64 {
        self.next_receipt_index
    }

    pub fn get(&self, key: &EnvelopeKey) -> Result<&Envelope, StepFatal> {
        self.envelopes
            .get(key)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope"))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&EnvelopeKey, &Envelope)> {
        self.envelopes.iter()
    }

    pub fn terminal_count(&self) -> usize {
        self.terminal_envelopes.len()
    }

    pub fn hydrate_or_validate(
        &mut self,
        projected: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let candidate = Self::new(self.next_receipt_index, projected)?;
        if candidate
            .envelopes
            .keys()
            .any(|key| self.terminal_envelopes.contains_key(key))
        {
            return Err(ledger_validation::invariant(
                "live projection overlaps terminal envelope evidence",
            ));
        }
        if self.envelopes.is_empty() {
            self.envelopes = candidate.envelopes;
            self.audits = candidate.audits;
            self.conservation = candidate.conservation;
        } else if self.envelopes != candidate.envelopes {
            return Err(ledger_validation::invariant(
                "live envelope projection disagrees with ledger",
            ));
        }
        Ok(())
    }

    pub fn validate_conservation(&self) -> Result<(), StepFatal> {
        ledger_conservation::validate(self)
    }

    /// Validates every authoritative envelope and its matching evidence rows.
    ///
    /// This read-only gate is intended for adapters that must reject an
    /// incomplete or contradictory ledger before deriving downstream input.
    pub fn validate_complete_evidence(&self) -> Result<(), StepFatal> {
        validate_ledger_evidence(self)
    }
}

fn validate_ledger_evidence(ledger: &EnvelopeLedger) -> Result<(), StepFatal> {
    ledger_conservation::validate(ledger)?;
    if ledger
        .envelopes
        .values()
        .any(|envelope| envelope.live() == ResVec::ZERO)
    {
        return Err(ledger_validation::invariant(
            "live envelope row has no live resources",
        ));
    }
    if ledger
        .terminal_envelopes
        .values()
        .any(|envelope| envelope.live() != ResVec::ZERO)
    {
        return Err(ledger_validation::invariant(
            "terminal envelope row still has live resources",
        ));
    }
    let expected_rows = ledger
        .envelopes
        .len()
        .checked_add(ledger.terminal_envelopes.len())
        .ok_or_else(|| ledger_validation::invariant("ledger row count overflow"))?;
    if ledger.audits.len() != expected_rows || ledger.conservation.len() != expected_rows {
        return Err(ledger_validation::invariant(
            "ledger evidence rows do not match envelope rows",
        ));
    }
    for (key, envelope) in ledger
        .envelopes
        .iter()
        .chain(ledger.terminal_envelopes.iter())
    {
        envelope.validate()?;
        if ledger.envelopes.contains_key(key) && ledger.terminal_envelopes.contains_key(key) {
            return Err(ledger_validation::invariant(
                "live and terminal envelope rows overlap",
            ));
        }
        if ledger.audits.get(key).copied() != Some(envelope.audit()) {
            return Err(ledger_validation::invariant(
                "envelope audit row disagrees with envelope",
            ));
        }
        if !ledger.conservation.contains_key(key) {
            return Err(ledger_validation::invariant(
                "envelope has no conservation row",
            ));
        }
    }
    Ok(())
}
