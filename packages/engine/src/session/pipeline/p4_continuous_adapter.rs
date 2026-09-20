use super::{
    p4_continuous::{ContinuousEnvelopeSnapshot, ContinuousStockInput},
    Envelope, EnvelopeKey, EnvelopeOrigin, P3ValidatedOperation, P3ValidationOutput, StepFatal,
};
use crate::{GameSession, StockCode, TradingPhase};
use std::collections::BTreeMap;

/// Builds read-only, stock-owned P4 inputs from the post-P0 shadow state and the complete P3
/// output. This adapter does not run a worker or apply any worker result.
pub(super) fn adapt_continuous_stock_inputs(
    session: &GameSession,
    validation: &P3ValidationOutput,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    validate_operations(validation)?;
    adapt_continuous_stock_inputs_from_operations(session, validation.operations())
}

/// Captures every post-P0 stock shadow exactly once before any adaptive P3/P4 route runs.
///
/// The returned inputs intentionally contain no operations. Callers must construct one
/// `IncrementalContinuousStockCoordinator` from this full set, then feed only newly accepted P3
/// operations to `apply_round`; rebuilding inputs between routes would discard same-tick P4 state.
pub(super) fn prepare_incremental_continuous_inputs(
    session: &GameSession,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    let inputs = adapt_continuous_stock_inputs_from_operations(session, &[])?;
    if inputs.iter().any(|input| !input.operations.is_empty()) {
        return Err(invariant(
            "incremental continuous input preparation produced an operation",
        ));
    }
    Ok(inputs)
}

pub(super) fn adapt_continuous_stock_inputs_from_operations(
    session: &GameSession,
    validated_operations: &[P3ValidatedOperation],
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    if session.phase() != TradingPhase::Continuous {
        return Err(invariant(
            "continuous stock inputs require the Continuous trading phase",
        ));
    }
    if session
        .auction_orders
        .values()
        .any(|orders| !orders.is_empty())
    {
        return Err(invariant(
            "Continuous phase contains residual auction orders",
        ));
    }
    validate_market_identity(session)?;
    validate_operation_order(validated_operations)?;

    let ledger = live_ledger(session)?;
    validate_live_order_keys(session, &ledger)?;

    let mut operations = session
        .markets
        .keys()
        .cloned()
        .map(|code| (code, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for operation in validated_operations {
        let code = operation_code(operation);
        operations
            .get_mut(code)
            .ok_or_else(|| invariant(&format!("P3 operation references unknown stock {}", code.0)))?
            .push(operation.clone());
    }

    let by_key = ledger
        .iter()
        .map(|envelope| (envelope.key().clone(), envelope.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut inputs = Vec::with_capacity(session.markets.len());
    for (code, market) in &session.markets {
        let mut envelopes = Vec::with_capacity(market.resting_order_count());
        for order in market.resting_orders() {
            let key = EnvelopeKey {
                account: order.owner,
                stock: code.clone(),
                order: order.id,
                side: order.side,
            };
            let envelope = by_key
                .get(&key)
                .ok_or_else(|| invariant("continuous order has no matching live envelope"))?
                .clone();
            envelopes.push(ContinuousEnvelopeSnapshot {
                audit: envelope.audit(),
                envelope,
            });
        }
        envelopes.sort_by(|left, right| left.envelope.key().cmp(right.envelope.key()));
        inputs.push(ContinuousStockInput {
            phase: session.phase(),
            market: market.clone(),
            envelopes,
            operations: operations
                .remove(code)
                .ok_or_else(|| invariant("continuous operation partition is missing a stock"))?,
            config: session.setup.config.clone(),
        });
    }
    Ok(inputs)
}

fn validate_operation_order(operations: &[P3ValidatedOperation]) -> Result<(), StepFatal> {
    if operations
        .windows(2)
        .any(|pair| pair[0].sealed_index() >= pair[1].sealed_index())
    {
        return Err(invariant(
            "P3 operations are not in strict global sealed_index order",
        ));
    }
    Ok(())
}

fn validate_market_identity(session: &GameSession) -> Result<(), StepFatal> {
    for (code, market) in &session.markets {
        if market.code() != code {
            return Err(invariant(
                "market map key disagrees with the market stock code",
            ));
        }
    }
    Ok(())
}

fn validate_operations(validation: &P3ValidationOutput) -> Result<(), StepFatal> {
    validate_operation_order(validation.operations())?;

    let accepted = validation
        .results()
        .iter()
        .filter_map(|result| match result {
            super::P3CandidateResult::Accepted { key, sealed_index } => Some((key, *sealed_index)),
            super::P3CandidateResult::Rejected { .. } => None,
        });
    if !accepted.eq(validation
        .operations()
        .iter()
        .map(|operation| (operation.candidate_key(), operation.sealed_index())))
    {
        return Err(invariant(
            "P3 accepted results and validated operations disagree",
        ));
    }
    Ok(())
}

fn live_ledger(session: &GameSession) -> Result<Vec<Envelope>, StepFatal> {
    let mut envelopes = Vec::new();
    for (_, envelope) in session.envelope_ledger.iter() {
        envelope.validate()?;
        if envelope.origin() != EnvelopeOrigin::TickStart {
            return Err(invariant(
                "post-P0 live ledger contains an envelope whose origin is not TickStart",
            ));
        }
        envelopes.push(envelope.clone());
    }
    envelopes.sort_by(|left, right| left.key().cmp(right.key()));
    Ok(envelopes)
}

fn validate_live_order_keys(session: &GameSession, ledger: &[Envelope]) -> Result<(), StepFatal> {
    let mut unmatched = ledger
        .iter()
        .map(|envelope| (envelope.key().clone(), envelope))
        .collect::<BTreeMap<_, _>>();
    let mut expected = session
        .project_live_envelopes()?
        .into_iter()
        .map(|envelope| (envelope.key().clone(), envelope))
        .collect::<BTreeMap<_, _>>();
    for (code, market) in &session.markets {
        for order in market.resting_orders() {
            let key = EnvelopeKey {
                account: order.owner,
                stock: code.clone(),
                order: order.id,
                side: order.side,
            };
            let envelope = unmatched
                .remove(&key)
                .ok_or_else(|| invariant("continuous order books contain an unledgered order"))?;
            let projected = expected
                .remove(&key)
                .ok_or_else(|| invariant("continuous order has no resource projection"))?;
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
        }
    }
    if !unmatched.is_empty() || !expected.is_empty() {
        return Err(invariant(
            "post-P0 envelope ledger contains no matching live order books",
        ));
    }
    Ok(())
}

fn operation_code(operation: &P3ValidatedOperation) -> &StockCode {
    match operation {
        P3ValidatedOperation::Place(draft) => draft.code(),
        P3ValidatedOperation::Cancel { code, .. } => code,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p4_continuous_adapter".to_owned(),
    }
}
