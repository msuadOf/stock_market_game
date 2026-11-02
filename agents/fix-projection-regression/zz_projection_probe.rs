use engine::{
    CivilDate, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money, NpcSetup,
    RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
};

fn setup(auction_ticks: u64) -> Result<SessionSetup, Box<dyn std::error::Error>> {
    Ok(SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600001".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup { retail_count: 0, inst_count: 0, hot_count: 0, retail_cash_median: Money::ZERO },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams { arrival_rate: 0.0, order_size_mean: 100, chase_prob: 0.0 },
            inst: InstParams { margin: 0.03, order_size: 100 },
            hot: HotParams { lookback: 2, trend_threshold: 0.01, order_size: 100 },
        },
        ticks_per_day: 20,
        auction_ticks,
        closing_auction_ticks: 2,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso("2030-01-02")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        runtime_sync_policy: Default::default(),
    })
}

#[test]
fn dump_projections() {
    let out = std::env::var("PROBE_OUT").expect("PROBE_OUT");
    for auction_ticks in [0u64, 3, 6] {
        let mut game = GameSession::new(setup(auction_ticks).unwrap(), 42).unwrap();
        for tick in 1..=20u64 {
            let events = game.step().expect("healthy step");
            let save = game.save().expect("healthy save");
            let snapshot = game.snapshot();
            let dir = format!("{out}/a{auction_ticks}/t{tick:02}");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                format!("{dir}/events.json"),
                serde_json::to_vec_pretty(&events).unwrap(),
            ).unwrap();
            std::fs::write(
                format!("{dir}/snapshot.json"),
                serde_json::to_vec_pretty(&snapshot).unwrap(),
            ).unwrap();
            std::fs::write(
                format!("{dir}/save.json"),
                serde_json::to_vec_pretty(&save).unwrap(),
            ).unwrap();
        }
    }
}

#[test]
fn dump_compact() {
    let out = std::env::var("PROBE_OUT").expect("PROBE_OUT");
    for auction_ticks in [0u64, 3, 6] {
        let mut game = GameSession::new(setup(auction_ticks).unwrap(), 42).unwrap();
        for tick in 1..=20u64 {
            let events = game.step().expect("healthy step");
            let save = game.save().expect("healthy save");
            let snapshot = game.snapshot();
            let bytes = serde_json::to_vec(&(events, snapshot, save)).unwrap();
            let digest = bytes.iter().fold(0xcbf29ce484222325u64, |d, b| {
                (d ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            });
            let dir = format!("{out}/a{auction_ticks}");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/t{tick:02}.bin"), bytes).unwrap();
            println!("a{auction_ticks} t{tick:02} {digest}");
        }
    }
}
