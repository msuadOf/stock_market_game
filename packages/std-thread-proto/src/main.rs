//! CLI：s5 负载基准入口。
//!
//! 用法示例：
//! ```text
//! std_thread_proto --width 16 --mode spawn --label s5-16w-spawn
//! std_thread_proto --npcs 20007 --ticks 3000 --width 128 --mode spawn
//! std_thread_proto --model cross --xrate 30 --width 0 --label x30-inline
//! ```
//! 输出人读摘要 + 可选 JSON（--json 路径）。
#![recursion_limit = "512"] // 跨股臂 JSON 字段多，json! 宏展开需要更高递归上限

use std_thread_proto::{
    run_benchmark, run_cross_benchmark, run_own_benchmark, ProtoConfig, RunMode,
};

fn main() {
    let mut npcs: u32 = 20_007;
    let mut ticks: u64 = 3_000;
    let mut width: u32 = 16;
    let mut mode = RunMode::Spawn;
    let mut seed: u64 = 42;
    let mut model = String::from("per-stock");
    let mut cross_pct: u32 = 30;
    let mut groups: u32 = 5;
    let mut label = String::new();
    let mut json_path: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        fn value(args: &mut impl Iterator<Item = String>, name: &str) -> String {
            args.next()
                .unwrap_or_else(|| usage_exit(&format!("参数 {name} 缺少取值")))
        }
        match arg.as_str() {
            "--npcs" => {
                npcs = value(&mut args, &arg)
                    .parse()
                    .unwrap_or_else(|_| usage_exit("--npcs 需要整数"))
            }
            "--ticks" => {
                ticks = value(&mut args, &arg)
                    .parse()
                    .unwrap_or_else(|_| usage_exit("--ticks 需要整数"))
            }
            "--width" => {
                width = value(&mut args, &arg)
                    .parse()
                    .unwrap_or_else(|_| usage_exit("--width 需要整数"))
            }
            "--mode" => {
                mode = match value(&mut args, &arg).as_str() {
                    "spawn" => RunMode::Spawn,
                    "persist" => RunMode::Persist,
                    other => usage_exit(&format!("未知 mode：{other}")),
                }
            }
            "--model" => {
                let m = value(&mut args, &arg);
                if m != "per-stock" && m != "cross" && m != "own" {
                    usage_exit("未知 model：只支持 per-stock | cross | own");
                }
                model = m;
            }
            "--xrate" => {
                cross_pct = value(&mut args, &arg)
                    .parse()
                    .unwrap_or_else(|_| usage_exit("--xrate 需要 0~100 整数"));
                if cross_pct > 100 {
                    usage_exit("--xrate 需要 0~100 整数");
                }
            }
            "--groups" => {
                groups = value(&mut args, &arg)
                    .parse()
                    .unwrap_or_else(|_| usage_exit("--groups 需要 1~255 整数"));
                if groups == 0 || groups > 255 {
                    usage_exit("--groups 需要 1~255 整数");
                }
            }
            "--seed" => {
                seed = value(&mut args, &arg)
                    .parse()
                    .unwrap_or_else(|_| usage_exit("--seed 需要整数"))
            }
            "--label" => label = value(&mut args, &arg),
            "--json" => json_path = Some(value(&mut args, &arg)),
            "--help" | "-h" => {
                println!("std_thread_proto [--model per-stock|cross|own] [--xrate 30] [--groups 5] [--npcs 20007] [--ticks 3000] [--width 16] [--mode spawn|persist] [--seed 42] [--label L] [--json PATH]");
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

    if model == "own" {
        if width != 0 {
            usage_exit("own 模型为固定拓扑（--groups 组线程 + 每股一交易部线程），不支持 --width");
        }
        run_own(&cfg, groups, cross_pct, &label, json_path, npcs);
        return;
    }

    if model == "cross" {
        run_cross(&cfg, width, cross_pct, &label, json_path, npcs);
        return;
    }

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
    println!(
        "吞吐 = {:.1} t/s（simulated_seconds / wall_second）",
        report.ticks_per_sec
    );
    println!(
        "wall = {:.3}s  cpu = {:.3}s  cpu/tick = {:.3} ms",
        report.wall_secs, report.cpu_secs, report.cpu_ms_per_tick
    );
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
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json).expect("JSON 序列化"),
        )
        .unwrap_or_else(|e| panic!("写 {path} 失败：{e}"));
        println!("JSON 已写 {path}");
    }
}

/// 所有权转移臂入口：人读摘要 + JSON。
fn run_own(
    cfg: &ProtoConfig,
    groups: u32,
    cross_pct: u32,
    label: &str,
    json_path: Option<String>,
    npcs: u32,
) {
    println!(
        "# std-thread 所有权转移臂：stocks={} npcs={} ticks={} groups={} cross={}%(retail 持≥2股) seed={}",
        cfg.stocks.len(),
        npcs,
        cfg.ticks,
        groups,
        cross_pct,
        cfg.seed
    );
    let report = match run_own_benchmark(cfg, groups, cross_pct) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("所有权臂基准失败（显式错误，不静默）：{e}");
            std::process::exit(1);
        }
    };
    println!("label = {}", if label.is_empty() { "-" } else { label });
    println!(
        "吞吐 = {:.1} t/s（simulated_seconds / wall_second）",
        report.ticks_per_sec
    );
    println!(
        "wall = {:.3}s  cpu = {:.3}s  cpu/tick = {:.3} ms",
        report.wall_secs, report.cpu_secs, report.cpu_ms_per_tick
    );
    println!("平均核占用 = {:.2}", report.cpu_secs / report.wall_secs);
    println!(
        "相位等待（ms/tick）：decide(收齐组)={:.3} match(收齐交易部)={:.3} dispatch={:.3}",
        report.decide_wait_ms, report.match_wait_ms, report.dispatch_ms
    );
    println!(
        "峰值线程 = {}（常驻：组 {} + 交易部 {} + main；无每 tick 决策线程）",
        report.peak_threads,
        report.groups,
        cfg.stocks.len()
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
        "跨股：持≥2股 retail {:.1}%（1只股={} 2只股={} 3只股={}）  下单 home={} cross={}  对齐撤单 = {}",
        report.cross_fraction * 100.0,
        report.hold_counts[0],
        report.hold_counts[1],
        report.hold_counts[2],
        report.home_places,
        report.cross_places,
        report.align_cancels
    );
    println!(
        "实体转移：CashProceeds={} CashRefund={} BoughtShares={} RefundedShares={} Closed={}（合计 {:.0}/tick）",
        report.entities.cash_proceeds,
        report.entities.cash_refund,
        report.entities.bought_shares,
        report.entities.refunded_shares,
        report.entities.closed,
        (report.entities.cash_proceeds
            + report.entities.cash_refund
            + report.entities.bought_shares
            + report.entities.refunded_shares
            + report.entities.closed) as f64
            / report.ticks as f64
    );
    println!(
        "channel 消息：组→交易部批次 = {}（{:.0}/tick）  交易部→组批次 = {}（{:.0}/tick）",
        report.group_flushes,
        report.group_flushes as f64 / report.ticks as f64,
        report.desk_batches,
        report.desk_batches as f64 / report.ticks as f64
    );
    println!(
        "挂单拒绝 = {}（组端 {}）  到期撤单 = {}  对齐撤单成交 = {}",
        report.skipped_orders, report.group_skipped, report.expired_orders, report.cancelled_orders
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
        "守恒：现金 {} == {}；负现金={} t1破坏={} 非整手={}",
        report.audit.final_total_cash_cents,
        report.audit.initial_total_cash_cents,
        report.audit.negative_cash_accounts,
        report.audit.t1_violations,
        report.audit.lot_violations
    );

    if let Some(path) = json_path {
        let json = serde_json::json!({
            "model": "own",
            "label": label,
            "groups": report.groups,
            "cross_pct": cross_pct,
            "cross_fraction": report.cross_fraction,
            "hold_counts": report.hold_counts,
            "stocks": cfg.stocks.len(),
            "npcs_total": npcs,
            "ticks": report.ticks,
            "seed": cfg.seed,
            "ticks_per_sec": report.ticks_per_sec,
            "wall_secs": report.wall_secs,
            "cpu_secs": report.cpu_secs,
            "cpu_ms_per_tick": report.cpu_ms_per_tick,
            "avg_cores": report.cpu_secs / report.wall_secs,
            "peak_threads": report.peak_threads,
            "decide_wait_ms": report.decide_wait_ms,
            "match_wait_ms": report.match_wait_ms,
            "dispatch_ms": report.dispatch_ms,
            "decide_calls": report.decide_calls,
            "intents": report.intents,
            "trades": report.trades,
            "volume_shares": report.volume_shares,
            "skipped_orders": report.skipped_orders,
            "group_skipped": report.group_skipped,
            "expired_orders": report.expired_orders,
            "cancelled_orders": report.cancelled_orders,
            "home_places": report.home_places,
            "cross_places": report.cross_places,
            "align_cancels": report.align_cancels,
            "cancel_intents": report.cancel_intents,
            "ent_cash_proceeds": report.entities.cash_proceeds,
            "ent_cash_refund": report.entities.cash_refund,
            "ent_bought_shares": report.entities.bought_shares,
            "ent_refunded_shares": report.entities.refunded_shares,
            "ent_closed": report.entities.closed,
            "group_flushes": report.group_flushes,
            "desk_batches": report.desk_batches,
            "audit_initial_cash": report.audit.initial_total_cash_cents,
            "audit_final_cash": report.audit.final_total_cash_cents,
            "negative_cash_accounts": report.audit.negative_cash_accounts,
            "t1_violations": report.audit.t1_violations,
            "lot_violations": report.audit.lot_violations,
        });
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json).expect("JSON 序列化"),
        )
        .unwrap_or_else(|e| panic!("写 {path} 失败：{e}"));
        println!("JSON 已写 {path}");
    }
}

/// 跨股臂入口：人读摘要 + JSON。
fn run_cross(
    cfg: &ProtoConfig,
    width: u32,
    cross_pct: u32,
    label: &str,
    json_path: Option<String>,
    npcs: u32,
) {
    println!(
        "# std-thread 跨股臂：stocks={} npcs={} ticks={} width={} cross={}%(retail 持≥2股) seed={}",
        cfg.stocks.len(),
        npcs,
        cfg.ticks,
        width,
        cross_pct,
        cfg.seed
    );
    let report = match run_cross_benchmark(cfg, width, cross_pct) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("跨股基准失败（显式错误，不静默）：{e}");
            std::process::exit(1);
        }
    };
    println!("label = {}", if label.is_empty() { "-" } else { label });
    println!(
        "吞吐 = {:.1} t/s（simulated_seconds / wall_second）",
        report.ticks_per_sec
    );
    println!(
        "wall = {:.3}s  cpu = {:.3}s  cpu/tick = {:.3} ms",
        report.wall_secs, report.cpu_secs, report.cpu_ms_per_tick
    );
    println!("平均核占用 = {:.2}", report.cpu_secs / report.wall_secs);
    println!(
        "相位等待/分拣（ms/tick）：phase1={:.3} phase2={:.3} dispatch={:.3}",
        report.phase1_wait_ms, report.phase2_wait_ms, report.dispatch_ms
    );
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
        "跨股：持≥2股 retail {:.1}%（1只股={} 2只股={} 3只股={}）  路由下单 = {}（{:.0}/tick）  对齐撤单路由 = {}  空转撤单 = {}",
        report.cross_fraction * 100.0,
        report.hold_counts[0],
        report.hold_counts[1],
        report.hold_counts[2],
        report.routed_places,
        report.routed_places as f64 / report.ticks as f64,
        report.routed_cancels,
        report.spurious_cancels
    );
    println!(
        "回执：BuyFill={} SellFill={} BuyClosed={} SellClosed={}（合计 {:.0}/tick）  席位峰值={}  防御跳过={}",
        report.receipts_applied[0],
        report.receipts_applied[1],
        report.receipts_applied[2],
        report.receipts_applied[3],
        report.receipts_applied.iter().sum::<u64>() as f64 / report.ticks as f64,
        report.desks_peak,
        report.foreign_skips
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
        "守恒：现金 {} == {}，股票全部 OK；负现金={} t1破坏={} 非整手={}",
        report.audit.final_total_cash_cents,
        report.audit.initial_total_cash_cents,
        report.audit.negative_cash_accounts,
        report.audit.t1_violations,
        report.audit.lot_violations
    );

    if let Some(path) = json_path {
        let json = serde_json::json!({
            "model": "cross",
            "label": label,
            "cross_pct": cross_pct,
            "cross_fraction": report.cross_fraction,
            "hold_counts": report.hold_counts,
            "width": width,
            "per_stock_threads": report.per_stock_threads,
            "stocks": cfg.stocks.len(),
            "npcs_total": npcs,
            "ticks": report.ticks,
            "seed": cfg.seed,
            "ticks_per_sec": report.ticks_per_sec,
            "wall_secs": report.wall_secs,
            "cpu_secs": report.cpu_secs,
            "cpu_ms_per_tick": report.cpu_ms_per_tick,
            "avg_cores": report.cpu_secs / report.wall_secs,
            "phase1_wait_ms": report.phase1_wait_ms,
            "phase2_wait_ms": report.phase2_wait_ms,
            "dispatch_ms": report.dispatch_ms,
            "peak_threads": report.peak_threads,
            "peak_decision_threads": report.peak_decision_threads,
            "decide_calls": report.decide_calls,
            "intents": report.intents,
            "trades": report.trades,
            "volume_shares": report.volume_shares,
            "skipped_orders": report.skipped_orders,
            "expired_orders": report.expired_orders,
            "cancelled_orders": report.cancelled_orders,
            "routed_places": report.routed_places,
            "routed_cancels": report.routed_cancels,
            "spurious_cancels": report.spurious_cancels,
            "foreign_skips": report.foreign_skips,
            "receipts_buy_fill": report.receipts_applied[0],
            "receipts_sell_fill": report.receipts_applied[1],
            "receipts_buy_closed": report.receipts_applied[2],
            "receipts_sell_closed": report.receipts_applied[3],
            "desks_peak": report.desks_peak,
            "audit_initial_cash": report.audit.initial_total_cash_cents,
            "audit_final_cash": report.audit.final_total_cash_cents,
            "negative_cash_accounts": report.audit.negative_cash_accounts,
            "t1_violations": report.audit.t1_violations,
            "lot_violations": report.audit.lot_violations,
        });
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json).expect("JSON 序列化"),
        )
        .unwrap_or_else(|e| panic!("写 {path} 失败：{e}"));
        println!("JSON 已写 {path}");
    }
}

fn usage_exit(msg: &str) -> ! {
    eprintln!("参数错误：{msg}");
    eprintln!("用法：std_thread_proto [--npcs 20007] [--ticks 3000] [--width 16] [--mode spawn|persist] [--seed 42] [--label L] [--json PATH]");
    std::process::exit(2);
}
