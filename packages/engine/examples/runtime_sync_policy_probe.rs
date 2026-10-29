//! 同一生产 ProtocolSession 按实际自然日日结精确停止的 RuntimeSyncPolicy 探针。
//! 纯自动 flush 没有公开独立计时读面；只报告真实边界合计 wall。

use engine::session::protocol::{ProtocolSession, SaveCandidateKey};
use engine::{RuntimeSyncPolicy, SessionSetup};
use serde_json::{json, Value};
use std::{env, fs, time::Instant};

#[derive(Debug)]
struct CivilStopPoint {
    ticks_per_day: u64,
    n: u64,
    target_days: u64,
    civil_days: u64,
    trading_days: u64,
    tick: u64,
}

impl CivilStopPoint {
    fn new(ticks_per_day: u64, n: u64) -> Result<Self, String> {
        if ticks_per_day == 0 || n == 0 {
            return Err("日长与自然日 N 必须为正".to_owned());
        }
        let target_days = n.checked_mul(2).ok_or("2N 自然日数溢出")?;
        ticks_per_day
            .checked_mul(target_days)
            .ok_or("2N 最大 tick 数溢出")?;
        Ok(Self {
            ticks_per_day,
            n,
            target_days,
            civil_days: 0,
            trading_days: 0,
            tick: 0,
        })
    }

    fn accept_tick(&mut self, tick: u64) -> Result<(), String> {
        let next = self.tick.checked_add(1).ok_or("市场 tick 溢出")?;
        let boundary = (self.trading_days + 1)
            .checked_mul(self.ticks_per_day)
            .ok_or("交易日日界溢出")?;
        if self.complete() || tick != next || tick > boundary {
            return Err("自然日诊断的市场 tick 不连续或越过未确认日结".to_owned());
        }
        self.tick = tick;
        Ok(())
    }

    fn accept_day(&mut self, tick: u64, trading: bool) -> Result<(), String> {
        let trading_days = self
            .trading_days
            .checked_add(u64::from(trading))
            .ok_or("交易日计数溢出")?;
        let expected = trading_days
            .checked_mul(self.ticks_per_day)
            .ok_or("交易日日界溢出")?;
        if self.complete() || self.tick != tick || tick != expected {
            return Err("自然日日结与已完成市场 tick 不一致".to_owned());
        }
        self.civil_days += 1;
        self.trading_days = trading_days;
        Ok(())
    }

    fn observation_due(&self) -> bool {
        self.civil_days == self.n || self.complete()
    }
    fn complete(&self) -> bool {
        self.civil_days == self.target_days
    }
}

#[derive(Default)]
struct CountingWriter {
    bytes: u64,
}

impl std::io::Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(u64::try_from(bytes.len()).map_err(std::io::Error::other)?)
            .ok_or_else(|| std::io::Error::other("serialized bytes 计数溢出"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialized_bytes(value: &impl serde::Serialize) -> Result<u64, String> {
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| format!("域序列化计数失败：{error}"))?;
    Ok(writer.bytes)
}

fn parse_vmrss_bytes(status: &str) -> Result<u64, String> {
    let mut rss = None;
    for line in status.lines().filter(|line| line.starts_with("VmRSS:")) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if rss.is_some()
            || fields.len() != 3
            || fields[2] != "kB"
            || fields[1].is_empty()
            || !fields[1].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err("/proc VmRSS 字段重复、格式或单位无效".to_owned());
        }
        let kib: u64 = fields[1]
            .parse()
            .map_err(|error| format!("VmRSS 数值无效：{error}"))?;
        let bytes = kib.checked_mul(1024).ok_or("VmRSS 字节数溢出")?;
        if bytes == 0 {
            return Err("/proc VmRSS 为零，不能作为有效 RSS 快照".to_owned());
        }
        rss = Some(bytes);
    }
    rss.ok_or_else(|| "/proc/status 缺少 VmRSS，不能补造 RSS".to_owned())
}

fn process_tree_rss_snapshot() -> Result<u64, String> {
    let tasks = fs::read_dir("/proc/self/task")
        .map_err(|error| format!("读取自身 task 列表失败：{error}"))?;
    let mut task_count = 0_u64;
    // 每个 task 都检查，避免只读主线程的 children 而漏掉其它线程的 child。
    for entry in tasks {
        let entry = entry.map_err(|error| format!("读取自身 task 项失败：{error}"))?;
        let task_id = entry
            .file_name()
            .into_string()
            .map_err(|_| "task id 不是 UTF-8")?;
        if task_id.is_empty() || !task_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("/proc/self/task 出现无效 task id".to_owned());
        }
        let children = fs::read_to_string(entry.path().join("children"))
            .map_err(|error| format!("读取 task {task_id} 的 children 失败：{error}"))?;
        if !children.trim().is_empty() {
            return Err(format!(
                "task {task_id} 存在 child，禁止把自身 VmRSS 当作整个进程树：{children}"
            ));
        }
        task_count = task_count.checked_add(1).ok_or("task 计数溢出")?;
    }
    if task_count == 0 {
        return Err("/proc/self/task 为空，无法验证进程树".to_owned());
    }
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|error| format!("读取自身 VmRSS 失败：{error}"))?;
    parse_vmrss_bytes(&status)
}

fn memory_observation(
    save: &engine::SaveSlot,
    stop: &CivilStopPoint,
    settled_date: engine::CivilDate,
) -> Result<Value, String> {
    let before = process_tree_rss_snapshot()?;
    let mut plan_count = 0_u64;
    let mut terminal_count = 0_u64;
    for id in save.plans.plan_ids() {
        plan_count = plan_count.checked_add(1).ok_or("PlanBook 计数溢出")?;
        if save
            .plans
            .entry(id)
            .map_err(|error| error.to_string())?
            .is_terminal()
        {
            terminal_count = terminal_count
                .checked_add(1)
                .ok_or("terminal PlanBook 计数溢出")?;
        }
    }
    // 计数 writer 不留大型 JSON；各域自定义 Serialize 仍可能分配临时空间。
    let domains = json!({
        "save_slot": { "serialized_bytes": serialized_bytes(save)?.to_string() },
        "plans": { "serialized_bytes": serialized_bytes(&save.plans)?.to_string(), "entry_count": plan_count, "terminal_entry_count": terminal_count, "owner_sequence_count": save.plans.owner_sequences().count() },
        "retained_market_days": { "serialized_bytes": serialized_bytes(&save.retained_market_days)?.to_string(), "entry_count": save.retained_market_days.len() },
        "personal_trade_confirmations": { "serialized_bytes": serialized_bytes(&save.personal_trade_confirmations)?.to_string(), "account_count": save.personal_trade_confirmations.len(), "entry_count": save.personal_trade_confirmations.values().map(Vec::len).sum::<usize>() },
        "filled_orders": { "serialized_bytes": serialized_bytes(&save.filled_orders)?.to_string(), "stock_count": save.filled_orders.len(), "entry_count": save.filled_orders.values().map(Vec::len).sum::<usize>() },
        "snapshot": { "serialized_bytes": serialized_bytes(&save.snapshot)?.to_string(), "account_count": save.snapshot.accounts.len(), "market_count": save.snapshot.markets.len(), "daily_candle_count": save.snapshot.daily_candles.values().map(Vec::len).sum::<usize>() },
        "runtime_state": { "serialized_bytes": serialized_bytes(&save.runtime_state)?.to_string(), "live_envelope_count": save.runtime_state.live_envelopes.len(), "retail_projection_seen_count": save.runtime_state.retail_projection_seen.len(), "strategy_state_count": save.runtime_state.strategy_states.len() },
    });
    let after = process_tree_rss_snapshot()?;
    Ok(json!({
        "civil_day_count": stop.civil_days, "settled_date": settled_date.to_iso(),
        "tick": save.snapshot.tick, "seq": save.snapshot.seq,
        "completed_trading_days": stop.trading_days, "closed_civil_days": stop.civil_days - stop.trading_days,
        "candidate_lifetime": "captured_clone_alive_update_dropped",
        "rss_before_serialization_bytes": before, "rss_after_serialization_bytes": after,
        "process_tree_process_count": 1, "rss_source": "linux_proc_self_status_vmrss_all_tasks_children_checked",
        "serialized_bytes_are_resident_allocation": false, "domains": domains,
    }))
}

fn civil_phase_name(phase: engine::session::CivilPhase) -> &'static str {
    match phase {
        engine::session::CivilPhase::IntradayTrading => "intraday_trading",
        engine::session::CivilPhase::ClosedDay => "closed_day",
    }
}

fn measure_memory_growth(
    protocol: &mut ProtocolSession,
    setup: &SessionSetup,
    input_setup: Value,
    policy: &str,
    seed: u64,
    n: u64,
    initialization_ns: u128,
) -> Result<Value, String> {
    let mut stop = CivilStopPoint::new(setup.ticks_per_day, n)?;
    let mut sequence = protocol.game().seq();
    let mut boundaries = Vec::new();
    let mut closed_dates = Vec::new();
    let mut observations = Vec::new();
    let started = Instant::now();
    while !stop.complete() {
        if protocol
            .civil_day_ready()
            .map_err(|error| error.to_string())?
        {
            let date = protocol.game().civil_date();
            let update = protocol
                .end_civil_day_update()
                .map_err(|error| error.to_string())?;
            update.validate().map_err(|error| error.to_string())?;
            record_sequence(&mut sequence, update.seq_from, update.seq_to)?;
            let phase = update.boundary.settled_phase;
            if update.boundary.settled_date != date
                || update.boundary.next_date != date.next().map_err(|error| error.to_string())?
            {
                return Err("自然日诊断的真实日结回执跳日或错日".to_owned());
            }
            stop.accept_day(
                update.tick,
                phase == engine::session::CivilPhase::IntradayTrading,
            )?;
            let key = SaveCandidateKey {
                settled_date: date,
                seq: update.seq_to,
            };
            let save = protocol
                .save_candidate(&key)
                .map_err(|error| error.to_string())?;
            if save.snapshot.tick != update.tick
                || save.snapshot.seq != update.seq_to
                || save.civil_clock.settled_through != Some(date)
            {
                return Err("自然日诊断候选的 tick/seq/日期与日结不同版".to_owned());
            }
            boundaries.push(json!({ "settled_date": date.to_iso(), "next_date": update.boundary.next_date.to_iso(), "tick": update.tick, "seq": update.seq_to, "settled_phase": civil_phase_name(phase) }));
            if phase == engine::session::CivilPhase::ClosedDay {
                closed_dates.push(date.to_iso());
            }
            drop(update);
            if stop.observation_due() {
                observations.push(memory_observation(&save, &stop, date)?);
            }
            // N 点只留下小观测；候选 clone 不得一直活到 2N 造成探针自身增长。
            drop(save);
        } else {
            if protocol.game().civil_clock().phase() != engine::session::CivilPhase::IntradayTrading
            {
                return Err("自然日诊断在未就绪休市日尝试市场推进".to_owned());
            }
            let frame = protocol.step_frame().map_err(|error| error.to_string())?;
            frame.validate().map_err(|error| error.to_string())?;
            stop.accept_tick(frame.tick)?;
            if protocol.game().tick() != frame.tick {
                return Err("自然日诊断 frame 与 GameSession tick 不一致".to_owned());
            }
            record_sequence(&mut sequence, frame.seq_from, frame.seq_to)?;
        }
    }
    let (groups, desks) = setup
        .default_new_runtime_topology()
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "schema": "runtime-sync-memory-growth-sample", "schema_version": 1,
        "policy": policy, "seed": seed.to_string(), "setup": input_setup, "effective_setup": setup,
        "civil_days_n": n, "completed_civil_days": stop.civil_days, "completed_trading_days": stop.trading_days,
        "completed_ticks": stop.tick, "last_tick": protocol.game().tick(), "civil_day_boundaries": boundaries,
        "closed_dates": closed_dates, "observations": observations,
        "initialization_wall_ns": initialization_ns.to_string(), "run_wall_ns": started.elapsed().as_nanos().to_string(),
        "final_civil_date": protocol.game().civil_date().to_iso(), "final_phase": civil_phase_name(protocol.game().civil_clock().phase()),
        "final_seq": sequence, "topology": { "groups": groups, "desks": desks }, "stocks": setup.stocks.len(),
        "npc_count": u64::from(setup.npcs.retail_count) + u64::from(setup.npcs.inst_count) + u64::from(setup.npcs.hot_count),
        "measurement_effects": "candidate clone 在前后 RSS 快照期间存活，CivilUpdate 已 drop；计数 serde 不保留大 JSON，但自定义 Serialize、allocator 留存与 Arc 按值表示会扰动，serialized bytes 不等于 resident allocation。",
        "pure_flush_wall_ns": null, "pure_flush_availability": "unavailable", "coverage_table_peak_count": null, "coverage_table_peak_availability": "unavailable",
    }))
}

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
            let save = protocol
                .save_candidate(&key)
                .map_err(|error| error.to_string())?;
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
    let memory_growth = args.first().is_some_and(|arg| arg == "memory-growth");
    let parameters = if memory_growth {
        &args[1..]
    } else {
        args.as_slice()
    };
    let [setup_path, policy_name, seed, days] = parameters else {
        return Err("用法：runtime_sync_policy_probe [memory-growth] <setup.json> <every_tick|day_end> <seed> <trading_days 或 civil_days_n>".to_owned());
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
        .map_err(|error| format!("trading_days 或 civil_days_n 必须为 u64：{error}"))?;
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
    if memory_growth {
        CivilStopPoint::new(setup.ticks_per_day, days)?;
    } else {
        StopPoint::new(setup.ticks_per_day, days)?;
    }
    if setup.runtime_sync_policy != policy || !engine::runtime::step_runtime_enabled() {
        return Err("本探针必须使用对应 policy 的 native runtime；禁止串行 fallback".to_owned());
    }
    let initialization = Instant::now();
    let mut protocol =
        ProtocolSession::new(setup.clone(), seed).map_err(|error| error.to_string())?;
    let initialization_ns = initialization.elapsed().as_nanos();
    let result = if memory_growth {
        measure_memory_growth(
            &mut protocol,
            &setup,
            input,
            policy_name,
            seed,
            days,
            initialization_ns,
        )
    } else {
        measure(
            &mut protocol,
            &setup,
            input,
            policy_name,
            seed,
            days,
            initialization_ns,
        )
    };
    let shutdown = protocol
        .shutdown()
        .map_err(|error| format!("Session/World 关闭失败：{error:?}"));
    match (result, shutdown) {
        (Ok(mut sample), Ok(report)) => {
            let (groups, desks) = setup
                .default_new_runtime_topology()
                .map_err(|error| error.to_string())?;
            if let Some(completion) = report.runtime.as_ref() {
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
            } else if !memory_growth || sample["completed_ticks"].as_u64() != Some(0) {
                return Err("真实市场拍完成后缺少 runtime 关闭确认".to_owned());
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
    use super::{parse_vmrss_bytes, serialized_bytes, CivilStopPoint, StopPoint};

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

    #[test]
    fn natural_day_observations_count_closed_days_without_market_ticks() {
        let mut stop = CivilStopPoint::new(2, 2).unwrap();
        stop.accept_day(0, false).unwrap();
        assert!(!stop.observation_due());
        stop.accept_tick(1).unwrap();
        stop.accept_tick(2).unwrap();
        assert!(stop.accept_tick(3).is_err(), "先交真实交易日日结");
        stop.accept_day(2, true).unwrap();
        assert!(stop.observation_due());
        assert_eq!(stop.trading_days, 1);
        stop.accept_day(2, false).unwrap();
        stop.accept_day(2, false).unwrap();
        assert!(stop.observation_due());
        assert!(stop.complete());
        assert_eq!(stop.tick, 2, "2N自然日不要求tick翻倍");
        assert!(stop.accept_day(2, false).is_err());
        assert!(stop.accept_tick(3).is_err());
    }

    #[test]
    fn natural_day_stop_rejects_incomplete_trading_days_and_overflow() {
        let mut stop = CivilStopPoint::new(2, 1).unwrap();
        stop.accept_tick(1).unwrap();
        assert!(stop.accept_day(1, true).is_err());
        assert!(stop.accept_day(1, false).is_err());
        assert_eq!(stop.civil_days, 0);
        assert!(CivilStopPoint::new(0, 1).is_err());
        assert!(CivilStopPoint::new(1, 0).is_err());
        assert!(CivilStopPoint::new(1, u64::MAX).is_err());
        assert!(CivilStopPoint::new(u64::MAX, 1).is_err());
    }

    #[test]
    fn counting_serialization_matches_small_json_without_retaining_buffer() {
        let value = serde_json::json!({"中文": [1, 2, 3], "empty": []});
        assert_eq!(
            serialized_bytes(&value).unwrap(),
            serde_json::to_vec(&value).unwrap().len() as u64
        );
    }

    #[test]
    fn rss_requires_one_valid_positive_kib_field_and_checks_overflow() {
        assert_eq!(
            parse_vmrss_bytes("Name:\tprobe\nVmRSS:\t42 kB\n").unwrap(),
            42 * 1024
        );
        for text in [
            "Name: probe",
            "VmRSS: 0 kB",
            "VmRSS: 1 MB",
            "VmRSS: x kB",
            "VmRSS: 1 kB\nVmRSS: 2 kB",
            "VmRSS: 18446744073709551615 kB",
        ] {
            assert!(
                parse_vmrss_bytes(text).is_err(),
                "不能用零或默认值掩盖无效/proc：{text}"
            );
        }
    }
}
