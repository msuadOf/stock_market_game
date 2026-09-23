use engine::session::protocol::*;
use engine::*;

fn setup() -> Result<SessionSetup, Box<dyn std::error::Error>> {
    Ok(SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600001".into()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 100,
            },
        },
        ticks_per_day: 20,
        auction_ticks: 6,
        closing_auction_ticks: 2,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-02")?,
        simulation_policy_id: SIMULATION_POLICY_ID_V1.into(),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = ProtocolSession::new(setup()?, 41)?;
    let mut guard = ReplayGuard::new(0, 0);
    let mut frames = Vec::new();
    for _ in 0..20 {
        frames.push(session.step_frame()?);
    }
    println!(
        "tick batch: tick={} seq={}",
        session.game().tick(),
        session.game().seq()
    );
    let batch = EngineUpdate::TickBatch(TickBatch {
        frames,
        runtime_snapshot: None,
    });
    guard.ingest(&batch)?;
    let civil = session.end_civil_day_update()?;
    println!(
        "civil: tick={} seq=({},{}] kinds={:?}",
        civil.tick, civil.seq_from, civil.seq_to, civil.kinds
    );
    let barrier = EngineUpdate::CivilUpdate(Box::new(civil.clone()));
    guard.ingest(&barrier)?;
    let next = session.step_frame()?;
    println!(
        "next tick: tick={} seq=({},{}]",
        next.tick, next.seq_from, next.seq_to
    );
    guard.ingest(&EngineUpdate::TickBatch(TickBatch {
        frames: vec![next],
        runtime_snapshot: None,
    }))?;
    let retry = guard.ingest(&barrier)?;
    println!("exact civil retry: {retry:?}");
    if retry != ReplayDecision::ExactRetry {
        return Err("retry did not deduplicate".into());
    }
    let mut changed = civil;
    changed.refresh.securities[0].total_shares += 100;
    changed.validate()?;
    let rejection = guard.ingest(&EngineUpdate::CivilUpdate(Box::new(changed)));
    println!("coherent civil mutation: {rejection:?}");
    if rejection != Err(ProtocolError::ReplayMismatch) {
        return Err("mutation was not rejected".into());
    }
    let code = StockCode("600001".into());
    let mut events: Vec<_> = (1..=2)
        .map(|seq| Event::AuctionTick {
            seq,
            tick: 1,
            code: code.clone(),
            phase: TradingPhase::CallAuction,
            indicative_price: Some(Money::from_cents(1000)),
            matched_volume: seq * 100,
            imbalance: 100,
        })
        .collect();
    events.push(Event::AuctionCompleted {
        seq: 3,
        tick: 1,
        code: code.clone(),
        phase: TradingPhase::CallAuction,
        clearing_price: Some(Money::from_cents(1000)),
        matched_volume: 200,
    });
    let mut facts = attach_facts(&events)?;
    facts.reverse();
    let projection = project_timeseries(&facts, session.game().runtime_snapshot());
    let points = projection
        .auction_points
        .get(&code)
        .ok_or("missing auction points")?;
    println!("multi-auction point count: {}", points.len());
    if points.len() != 3 {
        return Err("auction points were lost".into());
    }
    Ok(())
}
