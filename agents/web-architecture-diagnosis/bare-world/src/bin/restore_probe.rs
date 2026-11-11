//! 比较现行格式旧算法存档在旧算法和候选算法下的恢复与推进。
use engine::{GameSession, SaveDecodeLimits, SaveSlot, SessionSetup};
use std::{collections::BTreeSet, fs, io::Write};
#[path = "../../../../libos-integration/world-benchmark/event_projection.rs"]
mod event_projection;
#[path = "../../../../libos-integration/world-benchmark/fact_projection.rs"]
mod fact_projection;
#[cfg(test)]
#[path = "../../../../libos-integration/world-benchmark/event_projection_tests.rs"]
mod event_projection_tests;

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
fn candidate(argument: f64) -> f64 { -libm::expm1(argument) }
fn shutdown(game: GameSession) -> ProbeResult<()> {
    game.shutdown().map_err(|error| format!("会话关闭失败：{error:?}"))?;
    Ok(())
}

struct Outcome {
    trace: Vec<engine::strategy::ObservationTrace>,
    before: Vec<u8>, before_hash: String, start_tick: u64,
    events: Vec<Vec<u8>>, after: Vec<u8>, after_hash: String, end_tick: u64,
    facts: Vec<engine::runtime::world::TickRecord>,
    account_states: Vec<serde_json::Value>,
    stock_states: Vec<serde_json::Value>,
}

fn run(saved: &SaveSlot) -> ProbeResult<Outcome> {
    let (game, trace) = engine::strategy::verification_trace_observations(|| GameSession::restore(saved));
    let mut game = game?;
    let start_tick = game.tick();
    let before = serde_json::to_vec(&game.save()?)?;
    let before_hash = serde_json::to_string(&game.business_state_hash()?)?;
    let mut events = Vec::with_capacity(3000);
    let mut facts = Vec::with_capacity(3000);
    let mut account_states = Vec::with_capacity(3001);
    let mut stock_states = Vec::with_capacity(3001);
    let mut previous_accounts = std::collections::BTreeMap::<String, engine::session::AccountSnap>::new();
    capture_business_states(&game, &mut account_states, &mut stock_states, &mut previous_accounts, true, "initial")?;
    for _ in 0..3000 {
        events.push(serde_json::to_vec(&game.step()?)?);
        facts.push(game.verification_latest_runtime_record()?);
        capture_business_states(&game, &mut account_states, &mut stock_states, &mut previous_accounts, false, "change")?;
    }
    capture_business_states(&game, &mut account_states, &mut stock_states, &mut previous_accounts, true, "final")?;
    let after = serde_json::to_vec(&game.save()?)?;
    let after_hash = serde_json::to_string(&game.business_state_hash()?)?;
    let end_tick = game.tick();
    shutdown(game)?;
    Ok(Outcome {trace, before, before_hash, start_tick, events, facts, account_states, stock_states, after, after_hash, end_tick})
}

fn capture_business_states(
    game: &GameSession,
    accounts: &mut Vec<serde_json::Value>,
    stocks: &mut Vec<serde_json::Value>,
    previous_accounts: &mut std::collections::BTreeMap<String, engine::session::AccountSnap>,
    include_all: bool,
    stage: &str,
) -> ProbeResult<()> {
    let snapshot = game.verification_snapshot_including_npc_accounts();
    let tick = snapshot.tick;
    for (account, state) in snapshot.accounts {
        let account = format!("{account:?}");
        let changed = previous_accounts.get(&account) != Some(&state);
        if include_all || changed {
            accounts.push(serde_json::json!({"account":account,"tick":tick,"stage":stage,"state":serde_json::to_value(&state)?}));
        }
        previous_accounts.insert(account, state);
    }
    for (stock, state) in snapshot.markets {
        stocks.push(serde_json::json!({
            "code": format!("{stock:?}"), "tick": tick,"stage":stage,
            "state": serde_json::to_value(state)?,
        }));
    }
    Ok(())
}

fn business_facts(outcome: &Outcome) -> ProbeResult<serde_json::Value> {
    let mut trades = Vec::new();
    let mut rejections = Vec::new();
    let mut cancellations = Vec::new();
    for record in &outcome.facts {
        for fact in &record.desk_trades {
            let detail = format!("{fact:?}");
            for account in [fact.maker, fact.taker] {
                trades.push(serde_json::json!({"code":fact.stock,"account":format!("{account:?}"),"tick":record.tick,"detail":detail}));
            }
        }
        for fact in record.desk_receipts.iter().chain(&record.group_rejections) {
            rejections.push(serde_json::json!({"code":fact.order.0,"account":format!("{:?}",fact.owner),"tick":record.tick,"detail":format!("{fact:?}")}));
        }
        for fact in &record.desk_lifecycle {
            if format!("{:?}", fact.kind) == "Cancelled" {
                cancellations.push(serde_json::json!({"code":fact.stock,"account":format!("{:?}",fact.owner),"tick":record.tick,"detail":format!("{fact:?}")}));
            }
        }
    }
    Ok(serde_json::json!({
        "trades":trades, "rejections":rejections, "cancellations":cancellations,
        "accountStates":outcome.account_states, "marketStates":outcome.stock_states,
    }))
}

fn compare(left: &Outcome, right: &Outcome) -> ProbeResult<serde_json::Value> {
    if left.events.len() != right.events.len() { return Err("事件步数不一致".into()); }
    if left.facts.len() != left.events.len() || right.facts.len() != right.events.len() { return Err("事实步数不一致".into()); }
    let mut fact_first = None;
    let mut counts = [0usize; 6];
    for (index, (a, b)) in left.facts.iter().zip(&right.facts).enumerate() {
        if a.tick != left.start_tick + index as u64 + 1 || a.tick != b.tick { return Err("事实 tick 不连续或不一致".into()); }
        let canceled_a: Vec<_> = a.desk_lifecycle.iter().filter(|fact| format!("{:?}", fact.kind) == "Cancelled").collect();
        let canceled_b: Vec<_> = b.desk_lifecycle.iter().filter(|fact| format!("{:?}", fact.kind) == "Cancelled").collect();
        let comparisons = [
            ("desk_trades", a.desk_trades.len(), fact_projection::same_scoped(&a.desk_trades, &b.desk_trades, |x| vec![x.stock]) && fact_projection::same_scoped(&a.desk_trades, &b.desk_trades, |x| vec![x.maker, x.taker])),
            ("desk_lifecycle", a.desk_lifecycle.len(), fact_projection::same_scoped(&a.desk_lifecycle, &b.desk_lifecycle, |x| vec![x.stock]) && fact_projection::same_scoped(&a.desk_lifecycle, &b.desk_lifecycle, |x| vec![x.owner])),
            ("desk_receipts", a.desk_receipts.len(), fact_projection::same_scoped(&a.desk_receipts, &b.desk_receipts, |x| vec![x.order.0]) && fact_projection::same_scoped(&a.desk_receipts, &b.desk_receipts, |x| vec![x.owner])),
            ("group_rejections", a.group_rejections.len(), fact_projection::same_scoped(&a.group_rejections, &b.group_rejections, |x| vec![x.order.0]) && fact_projection::same_scoped(&a.group_rejections, &b.group_rejections, |x| vec![x.owner])),
            ("desk_auction", a.desk_auction.len(), fact_projection::same_scoped(&a.desk_auction, &b.desk_auction, |x| vec![x.stock])),
            ("cancellations", canceled_a.len(), fact_projection::same_scoped(&canceled_a, &canceled_b, |x| vec![x.stock]) && fact_projection::same_scoped(&canceled_a, &canceled_b, |x| vec![x.owner])),
        ];
        for (kind, (name, count, equal)) in comparisons.into_iter().enumerate() {
            counts[kind] += count;
            if !equal && fact_first.is_none() { fact_first = Some(serde_json::json!({"tick":a.tick,"kind":name,"baseline":format!("{a:?}"),"other":format!("{b:?}")})); }
        }
    }
    let first = left.events.iter().zip(&right.events).position(|(a,b)| a != b);
    let (mut left_seq, mut right_seq, mut local_first) = (None, None, None);
    for (index, (a, b)) in left.events.iter().zip(&right.events).enumerate() {
        let left_events: Vec<engine::Event> = serde_json::from_slice(a)?;
        let right_events: Vec<engine::Event> = serde_json::from_slice(b)?;
        event_projection::check_sequence(&left_events, &mut left_seq)?;
        event_projection::check_sequence(&right_events, &mut right_seq)?;
        if !event_projection::same_local_order(&left_events, &right_events)? && local_first.is_none() { local_first = Some(index); }
    }
    let local_detail = match local_first {
        Some(index) => Some(serde_json::json!({"offset":index,"baselineUtf8":std::str::from_utf8(&left.events[index])?,"otherUtf8":std::str::from_utf8(&right.events[index])?})),
        None => None,
    };
    let detail = match first {
        Some(index) => Some(serde_json::json!({"offset":index,"baselineUtf8":std::str::from_utf8(&left.events[index])?,"otherUtf8":std::str::from_utf8(&right.events[index])?})),
        None => None,
    };
    let left_business = business_facts(left)?;
    let right_business = business_facts(right)?;
    let business_comparison = fact_projection::same_scoped(&left.account_states, &right.account_states, |x| vec![x["account"].to_string()])
        && fact_projection::same_scoped(&left.stock_states, &right.stock_states, |x| vec![x["code"].to_string()]);
    let baseline_business = serde_json::json!({"trades":left_business["trades"],"rejections":left_business["rejections"],"cancellations":left_business["cancellations"],"accountStates":left_business["accountStates"],"marketStates":left_business["marketStates"]});
    let replay_business = serde_json::json!({"trades":right_business["trades"],"rejections":right_business["rejections"],"cancellations":right_business["cancellations"],"accountStates":right_business["accountStates"],"marketStates":right_business["marketStates"]});
    let left_trades = left_business["trades"].as_array().ok_or("成交事实不是数组")?;
    let right_trades = right_business["trades"].as_array().ok_or("回放成交事实不是数组")?;
    let left_rejections = left_business["rejections"].as_array().ok_or("拒单事实不是数组")?;
    let right_rejections = right_business["rejections"].as_array().ok_or("回放拒单事实不是数组")?;
    let left_cancellations = left_business["cancellations"].as_array().ok_or("撤单事实不是数组")?;
    let right_cancellations = right_business["cancellations"].as_array().ok_or("回放撤单事实不是数组")?;
    let business_equal = fact_projection::same_scoped(left_trades, right_trades, |x| vec![x["code"].to_string()])
        && fact_projection::same_scoped(left_trades, right_trades, |x| vec![x["account"].to_string()])
        && fact_projection::same_scoped(left_rejections, right_rejections, |x| vec![x["code"].to_string()])
        && fact_projection::same_scoped(left_rejections, right_rejections, |x| vec![x["account"].to_string()])
        && fact_projection::same_scoped(left_cancellations, right_cancellations, |x| vec![x["code"].to_string()])
        && fact_projection::same_scoped(left_cancellations, right_cancellations, |x| vec![x["account"].to_string()])
        && business_comparison;
    Ok(serde_json::json!({"startTick":right.start_tick,"endTick":right.end_tick,
        "restoredSaveEqual":left.before == right.before,"restoredHashEqual":left.before_hash == right.before_hash,
        "finalSaveEqual":left.after == right.after,"finalHashEqual":left.after_hash == right.after_hash,
        "firstEventDifference":detail,"firstLocalEventDifference":local_detail,"eventSteps":right.events.len(),
        "firstScopedFactDifference":fact_first,"factSteps":right.facts.len(),
        "firstBusinessFactDifference":if business_equal {None::<serde_json::Value>} else {Some(serde_json::json!({"kind":"businessFacts"}))},
        "businessFacts":{"baseline":baseline_business,"replay":replay_business},
        "factCounts":{"trades":counts[0],"lifecycle":counts[1],"deskReceipts":counts[2],"groupRejections":counts[3],"auction":counts[4],"cancellations":counts[5]}}))
}

fn main() -> ProbeResult<()> {
    if !engine::runtime::step_runtime_enabled() { return Err("runtime 未启用".into()); }
    let mut args = std::env::args().skip(1);
    let setup_path = args.next().ok_or("缺少 setup 路径")?;
    let save_path = args.next().ok_or("缺少存档路径")?;
    let order = args.next().unwrap_or_else(|| "old-first".to_owned());
    if order != "old-first" && order != "candidate-first" { return Err("执行顺序无效".into()); }
    if args.next().is_some() { return Err("参数过多".into()); }
    let setup: SessionSetup = serde_json::from_slice(&fs::read(setup_path)?)?;
    let expected_count = u64::from(setup.npcs.retail_count) + u64::from(setup.npcs.inst_count) + u64::from(setup.npcs.hot_count);
    if expected_count != 20007 || setup.issuer_repurchase_enabled { return Err("夹具 NPC 数量或专户配置不符".into()); }
    let mut original = GameSession::new(setup, 42)?;
    let roster = original.account_roster();
    if roster.len() != expected_count as usize + 1 || roster.iter().enumerate().any(|(index,id)| id.0 != index as u64) { return Err("夹具账户名册不符".into()); }
    while original.day() == original.civil_clock().completed_trading_sessions_expected()? { original.end_civil_day()?; }
    for _ in 0..1000 { original.step()?; }
    let save_tick = original.tick();
    let save_bytes = serde_json::to_vec(&original.save()?)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&save_path)?;
    file.write_all(&save_bytes)?; file.sync_all()?;
    shutdown(original)?;
    let disk_bytes = fs::read(&save_path)?;
    if disk_bytes != save_bytes { return Err("写入的存档字节不一致".into()); }
    let saved = engine::decode_save_slot(&disk_bytes, &SaveDecodeLimits::default())?;
    let (baseline, control, candidate_outcome) = if order == "old-first" {
        eprintln!("阶段：旧算法基准 A");
        let baseline = run(&saved).map_err(|error| format!("旧算法基准 A 失败：{error}"))?;
        eprintln!("阶段：旧算法重复对照 B");
        let control = run(&saved).map_err(|error| format!("旧算法对照 B 失败：{error}"))?;
        eprintln!("旧算法对照证据：{}", compare(&baseline, &control)?);
        eprintln!("阶段：候选算法 C");
        let candidate_outcome = engine::strategy::verification_with_observation_math(candidate, || run(&saved))
            .map_err(|error| format!("候选算法 C 失败：{error}"))?;
        (baseline, control, candidate_outcome)
    } else {
        eprintln!("阶段：候选算法 C");
        let candidate_outcome = engine::strategy::verification_with_observation_math(candidate, || run(&saved))
            .map_err(|error| format!("候选算法 C 失败：{error}"))?;
        eprintln!("阶段：旧算法基准 A");
        let baseline = run(&saved).map_err(|error| format!("旧算法基准 A 失败：{error}"))?;
        eprintln!("阶段：旧算法重复对照 B");
        let control = run(&saved).map_err(|error| format!("旧算法对照 B 失败：{error}"))?;
        eprintln!("旧算法对照证据：{}", compare(&baseline, &control)?);
        (baseline, control, candidate_outcome)
    };
    let mut changed = 0usize;
    let mut accounts = BTreeSet::new();
    for (index, (before, after)) in baseline.trace.iter().zip(&candidate_outcome.trace).enumerate() {
        if before.account_id != Some(index as u64 + 1) || before.account_id != after.account_id || !accounts.insert(before.account_id) || before.observations_bits != after.observations_bits || before.ticks_per_day != after.ticks_per_day || before.argument_bits != after.argument_bits || before.canonical_bits.is_none() || after.canonical_bits.is_none() || candidate(f64::from_bits(after.argument_bits)).to_bits() != after.probability_bits { return Err("概率追踪身份、输入、候选输出或规范化结果不完整".into()); }
        changed += usize::from(before.probability_bits != after.probability_bits);
    }
    if baseline.trace.len() != expected_count as usize || control.trace.len() != expected_count as usize || candidate_outcome.trace.len() != expected_count as usize { return Err("概率追踪未覆盖全部 NPC".into()); }
    let control_trace_equal = baseline.trace == control.trace;
    if changed == 0 { return Err("候选未改变任何重建概率，夹具无法覆盖差异".into()); }
    println!("{}", serde_json::json!({"seed":42,"saveTick":save_tick,"executionOrder":order,"npcCount":baseline.trace.len(),
        "recomputedProbabilityDifferences":changed,"controlTraceEqual":control_trace_equal,
        "baselineStartTick":baseline.start_tick,"baselineEndTick":baseline.end_tick,"baselineRestoredEqualsInput":baseline.before == disk_bytes,"saveBytes":disk_bytes.len(),
        "control":compare(&baseline,&control)?,"candidate":compare(&baseline,&candidate_outcome)?}));
    Ok(())
}
