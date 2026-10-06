use super::{
    continuous_matching::{ContinuousEnvelopeSnapshot, ContinuousStockInput},
    EnvelopeKey, EnvelopeOrigin, StepFatal,
};
use crate::{GameSession, Market, Order, StockCode, TradingPhase};
use rayon::prelude::*;

/// 结构性测试计数器：记录 prepare_stock_input 内 BTreeSet 增量插入查重的次数。
/// 融合排序方案落地后该路径应消失，计数保持 0。
#[cfg(test)]
pub(super) static IDENTITY_SET_INSERTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Validate and prepare each independent continuous book on the Rayon pool.
/// The session stays read-only until every stock input has been detached.
pub(super) fn prepare_incremental_continuous_inputs(
    session: &GameSession,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    prepare_inputs_with_observer(session, || {})
}

pub(super) fn prepare_inputs_with_observer(
    session: &GameSession,
    observe_row: impl Fn() + Sync,
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
        .state
        .auction_orders
        .values()
        .any(|orders| !orders.is_empty())
    {
        return Err(invariant(
            "continuous-book phase contains residual auction orders",
        ));
    }

    let mut prepared = session
        .state
        .markets
        .par_iter()
        .map(|(code, market)| {
            (
                code.clone(),
                prepare_stock_input(session, code, market, &observe_row),
            )
        })
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
    if matched != session.state.envelope_ledger.iter().count() {
        for (_, envelope) in session.state.envelope_ledger.iter() {
            envelope.validate()?;
            if envelope.origin() != EnvelopeOrigin::TickStart {
                return Err(invariant(
                    "报价过期后的 live ledger 包含 origin 非 TickStart 的 envelope",
                ));
            }
        }
        return Err(invariant("报价过期后的 envelope ledger 缺少对应活动订单簿"));
    }
    Ok(inputs)
}

fn prepare_stock_input(
    session: &GameSession,
    code: &StockCode,
    market: &Market,
    observe_row: &(impl Fn() + Sync),
) -> Result<(usize, ContinuousStockInput), StepFatal> {
    if market.code() != code {
        return Err(invariant(
            "market map key disagrees with the market stock code",
        ));
    }
    // 同一批订单只用一次 identity 排序承担两项职责：相邻比较查重 + 按 identity 序发射。
    // 单股内 stock 为常量，(account, order, side) 的 identity 序与完整 EnvelopeKey 序一致，
    // 因此不再需要捕获后的第二次 String 全键排序；identity 元组是 Copy，比较也不触 String。
    let mut plan: Vec<((crate::AccountId, crate::OrderId, u8), usize)> = market
        .resting_order_refs()
        .enumerate()
        .map(|(book_index, order)| (order_identity(order), book_index))
        .collect();
    plan.sort_unstable();
    let mut duplicate_by_book = vec![false; plan.len()];
    for pair in plan.windows(2) {
        if pair[0].0 == pair[1].0 {
            // 簿序较后的重复行负责报错，与旧 BTreeSet 增量插入对后出现行的标记一致。
            duplicate_by_book[pair[1].1] = true;
        }
    }
    let make_key = || EnvelopeKey {
        account: crate::AccountId(0),
        stock: code.clone(),
        order: crate::OrderId(0),
        side: crate::Side::Buy,
    };
    // 槽位按簿序存放捕获结果；首错仍按簿序选择，与旧行为一致。
    let mut slots: Vec<Option<ContinuousEnvelopeSnapshot>> = Vec::with_capacity(plan.len());
    if market.resting_order_count() >= 512 {
        // 并行核对独立行；Result 保序收集，首错不会按完成先后选择。
        let captured: Vec<_> = market
            .resting_order_refs()
            .enumerate()
            .collect::<Vec<_>>()
            .par_iter()
            .with_min_len(128)
            .map_init(make_key, |key, (book_index, order)| {
                capture_order(
                    session,
                    code,
                    order,
                    duplicate_by_book[*book_index],
                    key,
                    observe_row,
                )
            })
            .collect::<Vec<_>>();
        for result in captured {
            slots.push(match result {
                Ok(snapshot) => Some(snapshot),
                Err(error) => return Err(error),
            });
        }
    } else {
        let mut key = make_key();
        for (book_index, order) in market.resting_order_refs().enumerate() {
            slots.push(
                match capture_order(
                    session,
                    code,
                    order,
                    duplicate_by_book[book_index],
                    &mut key,
                    observe_row,
                ) {
                    Ok(snapshot) => Some(snapshot),
                    Err(error) => return Err(error),
                },
            );
        }
    }
    let mut envelopes = Vec::with_capacity(slots.len());
    for (_, book_index) in &plan {
        envelopes.push(
            slots[*book_index]
                .take()
                .expect("identity plan 覆盖每个簿序且簿序不重复"),
        );
    }
    let count = envelopes.len();
    Ok((
        count,
        ContinuousStockInput {
            phase: session.phase(),
            market: market.clone(),
            envelopes,
            operations: Vec::new(),
            config: session.state.setup.config.clone(),
        },
    ))
}

fn order_identity(order: &Order) -> (crate::AccountId, crate::OrderId, u8) {
    (
        order.owner,
        order.id,
        match order.side {
            crate::Side::Buy => 0,
            crate::Side::Sell => 1,
        },
    )
}

fn capture_order(
    session: &GameSession,
    code: &StockCode,
    order: &Order,
    duplicate: bool,
    key: &mut EnvelopeKey,
    observe_row: &(impl Fn() + Sync),
) -> Result<ContinuousEnvelopeSnapshot, StepFatal> {
    observe_row();
    key.account = order.owner;
    key.order = order.id;
    key.side = order.side;
    if duplicate {
        return Err(invariant(
            "continuous order books repeat an envelope identity",
        ));
    }
    let envelope = session
        .state
        .envelope_ledger
        .get(key)
        .map_err(|_| invariant("continuous order books contain an unledgered order"))?;
    envelope.validate()?;
    if envelope.origin() != EnvelopeOrigin::TickStart {
        return Err(invariant(
            "报价过期后的 live ledger 包含 origin 非 TickStart 的 envelope",
        ));
    }
    let projected = session.project_continuous_envelope_facts(code, order)?;
    let audit = envelope.audit();
    if envelope.pending_price().is_some()
        || audit.limit != order.price
        || audit.remaining_qty != order.qty
        || audit.filled_qty != order.filled_qty
        || audit.filled_value != order.filled_value
    {
        return Err(invariant(
            "continuous order books disagree with envelope audit",
        ));
    }
    if envelope.live() != projected.live {
        return Err(invariant(
            "continuous order books disagree with live envelope resources",
        ));
    }
    Ok(ContinuousEnvelopeSnapshot {
        audit,
        envelope: envelope.clone(),
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_matching_adapter".to_owned(),
    }
}
