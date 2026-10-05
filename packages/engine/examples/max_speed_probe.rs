//! 默认市场的完整 tick 性能探针；不改变 NPC 数量或交易语义。
use engine::{GameSession, SessionSetup};
use std::{collections::BTreeMap, env, fs, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let [setup_path, ticks, threads] = args.as_slice() else {
        return Err("用法：max_speed_probe <setup.json> <ticks> <threads>".into());
    };
    let setup: SessionSetup = serde_json::from_slice(&fs::read(setup_path)?)?;
    let ticks: u64 = ticks.parse()?;
    let threads: usize = threads.parse()?;
    if ticks == 0 || threads == 0 {
        return Err("ticks 与 threads 必须大于零".into());
    }
    // 与生产宿主一样，coordinator 运行在调用线程，保留全部 Rayon worker 处理计算任务。
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()?;
    (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut game = GameSession::new(setup, 42)?;
        let mut phases = BTreeMap::<String, u128>::new();
        let mut events = 0;
        let started = Instant::now();
        for _ in 0..ticks {
            while game.day() == game.civil_clock().completed_trading_sessions_expected()? {
                game.end_civil_day()?;
            }
            let result = game.step_with_phase_timing()?;
            let (batch, timing) = result.into_parts();
            events += batch.len();
            for record in timing.records() {
                *phases.entry(record.phase().name().to_owned()).or_default() += record.wall_time_ns();
            }
        }
        let wall_s = started.elapsed().as_secs_f64();
        // 在计时结束后记录实际负载，避免仅凭配置账户数猜测活跃订单与决策规模。
        let final_load = serde_json::json!({
            "live_envelopes": game.project_live_envelopes()?.len(),
            "last_retail_decisions": game.last_retail_decisions().len(),
            "last_retail_order_events": game.last_retail_order_events().len(),
            "plans": game.plans_debug().len(),
        });
        println!("{}", serde_json::json!({"ticks":ticks,"threads":threads,"wall_s":wall_s,"speed_x":ticks as f64 / wall_s,"events":events,"phase_ns":phases,"final_load":final_load}));
        Ok(())
    })().map_err(|error| -> Box<dyn std::error::Error> { error })
}
