//! Read-only mirror of the authoritative order router's tick, band, cage, and quantity guards.

use super::{QuoteDecisionInputs, QuoteError};
use crate::orderbook::Side;
use crate::Money;

pub(super) fn validate_guardrails(inputs: &QuoteDecisionInputs) -> Result<(), QuoteError> {
    for (field, value) in [
        ("tick", inputs.tick),
        ("band_down", inputs.band_down),
        ("band_up", inputs.band_up),
        ("protection_limit", inputs.protection_limit),
    ] {
        if value.cents() <= 0 {
            return Err(QuoteError::InvalidGuardrail {
                field,
                cents: value.cents(),
            });
        }
    }
    if inputs.band_down > inputs.band_up || inputs.lot_size == 0 || inputs.max_order_qty == 0 {
        return Err(QuoteError::InvalidGuardrail {
            field: "range_or_quantity",
            cents: 0,
        });
    }
    Ok(())
}

pub(super) fn validate_routed_quote(
    inputs: &QuoteDecisionInputs,
    price: Money,
) -> Result<(), QuoteError> {
    if price.cents() <= 0 || price.cents() % inputs.tick.cents() != 0 {
        return Err(QuoteError::InvalidTick {
            price,
            tick: inputs.tick,
        });
    }
    if price < inputs.band_down || price > inputs.band_up {
        return Err(QuoteError::OutsidePriceBand {
            price,
            band_down: inputs.band_down,
            band_up: inputs.band_up,
        });
    }
    if let Some(bound) = inputs.cage_bound {
        let outside = match inputs.side {
            Side::Buy => price > bound,
            Side::Sell => price < bound,
        };
        if outside {
            return Err(QuoteError::OutsidePriceCage {
                side: inputs.side,
                price,
                bound,
            });
        }
    }
    let valid_qty = inputs.desired_qty > 0
        && inputs.desired_qty <= inputs.max_order_qty
        && match inputs.side {
            Side::Buy => inputs.desired_qty.is_multiple_of(inputs.lot_size),
            Side::Sell => {
                inputs.desired_qty <= inputs.available_sell_qty
                    && (inputs.desired_qty.is_multiple_of(inputs.lot_size)
                        || inputs.desired_qty % inputs.lot_size
                            == inputs.available_sell_qty % inputs.lot_size)
            }
        };
    if !valid_qty {
        return Err(QuoteError::InvalidQuantity {
            qty: inputs.desired_qty,
        });
    }
    Ok(())
}
