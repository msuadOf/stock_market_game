//! 同一生产 ProtocolSession 按实际自然日日结精确停止的 RuntimeSyncPolicy 探针。
//! 纯自动 flush 没有公开独立计时读面；只报告真实边界合计 wall。

use engine::session::protocol::{ProtocolSession, SaveCandidateKey};
use engine::{RuntimeSyncPolicy, SessionSetup};
use serde_json::{json, Value};
use std::{env, fs, time::Instant};

#[derive(Debug)]
struct StopPoint {
    ticks_per_day: u64,
    target_days: u64,
    completed_days: u64,
    tick: u64,
    target_tick: u64,
}

impl StopPoint {
    fn new(ticks_per_day: u64, target_days: u64) -> Result<Self, String> {
        if ticks_per_day == 0 || target_days == 0 {
            return Err("日长与交易日数必须为正".to_owned());
        }
        let target_tick = ticks_per_day
            .checked_mul(target_days)
            .and_then(|tick| tick.checked_add(1))
            .ok_or_else(|| "精确停止 tick 溢出".to_owned())?;
        Ok(Self {
            ticks_per_day,
            target_days,
            completed_days: 0,
            tick: 0,
            target_tick,
        })
    }

    fn accept_tick(&mut self, tick: u64) -> Result<(), String> {
        let next = self
            .tick
            .checked_add(1)
            .ok_or_else(|| "tick 游标溢出".to_owned())?;
        if tick != next || tick > self.target_tick {
            return Err(format!(
                "市场 tick 不连续或超过停止点：当前{}，收到{tick}，终点{}",
                self.tick, self.target_tick
            ));
        }
        if (tick - 1) / self.ticks_per_day > self.completed_days {
            return Err("尚未确认真实交易日日结，不能进入下一日市场拍".to_owned());
        }
        self.tick = tick;
        Ok(())
    }

    fn accept_trading_day(&mut self, tick: u64) -> Result<(), String> {
        let days = self
            .completed_days
            .checked_add(1)
            .ok_or_else(|| "交易日计数溢出".to_owned())?;
        let expected = days
            .checked_mul(self.ticks_per_day)
            .ok_or_else(|| "日终 tick 溢出".to_owned())?;
        if days > self.target_days || tick != expected || self.tick != expected {
            return Err(format!(
                "真实交易日日结位置不符：收到{tick}，预期{expected}，已完成{}日",
                self.completed_days
            ));
        }
        self.completed_days = days;
        Ok(())
    }

    fn complete(&self) -> bool {
        self.completed_days == self.target_days && self.tick == self.target_tick
    }
}

fn add_ns(total: &mut u128, value: u128) -> Result<(), String> {
    *total = total
        .checked_add(value)
        .ok_or_else(|| "wall 计数溢出".to_owned())?;
    Ok(())
}

fn record_sequence(previous: &mut u64, from: u64, to: u64) -> Result<(), String> {
    if from != *previous || to < from {
        return Err(format!(
            "市场/civil 事实 seq 不连续：原{}，收到({from},{to}]",
            *previous
        ));
    }
    *previous = to;
    Ok(())
}

fn measure(
    protocol: &mut ProtocolSession,
    setup: &SessionSetup,
    input_setup: Value,
    policy: &str,
    seed: u64,
    days: u64,
    initialization_ns: u128,
) -> Result<Value, String> {
    let mut stop = StopPoint::new(setup.ticks_per_day, days)?;
    let (groups, desks) = setup
        .default_new_runtime_topology()
        .map_err(|error| error.to_string())?;
    let mut sequence = protocol.game().seq();
    let mut events = 0_u64;
    let mut civil_updates = 0_u64;
    let mut closed_civil_days = 0_u64;
    let mut intraday_ns = 0_u128;
    let mut closed_civil_ns = 0_u128;
    let mut terminal_step_ns = 0_u128;
    let mut boundaries = Vec::new();
    let mut boundary_wall = Vec::new();
    let started = Instant::now();
    loop {
        // 使用生产共享日历守卫，休市自然日逐日结算，不补造市场 tick。
        while protocol
            .civil_day_ready()
            .map_err(|error| error.to_string())?
        {
            let boundary_started = Instant::now();
            let update = protocol
                .end_civil_day_update()
                .map_err(|error| error.to_string())?;
            update.validate().map_err(|error| error.to_string())?;
            record_sequence(&mut sequence, update.seq_from, update.seq_to)?;
            let key = SaveCandidateKey {
                settled_date: update.boundary.settled_date,
                seq: update.seq_to,
            };
            let save = protocol.save_candidate(&key).map_err(|error| error.to_string())?;
            if save.snapshot.tick != update.tick
                || save.snapshot.seq != update.seq_to
                || save.civil_clock.settled_through != Some(update.boundary.settled_date)
            {
                return Err("真实日终候选的 tick/seq/日期与 civil publication 不同版".to_owned());
            }
            let origin = protocol.game().runtime_mirror_origin();
            origin.validate()?;
            let trading =
                update.boundary.settled_phase == engine::session::CivilPhase::IntradayTrading;
            if trading && (origin.source_tick != update.tick || origin.install_sequence == 0) {
                return Err("日终候选缺少当前市场截点的实际完整镜像安装".to_owned());
            }
            civil_updates = civil_updates.checked_add(1).ok_or("civil 计数溢出")?;
            events = events
                .checked_add(
                    u64::try_from(update.events().count()).map_err(|_| "civil 事件数溢出")?,
                )
                .ok_or("事件计数溢出")?;
            let settled_date = update.boundary.settled_date;
            let next_date = update.boundary.next_date;
            let tick = update.tick;
            let save_seq = save.snapshot.seq;
            let save_tick = save.snapshot.tick;
            // publication/候选读取和销毁都属于边界合计；不称为纯 flush。
            drop(save);
            drop(update);
            let elapsed = boundary_started.elapsed().as_nanos();
            if trading {
                stop.accept_trading_day(tick)?;
                let combined = terminal_step_ns
                    .checked_add(elapsed)
                    .ok_or("边界 wall 溢出")?;
                terminal_step_ns = 0;
                boundary_wall.push(combined.to_string());
                boundaries.push(json!({
                    "settled_date": settled_date.to_iso(), "next_date": next_date.to_iso(),
                    "tick": tick, "combined_boundary_wall_ns": combined.to_string(),
                    "save_candidate_tick": save_tick, "save_candidate_seq": save_seq,
                    "mirror_origin": origin,
                }));
            } else {
                closed_civil_days = closed_civil_days.checked_add(1).ok_or("休市日计数溢出")?;
                add_ns(&mut closed_civil_ns, elapsed)?;
            }
        }
        if protocol.game().civil_clock().phase() != engine::session::CivilPhase::IntradayTrading {
            return Err("共享 Calendar 尚未进入交易日却准备推进市场 tick".to_owned());
        }
        let tick_started = Instant::now();
        let frame = protocol.step_frame().map_err(|error| error.to_string())?;
        frame.validate().map_err(|error| error.to_string())?;
        stop.accept_tick(frame.tick)?;
        if protocol.game().tick() != frame.tick {
            return Err("Protocol frame 与 GameSession 已提交 tick 不一致".to_owned());
        }
        record_sequence(&mut sequence, frame.seq_from, frame.seq_to)?;
        events = events
            .checked_add(u64::try_from(frame.events().len()).map_err(|_| "市场事件数溢出")?)
            .ok_or("事件计数溢出")?;
        drop(frame);
        let elapsed = tick_started.elapsed().as_nanos();
        if stop.tick.is_multiple_of(setup.ticks_per_day) && !stop.complete() {
            terminal_step_ns = elapsed;
        } else {
            add_ns(&mut intraday_ns, elapsed)?;
        }
        if stop.complete() {
            break;
        }
    }
    let run_ns = started.elapsed().as_nanos();
    let npc_count = u64::from(setup.npcs.retail_count)
        + u64::from(setup.npcs.inst_count)
        + u64::from(setup.npcs.hot_count);
    Ok(json!({
        "schema": "runtime-sync-policy-sample", "schema_version": 1,
        "policy": policy, "seed": seed.to_string(), "setup": input_setup,
        "effective_setup": setup, "trading_days": days,
        "completed_trading_days": stop.completed_days, "completed_ticks": stop.tick,
        "expected_ticks": stop.target_tick, "last_tick": protocol.game().tick(),
        "civil_updates": civil_updates, "closed_civil_days": closed_civil_days,
        "trading_day_boundaries": boundaries,
        "run_wall_ns": run_ns.to_string(), "intraday_wall_ns": intraday_ns.to_string(),
        "boundary_wall_ns": boundary_wall, "closed_civil_wall_ns": closed_civil_ns.to_string(),
        "initialization_wall_ns": initialization_ns.to_string(),
        "final_civil_date": protocol.game().civil_date().to_iso(),
        "final_phase": "intraday_trading", "next_day_first_tick_confirmed": true,
        "final_seq": sequence, "final_mirror_origin": protocol.game().runtime_mirror_origin(),
        "events": events, "pure_flush_wall_ns": null, "pure_flush_availability": "unavailable",
        "topology": { "groups": groups, "desks": desks }, "stocks": setup.stocks.len(),
        "npc_count": npc_count,
    }))
}

fn run() -> Result<Value, String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let [setup_path, policy_name, seed, days] = args.as_slice() else {
        return Err("用法：runtime_sync_policy_probe <setup.json> <every_tick|day_end> <seed> <trading_days>".to_owned());
    };
    let policy = match policy_name.as_str() {
        "every_tick" => RuntimeSyncPolicy::EveryTick,
        "day_end" => RuntimeSyncPolicy::DayEnd,
        _ => return Err("本配对只接受 every_tick 或 day_end".to_owned()),
    };
    let seed: u64 = seed
        .parse()
        .map_err(|error| format!("seed 必须为 u64：{error}"))?;
    let days: u64 = days
        .parse()
        .map_err(|error| format!("trading_days 必须为 u64：{error}"))?;
    let mut input: Value =
        serde_json::from_slice(&fs::read(setup_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    input
        .as_object_mut()
        .ok_or("setup 必须为 JSON object")?
        .insert("runtime_sync_policy".to_owned(), json!(policy_name));
    let setup: SessionSetup =
        serde_json::from_value(input.clone()).map_err(|error| error.to_string())?;
    setup.validate().map_err(|error| error.to_string())?;
    StopPoint::new(setup.ticks_per_day, days)?;
    if setup.runtime_sync_policy != policy || !engine::runtime::step_runtime_enabled() {
        return Err("本探针必须使用对应 policy 的 native runtime；禁止串行 fallback".to_owned());
    }
    let initialization = Instant::now();
    let mut protocol =
        ProtocolSession::new(setup.clone(), seed).map_err(|error| error.to_string())?;
    let initialization_ns = initialization.elapsed().as_nanos();
    let result = measure(
        &mut protocol,
        &setup,
        input,
        policy_name,
        seed,
        days,
        initialization_ns,
    );
    let shutdown = protocol
        .shutdown()
        .map_err(|error| format!("Session/World 关闭失败：{error:?}"));
    match (result, shutdown) {
        (Ok(mut sample), Ok(report)) => {
            let completion = report
                .runtime
                .as_ref()
                .ok_or("真实市场拍完成后缺少 runtime 关闭确认")?;
            let (groups, desks) = setup
                .default_new_runtime_topology()
                .map_err(|error| error.to_string())?;
            if completion.owned_workers != groups + desks
                || completion.joined_workers != completion.owned_workers
                || completion.group_finals != groups
                || completion.desk_finals != desks
                || completion
                    .worker_results
                    .iter()
                    .any(|worker| worker.result.is_err())
            {
                return Err(
                    "runtime 关闭确认的 owned/joined/Final 数量或 worker 结果不符".to_owned(),
                );
            }
            sample["shutdown_confirmed"] = json!(true);
            sample["shutdown"] = serde_json::to_value(report).map_err(|error| error.to_string())?;
            Ok(sample)
        }
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}；{cleanup}")),
    }
}

fn main() {
    match run() {
        Ok(sample) => println!("{sample}"),
        Err(error) => {
            eprintln!("RuntimeSyncPolicy 测量失败：{error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StopPoint;

    #[test]
    fn stop_requires_actual_day_end_and_exact_next_first_tick() {
        let mut stop = StopPoint::new(2, 1).unwrap();
        stop.accept_tick(1).unwrap();
        stop.accept_tick(2).unwrap();
        assert!(!stop.complete());
        assert!(stop.accept_tick(3).is_err(), "未交真实日结不能越入次日");
        stop.accept_trading_day(2).unwrap();
        assert!(!stop.complete());
        stop.accept_tick(3).unwrap();
        assert!(stop.complete());
        assert!(stop.accept_tick(4).is_err(), "禁止超过精确停止点");
    }

    #[test]
    fn duplicate_skipped_ticks_and_false_boundaries_do_not_advance() {
        let mut stop = StopPoint::new(2, 2).unwrap();
        assert!(stop.accept_tick(2).is_err());
        assert_eq!(stop.tick, 0);
        stop.accept_tick(1).unwrap();
        assert!(stop.accept_tick(1).is_err());
        assert!(stop.accept_trading_day(1).is_err());
        stop.accept_tick(2).unwrap();
        stop.accept_trading_day(2).unwrap();
        assert!(stop.accept_trading_day(2).is_err());
        stop.accept_tick(3).unwrap();
        stop.accept_tick(4).unwrap();
        stop.accept_trading_day(4).unwrap();
        assert!(!stop.complete());
        stop.accept_tick(5).unwrap();
        assert!(stop.complete());
    }

    #[test]
    fn invalid_or_overflowed_workload_is_rejected() {
        assert!(StopPoint::new(0, 1).is_err());
        assert!(StopPoint::new(1, 0).is_err());
        assert!(StopPoint::new(u64::MAX, 1).is_err());
        assert!(StopPoint::new(2, u64::MAX).is_err());
    }
}
