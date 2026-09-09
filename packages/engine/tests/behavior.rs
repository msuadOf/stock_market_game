use std::collections::BTreeMap;

use engine::{
    decide_retail_position, decide_retail_position_with_experience, AccountRiskObservation,
    BehaviorMarketObservation, DecisionReason, EqualWeightMarketObservation, HorizonReturn,
    MarketView, Money, PositionAction, PositionRiskObservation, PositionView, PricePathObservation,
    RetailExperienceState, RetailStyle, Rng, SelfView, StockCode, StockView, Strategy,
    StrategyData, ZiNoiseStrategy,
};

struct FixedRng {
    value: f64,
    index: u32,
}

impl Rng for FixedRng {
    fn next_f64(&mut self) -> f64 {
        self.value
    }

    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            lo
        } else {
            lo + self.index.min(hi - lo - 1)
        }
    }
}

fn horizon(span: u16, value: Option<f64>) -> HorizonReturn {
    HorizonReturn {
        requested_span: span,
        available_span: value.map_or(0, |_| span),
        return_ratio: value,
    }
}

fn path(thirty_minute: Option<f64>, five_day: Option<f64>) -> PricePathObservation {
    PricePathObservation {
        one_minute: horizon(1, Some(0.0)),
        thirty_minute: horizon(30, thirty_minute),
        intraday: horizon(120, thirty_minute),
        five_day: horizon(5, five_day),
        twenty_day: horizon(20, None),
        one_hundred_twenty_day: horizon(120, None),
        two_hundred_fifty_day: horizon(250, None),
        prior_thirty_minute_range: None,
    }
}

fn market_and_observations(
    paths: impl IntoIterator<Item = (StockCode, PricePathObservation)>,
    decline_fraction: f64,
) -> (MarketView, BehaviorMarketObservation) {
    let price_paths: BTreeMap<_, _> = paths.into_iter().collect();
    let stocks = price_paths
        .keys()
        .map(|code| {
            (
                code.clone(),
                StockView {
                    best_bid: Some(Money::from_cents(999)),
                    best_ask: Some(Money::from_cents(1_001)),
                    last_price: Money::from_cents(1_000),
                    fundamental_value: None,
                    // 故意保持横盘：B02 不得再用 tick 数冒充 30 分钟。
                    recent_prices: vec![Money::from_cents(1_000); 20],
                    recent_market_minute_prices: vec![],
                    relative_volume: 1.0,
                    order_book_imbalance: 0.0,
                },
            )
        })
        .collect();
    (
        MarketView {
            stocks,
            tick: 500,
            market_minute: 0,
        },
        BehaviorMarketObservation {
            price_paths,
            thirty_minute_market: EqualWeightMarketObservation {
                total_stock_count: 10,
                observed_stock_count: 10,
                equal_weight_return: Some(-0.03 * decline_fraction),
                advance_fraction: Some(1.0 - decline_fraction),
                decline_fraction: Some(decline_fraction),
                unchanged_fraction: Some(0.0),
            },
        },
    )
}

fn own_and_risk(
    code: &StockCode,
    qty: u32,
    sellable_qty: u32,
    pnl: f64,
    equity_weight: f64,
) -> (SelfView, AccountRiskObservation) {
    let own = SelfView {
        cash: Money::from_cents(10_000_000),
        positions: [(
            code.clone(),
            PositionView {
                qty,
                sellable_qty,
                cost_price: Some(Money::from_cents((1_000.0 / (1.0 + pnl)).round() as i64)),
            },
        )]
        .into(),
    };
    let risk = AccountRiskObservation {
        equity: Money::from_cents(10_000_000),
        return_from_reference: None,
        drawdown_from_peak: None,
        positions: [(
            code.clone(),
            PositionRiskObservation {
                market_value: Money::from_cents(i64::from(qty) * 1_000),
                unrealized_return: Some(pnl),
                equity_weight: Some(equity_weight),
                drawdown_from_position_peak: None,
            },
        )]
        .into(),
    };
    (own, risk)
}

fn no_position() -> (SelfView, AccountRiskObservation) {
    (
        SelfView {
            cash: Money::from_cents(10_000_000),
            positions: BTreeMap::new(),
        },
        AccountRiskObservation {
            equity: Money::from_cents(10_000_000),
            return_from_reference: None,
            drawdown_from_peak: None,
            positions: BTreeMap::new(),
        },
    )
}

fn strategy() -> StrategyData {
    let mut strategy = StrategyData::retail(1.0, 500, 0.5, 1);
    strategy.stop_loss_threshold = 0.05;
    strategy.take_profit_threshold = 0.08;
    strategy.dip_threshold = 0.02;
    strategy.max_stock_fraction = 0.40;
    strategy
}

#[test]
fn position_risk_is_evaluated_after_a_deep_loss_goes_flat() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(-0.10)))], 0.3);
    let (own, risk) = own_and_risk(&code, 1_000, 1_000, -0.10, 0.10);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.9,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Exit);
    assert_eq!(decision.reason, DecisionReason::PositionRisk);
    assert_eq!(decision.desired_delta_shares, -1_000);
}

#[test]
fn same_deep_loss_can_be_held_by_a_long_term_person() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.002), Some(-0.10)))], 0.3);
    let (own, risk) = own_and_risk(&code, 1_000, 1_000, -0.10, 0.10);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.9,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Hold);
    assert_eq!(decision.reason, DecisionReason::PositionRisk);
    assert_eq!(decision.desired_delta_shares, 0);
    assert_eq!(decision.executable_delta_shares, 0);
}

#[test]
fn long_term_style_still_has_a_small_chance_to_reduce_a_deep_loss() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(-0.10)))], 0.3);
    let (own, risk) = own_and_risk(&code, 1_000, 1_000, -0.10, 0.10);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.99,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Reduce);
    assert_eq!(decision.reason, DecisionReason::PositionRisk);
    assert!(decision.desired_delta_shares < 0);
    assert!(decision.executable_delta_shares < 0);
}

#[test]
fn risk_intent_respects_t1_before_execution() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.001), Some(-0.10)))], 0.8);
    let (own, risk) = own_and_risk(&code, 1_000, 0, -0.10, 0.60);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Exit);
    assert_eq!(decision.reason, DecisionReason::T1Locked);
    assert_eq!(decision.target_position_fraction, 0.0);
    assert_eq!(decision.desired_delta_shares, -1_000);
    assert_eq!(decision.executable_delta_shares, 0);
}

#[test]
fn sellable_risk_is_handled_before_a_more_severe_t1_locked_position() {
    let locked = StockCode("600101".into());
    let sellable = StockCode("600102".into());
    let (market, observations) = market_and_observations(
        [
            (locked.clone(), path(Some(0.0), Some(-0.20))),
            (sellable.clone(), path(Some(0.0), Some(-0.10))),
        ],
        0.3,
    );
    let own = SelfView {
        cash: Money::from_cents(10_000_000),
        positions: [
            (
                locked.clone(),
                PositionView {
                    qty: 1_000,
                    sellable_qty: 0,
                    cost_price: Some(Money::from_cents(1_250)),
                },
            ),
            (
                sellable.clone(),
                PositionView {
                    qty: 1_000,
                    sellable_qty: 1_000,
                    cost_price: Some(Money::from_cents(1_111)),
                },
            ),
        ]
        .into(),
    };
    let risk = AccountRiskObservation {
        equity: Money::from_cents(10_000_000),
        return_from_reference: None,
        drawdown_from_peak: None,
        positions: [
            (
                locked,
                PositionRiskObservation {
                    market_value: Money::from_cents(1_000_000),
                    unrealized_return: Some(-0.20),
                    equity_weight: Some(0.10),
                    drawdown_from_position_peak: None,
                },
            ),
            (
                sellable.clone(),
                PositionRiskObservation {
                    market_value: Money::from_cents(1_000_000),
                    unrealized_return: Some(-0.10),
                    equity_weight: Some(0.10),
                    drawdown_from_position_peak: None,
                },
            ),
        ]
        .into(),
    };

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.code.as_ref(), Some(&sellable));
    assert_eq!(decision.action, PositionAction::Exit);
    assert!(decision.executable_delta_shares < 0);
}

#[test]
fn held_stock_is_observed_before_an_unheld_discovery_candidate() {
    let held = StockCode("600102".into());
    let other = StockCode("600101".into());
    let (market, observations) = market_and_observations(
        [
            (other, path(Some(0.04), Some(0.0))),
            (held.clone(), path(Some(0.0), Some(0.0))),
        ],
        0.2,
    );
    let (own, risk) = own_and_risk(&held, 1_000, 1_000, 0.0, 0.10);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::Dormant,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.code.as_ref(), Some(&held));
    assert_eq!(decision.action, PositionAction::Hold);
}

#[test]
fn market_discovery_can_select_an_unheld_stock_after_the_sixty_percent_boundary() {
    let held = StockCode("600101".into());
    let discovered = StockCode("600102".into());
    let (market, observations) = market_and_observations(
        [
            (held.clone(), path(Some(0.0), Some(0.0))),
            (discovered.clone(), path(Some(0.0), Some(0.0))),
        ],
        0.2,
    );
    let (own, risk) = own_and_risk(&held, 1_000, 1_000, 0.0, 0.10);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::Dormant,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.60,
            index: 1,
        },
    );

    assert_eq!(decision.code.as_ref(), Some(&discovered));
}

#[test]
fn styles_can_interpret_the_same_slow_fall_in_opposite_ways() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(-0.04), Some(0.06)))], 0.3);
    let (own, risk) = no_position();

    let dip_buyer = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
    let momentum = decide_retail_position(
        &strategy(),
        RetailStyle::Momentum,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(dip_buyer.action, PositionAction::TryBuy);
    assert_eq!(dip_buyer.reason, DecisionReason::Pullback);
    assert!(dip_buyer.target_position_fraction > 0.0);
    assert!(dip_buyer.desired_delta_shares > 0);
    assert_eq!(
        dip_buyer.desired_delta_shares,
        dip_buyer.executable_delta_shares
    );
    assert_eq!(momentum.action, PositionAction::Watch);
    assert_eq!(momentum.reason, DecisionReason::NoSignal);
}

#[test]
fn flat_price_never_claims_a_pullback_or_momentum_signal() {
    let code = StockCode("600101".into());
    let (market, observations) = market_and_observations([(code, path(Some(0.0), Some(0.0)))], 0.2);
    let (own, risk) = no_position();

    for style in [RetailStyle::DipBuyer, RetailStyle::Momentum] {
        let decision = decide_retail_position(
            &strategy(),
            style,
            &market,
            &own,
            &observations,
            &risk,
            &mut FixedRng {
                value: 0.0,
                index: 0,
            },
        );
        assert_eq!(decision.reason, DecisionReason::NoSignal);
    }
}

#[test]
fn broad_market_decline_changes_a_heavy_holders_response() {
    let code = StockCode("600101".into());
    let (calm_market, calm_observations) =
        market_and_observations([(code.clone(), path(Some(-0.01), Some(0.0)))], 0.3);
    let (stress_market, stress_observations) =
        market_and_observations([(code.clone(), path(Some(-0.01), Some(0.0)))], 0.8);
    let (own, risk) = own_and_risk(&code, 5_000, 5_000, -0.03, 0.60);

    let calm = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &calm_market,
        &own,
        &calm_observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
    let stressed = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &stress_market,
        &own,
        &stress_observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(calm.action, PositionAction::Hold);
    assert_eq!(stressed.action, PositionAction::Reduce);
    assert_eq!(stressed.reason, DecisionReason::BroadMarketRisk);
    assert!(stressed.desired_delta_shares < 0);
}

#[test]
fn same_loss_creates_more_pressure_for_a_heavy_position() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(-0.01), Some(0.0)))], 0.8);
    let (light_own, light_risk) = own_and_risk(&code, 1_000, 1_000, -0.03, 0.10);
    let (heavy_own, heavy_risk) = own_and_risk(&code, 5_000, 5_000, -0.03, 0.60);

    let light = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &light_own,
        &observations,
        &light_risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
    let heavy = decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &heavy_own,
        &observations,
        &heavy_risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(light.action, PositionAction::Hold);
    assert_eq!(heavy.action, PositionAction::Reduce);
    assert_eq!(heavy.reason, DecisionReason::BroadMarketRisk);
}

#[test]
fn same_market_price_is_interpreted_from_each_persons_cost_basis() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(0.0)))], 0.3);
    let (losing_own, losing_risk) = own_and_risk(&code, 1_000, 1_000, -0.10, 0.10);
    let (winning_own, winning_risk) = own_and_risk(&code, 1_000, 1_000, 0.10, 0.10);

    let losing = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &losing_own,
        &observations,
        &losing_risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
    let winning = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &winning_own,
        &observations,
        &winning_risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(losing.action, PositionAction::Add);
    assert_eq!(losing.reason, DecisionReason::PositionRisk);
    assert_eq!(winning.action, PositionAction::Reduce);
    assert_eq!(winning.reason, DecisionReason::TakeProfit);
}

#[test]
fn dip_buying_and_dormancy_are_tendencies_not_permanent_prohibitions() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(0.0)))], 0.3);
    let (losing_own, losing_risk) = own_and_risk(&code, 1_000, 1_000, -0.10, 0.10);
    let dip_exit = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &losing_own,
        &observations,
        &losing_risk,
        &mut FixedRng {
            value: 0.99,
            index: 0,
        },
    );
    assert_eq!(dip_exit.action, PositionAction::Reduce);

    let (winning_own, winning_risk) = own_and_risk(&code, 1_000, 1_000, 0.10, 0.10);
    let dormant_profit = decide_retail_position(
        &strategy(),
        RetailStyle::Dormant,
        &market,
        &winning_own,
        &observations,
        &winning_risk,
        &mut FixedRng {
            value: 0.99,
            index: 0,
        },
    );
    assert_eq!(dormant_profit.action, PositionAction::Reduce);
    assert_eq!(dormant_profit.reason, DecisionReason::TakeProfit);
}

#[test]
fn adding_to_an_existing_odd_lot_preserves_the_remainder_and_buys_whole_lots() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(-0.04), Some(0.0)))], 0.2);
    let (own, risk) = own_and_risk(&code, 50, 50, 0.0, 0.005);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Add);
    assert!(decision.desired_delta_shares > 0);
    assert_eq!(decision.desired_delta_shares % 100, 0);
    assert_eq!(
        decision.desired_delta_shares,
        decision.executable_delta_shares
    );
}

#[test]
fn reducing_an_odd_lot_position_sells_whole_lots_without_turning_into_exit() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(0.0)))], 0.2);
    let (own, risk) = own_and_risk(&code, 150, 150, 0.10, 0.015);

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Reduce);
    assert_eq!(decision.desired_delta_shares, -100);
    assert_eq!(decision.executable_delta_shares, -100);
    assert!((decision.target_position_fraction - 0.005).abs() < f64::EPSILON);
}

#[test]
#[should_panic(expected = "missing its equity weight")]
fn malformed_held_position_risk_never_silently_becomes_zero_weight() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(0.0)))], 0.2);
    let (own, mut risk) = own_and_risk(&code, 100, 100, -0.10, 0.01);
    risk.positions.get_mut(&code).unwrap().equity_weight = None;

    decide_retail_position(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
}

#[test]
fn unavailable_cost_return_is_skipped_without_fabricating_position_risk() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(0.0)))], 0.2);
    let (own, mut risk) = own_and_risk(&code, 100, 100, 0.0, 0.01);
    risk.positions.get_mut(&code).unwrap().unrealized_return = None;
    let mut no_arrival = strategy();
    no_arrival.arrival_rate = 0.0;

    let decision = decide_retail_position(
        &no_arrival,
        RetailStyle::Panic,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Hold);
    assert_eq!(decision.reason, DecisionReason::NoSignal);
}

#[test]
fn repeated_filled_buy_failures_reduce_willingness_to_try_the_same_pullback() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(-0.04), Some(0.02)))], 0.2);
    let (own, risk) = no_position();
    let mut experienced = RetailExperienceState::new(Money::from_cents(10_000_000)).unwrap();
    experienced.consecutive_failed_buys = 3;
    experienced
        .record_fill(
            &code,
            engine::Side::Buy,
            Money::from_cents(1_100),
            0,
            100,
            None,
            1,
        )
        .unwrap();
    experienced
        .record_fill(
            &code,
            engine::Side::Sell,
            Money::from_cents(1_000),
            100,
            0,
            Some(Money::from_cents(1_100)),
            2,
        )
        .unwrap();
    experienced.consecutive_failed_buys = 3;

    let fresh = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.9,
            index: 0,
        },
    );
    let cautious = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &experienced,
        200,
        &mut FixedRng {
            value: 0.9,
            index: 0,
        },
    );

    assert_eq!(fresh.action, PositionAction::TryBuy);
    assert_eq!(cautious.action, PositionAction::Watch);
    assert_eq!(cautious.reason, DecisionReason::LowConfidence);
}

#[test]
fn post_exit_cooldown_blocks_reentry_but_not_later_discovery() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(-0.04), Some(0.02)))], 0.2);
    let (own, risk) = no_position();
    let mut experience = RetailExperienceState::new(Money::from_cents(10_000_000)).unwrap();
    experience
        .record_fill(
            &code,
            engine::Side::Buy,
            Money::from_cents(1_100),
            0,
            100,
            None,
            1,
        )
        .unwrap();
    experience
        .record_fill(
            &code,
            engine::Side::Sell,
            Money::from_cents(1_000),
            100,
            0,
            Some(Money::from_cents(1_100)),
            2,
        )
        .unwrap();

    let cooling = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &experience,
        100,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
    let recovered = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &experience,
        122,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(cooling.action, PositionAction::Watch);
    assert_eq!(cooling.reason, DecisionReason::PostExitCooldown);
    assert_eq!(recovered.action, PositionAction::TryBuy);
}

#[test]
fn a_previously_hurt_long_term_holder_can_reduce_near_break_even() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(-0.05)))], 0.2);
    let (own, risk) = own_and_risk(&code, 1_000, 1_000, 0.005, 0.10);
    let mut experience = RetailExperienceState::new(Money::from_cents(10_000_000)).unwrap();
    experience.consecutive_failed_buys = 2;

    let decision = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &experience,
        200,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Reduce);
    assert_eq!(decision.reason, DecisionReason::BreakEvenRelief);
}

#[test]
fn observed_profit_giveback_differs_from_a_fresh_position_at_the_same_cost_return() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(0.0)))], 0.2);
    let (own, mut risk) = own_and_risk(&code, 1_000, 1_000, 0.03, 0.10);
    risk.positions
        .get_mut(&code)
        .unwrap()
        .drawdown_from_position_peak = Some(-0.08);
    let experience = RetailExperienceState::new(Money::from_cents(10_000_000)).unwrap();

    let decision = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &experience,
        200,
        &mut FixedRng {
            value: 0.99,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::Reduce);
    assert_eq!(decision.reason, DecisionReason::ProfitGiveback);
}

#[test]
#[should_panic(expected = "positive finite dip threshold")]
fn decision_rejects_a_non_finite_dip_threshold() {
    let code = StockCode("600101".into());
    let (market, observations) = market_and_observations([(code, path(Some(0.0), Some(0.0)))], 0.2);
    let (own, risk) = no_position();
    let mut invalid = strategy();
    invalid.dip_threshold = f64::NAN;

    decide_retail_position(
        &invalid,
        RetailStyle::Noise,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
}

#[test]
#[should_panic(expected = "non-negative finite volume confirmation")]
fn decision_rejects_a_negative_volume_confirmation() {
    let code = StockCode("600101".into());
    let (market, observations) = market_and_observations([(code, path(Some(0.0), Some(0.0)))], 0.2);
    let (own, risk) = no_position();
    let mut invalid = strategy();
    invalid.volume_confirmation = -0.1;

    decide_retail_position(
        &invalid,
        RetailStyle::Noise,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );
}

#[test]
fn thirty_market_minutes_drive_the_signal_instead_of_flat_tick_history() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code, path(Some(-0.03), Some(0.02)))], 0.2);
    let (own, risk) = no_position();

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.action, PositionAction::TryBuy);
    assert_eq!(decision.reason, DecisionReason::Pullback);
}

#[test]
fn unavailable_market_time_history_never_fabricates_a_trend_reason() {
    let code = StockCode("600101".into());
    let (market, observations) = market_and_observations([(code.clone(), path(None, None))], 0.0);
    let (own, risk) = no_position();

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::DipBuyer,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.code.as_ref(), Some(&code));
    assert_eq!(decision.reason, DecisionReason::InsufficientHistory);
}

#[test]
fn base_trading_can_supply_liquidity_without_fabricating_a_trend_signal() {
    let code = StockCode("600101".into());
    let (market, observations) = market_and_observations([(code.clone(), path(None, None))], 0.0);
    let (own, risk) = no_position();

    let decision = decide_retail_position(
        &strategy(),
        RetailStyle::Noise,
        &market,
        &own,
        &observations,
        &risk,
        &mut FixedRng {
            value: 0.0,
            index: 0,
        },
    );

    assert_eq!(decision.code.as_ref(), Some(&code));
    assert_eq!(decision.action, PositionAction::TryBuy);
    assert_eq!(decision.reason, DecisionReason::InsufficientHistory);
    assert!(decision.desired_delta_shares > 0);
}

#[test]
fn target_position_is_split_into_a_legal_child_sell_order() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(-0.10)))], 0.3);
    let (own, risk) = own_and_risk(&code, 1_000, 1_000, -0.10, 0.10);
    let mut strategy = ZiNoiseStrategy::new(1.0, 500, 0.5, 1).unwrap();

    let intents = strategy
        .decide_with_behavior(
            &market,
            &own,
            Some(&observations),
            Some(&risk),
            &mut FixedRng {
                value: 0.0,
                index: 0,
            },
        )
        .intents;

    assert!(matches!(
        intents.as_slice(),
        [engine::Intent::PlaceLimit {
            code: intent_code,
            side: engine::Side::Sell,
            price,
            qty: 500,
        }] if intent_code == &code && *price == Money::from_cents(999)
    ));
}

#[test]
fn target_position_does_not_bypass_t1_when_forming_the_child_order() {
    let code = StockCode("600101".into());
    let (market, observations) =
        market_and_observations([(code.clone(), path(Some(0.0), Some(-0.10)))], 0.8);
    let (own, risk) = own_and_risk(&code, 1_000, 0, -0.10, 0.60);
    let mut strategy = ZiNoiseStrategy::new(1.0, 500, 0.5, 1).unwrap();

    assert!(strategy
        .decide_with_behavior(
            &market,
            &own,
            Some(&observations),
            Some(&risk),
            &mut FixedRng {
                value: 0.0,
                index: 0
            },
        )
        .intents
        .is_empty());
}
