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

impl EnvelopeLedger {
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
        let mut shadow = self.clone();
        let candidate = ledger_candidate::normalize(&mut shadow.next_receipt_index, receipts)?;
        for receipt in &candidate {
            if !shadow.seen_local_keys.insert(receipt.local_key.clone()) {
                return Err(ledger_validation::invariant("duplicate receipt local key"));
            }
            shadow.apply_one(receipt)?;
        }
        ledger_conservation::validate(&shadow)?;
        *self = shadow;
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
        for key in keys {
            let envelope = shadow
                .envelopes
                .remove(key)
                .ok_or_else(|| ledger_validation::invariant("unknown terminal envelope"))?;
            if envelope.live() != ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "terminal envelope still has live resources",
                ));
            }
            shadow.terminal_envelopes.insert(key.clone(), envelope);
        }
        ledger_conservation::validate(&shadow)?;
        *self = shadow;
        Ok(())
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
                || shadow.envelopes.contains_key(&key)
                || shadow.terminal_envelopes.contains_key(&key)
                || shadow.audits.contains_key(&key)
                || shadow.conservation.contains_key(&key)
            {
                return Err(ledger_validation::invariant(
                    "inserted P3 envelope key conflicts with ledger evidence",
                ));
            }
            prepared.push((key, envelope));
        }
        validate_ledger_evidence(&shadow)?;

        for (key, envelope) in prepared {
            shadow.audits.insert(key.clone(), envelope.audit());
            shadow
                .conservation
                .insert(key.clone(), ConservationState::EMPTY);
            shadow.envelopes.insert(key, envelope);
        }
        validate_ledger_evidence(&shadow)?;
        *self = shadow;
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
        validate_ledger_evidence(&candidate)?;

        let mut envelopes = BTreeMap::new();
        let mut audits = BTreeMap::new();
        let mut conservation = BTreeMap::new();
        for (key, envelope) in &candidate.envelopes {
            let live = envelope.live();
            let rebased = Envelope::tick_start_existing(
                key.clone(),
                live.cash,
                live.shares,
                envelope.audit(),
            );
            rebased.validate()?;
            envelopes.insert(key.clone(), rebased);
            audits.insert(key.clone(), envelope.audit());
            conservation.insert(key.clone(), ConservationState::EMPTY);
        }

        candidate.envelopes = envelopes;
        candidate.terminal_envelopes.clear();
        candidate.audits = audits;
        candidate.conservation = conservation;
        candidate.seen_local_keys.clear();
        ledger_conservation::validate(&candidate)?;
        *self = candidate;
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
