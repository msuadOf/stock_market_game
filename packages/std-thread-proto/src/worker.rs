//! StockWorker：一股一个持久线程（用户模型核心）。
//!
//! 每 tick：收 TickStart → spawn NPC 决策线程（或 persist 池/内联）→ join →
//! 本股撮合 → T+1 解冻 → TickDone 回传 coordinator。NPC 名册与订单簿是
//! worker 私有状态，除决策只读外全部单线程变更，无锁。

use crate::book::{Placement, StockBook};
use crate::npc::{FeeTable, Npc};
use crate::setup::{build_roster, ProtoConfig, StockSpec};
use crate::ProtoError;
use engine::account::StockCode;
use engine::money::Money;
use engine::session::SplitMix64;
use engine::strategy::{
    decide_data, LimitPrice, MarketView, PositionView, SelfView, StockView, Intent,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};

/// 全局（进程内）线程计量：决策线程的并发峰值（精确计数）。
pub struct ThreadGauge {
    current: AtomicI32,
    peak: AtomicI32,
}

impl ThreadGauge {
    pub fn new() -> Self {
        ThreadGauge {
            current: AtomicI32::new(0),
            peak: AtomicI32::new(0),
        }
    }

    fn enter(&self) {
        let now = self.current.fetch_add(1, Ordering::Relaxed) + 1;
        let mut p = self.peak.load(Ordering::Relaxed);
        while now > p {
            match self
                .peak
                .compare_exchange_weak(p, now, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => p = actual,
            }
        }
    }

    fn exit(&self) {
        self.current.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn peak(&self) -> i32 {
        self.peak.load(Ordering::Relaxed)
    }
}

/// coordinator 下行的市场快照（跨股公共视图，Arc 共享只读）。
pub struct MarketSnapshot {
    pub tick: u64,
    pub market_minute: u64,
    /// 与股票表 index 对齐的公共状态。
    pub stocks: Vec<StockPublicState>,
}

/// 单股公共状态（coordinator 维护的历史 + worker 上一 tick 结果）。
#[derive(Clone)]
pub struct StockPublicState {
    pub last_price: i64,
    /// 最近 N 个 tick 价（cap 20，tick 级观察用）。
    pub recent_prices: Vec<i64>,
    /// 已完成标准交易分钟的收盘价（cap 20，游资趋势用）。
    pub minute_prices: Vec<i64>,
    pub rel_volume: f64,
    /// 量能期望（EWMA），coordinator 内部用。
    pub expected_volume: f64,
}

/// coordinator → worker 消息。
pub enum TickMsg {
    Start(Arc<MarketSnapshot>),
    Shutdown,
}

/// worker → coordinator 的每 tick 结果。
pub struct TickDone {
    pub stock_idx: usize,
    pub last_price: i64,
    pub best_bid: Option<i64>,
    pub best_ask: Option<i64>,
    /// 本 tick 成交量（增量）。
    pub volume_delta: u64,
    pub trade_count_delta: u64,
    pub decide_calls: u64,
    pub intents: u64,
    pub resting: u64,
}

/// worker 退出时上交的期末状态（审计用）。
pub struct FinalState {
    pub stock_idx: usize,
    pub npcs: Vec<Npc>,
    pub fee_sink: i64,
    pub trade_count: u64,
    pub volume_shares: u64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub min_trade_price: Option<i64>,
    pub max_trade_price: Option<i64>,
    /// 切分零头（初始分配时的守恒桶）。
    pub float_remainder: u64,
}

/// 一条决策产物：NPC 名册索引 + 意图。
struct IntentRecord {
    npc_idx: u32,
    intent: Intent,
}

/// 决策 chunk 统计。
#[derive(Default, Clone, Copy)]
struct ChunkStats {
    decide_calls: u64,
    intents: u64,
}

/// persist 模式的常驻决策线程池（每股一个）。
/// Drop 时关闭派工通道并 join 全部线程，保证对 NPC 名册 Arc 的独占回收。
struct PersistPool {
    work_txs: Vec<Sender<PersistWork>>,
    result_rx: Receiver<PersistResult>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl Drop for PersistPool {
    fn drop(&mut self) {
        self.work_txs.clear(); // 关闭派工端 → 线程 recv 失败退出。
        for handle in self.handles.drain(..) {
            if handle.join().is_err() {
                // 线程 panic 必须显式上抛，不能静默吞（铁律二）。
                std::panic::panic_any("persist 决策线程 panic");
            }
        }
    }
}

struct PersistWork {
    market: Arc<MarketView>,
    npc_from: u32,
    npc_to: u32,
    tick: u64,
}

struct PersistResult {
    slot: usize,
    records: Vec<IntentRecord>,
    stats: ChunkStats,
}

/// worker 主函数：构造初始状态后进入消息循环。
#[allow(clippy::too_many_arguments)]
pub fn worker_main(
    cfg: Arc<ProtoConfig>,
    spec: StockSpec,
    stock_idx: usize,
    id_base: u32,
    game_cfg: Arc<engine::config::GameConfig>,
    rx: Receiver<TickMsg>,
    done_tx: Sender<TickDone>,
    final_tx: Sender<FinalState>,
    gauge: Arc<ThreadGauge>,
    per_stock_threads: u32,
    persist_mode: bool,
) -> Result<(), ProtoError> {
    let (npcs, float_remainder) = build_roster(&cfg, stock_idx, &spec, id_base)?;
    let code = StockCode(spec.code.clone());
    let npcs: Arc<RwLock<Vec<Npc>>> = Arc::new(RwLock::new(npcs));
    let mut book = StockBook::new();
    let fees = FeeTable::from_config(&game_cfg);
    // 涨跌停带：初始价 ±limit（单日口径，3000 tick 不跨日，见 design.md 简化登记）。
    let band = (spec.initial_price_cents as f64 * spec.limit_pct) as i64;
    let upper = spec.initial_price_cents + band;
    let lower = (spec.initial_price_cents - band).max(1);
    let mut last_price = spec.initial_price_cents;

    // persist 模式：起常驻决策线程。
    let pool = if persist_mode && per_stock_threads > 0 {
        Some(spawn_persist_pool(
            Arc::clone(&npcs),
            Arc::new(code.clone()),
            Arc::clone(&cfg),
            Arc::clone(&game_cfg),
            Arc::clone(&gauge),
            per_stock_threads,
        ))
    } else {
        None
    };

    let mut result: Result<(), ProtoError> = Ok(());
    while let Ok(msg) = rx.recv() {
        let snapshot = match msg {
            TickMsg::Start(s) => s,
            TickMsg::Shutdown => break,
        };
        let tick = snapshot.tick;
        let step = run_tick(
            &cfg,
            &game_cfg,
            &code,
            stock_idx,
            &npcs,
            &mut book,
            &fees,
            upper,
            lower,
            last_price,
            &snapshot,
            tick,
            per_stock_threads,
            pool.as_ref(),
            &gauge,
        );
        match step {
            Ok(done) => {
                last_price = done.last_price;
                if done_tx.send(done).is_err() {
                    result = Err(ProtoError::ChannelClosed("done_tx".into()));
                    break;
                }
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    // Drop pool：关闭派工通道并 join 常驻线程，之后名册 Arc 独占可回收。
    drop(pool);
    let npcs = Arc::try_unwrap(npcs)
        .map_err(|_| ProtoError::ChannelClosed("npcs 仍有引用".into()))?
        .into_inner()
        .map_err(|_| ProtoError::ChannelClosed("npcs 锁中毒".into()))?;
    let final_state = FinalState {
        stock_idx,
        npcs,
        fee_sink: book.fee_sink,
        trade_count: book.trade_count,
        volume_shares: book.volume_shares,
        skipped_orders: book.skipped_orders,
        expired_orders: book.expired_orders,
        cancelled_orders: book.cancelled_orders,
        min_trade_price: book.min_trade_price,
        max_trade_price: book.max_trade_price,
        float_remainder,
    };
    let send = final_tx.send(final_state);
    if result.is_ok() && send.is_err() {
        result = Err(ProtoError::ChannelClosed("final_tx".into()));
    }
    result
}

/// 单个 tick 的完整相位：决策 → 到期 → 意图应用（含撮合）→ T+1 → 回报。
#[allow(clippy::too_many_arguments)]
fn run_tick(
    cfg: &ProtoConfig,
    game_cfg: &engine::config::GameConfig,
    code: &StockCode,
    stock_idx: usize,
    npcs: &Arc<RwLock<Vec<Npc>>>,
    book: &mut StockBook,
    fees: &FeeTable,
    upper: i64,
    lower: i64,
    last_price: i64,
    snapshot: &Arc<MarketSnapshot>,
    tick: u64,
    per_stock_threads: u32,
    pool: Option<&PersistPool>,
    gauge: &Arc<ThreadGauge>,
) -> Result<TickDone, ProtoError> {
    // ── 相位 1：决策（只读共享名册）──
    let market = build_market_view(snapshot, stock_idx, code, book, upper, lower);
    let market = Arc::new(market);
    let trade_count_before = book.trade_count;
    let volume_before = book.volume_shares;
    let (records, stats) = if per_stock_threads == 0 {
        // 内联串行（width=0 基线臂）。
        let guard = npcs
            .read()
            .map_err(|_| ProtoError::ChannelClosed("npcs 读锁中毒".into()))?;
        decide_range(&guard, 0, guard.len() as u32, code, &market, game_cfg, cfg.seed, tick)
    } else if let Some(pool) = pool {
        // persist 池：派工 → 收结果。
        for (slot, tx) in pool.work_txs.iter().enumerate() {
            let (from, to) = chunk_range(npcs_len(npcs)?, per_stock_threads, slot);
            tx.send(PersistWork {
                market: Arc::clone(&market),
                npc_from: from,
                npc_to: to,
                tick,
            })
            .map_err(|_| ProtoError::ChannelClosed("persist work_tx".into()))?;
        }
        let mut all = Vec::with_capacity(pool.work_txs.len());
        for _ in 0..pool.work_txs.len() {
            let r = pool
                .result_rx
                .recv()
                .map_err(|_| ProtoError::ChannelClosed("persist result_rx".into()))?;
            all.push(r);
        }
        all.sort_by_key(|r| r.slot);
        let mut records = Vec::new();
        let mut stats = ChunkStats::default();
        for r in all {
            records.extend(r.records);
            stats.decide_calls += r.stats.decide_calls;
            stats.intents += r.stats.intents;
        }
        (records, stats)
    } else {
        // spawn 模式：std::thread::scope 现起现 join（用户原模型）。
        let len = npcs_len(npcs)?;
        let guard = npcs
            .read()
            .map_err(|_| ProtoError::ChannelClosed("npcs 读锁中毒".into()))?;
        let seed = cfg.seed;
        let mut joined: Vec<(usize, Vec<IntentRecord>, ChunkStats)> =
            Vec::with_capacity(per_stock_threads as usize);
        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(per_stock_threads as usize);
            for slot in 0..per_stock_threads as usize {
                let (from, to) = chunk_range(len, per_stock_threads, slot);
                let guard = &*guard;
                let market = &market;
                let code = code;
                let game_cfg = game_cfg;
                let gauge = gauge;
                handles.push(scope.spawn(move || {
                    gauge.enter();
                    let out = decide_range(
                        guard,
                        from,
                        to,
                        code,
                        market,
                        game_cfg,
                        seed,
                        tick,
                    );
                    gauge.exit();
                    out
                }));
            }
            for (slot, handle) in handles.into_iter().enumerate() {
                match handle.join() {
                    Ok((records, stats)) => joined.push((slot, records, stats)),
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
    };

    // ── 相位 2：挂单到期（写锁）──
    {
        let mut guard = write_npcs(npcs)?;
        book.expire_due(&mut guard, tick)?;
        // ── 相位 3：意图应用（工作单对齐 + 撮合）──
        apply_intents(&mut guard, book, records, upper, lower, fees, tick, cfg)?;
        // ── 相位 4：T+1 解冻 ──
        for npc in guard.iter_mut() {
            npc.unfreeze_t1();
        }
    }
    Ok(TickDone {
        stock_idx,
        last_price: book.last_price.unwrap_or(last_price),
        best_bid: book.best_bid(),
        best_ask: book.best_ask(),
        volume_delta: book.volume_shares - volume_before,
        trade_count_delta: book.trade_count - trade_count_before,
        decide_calls: stats.decide_calls,
        intents: stats.intents,
        resting: book.resting_orders(),
    })
}

/// 名册区间决策（所有模式共用的内核）。返回保持到达序（slot 升序、NPC 升序）。
fn decide_range(
    npcs: &[Npc],
    from: u32,
    to: u32,
    code: &StockCode,
    market: &Arc<MarketView>,
    game_cfg: &engine::config::GameConfig,
    seed: u64,
    tick: u64,
) -> (Vec<IntentRecord>, ChunkStats) {
    // SelfView 模板只建一次：每 NPC 就地更新字段，避免每 NPC 重建 BTreeMap。
    let mut positions = BTreeMap::new();
    positions.insert(
        code.clone(),
        PositionView {
            qty: 0,
            sellable_qty: 0,
            cost_price: None,
        },
    );
    let mut self_view = SelfView {
        cash: Money::ZERO,
        positions,
    };
    let mut records = Vec::new();
    let mut stats = ChunkStats::default();
    for idx in from..to {
        let npc = &npcs[idx as usize];
        stats.decide_calls += 1;
        {
            let pos = self_view
                .positions
                .get_mut(code)
                .expect("模板已插入本股键");
            pos.qty = npc.qty;
            pos.sellable_qty = npc.sellable;
            pos.cost_price = Some(Money::from_cents(npc.cost_price_cents));
        }
        self_view.cash = Money::from_cents(npc.available_cash());
        let mut rng = SplitMix64::new(seed ^ tick ^ npc.id as u64);
        let intents = decide_data(&npc.strategy, market, &self_view, &mut rng, game_cfg);
        if intents.is_empty() {
            continue;
        }
        stats.intents += intents.len() as u64;
        for intent in intents {
            records.push(IntentRecord {
                npc_idx: idx,
                intent,
            });
        }
    }
    (records, stats)
}

/// 应用意图：先做该 NPC 的工作单对齐（撤自己全部挂单），再解析价格并下单。
fn apply_intents(
    npcs: &mut [Npc],
    book: &mut StockBook,
    records: Vec<IntentRecord>,
    upper: i64,
    lower: i64,
    fees: &FeeTable,
    tick: u64,
    cfg: &ProtoConfig,
) -> Result<(), ProtoError> {
    for rec in records {
        // 引擎语义：决策复核的股票两侧工作单与最新目标对齐。
        book.cancel_all_for(npcs, rec.npc_idx)?;
        let resolved = match &rec.intent {
            Intent::PlaceLimit {
                side,
                price,
                qty,
                ..
            } => {
                let p = match price {
                    LimitPrice::Fixed(m) => m.cents(),
                    // Highest/Lowest 解析为对面最优（无对面则带边界）。
                    LimitPrice::Highest => match side {
                        engine::orderbook::Side::Buy => book
                            .best_ask()
                            .unwrap_or(upper)
                            .min(upper),
                        engine::orderbook::Side::Sell => upper,
                    },
                    LimitPrice::Lowest => match side {
                        engine::orderbook::Side::Sell => book
                            .best_bid()
                            .unwrap_or(lower)
                            .max(lower),
                        engine::orderbook::Side::Buy => lower,
                    },
                };
                Some((*side, p, *qty))
            }
            Intent::PlaceMarket { side, qty, .. } => {
                // 市价以涨跌停价作保护（对齐引擎语义）。
                let p = match side {
                    engine::orderbook::Side::Buy => upper,
                    engine::orderbook::Side::Sell => lower,
                };
                Some((*side, p, *qty))
            }
            // 策略层不产 Cancel（会话层职责）；出现即显式计数，不静默。
            Intent::Cancel { .. } => None,
        };
        let Some((side, price, qty)) = resolved else {
            continue;
        };
        if price < lower || price > upper {
            // 带外价格显式拒绝（防御式）。
            book.skipped_orders += 1;
            continue;
        }
        let outcome = book.place(
            npcs,
            rec.npc_idx,
            side,
            price,
            qty,
            tick,
            cfg.order_ttl_ticks,
            fees,
        )?;
        debug_assert!(matches!(
            outcome,
            Placement::FullyFilled | Placement::Rested | Placement::Skipped(_)
        ));
    }
    Ok(())
}

/// 组装本股单股 MarketView（快照历史 + 实时盘口）。
fn build_market_view(
    snapshot: &MarketSnapshot,
    stock_idx: usize,
    code: &StockCode,
    book: &StockBook,
    upper: i64,
    lower: i64,
) -> MarketView {
    let st = &snapshot.stocks[stock_idx];
    let view = StockView {
        is_trading: true,
        best_bid: book.best_bid().map(Money::from_cents),
        best_ask: book.best_ask().map(Money::from_cents),
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
        order_book_imbalance: book.top5_imbalance(),
    };
    let mut stocks = BTreeMap::new();
    stocks.insert(code.clone(), view);
    MarketView {
        stocks,
        tick: snapshot.tick,
        market_minute: snapshot.market_minute,
    }
}

fn npcs_len(npcs: &Arc<RwLock<Vec<Npc>>>) -> Result<u32, ProtoError> {
    let len = npcs
        .read()
        .map_err(|_| ProtoError::ChannelClosed("npcs 读锁中毒".into()))?
        .len() as u32;
    Ok(len)
}

fn write_npcs(npcs: &Arc<RwLock<Vec<Npc>>>) -> Result<std::sync::RwLockWriteGuard<'_, Vec<Npc>>, ProtoError> {
    npcs.write()
        .map_err(|_| ProtoError::ChannelClosed("npcs 写锁中毒".into()))
}

/// 均匀切分：把 len 个 NPC 分给 threads 个线程，返回 slot 的 [from, to)。
fn chunk_range(len: u32, threads: u32, slot: usize) -> (u32, u32) {
    let threads = threads.max(1);
    let base = len / threads;
    let rem = len % threads;
    let slot_u = slot as u32;
    let from = slot_u * base + rem.min(slot_u);
    let to = from + base + if slot_u < rem { 1 } else { 0 };
    (from, to)
}

/// 起 persist 模式常驻决策线程池（每股 per_stock_threads 个）。
fn spawn_persist_pool(
    npcs: Arc<RwLock<Vec<Npc>>>,
    code: Arc<StockCode>,
    cfg: Arc<ProtoConfig>,
    game_cfg: Arc<engine::config::GameConfig>,
    gauge: Arc<ThreadGauge>,
    per_stock_threads: u32,
) -> PersistPool {
    let (result_tx, result_rx) = std::sync::mpsc::channel::<PersistResult>();
    let mut work_txs = Vec::new();
    let mut handles = Vec::new();
    for slot in 0..per_stock_threads as usize {
        let (work_tx, work_rx) = std::sync::mpsc::channel::<PersistWork>();
        work_txs.push(work_tx);
        let npcs = Arc::clone(&npcs);
        let code = Arc::clone(&code);
        let cfg = Arc::clone(&cfg);
        let game_cfg = Arc::clone(&game_cfg);
        let gauge = Arc::clone(&gauge);
        let result_tx = result_tx.clone();
        handles.push(std::thread::spawn(move || {
            gauge.enter();
            while let Ok(work) = work_rx.recv() {
                let out = npcs
                    .read()
                    .map_err(|_| ProtoError::ChannelClosed("npcs 读锁中毒".into()))
                    .and_then(|guard| {
                        Ok(decide_range(
                            &guard,
                            work.npc_from,
                            work.npc_to,
                            &code,
                            &work.market,
                            &game_cfg,
                            cfg.seed,
                            work.tick,
                        ))
                    });
                match out {
                    Ok((records, stats)) => {
                        if result_tx
                            .send(PersistResult {
                                slot,
                                records,
                                stats,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            gauge.exit();
        }));
    }
    PersistPool {
        work_txs,
        result_rx,
        handles,
    }
}
