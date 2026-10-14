//! CLI：s5 负载基准入口。
//!
//! 用法示例：
//! ```text
//! std_thread_proto --width 16 --mode spawn --label s5-16w-spawn
//! std_thread_proto --npcs 20007 --ticks 3000 --width 128 --mode spawn
//! ```
//! 输出人读摘要 + 可选 JSON（--json 路径）。

use std_thread_proto::{run_benchmark, ProtoConfig, RunMode};

fn main() {
    let mut npcs: u32 = 20_007;
    let mut ticks: u64 = 3_000;
    let mut width: u32 = 16;
    let mut mode = RunMode::Spawn;
    let mut seed: u64 = 42;
    let mut label = String::new();
    let mut json_path: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        fn value(args: &mut impl Iterator<Item = String>, name: &str) -> String {
            args.next()
                .unwrap_or_else(|| usage_exit(&format!("参数 {name} 缺少取值")))
        }
        match arg.as_str() {
            "--npcs" => npcs = value(&mut args, &arg).parse().unwrap_or_else(|_| usage_exit("--npcs 需要整数")),
            "--ticks" => ticks = value(&mut args, &arg).parse().unwrap_or_else(|_| usage_exit("--ticks 需要整数")),
            "--width" => width = value(&mut args, &arg).parse().unwrap_or_else(|_| usage_exit("--width 需要整数")),
            "--mode" => {
                mode = match value(&mut args, &arg).as_str() {
                    "spawn" => RunMode::Spawn,
                    "persist" => RunMode::Persist,
                    other => usage_exit(&format!("未知 mode：{other}")),
                }
            }
            "--seed" => seed = value(&mut args, &arg).parse().unwrap_or_else(|_| usage_exit("--seed 需要整数")),
            "--label" => label = value(&mut args, &arg),
            "--json" => json_path = Some(value(&mut args, &arg)),
            "--help" | "-h" => {
                println!("std_thread_proto [--npcs 20007] [--ticks 3000] [--width 16] [--mode spawn|persist] [--seed 42] [--label L] [--json PATH]");
                std::process::exit(0);
            }
            other => usage_exit(&format!("未知参数：{other}")),
        }
    }

    let cfg = ProtoConfig::s5()
        .with_npc_total(npcs)
        .unwrap_or_else(|e| usage_exit(&e.to_string()))
        .with_ticks(ticks);
    let cfg = ProtoConfig { seed, ..cfg };

    println!(
        "# std-thread 原型：stocks={} npcs={} ticks={} width={} mode={:?} seed={}",
        cfg.stocks.len(),
        npcs,
        ticks,
        width,
        mode,
        seed
    );
    let report = match run_benchmark(&cfg, mode, width) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("基准失败（显式错误，不静默）：{e}");
            std::process::exit(1);
        }
    };

    println!("label = {}", if label.is_empty() { "-" } else { &label });
    println!("吞吐 = {:.1} t/s（simulated_seconds / wall_second）", report.ticks_per_sec);
    println!("wall = {:.3}s  cpu = {:.3}s  cpu/tick = {:.3} ms", report.wall_secs, report.cpu_secs, report.cpu_ms_per_tick);
    println!("平均核占用 = {:.2}", report.cpu_secs / report.wall_secs);
    println!(
        "峰值线程 = {}（决策线程精确峰值 {}，每股决策线程 {}）",
        report.peak_threads, report.peak_decision_threads, report.per_stock_threads
    );
    println!(
        "decide 调用 = {}（{:.0}/tick）  意图 = {}（{:.1}/tick）  成交 = {}（{:.1}/tick）  成交量 = {} 股",
        report.decide_calls,
        report.decide_calls as f64 / report.ticks as f64,
        report.intents,
        report.intents as f64 / report.ticks as f64,
        report.trades,
        report.trades as f64 / report.ticks as f64,
        report.volume_shares
    );
    println!(
        "挂单拒绝 = {}  到期撤单 = {}  对齐撤单 = {}",
        report.skipped_orders, report.expired_orders, report.cancelled_orders
    );
    for stock in &report.audit.stocks {
        println!(
            "  {} 持仓 {} / 流通 {}（remainder {}）成交 {} 笔 价带 [{}, {}]",
            stock.code,
            stock.held_shares,
            stock.float_total_shares,
            stock.float_remainder_shares,
            stock.trade_count,
            stock.min_trade_price_cents,
            stock.max_trade_price_cents
        );
    }
    println!(
        "守恒：现金 {} == {}，股票全部 OK",
        report.audit.final_total_cash_cents, report.audit.initial_total_cash_cents
    );

    if let Some(path) = json_path {
        let json = serde_json::json!({
            "label": label,
            "mode": format!("{:?}", mode),
            "width": width,
            "per_stock_threads": report.per_stock_threads,
            "stocks": cfg.stocks.len(),
            "npcs_total": npcs,
            "ticks": report.ticks,
            "seed": seed,
            "ticks_per_sec": report.ticks_per_sec,
            "wall_secs": report.wall_secs,
            "cpu_secs": report.cpu_secs,
            "cpu_ms_per_tick": report.cpu_ms_per_tick,
            "avg_cores": report.cpu_secs / report.wall_secs,
            "peak_threads": report.peak_threads,
            "peak_decision_threads": report.peak_decision_threads,
            "decide_calls": report.decide_calls,
            "intents": report.intents,
            "trades": report.trades,
            "volume_shares": report.volume_shares,
            "skipped_orders": report.skipped_orders,
            "expired_orders": report.expired_orders,
            "cancelled_orders": report.cancelled_orders,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&json).expect("JSON 序列化"))
            .unwrap_or_else(|e| panic!("写 {path} 失败：{e}"));
        println!("JSON 已写 {path}");
    }
}

fn usage_exit(msg: &str) -> ! {
    eprintln!("参数错误：{msg}");
    eprintln!("用法：std_thread_proto [--npcs 20007] [--ticks 3000] [--width 16] [--mode spawn|persist] [--seed 42] [--label L] [--json PATH]");
    std::process::exit(2);
}
