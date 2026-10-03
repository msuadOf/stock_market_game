//! 外部配对性能测量的当前 engine endpoint。
//! 测量前完成编译；wall time/RSS 由 parent runner 采集，
//! endpoint 只报告实际提交的 tick 与 PhaseTimingPhase 观测。
use engine::{GameSession, SessionSetup};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    process,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    schema_version: u32,
    workload: Workload,
    environment_contract: Value,
    source_fingerprint: String,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Workload {
    scenario: String,
    seed: String,
    setup_manifest: Value,
    completed_ticks: u64,
    repetitions: u64,
    profile: String,
    features: Vec<String>,
}

fn execute(request: Request) -> Result<Value, String> {
    if request.schema != "escrow-performance-endpoint-request" || request.schema_version != 1 {
        return Err("unsupported endpoint request schema".into());
    }
    if request.source_fingerprint.len() != 64
        || !request
            .source_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("source fingerprint must be a lowercase SHA-256".into());
    }
    let workload = &request.workload;
    if workload.completed_ticks == 0
        || workload.repetitions == 0
        || !workload
            .completed_ticks
            .is_multiple_of(workload.repetitions)
    {
        return Err("completed tick count must be positive and divisible by repetitions".into());
    }
    let built_source_fingerprint = option_env!("ESCROW_SOURCE_FINGERPRINT")
        .ok_or("performance endpoint was not built by the source-fingerprinted escrow verification builder")?;
    if request.source_fingerprint != built_source_fingerprint {
        return Err("endpoint build fingerprint differs from requested source fingerprint".into());
    }
    let expected_profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    if workload.profile != expected_profile {
        return Err(format!(
            "endpoint build profile is {expected_profile}, requested {}",
            workload.profile
        ));
    }
    let mut features = Vec::new();
    if cfg!(feature = "simulation-diagnostics") {
        features.push("simulation-diagnostics".to_owned());
    }
    if cfg!(feature = "verification-harness") {
        features.push("verification-harness".to_owned());
    }
    let mut requested_features = workload.features.clone();
    requested_features.sort();
    if requested_features != features {
        return Err("endpoint build features differ from workload".into());
    }
    let threads = request
        .environment_contract
        .get("rayon_threads")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or("invalid actual rayon thread request")?;
    let setup: SessionSetup = serde_json::from_value(workload.setup_manifest.clone())
        .map_err(|error| format!("setup manifest is not a SessionSetup: {error}"))?;
    setup.validate().map_err(|error| error.to_string())?;
    let seed = workload
        .seed
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| error.to_string())?;
    let (completed, totals, samples) = pool.install(|| {
        let mut completed = 0_u64;
        let mut totals = BTreeMap::<String, u128>::new();
        let mut samples = Vec::new();
        for _ in 0..workload.repetitions {
            let mut game =
                GameSession::new(setup.clone(), seed).map_err(|error| error.to_string())?;
            for _ in 0..workload.completed_ticks / workload.repetitions {
                // 两个交易时段间完成真实自然日操作；
                // 不生成虚假 TickFrame，也不从 wall time 中扣除这些操作。
                while game.day()
                    == game
                        .civil_clock()
                        .completed_trading_sessions_expected()
                        .map_err(|error| error.to_string())?
                {
                    game.end_civil_day().map_err(|error| error.to_string())?;
                }
                let timed = game
                    .step_with_phase_timing()
                    .map_err(|error| error.to_string())?;
                for record in timed.timing().records() {
                    let total = totals.entry(record.phase().name().to_owned()).or_default();
                    *total = total
                        .checked_add(record.wall_time_ns())
                        .ok_or("phase wall overflow")?;
                    samples.push(record.runnable_thread_sample().minimum());
                    samples.push(record.runnable_thread_sample().maximum());
                }
                completed = completed
                    .checked_add(1)
                    .ok_or("committed tick count overflow")?;
            }
        }
        Ok::<_, String>((completed, totals, samples))
    })?;
    if completed != workload.completed_ticks || totals.len() != 10 || samples.is_empty() {
        return Err("已提交 workload 或全部 PhaseTimingPhase 计时不完整".into());
    }
    let phase_wall_ns = totals
        .into_iter()
        .map(|(phase, total)| {
            u64::try_from(total)
                .map(|total| (phase, total.to_string()))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(
        json!({ "schema": "escrow-performance-sample", "schema_version": 2, "status": "PASS", "workload": request.workload,
        "environment_contract": request.environment_contract, "source_fingerprint": request.source_fingerprint,
        "completed_ticks": completed, "phase_wall_ns": phase_wall_ns, "runnable_samples": samples }),
    )
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [input] = args.as_slice() else {
        return Err("usage: escrow_performance_endpoint <request.json>".into());
    };
    let request = serde_json::from_slice(&fs::read(input).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let output = execute(request)?;
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer(&mut writer, &output).map_err(|error| error.to_string())?;
    writeln!(writer).map_err(|error| error.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("performance endpoint FAIL: {error}");
        process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unexecuted_or_mismatched_workload_before_reporting_a_sample() {
        let request = Request {
            schema: "escrow-performance-endpoint-request".into(),
            schema_version: 1,
            source_fingerprint: "a".repeat(64),
            environment_contract: json!({"rayon_threads": 1}),
            workload: Workload {
                scenario: "test".into(),
                seed: "1".into(),
                setup_manifest: json!({}),
                completed_ticks: 3,
                repetitions: 2,
                profile: "debug".into(),
                features: Vec::new(),
            },
        };
        assert!(execute(request).unwrap_err().contains("divisible"));
    }
}
