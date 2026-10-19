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

/// B2 搬家：`FeeComponents` 定义移入中立费用域 [`crate::fees`]（市场域
/// desk 扣费三分项/个人域交割单与旧管线守恒审计共用）。此处仅重导出，
/// 不保留旧定义（不依赖逃生门）；`total()` 语义变化=返回 `MoneyError`，
/// 旧管线调用层（`ledger_validation` 等）自行映射为 `StepFatal`。
pub use crate::fees::FeeComponents;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum ConservationBasis {
    TickStart(ResVec, ResVec, ResVec),

    CreatedAtValidation(ResVec),
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
            ConservationBasis::TickStart(tick_start_live, expiry_released, allocation_live) => {
                tick_start_live.validate_nonnegative()?;
                expiry_released.validate_nonnegative()?;
                allocation_live.validate_nonnegative()?;
                if tick_start_live != expiry_released.checked_add(allocation_live)? {
                    return Err(conservation_error("preseal conservation mismatch"));
                }
                if allocation_live != sealed_total {
                    return Err(conservation_error("sealed conservation mismatch"));
                }
            }
            ConservationBasis::CreatedAtValidation(created) => {
                created.validate_nonnegative()?;
                if created != sealed_total {
                    return Err(conservation_error("created conservation mismatch"));
                }
            }
        }
        Ok(())
    }

    pub fn validate_with_preseal(&self, expiry_released: ResVec) -> Result<(), StepFatal> {
        expiry_released.validate_nonnegative()?;
        match self.basis {
            ConservationBasis::TickStart(_, recorded, _) if recorded == expiry_released => {
                self.validate()
            }
            ConservationBasis::TickStart(..) => {
                Err(conservation_error("preseal conservation row mismatch"))
            }
            ConservationBasis::CreatedAtValidation(_) if expiry_released == ResVec::ZERO => {
                self.validate()
            }
            ConservationBasis::CreatedAtValidation(_) => Err(conservation_error(
                "AccountValidation 创建的 envelope 含有 ExpiryShadow 贡献",
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
