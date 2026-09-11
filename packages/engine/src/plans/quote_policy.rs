//! Pure quote-intent policy. It never mutates an order book or assumes a fill.

use super::urgency::PauseAssessment;
use super::Urgency;
use crate::orderbook::{OrderId, Side};
use crate::Money;

mod validation;
use validation::{validate_guardrails, validate_routed_quote};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookTop {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveQuote {
    pub order_id: OrderId,
    pub price: Money,
    pub qty: u32,
}

/// Router-owned guardrails are passed as values so this module stays pure and cannot bypass routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuoteDecisionInputs {
    pub side: Side,
    pub urgency: Urgency,
    pub pause: PauseAssessment,
    pub book: BookTop,
    pub protection_limit: Money,
    pub band_down: Money,
    pub band_up: Money,
    pub tick: Money,
    /// Current continuous-auction cage bound: buy upper bound or sell lower bound.
    pub cage_bound: Option<Money>,
    pub desired_qty: u32,
    pub lot_size: u32,
    pub max_order_qty: u32,
    pub available_sell_qty: u32,
    pub active_order: Option<ActiveQuote>,
    pub cancellable_now: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuoteAction {
    Wait,
    Keep {
        order_id: OrderId,
    },
    Cancel {
        order_id: OrderId,
    },
    Replace {
        order_id: OrderId,
        price: Money,
        qty: u32,
    },
    Submit {
        price: Money,
        qty: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuoteReason {
    SameSidePatience,
    OppositeQuoteProbe,
    UrgentProtectedLimit,
    ExistingQuoteMatches,
    QueuePriorityLost,
    PauseRequested,
    PendingReconsideration,
    EmptyBook,
    IncompleteBook,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuoteDecision {
    pub action: QuoteAction,
    pub reason: QuoteReason,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum QuoteError {
    #[error("invalid quote guardrail {field} = {cents} cents")]
    InvalidGuardrail { field: &'static str, cents: i64 },
    #[error("quote price {price:?} is not a positive multiple of tick {tick:?}")]
    InvalidTick { price: Money, tick: Money },
    #[error("quote price {price:?} is outside [{band_down:?}, {band_up:?}]")]
    OutsidePriceBand {
        price: Money,
        band_down: Money,
        band_up: Money,
    },
    #[error("quote price {price:?} is outside the {side:?} cage bound {bound:?}")]
    OutsidePriceCage {
        side: Side,
        price: Money,
        bound: Money,
    },
    #[error("invalid routed quote quantity {qty}")]
    InvalidQuantity { qty: u32 },
}

pub fn decide_quote(inputs: &QuoteDecisionInputs) -> Result<QuoteDecision, QuoteError> {
    validate_guardrails(inputs)?;
    if let PauseAssessment::PauseAndRequestCancel(_) = inputs.pause {
        return Ok(match inputs.active_order {
            Some(active) if inputs.cancellable_now => QuoteDecision {
                action: QuoteAction::Cancel {
                    order_id: active.order_id,
                },
                reason: QuoteReason::PauseRequested,
            },
            Some(active) => QuoteDecision {
                action: QuoteAction::Keep {
                    order_id: active.order_id,
                },
                reason: QuoteReason::PendingReconsideration,
            },
            None => QuoteDecision {
                action: QuoteAction::Wait,
                reason: QuoteReason::PauseRequested,
            },
        });
    }

    let proposed = proposed_quote(inputs);
    let Some((price, reason)) = proposed else {
        let reason = if inputs.book.best_bid.is_none() && inputs.book.best_ask.is_none() {
            QuoteReason::EmptyBook
        } else {
            QuoteReason::IncompleteBook
        };
        return Ok(match inputs.active_order {
            Some(active) if inputs.cancellable_now => QuoteDecision {
                action: QuoteAction::Cancel {
                    order_id: active.order_id,
                },
                reason,
            },
            Some(active) => QuoteDecision {
                action: QuoteAction::Keep {
                    order_id: active.order_id,
                },
                reason: QuoteReason::PendingReconsideration,
            },
            None => QuoteDecision {
                action: QuoteAction::Wait,
                reason,
            },
        });
    };
    validate_routed_quote(inputs, price)?;

    Ok(match inputs.active_order {
        Some(active) if active.price == price && active.qty == inputs.desired_qty => {
            QuoteDecision {
                action: QuoteAction::Keep {
                    order_id: active.order_id,
                },
                reason: QuoteReason::ExistingQuoteMatches,
            }
        }
        Some(active) if inputs.cancellable_now => QuoteDecision {
            action: QuoteAction::Replace {
                order_id: active.order_id,
                price,
                qty: inputs.desired_qty,
            },
            reason: QuoteReason::QueuePriorityLost,
        },
        Some(active) => QuoteDecision {
            action: QuoteAction::Keep {
                order_id: active.order_id,
            },
            reason: QuoteReason::PendingReconsideration,
        },
        None => QuoteDecision {
            action: QuoteAction::Submit {
                price,
                qty: inputs.desired_qty,
            },
            reason,
        },
    })
}

fn proposed_quote(inputs: &QuoteDecisionInputs) -> Option<(Money, QuoteReason)> {
    let candidate = match inputs.urgency {
        Urgency::Patient => match inputs.side {
            Side::Buy => inputs.book.best_bid,
            Side::Sell => inputs.book.best_ask,
        }
        .map(|price| (price, QuoteReason::SameSidePatience)),
        Urgency::Normal => match (inputs.book.best_bid, inputs.book.best_ask, inputs.side) {
            (Some(_), Some(ask), Side::Buy) => Some((ask, QuoteReason::OppositeQuoteProbe)),
            (Some(bid), Some(_), Side::Sell) => Some((bid, QuoteReason::OppositeQuoteProbe)),
            (None, _, _) | (_, None, _) => None,
        },
        Urgency::Urgent => Some((inputs.protection_limit, QuoteReason::UrgentProtectedLimit)),
    }?;
    let protected = match inputs.side {
        Side::Buy => candidate.0.min(inputs.protection_limit),
        Side::Sell => candidate.0.max(inputs.protection_limit),
    };
    Some((protected, candidate.1))
}
