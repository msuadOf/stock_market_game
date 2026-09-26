use super::{
    p4_continuous::{ContinuousEnvelopeSnapshot, ContinuousStockInput},
    EnvelopeKey, EnvelopeOrigin, StepFatal,
};
use crate::{GameSession, Market, StockCode, TradingPhase};
use rayon::prelude::*;
use std::collections::BTreeSet;

/// Validate and prepare each independent continuous book on the Rayon pool.
/// The session stays read-only until every stock input has been detached.
pub(super) fn prepare_incremental_continuous_inputs(
    session: &GameSession,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    if !matches!(
        session.phase(),
        TradingPhase::Continuous | TradingPhase::PreOpen
    ) {
        return Err(invariant(
            "continuous-book stock inputs require Continuous or PreOpen phase",
        ));
    }
    if session
        .auction_orders
        .values()
        .any(|orders| !orders.is_empty())
    {
        return Err(invariant(
            "continuous-book phase contains residual auction orders",
        ));
    }

    let mut prepared = session
        .markets
        .par_iter()
        .map(|(code, market)| (code.clone(), prepare_stock_input(session, code, market)))
        .collect::<Vec<_>>();
    // Error selection and the returned layout are stable; neither ranks orders.
    prepared.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let mut inputs = Vec::with_capacity(prepared.len());
    let mut matched = 0_usize;
    for (_, result) in prepared {
        let (count, input) = result?;
        matched = matched
            .checked_add(count)
            .ok_or_else(|| invariant("continuous live order count overflow"))?;
        inputs.push(input);
    }
    if matched != session.envelope_ledger.iter().count() {
        for (_, envelope) in session.envelope_ledger.iter() {
            envelope.validate()?;
            if envelope.origin() != EnvelopeOrigin::TickStart {
                return Err(invariant(
                    "post-P0 live ledger contains an envelope whose origin is not TickStart",
                ));
            }
        }
        return Err(invariant(
            "post-P0 envelope ledger contains no matching live order books",
        ));
    }
    Ok(inputs)
}

fn prepare_stock_input(
    session: &GameSession,
    code: &StockCode,
    market: &Market,
) -> Result<(usize, ContinuousStockInput), StepFatal> {
    if market.code() != code {
        return Err(invariant(
            "market map key disagrees with the market stock code",
        ));
    }
    let mut envelopes = Vec::with_capacity(market.resting_order_count());
    let mut matched_keys = BTreeSet::new();
    for order in market.resting_order_refs() {
        let key = EnvelopeKey {
            account: order.owner,
            stock: code.clone(),
            order: order.id,
            side: order.side,
        };
        if !matched_keys.insert(key.clone()) {
            return Err(invariant(
                "continuous order books repeat an envelope identity",
            ));
        }
        let envelope = session
            .envelope_ledger
            .get(&key)
            .map_err(|_| invariant("continuous order books contain an unledgered order"))?;
        envelope.validate()?;
        if envelope.origin() != EnvelopeOrigin::TickStart {
            return Err(invariant(
                "post-P0 live ledger contains an envelope whose origin is not TickStart",
            ));
        }
        let projected = session.project_continuous_envelope(code, order)?;
        let audit = envelope.audit();
        if audit.limit != order.price
            || audit.remaining_qty != order.qty
            || audit.filled_qty != order.filled_qty
            || audit.filled_value != order.filled_value
        {
            return Err(invariant(
                "continuous order books disagree with envelope audit",
            ));
        }
        if envelope.live() != projected.live() {
            return Err(invariant(
                "continuous order books disagree with live envelope resources",
            ));
        }
        envelopes.push(ContinuousEnvelopeSnapshot {
            audit,
            envelope: envelope.clone(),
        });
    }
    envelopes.sort_unstable_by(|left, right| left.envelope.key().cmp(right.envelope.key()));
    let count = envelopes.len();
    Ok((
        count,
        ContinuousStockInput {
            phase: session.phase(),
            market: market.clone(),
            envelopes,
            operations: Vec::new(),
            config: session.setup.config.clone(),
        },
    ))
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p4_continuous_adapter".to_owned(),
    }
}
