//! Measure complete committed ticks with NPC, player, and plan orders in one session.
//!
//! The input describes one workload; run the same file with different Rayon pool
//! sizes. Enqueueing and host quote reads happen outside `step_wall_ns`, while
//! `run_wall_ns` includes them and civil-day work. No request or thread cap is
//! imposed by this endpoint.

use engine::plans::PlanOpen;
use engine::{
    AccountId, Event, GameSession, Intent, OpinionSource, PlanOpinion, PlanTarget, SessionSetup,
    Side, Urgency,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, process,
    time::Instant,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workload {
    setup: SessionSetup,
    seed: u64,
    ticks: u64,
    plan_accounts: usize,
    player_orders_per_tick: usize,
}

fn positive_usize(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|parsed| *parsed > 0)
        .ok_or_else(|| format!("{name} must be a positive integer"))
}

fn seed_active_plans(
    game: GameSession,
    plan_accounts: usize,
) -> Result<(GameSession, BTreeSet<AccountId>), String> {
    let mut save = game.save().map_err(|error| error.to_string())?;
    let accounts = save
        .belief_books
        .keys()
        .copied()
        .take(plan_accounts)
        .collect::<Vec<_>>();
    if accounts.len() != plan_accounts {
        return Err(format!(
            "requested {plan_accounts} plan accounts, but setup has only {} belief accounts",
            accounts.len()
        ));
    }
    for (index, account) in accounts.iter().enumerate() {
        let attention = save
            .npc_attention
            .get_mut(account)
            .ok_or_else(|| format!("plan account {account:?} lacks NPC attention state"))?;
        attention.next_attention_candidate_tick = save.snapshot.tick;
        save.plans
            .create(PlanOpen {
                account: *account,
                code: save.setup.stocks[index % save.setup.stocks.len()]
                    .code
                    .clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(10_000),
                opinion: PlanOpinion {
                    signal_score_bp: 5_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 60,
                created_trading_day: u64::from(save.snapshot.day),
            })
            .map_err(|error| error.to_string())?;
    }
    let restored = GameSession::restore(&save).map_err(|error| error.to_string())?;
    Ok((restored, accounts.into_iter().collect()))
}

fn execute(
    workload: Workload,
    threads: usize,
    input_fingerprint: String,
) -> Result<serde_json::Value, String> {
    if workload.ticks == 0 || workload.plan_accounts == 0 || workload.player_orders_per_tick == 0 {
        return Err("ticks, plan_accounts, and player_orders_per_tick must all be positive".into());
    }
    workload
        .setup
        .validate()
        .map_err(|error| error.to_string())?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| error.to_string())?;
    pool.install(|| {
        let game = GameSession::new(workload.setup.clone(), workload.seed)
            .map_err(|error| error.to_string())?;
        let (mut game, plan_accounts) = seed_active_plans(game, workload.plan_accounts)?;
        let stock_codes = workload
            .setup
            .stocks
            .iter()
            .map(|stock| stock.code.clone())
            .collect::<Vec<_>>();
        let run_start = Instant::now();
        let mut step_wall_ns = 0_u128;
        let mut player_resting_orders = 0_u64;
        let mut other_npc_resting_orders = 0_u64;
        let mut plan_account_resting_orders = 0_u64;
        let mut player_rejected = 0_u64;
        let mut phase_wall_ns = BTreeMap::<String, u128>::new();
        for tick in 0..workload.ticks {
            while game.day()
                == game
                    .civil_clock()
                    .completed_trading_sessions_expected()
                    .map_err(|error| error.to_string())?
            {
                game.end_civil_day().map_err(|error| error.to_string())?;
            }
            let quotes = game.runtime_snapshot().markets;
            for offset in 0..workload.player_orders_per_tick {
                let stock_index = (tick as usize)
                    .wrapping_mul(workload.player_orders_per_tick)
                    .wrapping_add(offset)
                    % stock_codes.len();
                let code = stock_codes[stock_index].clone();
                let price = quotes
                    .get(&code)
                    .ok_or_else(|| format!("stock {code:?} has no runtime quote"))?
                    .last_price;
                game.enqueue_player_intent(
                    AccountId(0),
                    Intent::PlaceLimit {
                        code,
                        side: Side::Buy,
                        price,
                        qty: 100,
                    },
                )
                .map_err(|error| error.to_string())?;
            }
            let started = Instant::now();
            let timed = game
                .step_with_phase_timing()
                .map_err(|error| format!("workload tick {tick}: {error}"))?;
            step_wall_ns = step_wall_ns
                .checked_add(started.elapsed().as_nanos())
                .ok_or("step wall-time counter overflow")?;
            let (events, timing) = timed.into_parts();
            for record in timing.records() {
                let total = phase_wall_ns
                    .entry(format!("P{}", record.phase().rank()))
                    .or_default();
                *total = total
                    .checked_add(record.wall_time_ns())
                    .ok_or("phase wall-time counter overflow")?;
            }
            for event in events {
                match event {
                    Event::OrderAccepted { account, .. } if account == AccountId(0) => {
                        player_resting_orders += 1;
                    }
                    Event::OrderAccepted { account, .. }
                        if plan_accounts.contains(&account) =>
                    {
                        plan_account_resting_orders += 1;
                    }
                    Event::OrderAccepted { .. } => other_npc_resting_orders += 1,
                    Event::IntentRejected { account, .. } if account == AccountId(0) => {
                        player_rejected += 1;
                    }
                    _ => {}
                }
            }
        }
        let run_wall_ns = run_start.elapsed().as_nanos();
        let final_save = game.save().map_err(|error| error.to_string())?;
        let mut plan_children_or_fills = 0_usize;
        for plan_id in final_save.plans.plan_ids() {
            let plan = final_save
                .plans
                .plan(plan_id)
                .map_err(|error| error.to_string())?;
            if plan_accounts.contains(&plan.account)
                && (plan.active_child_order_id.is_some() || plan.filled_qty > 0)
            {
                plan_children_or_fills += 1;
            }
        }
        if player_resting_orders == 0
            || other_npc_resting_orders == 0
            || plan_children_or_fills == 0
        {
            return Err(format!(
                "load was not active in all three sources: player resting orders {player_resting_orders}, other NPC resting orders {other_npc_resting_orders}, plan children/fills {plan_children_or_fills}"
            ));
        }
        let phase_wall_ns = phase_wall_ns
            .into_iter()
            .map(|(phase, wall)| (phase, wall.to_string()))
            .collect::<BTreeMap<_, _>>();
        Ok(json!({
            "schema": "production-entry-performance-v2",
            "input_fingerprint_fnv1a64": input_fingerprint,
            "seed": workload.seed,
            "setup": workload.setup,
            "ticks": workload.ticks,
            "plan_accounts": workload.plan_accounts,
            "player_orders_per_tick": workload.player_orders_per_tick,
            "rayon_threads": rayon::current_num_threads(),
            "step_wall_ns": step_wall_ns.to_string(),
            "run_wall_ns": run_wall_ns.to_string(),
            "phase_wall_ns": phase_wall_ns,
            "player_resting_orders": player_resting_orders,
            "player_rejected": player_rejected,
            "other_npc_resting_orders": other_npc_resting_orders,
            "plan_account_resting_orders": plan_account_resting_orders,
            "plan_children_or_fills": plan_children_or_fills,
        }))
    })
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [input, threads] = args.as_slice() else {
        return Err("usage: production_entry_performance <workload.json> <rayon-threads>".into());
    };
    let input_bytes = fs::read(input).map_err(|e| e.to_string())?;
    let workload: Workload =
        serde_json::from_slice(&input_bytes).map_err(|error| error.to_string())?;
    let fingerprint = input_bytes
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    let output = execute(
        workload,
        positive_usize(threads, "rayon-threads")?,
        format!("{fingerprint:016x}"),
    )?;
    println!("{output}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("production entry performance FAIL: {error}");
        process::exit(2);
    }
}
