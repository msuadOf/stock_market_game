//! coordinator：tick 驱动、广播/收束安装、指标采样、期末审计。
//!
//! 每 tick：组装 Arc<MarketSnapshot> → 广播 5 个 worker → recv 5 个 TickDone →
//! 单线程更新公共历史（收束安装点）→ 下一 tick。这是用户模型的串行脊，
//! 其成本直接决定吞吐上限（与 spine 的串行脊同理）。

use crate::npc::Npc;
use crate::setup::ProtoConfig;
use crate::worker::{
    worker_main, FinalState, MarketSnapshot, StockPublicState, ThreadGauge, TickDone, TickMsg,
};
use crate::ProtoError;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Instant;

/// 决策线程组织方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// 用户原模型：每 tick std::thread::scope 现起现 join。
    Spawn,
    /// 对照臂：常驻决策线程池（测线程起停成本差）。
    Persist,
}

/// 运行报告（指标 + 审计）。
#[derive(Clone, Debug)]
pub struct RunReport {
    pub ticks: u64,
    pub wall_secs: f64,
    pub cpu_secs: f64,
    pub cpu_ms_per_tick: f64,
    pub ticks_per_sec: f64,
    /// 进程级线程数采样峰值（含主线程 / worker / 决策线程）。
    pub peak_threads: u32,
    /// 决策线程并发峰值（原子精确计数）。
    pub peak_decision_threads: i32,
    /// 每股决策线程数。
    pub per_stock_threads: u32,
    pub decide_calls: u64,
    pub intents: u64,
    pub trades: u64,
    pub volume_shares: u64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub audit: AuditReport,
}

/// 期末审计（守恒与不变量）。
#[derive(Clone, Debug)]
pub struct AuditReport {
    pub initial_total_cash_cents: i64,
    pub final_total_cash_cents: i64,
    pub stocks: Vec<StockAudit>,
}

#[derive(Clone, Debug)]
pub struct StockAudit {
    pub code: String,
    pub float_total_shares: u64,
    pub float_remainder_shares: u64,
    pub held_shares: u64,
    pub lower_limit_cents: i64,
    pub upper_limit_cents: i64,
    pub min_trade_price_cents: i64,
    pub max_trade_price_cents: i64,
    pub trade_count: u64,
    pub negative_cash_accounts: u64,
    pub t1_violations: u64,
    pub lot_violations: u64,
}

/// 主入口：跑完整基准并做期末守恒审计。
pub fn run_benchmark(
    cfg: &ProtoConfig,
    mode: RunMode,
    width: u32,
) -> Result<RunReport, ProtoError> {
    if cfg.stocks.is_empty() || cfg.ticks == 0 {
        return Err(ProtoError::InvalidConfig(
            "stocks 与 ticks 必须非空".into(),
        ));
    }
    let stocks = cfg.stocks.len() as u32;
    let per_stock_threads = if width == 0 {
        0
    } else {
        (width / stocks).max(1)
    };
    let persist_mode = mode == RunMode::Persist;

    // 与 default-setup.json 对齐的权威费用/整手配置。
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
    let (done_tx, done_rx) = channel::<TickDone>();
    let (final_tx, final_rx) = channel::<FinalState>();
    let cfg = Arc::new(cfg.clone());

    // 起持久 StockWorker（每股一个）。
    let mut tick_txs = Vec::with_capacity(stocks as usize);
    let mut handles = Vec::with_capacity(stocks as usize);
    let mut id_base: u32 = 0;
    for (stock_idx, spec) in cfg.stocks.iter().enumerate() {
        let (tx, rx) = channel::<TickMsg>();
        tick_txs.push(tx);
        let handle = std::thread::Builder::new()
            .name(format!("stock-{}", spec.code))
            .spawn({
                let cfg = Arc::clone(&cfg);
                let spec = spec.clone();
                let game_cfg = Arc::clone(&game_cfg);
                let gauge = Arc::clone(&gauge);
                let done_tx = done_tx.clone();
                let final_tx = final_tx.clone();
                move || {
                    worker_main(
                        cfg,
                        spec,
                        stock_idx,
                        id_base,
                        game_cfg,
                        rx,
                        done_tx,
                        final_tx,
                        gauge,
                        per_stock_threads,
                        persist_mode,
                    )
                }
            })
            .map_err(|e| ProtoError::ChannelClosed(format!("spawn worker：{e}")))?;
        id_base += cfg.retail_per_stock + cfg.inst_per_stock + 1; // hot 槽位预留
        handles.push(handle);
    }
    drop(done_tx);
    drop(final_tx);

    // 初始公共状态。
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

    for tick in 1..=cfg.ticks {
        let snapshot = Arc::new(MarketSnapshot {
            tick,
            market_minute: tick / 60,
            stocks: public.clone(),
        });
        for tx in &tick_txs {
            tx.send(TickMsg::Start(Arc::clone(&snapshot)))
                .map_err(|_| ProtoError::ChannelClosed("tick_tx".into()))?;
        }
        // 收束：recv 全部 worker 的 TickDone（固定 5 次，逐个安装）。
        for _ in 0..tick_txs.len() {
            let done = done_rx
                .recv()
                .map_err(|_| ProtoError::ChannelClosed("done_rx".into()))?;
            let st = &mut public[done.stock_idx];
            st.last_price = done.last_price;
            // 盘口快照安装（跨股臂外股视图读取；本臂填充但不消费）。
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
            // 量能期望 EWMA + 相对量。
            let vol = done.volume_delta as f64;
            st.rel_volume = (vol / st.expected_volume.max(1.0)).clamp(0.0, 4.0);
            st.expected_volume = st.expected_volume * 0.95 + vol * 0.05;
            stats_decide += done.decide_calls;
            stats_intents += done.intents;
        }
        peak_threads = peak_threads.max(read_thread_count());
    }
    let wall_secs = wall_start.elapsed().as_secs_f64();
    let cpu_secs = read_cpu_secs() - cpu_start;

    // 关停 worker 并收期末状态。
    for tx in &tick_txs {
        tx.send(TickMsg::Shutdown)
            .map_err(|_| ProtoError::ChannelClosed("shutdown".into()))?;
    }
    let mut finals: Vec<Option<FinalState>> =
        (0..tick_txs.len()).map(|_| None).collect();
    for _ in 0..tick_txs.len() {
        let f = final_rx
            .recv()
            .map_err(|_| ProtoError::ChannelClosed("final_rx".into()))?;
        let idx = f.stock_idx;
        finals[idx] = Some(f);
    }
    for handle in handles {
        handle
            .join()
            .map_err(|_| ProtoError::ChannelClosed("worker panic".into()))?
            .map_err(|e| e)?;
    }
    // 期末累计统计（以 FinalState 为权威）。
    let mut stats_trades = 0u64;
    let mut stats_volume = 0u64;
    let mut stats_skipped = 0u64;
    let mut stats_expired = 0u64;
    let mut stats_cancelled = 0u64;
    for f in finals.iter().flatten() {
        stats_trades += f.trade_count;
        stats_volume += f.volume_shares;
        stats_skipped += f.skipped_orders;
        stats_expired += f.expired_orders;
        stats_cancelled += f.cancelled_orders;
    }

    // ── 期末审计：守恒 + 不变量（显式失败，不静默）──
    let initial_total_cash = cfg.initial_total_cash_cents();
    let mut final_total_cash = 0i64;
    let mut stock_audits = Vec::with_capacity(finals.len());
    for (stock_idx, final_state) in finals.iter().enumerate() {
        let f = final_state
            .as_ref()
            .ok_or_else(|| ProtoError::ChannelClosed(format!("stock {stock_idx} 无期末状态")))?;
        let FinalState {
            npcs,
            fee_sink,
            trade_count,
            min_trade_price,
            max_trade_price,
            float_remainder,
            ..
        } = f;
        let mut held: u64 = 0;
        let mut negative_cash = 0u64;
        let mut t1_violations = 0u64;
        let mut lot_violations = 0u64;
        for npc in npcs {
            final_total_cash += npc.cash;
            held += npc.qty as u64;
            if npc.cash < 0 {
                negative_cash += 1;
            }
            if !npc.t1_ok() {
                t1_violations += 1;
            }
            if npc.qty % 100 != 0 {
                lot_violations += 1;
            }
        }
        final_total_cash += fee_sink;
        let spec = &cfg.stocks[stock_idx];
        let band = (spec.initial_price_cents as f64 * spec.limit_pct) as i64;
        let total = held + float_remainder;
        if total != spec.float_shares {
            return Err(ProtoError::ShareConservation {
                code: spec.code.clone(),
                held,
                remainder: *float_remainder,
                total: spec.float_shares,
            });
        }
        stock_audits.push(StockAudit {
            code: spec.code.clone(),
            float_total_shares: spec.float_shares,
            float_remainder_shares: *float_remainder,
            held_shares: held,
            lower_limit_cents: spec.initial_price_cents - band,
            upper_limit_cents: spec.initial_price_cents + band,
            min_trade_price_cents: min_trade_price.unwrap_or(0),
            max_trade_price_cents: max_trade_price.unwrap_or(0),
            trade_count: *trade_count,
            negative_cash_accounts: negative_cash,
            t1_violations,
            lot_violations,
        });
    }
    if final_total_cash != initial_total_cash {
        return Err(ProtoError::CashConservation {
            initial: initial_total_cash,
            final_: final_total_cash,
        });
    }

    Ok(RunReport {
        ticks: cfg.ticks,
        wall_secs,
        cpu_secs,
        cpu_ms_per_tick: cpu_secs * 1000.0 / cfg.ticks as f64,
        ticks_per_sec: cfg.ticks as f64 / wall_secs,
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
        audit: AuditReport {
            initial_total_cash_cents: initial_total_cash,
            final_total_cash_cents: final_total_cash,
            stocks: stock_audits,
        },
    })
}

/// 进程 CPU 时间（utime+stime，秒）。/proc/self/stat 字段 14/15，CLK_TCK=100（Linux 标准）。
fn read_cpu_secs() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat 可读");
    let pos = stat.rfind(')').expect("stat 含 comm 括号");
    let fields: Vec<&str> = stat[pos + 1..].split_whitespace().collect();
    // fields[0] = state(字段3)，utime=字段14 → 下标 11，stime=字段15 → 下标 12。
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

/// NPC 计数辅助（测试用）。
pub fn npc_total(cfg: &ProtoConfig) -> u32 {
    let stocks = cfg.stocks.len() as u32;
    stocks * (cfg.retail_per_stock + cfg.inst_per_stock) + cfg.hot_total
}

/// 允许测试直接检查 Npc 结构。
pub fn audit_npc_t1(npc: &Npc) -> bool {
    npc.t1_ok()
}
