//! std::thread 原型正确性集成测试（先于实现编写，TDD 红→绿）。
//!
//! 验证三条守恒与结构不变量（设计见 .tmp/std-thread-proto/design.md §7）：
//! 1. 现金守恒：Σ NPC cash（含挂单冻结）+ fee sink ≡ 初始总现金；
//! 2. 股票守恒：每股 Σ持仓 + float_remainder ≡ 流通股总数；
//! 3. 结构不变量：cash ≥ 0、qty = sellable + reserved + frozen_today、整手、价格带。

use std_thread_proto::{run_benchmark, ProtoConfig, RunMode};

/// 小世界 fixture：2 股、每股 40 retail + 1 inst、hot 2 只放前两股、60 tick。
/// 目标是让每条 case 在 10 秒硬上限内完成（含小规模撮合压力）。
fn small(seed: u64) -> ProtoConfig {
    ProtoConfig::small(seed)
}

#[test]
fn 现金守恒_spawn模式() {
    let report = run_benchmark(&small(7), RunMode::Spawn, 4).expect("基准运行必须成功");
    assert!(
        report.audit.initial_total_cash_cents == report.audit.final_total_cash_cents,
        "现金守恒破坏：initial={} final={}",
        report.audit.initial_total_cash_cents,
        report.audit.final_total_cash_cents
    );
    assert!(report.trades > 0, "小世界必须产生真实成交才有验证力");
}

#[test]
fn 股票守恒与价格带() {
    let report = run_benchmark(&small(11), RunMode::Spawn, 3).expect("基准运行必须成功");
    for stock in &report.audit.stocks {
        assert_eq!(
            stock.held_shares + stock.float_remainder_shares,
            stock.float_total_shares,
            "股票 {} 守恒破坏：held={} remainder={} total={}",
            stock.code,
            stock.held_shares,
            stock.float_remainder_shares,
            stock.float_total_shares
        );
        if stock.trade_count > 0 {
            assert!(
                stock.min_trade_price_cents >= stock.lower_limit_cents
                    && stock.max_trade_price_cents <= stock.upper_limit_cents,
                "股票 {} 成交价越界：[{}, {}] 不在 [{}, {}]",
                stock.code,
                stock.min_trade_price_cents,
                stock.max_trade_price_cents,
                stock.lower_limit_cents,
                stock.upper_limit_cents
            );
        }
    }
}

#[test]
fn 不变量_现金非负_t1结构_整手() {
    let report = run_benchmark(&small(13), RunMode::Persist, 4).expect("基准运行必须成功");
    for stock in &report.audit.stocks {
        assert_eq!(stock.negative_cash_accounts, 0, "股票 {} 出现负现金账户", stock.code);
        assert_eq!(stock.t1_violations, 0, "股票 {} T+1 结构破坏 qty≠sellable+reserved+frozen", stock.code);
        assert_eq!(stock.lot_violations, 0, "股票 {} 出现非整手成交/挂单", stock.code);
    }
}

#[test]
fn 同种子串行两跑结果一致() {
    // 并发下不要求可复现，但 width=0（worker 线程内联决策）时处理顺序确定，
    // 两次运行应得到一致的成交数与期末审计——用于排除数据竞争导致的静默状态破坏。
    let a = run_benchmark(&small(42), RunMode::Spawn, 0).expect("第一跑");
    let b = run_benchmark(&small(42), RunMode::Spawn, 0).expect("第二跑");
    assert_eq!(a.trades, b.trades, "串行两跑成交数不一致：{} vs {}", a.trades, b.trades);
    assert_eq!(
        a.audit.final_total_cash_cents, b.audit.final_total_cash_cents,
        "串行两跑期末现金不一致"
    );
    for (sa, sb) in a.audit.stocks.iter().zip(b.audit.stocks.iter()) {
        assert_eq!(sa.held_shares, sb.held_shares, "串行两跑持仓不一致");
    }
}

#[test]
fn persist模式吞吐与spawn同数量级() {
    // 对照臂健全性：persist 不应比 spawn 慢一个数量级（否则实现有问题）。
    let spawn = run_benchmark(&small(99), RunMode::Spawn, 4).expect("spawn 跑");
    let persist = run_benchmark(&small(99), RunMode::Persist, 4).expect("persist 跑");
    assert!(persist.ticks_per_sec > spawn.ticks_per_sec * 0.1,
        "persist 吞吐异常低：{:.1} vs spawn {:.1}",
        persist.ticks_per_sec, spawn.ticks_per_sec);
}
