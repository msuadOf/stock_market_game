//! 连续撮合与集合清算共享的逐腿费用、冻结资源和审计转换。

use super::{
    transition::{BuyFillInput, FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeReceipt, FeeComponents, JournalRank, ReceiptDelta,
    ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition, ResVec, StepFatal,
};
use crate::{GameConfig, Money, Side};

pub(super) fn fill_receipt(
    envelope: &mut Envelope,
    source: ReceiptSource,
    ordinal: u64,
    filled_value_before: Money,
    fill_qty: u32,
    gross: Money,
    config: &GameConfig,
) -> Result<EnvelopeReceipt, StepFatal> {
    let audit = envelope.audit();
    if fill_qty == 0 || audit.filled_value != filled_value_before {
        return Err(invariant("成交腿数量或累计成交额与 Envelope 审计不符"));
    }
    let remaining_qty_after = audit
        .remaining_qty
        .checked_sub(fill_qty)
        .ok_or_else(|| invariant("成交腿超过活动委托剩余量"))?;
    let value_after = audit
        .filled_value
        .add(gross)
        .map_err(|error| invariant(&error.to_string()))?;
    let transition = match envelope.key().side {
        Side::Buy => FillTransition::buy(BuyFillInput {
            config,
            limit: audit.limit,
            fill_qty,
            remaining_qty_after,
            filled_value_before: audit.filled_value,
            gross_delta: gross,
            live_before: envelope.live(),
        })?,
        Side::Sell => FillTransition::sell(SellFillInput {
            config,
            fill_qty,
            remaining_qty_after,
            filled_value_before: audit.filled_value,
            gross_delta: gross,
            nominal_before: audit.nominal,
            charged_before: audit.charged,
        })?,
    };
    let key = envelope.key().clone();
    let receipt = EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            source,
            ReceiptTransition {
                envelope: key.clone(),
                ordinal,
            },
        )?,
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: audit.remaining_qty,
        qty_after: remaining_qty_after,
        value_before: audit.filled_value,
        value_after,
        delta: transition.delta,
        nominal: transition.nominal,
        charged: transition.charged,
        charged_before: audit.charged,
        charged_after: transition.charged_after,
        deliver_qty: transition.deliver_qty,
        deliver_cash: transition.deliver_cash,
    };
    let filled_qty = audit
        .filled_qty
        .checked_add(fill_qty)
        .ok_or_else(|| invariant("累计成交股数溢出"))?;
    envelope.apply(
        transition.delta,
        EnvelopeAudit {
            limit: audit.limit,
            remaining_qty: remaining_qty_after,
            filled_qty,
            filled_value: value_after,
            nominal: transition.nominal_after,
            charged: transition.charged_after,
        },
        false,
    )?;
    Ok(receipt)
}

pub(super) fn boundary_receipt(
    envelope: &Envelope,
    source: ReceiptSource,
    ordinal: u64,
    kind: ReceiptKind,
) -> Result<EnvelopeReceipt, StepFatal> {
    if !matches!(kind, ReceiptKind::Rollover | ReceiptKind::Release) {
        return Err(invariant("边界收据只能承接或释放剩余委托"));
    }
    let audit = envelope.audit();
    let key = envelope.key().clone();
    let (released, live_after) = if kind == ReceiptKind::Rollover {
        (ResVec::ZERO, envelope.live())
    } else {
        (envelope.live(), ResVec::ZERO)
    };
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            source,
            ReceiptTransition {
                envelope: key.clone(),
                ordinal,
            },
        )?,
        envelope: key,
        kind,
        qty_before: audit.remaining_qty,
        qty_after: audit.remaining_qty,
        value_before: audit.filled_value,
        value_after: audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, released, live_after),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: audit.charged,
        charged_after: audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

pub(super) fn day_end_release_receipt(
    envelope: &Envelope,
    source: u32,
) -> Result<EnvelopeReceipt, StepFatal> {
    boundary_receipt(
        envelope,
        ReceiptSource::DayEnd(source),
        0,
        ReceiptKind::Release,
    )
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::order_receipts".to_owned(),
    }
}
