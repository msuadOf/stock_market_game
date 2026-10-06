use super::conservation::{FeeComponents, ReceiptDelta, ResVec};
use super::{EnvelopeKey, StepFatal};
use crate::{LimitPrice, Money};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum EnvelopeOrigin {
    TickStart,

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

/// 只读 TickStart 投影借用股票代码；费用与资源仍由订单事实独立计算。
#[derive(Clone, Copy, Debug)]
pub(in crate::session) struct BorrowedTickStartEnvelope<'a> {
    pub account: crate::AccountId,
    pub stock: &'a crate::StockCode,
    pub order: crate::OrderId,
    pub side: crate::Side,
    pub live: ResVec,
    pub audit: EnvelopeAudit,
}

impl BorrowedTickStartEnvelope<'_> {
    pub(in crate::session) fn identity(
        &self,
    ) -> (crate::AccountId, &crate::StockCode, crate::OrderId, u8) {
        (
            self.account,
            self.stock,
            self.order,
            match self.side {
                crate::Side::Buy => 0,
                crate::Side::Sell => 1,
            },
        )
    }

    pub(in crate::session) fn copy_key_into(&self, key: &mut EnvelopeKey) {
        key.account = self.account;
        key.stock.0.clone_from(&self.stock.0);
        key.order = self.order;
        key.side = self.side;
    }

    pub(in crate::session) fn matches_key(&self, key: &EnvelopeKey) -> bool {
        key.account == self.account
            && key.stock == *self.stock
            && key.order == self.order
            && key.side == self.side
    }

    pub(in crate::session) fn matches(&self, envelope: &Envelope) -> bool {
        // 显式解构所有字段，Envelope 新增字段时必须同步此完整比较。
        let Envelope {
            key,
            origin,
            basis,
            live,
            spent,
            released,
            audit,
            pending_price,
        } = envelope;
        self.matches_key(key)
            && *origin == EnvelopeOrigin::TickStart
            && *basis == self.live
            && *live == self.live
            && *spent == ResVec::ZERO
            && *released == ResVec::ZERO
            && *audit == self.audit
            && pending_price.is_none()
    }
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

    /// 只重置已校验 private candidate 的 tick 局部资源，不复制键或累计费用。
    pub(super) fn rebase_for_next_tick(&mut self) -> Result<(), StepFatal> {
        if self.pending_price.is_some() {
            return Err(super::ledger_validation::invariant(
                "live envelope reached tick commit with an unresolved price",
            ));
        }
        self.origin = EnvelopeOrigin::TickStart;
        self.basis = self.live;
        self.spent = ResVec::ZERO;
        self.released = ResVec::ZERO;
        self.validate()
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
                description: format!(
                    "envelope {:?} 在 AccountValidation 之外仍含有 pending price",
                    self.key
                ),
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

#[cfg(test)]
mod borrowed_projection_tests {
    use super::*;
    use crate::{AccountId, OrderId, Side, StockCode};

    #[test]
    fn borrowed_projection_matches_exact_owned_tick_start_payload_including_every_field() {
        let stock = StockCode("600888".to_owned());
        let projection = BorrowedTickStartEnvelope {
            account: AccountId(1),
            stock: &stock,
            order: OrderId(9),
            side: Side::Buy,
            live: ResVec::new(Money::from_cents(100), 0),
            audit: EnvelopeAudit {
                limit: Money::from_cents(1),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        };
        let expected = Envelope::tick_start_existing(
            EnvelopeKey {
                account: projection.account,
                stock: stock.clone(),
                order: projection.order,
                side: projection.side,
            },
            projection.live.cash,
            projection.live.shares,
            projection.audit,
        );
        let mut cases = vec![expected.clone()];
        macro_rules! changed {
            ($field:ident $(.$nested:ident)*, $value:expr) => {{
                let mut candidate = expected.clone();
                candidate.$field$(.$nested)* = $value;
                cases.push(candidate);
            }};
        }
        changed!(key.account, AccountId(2));
        changed!(key.stock, StockCode("600889".to_owned()));
        changed!(key.order, OrderId(10));
        changed!(key.side, Side::Sell);
        changed!(origin, EnvelopeOrigin::CreatedAtValidation);
        changed!(basis.cash, Money::from_cents(101));
        changed!(basis.shares, 1);
        changed!(live.cash, Money::from_cents(99));
        changed!(live.shares, 1);
        changed!(spent.cash, Money::from_cents(1));
        changed!(spent.shares, 1);
        changed!(released.cash, Money::from_cents(1));
        changed!(released.shares, 1);
        changed!(audit.limit, Money::from_cents(2));
        changed!(audit.remaining_qty, 99);
        changed!(audit.filled_qty, 1);
        changed!(audit.filled_value, Money::from_cents(1));
        changed!(audit.nominal.commission, Money::from_cents(1));
        changed!(audit.nominal.stamp_tax, Money::from_cents(1));
        changed!(audit.nominal.transfer_fee, Money::from_cents(1));
        changed!(audit.charged.commission, Money::from_cents(1));
        changed!(audit.charged.stamp_tax, Money::from_cents(1));
        changed!(audit.charged.transfer_fee, Money::from_cents(1));
        changed!(pending_price, Some(crate::LimitPrice::Highest));
        for candidate in cases {
            assert_eq!(
                projection.matches(&candidate),
                candidate == expected,
                "{candidate:?}"
            );
        }
    }
}
