use super::{
    Envelope, EnvelopeAudit, EnvelopeReceipt, FeeComponents, ReceiptKind, ReceiptSource, ResVec,
    StepFatal,
};
use crate::{Money, MoneyError, Side};

pub(super) fn validate(
    receipt: &EnvelopeReceipt,
    audit: EnvelopeAudit,
    envelope: Option<&Envelope>,
) -> Result<(), StepFatal> {
    validate_kind(receipt)?;
    validate_audit(receipt, audit)?;
    validate_resources(
        receipt,
        envelope.ok_or_else(|| invariant("unknown envelope"))?,
    )
}

fn validate_kind(receipt: &EnvelopeReceipt) -> Result<(), StepFatal> {
    let valid = matches!(
        (receipt.local_key.source(), receipt.kind),
        (ReceiptSource::P0Expiry(_), ReceiptKind::Release)
            | (
                ReceiptSource::SealedIntent(_),
                ReceiptKind::Fill | ReceiptKind::Release | ReceiptKind::Reject
            )
            | (
                ReceiptSource::Auction(_),
                ReceiptKind::Fill | ReceiptKind::Rollover
            )
            | (ReceiptSource::DayEnd(_), ReceiptKind::Release)
    );
    if valid {
        Ok(())
    } else {
        Err(invariant("receipt kind is illegal for journal/source"))
    }
}

fn validate_audit(receipt: &EnvelopeReceipt, audit: EnvelopeAudit) -> Result<(), StepFatal> {
    if receipt.qty_before != audit.remaining_qty
        || receipt.value_before != audit.filled_value
        || receipt.charged_before != audit.charged
    {
        return Err(invariant(
            "receipt before fields disagree with envelope audit",
        ));
    }
    if receipt.qty_after > receipt.qty_before || receipt.value_after < receipt.value_before {
        return Err(invariant("receipt cumulative chain regressed"));
    }
    for fee in [
        receipt.nominal,
        receipt.charged,
        receipt.charged_before,
        receipt.charged_after,
    ] {
        validate_fee(fee)?;
    }
    if receipt.kind == ReceiptKind::Fill
        && (receipt.qty_after == receipt.qty_before || receipt.value_after == receipt.value_before)
    {
        return Err(invariant("fill receipt has no cumulative movement"));
    }
    if receipt.kind != ReceiptKind::Fill
        && (receipt.qty_after != receipt.qty_before || receipt.value_after != receipt.value_before)
    {
        return Err(invariant("non-fill receipt changes cumulative fill audit"));
    }
    let nominal_after = fee_add(audit.nominal, receipt.nominal)?;
    if receipt.charged_after != fee_add(audit.charged, receipt.charged)?
        || receipt.charged_after.total()?
            != receipt
                .charged_before
                .total()?
                .add(receipt.charged.total()?)
                .map_err(money)?
        || exceeds(receipt.charged_after, nominal_after)
    {
        return Err(invariant("charged component chain mismatch"));
    }
    Ok(())
}

fn validate_fee(fee: FeeComponents) -> Result<(), StepFatal> {
    if fee.commission < Money::ZERO || fee.stamp_tax < Money::ZERO || fee.transfer_fee < Money::ZERO
    {
        Err(invariant("receipt fee component is negative"))
    } else {
        Ok(())
    }
}

fn validate_resources(receipt: &EnvelopeReceipt, envelope: &Envelope) -> Result<(), StepFatal> {
    validate_non_fill_transition(receipt)?;
    if is_terminal(receipt) && receipt.delta.live_after != ResVec::ZERO {
        return Err(invariant("terminal receipt leaves live resources"));
    }
    match envelope.key().side {
        Side::Buy => validate_buyer_resources(receipt, envelope),
        Side::Sell
            if envelope.live().cash != Money::ZERO
                || receipt.delta.spent.cash != Money::ZERO
                || receipt.delta.released.cash != Money::ZERO
                || receipt.delta.live_after.cash != Money::ZERO =>
        {
            Err(invariant("seller cash envelope resources must be zero"))
        }
        Side::Sell => validate_seller_delivery(receipt, envelope.audit()),
    }
}

fn validate_non_fill_transition(receipt: &EnvelopeReceipt) -> Result<(), StepFatal> {
    if receipt.kind == ReceiptKind::Fill {
        return Ok(());
    }
    if receipt.delta.spent != ResVec::ZERO
        || receipt.nominal != FeeComponents::ZERO
        || receipt.charged != FeeComponents::ZERO
    {
        return Err(invariant(
            "non-fill receipt spends resources or accrues fees",
        ));
    }
    if receipt.kind == ReceiptKind::Rollover && receipt.delta.released != ResVec::ZERO {
        return Err(invariant("rollover receipt releases live resources"));
    }
    Ok(())
}

fn is_terminal(receipt: &EnvelopeReceipt) -> bool {
    matches!(receipt.kind, ReceiptKind::Release | ReceiptKind::Reject)
        || (receipt.kind == ReceiptKind::Fill && receipt.qty_after == 0)
}

fn validate_buyer_resources(
    receipt: &EnvelopeReceipt,
    envelope: &Envelope,
) -> Result<(), StepFatal> {
    if envelope.live().shares != 0
        || receipt.delta.spent.shares != 0
        || receipt.delta.released.shares != 0
        || receipt.delta.live_after.shares != 0
    {
        return Err(invariant("buyer share envelope resources must be zero"));
    }
    let filled = receipt
        .qty_before
        .checked_sub(receipt.qty_after)
        .ok_or_else(|| invariant("buyer quantity chain regressed"))?;
    match receipt.kind {
        ReceiptKind::Fill => {
            let gross = receipt
                .value_after
                .sub(receipt.value_before)
                .map_err(money)?;
            let expected_spent = gross.add(receipt.charged.total()?).map_err(money)?;
            if receipt.deliver_qty != filled
                || receipt.deliver_cash != Money::ZERO
                || receipt.delta.spent.cash != expected_spent
            {
                Err(invariant("buyer fill resource or delivery mismatch"))
            } else {
                Ok(())
            }
        }
        ReceiptKind::Release | ReceiptKind::Reject | ReceiptKind::Rollover
            if receipt.deliver_qty == 0 && receipt.deliver_cash == Money::ZERO =>
        {
            Ok(())
        }
        ReceiptKind::Release | ReceiptKind::Reject | ReceiptKind::Rollover => {
            Err(invariant("non-fill buyer receipt has delivery"))
        }
    }
}

fn validate_seller_delivery(
    receipt: &EnvelopeReceipt,
    audit: EnvelopeAudit,
) -> Result<(), StepFatal> {
    let filled = receipt
        .qty_before
        .checked_sub(receipt.qty_after)
        .ok_or_else(|| invariant("seller quantity chain regressed"))?;
    match receipt.kind {
        ReceiptKind::Fill => {
            let gross = receipt
                .value_after
                .sub(receipt.value_before)
                .map_err(money)?;
            validate_seller_fill_charge(receipt, audit, gross)?;
            if receipt.deliver_qty != 0
                || receipt.delta.spent.shares != filled
                || receipt.delta.released.shares != 0
                || receipt.delta.live_after.shares != receipt.qty_after
                || receipt.deliver_cash < Money::ZERO
                || receipt.charged.total()? > gross
                || receipt
                    .deliver_cash
                    .add(receipt.charged.total()?)
                    .map_err(money)?
                    != gross
            {
                Err(invariant("seller fill resource or delivery mismatch"))
            } else {
                Ok(())
            }
        }
        ReceiptKind::Release | ReceiptKind::Reject | ReceiptKind::Rollover
            if receipt.deliver_qty == 0 && receipt.deliver_cash == Money::ZERO =>
        {
            Ok(())
        }
        ReceiptKind::Release | ReceiptKind::Reject | ReceiptKind::Rollover => {
            Err(invariant("non-fill seller receipt has delivery"))
        }
    }
}

fn validate_seller_fill_charge(
    receipt: &EnvelopeReceipt,
    audit: EnvelopeAudit,
    gross: Money,
) -> Result<(), StepFatal> {
    if exceeds(receipt.charged_before, audit.nominal) {
        return Err(invariant("seller charged history exceeds nominal history"));
    }
    let charged_before_total = receipt.charged_before.total()?;
    if charged_before_total > receipt.value_before {
        return Err(invariant(
            "seller charged history exceeds cumulative gross value",
        ));
    }

    let nominal_after = fee_add(audit.nominal, receipt.nominal)?;
    let unpaid = fee_sub(nominal_after, receipt.charged_before)?;
    let unpaid_total = nominal_after
        .total()?
        .sub(charged_before_total)
        .map_err(money)?;
    let charge_cap = unpaid_total.min(gross);
    let expected = allocate_seller_charge(unpaid, charge_cap)?;
    if receipt.charged != expected {
        return Err(invariant(
            "seller charged delta violates cap or component priority",
        ));
    }
    Ok(())
}

fn fee_sub(left: FeeComponents, right: FeeComponents) -> Result<FeeComponents, StepFatal> {
    if exceeds(right, left) {
        return Err(invariant("fee component subtraction underflow"));
    }
    Ok(FeeComponents {
        commission: left.commission.sub(right.commission).map_err(money)?,
        stamp_tax: left.stamp_tax.sub(right.stamp_tax).map_err(money)?,
        transfer_fee: left.transfer_fee.sub(right.transfer_fee).map_err(money)?,
    })
}

fn allocate_seller_charge(
    unpaid: FeeComponents,
    charge_cap: Money,
) -> Result<FeeComponents, StepFatal> {
    let commission = unpaid.commission.min(charge_cap);
    let after_commission = charge_cap.sub(commission).map_err(money)?;
    let stamp_tax = unpaid.stamp_tax.min(after_commission);
    let after_stamp_tax = after_commission.sub(stamp_tax).map_err(money)?;
    let transfer_fee = unpaid.transfer_fee.min(after_stamp_tax);
    let allocated = FeeComponents {
        commission,
        stamp_tax,
        transfer_fee,
    };
    if allocated.total()? != charge_cap {
        return Err(invariant("seller charge allocation did not exhaust cap"));
    }
    Ok(allocated)
}

pub(super) fn next_audit(
    receipt: &EnvelopeReceipt,
    audit: EnvelopeAudit,
) -> Result<EnvelopeAudit, StepFatal> {
    let filled_qty = audit
        .filled_qty
        .checked_add(
            receipt
                .qty_before
                .checked_sub(receipt.qty_after)
                .ok_or_else(|| invariant("quantity chain regressed"))?,
        )
        .ok_or_else(|| invariant("filled quantity overflow"))?;
    Ok(EnvelopeAudit {
        limit: audit.limit,
        remaining_qty: receipt.qty_after,
        filled_qty,
        filled_value: receipt.value_after,
        nominal: fee_add(audit.nominal, receipt.nominal)?,
        charged: receipt.charged_after,
    })
}

fn fee_add(left: FeeComponents, right: FeeComponents) -> Result<FeeComponents, StepFatal> {
    Ok(FeeComponents {
        commission: left.commission.add(right.commission).map_err(money)?,
        stamp_tax: left.stamp_tax.add(right.stamp_tax).map_err(money)?,
        transfer_fee: left.transfer_fee.add(right.transfer_fee).map_err(money)?,
    })
}
fn exceeds(charged: FeeComponents, nominal: FeeComponents) -> bool {
    charged.commission > nominal.commission
        || charged.stamp_tax > nominal.stamp_tax
        || charged.transfer_fee > nominal.transfer_fee
}
fn money(error: MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::EnvelopeLedger".to_owned(),
    }
}
pub(super) fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::EnvelopeLedger".to_owned(),
    }
}
