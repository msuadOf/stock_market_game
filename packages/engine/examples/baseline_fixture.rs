//! 变更前基线固定装置：用当前公开 setup API 显式构造「当前合法 setup 输入」并运行
//! 单 seed 量价基线。与 `price_volume_baseline` 不同，本工具不读取任何存档文件——
//! setup 在代码内构造、每次创建全新会话，绝不恢复或改写用户存档。
//!
//! 两个场景，绝不混在同一份报告：
//! - `matrix`：`apps/web/src/config/defaults.ts` `DEFAULT_SETUP` 的等值 Rust 副本
//!   （完整 15300 tick/日：900 开盘窗口 + 14400 连续竞价 + 180 收盘集合竞价）。
//! - `compressed-300`：`packages/engine/tests/session.rs`
//!   `large_retail_account_setup(20_000)` 的等值副本（压缩 300 tick/日成本场景）。
//!
//! 副本漂移由 `scripts/simulation/baseline-run.mjs` 的对账校验与仓库测试共同看住。

use std::{env, process};

use engine::{
    run_price_volume_baseline, FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcSetup,
    RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams, VParams,
};

const USAGE: &str = "用法：\n  cargo run -p engine --release --example baseline_fixture -- <matrix|compressed-300> <seed> [trading_days=30]";

fn main() {
    if let Err(error) = run() {
        eprintln!("基线固定装置运行失败：{error}\n\n{USAGE}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.len() < 2 || args.len() > 3 {
        return Err(format!("需要 2–3 个参数，实际收到 {} 个", args.len()));
    }
    let scenario_name = args[0].as_str();
    let seed = args[1]
        .parse::<u64>()
        .map_err(|error| format!("seed `{}` 不是 u64：{error}", args[1]))?;
    let trading_days = match args.get(2) {
        Some(raw) => raw
            .parse::<u32>()
            .map_err(|error| format!("交易日数 `{raw}` 不是 u32：{error}"))?,
        None => 30,
    };
    if trading_days == 0 {
        return Err("交易日数必须大于 0".to_string());
    }

    let setup = scenario_setup(scenario_name)?;
    setup
        .validate()
        .map_err(|error| format!("场景 `{scenario_name}` 的 setup 非法：{error}"))?;
    let report = run_price_volume_baseline(&setup, &[seed], trading_days)
        .map_err(|error| format!("场景 `{scenario_name}` seed {seed} 基线失败：{error}"))?;

    let mut stock_codes: Vec<String> = setup.stocks.iter().map(|s| s.code.0.clone()).collect();
    stock_codes.sort();
    let wrapper = serde_json::json!({
        "tool": "baseline_fixture",
        "scenario": scenario_name,
        "seed": seed.to_string(),
        "trading_days": trading_days,
        "config": {
            "stock_codes": stock_codes,
            "retail_count": setup.npcs.retail_count,
            "inst_count": setup.npcs.inst_count,
            "hot_count": setup.npcs.hot_count,
            "ticks_per_day": setup.ticks_per_day.to_string(),
            "auction_ticks": setup.auction_ticks.to_string(),
            "closing_auction_ticks": setup.closing_auction_ticks.to_string(),
            "history_len": setup.history_len,
            "t1_enabled": setup.t1_enabled,
        },
        "report": report,
    });
    let output = serde_json::to_string_pretty(&wrapper)
        .map_err(|error| format!("报告 JSON 序列化失败：{error}"))?;
    println!("{output}");
    Ok(())
}

fn scenario_setup(name: &str) -> Result<SessionSetup, String> {
    match name {
        "matrix" => Ok(matrix_setup()),
        "compressed-300" => Ok(compressed_setup()),
        other => Err(format!(
            "未知场景 `{other}`；仅支持 matrix 或 compressed-300"
        )),
    }
}

/// 由 StockSpec 的自然字段构造单只股票；涨跌幅跟随证券类别的现行规则。
fn stock(
    code: &str,
    exchange: StockExchange,
    category: SecurityCategory,
    price_cents: i64,
    total_shares: u64,
    float_shares: u32,
) -> StockSpec {
    StockSpec {
        code: StockCode(code.to_string()),
        exchange,
        initial_price: Money::from_cents(price_cents),
        category,
        limit_pct: category.limit_pct(),
        v_initial: Money::from_cents(price_cents),
        tick: Money::from_cents(1),
        total_shares,
        float_shares,
    }
}

/// 矩阵场景：web 端 `DEFAULT_SETUP` 的等值副本（真源见该文件，2026-09 revision 6ad461e）。
fn matrix_setup() -> SessionSetup {
    let stocks = vec![
        stock(
            "600101",
            StockExchange::Shanghai,
            SecurityCategory::MainBoard,
            1120,
            8_928_571_429,
            3_571_428_571,
        ),
        stock(
            "002156",
            StockExchange::Shenzhen,
            SecurityCategory::MainBoard,
            2735,
            2_925_045_704,
            2_047_531_993,
        ),
        stock(
            "300260",
            StockExchange::Shenzhen,
            SecurityCategory::ChiNext,
            3680,
            815_217_391,
            611_413_043,
        ),
        stock(
            "600610",
            StockExchange::Shanghai,
            SecurityCategory::MainBoard,
            755,
            1_059_602_649,
            847_682_119,
        ),
        stock(
            "000812",
            StockExchange::Shenzhen,
            SecurityCategory::StMainBoard,
            285,
            1_052_631_579,
            842_105_263,
        ),
    ];
    let fundamental_value_means = stocks
        .iter()
        .map(|spec| (spec.code.clone(), spec.v_initial))
        .collect();
    SessionSetup {
        stocks,
        npcs: NpcSetup {
            retail_count: 20_000,
            inst_count: 5,
            hot_count: 2,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        // 与 web DEFAULT_SETUP.config 一致（玩家初始资金 1 千万分，其余为现行 A 股基线）。
        config: GameConfig::new(
            0.00025,
            Money::from_cents(500),
            0.0005,
            0.10,
            0.10,
            100,
            Money::from_cents(1_000_000_000),
        )
        .expect("web DEFAULT_SETUP 副本恒合法；失败说明副本与真源不一致"),
        v_params: VParams {
            long_run_mean: Money::from_cents(1120),
            mean_reversion: 0.5,
            volatility: 0.02,
        },
        fundamental_value_means,
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.3,
                order_size_mean: 300,
                chase_prob: 0.4,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 200_000,
            },
            hot: HotParams {
                lookback: 20,
                trend_threshold: 0.03,
                order_size: 100_000,
            },
        },
        // 09:15–09:30 开盘窗口 900 tick + 09:30–14:57 连续竞价 14400 tick +
        // 14:57–15:00 收盘集合竞价 180 tick；一个 tick 为一秒。
        ticks_per_day: 15_300,
        auction_ticks: 900,
        closing_auction_ticks: 180,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.45,
            inst: 0.53,
            hot: 0.02,
        },
    }
}

/// 压缩成本场景：engine 大规模账户测试 `large_retail_account_setup(20_000)` 的等值副本。
fn compressed_setup() -> SessionSetup {
    let stocks = ["600101", "002156", "300260", "600610", "000812"]
        .into_iter()
        .map(|code| {
            let category = if code.starts_with("300") {
                SecurityCategory::ChiNext
            } else if code == "000812" {
                SecurityCategory::StMainBoard
            } else {
                SecurityCategory::MainBoard
            };
            let exchange = if code.starts_with('6') {
                StockExchange::Shanghai
            } else {
                StockExchange::Shenzhen
            };
            stock(code, exchange, category, 1_000, 100_000_000, 80_000_000)
        })
        .collect();
    let fundamental_value_means = ["600101", "002156", "300260", "600610", "000812"]
        .into_iter()
        .map(|code| (StockCode(code.to_string()), Money::from_cents(1_000)))
        .collect();
    SessionSetup {
        stocks,
        npcs: NpcSetup {
            retail_count: 20_000,
            inst_count: 5,
            hot_count: 2,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(1_000),
            mean_reversion: 0.5,
            volatility: 0.0,
        },
        fundamental_value_means,
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.5,
                order_size_mean: 100,
                chase_prob: 0.2,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        // 压缩墙钟粒度（300 tick/日、无集合竞价窗口）以覆盖完整日终压力；
        // 与 matrix 场景分开报告，绝不混用。
        ticks_per_day: 300,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.45,
            inst: 0.53,
            hot: 0.02,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{compressed_setup, matrix_setup, scenario_setup};
    use engine::{Money, StockExchange};

    const SORTED_CODES: [&str; 5] = ["000812", "002156", "300260", "600101", "600610"];

    #[test]
    fn both_scenarios_are_current_legal_setups() {
        for setup in [matrix_setup(), compressed_setup()] {
            setup.validate().expect("场景 setup 必须通过现行合法性校验");
            assert_eq!(setup.npcs.retail_count, 20_000);
            assert_eq!(setup.npcs.inst_count, 5);
            assert_eq!(setup.npcs.hot_count, 2);
            let mut codes: Vec<String> = setup.stocks.iter().map(|s| s.code.0.clone()).collect();
            codes.sort();
            assert_eq!(codes, SORTED_CODES);
        }
    }

    /// matrix 是 apps/web/src/config/defaults.ts DEFAULT_SETUP 的逐字段副本；
    /// 期望值按该真源硬编码，任何一侧漂移都会在此失败，防止悄悄换掉 before 锚点输入。
    #[test]
    fn matrix_pins_every_default_setup_field() {
        let setup = matrix_setup();
        // (代码, 交易所, 类别涨跌幅, 初始价/分, 总股本, 流通股)
        let pinned: [(&str, StockExchange, f64, i64, u64, u32); 5] = [
            (
                "600101",
                StockExchange::Shanghai,
                0.10,
                1120,
                8_928_571_429,
                3_571_428_571,
            ),
            (
                "002156",
                StockExchange::Shenzhen,
                0.10,
                2735,
                2_925_045_704,
                2_047_531_993,
            ),
            (
                "300260",
                StockExchange::Shenzhen,
                0.20,
                3680,
                815_217_391,
                611_413_043,
            ),
            (
                "600610",
                StockExchange::Shanghai,
                0.10,
                755,
                1_059_602_649,
                847_682_119,
            ),
            (
                "000812",
                StockExchange::Shenzhen,
                0.10,
                285,
                1_052_631_579,
                842_105_263,
            ),
        ];
        for (spec, (code, exchange, limit, price, total, float)) in setup.stocks.iter().zip(pinned)
        {
            assert_eq!(spec.code.0, code);
            assert_eq!(spec.exchange, exchange);
            assert_eq!(spec.limit_pct, limit);
            assert_eq!(spec.initial_price, Money::from_cents(price));
            assert_eq!(spec.v_initial, Money::from_cents(price));
            assert_eq!(spec.tick, Money::from_cents(1));
            assert_eq!(spec.total_shares, total);
            assert_eq!(spec.float_shares, float);
        }
        assert_eq!(setup.npcs.retail_cash_median, Money::from_cents(20_000_000));
        assert_eq!(setup.config.starting_cash, Money::from_cents(1_000_000_000));
        assert_eq!(setup.config.commission_rate, 0.00025);
        assert_eq!(setup.config.stamp_tax_rate, 0.0005);
        assert_eq!(
            (setup.v_params.mean_reversion, setup.v_params.volatility),
            (0.5, 0.02)
        );
        assert_eq!(setup.v_params.long_run_mean, Money::from_cents(1120));
        assert_eq!(setup.strategy_params.retail.arrival_rate, 0.3);
        assert_eq!(setup.strategy_params.retail.order_size_mean, 300);
        assert_eq!(setup.strategy_params.retail.chase_prob, 0.4);
        assert_eq!(setup.strategy_params.retail.tick_cents, 1);
        assert_eq!(setup.strategy_params.inst.margin, 0.02);
        assert_eq!(setup.strategy_params.inst.order_size, 200_000);
        assert_eq!(setup.strategy_params.hot.lookback, 20);
        assert_eq!(setup.strategy_params.hot.trend_threshold, 0.03);
        assert_eq!(setup.strategy_params.hot.order_size, 100_000);
        assert_eq!(
            (
                setup.ticks_per_day,
                setup.auction_ticks,
                setup.closing_auction_ticks
            ),
            (15_300, 900, 180)
        );
        assert_eq!(setup.history_len, 20);
        assert!(setup.t1_enabled);
        match setup.float_allocation {
            engine::FloatAllocation::ByKind { retail, inst, hot } => {
                assert_eq!((retail, inst, hot), (0.45, 0.53, 0.02));
            }
            engine::FloatAllocation::Random => panic!("matrix 必须使用 ByKind 浮筹分配"),
        }
    }

    /// compressed 是 packages/engine/tests/session.rs large_retail_account_setup(20_000)
    /// 的逐字段副本；期望值按该真源硬编码。
    #[test]
    fn compressed_pins_every_large_retail_setup_field() {
        let setup = compressed_setup();
        for spec in setup.stocks.iter() {
            assert_eq!(spec.initial_price, Money::from_cents(1_000));
            assert_eq!(spec.v_initial, Money::from_cents(1_000));
            assert_eq!(spec.tick, Money::from_cents(1));
            assert_eq!(spec.total_shares, 100_000_000);
            assert_eq!(spec.float_shares, 80_000_000);
        }
        assert_eq!(setup.config.starting_cash, Money::from_cents(10_000_000));
        assert_eq!(setup.config.commission_min, Money::from_cents(500));
        assert_eq!(setup.config.commission_rate, 0.00025);
        assert_eq!(setup.config.stamp_tax_rate, 0.0005);
        assert_eq!(setup.config.lot_size, 100);
        assert_eq!(
            (setup.v_params.mean_reversion, setup.v_params.volatility),
            (0.5, 0.0)
        );
        assert_eq!(setup.strategy_params.retail.arrival_rate, 0.5);
        assert_eq!(setup.strategy_params.retail.order_size_mean, 100);
        assert_eq!(setup.strategy_params.retail.chase_prob, 0.2);
        assert_eq!(setup.strategy_params.inst.margin, 0.05);
        assert_eq!(setup.strategy_params.inst.order_size, 200);
        assert_eq!(setup.strategy_params.hot.lookback, 3);
        assert_eq!(setup.strategy_params.hot.trend_threshold, 0.02);
        assert_eq!(setup.strategy_params.hot.order_size, 200);
        assert_eq!(
            (
                setup.ticks_per_day,
                setup.auction_ticks,
                setup.closing_auction_ticks
            ),
            (300, 0, 0)
        );
        assert_eq!(setup.history_len, 5);
        assert!(setup.t1_enabled);
    }

    #[test]
    fn scenarios_keep_full_day_and_compressed_clocks_apart() {
        let matrix = matrix_setup();
        let compressed = compressed_setup();
        assert_eq!(
            (
                matrix.ticks_per_day,
                matrix.auction_ticks,
                matrix.closing_auction_ticks
            ),
            (15_300, 900, 180)
        );
        assert_eq!(
            (
                compressed.ticks_per_day,
                compressed.auction_ticks,
                compressed.closing_auction_ticks
            ),
            (300, 0, 0)
        );
        // matrix 股本副本取自 web DEFAULT_SETUP；漂移即失败，防止悄悄换输入。
        assert_eq!(matrix.stocks[0].total_shares, 8_928_571_429);
        assert_eq!(compressed.stocks[0].total_shares, 100_000_000);
    }

    #[test]
    fn unknown_scenario_is_rejected_explicitly() {
        assert!(scenario_setup("nope").is_err());
        assert!(scenario_setup("matrix").is_ok());
        assert!(scenario_setup("compressed-300").is_ok());
    }
}
