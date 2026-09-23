//! Controlled Task 9 performance adapter for historical revision 7041d35.
//!
//! This file is copied verbatim into the historical checkout and compiled
//! there.  It executes the historical production `GameSession::step` path for
//! exactly the common workload.  The old engine has no P0--P9 timing seam, so
//! the corresponding sample fields are deliberately `null`; the external
//! runner still measures wall time, process-tree RSS, and runnable threads.

use engine::{GameSession, SessionSetup};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, Write},
    process,
};

const BASELINE_COMMIT: &str = "7041d35dc362ca74f4f3313e6804db9499f0679a";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
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
    if request.schema != "escrow-perf-endpoint-request-v1" {
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
        || workload.completed_ticks % workload.repetitions != 0
    {
        return Err("completed tick count must be positive and divisible by repetitions".into());
    }
    let built_source_fingerprint = option_env!("ESCROW_SOURCE_FINGERPRINT")
        .ok_or("historical endpoint was not built by the source-fingerprinted Task 9 builder")?;
    if request.source_fingerprint != built_source_fingerprint {
        return Err(
            "historical endpoint build fingerprint differs from requested source fingerprint"
                .into(),
        );
    }
    if workload.profile
        != if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    {
        return Err("endpoint build profile differs from workload".into());
    }
    if !workload.features.is_empty() {
        return Err("historical baseline adapter requires the common empty feature set".into());
    }
    let threads = request
        .environment_contract
        .get("rayon_threads")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or("invalid actual rayon thread request")?;

    // The policy identifier names the implementation revision, not workload
    // shape.  The current v2 manifest is echoed unchanged in the sample, while
    // this one explicit compatibility rewrite lets revision 7041d35 validate
    // its own v1 identity.  Every market/config/NPC/calendar field is otherwise
    // deserialized from the exact common manifest.
    let mut baseline_setup = workload.setup_manifest.clone();
    baseline_setup
        .as_object_mut()
        .ok_or("setup manifest must be an object")?
        .insert(
            "simulation_policy_id".into(),
            Value::String("a-share-simulation-v1".into()),
        );
    let setup: SessionSetup = serde_json::from_value(baseline_setup)
        .map_err(|error| format!("setup manifest is not a historical SessionSetup: {error}"))?;
    setup.validate().map_err(|error| error.to_string())?;
    let seed = workload
        .seed
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| error.to_string())?;
    let completed = pool.install(|| {
        let mut completed = 0_u64;
        for _ in 0..workload.repetitions {
            let mut game =
                GameSession::new(setup.clone(), seed).map_err(|error| error.to_string())?;
            for _ in 0..workload.completed_ticks / workload.repetitions {
                while game.day()
                    == game
                        .civil_clock()
                        .completed_trading_sessions_expected()
                        .map_err(|error| error.to_string())?
                {
                    game.end_civil_day().map_err(|error| error.to_string())?;
                }
                let _events = game.step();
                completed = completed
                    .checked_add(1)
                    .ok_or("committed tick count overflow")?;
            }
        }
        Ok::<u64, String>(completed)
    })?;
    if completed != workload.completed_ticks {
        return Err("historical baseline did not complete the declared workload".into());
    }
    Ok(json!({
        "schema": "escrow-perf-sample-v2",
        "status": "PASS",
        "workload": request.workload,
        "environment_contract": request.environment_contract,
        "source_fingerprint": request.source_fingerprint,
        "completed_ticks": completed,
        "phase_wall_ns": null,
        "runnable_samples": null,
    }))
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [input] = args.as_slice() else {
        return Err(format!("usage: escrow_performance_baseline_adapter <request.json> (baseline {BASELINE_COMMIT})"));
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
        eprintln!("historical performance adapter FAIL: {error}");
        process::exit(2);
    }
}
