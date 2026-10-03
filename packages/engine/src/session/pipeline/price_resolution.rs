//! One-time symbolic limit resolution at the stock's actual admission position.

use super::{
    Envelope, EnvelopeDraft, EnvelopeReceipt, FeeComponents, JournalRank, ReceiptDelta,
    ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition, ResVec, StepFatal,
};
use crate::{session::buy_order_reservation, GameConfig, Market, Money, Side};

pub(super) fn resolve_draft(
    draft: &EnvelopeDraft,
    envelope: &Envelope,
    market: &Market,
    config: &GameConfig,
    apply_price_cage: bool,
) -> Result<(Money, Option<EnvelopeReceipt>), StepFatal> {
    let Some(requested) = draft.requested_price() else {
        return Ok((draft.limit(), None));
    };
    if envelope.pending_price() != Some(requested) || envelope.audit().limit != draft.limit() {
        return Err(invariant(
            "AccountValidation draft 与 pending envelope 的价格不一致",
        ));
    }
    let price = market
        .resolve_limit_price(draft.side(), requested, apply_price_cage)
        .map_err(|error| invariant(&error.to_string()))?;
    let released = match draft.side() {
        Side::Buy => {
            let required = buy_order_reservation(config, price, draft.qty(), Money::ZERO)
                .map_err(|error| invariant(&error.to_string()))?;
            ResVec::new(
                envelope
                    .live()
                    .cash
                    .sub(required)
                    .map_err(|error| invariant(&error.to_string()))?,
                0,
            )
        }
        Side::Sell => ResVec::ZERO,
    };
    if released.cash < Money::ZERO {
        return Err(invariant("解析价格超出 AccountValidation 现金预留"));
    }
    let live_after = envelope.live().checked_sub(released)?;
    let audit = envelope.audit();
    let key = envelope.key().clone();
    let receipt = EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(draft.sealed_index()),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )?,
        envelope: key,
        kind: ReceiptKind::PriceResolved {
            requested,
            before: audit.limit,
            after: price,
        },
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
    };
    Ok((price, Some(receipt)))
}

pub(super) fn apply_to_auction_order(
    envelope: &mut Envelope,
    receipt: &EnvelopeReceipt,
) -> Result<(), StepFatal> {
    let audit_after = super::ledger_validation::next_audit(receipt, envelope.audit())?;
    envelope.apply(receipt.delta, audit_after, true)
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::price_resolution".to_owned(),
    }
}
