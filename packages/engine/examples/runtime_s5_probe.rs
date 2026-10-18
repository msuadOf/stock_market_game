//! runtime s5 负载端到端探针（批 8）：首次真实引擎语义经 runtime 的性能数字。
//!
//! 负载口径对齐原型 own-inline（s5）：5 股、~20000 NPC（原型 20007；本探针
//! 10 组×2000）、retail 决策内联（decide_data 直调）、撮合/费用/相位/行情/
//! T+1/日结全语义。用法：`cargo run -p engine --release --example
//! runtime_s5_probe -- [ticks] [groups]`。

use engine::config::GameConfig;
use engine::money::Money;
use engine::runtime::decision::{DecisionSetup, NpcProfile};
use engine::runtime::watchdog::WatchdogConfig;
use engine::runtime::world::{DeskStockSpec, World, WorldSpec};
use engine::strategy::StrategyData;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let ticks: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(300);
    let n_groups: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let per_group: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let n_stocks = 5usize;

    let config = Arc::new(GameConfig::proposed_defaults());
    let profiles: Vec<NpcProfile> = (0..per_group)
        .map(|i| NpcProfile {
            // retail 参数对齐引擎 proposed 侧写形状（到达率/单量/追势）。
            strategy: StrategyData::retail(0.30, 200, 0.35),
            seed: 0x9E37_79B9_7F4A_7C15u64 ^ (i as u64),
            next_due: (i % 8) as u64,
            attention_base: 0.30,
            retail_style: None,
        })
        .collect();
    let decision = DecisionSetup {
        profiles,
        attention_gap_ticks: 4,
    };

    let spec = WorldSpec {
        n_groups,
        accounts_per_group: per_group,
        initial_cash: 1_000_000_000, // 千万元级，避免早期普遍 skip
        initial_shares: vec![vec![vec![1_000u32; n_stocks]; per_group]; n_groups],
        script_for: Box::new(|_g| Box::new(|_t, _r| engine::runtime::group::TickPlan::default())),
        chunk: 512,
        watchdog: WatchdogConfig {
            normal: Duration::from_secs(30),
            long: Duration::from_secs(120),
        },
        rounds_cap: 4,
        initial_desk_next_serial: Vec::new(),
        initial_last_day_end_tick: None,
        initial_pending_sell: Vec::new(),
        stocks: (0..n_stocks)
            .map(|i| DeskStockSpec {
                exchange: if i % 2 == 0 {
                    engine::StockExchange::Shanghai
                } else {
                    engine::StockExchange::Shenzhen
                },
                initial_price: 1000 + (i as i64) * 25,
                limit_bps: 1000,
                max_order_qty_limit: 1_000_000,
                max_order_qty_market: 1_000_000,
                initial_daily_bars: Vec::new(),
                initial_recent_prices: Vec::new(),
                initial_traded_days: 0,
                initial_market: None,
                initial_public_cutoff: 0,
            })
            .collect(),
        config: Arc::clone(&config),
        ticks_per_day: 240,
        auction_ticks: 0,
        closing_auction_ticks: 3,
        order_ttl_ticks: 1_000_000,
        history_len: 16,
        decision: Some(decision),
        rights_computer: None,
        initial_cash_override: None,
        plan_factory: None,
        belief_seeds: Vec::new(),
        library: None,
        initial_desk_orders: Vec::new(),
        root_projection: None,
        civil_start_date: engine::calendar::CivilDate::from_ymd(2026, 1, 5)
            .expect("探针日期合法"),
        initial_committed_tick: 0,
        initial_invested: Vec::new(),
        initial_recovered: Vec::new(),
        initial_ever_held: Vec::new(),
        initial_frozen_today: Vec::new(),
        initial_purse_held: Vec::new(),
    };

    let npcs = n_groups * per_group;
    println!("runtime s5 探针：{ticks} tick、{n_groups} 组×{per_group} NPC={npcs}、{n_stocks} 股");
    let mut world = World::spawn(spec).expect("世界构造");
    let started = Instant::now();
    let committed = world.run_ticks(ticks).expect("tick 全部收敛");
    let wall = started.elapsed().as_secs_f64();

    let seal_ns = world.seal_ns_accumulated;
    let (first_ack_ns, ack_spread_ns) = (world.seal_first_ack_ns, world.seal_ack_spread_ns);
    let report = world.shutdown().expect("关停对账");
    let mut cash: i128 = 0;
    let mut shares = 0u64;
    let mut frozen = 0u64;
    let mut resting = 0u64;
    let mut trades = 0u64;
    let mut fees: i64 = 0;
    for f in &report.group_finals {
        for a in &f.accounts {
            cash += a.cash as i128;
            shares += a.shares.iter().map(|&s| s as u64).sum::<u64>();
            frozen += a.frozen_today.iter().map(|&s| s as u64).sum::<u64>();
        }
        resting += f
            .accounts
            .iter()
            .map(|a| a.foreign_active.iter().map(|&x| x as u64).sum::<u64>())
            .sum::<u64>();
    }
    for d in &report.desk_finals {
        trades += d.market.volume_today;
        fees += d.fee_sink;
    }
    let tps = committed as f64 / wall;
    // Q2 A 口径归因诊断：seal_round 累计 wall 占比（广播+收集等待——
    // 组端 drain/落账/分片构建反映在等待里）与每拍均值。
    let seal_pct = (seal_ns as f64) / (wall * 1e9) * 100.0;
    // Seal 地板分解：广播→首 Ack（最快组全链路）与首末 Ack 间隔
    ///（组间长尾）——换向链固有 vs 组端不均的归因面。
    let first_ack_us = first_ack_ns as f64 / committed as f64 / 1000.0;
    let ack_spread_us = ack_spread_ns as f64 / committed as f64 / 1000.0;
    println!(
        "结果：{tps:.1} t/s（{committed} tick / {wall:.2}s）；成交量 {trades} 股；费用桶 {fees} 分；挂簿余 {resting}；T+1 冻结 {frozen}；Seal 相位 {seal_pct:.1}%（{:.0}μs/tick）",
        seal_ns as f64 / committed as f64 / 1000.0
    );
    println!(
        "Seal 分解：广播→首Ack {first_ack_us:.0}μs/tick；首末Ack间隔 {ack_spread_us:.0}μs/tick（长尾）"
    );
    {
        let mut handles: Vec<f64> = report
            .group_finals
            .iter()
            .map(|f| f.seal_handle_ns as f64 / committed as f64 / 1000.0)
            .collect();
        handles.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = handles.len();
        let mean: f64 = handles.iter().sum::<f64>() / n as f64;
        println!(
            "组端Seal处理 μs/tick：min {:.0} / mean {:.0} / max {:.0}（{} 组）",
            handles[0], mean, handles[n - 1], n
        );
    }
    // 守恒：现金 + 费用 = 初始（撤到 Drain 后在途清零）。
    let initial_total = (npcs as i128) * 1_000_000_000;
    let shares_total = (npcs as u64) * 1000 * n_stocks as u64;
    assert_eq!(
        cash + fees as i128,
        initial_total,
        "现金守恒（含 fee_sink）"
    );
    assert_eq!(shares, shares_total, "股份守恒（Drain 回流后）");
    println!("守恒：现金+费用=初始 ✓；股份=初始 ✓");
    let _ = Money::ZERO;
}
