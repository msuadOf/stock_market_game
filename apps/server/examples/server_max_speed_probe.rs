//! 使用生产 SessionActor 的 Fastest 路径测量已提交市场 tick；不包含网络与前端。
use engine::{session::protocol::EngineUpdate, SessionSetup};
use server::SessionManager;
use std::{env, fs, time::Instant};


/// 基线没有独立 context API，实际计算使用唯一 Rayon global registry；
/// 当前实现的同名 inherent getter 优先读取真实宿主 context，不创建第二个 global pool。
trait BaselineRegistryAccess {
    fn compute_workers(&self) -> Result<usize, Box<dyn std::error::Error>>;
}
impl BaselineRegistryAccess for SessionManager {
    fn compute_workers(&self) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(rayon::current_num_threads())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let [setup_path, ticks, threads] = args.as_slice() else {
        return Err("用法：server_max_speed_probe <setup.json> <ticks> <threads>".into());
    };
    let setup: SessionSetup = serde_json::from_slice(&fs::read(setup_path)?)?;
    let ticks: u64 = ticks.parse()?;
    let threads: usize = threads.parse()?;
    if ticks == 0 || threads == 0 {
        return Err("ticks 与 threads 必须大于零".into());
    }
    // 在首次使用 Rayon 前固定 worker 数；进程 affinity 由外部 supervisor 设置。
    env::set_var("RAYON_NUM_THREADS", threads.to_string());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let manager = SessionManager::default_base();
        let actual_workers = manager.compute_workers()?;
        if actual_workers != threads { return Err(format!("实际 registry={actual_workers}，请求={threads}").into()); }
        let initialization = Instant::now();
        let id = manager.new_session(setup, 42)?;
        let handles = manager.lookup(&id).ok_or("新建 session 未注册")?;
        let initialization_s = initialization.elapsed().as_secs_f64();
        let mut receiver = handles.subscribe_events();
        handles.set_speed(f64::INFINITY).await?;
        let started = Instant::now();
        handles.set_running(true).await?;
        let mut committed = 0_u64;
        let mut last_tick = 0_u64;
        let mut events = 0_usize;
        let mut batches = 0_usize;
        let mut civil_updates = 0_usize;
        while committed < ticks {
            let update = receiver.recv().await?;
            if let Some(failure) = update.failure {
                return Err(format!("actor 失败：{failure:?}").into());
            }
            match update.update.ok_or("actor 发布了没有 update 的消息")? {
                EngineUpdate::TickBatch(batch) => {
                    batches += 1;
                    for frame in &batch.frames {
                        if frame.tick != last_tick + 1 {
                            return Err(format!("tick 不连续：{last_tick} → {}", frame.tick).into());
                        }
                        last_tick = frame.tick;
                        committed += 1;
                        events += frame.events().len();
                    }
                }
                EngineUpdate::CivilUpdate(_) => civil_updates += 1,
            }
        }
        let wall_s = started.elapsed().as_secs_f64();
        let metrics = handles.speed_metrics().await?;
        handles.set_running(false).await?;
        handles.shutdown().await?;
        println!("{}", serde_json::json!({
            "path": "production_server_actor_fastest", "ticks": committed,
            "rayon_threads": actual_workers, "requested_rayon_threads": threads, "tokio_threads": 2,
            "wall_s": wall_s, "speed_x": committed as f64 / wall_s,
            "initialization_s": initialization_s, "events": events,
            "batches": batches, "civil_updates": civil_updates, "metrics": metrics,
        }));
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}
