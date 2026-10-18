//! std::thread 原型正确性集成测试（先于实现编写，TDD 红→绿）。
//!
//! 验证三条守恒与结构不变量（设计见 .tmp/std-thread-proto/design.md §7）：
//! 1. 现金守恒：Σ NPC cash（含挂单冻结）+ fee sink ≡ 初始总现金；
//! 2. 股票守恒：每股 Σ持仓 + float_remainder ≡ 流通股总数；
//! 3. 结构不变量：cash ≥ 0、qty = sellable + reserved + frozen_today、整手、价格带。

use std_thread_proto::{
    run_benchmark, run_cross_benchmark, run_own_benchmark, ProtoConfig, RunMode,
};

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
        assert_eq!(
            stock.negative_cash_accounts, 0,
            "股票 {} 出现负现金账户",
            stock.code
        );
        assert_eq!(
            stock.t1_violations, 0,
            "股票 {} T+1 结构破坏 qty≠sellable+reserved+frozen",
            stock.code
        );
        assert_eq!(
            stock.lot_violations, 0,
            "股票 {} 出现非整手成交/挂单",
            stock.code
        );
    }
}

#[test]
fn 同种子串行两跑结果一致() {
    // 并发下不要求可复现，但 width=0（worker 线程内联决策）时处理顺序确定，
    // 两次运行应得到一致的成交数与期末审计——用于排除数据竞争导致的静默状态破坏。
    let a = run_benchmark(&small(42), RunMode::Spawn, 0).expect("第一跑");
    let b = run_benchmark(&small(42), RunMode::Spawn, 0).expect("第二跑");
    assert_eq!(
        a.trades, b.trades,
        "串行两跑成交数不一致：{} vs {}",
        a.trades, b.trades
    );
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
    assert!(
        persist.ticks_per_sec > spawn.ticks_per_sec * 0.1,
        "persist 吞吐异常低：{:.1} vs spawn {:.1}",
        persist.ticks_per_sec,
        spawn.ticks_per_sec
    );
}

/// 跨股臂守恒：账户跨股持仓 + intent 路由 + escrow + 回执记账下，
/// 现金/股票守恒与结构不变量必须闭合（Drain 清空在途 escrow 后审计）。
#[test]
fn 跨股臂_现金与股票守恒() {
    let report = run_cross_benchmark(&small(7), 0, 50).expect("跨股臂运行必须成功");
    assert_eq!(
        report.audit.initial_total_cash_cents, report.audit.final_total_cash_cents,
        "跨股臂现金守恒破坏：initial={} final={}",
        report.audit.initial_total_cash_cents, report.audit.final_total_cash_cents
    );
    for stock in &report.audit.stocks {
        assert_eq!(
            stock.held_shares + stock.float_remainder_shares,
            stock.float_total_shares,
            "跨股臂股票 {} 守恒破坏：held={} remainder={} total={}",
            stock.code,
            stock.held_shares,
            stock.float_remainder_shares,
            stock.float_total_shares
        );
        if stock.trade_count > 0 {
            assert!(
                stock.min_trade_price_cents >= stock.lower_limit_cents
                    && stock.max_trade_price_cents <= stock.upper_limit_cents,
                "跨股臂股票 {} 成交价越界：[{}, {}] 不在 [{}, {}]",
                stock.code,
                stock.min_trade_price_cents,
                stock.max_trade_price_cents,
                stock.lower_limit_cents,
                stock.upper_limit_cents
            );
        }
    }
    assert_eq!(
        report.audit.negative_cash_accounts, 0,
        "跨股臂出现负现金账户"
    );
    assert_eq!(report.audit.t1_violations, 0, "跨股臂 T+1 结构破坏");
    assert_eq!(report.audit.lot_violations, 0, "跨股臂出现非整手持仓");
}

/// 跨股臂流量健全性：构造 50% 跨股持仓时必须真的产生跨股路由、
/// 成交回执与次级仓买卖（否则测试空转）。
#[test]
fn 跨股臂_路由流量与次级仓活动() {
    let report = run_cross_benchmark(&small(11), 0, 50).expect("跨股臂运行必须成功");
    assert!(report.trades > 0, "跨股臂必须产生真实成交");
    assert!(report.routed_places > 0, "跨股臂必须产生跨股路由下单");
    let receipts: u64 = report.receipts_applied.iter().sum();
    assert!(receipts > 0, "跨股臂必须产生成交/终结回执");
    assert!(
        report.receipts_applied[0] > 0 && report.receipts_applied[1] > 0,
        "跨股臂必须同时出现买/卖回执（BuyFill={} SellFill={}）",
        report.receipts_applied[0],
        report.receipts_applied[1]
    );
    assert!(
        report.cross_fraction > 0.2,
        "50% 构造参数下实际跨股持仓率异常低：{:.3}",
        report.cross_fraction
    );
}

/// 跨股臂零跨股对照（协议空载）：相位结构存在但不路由时守恒仍闭合。
#[test]
fn 跨股臂_零跨股对照() {
    let report = run_cross_benchmark(&small(13), 0, 0).expect("零跨股对照必须成功");
    assert_eq!(
        report.audit.initial_total_cash_cents, report.audit.final_total_cash_cents,
        "零跨股对照现金守恒破坏"
    );
    assert_eq!(report.routed_places, 0, "零跨股对照不应产生路由下单");
    assert!(report.trades > 0, "零跨股对照仍应产生本股成交");
}

// ── 所有权转移臂（第三原型）：资源实体实际 move（非 escrow）──

/// 所有权臂守恒：现金/股票实体在组线程 ↔ 交易部线程间实际搬迁、
/// 失败原路退回、Drain 清空在途实体后，现金/股票守恒与结构不变量必须闭合。
#[test]
fn 所有权臂_现金与股票守恒() {
    let report = run_own_benchmark(&small(7), 3, 50).expect("所有权臂运行必须成功");
    assert_eq!(
        report.audit.initial_total_cash_cents, report.audit.final_total_cash_cents,
        "所有权臂现金守恒破坏：initial={} final={}",
        report.audit.initial_total_cash_cents, report.audit.final_total_cash_cents
    );
    for stock in &report.audit.stocks {
        assert_eq!(
            stock.held_shares + stock.float_remainder_shares,
            stock.float_total_shares,
            "所有权臂股票 {} 守恒破坏：held={} remainder={} total={}",
            stock.code,
            stock.held_shares,
            stock.float_remainder_shares,
            stock.float_total_shares
        );
        if stock.trade_count > 0 {
            assert!(
                stock.min_trade_price_cents >= stock.lower_limit_cents
                    && stock.max_trade_price_cents <= stock.upper_limit_cents,
                "所有权臂股票 {} 成交价越界：[{}, {}] 不在 [{}, {}]",
                stock.code,
                stock.min_trade_price_cents,
                stock.max_trade_price_cents,
                stock.lower_limit_cents,
                stock.upper_limit_cents
            );
        }
    }
    assert_eq!(
        report.audit.negative_cash_accounts, 0,
        "所有权臂出现负现金账户"
    );
    assert_eq!(report.audit.t1_violations, 0, "所有权臂 T+1 结构破坏");
    assert_eq!(report.audit.lot_violations, 0, "所有权臂出现非整手持仓");
}

/// 所有权臂流量健全性：50% 跨股构造下必须真的产生跨股实体转移
/// （现金实体流出、股票实体回流、跨股下单），否则测试空转。
#[test]
fn 所有权臂_实体转移流量() {
    let report = run_own_benchmark(&small(11), 2, 50).expect("所有权臂运行必须成功");
    assert!(report.trades > 0, "所有权臂必须产生真实成交");
    assert!(
        report.cross_places > 0,
        "所有权臂必须产生跨股下单（目标≠home）"
    );
    assert!(
        report.entities.bought_shares > 0 && report.entities.cash_proceeds > 0,
        "所有权臂买/卖成交实体必须回流（BoughtShares={} CashProceeds={}）",
        report.entities.bought_shares,
        report.entities.cash_proceeds
    );
    assert!(report.entities.closed > 0, "所有权臂必须有挂单终结标记回流");
    assert!(
        report.cross_fraction > 0.2,
        "50% 构造参数下实际跨股持仓率异常低：{:.3}",
        report.cross_fraction
    );
}

/// 所有权臂零跨股对照（纯本股流动）：实体通道空载时守恒仍闭合。
#[test]
fn 所有权臂_零跨股对照() {
    let report = run_own_benchmark(&small(13), 5, 0).expect("零跨股对照必须成功");
    assert_eq!(
        report.audit.initial_total_cash_cents, report.audit.final_total_cash_cents,
        "零跨股对照现金守恒破坏"
    );
    assert_eq!(report.cross_places, 0, "零跨股对照不应产生跨股下单");
    assert!(report.trades > 0, "零跨股对照仍应产生本股成交");
}
