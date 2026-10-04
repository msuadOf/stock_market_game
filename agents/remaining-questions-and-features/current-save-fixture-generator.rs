use engine::{GameSession, SessionSetup};
use serde_json::Value;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let input_path = arguments.next().ok_or("缺少旧 fixture 路径")?;
    let output_path = arguments.next().ok_or("缺少临时输出路径")?;
    if arguments.next().is_some() {
        return Err("参数过多".into());
    }

    let old_fixture: Value = serde_json::from_str(&fs::read_to_string(input_path)?)?;
    let mut setup_value = old_fixture
        .get("setup")
        .ok_or("旧 fixture 缺少 setup")?
        .clone();
    let seed_text = old_fixture
        .get("seed")
        .and_then(Value::as_str)
        .ok_or("旧 fixture 缺少十进制字符串 seed")?;
    let seed = seed_text.parse::<u64>()?;

    let setup_object = setup_value
        .as_object_mut()
        .ok_or("旧 fixture 的 setup 不是对象")?;
    setup_object.insert("company_operations".to_owned(), Value::Null);
    setup_object.insert("groups".to_owned(), Value::Array(Vec::new()));
    let setup: SessionSetup = serde_json::from_value(setup_value)?;

    if seed != 666_959_854
        || setup.stocks.len() != 5
        || setup.npcs.retail_count + setup.npcs.inst_count + setup.npcs.hot_count != 26
        || setup.ticks_per_day != 60
    {
        return Err("旧 fixture 的 setup/seed 不符合要求".into());
    }

    let ticks_per_day = setup.ticks_per_day;
    let mut session = GameSession::new(setup, seed)?;
    for _ in 0..2 {
        for _ in 0..ticks_per_day {
            session.step()?;
        }
        session.end_civil_day()?;
    }

    let save = session.save()?;
    if save.snapshot.tick != 120 || save.civil_clock.settled_through.is_none() {
        return Err("生成结果未完成两个交易日的日终结算".into());
    }
    let encoded = serde_json::to_value(&save)?;
    let history_reads = encoded
        .get("history_reads")
        .ok_or("Engine 保存结果缺少 history_reads")?;
    let history_accounts = history_reads
        .as_object()
        .ok_or("Engine 保存结果的 history_reads 不是对象")?;
    let snapshot_accounts = encoded
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("accounts"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 snapshot.accounts")?;
    let history_account_keys = history_accounts
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let snapshot_account_keys = snapshot_accounts
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if history_account_keys != snapshot_account_keys {
        return Err("history_reads account keys 与 snapshot.accounts 不一致".into());
    }

    let price_memories = encoded
        .get("price_memories")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 price_memories")?;
    for (account_id, memory) in price_memories {
        let stocks = memory
            .get("stocks")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("price_memories.{account_id}.stocks 不是对象"))?;
        for (code, stock_memory) in stocks {
            for old_field in [
                "public_history_read_count",
                "last_public_history_read_minute",
            ] {
                if stock_memory.get(old_field).is_some() {
                    return Err(format!(
                        "price_memories.{account_id}.stocks.{code} 含已删除字段 {old_field}"
                    )
                    .into());
                }
            }
        }
    }

    let restored = GameSession::restore(&save)?;
    if serde_json::to_value(restored.save()?)? != encoded {
        return Err("GameSession::restore 后重新保存与原保存不一致".into());
    }
    fs::write(output_path, serde_json::to_string(&save)? + "\n")?;
    Ok(())
}
