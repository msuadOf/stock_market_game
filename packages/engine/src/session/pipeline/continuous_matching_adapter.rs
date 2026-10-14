use super::{
    continuous_matching::{ContinuousEnvelopeSnapshot, ContinuousStockInput, StockMarketDataPrior},
    npc_split::SessionReadView,
    EnvelopeKey, EnvelopeOrigin, StepFatal,
};
use crate::compute::parallel::*;
use crate::{GameSession, Market, Order, StockId, TradingPhase};

/// 结构性测试计数器：记录 prepare_stock_input_on 内 BTreeSet 增量插入查重的次数。
/// 融合排序方案落地后该路径应消失，计数保持 0。
#[cfg(test)]
pub(super) static IDENTITY_SET_INSERTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Validate and prepare each independent continuous book on the Rayon pool.
/// The session stays read-only until every stock input has been detached.
pub(super) fn prepare_incremental_continuous_inputs(
    session: &GameSession,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    prepare_incremental_continuous_inputs_on(&session.read_view())
}

/// 读视图版本（prepare 链投机路径共用）。
pub(super) fn prepare_incremental_continuous_inputs_on(
    view: &SessionReadView<'_>,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    prepare_inputs_with_observer_on(view, || {})
}

/// T4（ADR-0042 §6-T4「行情」面）：从只读 view 逐股构建行情先验（活动日 K/
/// 价格历史/分钟收盘/裁剪长度/日界基线），交 `continuous_shards` 注入股票
/// owner——owner 连续持有并在收尾一次产出 patch。`None` 先验=会话侧缺该
/// 股条目（收尾需要时按原错误文案显式失败）。
pub(super) fn market_data_priors(
    view: &SessionReadView<'_>,
) -> std::collections::BTreeMap<crate::StockId, StockMarketDataPrior> {
    let candle_time = crate::session::candles::candle_date_time(view.civil_clock.current_date());
    view.markets
        .keys()
        .map(|code| {
            (
                code.clone(),
                StockMarketDataPrior {
                    candle: view.candle_book.active().get(code).cloned(),
                    price_history: view.price_history.get(code).cloned(),
                    minute_closes: view.market_minute_closes.get(code).cloned(),
                    candle_time,
                    history_len: view.setup.history_len,
                    day: u64::from(view.day),
                },
            )
        })
        .collect()
}

pub(super) fn prepare_inputs_with_observer(
    session: &GameSession,
    observe_row: impl Fn() + Sync,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    prepare_inputs_with_observer_on(&session.read_view(), observe_row)
}

/// 读视图版本（读集：phase 经 view 推导，markets/envelope_ledger）。
pub(super) fn prepare_inputs_with_observer_on(
    view: &SessionReadView<'_>,
    observe_row: impl Fn() + Sync,
) -> Result<Vec<ContinuousStockInput>, StepFatal> {
    crate::session::envelope_projection::verify_fee_config_unchanged(view)?;
    let mut prepared = view.compute.parallel(|| {
        view.compute
            .record(crate::compute::ComputeWork::StockPrepare);
        view.markets
            .par_iter()
            .map(|(code, market)| {
                (
                    code.clone(),
                    prepare_stock_input_on(view, code, market, &observe_row),
                )
            })
            .collect::<Vec<_>>()
    });
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
    if matched != view.envelope_ledger.iter().count() {
        for (_, envelope) in view.envelope_ledger.iter() {
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

/// 读视图版本（内部叶子；读 phase/setup.config，逐股簿捕获）。
fn prepare_stock_input_on(
    view: &SessionReadView<'_>,
    code: &StockId,
    market: &Market,
    observe_row: &(impl Fn() + Sync),
) -> Result<(usize, ContinuousStockInput), StepFatal> {
    if market.code() != code {
        return Err(invariant(
            "market map key disagrees with the market stock code",
        ));
    }
    let policy = super::phase::PhasePolicy::capture_on(view);
    if market.matching_mode() != policy.matching_mode() {
        return Err(invariant("市场 book 模式与 PhasePolicy 不一致"));
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
        // 方向A+B·任务粗化→宽度封顶（宽度税治理）：挂单捕获原先按单订单
        // 分裂（s5 实测每_tick ~1.8k 单订单任务=LOCAL_PUSH 主力），方向A
        // 加 with_min_len 后仍残留 rayon join 驱动线程 SpinLatch 自旋+全池
        // O(N) 偷取扫描（128w ~77 万次/ tick，.tmp/width-fix-A/ 归因）；
        // 方向B 改走 scatter-gather：min(订单数, active_width) 个粗 chunk、
        // 协调线程阻塞归并零自旋。捕获结果与首错选择（按簿序）不变。
        let orders: Vec<(usize, &Order)> = market.resting_order_refs().enumerate().collect();
        let captured: Vec<ContinuousEnvelopeSnapshot> = view
            .compute
            .parallel_map_chunks(orders.len(), |index| {
                let (book_index, order) = orders[index];
                let mut key = make_key();
                capture_order_on(
                    view,
                    code,
                    order,
                    duplicate_by_book[book_index],
                    &mut key,
                    observe_row,
                )
            })?;
        slots.extend(captured.into_iter().map(Some));
    } else {
        let mut key = make_key();
        for (book_index, order) in market.resting_order_refs().enumerate() {
            slots.push(
                match capture_order_on(
                    view,
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
            policy,
            market: market.clone(),
            envelopes,
            operations: Vec::new(),
            config: view.setup.config.clone(),
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

/// 读视图版本（内部叶子；读 envelope_ledger）。
///
/// 任务65 #1 轻量 capture：保留 book↔ledger 四字段交叉核对与 envelope 完整校验，
/// 不再对既有 resting 单重算预留/名义费用——该值由 AccountValidation 创建、逐腿
/// receipt 与恢复 hydrate 权威确立；费率输入不可变由 prepare 入口的指纹守卫兜底。
fn capture_order_on(
    view: &SessionReadView<'_>,
    _code: &StockId,
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
    let envelope = view
        .envelope_ledger
        .get(key)
        .map_err(|_| invariant("continuous order books contain an unledgered order"))?;
    envelope.validate()?;
    if envelope.origin() != EnvelopeOrigin::TickStart {
        return Err(invariant(
            "报价过期后的 live ledger 包含 origin 非 TickStart 的 envelope",
        ));
    }
    if envelope.key() != key {
        return Err(invariant("ledger行键与规范订单身份不一致"));
    }
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
    // 每订单proof随clone进入private worker；未变事实不在worker初始化中重建。
    envelope.verified_live_for_capture(&view.setup.config)?;
    Ok(ContinuousEnvelopeSnapshot {
        envelope: envelope.clone(),
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_matching_adapter".to_owned(),
    }
}
