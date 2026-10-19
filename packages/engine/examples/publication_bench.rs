//! Q1 发布协议 J2/J3 数字微基准（release 运行：cargo run --release
//! --example publication_bench -p engine -- [ticks] [readers]）。
//!
//! - J2 读路径延迟：写者线程满速 publish（模拟引擎每拍发布一次）、
//!   并发读者循环 load()——报 p50/p99/max 与读次数。
//! - J3 保留内存：读者钉住 N-k 代版本、发布前进 1000 代，验证发布槽
//!   只持 1 代（新旧不同代）+ 每代估算字节登记（无隐藏多代）。

use engine::runtime::coordinator::GlobalState;
use engine::runtime::publication::{
    DeskShard, EventSegment, PublishedTick, TickHashes, ViewPublication,
};
use engine::runtime::{StockIdx, StockPublic};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// 合成分片（recent_prices 512 档——B 口径量级的观测面负载）。
/// 战役收袋维护注：字段面随 StockPublic 演进（刀 2b Arc 化+扩
/// turnover/trade_count 双字段）——bench 保持合成占位（非业务语义）。
fn synthetic_view(tick: u64) -> StockPublic {
    StockPublic {
        last_price: 1000 + (tick as i64 % 50),
        best_bid: Some(999),
        best_ask: Some(1001),
        max_buy_price: 1100,
        daily_upper_limit: 1100,
        daily_lower_limit: 900,
        min_sell_price: 900,
        recent_prices: vec![1000; 512],
        minute_closes: Vec::new(),
        daily_bars: Vec::new(),
        daily_active: None,
        daily_active_turnover_cents: 0,
        daily_active_trade_count: 0,
        daily_total_completed: 0,
        daily_total_traded: 0,
        volume_today: 0,
        relative_volume: 1.0,
        order_book_imbalance: 0.0,
        public_cutoff_serial: 0,
        depth_bids: vec![(999, 100); 5],
        depth_asks: vec![(1001, 100); 5],
    }
}

fn synthetic_tick(publication: &ViewPublication, tick: u64) {
    let desks: Vec<DeskShard> = (0..5)
        .map(|s| DeskShard {
            stock: s as StockIdx,
            view: Arc::new(synthetic_view(tick)),
            resting_count: 0,
            resting: Arc::new(Vec::new()),
            next_serial: 0,
        })
        .collect();
    publication.publish(Arc::new(PublishedTick {
        tick,
        global: GlobalState {
            committed_tick: tick,
            day: 0,
            events_installed: 0,
        },
        desks,
        groups: Vec::new(),
        events: EventSegment::default(),
        hashes: TickHashes::default(),
    }));
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let ticks: u64 = args.first().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let readers: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(2);
    println!("J2/J3 微基准：{ticks} 代发布、{readers} 并发读者");

    let publication = Arc::new(ViewPublication::new(Arc::new(PublishedTick {
        tick: 0,
        global: GlobalState::new(),
        desks: Vec::new(),
        groups: Vec::new(),
        events: EventSegment::default(),
        hashes: TickHashes::default(),
    })));

    // ── J2：并发读者 + 写者满速发布 ──
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    for _ in 0..readers {
        let publication = Arc::clone(&publication);
        let stop = Arc::clone(&stop);
        handles.push(std::thread::spawn(move || {
            let mut latencies_ns: Vec<u64> = Vec::with_capacity(1 << 20);
            let mut last_tick = 0u64;
            let mut regressions = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let t0 = Instant::now();
                let version = publication.load();
                let dt = t0.elapsed().as_nanos() as u64;
                latencies_ns.push(dt);
                if version.tick < last_tick {
                    regressions += 1;
                }
                last_tick = last_tick.max(version.tick);
            }
            (latencies_ns, regressions)
        }));
    }
    let started = Instant::now();
    for t in 1..=ticks {
        synthetic_tick(&publication, t);
    }
    let publish_wall = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let mut all = Vec::new();
    let mut regressions = 0u64;
    for h in handles {
        let (lat, reg) = h.join().expect("读者合并");
        regressions += reg;
        all.extend(lat);
    }
    assert_eq!(regressions, 0, "J2 微基准期间版本回退必须为 0");
    all.sort_unstable();
    let n = all.len();
    let pct = |p: f64| all[((n as f64 * p) as usize).min(n - 1)];
    println!(
        "J2 load(): n={n} p50={}ns p99={}ns max={}ns（publish {ticks} 代 {publish_wall:?}，均值 {:.0}ns/代）",
        pct(0.50),
        pct(0.99),
        all[n - 1],
        publish_wall.as_nanos() as f64 / ticks as f64,
    );

    // ── J3：旧版本保留（读者钉 N-k 代、发布前进 1000 代）──
    let pinned = publication.load();
    let pinned_tick = pinned.tick;
    for t in (pinned_tick + 1)..=(pinned_tick + 1000) {
        synthetic_tick(&publication, t);
    }
    let current = publication.load();
    assert!(
        !Arc::ptr_eq(&pinned, &current),
        "J3：发布槽必须只持 1 代（新旧不同代）"
    );
    assert_eq!(pinned.tick, pinned_tick, "钉住版本内容不可变");
    let view_bytes = std::mem::size_of::<StockPublic>() + 512 * 8 + 10 * 16;
    let per_generation_bytes =
        std::mem::size_of::<PublishedTick>() + 5 * (std::mem::size_of::<DeskShard>() + view_bytes);
    println!(
        "J3 保留：钉住代 tick={pinned_tick}、槽已前进至 {}；每代估算 ≈{per_generation_bytes} 字节（5 股×512 档 recent）——无隐藏多代，旧代随读者 drop 释放",
        current.tick
    );
}
