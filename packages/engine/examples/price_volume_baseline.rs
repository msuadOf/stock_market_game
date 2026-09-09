use std::{env, fs, path::Path, process};

use engine::{run_price_volume_baseline, SaveSlot};

const USAGE: &str = "用法：\n  cargo run -p engine --release --example price_volume_baseline -- <存档.json> <交易日数> <seed[,seed...]>\n\n示例：\n  cargo run -p engine --release --example price_volume_baseline -- stock-game-save.json 30 1,2,3,4,5";

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
    let slot: SaveSlot = serde_json::from_str(&json)
        .map_err(|error| format!("存档 `{}` 结构无效：{error}", save_path.display()))?;
    let report = run_price_volume_baseline(&slot.setup, &seeds, trading_days)
        .map_err(|error| error.to_string())?;
    let output = serde_json::to_string_pretty(&report)
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

#[cfg(test)]
mod tests {
    use super::parse_seeds;

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
