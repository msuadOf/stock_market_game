//! 跨股臂 coordinator：tick 驱动、三相位中转（Sol 建议先测中转模型）、
//! 指标采样、期末审计。
//!
//! 每 tick 三次全局交换（对比按股私有臂的一次收束）：
//! 1. 下发 Start → 收齐 Phase1（路由操作）；
//! 2. 按目标股分拣（先撤后挂）下发 Route → 收齐 Phase2（终态 + 回执）；
//!    收束安装公共状态（与按股私有臂同法）；
//! 3. 按家端分拣回执下发 Receipts。相位 3 无屏障（家端 FIFO 通道保证
//!    回执先于下一 Start 处理，家端账户只在自家线程变更）。
//!
//! 相位等待时间与分拣/发送时间是「路由税/同步税」的直接计量。

use crate::setup::ProtoConfig;
use crate::worker::{MarketSnapshot, StockPublicState, ThreadGauge, TickDone};
use crate::xworker::{xworker_main, RoutedOp, XMsg, XUp};
use crate::book::FillReceipt;
use crate::ProtoError;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 跨股臂运行报告。
#[derive(Clone, Debug)]
pub struct XRunReport {
    // 公共指标（口径与按股私有臂 RunReport 一致）。
    pub ticks: u64,
    pub wall_secs: f64,
    pub cpu_secs: f64,
    pub cpu_ms_per_tick: f64,
    pub ticks_per_sec: f64,
    pub peak_threads: u32,
    pub peak_decision_threads: i32,
    pub per_stock_threads: u32,
    pub decide_calls: u64,
    pub intents: u64,
    pub trades: u64,
    pub volume_shares: u64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub audit: XAuditReport,
    // 跨股专属。
    /// 跨股持仓率（构造参数，0~100 整数百分比）。
    pub cross_pct: u32,
    /// 实际持 ≥2 只股的 retail 占比。
    pub cross_fraction: f64,
    /// 按次级仓数量分布的 retail 数（下标 0=只持主股，1=持 2 只，2=持 3 只）。
    pub hold_counts: [u64; 4],
    pub routed_places: u64,
    pub routed_cancels: u64,
    pub spurious_cancels: u64,
    pub foreign_skips: u64,
    pub cancel_intents: u64,
    /// BuyFill / SellFill / BuyClosed / SellClosed 应用次数。
    pub receipts_applied: [u64; 4],
    pub desks_peak: u64,
    /// 相位 1 等待（Start 下发 → 收齐 Phase1）均值，ms/tick。
    pub phase1_wait_ms: f64,
    /// 相位 2 等待（Route 下发 → 收齐 Phase2）均值，ms/tick。
    pub phase2_wait_ms: f64,
    /// 分拣 + 发送（路由/回执）均值，ms/tick。
    pub dispatch_ms: f64,
}

/// 跨股臂期末审计。
#[derive(Clone, Debug)]
pub struct XAuditReport {
    pub initial_total_cash_cents: i64,
    pub final_total_cash_cents: i64,
    pub stocks: Vec<XStockAudit>,
    /// 家端账户级违规（全局计数）。
    pub negative_cash_accounts: u64,
    pub t1_violations: u64,
    pub lot_violations: u64,
}

#[derive(Clone, Debug)]
pub struct XStockAudit {
    pub code: String,
    pub float_total_shares: u64,
    pub float_remainder_shares: u64,
    pub held_shares: u64,
    pub lower_limit_cents: i64,
    pub upper_limit_cents: i64,
    pub min_trade_price_cents: i64,
    pub max_trade_price_cents: i64,
    pub trade_count: u64,
}

/// 主入口：跑跨股臂基准并做期末守恒审计。
pub fn run_cross_benchmark(
    cfg: &ProtoConfig,
    width: u32,
    cross_pct: u32,
) -> Result<XRunReport, ProtoError> {
    if cfg.stocks.is_empty() || cfg.ticks == 0 {
        return Err(ProtoError::InvalidConfig(
            "stocks 与 ticks 必须非空".into(),
        ));
    }
    let stocks = cfg.stocks.len() as u32;
    let per_stock_threads = if width == 0 { 0 } else { (width / stocks).max(1) };

    // 与 default-setup.json 对齐的权威费用/整手配置（与按股私有臂同参）。
    let game_cfg = Arc::new(
        engine::config::GameConfig::new(
            0.00025,
            engine::money::Money::from_cents(500),
            0.0005,
            0.10,
            0.10,
            true,
            100,
            engine::money::Money::from_cents(1_000_000_000_000),
        )
        .map_err(|e| ProtoError::InvalidConfig(format!("GameConfig：{e}")))?,
    );

    let gauge = Arc::new(ThreadGauge::new());
    let (up_tx, up_rx) = channel::<XUp>();
    let cfg = Arc::new(cfg.clone());

    let mut txs = Vec::with_capacity(stocks as usize);
    let mut handles = Vec::with_capacity(stocks as usize);
    for stock_idx in 0..stocks as usize {
        let (tx, rx) = channel::<XMsg>();
        txs.push(tx);
        let handle = std::thread::Builder::new()
            .name(format!("xstock-{}", cfg.stocks[stock_idx].code))
            .spawn({
                let cfg = Arc::clone(&cfg);
                let game_cfg = Arc::clone(&game_cfg);
                let gauge = Arc::clone(&gauge);
                let up_tx = up_tx.clone();
                move || {
                    if crate::xdbg() {
                        eprintln!("[w{stock_idx}] 线程闭包启动");
                    }
                    let r = xworker_main(
                        cfg,
                        stock_idx,
                        game_cfg,
                        cross_pct,
                        rx,
                        up_tx,
                        gauge,
                        per_stock_threads,
                    );
                    if crate::xdbg() {
                        eprintln!("[w{stock_idx}] 线程退出：{r:?}");
                    }
                    if let Err(e) = r {
                        // 线程内失败必须显式可见（铁律二）：panic 上抛让 join 收到。
                        std::panic::panic_any(format!("xworker {stock_idx} 失败：{e}"));
                    }
                }
            })
            .map_err(|e| ProtoError::ChannelClosed(format!("spawn xworker：{e}")))?;
        handles.push(handle);
    }
    drop(up_tx);

    // 初始公共状态（与按股私有臂同法）。
    let mut public: Vec<StockPublicState> = cfg
        .stocks
        .iter()
        .map(|spec| StockPublicState {
            last_price: spec.initial_price_cents,
            recent_prices: Vec::new(),
            minute_prices: Vec::new(),
            rel_volume: 1.0,
            expected_volume: (spec.float_shares as f64) * 2e-4,
            best_bid: None,
            best_ask: None,
            imbalance: 0.0,
        })
        .collect();

    let cpu_start = read_cpu_secs();
    let wall_start = Instant::now();
    let mut peak_threads: u32 = 0;
    let mut stats_decide = 0u64;
    let mut stats_intents = 0u64;
    let mut t_phase1_wait = Duration::ZERO;
    let mut t_phase2_wait = Duration::ZERO;
    let mut t_dispatch = Duration::ZERO;

    for tick in 1..=cfg.ticks {
        if crate::xdbg() {
            eprintln!("[coord] tick {tick} 发送 Start");
        }
        // ── 相位 1：下发 Start，收齐路由操作 ──
        let snapshot = Arc::new(MarketSnapshot {
            tick,
            market_minute: tick / 60,
            stocks: public.clone(),
        });
        for tx in &txs {
            tx.send(XMsg::Start(Arc::clone(&snapshot)))
                .map_err(|_| ProtoError::ChannelClosed("x Start".into()))?;
        }
        let t0 = Instant::now();
        let mut all_ops: Vec<RoutedOp> = Vec::new();
        for _ in 0..txs.len() {
            match up_rx
                .recv()
                .map_err(|_| ProtoError::ChannelClosed("x Phase1".into()))?
            {
                XUp::Phase1(ops) => all_ops.extend(ops),
                XUp::Phase2(..) | XUp::DrainReceipts(..) | XUp::Final(..) => {
                    return Err(ProtoError::ChannelClosed(
                        "相位 1 期望 Phase1，收到其它上行消息（实现 bug）".into(),
                    ))
                }
            }
        }
        let w1 = t0.elapsed();
        if crate::xdbg() {
            eprintln!("[coord] tick {tick} 收齐 Phase1（ops={}）", all_ops.len());
        }
        // ── 分拣：按目标股，先撤后挂（保持各家端内部顺序）──
        let t1 = Instant::now();
        let mut routes: Vec<Vec<RoutedOp>> = (0..txs.len()).map(|_| Vec::new()).collect();
        for op in all_ops.iter().filter(|o| matches!(o, RoutedOp::CancelAll { .. })) {
            routes[op.target() as usize].push(*op);
        }
        for op in all_ops.iter().filter(|o| matches!(o, RoutedOp::Place { .. })) {
            routes[op.target() as usize].push(*op);
        }
        for (target, ops) in routes.into_iter().enumerate() {
            txs[target]
                .send(XMsg::Route(ops))
                .map_err(|_| ProtoError::ChannelClosed("x Route".into()))?;
        }
        let d1 = t1.elapsed();
        if crate::xdbg() {
            eprintln!("[coord] tick {tick} 已下发 Route");
        }
        // ── 相位 2：收齐终态 + 回执，收束安装 ──
        let t2 = Instant::now();
        let mut receipts: Vec<FillReceipt> = Vec::new();
        for _ in 0..txs.len() {
            match up_rx
                .recv()
                .map_err(|_| ProtoError::ChannelClosed("x Phase2".into()))?
            {
                XUp::Phase2(done, recs) => {
                    install_done(&mut public, &done, tick);
                    stats_decide += done.decide_calls;
                    stats_intents += done.intents;
                    receipts.extend(recs);
                }
                XUp::Phase1(..) | XUp::DrainReceipts(..) | XUp::Final(..) => {
                    return Err(ProtoError::ChannelClosed(
                        "相位 2 期望 Phase2，收到其它上行消息（实现 bug）".into(),
                    ))
                }
            }
        }
        let w2 = t2.elapsed();
        if crate::xdbg() {
            eprintln!("[coord] tick {tick} 收齐 Phase2（receipts={}）", receipts.len());
        }
        // ── 分拣回执：按家端下发（相位 3 无屏障）──
        let t3 = Instant::now();
        let mut homes: Vec<Vec<FillReceipt>> = (0..txs.len()).map(|_| Vec::new()).collect();
        for r in receipts {
            homes[r.home() as usize].push(r);
        }
        for (home, rs) in homes.into_iter().enumerate() {
            txs[home]
                .send(XMsg::Receipts(rs))
                .map_err(|_| ProtoError::ChannelClosed("x Receipts".into()))?;
        }
        let d2 = t3.elapsed();
        t_phase1_wait += w1;
        t_phase2_wait += w2;
        t_dispatch += d1 + d2;
        peak_threads = peak_threads.max(read_thread_count());
    }
    let wall_secs = wall_start.elapsed().as_secs_f64();
    let cpu_secs = read_cpu_secs() - cpu_start;

    // ── Drain：强撤外部挂单，回执归位后审计才可闭合 ──
    for tx in &txs {
        tx.send(XMsg::Drain)
            .map_err(|_| ProtoError::ChannelClosed("x Drain".into()))?;
    }
    let mut drained: Vec<FillReceipt> = Vec::new();
    for _ in 0..txs.len() {
        match up_rx
            .recv()
            .map_err(|_| ProtoError::ChannelClosed("x DrainReceipts".into()))?
        {
            XUp::DrainReceipts(rs) => drained.extend(rs),
            XUp::Phase1(..) | XUp::Phase2(..) | XUp::Final(..) => {
                return Err(ProtoError::ChannelClosed(
                    "Drain 期望 DrainReceipts，收到其它上行消息（实现 bug）".into(),
                ))
            }
        }
    }
    let mut homes: Vec<Vec<FillReceipt>> = (0..txs.len()).map(|_| Vec::new()).collect();
    for r in drained {
        homes[r.home() as usize].push(r);
    }
    for (home, rs) in homes.into_iter().enumerate() {
        txs[home]
            .send(XMsg::Receipts(rs))
            .map_err(|_| ProtoError::ChannelClosed("x Drain Receipts".into()))?;
    }
    // 关停 worker 并收期末状态。
    for tx in &txs {
        tx.send(XMsg::Shutdown)
            .map_err(|_| ProtoError::ChannelClosed("x Shutdown".into()))?;
    }
    let mut finals: Vec<Option<crate::xworker::XFinalState>> =
        (0..txs.len()).map(|_| None).collect();
    for _ in 0..txs.len() {
        match up_rx
            .recv()
            .map_err(|_| ProtoError::ChannelClosed("x Final".into()))?
        {
            XUp::Final(f) => {
                let idx = f.stock_idx;
                finals[idx] = Some(*f);
            }
            XUp::Phase1(..) | XUp::Phase2(..) | XUp::DrainReceipts(..) => {
                return Err(ProtoError::ChannelClosed(
                    "关停期望 Final，收到其它上行消息（实现 bug）".into(),
                ))
            }
        }
    }
    for (i, handle) in handles.into_iter().enumerate() {
        // 线程内失败以 panic 上抛（闭包保证），join Err 即显式失败。
        handle
            .join()
            .map_err(|_| ProtoError::ChannelClosed(format!("xworker {i} panic/失败")))?;
    }

    // ── 期末审计：守恒 + 不变量（显式失败，不静默）──
    let initial_total_cash = cfg.initial_total_cash_cents();
    let mut final_total_cash = 0i64;
    let mut stats_trades = 0u64;
    let mut stats_volume = 0u64;
    let mut stats_skipped = 0u64;
    let mut stats_expired = 0u64;
    let mut stats_cancelled = 0u64;
    let mut negative_cash = 0u64;
    let mut t1_violations = 0u64;
    let mut lot_violations = 0u64;
    let mut routed_places = 0u64;
    let mut routed_cancels = 0u64;
    let mut spurious_cancels = 0u64;
    let mut foreign_skips = 0u64;
    let mut cancel_intents = 0u64;
    let mut receipts_applied = [0u64; 4];
    let mut desks_peak = 0u64;
    let mut hold_counts = [0u64; 4];
    // 每股持仓聚合（主仓在家端 own 股，次级仓按 stock_idx 归股）。
    let mut held_per_stock = vec![0u64; cfg.stocks.len()];
    let mut remainder_per_stock = vec![0u64; cfg.stocks.len()];
    for final_state in finals.iter() {
        let f = final_state
            .as_ref()
            .ok_or_else(|| ProtoError::ChannelClosed("xworker 无期末状态".into()))?;
        stats_trades += f.trade_count;
        stats_volume += f.volume_shares;
        stats_skipped += f.skipped_orders;
        stats_expired += f.expired_orders;
        stats_cancelled += f.cancelled_orders;
        routed_places += f.routed_places;
        routed_cancels += f.routed_cancels;
        spurious_cancels += f.spurious_cancels;
        foreign_skips += f.foreign_skips;
        cancel_intents += f.cancel_intents;
        for i in 0..4 {
            receipts_applied[i] += f.receipts_applied[i];
        }
        desks_peak = desks_peak.max(f.desks_peak);
        for (i, &c) in f.setup.hold_counts.iter().enumerate() {
            hold_counts[i] += c;
        }
        remainder_per_stock[f.stock_idx] = f.float_remainder;
        for npc in &f.roster {
            final_total_cash += npc.cash;
            if npc.cash < 0 {
                negative_cash += 1;
            }
            if !npc.x_t1_ok() {
                t1_violations += 1;
            }
            if npc.qty % 100 != 0 {
                lot_violations += 1;
            }
            for pos in &npc.secondaries {
                if pos.qty % 100 != 0 {
                    lot_violations += 1;
                }
            }
            held_per_stock[f.stock_idx] += npc.qty as u64;
            for pos in &npc.secondaries {
                held_per_stock[pos.stock_idx as usize] += pos.qty as u64;
            }
        }
        final_total_cash += f.fee_sink;
    }
    if final_total_cash != initial_total_cash {
        return Err(ProtoError::CashConservation {
            initial: initial_total_cash,
            final_: final_total_cash,
        });
    }
    let mut stock_audits = Vec::with_capacity(cfg.stocks.len());
    for (s, spec) in cfg.stocks.iter().enumerate() {
        if crate::xdbg() {
            let per_home: Vec<String> = finals
                .iter()
                .enumerate()
                .map(|(h, f)| {
                    let primary = if f.as_ref().is_some_and(|f| f.stock_idx == s) {
                        f.as_ref()
                            .map(|f| f.roster.iter().map(|n| n.qty as u64).sum::<u64>())
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    let secondaries = f
                        .as_ref()
                        .map(|f| {
                            f.roster
                                .iter()
                                .flat_map(|n| n.secondaries.iter())
                                .filter(|p| p.stock_idx as usize == s)
                                .map(|p| p.qty as u64)
                                .sum::<u64>()
                        })
                        .unwrap_or(0);
                    format!("h{h}:p{primary}+s{secondaries}")
                })
                .collect();
            eprintln!(
                "[audit] stock {s}({}) held={} remainder={} 每家端贡献 {}",
                spec.code, held_per_stock[s], remainder_per_stock[s],
                per_home.join(" ")
            );
        }
        let band = (spec.initial_price_cents as f64 * spec.limit_pct) as i64;
        let held = held_per_stock[s];
        let remainder = remainder_per_stock[s];
        if held + remainder != spec.float_shares {
            return Err(ProtoError::ShareConservation {
                code: spec.code.clone(),
                held,
                remainder,
                total: spec.float_shares,
            });
        }
        let f = finals[s].as_ref().expect("期末状态已收齐");
        stock_audits.push(XStockAudit {
            code: spec.code.clone(),
            float_total_shares: spec.float_shares,
            float_remainder_shares: remainder,
            held_shares: held,
            lower_limit_cents: spec.initial_price_cents - band,
            upper_limit_cents: spec.initial_price_cents + band,
            min_trade_price_cents: f.min_trade_price.unwrap_or(0),
            max_trade_price_cents: f.max_trade_price.unwrap_or(0),
            trade_count: f.trade_count,
        });
        if f.trade_count > 0 {
            if f.min_trade_price.unwrap_or(0) < spec.initial_price_cents - band
                || f.max_trade_price.unwrap_or(0) > spec.initial_price_cents + band
            {
                return Err(ProtoError::InvalidConfig(format!(
                    "股票 {} 成交价越界：[{}, {}] 不在 [{}, {}]",
                    spec.code,
                    f.min_trade_price.unwrap_or(0),
                    f.max_trade_price.unwrap_or(0),
                    spec.initial_price_cents - band,
                    spec.initial_price_cents + band
                )));
            }
        }
    }
    let retail_total: u64 = cfg.retail_per_stock as u64 * cfg.stocks.len() as u64;
    let hold_2_or_3 = hold_counts[1] + hold_counts[2] + hold_counts[3];
    let ticks_f = cfg.ticks as f64;
    Ok(XRunReport {
        ticks: cfg.ticks,
        wall_secs,
        cpu_secs,
        cpu_ms_per_tick: cpu_secs * 1000.0 / ticks_f,
        ticks_per_sec: ticks_f / wall_secs,
        peak_threads,
        peak_decision_threads: gauge.peak(),
        per_stock_threads,
        decide_calls: stats_decide,
        intents: stats_intents,
        trades: stats_trades,
        volume_shares: stats_volume,
        skipped_orders: stats_skipped,
        expired_orders: stats_expired,
        cancelled_orders: stats_cancelled,
        audit: XAuditReport {
            initial_total_cash_cents: initial_total_cash,
            final_total_cash_cents: final_total_cash,
            stocks: stock_audits,
            negative_cash_accounts: negative_cash,
            t1_violations,
            lot_violations,
        },
        cross_pct,
        cross_fraction: if retail_total > 0 {
            hold_2_or_3 as f64 / retail_total as f64
        } else {
            0.0
        },
        hold_counts,
        routed_places,
        routed_cancels,
        spurious_cancels,
        foreign_skips,
        cancel_intents,
        receipts_applied,
        desks_peak,
        phase1_wait_ms: t_phase1_wait.as_secs_f64() * 1000.0 / ticks_f,
        phase2_wait_ms: t_phase2_wait.as_secs_f64() * 1000.0 / ticks_f,
        dispatch_ms: t_dispatch.as_secs_f64() * 1000.0 / ticks_f,
    })
}

/// 收束安装（与按股私有臂 run.rs 同法 + 盘口快照字段）。
fn install_done(public: &mut [StockPublicState], done: &TickDone, tick: u64) {
    let st = &mut public[done.stock_idx];
    st.last_price = done.last_price;
    st.best_bid = done.best_bid;
    st.best_ask = done.best_ask;
    st.imbalance = done.imbalance;
    st.recent_prices.push(done.last_price);
    if st.recent_prices.len() > 20 {
        st.recent_prices.remove(0);
    }
    if tick % 60 == 0 {
        st.minute_prices.push(done.last_price);
        if st.minute_prices.len() > 20 {
            st.minute_prices.remove(0);
        }
    }
    let vol = done.volume_delta as f64;
    st.rel_volume = (vol / st.expected_volume.max(1.0)).clamp(0.0, 4.0);
    st.expected_volume = st.expected_volume * 0.95 + vol * 0.05;
}

/// 进程 CPU 时间（utime+stime，秒）。与 run.rs 同口径。
fn read_cpu_secs() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat 可读");
    let pos = stat.rfind(')').expect("stat 含 comm 括号");
    let fields: Vec<&str> = stat[pos + 1..].split_whitespace().collect();
    let utime: u64 = fields[11].parse().expect("utime 数字");
    let stime: u64 = fields[12].parse().expect("stime 数字");
    (utime + stime) as f64 / 100.0
}

/// 进程当前线程数（/proc/self/status Threads:）。
fn read_thread_count() -> u32 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status 可读");
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Threads:") {
            return rest.trim().parse().expect("Threads 数字");
        }
    }
    unreachable!("/proc/self/status 必含 Threads 字段")
}
