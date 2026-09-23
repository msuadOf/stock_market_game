use super::{EnvelopeKey, StepFatal};
use crate::Money;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct ResVec {
    pub cash: Money,
    pub shares: u32,
}

impl ResVec {
    pub const ZERO: Self = Self {
        cash: Money::ZERO,
        shares: 0,
    };

    pub const fn new(cash: Money, shares: u32) -> Self {
        Self { cash, shares }
    }

    pub fn checked_add(self, other: Self) -> Result<Self, StepFatal> {
        Ok(Self {
            cash: self.cash.add(other.cash).map_err(invariant)?,
            shares: self
                .shares
                .checked_add(other.shares)
                .ok_or_else(|| overflow("shares add"))?,
        })
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, StepFatal> {
        let cash = self.cash.sub(other.cash).map_err(invariant)?;
        if cash.cents() < 0 {
            return Err(overflow("cash underflow"));
        }
        Ok(Self {
            cash,
            shares: self
                .shares
                .checked_sub(other.shares)
                .ok_or_else(|| overflow("shares underflow"))?,
        })
    }

    pub(super) fn validate_nonnegative(self) -> Result<(), StepFatal> {
        if self.cash.cents() < 0 {
            return Err(overflow("negative cash resource"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptDelta {
    pub spent: ResVec,
    pub released: ResVec,
    pub live_after: ResVec,
}

impl ReceiptDelta {
    pub const fn sealed(spent: ResVec, released: ResVec, live_after: ResVec) -> Self {
        Self {
            spent,
            released,
            live_after,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct FeeComponents {
    pub commission: Money,
    pub stamp_tax: Money,
    pub transfer_fee: Money,
}

impl FeeComponents {
    pub const ZERO: Self = Self {
        commission: Money::ZERO,
        stamp_tax: Money::ZERO,
        transfer_fee: Money::ZERO,
    };

    pub fn total(self) -> Result<Money, StepFatal> {
        self.commission
            .add(self.stamp_tax)
            .and_then(|fee| fee.add(self.transfer_fee))
            .map_err(invariant)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum ConservationBasis {
    TickStart(ResVec, ResVec, ResVec),
    P3Created(ResVec),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ConservationRow {
    pub key: EnvelopeKey,
    pub basis: ConservationBasis,
    pub sealed_spent: ResVec,
    pub sealed_released: ResVec,
    pub commit_live: ResVec,
}

impl ConservationRow {
    pub fn validate(&self) -> Result<(), StepFatal> {
        self.sealed_spent.validate_nonnegative()?;
        self.sealed_released.validate_nonnegative()?;
        self.commit_live.validate_nonnegative()?;
        let sealed_total = self.sealed_spent.checked_add(self.sealed_released)?;
        let sealed_total = sealed_total.checked_add(self.commit_live)?;
        match self.basis {
            ConservationBasis::TickStart(tick_start_live, p0_released, p1_live) => {
                tick_start_live.validate_nonnegative()?;
                p0_released.validate_nonnegative()?;
                p1_live.validate_nonnegative()?;
                if tick_start_live != p0_released.checked_add(p1_live)? {
                    return Err(conservation_error("preseal conservation mismatch"));
                }
                if p1_live != sealed_total {
                    return Err(conservation_error("sealed conservation mismatch"));
                }
            }
            ConservationBasis::P3Created(created) => {
                created.validate_nonnegative()?;
                if created != sealed_total {
                    return Err(conservation_error("created conservation mismatch"));
                }
            }
        }
        Ok(())
    }

    pub fn validate_with_preseal(&self, p0_released: ResVec) -> Result<(), StepFatal> {
        p0_released.validate_nonnegative()?;
        match self.basis {
            ConservationBasis::TickStart(_, recorded, _) if recorded == p0_released => {
                self.validate()
            }
            ConservationBasis::TickStart(..) => {
                Err(conservation_error("preseal conservation row mismatch"))
            }
            ConservationBasis::P3Created(_) if p0_released == ResVec::ZERO => self.validate(),
            ConservationBasis::P3Created(_) => Err(conservation_error(
                "P3-created envelope has a P0 contribution",
            )),
        }
    }
}

fn invariant(error: crate::MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::ResVec".to_owned(),
    }
}

fn overflow(detail: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: detail.to_owned(),
        location: "pipeline::ResVec".to_owned(),
    }
}

fn conservation_error(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ConservationRow".to_owned(),
    }
}
