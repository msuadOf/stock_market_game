use engine::{
    CivilDate, Event, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
};

fn setup(auction_ticks: u64) -> Result<SessionSetup, Box<dyn std::error::Error>> {
    Ok(SessionSetup {
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
                margin: 0.03,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 20,
        auction_ticks,
        closing_auction_ticks: 2,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-02")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V2.to_owned(),
    })
}

#[test]
fn characterization_ticks_preserve_price_and_day_boundaries(
) -> Result<(), Box<dyn std::error::Error>> {
    for auction_ticks in [0, 3, 6] {
        let mut game = GameSession::new(setup(auction_ticks)?, 42)?;
        let mut day_boundaries = 0;
        for expected_tick in 1..=60 {
            let events = game.step().expect("healthy step");
            day_boundaries += events
                .iter()
                .filter(|event| matches!(event, Event::DayBoundary { .. }))
                .count();
            let snapshot = game.snapshot();
            assert_eq!(snapshot.tick, expected_tick);
            assert_eq!(
                snapshot.markets[&StockCode("600001".to_owned())].last_price,
                Money::from_cents(1000)
            );
        }
        assert_eq!(day_boundaries, 3);
        assert_eq!(game.snapshot().day, 3);
    }
    Ok(())
}

#[test]
fn characterization_complete_projections() -> Result<(), Box<dyn std::error::Error>> {
    for (auction_ticks, expected) in [
        (0, 0xcae6f69876f7ffed),
        (3, 0xfbfb7fd00163dd72),
        (6, 0x99823bf26d2ccdcc),
    ] {
        let mut game = GameSession::new(setup(auction_ticks)?, 42)?;
        let mut digest = 0xcbf29ce484222325_u64;
        for _ in 0..60 {
            let events = game.step().expect("healthy step");
            let bytes =
                serde_json::to_vec(&(events, game.snapshot(), game.save().expect("healthy save")))?;
            for byte in bytes {
                digest = (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        println!("auction_ticks={auction_ticks} projection_digest={digest:016x}");
        assert_eq!(digest, expected);
    }
    Ok(())
}

#[test]
fn healthy_step_returns_ok_and_advances_business_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut game = GameSession::new(setup(0)?, 42)?;
    let before = game.business_state_hash()?;
    let events: Vec<Event> = game.step()?;
    assert!(!events.is_empty());
    assert_ne!(game.business_state_hash()?, before);
    assert!(game.poison_reason().is_none());
    assert!(game.save().is_ok());
    Ok(())
}
