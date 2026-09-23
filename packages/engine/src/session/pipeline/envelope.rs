use super::conservation::{FeeComponents, ReceiptDelta, ResVec};
use super::{EnvelopeKey, StepFatal};
use crate::Money;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum EnvelopeOrigin {
    TickStart,
    P3Created,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct EnvelopeAudit {
    pub limit: Money,
    pub remaining_qty: u32,
    pub filled_qty: u32,
    pub filled_value: Money,
    pub nominal: FeeComponents,
    pub charged: FeeComponents,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Envelope {
    key: EnvelopeKey,
    origin: EnvelopeOrigin,
    basis: ResVec,
    live: ResVec,
    spent: ResVec,
    released: ResVec,
    audit: EnvelopeAudit,
}

impl Envelope {
    pub const fn tick_start_existing(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
    ) -> Self {
        let live = ResVec::new(cash, shares);
        Self {
            key,
            origin: EnvelopeOrigin::TickStart,
            basis: live,
            live,
            spent: ResVec::ZERO,
            released: ResVec::ZERO,
            audit,
        }
    }

    pub const fn p3_created(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
    ) -> Self {
        let live = ResVec::new(cash, shares);
        Self {
            key,
            origin: EnvelopeOrigin::P3Created,
            basis: live,
            live,
            spent: ResVec::ZERO,
            released: ResVec::ZERO,
            audit,
        }
    }

    pub const fn live(&self) -> ResVec {
        self.live
    }
    pub const fn basis(&self) -> ResVec {
        self.basis
    }
    pub const fn spent(&self) -> ResVec {
        self.spent
    }
    pub const fn released(&self) -> ResVec {
        self.released
    }

    pub const fn key(&self) -> &EnvelopeKey {
        &self.key
    }

    pub const fn origin(&self) -> EnvelopeOrigin {
        self.origin
    }
    pub const fn audit(&self) -> EnvelopeAudit {
        self.audit
    }

    pub fn apply(
        &mut self,
        delta: ReceiptDelta,
        audit_after: EnvelopeAudit,
    ) -> Result<(), StepFatal> {
        let mut candidate = self.clone();
        let consumed = delta.spent.checked_add(delta.released)?;
        let expected = candidate.live.checked_sub(consumed)?;
        if expected != delta.live_after {
            return Err(StepFatal::InvariantViolation {
                description: format!("envelope {:?} chain mismatch", candidate.key),
                location: "pipeline::Envelope::apply".to_owned(),
            });
        }
        candidate.spent = candidate.spent.checked_add(delta.spent)?;
        candidate.released = candidate.released.checked_add(delta.released)?;
        candidate.live = delta.live_after;
        candidate.audit = audit_after;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), StepFatal> {
        self.basis.validate_nonnegative()?;
        self.spent.validate_nonnegative()?;
        self.released.validate_nonnegative()?;
        self.live.validate_nonnegative()?;
        let used = self
            .spent
            .checked_add(self.released)?
            .checked_add(self.live)?;
        if used != self.basis {
            return Err(StepFatal::InvariantViolation {
                description: format!("envelope {:?} conservation mismatch", self.key),
                location: "pipeline::Envelope::validate".to_owned(),
            });
        }
        Ok(())
    }
}
