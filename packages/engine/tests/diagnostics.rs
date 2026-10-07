include!("../test-support/simple_company.rs");

use engine::{
    run_combined_diagnostics, run_price_volume_baseline, BaselineError, FloatAllocation,
    GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams, SecurityCategory,
    SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};

fn diagnostic_setup() -> SessionSetup {
    let code = StockCode("600101".to_string());
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600101"]),
        stocks: vec![StockSpec {
            code: code.clone(),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
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
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
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
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::random(),
        start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
        flat_withholding_bp: Some(1000),
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    }
}

#[test]
fn combined_diagnostics_pairs_reports_from_the_same_trade_receipts() {
    let seeds = [7, 11, 19, 23, 31];
    let report = run_combined_diagnostics(&diagnostic_setup(), &seeds, 20).unwrap();
    let run = report
        .causal_runs
        .iter()
        .find(|run| run.source.trade_receipt_count > 0)
        .expect("at least one real seeded market run must produce a Trade receipt");
    let price_volume_run = report
        .price_volume
        .runs
        .iter()
        .find(|price_volume_run| price_volume_run.run_id == run.run_id)
        .expect("the paired price-volume report must use the same run identity");
    let stock = &price_volume_run.stocks[&StockCode("600101".to_string())];

    assert!(
        run.source.trade_receipt_count > 0,
        "fixture must produce an actual Trade receipt"
    );
    assert!(
        run.source.trade_receipt_shares > 0,
        "actual Trade receipts must carry shares"
    );
    assert!(
        run.source.trade_receipt_turnover_cents > 0,
        "actual Trade receipts must carry turnover"
    );
    assert_eq!(run.run_id, price_volume_run.run_id);
    assert_eq!(run.seed, price_volume_run.seed);
    assert_eq!(
        run.source.trade_receipt_count,
        price_volume_run.trade_events
    );
    assert_eq!(run.source.trade_receipt_shares, stock.trade_event_volume);
    assert_eq!(
        run.source.trade_receipt_turnover_cents,
        stock.trade_event_turnover_cents
    );
    assert_eq!(
        run.source.causal_execution_count,
        run.source.trade_receipt_count
    );
    assert_eq!(
        run.source.causal_execution_shares,
        run.source.trade_receipt_shares
    );
    assert_eq!(
        run.source.causal_execution_turnover_cents,
        run.source.trade_receipt_turnover_cents
    );
    assert_eq!(run.source.collection_mode, "shared_session");
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
    assert!(matches!(
        run_price_volume_baseline(&setup, &[7, 7], 2),
        Err(BaselineError::DuplicateSeed(7))
    ));

    let mut overflowing = setup;
    overflowing.ticks_per_day = u64::MAX;
    assert!(matches!(
        run_price_volume_baseline(&overflowing, &[1], 2),
        Err(BaselineError::TickCountOverflow { .. })
    ));
}

#[test]
fn baseline_keeps_each_seed_visible_and_summarizes_observed_runs() {
    let setup = diagnostic_setup();
    let first = run_price_volume_baseline(&setup, &[7, 11], 3).unwrap();
    let second = run_price_volume_baseline(&setup, &[7, 11], 3).unwrap();

    // 自由并发受理可产生不同成交；逐次核对实际报告，不以 seed 强制整局相等。
    for report in [first, second] {
        assert_eq!(report.trading_days, 3);
        assert_eq!(
            report.runs.iter().map(|run| run.seed).collect::<Vec<_>>(),
            vec![7, 11]
        );
        let aggregate = &report.stocks[&StockCode("600101".to_string())];
        assert_eq!(aggregate.seed_count, 2);
        assert_eq!(aggregate.mean_daily_volume.sample_count, 2);
        assert!(aggregate.mean_daily_volume.minimum <= aggregate.mean_daily_volume.median);
        assert!(aggregate.mean_daily_volume.median <= aggregate.mean_daily_volume.maximum);
        assert!(aggregate.mean_daily_volume.mean_ci95_low <= aggregate.mean_daily_volume.mean);
        assert!(aggregate.mean_daily_volume.mean <= aggregate.mean_daily_volume.mean_ci95_high);
        assert_eq!(aggregate.longest_continuous_no_trade_ticks.sample_count, 2);
        assert!(aggregate
            .auction_volume_share
            .as_ref()
            .is_none_or(|summary| summary.sample_count <= 2));
        assert!(aggregate
            .mean_quoted_spread_bps
            .as_ref()
            .is_none_or(|summary| summary.sample_count <= 2));
        assert!(aggregate
            .return_excess_kurtosis
            .as_ref()
            .is_none_or(|summary| summary.sample_count <= 2));
        assert!(report
            .extreme_cases
            .iter()
            .all(|case| [7, 11].contains(&case.seed)));
    }
}

#[test]
fn behavior_loop_runs_through_the_real_order_book_across_multiple_seeds() {
    let seeds = [7, 11, 19, 23, 31];
    let report = run_price_volume_baseline(&diagnostic_setup(), &seeds, 20).unwrap();

    // 并发受理顺序不由 seed 固定；实际报告必须满足真实成交与数量守恒。
    assert!(
        report
            .runs
            .iter()
            .all(|run| run.retail_behavior.observed_decisions > 0),
        "散户行为诊断报告必须保留每个 seed 实际产生的散户目标仓位决策"
    );
    assert!(
        report.runs.iter().all(|run| {
            run.retail_behavior.desired_buy_shares >= run.retail_behavior.executable_buy_shares
                && run.retail_behavior.desired_sell_shares
                    >= run.retail_behavior.executable_sell_shares
        }),
        "diagnostics must distinguish complete desired target changes from the executable T+1-limited part"
    );
    assert!(
        report
            .runs
            .iter()
            .all(|run| run.retail_execution.submitted_orders > 0),
        "散户行为诊断报告必须分别保留真实散户订单与目标仓位"
    );
    assert!(report.runs.iter().all(|run| {
        run.retail_execution.filled_shares
            + run.retail_execution.canceled_shares
            + run.retail_execution.aborted_shares
            + run.retail_execution.open_shares
            == run.retail_execution.submitted_shares
    }));
    assert!(report.runs.iter().all(|run| {
        run.retail_execution.rejected_intents
            == run
                .retail_execution
                .rejection_reason_counts
                .values()
                .sum::<u64>()
    }));
    assert!(report.runs.iter().all(|run| {
        run.retail_execution
            .filled_share_ratio
            .is_some_and(f64::is_finite)
    }));
    assert!(
        report
            .runs
            .iter()
            .any(|run| run.retail_execution.filled_shares > 0),
        "the cross-tick collector must receive actual retail fills, not only submissions"
    );
    let total_volumes: std::collections::BTreeSet<u64> = report
        .runs
        .iter()
        .map(|run| {
            assert_eq!(run.engine_error_events, 0);
            let stock = &run.stocks[&StockCode("600101".to_string())];
            assert_eq!(stock.trade_event_volume, stock.total_daily_volume);
            stock.total_daily_volume
        })
        .collect();
    assert!(
        total_volumes.iter().any(|volume| *volume > 0),
        "the behavior loop must reach real matching rather than only producing plans"
    );
    assert!(
        total_volumes.len() > 1,
        "independent seeded participants should not collapse every seed to one scripted path"
    );
    assert!(report.runs.iter().all(|run| {
        run.participant_execution.two_sided_participant_shares
            == run
                .participant_execution
                .two_sided_participant_shares_by_profile
                .values()
                .sum::<u64>()
    }));
    assert!(
        report.runs.iter().all(|run| {
            run.participant_execution.two_sided_participant_shares
                == run
                    .stocks
                    .values()
                    .map(|stock| stock.trade_event_volume)
                    .sum::<u64>()
                    .saturating_mul(2)
        }),
        "every recorded trade has two explicitly classified participants"
    );
    assert!(
        report.runs.iter().any(|run| {
            run.participant_execution
                .two_sided_participant_shares_by_profile
                .keys()
                .any(|profile| profile.starts_with("retail_"))
        }),
        "the report must distinguish retail styles rather than aggregating all NPC activity"
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
        stock.trade_event_turnover_cents,
        stock.total_daily_turnover_cents
    );
    assert_eq!(
        stock.zero_volume_days + stock.traded_days,
        stock.completed_days
    );
    assert!(stock.max_zero_volume_streak <= stock.completed_days);
    assert!(stock
        .mean_daily_turnover_rate_bps
        .is_some_and(|value| value >= 0.0));
    assert!(stock.maximum_drawdown_bps >= 0.0);
    assert!(stock.return_excess_kurtosis.is_none_or(f64::is_finite));
    assert!(stock
        .volume_absolute_return_correlation
        .is_none_or(f64::is_finite));
    assert_eq!(
        stock.auction_volume + stock.continuous_volume,
        stock.total_daily_volume
    );
    assert_eq!(
        stock.opening_auction_volume + stock.closing_auction_volume,
        stock.auction_volume
    );
    assert_eq!(
        stock.continuous_volume_by_decile.iter().sum::<u64>(),
        stock.continuous_volume
    );
    assert!(stock
        .continuous_volume_share_by_decile
        .is_none_or(|shares| shares.len() == 10));
    assert!(
        stock.longest_continuous_no_trade_ticks
            <= report.ticks_per_day * u64::from(report.trading_days)
    );
    assert!(stock.mean_quoted_spread_bps.is_none_or(f64::is_finite));
    assert!(stock.mean_top_five_depth_shares.is_none_or(f64::is_finite));
    assert!(stock
        .mean_absolute_top_five_imbalance
        .is_none_or(f64::is_finite));
    assert!(stock
        .all_cancellations_to_accept_ratio
        .is_none_or(f64::is_finite));
}

#[test]
fn baseline_json_serializes_large_seeds_without_precision_loss() {
    let mut report = run_price_volume_baseline(&diagnostic_setup(), &[u64::MAX], 1).unwrap();
    report.ticks_per_day = u64::MAX;
    report.runs[0].final_tick = u64::MAX;
    report.runs[0].trade_events = u64::MAX;
    report.runs[0].rejection_events = u64::MAX;
    report.runs[0].engine_error_events = u64::MAX;
    report.runs[0]
        .participant_execution
        .two_sided_participant_shares = u64::MAX;
    report.runs[0]
        .participant_execution
        .two_sided_participant_shares_by_profile
        .insert("retail_noise".to_string(), u64::MAX);
    report.runs[0]
        .retail_behavior
        .action_counts
        .insert("hold", u64::MAX);
    report.runs[0]
        .retail_behavior
        .reason_counts
        .insert("no_signal", u64::MAX);
    let stock = report.runs[0].stocks.values_mut().next().unwrap();
    stock.trade_event_volume = u64::MAX;
    stock.total_daily_volume = u64::MAX;
    stock.trade_event_turnover_cents = u64::MAX;
    stock.total_daily_turnover_cents = u64::MAX;
    stock.auction_volume = u64::MAX;
    stock.continuous_volume = u64::MAX;
    stock.continuous_volume_by_decile = [u64::MAX; 10];
    stock.longest_continuous_no_trade_ticks = u64::MAX;
    stock.resting_order_acceptances = u64::MAX;
    stock.all_cancellations_including_day_expiry = u64::MAX;
    let json = serde_json::to_string(&report).unwrap();

    assert!(json.contains(r#""seed":"18446744073709551615""#));
    for field in [
        "ticks_per_day",
        "final_tick",
        "trade_events",
        "rejection_events",
        "engine_error_events",
        "two_sided_participant_shares",
        "trade_event_volume",
        "total_daily_volume",
        "trade_event_turnover_cents",
        "total_daily_turnover_cents",
        "auction_volume",
        "continuous_volume",
        "longest_continuous_no_trade_ticks",
        "resting_order_acceptances",
        "all_cancellations_including_day_expiry",
    ] {
        assert!(json.contains(&format!(r#""{field}":"18446744073709551615""#)));
    }
    assert!(json.contains(
        r#""continuous_volume_by_decile":["18446744073709551615","18446744073709551615""#
    ));
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let behavior = &value["runs"][0]["retail_behavior"];
    assert_eq!(
        behavior["action_counts"]["hold"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(
        behavior["reason_counts"]["no_signal"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(
        value["runs"][0]["participant_execution"]["two_sided_participant_shares_by_profile"]
            ["retail_noise"],
        serde_json::Value::String(u64::MAX.to_string())
    );
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
    assert_eq!(report.runs[0].retail_behavior.observed_decisions, 0);
    assert!(report.runs[0].retail_behavior.action_counts.is_empty());
    assert!(report.runs[0].retail_behavior.reason_counts.is_empty());
    assert_eq!(report.runs[0].retail_execution.filled_share_ratio, None);
    assert_eq!(
        report.runs[0]
            .participant_execution
            .two_sided_participant_shares,
        0
    );
    assert!(report.runs[0]
        .participant_execution
        .two_sided_participant_shares_by_profile
        .is_empty());
    assert_eq!(stock.mean_daily_volume, 0.0);
    assert_eq!(stock.daily_returns_bps, vec![0.0; 4]);
    assert_eq!(stock.daily_return_stddev_bps, 0.0);
    assert_eq!(stock.absolute_return_lag1_correlation, None);
}

#[test]
fn continuous_no_trade_streak_excludes_auction_and_preopen_windows() {
    let mut setup = diagnostic_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 0;
    setup.auction_ticks = 9;

    let report = run_price_volume_baseline(&setup, &[9], 2).unwrap();
    let stock = report.runs[0].stocks.values().next().unwrap();

    assert_eq!(stock.longest_continuous_no_trade_ticks, 21);
}

#[test]
fn continuous_no_trade_streak_resets_at_day_boundary_without_an_auction() {
    let mut setup = diagnostic_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 0;
    setup.auction_ticks = 0;

    let report = run_price_volume_baseline(&setup, &[9], 2).unwrap();
    let stock = report.runs[0].stocks.values().next().unwrap();

    assert_eq!(stock.longest_continuous_no_trade_ticks, 30);
}
