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

fn complete_projection(auction_ticks: u64, seed: u64, ticks: u64) -> Vec<Vec<u8>> {
    // A single worker controls scheduling on the same production path. The
    // harness still runs the independent tests concurrently (ADR-0017).
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            let mut game = GameSession::new(setup(auction_ticks).unwrap(), seed).unwrap();
            (0..ticks)
                .map(|_| {
                    let events = game.step().expect("healthy step");
                    let save = game.save().expect("healthy save");
                    assert!(save.setup.config.price_cage_enabled);
                    assert!(save.resting_orders.values().all(Vec::is_empty));
                    assert!(save.auction_orders.values().all(Vec::is_empty));
                    assert!(!events
                        .iter()
                        .any(|event| matches!(event, Event::Trade { .. })));
                    serde_json::to_vec(&(events, game.snapshot(), save)).unwrap()
                })
                .collect()
        })
}

fn projection_digest(projection: &[Vec<u8>]) -> u64 {
    projection
        .iter()
        .flatten()
        .fold(0xcbf29ce484222325, |digest, byte| {
            (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

#[test]
fn characterization_complete_projections() {
    // Current-contract anchors after ADR-0021/0022, not historical equivalence
    // evidence. In particular the saved projection explicitly records the price
    // cage switch and every market's historical book cursor. Each anchor covers
    // one complete 20-tick day; the independent boundary test still covers three
    // days. This avoids serializing the same
    // company history 180 times in a short characterization. The scenario retains
    // its independent price, day-boundary, no-order and no-trade assertions;
    // the test below checks repeatability and distinguishes seed and tick-order
    // perturbations.
    // Old escrow-v2 digests: 54819b49083a6de9 / a22f8a5f963d6b66 / 626ae53ddcd6fa58.
    let actual = [0, 3, 6]
        .map(|auction_ticks| projection_digest(&complete_projection(auction_ticks, 42, 20)));
    assert_eq!(
        actual,
        [0x7e8ca11d72f312a9, 0x46bf286fb78cd84a, 0x169259aea14df5a0]
    );
}

#[test]
fn complete_projection_is_repeatable_and_distinguishes_seed_and_tick_order() {
    let projection = complete_projection(3, 42, 2);
    assert_eq!(projection, complete_projection(3, 42, 2));
    assert_ne!(
        projection_digest(&projection),
        projection_digest(&complete_projection(3, 43, 2)),
        "the complete save projection must retain seed-dependent state"
    );
    let mut reordered = projection.clone();
    reordered.swap(0, 1);
    assert_ne!(
        projection_digest(&projection),
        projection_digest(&reordered),
        "the digest must distinguish causal tick order"
    );
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
