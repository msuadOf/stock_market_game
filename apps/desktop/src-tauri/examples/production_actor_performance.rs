//! Exercise the real desktop actor command path without opening a GUI window.
//! Run with `RAYON_NUM_THREADS=<workers>`; the pool size does not limit requests.

use engine::{calendar::CivilDate, Intent, SessionSetup, Side};
use serde::Deserialize;
use std::{env, fs, process, time::Instant};
use stock_market_game_lib::actor::SessionManager;

#[derive(Deserialize)]
struct Workload {
    setup: SessionSetup,
    seed: u64,
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [input, steps] = args.as_slice() else {
        return Err("usage: production_actor_performance <workload.json> <steps>".into());
    };
    let steps = steps.parse::<u64>()?;
    if steps == 0 {
        return Err("steps must be positive".into());
    }
    let mut workload: Workload = serde_json::from_slice(&fs::read(input)?)?;
    // This probe starts on a trading day so every requested step exercises the
    // actor's market path rather than a civil-day publication boundary.
    workload.setup.start_date = CivilDate::from_iso("2030-01-02")?;
    let first_stock = workload
        .setup
        .stocks
        .first()
        .ok_or("workload has no stock")?;
    let code = first_stock.code.clone();
    let price = first_stock.initial_price;
    let configured_npc_accounts = u64::from(workload.setup.npcs.retail_count)
        + u64::from(workload.setup.npcs.inst_count)
        + u64::from(workload.setup.npcs.hot_count);
    let app = tauri::test::mock_app();
    let manager = SessionManager::default();
    let session_id = manager
        .new_session(workload.setup, workload.seed, app.handle().clone())
        .await?;
    let handles = manager.lookup(&session_id).await.ok_or("actor is absent")?;
    let start = Instant::now();
    for _ in 0..steps {
        handles
            .enqueue(Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price,
                qty: 100,
            })
            .await?;
        handles.step(1).await?;
    }
    let elapsed = start.elapsed();
    handles.shutdown().await?;
    println!(
        "{}",
        serde_json::json!({
            "steps": steps,
            "step_wall_ns": elapsed.as_nanos().to_string(),
            "configured_npc_accounts": configured_npc_accounts,
            "player_requests": steps,
        })
    );
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("desktop actor performance failed: {error}");
        process::exit(2);
    }
}
