use super::conservation::{FeeComponents, ReceiptDelta, ResVec};
use super::{EnvelopeKey, StepFatal};
use crate::{LimitPrice, Money};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum EnvelopeOrigin {
    TickStart,
    #[serde(rename = "P3Created")]
    CreatedAtValidation,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_price: Option<LimitPrice>,
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
            pending_price: None,
        }
    }

    pub const fn created_at_validation(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
    ) -> Self {
        let live = ResVec::new(cash, shares);
        Self {
            key,
            origin: EnvelopeOrigin::CreatedAtValidation,
            basis: live,
            live,
            spent: ResVec::ZERO,
            released: ResVec::ZERO,
            audit,
            pending_price: None,
        }
    }

    pub fn created_at_validation_with_pending_price(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
        pending_price: Option<LimitPrice>,
    ) -> Self {
        let mut envelope = Self::created_at_validation(key, cash, shares, audit);
        envelope.pending_price = pending_price;
        envelope
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

    pub const fn pending_price(&self) -> Option<LimitPrice> {
        self.pending_price
    }

    pub fn apply(
        &mut self,
        delta: ReceiptDelta,
        audit_after: EnvelopeAudit,
        resolved_price: bool,
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
        if resolved_price {
            if candidate.pending_price.is_none() {
                return Err(StepFatal::InvariantViolation {
                    description: format!("envelope {:?} has no pending price", candidate.key),
                    location: "pipeline::Envelope::apply".to_owned(),
                });
            }
            candidate.pending_price = None;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), StepFatal> {
        if self.pending_price.is_some() && self.origin != EnvelopeOrigin::CreatedAtValidation {
            return Err(StepFatal::InvariantViolation {
                description: format!("envelope {:?} has a pending price outside P3", self.key),
                location: "pipeline::Envelope::validate".to_owned(),
            });
        }
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
