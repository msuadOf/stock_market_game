// 生产 GameSession 直跑：不构造 ProtocolSession 和宿主帧。
use engine::{GameSession, SessionSetup};
use std::{fs, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !engine::runtime::step_runtime_enabled() { return Err("runtime 未启用".into()); }
    let path=std::env::args().nth(1).ok_or("缺少 setup")?;
    let setup: SessionSetup=serde_json::from_slice(&fs::read(path)?)?;
    let mut game=GameSession::new(setup,42)?;
    while game.day()==game.civil_clock().completed_trading_sessions_expected()? {game.end_civil_day()?;}
    let warm=Instant::now();
    for _ in 0..1000 {drop(game.step()?);}
    let warm_s=warm.elapsed().as_secs_f64();
    let start=Instant::now();
    let mut event_count=0usize;
    for _ in 0..3000 {event_count+=game.step()?.len();}
    let seconds=start.elapsed().as_secs_f64();
    println!("{}",serde_json::json!({"seed":42,"warmup_ticks":1000,"warmup_seconds":warm_s,"sample_from_tick":1000,"sample_to_tick":4000,"sample_ticks":3000,"sample_seconds":seconds,"ticks_per_second":3000.0/seconds,"events":event_count,"runtime":"native default, 1 group"}));
    Ok(())
}
