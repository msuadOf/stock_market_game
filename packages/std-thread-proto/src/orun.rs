//! 所有权转移臂 coordinator：tick 驱动、收束安装、指标采样、期末审计。
//!
//! 每 tick 一次全局交换（对比跨股臂的三相位中转）：
//! 1. 下发 Start → 组线程（G 个）内联决策 + 实体扣除，批次**直发**目标股
//!    交易部（不过 coordinator），完成后上交 GroupStats；
//! 2. 交易部（一股一线程）收齐全部组的 final 标记后 flush 实体批次回
//!    组线程、上交 DeskDone；
//! 3. coordinator 收齐 G GroupStats + N DeskDone，安装公共状态 → 下一 tick。
//! 数据面（订单/实体）全程不经 coordinator——这是与跨股中转臂的结构性差异。
//!
//! 收尾：Drain（交易部强撤全部挂单，实体回流）→ Shutdown（组线程落完
//! 全部在途实体后上交期末名册）→ 审计（守恒 + 不变量，显式失败不静默）。

use crate::npc::Npc;
use crate::oworker::{
    desk_main, group_main, DeskFinal, DeskMsg, EntCounters, GroupFinal, GroupIn, OwnShared, OwnUp,
};
use crate::setup::ProtoConfig;
use crate::worker::{MarketSnapshot, StockPublicState, TickDone};
use crate::xsetup::build_cross_roster;
use crate::ProtoError;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 所有权臂运行报告（公共口径与按股私有/跨股臂一致 + 实体转移专属）。
#[derive(Clone, Debug)]
pub struct OwnRunReport {
    pub ticks: u64,
    pub wall_secs: f64,
    pub cpu_secs: f64,
    pub cpu_ms_per_tick: f64,
    pub ticks_per_sec: f64,
    pub peak_threads: u32,
    pub peak_decision_threads: i32,
    /// 组数（本臂决策并行度 = 组线程数；无每 tick 决策线程）。
    pub groups: u32,
    pub decide_calls: u64,
    pub intents: u64,
    pub trades: u64,
    pub volume_shares: u64,
    pub skipped_orders: u64,
    pub expired_orders: u64,
    pub cancelled_orders: u64,
    pub audit: OwnAuditReport,
    // ── 实体转移专属 ──
    pub cross_pct: u32,
    pub cross_fraction: f64,
    pub hold_counts: [u64; 4],
    /// 目标 ≠ home 的下单数。
    pub cross_places: u64,
    /// 目标 = home 的下单数。
    pub home_places: u64,
    /// 对齐撤单消息数。
    pub align_cancels: u64,
    /// 组端拒绝（严格所有权口径：在途退款未到账等造成的资金/可卖不足）。
    pub group_skipped: u64,
    pub cancel_intents: u64,
    /// 实体计数（成交所得/退款/新股票/余股/终结）。
    pub entities: EntCounters,
    /// 组→交易部批次消息总数。
    pub group_flushes: u64,
    /// 交易部→组批次消息总数。
    pub desk_batches: u64,
    /// coordinator 等齐全部 GroupStats 的均值（决策相位等待），ms/tick。
    pub decide_wait_ms: f64,
    /// 从决策相位完成到收齐全部 DeskDone 的均值（撮合相位等待），ms/tick。
    pub match_wait_ms: f64,
    /// Start 发送 + 收束安装的均值，ms/tick。
    pub dispatch_ms: f64,
}

/// 所有权臂期末审计。
#[derive(Clone, Debug)]
pub struct OwnAuditReport {
    pub initial_total_cash_cents: i64,
    pub final_total_cash_cents: i64,
    pub stocks: Vec<OwnStockAudit>,
    pub negative_cash_accounts: u64,
    pub t1_violations: u64,
    pub lot_violations: u64,
}

#[derive(Clone, Debug)]
pub struct OwnStockAudit {
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

/// 主入口：跑所有权臂基准并做期末守恒审计。
pub fn run_own_benchmark(
    cfg: &ProtoConfig,
    groups: u32,
    cross_pct: u32,
) -> Result<OwnRunReport, ProtoError> {
    if cfg.stocks.is_empty() || cfg.ticks == 0 {
        return Err(ProtoError::InvalidConfig("stocks 与 ticks 必须非空".into()));
    }
    if groups == 0 || groups > 255 {
        return Err(ProtoError::InvalidConfig(format!(
            "groups 需要 1~255，收到 {groups}"
        )));
    }
    if cfg.stocks.len() > 8 {
        return Err(ProtoError::InvalidConfig(
            "所有权臂最多支持 8 只股票（在途登记定长）".into(),
        ));
    }
    let stocks = cfg.stocks.len();
    let groups_u = groups as u8;

    // 与 default-setup.json 对齐的权威费用/整手配置（与另两臂同参）。
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

    // ── 世界构造：与跨股臂同 seed 同法（5 个 home 股名册拼接后按组切分）──
    let mut all: Vec<Npc> = Vec::new();
    let mut home_of_all: Vec<u8> = Vec::new();
    let mut remainders = vec![0u64; stocks];
    let mut hold_counts = [0u64; 4];
    for s in 0..stocks {
        let (roster, remainder, info) = build_cross_roster(cfg, s, cross_pct)?;
        remainders[s] = remainder;
        for (i, &c) in info.hold_counts.iter().enumerate() {
            hold_counts[i] += c;
        }
        for npc in roster {
            home_of_all.push(s as u8);
            all.push(npc);
        }
    }
    let retail_total: u64 = cfg.retail_per_stock as u64 * stocks as u64;
    let hold_2_or_3 = hold_counts[1] + hold_counts[2] + hold_counts[3];
    let cross_fraction = if retail_total > 0 {
        hold_2_or_3 as f64 / retail_total as f64
    } else {
        0.0
    };
    // 连续区间切分（组内 NPC 混多 home 股——订单按意图路由到任意交易部）。
    let total = all.len() as u32;
    let mut group_rosters: Vec<Vec<Npc>> = vec![Vec::new(); groups as usize];
    let mut group_homes: Vec<Vec<u8>> = vec![Vec::new(); groups as usize];
    for (i, npc) in all.into_iter().enumerate() {
        let g = target_group(total, groups, i as u64) as usize;
        group_homes[g].push(home_of_all[i]);
        group_rosters[g].push(npc);
    }
    // 组内掩码（与切分后的名册按下标配对）。
    let mut group_masks: Vec<Vec<u8>> = group_rosters
        .iter()
        .zip(group_homes.iter())
        .map(|(roster, homes)| {
            roster
                .iter()
                .zip(homes.iter())
                .map(|(npc, &home)| {
                    let mut m = 1u8 << home;
                    for pos in &npc.secondaries {
                        m |= 1 << pos.stock_idx;
                    }
                    m
                })
                .collect()
        })
        .collect();

    // 共享静态表。
    let codes: Vec<engine::account::StockCode> = cfg
        .stocks
        .iter()
        .map(|s| engine::account::StockCode(s.code.clone()))
        .collect();
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
            (
                (s.initial_price_cents - band).max(1),
                s.initial_price_cents + band,
            )
        })
        .collect();
    let initial_prices: Vec<i64> = cfg.stocks.iter().map(|s| s.initial_price_cents).collect();
    let shared = Arc::new(OwnShared {
        cfg: Arc::new(cfg.clone()),
        game_cfg,
        codes,
        code_idx,
        bands,
        initial_prices,
    });

    // ── 通道拓扑：组 ↔ 交易部 全互联直发；上行共用一个 mpsc ──
    let abort = Arc::new(AtomicBool::new(false));
    let (up_tx, up_rx) = channel::<OwnUp>();
    let mut group_txs = Vec::with_capacity(groups as usize);
    let mut group_rxs = Vec::with_capacity(groups as usize);
    for _ in 0..groups {
        let (tx, rx) = channel::<GroupIn>();
        group_txs.push(tx);
        group_rxs.push(rx);
    }
    let mut desk_txs = Vec::with_capacity(stocks);
    let mut desk_rxs = Vec::with_capacity(stocks);
    for _ in 0..stocks {
        let (tx, rx) = channel::<DeskMsg>();
        desk_txs.push(tx);
        desk_rxs.push(rx);
    }

    let mut handles = Vec::with_capacity(groups as usize + stocks);
    // 组线程。
    for g in 0..groups as usize {
        let roster = std::mem::take(&mut group_rosters[g]);
        let homes = std::mem::take(&mut group_homes[g]);
        let masks = std::mem::take(&mut group_masks[g]);
        let rx = std::mem::replace(&mut group_rxs[g], channel::<GroupIn>().1);
        let handle = std::thread::Builder::new()
            .name(format!("own-group-{g}"))
            .spawn({
                let shared = Arc::clone(&shared);
                let abort = Arc::clone(&abort);
                let up_tx = up_tx.clone();
                let desk_txs = desk_txs.clone();
                move || {
                    let r = group_main(
                        shared, g as u8, groups_u, roster, homes, masks, desk_txs, rx, up_tx, abort,
                    );
                    if let Err(e) = r {
                        std::panic::panic_any(format!("own-group {g} 失败：{e}"));
                    }
                }
            })
            .map_err(|e| ProtoError::ChannelClosed(format!("spawn own-group {g}：{e}")))?;
        handles.push((format!("own-group-{g}"), handle));
    }
    // 交易部线程（一股一线程）。
    for s in 0..stocks {
        let rx = std::mem::replace(&mut desk_rxs[s], channel::<DeskMsg>().1);
        let handle = std::thread::Builder::new()
            .name(format!("own-desk-{}", shared.cfg.stocks[s].code))
            .spawn({
                let shared = Arc::clone(&shared);
                let abort = Arc::clone(&abort);
                let up_tx = up_tx.clone();
                let group_txs = group_txs.clone();
                move || {
                    let r = desk_main(shared, s, groups_u, rx, group_txs, up_tx, abort);
                    if let Err(e) = r {
                        std::panic::panic_any(format!("own-desk {s} 失败：{e}"));
                    }
                }
            })
            .map_err(|e| ProtoError::ChannelClosed(format!("spawn own-desk {s}：{e}")))?;
        handles.push((format!("own-desk-{s}"), handle));
    }
    drop(up_tx);

    // 初始公共状态（与另两臂同法）。
    let mut public: Vec<StockPublicState> = shared
        .cfg
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
    let mut t_decide_wait = Duration::ZERO;
    let mut t_match_wait = Duration::ZERO;
    let mut t_dispatch = Duration::ZERO;

    for tick in 1..=shared.cfg.ticks {
        let snapshot = Arc::new(MarketSnapshot {
            tick,
            market_minute: tick / 60,
            stocks: public.clone(),
        });
        let t_send = Instant::now();
        for tx in &group_txs {
            tx.send(GroupIn::Start(Arc::clone(&snapshot)))
                .map_err(|_| ProtoError::ChannelClosed("own Start".into()))?;
        }
        let mut send_dur = t_send.elapsed();
        let t0 = Instant::now();
        // 收束：G GroupStats + N DeskDone（数据面直发，不经此处）。
        let mut stats_left = groups;
        let mut done_left = stocks as u32;
        let mut decide_done: Option<Duration> = None;
        while stats_left > 0 || done_left > 0 {
            match up_rx
                .recv()
                .map_err(|_| ProtoError::ChannelClosed("own 收束上行".into()))?
            {
                OwnUp::GroupStats {
                    decide_calls,
                    intents,
                    ..
                } => {
                    stats_left -= 1;
                    stats_decide += decide_calls;
                    stats_intents += intents;
                    if stats_left == 0 {
                        decide_done = Some(t0.elapsed());
                    }
                }
                OwnUp::DeskDone(done) => {
                    done_left -= 1;
                    let ti = Instant::now();
                    install_done(&mut public, &done, tick);
                    send_dur += ti.elapsed();
                }
                OwnUp::Failed(who, e) => {
                    return Err(ProtoError::ChannelClosed(format!(
                        "worker {who} 失败（显式上抛）：{e}"
                    )));
                }
                OwnUp::GroupFinal(..) | OwnUp::DeskFinal(..) | OwnUp::DeskDrained => {
                    return Err(ProtoError::ChannelClosed(
                        "tick 收束期望 GroupStats/DeskDone，收到其它上行消息（实现 bug）".into(),
                    ));
                }
            }
        }
        let t_all = t0.elapsed();
        let d = decide_done.unwrap_or(Duration::ZERO);
        t_decide_wait += d;
        t_match_wait += t_all - d;
        t_dispatch += send_dur;
        peak_threads = peak_threads.max(read_thread_count());
    }
    let wall_secs = wall_start.elapsed().as_secs_f64();
    let cpu_secs = read_cpu_secs() - cpu_start;

    // ── 收尾：Drain → Shutdown → Final ──
    for tx in &desk_txs {
        tx.send(DeskMsg::Drain)
            .map_err(|_| ProtoError::ChannelClosed("own Drain".into()))?;
    }
    for _ in 0..stocks {
        match up_rx
            .recv()
            .map_err(|_| ProtoError::ChannelClosed("own DeskDrained".into()))?
        {
            OwnUp::DeskDrained => {}
            OwnUp::Failed(who, e) => {
                return Err(ProtoError::ChannelClosed(format!(
                    "worker {who} Drain 失败（显式上抛）：{e}"
                )));
            }
            other => {
                return Err(ProtoError::ChannelClosed(format!(
                    "Drain 期望 DeskDrained，收到 {other:?}（实现 bug）"
                )));
            }
        }
    }
    // 组线程的收件箱中，全部实体批次先于 Shutdown 入队（Drain flush 先于
    // DeskDrained，coordinator 收齐后才发 Shutdown）——期末名册已含全部回流。
    for tx in &group_txs {
        tx.send(GroupIn::Shutdown)
            .map_err(|_| ProtoError::ChannelClosed("own Shutdown".into()))?;
    }
    let mut group_finals: Vec<Option<GroupFinal>> = (0..groups as usize).map(|_| None).collect();
    for _ in 0..groups {
        match up_rx
            .recv()
            .map_err(|_| ProtoError::ChannelClosed("own GroupFinal".into()))?
        {
            OwnUp::GroupFinal(f) => {
                let g = f.group;
                group_finals[g as usize] = Some(*f);
            }
            OwnUp::Failed(who, e) => {
                return Err(ProtoError::ChannelClosed(format!(
                    "worker {who} 关停失败（显式上抛）：{e}"
                )));
            }
            other => {
                return Err(ProtoError::ChannelClosed(format!(
                    "关停期望 GroupFinal，收到 {other:?}（实现 bug）"
                )));
            }
        }
    }
    // 关闭交易部通道（coordinator 端 + 组端已随组线程退出释放）→ 收 DeskFinal。
    drop(desk_txs);
    let mut desk_finals: Vec<Option<DeskFinal>> = (0..stocks).map(|_| None).collect();
    for _ in 0..stocks {
        match up_rx
            .recv()
            .map_err(|_| ProtoError::ChannelClosed("own DeskFinal".into()))?
        {
            OwnUp::DeskFinal(f) => {
                let s = f.stock_idx;
                desk_finals[s] = Some(*f);
            }
            OwnUp::Failed(who, e) => {
                return Err(ProtoError::ChannelClosed(format!(
                    "worker {who} 期末失败（显式上抛）：{e}"
                )));
            }
            other => {
                return Err(ProtoError::ChannelClosed(format!(
                    "关停期望 DeskFinal，收到 {other:?}（实现 bug）"
                )));
            }
        }
    }
    for (name, handle) in handles {
        // 线程内失败以 panic 上抛（闭包保证），join Err 即显式失败。
        handle
            .join()
            .map_err(|_| ProtoError::ChannelClosed(format!("{name} panic/失败")))?;
    }

    // ── 期末审计：守恒 + 不变量（显式失败，不静默）──
    let initial_total_cash = shared.cfg.initial_total_cash_cents();
    let mut final_total_cash = 0i64;
    let mut stats_trades = 0u64;
    let mut stats_volume = 0u64;
    let mut stats_skipped = 0u64;
    let mut stats_expired = 0u64;
    let mut stats_cancelled = 0u64;
    let mut negative_cash = 0u64;
    let mut t1_violations = 0u64;
    let mut lot_violations = 0u64;
    let mut group_skipped = 0u64;
    let mut cancel_intents = 0u64;
    let mut cross_places = 0u64;
    let mut home_places = 0u64;
    let mut align_cancels = 0u64;
    let mut group_flushes = 0u64;
    let mut desk_batches = 0u64;
    let mut entities = EntCounters::default();
    let mut held_per_stock = vec![0u64; stocks];
    for (g, final_state) in group_finals.iter().enumerate() {
        let f = final_state
            .as_ref()
            .ok_or_else(|| ProtoError::ChannelClosed(format!("组 {g} 无期末状态")))?;
        group_skipped += f.skipped_orders;
        cancel_intents += f.cancel_intents;
        cross_places += f.cross_places;
        home_places += f.home_places;
        align_cancels += f.align_cancels;
        group_flushes += f.flushes_sent;
        for (k, npc) in f.roster.iter().enumerate() {
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
            // 主仓归属股：与构造期切分同法反解全局下标 → home。
            let global = global_index(total, groups, g as u64, k as u64);
            let home = home_of_all[global as usize] as usize;
            held_per_stock[home] += npc.qty as u64;
            for pos in &npc.secondaries {
                held_per_stock[pos.stock_idx as usize] += pos.qty as u64;
            }
        }
    }
    for f in desk_finals.iter().flatten() {
        final_total_cash += f.fee_sink;
        stats_trades += f.trade_count;
        stats_volume += f.volume_shares;
        stats_skipped += f.skipped_orders;
        stats_expired += f.expired_orders;
        stats_cancelled += f.cancelled_orders;
        entities.cash_proceeds += f.entities.cash_proceeds;
        entities.cash_refund += f.entities.cash_refund;
        entities.bought_shares += f.entities.bought_shares;
        entities.refunded_shares += f.entities.refunded_shares;
        entities.closed += f.entities.closed;
        desk_batches += f.batches_sent;
    }
    stats_skipped += group_skipped;
    if final_total_cash != initial_total_cash {
        return Err(ProtoError::CashConservation {
            initial: initial_total_cash,
            final_: final_total_cash,
        });
    }
    let mut stock_audits = Vec::with_capacity(stocks);
    for (s, spec) in shared.cfg.stocks.iter().enumerate() {
        let band = (spec.initial_price_cents as f64 * spec.limit_pct) as i64;
        let held = held_per_stock[s];
        let remainder = remainders[s];
        if held + remainder != spec.float_shares {
            return Err(ProtoError::ShareConservation {
                code: spec.code.clone(),
                held,
                remainder,
                total: spec.float_shares,
            });
        }
        let f = desk_finals[s]
            .as_ref()
            .ok_or_else(|| ProtoError::ChannelClosed(format!("交易部 {s} 无期末状态")))?;
        let min_p = f.min_trade_price.unwrap_or(0);
        let max_p = f.max_trade_price.unwrap_or(0);
        if f.trade_count > 0
            && (min_p < spec.initial_price_cents - band || max_p > spec.initial_price_cents + band)
        {
            return Err(ProtoError::InvalidConfig(format!(
                "股票 {} 成交价越界：[{}, {}] 不在 [{}, {}]",
                spec.code,
                min_p,
                max_p,
                spec.initial_price_cents - band,
                spec.initial_price_cents + band
            )));
        }
        stock_audits.push(OwnStockAudit {
            code: spec.code.clone(),
            float_total_shares: spec.float_shares,
            float_remainder_shares: remainder,
            held_shares: held,
            lower_limit_cents: spec.initial_price_cents - band,
            upper_limit_cents: spec.initial_price_cents + band,
            min_trade_price_cents: min_p,
            max_trade_price_cents: max_p,
            trade_count: f.trade_count,
        });
    }
    let ticks_f = shared.cfg.ticks as f64;
    Ok(OwnRunReport {
        ticks: shared.cfg.ticks,
        wall_secs,
        cpu_secs,
        cpu_ms_per_tick: cpu_secs * 1000.0 / ticks_f,
        ticks_per_sec: ticks_f / wall_secs,
        peak_threads,
        peak_decision_threads: 0,
        groups,
        decide_calls: stats_decide,
        intents: stats_intents,
        trades: stats_trades,
        volume_shares: stats_volume,
        skipped_orders: stats_skipped,
        expired_orders: stats_expired,
        cancelled_orders: stats_cancelled,
        audit: OwnAuditReport {
            initial_total_cash_cents: initial_total_cash,
            final_total_cash_cents: final_total_cash,
            stocks: stock_audits,
            negative_cash_accounts: negative_cash,
            t1_violations,
            lot_violations,
        },
        cross_pct,
        cross_fraction,
        hold_counts,
        cross_places,
        home_places,
        align_cancels,
        group_skipped,
        cancel_intents,
        entities,
        group_flushes,
        desk_batches,
        decide_wait_ms: t_decide_wait.as_secs_f64() * 1000.0 / ticks_f,
        match_wait_ms: t_match_wait.as_secs_f64() * 1000.0 / ticks_f,
        dispatch_ms: t_dispatch.as_secs_f64() * 1000.0 / ticks_f,
    })
}

/// 收束安装（与另两臂同法 + 盘口快照字段）。
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

/// 均匀连续切分：全局索引 i → 组号（与 chunk_range 同分布）。
fn target_group(total: u32, groups: u32, i: u64) -> u32 {
    // 反函数：先用与 chunk_range 相同的分布正向建表代价 O(total)，这里
    // 直接按「前 total%groups 组各多 1」的规则反解。
    let base = total / groups;
    let rem = total % groups;
    let big_end = rem * (base + 1);
    if i < big_end as u64 {
        (i / (base as u64 + 1)) as u32
    } else if base == 0 {
        rem // 理论不可达（groups ≤ total 由外部保证）；防御返回。
    } else {
        rem + ((i - big_end as u64) / base as u64) as u32
    }
}

/// 审计期重算用的全局索引（组 g 内第 k 个 → 拼接名册下标）。
fn global_index(total: u32, groups: u32, g: u64, k: u64) -> u64 {
    let base = total as u64 / groups as u64;
    let rem = total as u64 % groups as u64;
    let big = rem * (base + 1);
    if g < rem {
        g * (base + 1) + k
    } else {
        big + (g - rem) * base + k
    }
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
