//! runtime s5 负载端到端探针（批 8）：首次真实引擎语义经 runtime 的性能数字。
//!
//! 负载口径对齐原型 own-inline（s5）：5 股、~20000 NPC（原型 20007；本探针
//! 10 组×2000）、retail 决策内联（decide_data 直调）、撮合/费用/相位/行情/
//! T+1/日结全语义。用法：`cargo run -p engine --release --example
//! runtime_s5_probe -- [ticks] [groups]`。

use engine::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use engine::config::GameConfig;
use engine::money::Money;
use engine::runtime::decision::{DecisionSetup, NpcProfile};
use engine::runtime::wait_contract::DeadlineConfig;
use engine::runtime::world::{DeskStockSpec, ShutdownReport, World, WorldSpec};
use engine::strategy::StrategyData;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let ticks: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(300);
    let n_groups: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let per_group: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
    run_probe(ticks, n_groups, per_group);
}

fn probe_spec(n_groups: usize, per_group: usize) -> WorldSpec {
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

    let start_date = CivilDate::from_ymd(2026, 1, 5).expect("探针日期合法");
    WorldSpec {
        initial_plan_books: Vec::new(),
        initial_repurchase_projections: Default::default(),
        admin_treasury_bindings: Vec::new(),
        n_groups,
        accounts_per_group: per_group,
        initial_cash: 1_000_000_000, // 千万元级，避免早期普遍 skip
        initial_shares: vec![vec![vec![1_000u32; n_stocks]; per_group]; n_groups],
        script_for: Box::new(|_g| Box::new(|_t, _r| engine::runtime::group::TickPlan::default())),
        chunk: 512,
        deadline: DeadlineConfig {
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
        civil_start_date: start_date,
        calendar_start_date: start_date,
        initial_committed_tick: 0,
        initial_invested: Vec::new(),
        initial_recovered: Vec::new(),
        initial_ever_held: Vec::new(),
        initial_frozen_today: Vec::new(),
        initial_purse_held: Vec::new(),
    }
}

fn run_probe(ticks: u64, n_groups: usize, per_group: usize) -> u64 {
    let spec = probe_spec(n_groups, per_group);
    let n_stocks = spec.stocks.len();
    let start_date = spec.civil_start_date;
    let ticks_per_day = spec.ticks_per_day;
    let exchanges = spec
        .stocks
        .iter()
        .map(|stock| match stock.exchange {
            engine::StockExchange::Shanghai => CalendarExchange::Sse,
            engine::StockExchange::Shenzhen => CalendarExchange::Szse,
        })
        .collect::<Vec<_>>();
    let npcs = n_groups * per_group;
    println!("runtime s5 探针：{ticks} tick、{n_groups} 组×{per_group} NPC={npcs}、{n_stocks} 股");
    let calendar = TradingCalendar::current_default_calendar().expect("探针 Calendar 政策合法");
    let mut world = World::spawn(spec, calendar.clone()).expect("世界构造");
    let started = Instant::now();
    let progress = run_probe_ticks(
        &mut world,
        &calendar,
        start_date,
        ticks_per_day,
        &exchanges,
        ticks,
    );
    let committed = progress.committed;
    let wall = started.elapsed().as_secs_f64();

    let seal_ns = world.seal_ns_accumulated;
    let (first_ack_ns, ack_spread_ns) = (world.seal_first_ack_ns, world.seal_ack_spread_ns);
    let last_ack_groups = world.seal_last_ack_groups.clone();
    let report = world.shutdown().expect("关停对账");
    let mut cash: i128 = 0;
    let mut shares = 0u64;
    let mut frozen = 0u64;
    let mut resting = 0u64;
    let trades = probe_trade_volume(&progress, &report);
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
        fees += d.fee_sink;
    }
    let tps = committed as f64 / wall;
    // Q2 A 口径归因诊断：seal_round 累计 wall 占比（广播+收集等待——
    // 组端 drain/落账/分片构建反映在等待里）与每拍均值。
    let seal_pct = (seal_ns as f64) / (wall * 1e9) * 100.0;
    // Seal 地板分解：广播→首 Ack（最快组全链路）与首末 Ack 间隔
    //（组间长尾）——换向链固有 vs 组端不均的归因面。
    let first_ack_us = first_ack_ns as f64 / committed as f64 / 1000.0;
    let ack_spread_us = ack_spread_ns as f64 / committed as f64 / 1000.0;
    println!(
        "结果：{tps:.1} t/s（{committed} tick / {wall:.2}s）；成交量 {trades} 股；费用桶 {fees} 分；挂簿余 {resting}；T+1 冻结 {frozen}；Seal 相位 {seal_pct:.1}%（{:.0}μs/tick）",
        seal_ns as f64 / committed as f64 / 1000.0
    );
    println!(
        "Seal 分解：广播→首Ack {first_ack_us:.0}μs/tick；首末Ack间隔 {ack_spread_us:.0}μs/tick（长尾）"
    );
    // B-1' 验证批：最晚 Ack 组身份分布（每组出现次数——唤醒级联假设的
    // 直接证据：集中在闲组=唤醒主导；集中在活跃组=组端处理主导）。
    {
        let mut counts = std::collections::BTreeMap::new();
        for g in &last_ack_groups {
            *counts.entry(*g).or_insert(0u32) += 1;
        }
        let dist: Vec<String> = counts.iter().map(|(g, c)| format!("{g}:{c}")).collect();
        println!("最晚Ack组分布：{}", dist.join(" "));
    }
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
            handles[0],
            mean,
            handles[n - 1],
            n
        );
        // H1/H2/H3 构成：drain 批占比/延迟相/分片构建（max 组口径）。
        // 诊断面：全部组的构成明细（组号/handle/drain批/等待批/延迟相/分片）。
        for f in &report.group_finals {
            println!(
                "  组 {:>2}: handle {:>6.1}μs | drain批 {:>5} wait批 {:>5} | 延迟相 {:>6.1}μs | 分片 {:>5.1}μs",
                f.group,
                f.seal_handle_ns as f64 / committed as f64 / 1000.0,
                f.seal_drain_batches,
                f.seal_wait_batches,
                f.seal_deferred_ns as f64 / committed as f64 / 1000.0,
                f.seal_shard_ns as f64 / committed as f64 / 1000.0,
            );
        }
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
    committed
}

struct ProbeProgress {
    committed: u64,
    completed_day_volume: u64,
    ended_on_day_boundary: bool,
}

// 仅驱动从零开始的 raw 探针世界；完整日显式日结，日期来自同份 Calendar。
fn run_probe_ticks(
    world: &mut World,
    calendar: &TradingCalendar,
    mut date: CivilDate,
    ticks_per_day: u64,
    exchanges: &[CalendarExchange],
    ticks: u64,
) -> ProbeProgress {
    assert!(ticks_per_day > 0, "探针每天 tick 数必须为正");
    assert_eq!(world.global.committed_tick, 0, "探针驱动只接收全新世界");
    let mut remaining = ticks;
    let mut progress = ProbeProgress {
        committed: world.global.committed_tick,
        completed_day_volume: 0,
        ended_on_day_boundary: false,
    };
    while remaining > 0 {
        let chunk = remaining.min(ticks_per_day);
        progress.committed = world.run_ticks(chunk).expect("tick 全部收敛");
        remaining -= chunk;
        progress.ended_on_day_boundary = chunk == ticks_per_day;
        if progress.ended_on_day_boundary {
            world.run_day_end_public().expect("探针完整交易日日结");
            let bars = world.take_pending_day_bars();
            assert_eq!(bars.len(), exchanges.len(), "探针日结须覆盖全部股票");
            for stock_bars in bars {
                for bar in stock_bars.expect("探针日结缺少股票分钟桶应答") {
                    progress.completed_day_volume = progress
                        .completed_day_volume
                        .checked_add(bar.volume_shares)
                        .expect("探针累计成交量溢出");
                }
            }
            if remaining > 0 {
                date = next_probe_trading_date(calendar, exchanges, date);
                world.set_civil_date(date);
            }
        }
    }
    progress
}

fn next_probe_trading_date(
    calendar: &TradingCalendar,
    exchanges: &[CalendarExchange],
    mut date: CivilDate,
) -> CivilDate {
    assert!(!exchanges.is_empty(), "探针必须绑定证券交易所");
    loop {
        date = date.next().expect("探针下一自然日越界");
        for exchange in exchanges {
            if calendar
                .is_trading_day(*exchange, date)
                .expect("探针下一交易日查询失败")
            {
                return date;
            }
        }
    }
}

fn probe_trade_volume(progress: &ProbeProgress, report: &ShutdownReport) -> u64 {
    let mut volume = progress.completed_day_volume;
    // 完整日已从日结桶累计；只补未完成日的真实关停成交量。
    if !progress.ended_on_day_boundary {
        for desk in &report.desk_finals {
            volume = volume
                .checked_add(desk.market.volume_today)
                .expect("探针累计成交量溢出");
        }
    }
    volume
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::calendar::{CalendarPolicy, OfficialCoverageEntry};
    use engine::runtime::group::{PlaceSpec, TickPlan};

    fn scripted_spec() -> WorldSpec {
        let mut spec = probe_spec(2, 2);
        spec.ticks_per_day = 2;
        spec.closing_auction_ticks = 0;
        spec.decision = None;
        spec.stocks.truncate(2);
        for group in &mut spec.initial_shares {
            for shares in group {
                shares.truncate(2);
            }
        }
        spec.script_for = Box::new(|group| {
            Box::new(move |tick, _| {
                let mut plan = TickPlan::default();
                if tick == 1 || tick == 3 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 1,
                        side: if group == 0 {
                            engine::Side::Sell
                        } else {
                            engine::Side::Buy
                        },
                        price: 1025,
                        qty: 100,
                    });
                }
                plan
            })
        });
        spec
    }

    #[test]
    fn probe_volume_includes_completed_and_partial_days_once() {
        for (ticks, expected_volume) in [(2, 100), (3, 200), (4, 200)] {
            let spec = scripted_spec();
            let date = spec.civil_start_date;
            let calendar = TradingCalendar::current_default_calendar().unwrap();
            let mut world = World::spawn(spec, calendar.clone()).unwrap();
            let progress = run_probe_ticks(
                &mut world,
                &calendar,
                date,
                2,
                &[CalendarExchange::Sse, CalendarExchange::Szse],
                ticks,
            );
            let report = world.shutdown().unwrap();
            assert_eq!(progress.committed, ticks);
            assert_eq!(probe_trade_volume(&progress, &report), expected_volume);
            let cash: i128 = report
                .group_finals
                .iter()
                .flat_map(|group| &group.accounts)
                .map(|account| i128::from(account.cash))
                .sum();
            let fees: i128 = report
                .desk_finals
                .iter()
                .map(|desk| i128::from(desk.fee_sink))
                .sum();
            assert_eq!(cash + fees, 4 * 1_000_000_000, "跨日只读取一次累计费用");
        }
    }

    #[test]
    fn probe_uses_union_calendar_and_keeps_closed_stock_without_extra_day() {
        let mut spec = scripted_spec();
        let first = spec.civil_start_date;
        let second = first.next().unwrap();
        let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
        // synthetic-test 仅构造两所开休市差异，不冒充真实年度休市公告。
        policy.official_coverage = vec![OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2026,
            vec![(second, second)],
            "synthetic-test-probe-sse-closed".into(),
            "synthetic-test-probe-source".into(),
        )];
        let calendar =
            TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap();
        spec.script_for = Box::new(|group| {
            Box::new(move |tick, _| {
                let mut plan = TickPlan::default();
                let stocks: &[u16] = match tick {
                    1 => &[0, 1],
                    3 => &[1],
                    _ => &[],
                };
                for &stock in stocks {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock,
                        side: if group == 0 {
                            engine::Side::Sell
                        } else {
                            engine::Side::Buy
                        },
                        price: 1000 + i64::from(stock) * 25,
                        qty: 100,
                    });
                }
                plan
            })
        });
        assert_eq!(
            next_probe_trading_date(
                &calendar,
                &[CalendarExchange::Sse, CalendarExchange::Szse],
                first
            ),
            second
        );
        assert_eq!(
            next_probe_trading_date(
                &calendar,
                &[CalendarExchange::Sse, CalendarExchange::Szse],
                CivilDate::from_ymd(2026, 1, 9).unwrap(),
            ),
            CivilDate::from_ymd(2026, 1, 12).unwrap(),
            "周末不产生共享交易日",
        );
        let mut world = World::spawn(spec, calendar.clone()).unwrap();
        let progress = run_probe_ticks(
            &mut world,
            &calendar,
            first,
            2,
            &[CalendarExchange::Sse, CalendarExchange::Szse],
            4,
        );
        let report = world.shutdown().unwrap();
        assert_eq!(progress.committed, 4);
        assert_eq!(probe_trade_volume(&progress, &report), 300);
        assert_eq!(
            report.desk_finals[0].market.candles.len(),
            1,
            "SSE 休市不额外归档"
        );
        assert_eq!(
            report.desk_finals[1].market.candles.len(),
            2,
            "SZSE 两日正常归档"
        );
    }

    #[test]
    fn probe_runs_default_tick_count_across_civil_days() {
        assert_eq!(super::run_probe(300, 2, 4), 300);
    }
}
