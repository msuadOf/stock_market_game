use super::{
    Envelope, EnvelopeKey, EnvelopeOrigin, FeeComponents, P3CandidateResult, P3ValidatedOperation,
    P3ValidationOutput, ResVec, StepFatal,
    p3_p4_normalizer::P3P4NormalizedOperations,
    stock_auction::{
        AuctionCompletionInput, AuctionOperation, AuctionOrder, AuctionPhase, StockAuctionState,
    },
};
use crate::orderbook::js_safe_u64;
use crate::{GameSession, Market, OrderId, RejectionReason, StockCode, TradingPhase};
use std::collections::BTreeMap;

/// Read-only, stock-owned auction worker input assembled at the P3/P4 boundary.
/// Operations remain unmodified so their global sealed indices and place kinds
/// survive partitioning. A later worker applies them before completing auction.
#[derive(Debug)]
pub(super) struct AuctionStockInput {
    pub(super) code: StockCode,
    pub(super) market: Market,
    pub(super) completion: AuctionCompletionInput,
    pub(super) continuous_envelopes: Vec<Envelope>,
    pub(super) operations: Vec<P3ValidatedOperation>,
}

pub(super) fn adapt_auction_stock_inputs(
    session: &GameSession,
    validation: &P3ValidationOutput,
    normalized: &P3P4NormalizedOperations,
) -> Result<Vec<AuctionStockInput>, StepFatal> {
    let mut inputs = prepare_incremental_auction_inputs(session)?;
    validate_operations(validation)?;
    validate_normalized_handoff(session, validation, normalized)?;

    let mut operations = inputs
        .iter()
        .map(|input| (input.code.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for operation in normalized.operations() {
        let code = operation_code(operation);
        operations
            .get_mut(code)
            .ok_or_else(|| invariant(&format!("P3 operation references unknown stock {}", code.0)))?
            .push(operation.clone());
    }
    for input in &mut inputs {
        input.operations = operations
            .remove(&input.code)
            .ok_or_else(|| invariant("auction operation partition is missing a stock"))?;
    }
    if !operations.is_empty() {
        return Err(invariant(
            "auction operation partition retained an unknown stock",
        ));
    }
    if validation.next_order_id_after() > js_safe_u64::MAX {
        return Err(invariant(
            "P3 next order id exceeds the serializable authoritative boundary",
        ));
    }
    Ok(inputs)
}

/// Builds the operation-free post-P0 stock shadows used by incremental auction P4.
///
/// Every subsequent round must reuse these owned inputs. Rebuilding them from a session that has
/// already received private continuation projections would lose tick-start envelope identity.
pub(in crate::session::pipeline) fn prepare_incremental_auction_inputs(
    session: &GameSession,
) -> Result<Vec<AuctionStockInput>, StepFatal> {
    let phase = auction_phase(session)?;
    let specs = validate_market_and_spec_identity(session)?;
    let envelopes = validate_live_orders(session, phase)?;

    let mut inputs = Vec::with_capacity(session.markets.len());
    for (code, market) in &session.markets {
        let spec = specs
            .get(code)
            .ok_or_else(|| invariant("market has no canonical stock specification"))?;
        let mut state = StockAuctionState::new(code.clone());
        if let Some(orders) = session.auction_orders.get(code) {
            validate_arrival_order(orders)?;
            for order in orders {
                validate_serializable_arrival(order.arrival_seq)?;
                let key = EnvelopeKey {
                    account: order.owner,
                    stock: code.clone(),
                    order: OrderId(order.arrival_seq),
                    side: order.side,
                };
                let envelope = envelopes
                    .get(&key)
                    .ok_or_else(|| invariant("auction order is unledgered after validation"))?
                    .clone();
                let output = state.apply_operation(
                    phase,
                    AuctionOperation::Place(AuctionOrder {
                        envelope,
                        arrival_seq: order.arrival_seq,
                    }),
                )?;
                if output.receipt.is_some() || output.terminal_key.is_some() {
                    return Err(invariant(
                        "seeding a tick-start auction order produced a transition",
                    ));
                }
            }
        }
        let mut continuous_envelopes = market
            .resting_orders()
            .into_iter()
            .map(|order| {
                let key = EnvelopeKey {
                    account: order.owner,
                    stock: code.clone(),
                    order: order.id,
                    side: order.side,
                };
                envelopes
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| invariant("continuous order is missing its validated envelope"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        continuous_envelopes.sort_by(|left, right| left.key().cmp(right.key()));
        inputs.push(AuctionStockInput {
            code: code.clone(),
            market: market.clone(),
            completion: AuctionCompletionInput {
                state,
                phase,
                previous_close: market.last_close(),
                exchange: spec.exchange,
                price_tick: spec.tick,
                config: session.setup.config.clone(),
                day_end_envelopes: Vec::new(),
            },
            continuous_envelopes,
            operations: Vec::new(),
        });
    }
    Ok(inputs)
}

fn auction_phase(session: &GameSession) -> Result<AuctionPhase, StepFatal> {
    match session.phase() {
        TradingPhase::CallAuction => Ok(AuctionPhase::Opening {
            elapsed_ticks: session.tick() % session.setup.ticks_per_day,
            cancelable_ticks: session.setup.auction_ticks / 3,
        }),
        TradingPhase::ClosingAuction => Ok(AuctionPhase::Closing),
        TradingPhase::PreOpen | TradingPhase::Continuous => Err(invariant(
            "auction stock inputs require an opening or closing auction phase",
        )),
    }
}

fn validate_market_and_spec_identity(
    session: &GameSession,
) -> Result<BTreeMap<StockCode, &crate::StockSpec>, StepFatal> {
    let specs = session
        .setup
        .stocks
        .iter()
        .map(|spec| (spec.code.clone(), spec))
        .collect::<BTreeMap<_, _>>();
    if specs.len() != session.setup.stocks.len() {
        return Err(invariant("stock specifications contain duplicate codes"));
    }
    for (code, market) in &session.markets {
        if market.code() != code {
            return Err(invariant(
                "market map key disagrees with the market stock code",
            ));
        }
        if !specs.contains_key(code) {
            return Err(invariant(
                "market references an unknown stock specification",
            ));
        }
    }
    if specs.len() != session.markets.len() {
        return Err(invariant(
            "market and stock specification code sets disagree",
        ));
    }
    for code in session.auction_orders.keys() {
        if !session.markets.contains_key(code) {
            return Err(invariant(&format!(
                "auction queue references unknown stock {}",
                code.0
            )));
        }
    }
    Ok(specs)
}

fn validate_operations(validation: &P3ValidationOutput) -> Result<(), StepFatal> {
    if validation
        .operations()
        .windows(2)
        .any(|pair| pair[0].sealed_index() >= pair[1].sealed_index())
    {
        return Err(invariant(
            "P3 operations are not in strict global sealed_index order",
        ));
    }
    let accepted = validation
        .results()
        .iter()
        .filter_map(|result| match result {
            P3CandidateResult::Accepted { key, sealed_index } => Some((key, *sealed_index)),
            P3CandidateResult::Rejected { .. } => None,
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
    for operation in validation.operations() {
        validate_operation_order_identity(operation)?;
    }
    Ok(())
}

fn validate_normalized_handoff(
    session: &GameSession,
    validation: &P3ValidationOutput,
    normalized: &P3P4NormalizedOperations,
) -> Result<(), StepFatal> {
    let mut operations = normalized.operations().iter();
    let mut rejections = normalized.rejections().iter();
    for operation in validation.operations() {
        if session.markets.contains_key(operation_code(operation)) {
            if operations.next() != Some(operation) {
                return Err(invariant(
                    "normalized P3 handoff omitted or changed a known-stock operation",
                ));
            }
            continue;
        }
        let P3ValidatedOperation::Cancel {
            candidate_key,
            sealed_index,
            account,
            code,
            order_id,
        } = operation
        else {
            return Err(invariant(
                "accepted place references an unknown stock after normalization",
            ));
        };
        let Some(rejection) = rejections.next() else {
            return Err(invariant(
                "normalized P3 handoff omitted an unknown-stock cancel rejection",
            ));
        };
        if rejection.candidate_key() != candidate_key
            || rejection.sealed_index() != *sealed_index
            || rejection.owner() != *account
            || rejection.code() != code
            || rejection.order_id() != *order_id
            || rejection.reason() != &RejectionReason::UnknownStock
        {
            return Err(invariant(
                "normalized P3 handoff changed an unknown-stock cancel rejection",
            ));
        }
    }
    if operations.next().is_some() || rejections.next().is_some() {
        return Err(invariant(
            "normalized P3 handoff contains an operation or rejection absent from P3",
        ));
    }
    Ok(())
}

fn validate_operation_order_identity(operation: &P3ValidatedOperation) -> Result<(), StepFatal> {
    match operation {
        P3ValidatedOperation::Place(draft) => validate_serializable_arrival(draft.order_id().0),
        P3ValidatedOperation::Cancel { order_id, .. } if order_id.0 > js_safe_u64::MAX => Err(
            invariant("P3 cancel order id exceeds the serializable authoritative boundary"),
        ),
        P3ValidatedOperation::Cancel { .. } => Ok(()),
    }
}

fn validate_arrival_order(orders: &[crate::AuctionOrderSnap]) -> Result<(), StepFatal> {
    if orders
        .windows(2)
        .any(|pair| pair[0].arrival_seq >= pair[1].arrival_seq)
    {
        return Err(invariant(
            "auction queue is not in strict arrival sequence order",
        ));
    }
    Ok(())
}

fn validate_serializable_arrival(arrival_seq: u64) -> Result<(), StepFatal> {
    if arrival_seq > js_safe_u64::MAX {
        return Err(invariant(
            "auction arrival sequence exceeds the serializable authoritative boundary",
        ));
    }
    let next = arrival_seq
        .checked_add(1)
        .ok_or_else(|| invariant("auction next arrival sequence overflow"))?;
    if next > js_safe_u64::MAX {
        return Err(invariant(
            "auction next arrival sequence exceeds the serializable authoritative boundary",
        ));
    }
    Ok(())
}

fn validate_live_orders(
    session: &GameSession,
    phase: AuctionPhase,
) -> Result<BTreeMap<EnvelopeKey, Envelope>, StepFatal> {
    validate_ledger_evidence(session)?;
    let mut authoritative = BTreeMap::new();
    for (key, envelope) in session.envelope_ledger.iter() {
        if authoritative
            .insert(key.clone(), envelope.clone())
            .is_some()
        {
            return Err(invariant("post-P0 ledger contains duplicate envelope keys"));
        }
    }
    let mut unmatched = authoritative.clone();
    let projected = session
        .project_live_envelopes()
        .map_err(|error| invariant(&format!("live envelope projection failed: {error}")))?;
    let mut expected = projected
        .into_iter()
        .map(|envelope| (envelope.key().clone(), envelope))
        .collect::<BTreeMap<_, _>>();

    for (code, market) in &session.markets {
        let resting = market.resting_orders();
        if matches!(phase, AuctionPhase::Opening { .. }) && !resting.is_empty() {
            return Err(invariant(
                "opening auction contains continuous order-book residue",
            ));
        }
        for order in resting {
            let key = EnvelopeKey {
                account: order.owner,
                stock: code.clone(),
                order: order.id,
                side: order.side,
            };
            let envelope = remove_live(&mut unmatched, &mut expected, &key, "continuous order")?;
            validate_continuous_audit(&envelope, &order)?;
        }
    }
    for (code, orders) in &session.auction_orders {
        validate_arrival_order(orders)?;
        for order in orders {
            validate_serializable_arrival(order.arrival_seq)?;
            let key = EnvelopeKey {
                account: order.owner,
                stock: code.clone(),
                order: OrderId(order.arrival_seq),
                side: order.side,
            };
            let envelope = remove_live(&mut unmatched, &mut expected, &key, "auction order")?;
            validate_auction_audit(&envelope, order)?;
        }
    }
    if !unmatched.is_empty() || !expected.is_empty() {
        return Err(invariant(
            "post-P0 envelope ledger contains no matching live order",
        ));
    }
    Ok(authoritative)
}

fn validate_ledger_evidence(session: &GameSession) -> Result<(), StepFatal> {
    let ledger = &session.envelope_ledger;
    ledger.validate_conservation().map_err(|error| {
        invariant(&format!(
            "post-P0 envelope ledger conservation is invalid: {error}"
        ))
    })?;
    let expected_rows = ledger
        .envelopes
        .len()
        .checked_add(ledger.terminal_envelopes.len())
        .ok_or_else(|| invariant("post-P0 envelope ledger row count overflow"))?;
    if ledger.audits.len() != expected_rows || ledger.conservation.len() != expected_rows {
        return Err(invariant(
            "post-P0 envelope ledger evidence rows do not match envelope rows",
        ));
    }
    for (key, envelope) in ledger
        .envelopes
        .iter()
        .chain(ledger.terminal_envelopes.iter())
    {
        envelope.validate().map_err(|error| {
            invariant(&format!(
                "post-P0 envelope ledger contains an invalid envelope: {error}"
            ))
        })?;
        if envelope.origin() != EnvelopeOrigin::TickStart {
            return Err(invariant(
                "post-P0 auction input contains a same-tick envelope",
            ));
        }
        if ledger.envelopes.contains_key(key) && ledger.terminal_envelopes.contains_key(key) {
            return Err(invariant("post-P0 live and terminal envelope rows overlap"));
        }
        if ledger.audits.get(key).copied() != Some(envelope.audit()) {
            return Err(invariant(
                "post-P0 envelope audit row disagrees with envelope",
            ));
        }
        if !ledger.conservation.contains_key(key) {
            return Err(invariant(
                "post-P0 envelope has no conservation evidence row",
            ));
        }
    }
    if ledger
        .envelopes
        .values()
        .any(|envelope| envelope.live() == ResVec::ZERO)
    {
        return Err(invariant("post-P0 live envelope row has no live resources"));
    }
    if ledger
        .terminal_envelopes
        .values()
        .any(|envelope| envelope.live() != ResVec::ZERO)
    {
        return Err(invariant(
            "post-P0 terminal envelope row still has live resources",
        ));
    }
    Ok(())
}

fn remove_live(
    unmatched: &mut BTreeMap<EnvelopeKey, Envelope>,
    expected: &mut BTreeMap<EnvelopeKey, Envelope>,
    key: &EnvelopeKey,
    label: &str,
) -> Result<Envelope, StepFatal> {
    let envelope = unmatched
        .remove(key)
        .ok_or_else(|| invariant(&format!("{label} is unledgered")))?;
    let projected = expected
        .remove(key)
        .ok_or_else(|| invariant(&format!("{label} has no resource projection")))?;
    if envelope.live() != projected.live() {
        return Err(invariant(&format!(
            "{label} disagrees with live envelope resources"
        )));
    }
    Ok(envelope)
}

fn validate_continuous_audit(envelope: &Envelope, order: &crate::Order) -> Result<(), StepFatal> {
    let audit = envelope.audit();
    if audit.limit != order.price
        || audit.remaining_qty != order.qty
        || audit.filled_qty != order.filled_qty
        || audit.filled_value != order.filled_value
    {
        return Err(invariant(
            "continuous order book disagrees with envelope audit",
        ));
    }
    Ok(())
}

fn validate_auction_audit(
    envelope: &Envelope,
    order: &crate::AuctionOrderSnap,
) -> Result<(), StepFatal> {
    let audit = envelope.audit();
    if audit.limit != order.limit
        || audit.remaining_qty != order.qty
        || audit.filled_qty != 0
        || audit.filled_value != crate::Money::ZERO
        || audit.nominal != FeeComponents::ZERO
        || audit.charged != FeeComponents::ZERO
    {
        return Err(invariant("auction queue disagrees with envelope audit"));
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
        location: "pipeline::stock_auction_adapter".to_owned(),
    }
}
