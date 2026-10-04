use std::{env, fs, path::Path, process};

use engine::{run_price_volume_baseline, SessionSetup};

const USAGE: &str = "用法：\n  cargo run -p engine --release --features simulation-diagnostics --example price_volume_baseline -- <存档.json> <交易日数> <seed[,seed...]>\n\n示例：\n  cargo run -p engine --release --features simulation-diagnostics --example price_volume_baseline -- stock-game-save.json 30 1,2,3,4,5";

fn main() {
    if let Err(error) = run() {
        eprintln!("量价基线生成失败：{error}\n\n{USAGE}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.len() != 3 {
        return Err(format!("需要 3 个参数，实际收到 {} 个", args.len()));
    }

    let save_path = Path::new(&args[0]);
    let trading_days = args[1]
        .parse::<u32>()
        .map_err(|error| format!("交易日数 `{}` 不是 u32 正整数：{error}", args[1]))?;
    let seeds = parse_seeds(&args[2])?;
    let json = fs::read_to_string(save_path)
        .map_err(|error| format!("无法读取存档 `{}`：{error}", save_path.display()))?;
    let setup = parse_setup(&json).map_err(|error| {
        format!(
            "存档 `{}` 的 SessionSetup 无效：{error}",
            save_path.display()
        )
    })?;
    let report = run_price_volume_baseline(&setup, &seeds, trading_days)
        .map_err(|error| error.to_string())?;
    let mut causal_runs = Vec::new();
    for seed in seeds {
        let mut session =
            engine::GameSession::new(setup.clone(), seed).map_err(|error| error.to_string())?;
        let ticks = setup
            .ticks_per_day
            .checked_mul(u64::from(trading_days))
            .ok_or("tick count overflow")?;
        for _ in 0..ticks {
            session
                .step()
                .map_err(|error| format!("diagnostic market step failed: {error}"))?;
        }
        causal_runs.push(
            session
                .causal_diagnostics()
                .map_err(|error| error.to_string())?,
        );
    }
    let output = serde_json::to_string_pretty(
        &serde_json::json!({ "price_volume": report, "causal_runs": causal_runs }),
    )
    .map_err(|error| format!("报告 JSON 序列化失败：{error}"))?;
    println!("{output}");
    Ok(())
}

fn parse_seeds(value: &str) -> Result<Vec<u64>, String> {
    if value.is_empty() {
        return Err("seed 列表不能为空".to_string());
    }
    value
        .split(',')
        .enumerate()
        .map(|(index, raw)| {
            if raw.is_empty() {
                return Err(format!("第 {} 个 seed 为空", index + 1));
            }
            raw.parse::<u64>()
                .map_err(|error| format!("第 {} 个 seed `{raw}` 无效：{error}", index + 1))
        })
        .collect()
}

fn parse_setup(json: &str) -> Result<SessionSetup, String> {
    #[derive(serde::Deserialize)]
    struct SetupProjection {
        setup: SessionSetup,
    }
    let projection: SetupProjection =
        serde_json::from_str(json).map_err(|error| format!("setup 投影失败：{error}"))?;
    projection
        .setup
        .validate()
        .map_err(|error| error.to_string())?;
    Ok(projection.setup)
}

#[cfg(test)]
mod tests {
    use super::{parse_seeds, parse_setup};

    fn setup_json() -> serde_json::Value {
        serde_json::json!({
            "stocks": [{
                "code": "600101", "exchange": "Shanghai", "category": "MainBoard",
                "initial_price": "1000", "limit_pct": 0.1, "tick": "1",
                "total_shares": "100000", "float_shares": 100000
            }],
            "npcs": {"retail_count": 4, "inst_count": 2, "hot_count": 2, "retail_cash_median": "10000000"},
            "config": engine::GameConfig::proposed_defaults(),
            "strategy_params": {
                "retail": {"arrival_rate": 0.8, "order_size_mean": 200, "chase_prob": 0.4},
                "inst": {"margin": 0.02, "order_size": 500},
                "hot": {"lookback": 3, "trend_threshold": 0.01, "order_size": 300}
            },
            "ticks_per_day": 30, "auction_ticks": 0, "closing_auction_ticks": 0,
            "history_len": 20, "t1_enabled": true, "float_allocation": "Random",
            "start_date": "2030-01-01", "simulation_policy_id": engine::SIMULATION_POLICY_ID
        })
    }

    #[test]
    fn projects_setup_without_deserializing_unrelated_save_state() {
        let setup = setup_json();
        let expected: engine::SessionSetup = serde_json::from_value(setup.clone()).unwrap();
        expected.validate().unwrap();
        let expected_json = serde_json::to_value(expected).unwrap();
        for input in [
            serde_json::json!({"setup": setup}),
            serde_json::json!({
                "setup": setup, "seed": [], "schema_version": "broken",
                "runtime_state": false, "snapshot": "broken", "orders": false, "accounts": 4
            }),
        ] {
            let projected = parse_setup(&input.to_string()).unwrap();
            assert_eq!(serde_json::to_value(projected).unwrap(), expected_json);
        }
    }

    #[test]
    fn rejects_invalid_json_missing_setup_and_invalid_setup() {
        for input in ["{", "[]", "{}", "{\"setup\":null}", "{\"setup\":{}}"] {
            assert!(parse_setup(input).is_err(), "{input}");
        }
        let mut setup = setup_json();
        setup["t1_enabled"] = serde_json::json!(false);
        assert!(parse_setup(&serde_json::json!({"setup": setup}).to_string()).is_err());
        for (field, value) in [
            ("simulation_policy_id", serde_json::json!("obsolete-policy")),
            ("start_date", serde_json::json!("2100-01-01")),
            ("company_operations", serde_json::json!("invalid-config")),
            ("groups", serde_json::json!(false)),
        ] {
            let mut setup = setup_json();
            setup[field] = value;
            assert!(
                parse_setup(&serde_json::json!({"setup": setup}).to_string()).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_setup_and_reports_setup_field_failures() {
        let setup = setup_json();
        let duplicate = format!("{{\"setup\":{setup},\"setup\":{setup}}}");
        assert!(parse_setup(&duplicate)
            .unwrap_err()
            .contains("duplicate field"));
        let mut invalid = setup;
        invalid["ticks_per_day"] = serde_json::json!("not-a-number");
        assert!(parse_setup(&serde_json::json!({"setup": invalid}).to_string()).is_err());
    }

    #[test]
    fn parses_seed_list_without_losing_u64_precision() {
        assert_eq!(
            parse_seeds("1,42,18446744073709551615").unwrap(),
            vec![1, 42, u64::MAX]
        );
    }

    #[test]
    fn rejects_empty_or_malformed_seed_entries() {
        assert!(parse_seeds("").is_err());
        assert!(parse_seeds("1,,2").is_err());
        assert!(parse_seeds("not-a-seed").is_err());
    }
}
