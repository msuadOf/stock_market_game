use std::collections::BTreeMap;

use engine::{
    build_account_risk_observation, build_equal_weight_market_observation,
    build_market_minute_closes, build_price_path_observation, completed_market_minute_count,
    CompletedDayClose, MarketTickPrice, Money, RiskPositionInput, StockCode,
    GAME_INTRADAY_MINUTES_PER_DAY,
};

fn price(cents: i64) -> Money {
    Money::from_cents(cents)
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "actual={actual}, expected={expected}"
    );
}

#[test]
fn market_minute_closes_are_invariant_to_tick_sampling_density() {
    let one_tick_per_minute: Vec<_> = (0..GAME_INTRADAY_MINUTES_PER_DAY)
        .map(|minute| MarketTickPrice {
            continuous_tick: u64::from(minute),
            price: price(10_000 + i64::from(minute)),
        })
        .collect();
    let two_ticks_per_minute: Vec<_> = (0..GAME_INTRADAY_MINUTES_PER_DAY)
        .flat_map(|minute| {
            [
                MarketTickPrice {
                    continuous_tick: u64::from(minute) * 2,
                    price: price(9_999 + i64::from(minute)),
                },
                MarketTickPrice {
                    continuous_tick: u64::from(minute) * 2 + 1,
                    price: price(10_000 + i64::from(minute)),
                },
            ]
        })
        .collect();

    let coarse = build_market_minute_closes(
        7,
        u64::from(GAME_INTRADAY_MINUTES_PER_DAY),
        &one_tick_per_minute,
    )
    .unwrap();
    let fine = build_market_minute_closes(
        7,
        u64::from(GAME_INTRADAY_MINUTES_PER_DAY) * 2,
        &two_ticks_per_minute,
    )
    .unwrap();

    assert_eq!(coarse, fine);
    assert_eq!(coarse.len(), usize::from(GAME_INTRADAY_MINUTES_PER_DAY));
}

#[test]
fn completed_market_minute_count_scales_without_iterating_over_ticks() {
    assert_eq!(completed_market_minute_count(1, 10).unwrap(), 24);
    assert_eq!(completed_market_minute_count(1, 480).unwrap(), 0);
    assert_eq!(completed_market_minute_count(2, 480).unwrap(), 1);
    assert_eq!(completed_market_minute_count(480, 480).unwrap(), 240);
    assert!(completed_market_minute_count(0, 0).is_err());
    assert!(completed_market_minute_count(481, 480).is_err());
}

#[test]
fn slow_decline_short_crash_and_range_break_have_distinct_signals() {
    let slow_ticks: Vec<_> = (0_u64..=30)
        .map(|minute| MarketTickPrice {
            continuous_tick: minute,
            price: price(10_000 - i64::try_from(minute).unwrap() * 20),
        })
        .chain(std::iter::once(MarketTickPrice {
            continuous_tick: 31,
            price: price(9_400),
        }))
        .collect();
    let slow = build_market_minute_closes(0, 240, &slow_ticks).unwrap();
    let slow_signal = build_price_path_observation(&slow, &[], Some(price(10_000))).unwrap();
    assert_close(slow_signal.one_minute.return_ratio.unwrap(), 0.0);
    assert_close(
        slow_signal.thirty_minute.return_ratio.unwrap(),
        -580.0 / 9_980.0,
    );

    let crash_ticks: Vec<_> = (0_u64..=30)
        .map(|minute| MarketTickPrice {
            continuous_tick: minute,
            price: price(10_000),
        })
        .chain(std::iter::once(MarketTickPrice {
            continuous_tick: 31,
            price: price(9_400),
        }))
        .collect();
    let crash = build_market_minute_closes(0, 240, &crash_ticks).unwrap();
    let crash_signal = build_price_path_observation(&crash, &[], Some(price(10_000))).unwrap();
    assert_close(crash_signal.one_minute.return_ratio.unwrap(), -0.06);
    assert_close(crash_signal.thirty_minute.return_ratio.unwrap(), -0.06);

    let range_ticks: Vec<_> = (0_u64..=30)
        .map(|minute| MarketTickPrice {
            continuous_tick: minute,
            price: price(10_000),
        })
        .chain(std::iter::once(MarketTickPrice {
            continuous_tick: 31,
            price: price(9_500),
        }))
        .collect();
    let range = build_market_minute_closes(0, 240, &range_ticks).unwrap();
    let range_signal = build_price_path_observation(&range, &[], Some(price(10_000))).unwrap();
    assert_close(
        range_signal
            .prior_thirty_minute_range
            .as_ref()
            .unwrap()
            .distance_from_low,
        -0.05,
    );
    assert!(range_signal.prior_thirty_minute_range.unwrap().broke_below);
}

#[test]
fn insufficient_minute_and_daily_history_remains_explicitly_unavailable() {
    let minute_closes = build_market_minute_closes(
        3,
        240,
        &[
            MarketTickPrice {
                continuous_tick: 0,
                price: price(1_000),
            },
            MarketTickPrice {
                continuous_tick: 1,
                price: price(1_010),
            },
        ],
    )
    .unwrap();
    let completed_days = vec![CompletedDayClose {
        trading_day: 2,
        close: price(990),
    }];

    let observation =
        build_price_path_observation(&minute_closes, &completed_days, Some(price(1_000))).unwrap();
    assert_eq!(observation.one_minute.available_span, 1);
    assert!(observation.one_minute.return_ratio.is_some());
    assert_eq!(observation.thirty_minute.available_span, 1);
    assert!(observation.thirty_minute.return_ratio.is_none());
    assert_eq!(observation.five_day.available_span, 1);
    assert!(observation.five_day.return_ratio.is_none());
    assert!(observation.prior_thirty_minute_range.is_none());
}

#[test]
fn observations_reject_ambiguous_or_invalid_time_series() {
    let duplicate = [
        MarketTickPrice {
            continuous_tick: 2,
            price: price(1_000),
        },
        MarketTickPrice {
            continuous_tick: 2,
            price: price(1_001),
        },
    ];
    assert!(build_market_minute_closes(0, 240, &duplicate).is_err());
    assert!(build_market_minute_closes(0, 0, &[]).is_err());
    assert!(build_market_minute_closes(
        0,
        240,
        &[MarketTickPrice {
            continuous_tick: 240,
            price: price(1_000),
        }],
    )
    .is_err());
    assert!(build_market_minute_closes(
        0,
        240,
        &[MarketTickPrice {
            continuous_tick: 0,
            price: Money::ZERO,
        }],
    )
    .is_err());

    let minute = engine::MarketMinuteClose {
        absolute_trading_minute: 5 * u64::from(GAME_INTRADAY_MINUTES_PER_DAY),
        close: price(1_000),
    };
    let gapped_days = [
        CompletedDayClose {
            trading_day: 0,
            close: price(900),
        },
        CompletedDayClose {
            trading_day: 2,
            close: price(950),
        },
    ];
    assert!(build_price_path_observation(&[minute], &gapped_days, Some(price(1_000))).is_err());
    assert!(build_market_minute_closes(
        0,
        240,
        &[MarketTickPrice {
            continuous_tick: 1,
            price: price(1_000),
        }],
    )
    .is_err());
}

#[test]
fn intraday_return_includes_the_auction_or_opening_gap_from_authoritative_open() {
    let minute = engine::MarketMinuteClose {
        absolute_trading_minute: 0,
        close: price(1_100),
    };
    let observation = build_price_path_observation(&[minute], &[], Some(price(1_000))).unwrap();

    assert_eq!(observation.intraday.requested_span, 1);
    assert_eq!(observation.intraday.available_span, 1);
    assert_close(observation.intraday.return_ratio.unwrap(), 0.10);

    let missing_open = build_price_path_observation(&[minute], &[], None).unwrap();
    assert!(missing_open.intraday.return_ratio.is_none());
    assert!(build_price_path_observation(&[minute], &[], Some(Money::ZERO)).is_err());
}

#[test]
fn equal_weight_market_observation_reports_breadth_and_missing_coverage() {
    let returns = BTreeMap::from([
        (StockCode("000001".into()), Some(0.02)),
        (StockCode("000002".into()), Some(-0.01)),
        (StockCode("000003".into()), Some(0.0)),
        (StockCode("000004".into()), None),
    ]);
    let market = build_equal_weight_market_observation(&returns).unwrap();

    assert_eq!(market.total_stock_count, 4);
    assert_eq!(market.observed_stock_count, 3);
    assert_close(market.equal_weight_return.unwrap(), 0.01 / 3.0);
    assert_close(market.advance_fraction.unwrap(), 1.0 / 3.0);
    assert_close(market.decline_fraction.unwrap(), 1.0 / 3.0);
    assert_close(market.unchanged_fraction.unwrap(), 1.0 / 3.0);

    let unavailable = build_equal_weight_market_observation(&BTreeMap::from([(
        StockCode("000001".into()),
        None,
    )]))
    .unwrap();
    assert_eq!(unavailable.observed_stock_count, 0);
    assert!(unavailable.equal_weight_return.is_none());
}

#[test]
fn equal_weight_market_observation_rejects_non_finite_inputs_and_sum() {
    assert!(build_equal_weight_market_observation(&BTreeMap::from([(
        StockCode("000001".into()),
        Some(f64::NAN),
    )]))
    .is_err());
    assert!(build_equal_weight_market_observation(&BTreeMap::from([
        (StockCode("000001".into()), Some(f64::MAX)),
        (StockCode("000002".into()), Some(f64::MAX)),
    ]))
    .is_err());
}

#[test]
fn account_risk_uses_each_accounts_cost_weight_and_explicit_references() {
    let positions = BTreeMap::from([
        (
            StockCode("000001".into()),
            RiskPositionInput {
                qty: 1_000,
                cost_price: Some(price(1_000)),
                last_price: price(800),
                peak_price_since_entry: Some(price(1_200)),
            },
        ),
        (
            StockCode("000002".into()),
            RiskPositionInput {
                qty: 100,
                cost_price: Some(price(600)),
                last_price: price(800),
                peak_price_since_entry: None,
            },
        ),
    ]);
    let risk = build_account_risk_observation(
        price(200_000),
        &positions,
        Some(price(1_100_000)),
        Some(price(1_200_000)),
    )
    .unwrap();

    assert_eq!(risk.equity, price(1_080_000));
    assert_close(risk.return_from_reference.unwrap(), -0.01818181818181818);
    assert_close(risk.drawdown_from_peak.unwrap(), -0.10);

    let loss = &risk.positions[&StockCode("000001".into())];
    assert_close(loss.unrealized_return.unwrap(), -0.20);
    assert_close(loss.equity_weight.unwrap(), 800_000.0 / 1_080_000.0);
    assert_close(loss.drawdown_from_position_peak.unwrap(), -1.0 / 3.0);

    let profit = &risk.positions[&StockCode("000002".into())];
    assert_close(profit.unrealized_return.unwrap(), 1.0 / 3.0);
    assert!(profit.drawdown_from_position_peak.is_none());
}

#[test]
fn account_risk_rejects_invalid_prices_costs_and_references() {
    let code = StockCode("000001".into());
    let invalid_cost = BTreeMap::from([(
        code.clone(),
        RiskPositionInput {
            qty: 100,
            cost_price: Some(Money::ZERO),
            last_price: price(800),
            peak_price_since_entry: None,
        },
    )]);
    assert!(build_account_risk_observation(Money::ZERO, &invalid_cost, None, None).is_err());

    let invalid_peak = BTreeMap::from([(
        code,
        RiskPositionInput {
            qty: 100,
            cost_price: Some(price(700)),
            last_price: price(800),
            peak_price_since_entry: Some(price(750)),
        },
    )]);
    assert!(build_account_risk_observation(Money::ZERO, &invalid_peak, None, None).is_err());
    assert!(
        build_account_risk_observation(Money::ZERO, &BTreeMap::new(), Some(Money::ZERO), None)
            .is_err()
    );
}
