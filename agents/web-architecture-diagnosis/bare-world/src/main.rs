// 生产 GameSession 直跑：不构造 ProtocolSession 和宿主帧。
use engine::{GameSession, SessionSetup};
use std::{fs, time::Instant};
#[path = "../../../libos-integration/world-benchmark/trace.rs"]
mod trace;
#[path = "../../../libos-integration/world-benchmark/math_candidate.rs"]
mod math_candidate;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !engine::runtime::step_runtime_enabled() { return Err("runtime 未启用".into()); }
    let path=std::env::args().nth(1).ok_or("缺少 setup")?;
    let setup: SessionSetup=serde_json::from_slice(&fs::read(path)?)?;
    let math_mode = match std::env::var("LIBOS_DIAGNOSTIC_MATH") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "std".to_owned(),
        Err(error) => return Err(error.into()),
    };
    if math_mode != "std" && math_mode != "libm" { return Err("未知观察概率数学模式".into()); }
    let (game, observations)=engine::strategy::verification_trace_observations(|| {
        if math_mode == "libm" { engine::strategy::verification_with_observation_math(|argument| -libm::expm1(argument), || GameSession::new(setup,42)) }
        else { GameSession::new(setup,42) }
    });
    let mut game=game?;
    while game.day()==game.civil_clock().completed_trading_sessions_expected()? {game.end_civil_day()?;}
    let warm=Instant::now();
    let warm_from=game.tick();
    let mut warm_events=0usize;
    for _ in 0..1000 {warm_events+=game.step()?.len();}
    let warm_s=warm.elapsed().as_secs_f64();
    let count: u64=std::env::args().nth(2).ok_or("缺少采样 tick 数")?.parse()?;
    let start_tick=game.tick();
    let profiling = match std::env::var("LIBOS_DIAGNOSTIC_PROFILE") {
        Ok(value) if value == "1" => true,
        Ok(value) if value == "0" => false,
        Err(std::env::VarError::NotPresent) => false,
        Ok(_) => return Err("画像参数必须为 0 或 1".into()),
        Err(error) => return Err(error.into()),
    };
    if profiling { engine::runtime::step_profile::enable_on_this_thread(); }
    let start=Instant::now();
    let mut event_count=0usize;
    for _ in 0..count {event_count+=game.step()?.len();}
    let seconds=start.elapsed().as_secs_f64();
    let step_profile = if profiling {
        Some(engine::runtime::step_profile::take_ledger().ok_or("step 画像账本缺失")?.into_iter()
            .map(|(name, ns, count)| (name, ns.to_string(), count)).collect::<Vec<_>>())
    } else { None };
    let end_tick=game.tick();
    let business_hash=serde_json::to_string(&game.business_state_hash()?)?;
    let (trace_hash, fields)=game.verification_business_hash_trace()?;
    if serde_json::to_string(&trace_hash)? != business_hash { return Err("追踪摘要与业务摘要不一致".into()); }
    let field_trace=trace::summarize(&fields);
    let field_samples=[trace::sample(&fields, 623)?, trace::sample(&fields, 624)?];
    let observation_trace: Vec<_> = observations.iter().map(|value| [value.account_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_owned()), format!("{:016x}", value.observations_bits), value.ticks_per_day.to_string(), format!("{:016x}", value.argument_bits), format!("{:016x}", value.probability_bits), value.canonical_bits.map(|bits| format!("{bits:016x}")).unwrap_or_else(|| "none".to_owned())]).collect();
    let candidate_trace: Vec<_> = observations.iter().map(|value| {
        let (raw, canonical) = math_candidate::evaluate(f64::from_bits(value.argument_bits))?;
        Ok::<_, serde_json::Error>([value.account_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_owned()), format!("{:016x}", value.argument_bits), format!("{raw:016x}"), format!("{canonical:016x}")])
    }).collect::<Result<_, _>>()?;
    println!("{}",serde_json::json!({"mathMode":math_mode,"stepProfile":step_profile,"seed":42,"warm":{"from":warm_from,"to":start_tick,"events":warm_events},"warmMs":warm_s*1000.0,"sample":{"from":start_tick,"to":end_tick,"events":event_count},"elapsedMs":seconds*1000.0,"businessHash":business_hash,"fieldTrace":field_trace,"fieldSamples":field_samples,"accountRoster":game.account_roster(),"observationTrace":observation_trace,"candidateTrace":candidate_trace,"runtime":"native default, 1 group"}));
    Ok(())
}
