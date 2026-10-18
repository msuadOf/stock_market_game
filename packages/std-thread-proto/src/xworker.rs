//! 跨股臂 StockWorker：家端账户（primary + secondaries）+ 本股簿 + 外部席位。
//!
//! 每 tick 三相位（coordinator 中转，Sol 建议先测中转模型）：
//! - 相位 1（Start）：决策（多股视图，只读名册）→ 到期撤销 → 本股意图直落
//!   本股簿（与按股私有臂同路径）→ 外股意图 escrow（现金/可卖预留）→
//!   上交路由操作（对齐撤单 + 跨股下单）。
//! - 相位 2（Route）：先撤后挂——应用路由撤单，再以「外部席位」落簿撮合；
//!   成交/终结产生 FillReceipt，随 TickDone 上交。
//! - 相位 3（Receipts）：应用回执到次级仓/现金（家端权威记账），T+1 解冻。
//!   相位 3 不设全局屏障（家端 FIFO 通道保证回执先于下一 Start 处理）。
//!
//! 账户所有权：NPC 账户全部归主交互股（home）worker 私有；外股簿上的
//! 挂单通过「席位 = escrow 资源实体」持有资金/股数，撮合只动席位，
//! 家端只通过回执变更（消除跨 worker 共享写）。

use crate::book::{Arena, FillReceipt, ForeignId, Placement, StockBook};
use crate::npc::{FeeTable, Npc, XPos};
use crate::setup::ProtoConfig;
use crate::worker::{
    ChunkStats, MarketSnapshot, RoutedPrice, ThreadGauge, TickDone,
};
use crate::xsetup::{build_cross_roster, CrossSetupInfo};
use crate::ProtoError;
use engine::account::StockCode;
use engine::money::Money;
use engine::session::SplitMix64;
use engine::strategy::{
    decide_data, Intent, MarketView, PositionView, SelfView, StockView,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};

/// coordinator → 家端/目标股 worker 的下行消息。
pub enum XMsg {
    /// 相位 1 启动（tick 快照）。
    Start(Arc<MarketSnapshot>),
    /// 相位 2：本股作为目标股收到的一批路由操作（先撤后挂已排序）。
    Route(Vec<RoutedOp>),
    /// 相位 3：本股作为家端收到的一批回执。
    Receipts(Vec<FillReceipt>),
    /// 收尾：强撤全部外部挂单并上交回执（期末审计前清空在途 escrow）。
    Drain,
    Shutdown,
}

/// worker → coordinator 的上行消息。
pub enum XUp {
    /// 相位 1 完成：携带待路由操作（按目标股分拣前的原始序）。
    Phase1(Vec<RoutedOp>),
    /// 相位 2 完成：本 tick 终态 + 回执（含相位 1/2 期间产生的全部回执）。
    Phase2(TickDone, Vec<FillReceipt>),
    /// Drain 完成：强撤产生的回执。
    DrainReceipts(Vec<FillReceipt>),
    /// worker 退出：期末状态（审计用）。
    Final(Box<XFinalState>),
}

/// 跨股路由操作（coordinator 中转；`target` = 目标股索引，分拣用）。
#[derive(Clone, Copy, Debug)]
pub enum RoutedOp {
    /// 对齐撤单：撤该外部 NPC 在目标股簿的全部挂单（escrow 回流家端）。
    CancelAll { home: u8, npc: u32, target: u8 },
    /// 跨股下单：escrow 已在家端完成（买=escrow_cash，卖=qty 股可卖），
    /// 价格符号在目标股受理时解析。
    Place {
        home: u8,
        npc: u32,
        target: u8,
        side: engine::orderbook::Side,
        price: RoutedPrice,
        qty: u32,
        escrow_cash: i64,
    },
}

impl RoutedOp {
    /// 目标股索引（coordinator 分拣用）。
    pub fn target(&self) -> u8 {
        match *self {
            RoutedOp::CancelAll { target, .. } | RoutedOp::Place { target, .. } => target,
        }
    }
}

/// 跨股臂期末状态（审计用）。
pub struct XFinalState {
    pub stock_idx: usize,
    /// 家端名册（primary + secondaries）。
    pub roster: Vec<Npc>,
    pub fee_sink: i64,
    pub trade_count: u64,
    pub volume_shares: u64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub min_trade_price: Option<i64>,
    pub max_trade_price: Option<i64>,
    pub float_remainder: u64,
    pub setup: CrossSetupInfo,
    // 跨股计数（报告用）。
    pub routed_places: u64,
    pub routed_cancels: u64,
    pub spurious_cancels: u64,
    pub foreign_skips: u64,
    pub cancel_intents: u64,
    /// BuyFill / SellFill / BuyClosed / SellClosed 的应用次数。
    pub receipts_applied: [u64; 4],
    pub desks_peak: u64,
}

/// 一条跨股决策产物：名册索引 + 目标股 + 意图。
struct XRecord {
    npc_idx: u32,
    target: u8,
    intent: Intent,
}

/// worker 主函数：构造家端名册后进入三相位消息循环。
#[allow(clippy::too_many_arguments)]
pub fn xworker_main(
    cfg: Arc<ProtoConfig>,
    stock_idx: usize,
    game_cfg: Arc<engine::config::GameConfig>,
    cross_pct: u32,
    rx: Receiver<XMsg>,
    up_tx: Sender<XUp>,
    gauge: Arc<ThreadGauge>,
    per_stock_threads: u32,
) -> Result<(), ProtoError> {
    let (roster, float_remainder, setup) = build_cross_roster(&cfg, stock_idx, cross_pct)?;
    let home = stock_idx as u8;
    let xdbg = crate::xdbg();
    if xdbg {
        eprintln!("[w{stock_idx}] 名册构造完成 npcs={}", roster.len());
    }
    // 全股静态表：code / 涨跌停带 / code→索引。
    let codes: Vec<StockCode> = cfg.stocks.iter().map(|s| StockCode(s.code.clone())).collect();
    let code_idx: HashMap<String, u8> = codes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.0.clone(), i as u8))
        .collect();
    let bands: Vec<(i64, i64)> = cfg
        .stocks
        .iter()
        .map(|s| {
            let band = (s.initial_price_cents as f64 * s.limit_pct) as i64;
            ((s.initial_price_cents - band).max(1), s.initial_price_cents + band)
        })
        .collect();
    let initial_prices: Vec<i64> = cfg.stocks.iter().map(|s| s.initial_price_cents).collect();
    // 持股集合位掩码（静态：retail 只交易视图内股票，掩码不会增长）。
    let masks: Vec<u8> = roster
        .iter()
        .map(|npc| {
            let mut m = 1u8 << home;
            for pos in &npc.secondaries {
                m |= 1 << pos.stock_idx;
            }
            m
        })
        .collect();
    let mut views_masks: Vec<u8> = masks.iter().copied().collect();
    views_masks.sort_unstable();
    views_masks.dedup();

    let arena: Arc<RwLock<Arena>> = Arc::new(RwLock::new(Arena::new(roster, home)));
    let mut book = StockBook::new();
    let fees = FeeTable::from_config(&game_cfg);
    let (lower_home, upper_home) = bands[stock_idx];
    let mut last_price = initial_prices[stock_idx];

    // 家端在途外部单登记：npc → 每股在途路由单**计数**（而非布尔位——
    // 旧单的 Closed 回执可能晚于新单 escrow 到达，布尔位会被迟到的回执
    // 误清；计数保证旧单回执只抵消旧单自己）。
    if cfg.stocks.len() > 8 {
        return Err(ProtoError::InvalidConfig(
            "跨股臂最多支持 8 只股票（在途登记定长）".into(),
        ));
    }
    let roster_len = {
        let g = arena
            .read()
            .map_err(|_| ProtoError::ChannelClosed("arena 读锁中毒".into()))?;
        g.roster_len
    };
    let mut foreign_active = vec![[0u16; 8]; roster_len as usize];
    // 外部席位登记与空闲槽回收（避免竞技场无限增长）。
    let mut desk_index: HashMap<ForeignId, u32> = HashMap::new();
    let mut desk_free: Vec<u32> = Vec::new();
    let mut desks_peak = 0u64;

    let mut result: Result<(), ProtoError> = Ok(());
    let xdbg = crate::xdbg();
    let mut pending_decide = 0u64;
    let mut pending_intents = 0u64;
    let mut vol_before = 0u64;
    let mut trades_before = 0u64;
    let mut cur_tick = 0u64;
    let mut routed_places = 0u64;
    let mut routed_cancels = 0u64;
    let mut spurious_cancels = 0u64;
    let mut foreign_skips = 0u64;
    let mut cancel_intents = 0u64;
    let mut receipts_applied = [0u64; 4];

    while let Ok(msg) = rx.recv() {
        match msg {
            XMsg::Start(snapshot) => {
                if xdbg {
                    eprintln!("[w{}] 收到 Start tick={}", stock_idx, snapshot.tick);
                }
                cur_tick = snapshot.tick;
                vol_before = book.volume_shares;
                trades_before = book.trade_count;
                // ── 相位 1：决策 → 到期 → 本股应用 → 外股 escrow ──
                let views = build_views(&cfg, &snapshot, &codes, &bands, &views_masks);
                let step = phase1(
                    &cfg,
                    &game_cfg,
                    home,
                    stock_idx,
                    &codes,
                    &code_idx,
                    &bands,
                    &views,
                    &masks,
                    &arena,
                    &mut book,
                    &fees,
                    per_stock_threads,
                    &gauge,
                    &mut foreign_active,
                    snapshot.tick,
                    &mut routed_places,
                    &mut routed_cancels,
                    &mut foreign_skips,
                    &mut cancel_intents,
                );
                match step {
                    Ok((ops, decide_calls, intents)) => {
                        pending_decide = decide_calls;
                        pending_intents = intents;
                        if xdbg {
                            eprintln!("[w{}] Phase1 完成 ops={} tick={}", stock_idx, ops.len(), cur_tick);
                        }
                        if up_tx.send(XUp::Phase1(ops)).is_err() {
                            result = Err(ProtoError::ChannelClosed("up_tx Phase1".into()));
                            break;
                        }
                    }
                    Err(e) => {
                        if xdbg {
                            eprintln!("[w{}] Phase1 失败：{e}", stock_idx);
                        }
                        result = Err(e);
                        break;
                    }
                }
            }
            XMsg::Route(ops) => {
                if xdbg {
                    if let Ok(g) = arena.read() {
                        if let Some(v) = roster_invariant_scan(&g, "相位1后") {
                            eprintln!("[w{stock_idx}] {v}");
                        }
                    }
                    eprintln!("[w{}] 收到 Route ops={} tick={}", stock_idx, ops.len(), cur_tick);
                }
                // ── 相位 2：先撤后挂 + 撮合，回执随 TickDone 上交 ──
                let arena_rc = Arc::clone(&arena);
                let mut guard = arena_rc
                    .write()
                    .map_err(|_| ProtoError::ChannelClosed("arena 写锁中毒".into()))?;
                // 先撤（同 NPC 的 cancel 必须先于 place 落地；跨 NPC 顺序无语义）。
                for op in &ops {
                    if let RoutedOp::CancelAll { home: h, npc, .. } = *op {
                        match desk_index.get(&(h, npc)) {
                            Some(&slot) => {
                                let n = book.cancel_all_for(&mut guard, slot)?;
                                if n == 0 {
                                    spurious_cancels += 1;
                                }
                                if !book.has_active_orders(slot) {
                                    desk_index.remove(&(h, npc));
                                    desk_free.push(slot);
                                    guard.free_desk(slot);
                                }
                            }
                            None => spurious_cancels += 1,
                        }
                    }
                }
                // 后挂（外部席位 escrow 实体）。
                for op in ops {
                    if let RoutedOp::Place {
                        home: h,
                        npc,
                        side,
                        price,
                        qty,
                        escrow_cash,
                        ..
                    } = op
                    {
                        let step = apply_routed_place(
                            &mut book,
                            &mut guard,
                            &mut desk_index,
                            &mut desk_free,
                            &mut desks_peak,
                            &mut foreign_skips,
                            home,
                            cur_tick,
                            cfg.order_ttl_ticks,
                            &fees,
                            (lower_home, upper_home),
                            h,
                            npc,
                            side,
                            price,
                            qty,
                            escrow_cash,
                        );
                        if let Err(e) = step {
                            result = Err(e);
                            break;
                        }
                    }
                }
                if result.is_err() {
                    break;
                }
                sweep_desks(&book, &mut desk_index, &mut desk_free, &mut guard);
                desks_peak = desks_peak.max(desk_index.len() as u64);
                let done = TickDone {
                    stock_idx,
                    last_price: book.last_price.unwrap_or(last_price),
                    best_bid: book.best_bid(),
                    best_ask: book.best_ask(),
                    volume_delta: book.volume_shares - vol_before,
                    trade_count_delta: book.trade_count - trades_before,
                    decide_calls: pending_decide,
                    intents: pending_intents,
                    resting: book.resting_orders(),
                    imbalance: book.top5_imbalance(),
                };
                last_price = done.last_price;
                let receipts = std::mem::take(&mut guard.receipts);
                drop(guard);
                if up_tx.send(XUp::Phase2(done, receipts)).is_err() {
                    result = Err(ProtoError::ChannelClosed("up_tx Phase2".into()));
                    break;
                }
            }
            XMsg::Receipts(receipts) => {
                if xdbg {
                    if let Ok(g) = arena.read() {
                        if let Some(v) = roster_invariant_scan(&g, "相位2后") {
                            eprintln!("[w{stock_idx}] {v}");
                        }
                    }
                }
                // ── 相位 3：回执应用（家端权威记账）+ T+1 解冻 ──
                let arena_rc = Arc::clone(&arena);
                let mut guard = arena_rc
                    .write()
                    .map_err(|_| ProtoError::ChannelClosed("arena 写锁中毒".into()))?;
                let step = apply_receipts(
                    &mut guard,
                    &mut foreign_active,
                    &initial_prices,
                    &mut receipts_applied,
                    receipts,
                );
                if let Err(e) = step {
                    result = Err(e);
                    break;
                }
                for npc in guard.accounts_mut() {
                    npc.unfreeze_t1_all();
                }
                if xdbg {
                    if let Some(v) = roster_invariant_scan(&guard, "相位3后") {
                        eprintln!("[w{stock_idx}] {v}");
                    }
                }
            }
            XMsg::Drain => {
                // 强撤全部外部挂单：清空在途 escrow，期末守恒才可闭合。
                let arena_rc = Arc::clone(&arena);
                let mut guard = arena_rc
                    .write()
                    .map_err(|_| ProtoError::ChannelClosed("arena 写锁中毒".into()))?;
                let slots: Vec<u32> = desk_index.values().copied().collect();
                for slot in slots {
                    book.cancel_all_for(&mut guard, slot)?;
                }
                sweep_desks(&book, &mut desk_index, &mut desk_free, &mut guard);
                let receipts = std::mem::take(&mut guard.receipts);
                drop(guard);
                if up_tx.send(XUp::DrainReceipts(receipts)).is_err() {
                    result = Err(ProtoError::ChannelClosed("up_tx Drain".into()));
                    break;
                }
            }
            XMsg::Shutdown => break,
        }
    }
    let roster = Arc::try_unwrap(arena)
        .map_err(|_| ProtoError::ChannelClosed("arena 仍有引用".into()))?
        .into_inner()
        .map_err(|_| ProtoError::ChannelClosed("arena 锁中毒".into()))?
        .into_roster();
    let send = up_tx.send(XUp::Final(Box::new(XFinalState {
        stock_idx,
        roster,
        fee_sink: book.fee_sink,
        trade_count: book.trade_count,
        volume_shares: book.volume_shares,
        skipped_orders: book.skipped_orders,
        expired_orders: book.expired_orders,
        cancelled_orders: book.cancelled_orders,
        min_trade_price: book.min_trade_price,
        max_trade_price: book.max_trade_price,
        float_remainder,
        setup,
        routed_places,
        routed_cancels,
        spurious_cancels,
        foreign_skips,
        cancel_intents,
        receipts_applied,
        desks_peak,
    })));
    if result.is_ok() && send.is_err() {
        result = Err(ProtoError::ChannelClosed("up_tx Final".into()));
    }
    result
}

/// 相位 1：决策（多股视图）→ 到期 → 本股意图直落 → 外股 escrow 路由。
#[allow(clippy::too_many_arguments)]
fn phase1(
    cfg: &ProtoConfig,
    game_cfg: &engine::config::GameConfig,
    home: u8,
    stock_idx: usize,
    codes: &[StockCode],
    code_idx: &HashMap<String, u8>,
    bands: &[(i64, i64)],
    views: &[Option<Arc<MarketView>>],
    masks: &[u8],
    arena: &Arc<RwLock<Arena>>,
    book: &mut StockBook,
    fees: &FeeTable,
    per_stock_threads: u32,
    gauge: &Arc<ThreadGauge>,
    foreign_active: &mut [[u16; 8]],
    tick: u64,
    routed_places: &mut u64,
    routed_cancels: &mut u64,
    foreign_skips: &mut u64,
    cancel_intents: &mut u64,
) -> Result<(Vec<RoutedOp>, u64, u64), ProtoError> {
    // 决策（只读名册；width=0 内联 / width>0 scope 线程）。
    let (records, stats) = {
        let guard = arena
            .read()
            .map_err(|_| ProtoError::ChannelClosed("arena 读锁中毒".into()))?;
        let roster = guard.accounts();
        if per_stock_threads == 0 {
            decide_range_x(
                roster,
                masks,
                0,
                guard.roster_len,
                home,
                codes,
                code_idx,
                views,
                game_cfg,
                cfg.seed,
                tick,
            )?
        } else {
            let len = guard.roster_len;
            let mut joined: Vec<(usize, Vec<XRecord>, ChunkStats)> =
                Vec::with_capacity(per_stock_threads as usize);
            std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(per_stock_threads as usize);
                for slot in 0..per_stock_threads as usize {
                    let (from, to) = chunk_range(len, per_stock_threads, slot);
                    handles.push(scope.spawn(move || {
                        gauge.enter();
                        let out = decide_range_x(
                            roster,
                            masks,
                            from,
                            to,
                            home,
                            codes,
                            code_idx,
                            views,
                            game_cfg,
                            cfg.seed,
                            tick,
                        );
                        gauge.exit();
                        out
                    }));
                }
                for (slot, handle) in handles.into_iter().enumerate() {
                    match handle.join() {
                        Ok(Ok((records, stats))) => joined.push((slot, records, stats)),
                        Ok(Err(e)) => std::panic::panic_any(format!("决策线程失败：{e}")),
                        Err(panic) => std::panic::resume_unwind(panic),
                    }
                }
            });
            joined.sort_by_key(|(slot, _, _)| *slot);
            let mut records = Vec::new();
            let mut stats = ChunkStats::default();
            for (_, r, s) in joined {
                records.extend(r);
                stats.decide_calls += s.decide_calls;
                stats.intents += s.intents;
            }
            (records, stats)
        }
    };

    let mut ops: Vec<RoutedOp> = Vec::new();
    {
        let mut guard = arena
            .write()
            .map_err(|_| ProtoError::ChannelClosed("arena 写锁中毒".into()))?;
        let (lower_home, upper_home) = bands[stock_idx];
        // 到期撤销（整簿；外部挂单终结回执进入缓冲，随相位 2 上交）。
        book.expire_due(&mut guard, tick)?;
        // 每 NPC 每 tick 一次外部对齐标记。
        let mut aligned = vec![false; guard.roster_len as usize];
        for rec in records {
            let npc_idx = rec.npc_idx;
            // 工作单对齐（引擎语义）：新意图先撤两侧挂单——本股直接撤，
            // 外股发对齐撤单路由。
            if !aligned[npc_idx as usize] {
                aligned[npc_idx as usize] = true;
                let active = &foreign_active[npc_idx as usize];
                for (t, &count) in active.iter().enumerate().take(bands.len()) {
                    if count > 0 {
                        ops.push(RoutedOp::CancelAll {
                            home,
                            npc: npc_idx,
                            target: t as u8,
                        });
                        *routed_cancels += 1;
                    }
                }
            }
            let Some((side, rp, qty)) = (match &rec.intent {
                Intent::PlaceLimit { side, price, qty, .. } => {
                    Some((*side, RoutedPrice::from_limit(price, false), *qty))
                }
                Intent::PlaceMarket { side, qty, .. } => {
                    Some((*side, RoutedPrice::Market, *qty))
                }
                // 策略层不产 Cancel（会话层职责）；出现即显式计数，不静默。
                Intent::Cancel { .. } => None,
            }) else {
                *cancel_intents += 1;
                continue;
            };
            if rec.target == home {
                // 本股意图：与按股私有臂完全同路径（直写家端主仓）。
                book.cancel_all_for(&mut guard, npc_idx)?;
                let price = rp.resolve(side, book, upper_home, lower_home);
                if price < lower_home || price > upper_home {
                    book.skipped_orders += 1;
                    continue;
                }
                book.place(
                    &mut guard,
                    npc_idx,
                    side,
                    price,
                    qty,
                    tick,
                    cfg.order_ttl_ticks,
                    fees,
                )?;
            } else {
                // 外股意图：家端 escrow → 路由。
                let target = rec.target;
                let (_, upper_t) = bands[target as usize];
                let npc = &mut guard.accounts[npc_idx as usize];
                match side {
                    engine::orderbook::Side::Buy => {
                        // escrow 上限价：Fixed 用固定价，符号价/市价用目标股涨停。
                        let p = match rp {
                            RoutedPrice::Fixed(p) => p,
                            _ => upper_t,
                        };
                        let value = p
                            .checked_mul(qty as i64)
                            .ok_or_else(|| ProtoError::Overflow {
                                op: "escrow price*qty",
                                operand: format!("{p}*{qty}"),
                            })?;
                        let escrow = value
                            .checked_add(fees.commission(value)?)
                            .ok_or_else(|| ProtoError::Overflow {
                                op: "escrow+fee",
                                operand: format!("{value}"),
                            })?;
                        if npc.available_cash() < escrow {
                            book.skipped_orders += 1;
                            continue;
                        }
                        npc.reserved_cash += escrow;
                        foreign_active[npc_idx as usize][target as usize] += 1;
                        *routed_places += 1;
                        ops.push(RoutedOp::Place {
                            home,
                            npc: npc_idx,
                            target,
                            side,
                            price: rp,
                            qty,
                            escrow_cash: escrow,
                        });
                    }
                    engine::orderbook::Side::Sell => {
                        // 次级仓可卖 escrow（主仓卖出必然 target==home，不会走到这里）。
                        let Some(pos) = npc
                            .secondaries
                            .iter_mut()
                            .find(|p| p.stock_idx == target)
                        else {
                            book.skipped_orders += 1;
                            *foreign_skips += 1;
                            continue;
                        };
                        if pos.sellable < qty {
                            book.skipped_orders += 1;
                            *foreign_skips += 1;
                            continue;
                        }
                        pos.sellable -= qty;
                        pos.reserved_shares += qty;
                        foreign_active[npc_idx as usize][target as usize] += 1;
                        *routed_places += 1;
                        ops.push(RoutedOp::Place {
                            home,
                            npc: npc_idx,
                            target,
                            side,
                            price: rp,
                            qty,
                            escrow_cash: 0,
                        });
                    }
                }
            }
        }
    }
    Ok((ops, stats.decide_calls, stats.intents))
}

/// 相位 2 的单笔路由下单：解析价格 → 建席位 → 落簿撮合。
#[allow(clippy::too_many_arguments)]
fn apply_routed_place(
    book: &mut StockBook,
    arena: &mut Arena,
    desk_index: &mut HashMap<ForeignId, u32>,
    desk_free: &mut Vec<u32>,
    desks_peak: &mut u64,
    foreign_skips: &mut u64,
    home_stock: u8,
    tick: u64,
    ttl: u64,
    fees: &FeeTable,
    band: (i64, i64),
    h: u8,
    npc: u32,
    side: engine::orderbook::Side,
    rp: RoutedPrice,
    qty: u32,
    escrow_cash: i64,
) -> Result<(), ProtoError> {
    let (lower, upper) = band;
    let price = rp.resolve(side, book, upper, lower);
    if price < lower || price > upper {
        // 带外价格显式拒绝（防御：家端已预检 Fixed，符号价按构造在带内）。
        book.skipped_orders += 1;
        *foreign_skips += 1;
        arena.receipts.push(match side {
            engine::orderbook::Side::Buy => FillReceipt::BuyClosed {
                home: h,
                npc,
                stock: home_stock,
                release: escrow_cash,
            },
            engine::orderbook::Side::Sell => FillReceipt::SellClosed {
                home: h,
                npc,
                stock: home_stock,
                qty,
            },
        });
        return Ok(());
    }
    let id = (h, npc);
    let buy_cash = if side == engine::orderbook::Side::Buy { escrow_cash } else { 0 };
    let sell_shares = if side == engine::orderbook::Side::Sell { qty } else { 0 };
    if crate::xdbg() {
        eprintln!(
            "[place] stock={home_stock} tick={tick} id={id:?} side={side:?} price={price:?} qty={qty} escrow={escrow_cash} desk_entry={:?} free={:?}",
            desk_index.get(&id),
            desk_free
        );
    }
    let slot = if let Some(&slot) = desk_index.get(&id) {
        if book.has_active_orders(slot) {
            // 同一 (家端, NPC) 在同一目标股已有活跃挂单且未先撤——对齐语义
            // 下不可达（家端发新单前必发对齐撤单）；显式失败暴露实现 bug。
            let active_ids = book.active_order_ids(slot);
            return Err(ProtoError::InvalidConfig(format!(
                "外部席位并发挂单：home={h} npc={npc} stock={home_stock} tick={tick} 旧单={active_ids:?}"
            )));
        }
        // 席位刚在本批次被撤空：复用槽位，重置 escrow。
        arena.reuse_desk(slot, id, buy_cash, sell_shares);
        slot
    } else if let Some(slot) = desk_free.pop() {
        arena.reuse_desk(slot, id, buy_cash, sell_shares);
        desk_index.insert(id, slot);
        *desks_peak = (*desks_peak).max(desk_index.len() as u64);
        slot
    } else {
        let slot = arena.alloc_desk(id, buy_cash, sell_shares);
        desk_index.insert(id, slot);
        *desks_peak = (*desks_peak).max(desk_index.len() as u64);
        slot
    };
    let outcome = book.place(arena, slot, side, price, qty, tick, ttl, fees)?;
    match outcome {
        Placement::FullyFilled | Placement::Rested => Ok(()),
        Placement::Skipped(_) => {
            // 防御分支：家端按同一公式预留（value+佣金上限），理论不可达；
            // escrow 全额回流家端并回收席位，不静默。
            *foreign_skips += 1;
            arena.receipts.push(match side {
                engine::orderbook::Side::Buy => FillReceipt::BuyClosed {
                    home: h,
                    npc,
                    stock: home_stock,
                    release: escrow_cash,
                },
                engine::orderbook::Side::Sell => FillReceipt::SellClosed {
                    home: h,
                    npc,
                    stock: home_stock,
                    qty,
                },
            });
            Ok(())
        }
    }
}

/// 相位 3：回执应用到家端账户（次级仓/现金），并清在途登记。
fn apply_receipts(
    arena: &mut Arena,
    foreign_active: &mut [[u16; 8]],
    initial_prices: &[i64],
    receipts_applied: &mut [u64; 4],
    receipts: Vec<FillReceipt>,
) -> Result<(), ProtoError> {
    for r in receipts {
        match r {
            FillReceipt::BuyFill {
                npc, stock, qty, charge, ..
            } => {
                receipts_applied[0] += 1;
                let acct = &mut arena.accounts[npc as usize];
                acct.reserved_cash = acct
                    .reserved_cash
                    .checked_sub(charge)
                    .ok_or_else(|| ProtoError::InvalidConfig(format!(
                        "BuyFill 回执导致 reserved_cash 下溢：npc={npc} charge={charge}"
                    )))?;
                acct.cash = acct
                    .cash
                    .checked_sub(charge)
                    .ok_or_else(|| ProtoError::NegativeCash {
                        code: format!("npc-{}", npc),
                        npc,
                        cash: acct.cash - charge,
                    })?;
                // 次级仓找或建（运行期掩码静态 ⇒ 理论必命中；建分支防御保留）。
                if let Some(pos) = acct.secondaries.iter_mut().find(|p| p.stock_idx == stock) {
                    pos.qty += qty;
                    pos.frozen_today += qty;
                } else {
                    acct.secondaries.push(XPos {
                        stock_idx: stock,
                        qty,
                        sellable: 0,
                        reserved_shares: 0,
                        frozen_today: qty,
                        cost_price_cents: initial_prices[stock as usize],
                    });
                }
            }
            FillReceipt::SellFill {
                npc, stock, qty, net, ..
            } => {
                receipts_applied[1] += 1;
                let acct = &mut arena.accounts[npc as usize];
                let Some(pos) = acct.secondaries.iter_mut().find(|p| p.stock_idx == stock) else {
                    return Err(ProtoError::InvalidConfig(format!(
                        "SellFill 回执无对应次级仓：npc={npc} stock={stock}"
                    )));
                };
                pos.qty = pos
                    .qty
                    .checked_sub(qty)
                    .ok_or_else(|| ProtoError::InvalidConfig(format!(
                        "SellFill 回执导致持仓下溢：npc={npc} stock={stock}"
                    )))?;
                pos.reserved_shares = pos
                    .reserved_shares
                    .checked_sub(qty)
                    .ok_or_else(|| ProtoError::InvalidConfig(format!(
                        "SellFill 回执导致预留下溢：npc={npc} stock={stock}"
                    )))?;
                acct.cash += net; // 卖出净额只增现金
            }
            FillReceipt::BuyClosed {
                npc, stock, release, ..
            } => {
                receipts_applied[2] += 1;
                let acct = &mut arena.accounts[npc as usize];
                acct.reserved_cash = acct
                    .reserved_cash
                    .checked_sub(release)
                    .ok_or_else(|| ProtoError::InvalidConfig(format!(
                        "BuyClosed 回执导致 reserved_cash 下溢：npc={npc} release={release}"
                    )))?;
                decrement_active(foreign_active, npc, stock)?;
            }
            FillReceipt::SellClosed {
                npc, stock, qty, ..
            } => {
                receipts_applied[3] += 1;
                let acct = &mut arena.accounts[npc as usize];
                let Some(pos) = acct.secondaries.iter_mut().find(|p| p.stock_idx == stock) else {
                    return Err(ProtoError::InvalidConfig(format!(
                        "SellClosed 回执无对应次级仓：npc={npc} stock={stock}"
                    )));
                };
                pos.reserved_shares = pos
                    .reserved_shares
                    .checked_sub(qty)
                    .ok_or_else(|| ProtoError::InvalidConfig(format!(
                        "SellClosed 回执导致预留下溢：npc={npc} stock={stock}"
                    )))?;
                pos.sellable += qty;
                decrement_active(foreign_active, npc, stock)?;
            }
        }
    }
    Ok(())
}

/// 调试用：家端名册不变量扫描（负现金/负预留/t1 结构）。返回首个违规描述。
#[allow(dead_code)]
fn roster_invariant_scan(arena: &Arena, phase: &str) -> Option<String> {
    for (i, npc) in arena.roster().iter().enumerate() {
        if npc.cash < 0 || npc.reserved_cash < 0 || !npc.x_t1_ok() {
            return Some(format!(
                "[{phase}] npc槽{i} id={} cash={} reserved={} qty={} sellable={} rs={} ft={} secondaries={:?}",
                npc.id,
                npc.cash,
                npc.reserved_cash,
                npc.qty,
                npc.sellable,
                npc.reserved_shares,
                npc.frozen_today,
                npc.secondaries.len()
            ));
        }
    }
    None
}

/// 在途登记计数递减（Closed 回执）。计数为 0 时收到 Closed = 协议错位，
// 显式失败（铁律二）。
fn decrement_active(
    foreign_active: &mut [[u16; 8]],
    npc: u32,
    stock: u8,
) -> Result<(), ProtoError> {
    let slot = &mut foreign_active[npc as usize][stock as usize];
    if *slot == 0 {
        return Err(ProtoError::InvalidConfig(format!(
            "Closed 回执无对应在途单：npc={npc} stock={stock}"
        )));
    }
    *slot -= 1;
    Ok(())
}

/// 清扫无活跃挂单的外部席位（成交/到期/撤单后回收槽位）。
fn sweep_desks(
    book: &StockBook,
    desk_index: &mut HashMap<ForeignId, u32>,
    desk_free: &mut Vec<u32>,
    arena: &mut Arena,
) {
    let dead: Vec<ForeignId> = desk_index
        .iter()
        .filter(|(_, &slot)| !book.has_active_orders(slot))
        .map(|(id, _)| *id)
        .collect();
    for id in dead {
        if let Some(slot) = desk_index.remove(&id) {
            desk_free.push(slot);
            arena.free_desk(slot);
        }
    }
}

/// 名册区间决策（跨股内核）：多股视图（按持股掩码预建、Arc 共享）+
/// 逐 NPC SelfView（单持仓走模板快路径，多持仓逐 NPC 重建）。
#[allow(clippy::too_many_arguments)]
fn decide_range_x(
    npcs: &[Npc],
    masks: &[u8],
    from: u32,
    to: u32,
    home: u8,
    codes: &[StockCode],
    code_idx: &HashMap<String, u8>,
    views: &[Option<Arc<MarketView>>],
    game_cfg: &engine::config::GameConfig,
    seed: u64,
    tick: u64,
) -> Result<(Vec<XRecord>, ChunkStats), ProtoError> {
    // 单持仓模板（≈70% NPC 的快路径：模板就地把主股字段刷新，零分配）。
    let mut single_positions = BTreeMap::new();
    single_positions.insert(
        codes[home as usize].clone(),
        PositionView {
            qty: 0,
            sellable_qty: 0,
            cost_price: None,
        },
    );
    let mut own_single = SelfView {
        cash: Money::ZERO,
        positions: single_positions,
    };
    // 多持仓 scratch（clear 复用容器）。
    let mut own_multi = SelfView {
        cash: Money::ZERO,
        positions: BTreeMap::new(),
    };
    let mut records = Vec::new();
    let mut stats = ChunkStats::default();
    for idx in from..to {
        let npc = &npcs[idx as usize];
        stats.decide_calls += 1;
        let mask = masks[idx as usize];
        let market = views[mask as usize]
            .as_ref()
            .expect("持股掩码的视图必须预建（掩码静态）");
        if npc.secondaries.is_empty() {
            let pos = own_single
                .positions
                .get_mut(&codes[home as usize])
                .expect("模板已插入主股键");
            pos.qty = npc.qty;
            pos.sellable_qty = npc.sellable;
            pos.cost_price = Some(Money::from_cents(npc.cost_price_cents));
            own_single.cash = Money::from_cents(npc.available_cash());
        } else {
            own_multi.positions.clear();
            own_multi.positions.insert(
                codes[home as usize].clone(),
                PositionView {
                    qty: npc.qty,
                    sellable_qty: npc.sellable,
                    cost_price: Some(Money::from_cents(npc.cost_price_cents)),
                },
            );
            for p in &npc.secondaries {
                own_multi.positions.insert(
                    codes[p.stock_idx as usize].clone(),
                    PositionView {
                        qty: p.qty,
                        sellable_qty: p.sellable,
                        cost_price: Some(Money::from_cents(p.cost_price_cents)),
                    },
                );
            }
            own_multi.cash = Money::from_cents(npc.available_cash());
        }
        let own: &SelfView = if npc.secondaries.is_empty() {
            &own_single
        } else {
            &own_multi
        };
        let mut rng = SplitMix64::new(seed ^ tick ^ npc.id as u64);
        let intents = decide_data(&npc.strategy, market, own, &mut rng, game_cfg);
        if intents.is_empty() {
            continue;
        }
        stats.intents += intents.len() as u64;
        for intent in intents {
            let code = match &intent {
                Intent::PlaceLimit { code, .. }
                | Intent::PlaceMarket { code, .. }
                | Intent::Cancel { code, .. } => code,
            };
            let target = *code_idx.get(code.0.as_str()).ok_or_else(|| {
                ProtoError::InvalidConfig(format!("意图指向未知股票：{:?}", code.0))
            })?;
            records.push(XRecord {
                npc_idx: idx,
                target,
                intent,
            });
        }
    }
    Ok((records, stats))
}

/// 每 mask 的多股视图（快照口径：外股盘口 = 上一 tick 收束值，与本股
/// 决策时刻的活簿等新鲜——两臂的决策信息集一致，公平对照）。
/// `views_masks` 是名册中实际出现的持股掩码集合（静态）。
fn build_views(
    cfg: &ProtoConfig,
    snap: &MarketSnapshot,
    codes: &[StockCode],
    bands: &[(i64, i64)],
    views_masks: &[u8],
) -> Vec<Option<Arc<MarketView>>> {
    let n = cfg.stocks.len();
    let base: Vec<StockView> = (0..n)
        .map(|i| {
            let st = &snap.stocks[i];
            let (lower, upper) = bands[i];
            StockView {
                is_trading: true,
                best_bid: st.best_bid.map(Money::from_cents),
                best_ask: st.best_ask.map(Money::from_cents),
                last_price: Money::from_cents(st.last_price.max(0)),
                max_buy_price: Money::from_cents(upper),
                daily_upper_limit: Money::from_cents(upper),
                min_sell_price: Money::from_cents(lower),
                recent_prices: st.recent_prices.iter().map(|&c| Money::from_cents(c)).collect(),
                recent_market_minute_prices: st
                    .minute_prices
                    .iter()
                    .map(|&c| Money::from_cents(c))
                    .collect(),
                relative_volume: st.rel_volume,
                order_book_imbalance: st.imbalance,
            }
        })
        .collect();
    let mut out: Vec<Option<Arc<MarketView>>> = (0..32).map(|_| None).collect();
    for &mask in views_masks {
        let mut map = BTreeMap::new();
        for i in 0..n {
            if mask & (1 << i) != 0 {
                map.insert(codes[i].clone(), base[i].clone());
            }
        }
        out[mask as usize] = Some(Arc::new(MarketView {
            stocks: map,
            tick: snap.tick,
            market_minute: snap.market_minute,
        }));
    }
    out
}

/// 均匀切分（与按股私有臂同法）。
fn chunk_range(len: u32, threads: u32, slot: usize) -> (u32, u32) {
    let threads = threads.max(1);
    let base = len / threads;
    let rem = len % threads;
    let slot_u = slot as u32;
    let from = slot_u * base + rem.min(slot_u);
    let to = from + base + if slot_u < rem { 1 } else { 0 };
    (from, to)
}
