use super::{
    stock_auction::{
        AuctionCompletionInput, AuctionOperation, AuctionOrder, AuctionPhase, StockAuctionState,
    },
    Envelope, EnvelopeKey, EnvelopeOrigin, FeeComponents, ResVec, StepFatal, ValidatedOperation,
};
use crate::orderbook::js_safe_u64;
use crate::{GameSession, Market, OrderId, StockCode, TradingPhase};
use std::collections::BTreeMap;

/// 在 AccountValidation/StockProcessing 边界装配股票拥有的只读 Auction worker 输入。
/// operations 不经改写，分片后仍保留全局 sealed index 与 Place kind；
/// worker 先应用 operations，再完成 Auction。
#[derive(Debug)]
pub(super) struct AuctionStockInput {
    pub(super) code: StockCode,
    pub(super) market: Market,
    pub(super) completion: AuctionCompletionInput,
    pub(super) continuous_envelopes: Vec<Envelope>,
    pub(super) operations: Vec<ValidatedOperation>,
}

/// 为增量 Auction 构建不含 operation 的报价过期后逐股 shadow。
///
/// 后续轮次必须重用这些拥有所有权的输入。若从已收到私有 continuation 投影的会话重建，
/// 将丢失 tick 起点 envelope 身份。
pub(in crate::session::pipeline) fn prepare_incremental_auction_inputs(
    session: &GameSession,
) -> Result<Vec<AuctionStockInput>, StepFatal> {
    let phase = auction_phase(session)?;
    let specs = validate_market_and_spec_identity(session)?;
    let envelopes = validate_live_orders(session, phase)?;

    let mut inputs = Vec::with_capacity(session.state.markets.len());
    for (code, market) in &session.state.markets {
        if matches!(session.stock_day_status(code).map_err(|error| invariant(&error.to_string()))?, crate::DayStatus::Closed(_)) {
            if market.resting_order_count() != 0
                || session.state.auction_orders.get(code).is_some_and(|orders| !orders.is_empty()) {
                return Err(invariant("休市证券不能保留活动委托"));
            }
            continue;
        }
        let spec = specs
            .get(code)
            .ok_or_else(|| invariant("market has no canonical stock specification"))?;
        let mut state = StockAuctionState::new(code.clone());
        state.use_committed_fills(market.clone());
        if let Some(orders) = session.state.auction_orders.get(code) {
            for order in orders {
                validate_serializable_order_id(order.order_id)?;
                let key = EnvelopeKey {
                    account: order.owner,
                    stock: code.clone(),
                    order: OrderId(order.order_id),
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
                        arrival_seq: 0,
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
                config: session.state.setup.config.clone(),
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
            elapsed_ticks: session.tick() % session.state.setup.ticks_per_day,
            cancelable_ticks: session.state.setup.auction_ticks / 3,
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
        .state
        .setup
        .stocks
        .iter()
        .map(|spec| (spec.code.clone(), spec))
        .collect::<BTreeMap<_, _>>();
    if specs.len() != session.state.setup.stocks.len() {
        return Err(invariant("stock specifications contain duplicate codes"));
    }
    for (code, market) in &session.state.markets {
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
    if specs.len() != session.state.markets.len() {
        return Err(invariant(
            "market and stock specification code sets disagree",
        ));
    }
    for code in session.state.auction_orders.keys() {
        if !session.state.markets.contains_key(code) {
            return Err(invariant(&format!(
                "auction queue references unknown stock {}",
                code.0
            )));
        }
    }
    Ok(specs)
}

fn validate_serializable_order_id(order_id: u64) -> Result<(), StepFatal> {
    if order_id > js_safe_u64::MAX {
        return Err(invariant(
            "auction order id exceeds the serializable authoritative boundary",
        ));
    }
    let next = order_id
        .checked_add(1)
        .ok_or_else(|| invariant("auction next order id overflow"))?;
    if next > js_safe_u64::MAX {
        return Err(invariant(
            "auction next order id exceeds the serializable authoritative boundary",
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
    for (key, envelope) in session.state.envelope_ledger.iter() {
        if authoritative
            .insert(key.clone(), envelope.clone())
            .is_some()
        {
            return Err(invariant("报价过期后的 ledger 包含重复 envelope key"));
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

    for (code, market) in &session.state.markets {
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
    for (code, orders) in &session.state.auction_orders {
        for order in orders {
            validate_serializable_order_id(order.order_id)?;
            let key = EnvelopeKey {
                account: order.owner,
                stock: code.clone(),
                order: OrderId(order.order_id),
                side: order.side,
            };
            let envelope = remove_live(&mut unmatched, &mut expected, &key, "auction order")?;
            validate_auction_audit(&envelope, order)?;
        }
    }
    if !unmatched.is_empty() || !expected.is_empty() {
        return Err(invariant("报价过期后的 envelope ledger 缺少对应活动订单"));
    }
    Ok(authoritative)
}

fn validate_ledger_evidence(session: &GameSession) -> Result<(), StepFatal> {
    let ledger = &session.state.envelope_ledger;
    ledger
        .validate_conservation()
        .map_err(|error| invariant(&format!("报价过期后的 envelope ledger 守恒无效：{error}")))?;
    let expected_rows = ledger
        .envelopes
        .len()
        .checked_add(ledger.terminal_envelopes.len())
        .ok_or_else(|| invariant("报价过期后的 envelope ledger 行数溢出"))?;
    if ledger.audits.len() != expected_rows || ledger.conservation.len() != expected_rows {
        return Err(invariant(
            "报价过期后的 envelope ledger 证据行与 envelope 行不一致",
        ));
    }
    for (key, envelope) in ledger
        .envelopes
        .iter()
        .chain(ledger.terminal_envelopes.iter())
    {
        envelope.validate().map_err(|error| {
            invariant(&format!(
                "报价过期后的 envelope ledger 包含无效 envelope：{error}"
            ))
        })?;
        if envelope.origin() != EnvelopeOrigin::TickStart {
            return Err(invariant(
                "报价过期后的 Auction 输入包含同 tick 创建的 envelope",
            ));
        }
        if ledger.envelopes.contains_key(key) && ledger.terminal_envelopes.contains_key(key) {
            return Err(invariant("报价过期后的 live 与 terminal envelope 行重叠"));
        }
        if ledger.audits.get(key).copied() != Some(envelope.audit()) {
            return Err(invariant(
                "报价过期后的 envelope audit 行与 envelope 不一致",
            ));
        }
        if !ledger.conservation.contains_key(key) {
            return Err(invariant("报价过期后的 envelope 缺少守恒证据行"));
        }
    }
    if ledger
        .envelopes
        .values()
        .any(|envelope| envelope.live() == ResVec::ZERO)
    {
        return Err(invariant(
            "报价过期后的 live envelope 行缺少 live resources",
        ));
    }
    if ledger
        .terminal_envelopes
        .values()
        .any(|envelope| envelope.live() != ResVec::ZERO)
    {
        return Err(invariant(
            "报价过期后的 terminal envelope 行仍含有 live resources",
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
    if envelope.pending_price().is_some()
        || audit.limit != order.price
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
    if envelope.pending_price().is_some()
        || audit.limit != order.limit
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

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::stock_auction_adapter".to_owned(),
    }
}
