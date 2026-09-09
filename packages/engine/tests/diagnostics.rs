use engine::{
    run_price_volume_baseline, BaselineError, FloatAllocation, GameConfig, HotParams, InstParams,
    Money, NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange,
    StockSpec, StrategyParams, VParams,
};

fn diagnostic_setup() -> SessionSetup {
    let code = StockCode("600101".to_string());
    SessionSetup {
        stocks: vec![StockSpec {
            code: code.clone(),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            v_initial: Money::from_cents(1_000),
            tick: Money::from_cents(1),
            total_shares: 100_000,
            float_shares: 100_000,
        }],
        npcs: NpcSetup {
            retail_count: 4,
            inst_count: 2,
            hot_count: 2,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(1_000),
            mean_reversion: 0.1,
            volatility: 0.01,
        },
        fundamental_value_means: [(code, Money::from_cents(1_000))].into(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: 30,
        auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
    }
}

#[test]
fn baseline_rejects_empty_seeds_and_zero_days_explicitly() {
    let setup = diagnostic_setup();
    assert!(matches!(
        run_price_volume_baseline(&setup, &[], 2),
        Err(BaselineError::EmptySeeds)
    ));
    assert!(matches!(
        run_price_volume_baseline(&setup, &[1], 0),
        Err(BaselineError::ZeroTradingDays)
    ));

    let mut overflowing = setup;
    overflowing.ticks_per_day = u64::MAX;
    assert!(matches!(
        run_price_volume_baseline(&overflowing, &[1], 2),
        Err(BaselineError::TickCountOverflow { .. })
    ));
}

#[test]
fn baseline_is_deterministic_and_keeps_each_seed_visible() {
    let setup = diagnostic_setup();
    let first = run_price_volume_baseline(&setup, &[7, 11], 3).unwrap();
    let second = run_price_volume_baseline(&setup, &[7, 11], 3).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.trading_days, 3);
    assert_eq!(
        first.runs.iter().map(|run| run.seed).collect::<Vec<_>>(),
        vec![7, 11]
    );
}

#[test]
fn baseline_excludes_preset_history_and_reconciles_trade_volume_to_daily_candles() {
    let report = run_price_volume_baseline(&diagnostic_setup(), &[42], 4).unwrap();
    let run = &report.runs[0];
    let stock = &run.stocks[&StockCode("600101".to_string())];

    assert_eq!(stock.completed_days, 4);
    assert_eq!(stock.daily_returns_bps.len(), 4);
    assert_eq!(stock.trade_event_volume, stock.total_daily_volume);
    assert_eq!(
        stock.zero_volume_days + stock.traded_days,
        stock.completed_days
    );
    assert!(stock.max_zero_volume_streak <= stock.completed_days);
}

#[test]
fn baseline_json_serializes_large_seeds_without_precision_loss() {
    let mut report = run_price_volume_baseline(&diagnostic_setup(), &[u64::MAX], 1).unwrap();
    report.ticks_per_day = u64::MAX;
    report.runs[0].final_tick = u64::MAX;
    report.runs[0].trade_events = u64::MAX;
    report.runs[0].rejection_events = u64::MAX;
    report.runs[0].engine_error_events = u64::MAX;
    let stock = report.runs[0].stocks.values_mut().next().unwrap();
    stock.trade_event_volume = u64::MAX;
    stock.total_daily_volume = u64::MAX;
    let json = serde_json::to_string(&report).unwrap();

    assert!(json.contains(r#""seed":"18446744073709551615""#));
    for field in [
        "ticks_per_day",
        "final_tick",
        "trade_events",
        "rejection_events",
        "engine_error_events",
        "trade_event_volume",
        "total_daily_volume",
    ] {
        assert!(json.contains(&format!(r#""{field}":"18446744073709551615""#)));
    }
}

#[test]
fn baseline_reports_a_complete_zero_trade_run_without_nan_or_fake_activity() {
    let mut setup = diagnostic_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 0;

    let report = run_price_volume_baseline(&setup, &[9], 4).unwrap();
    let run = &report.runs[0];
    let stock = run.stocks.values().next().unwrap();

    assert_eq!(run.trade_events, 0);
    assert_eq!(stock.traded_days, 0);
    assert_eq!(stock.zero_volume_days, 4);
    assert_eq!(stock.max_zero_volume_streak, 4);
    assert_eq!(stock.total_daily_volume, 0);
    assert_eq!(stock.mean_daily_volume, 0.0);
    assert_eq!(stock.daily_returns_bps, vec![0.0; 4]);
    assert_eq!(stock.daily_return_stddev_bps, 0.0);
    assert_eq!(stock.absolute_return_lag1_correlation, None);
}
