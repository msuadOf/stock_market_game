//! engine strategy 三策略 + 工厂集成测试（TDD 红绿循环）。
use engine::account::StockCode;
use engine::money::Money;
use engine::strategy::{MarketView, Rng, StockView};
use std::collections::BTreeMap;

// 固定种子 mock Rng：返回预定序列。
struct SeqRng {
    vals: Vec<f64>,
    idx: usize,
    u32s: Vec<u32>,
    uidx: usize,
}
impl SeqRng {
    fn new_f64(v: f64) -> Self {
        SeqRng {
            vals: vec![v],
            idx: 0,
            u32s: vec![],
            uidx: 0,
        }
    }
}
impl Rng for SeqRng {
    fn next_f64(&mut self) -> f64 {
        let v = self.vals[self.idx.min(self.vals.len() - 1)];
        self.idx += 1;
        v
    }
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        let v = self.u32s.get(self.uidx).copied().unwrap_or(lo);
        self.uidx += 1;
        if hi <= lo {
            lo
        } else {
            v
        }
    }
}

fn one_stock_view(last: i64) -> MarketView {
    let mut stocks = BTreeMap::new();
    stocks.insert(
        StockCode("600101".to_string()),
        StockView {
            best_bid: Some(Money::from_cents(last - 1)),
            best_ask: Some(Money::from_cents(last + 1)),
            last_price: Money::from_cents(last),
            recent_prices: vec![Money::from_cents(last)],
            recent_market_minute_prices: vec![],
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        },
    );
    MarketView {
        stocks,
        tick: 0,
        market_minute: 0,
    }
}

#[test]
fn view_and_intent_serde_roundtrip() {
    let mv = one_stock_view(1000);
    let j = serde_json::to_value(&mv).unwrap();
    let back: MarketView = serde_json::from_value(j).unwrap();
    assert_eq!(back.stocks.len(), 1);
}

use engine::orderbook::Side;
use engine::strategy::{Intent, SelfView, Strategy, StrategyError, ZiNoiseStrategy};

#[test]
fn zi_noise_arrival_rate_zero_produces_nothing() {
    let mut s = ZiNoiseStrategy::new(0.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.is_empty()); // arrival_rate=0 → 不动作
}

#[test]
fn zi_noise_arrival_rate_one_acts_on_some_stock() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.3)); // 0.3<0.5 → 买
    assert_eq!(ints.len(), 1);
    assert!(matches!(
        ints[0],
        Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }
    ));
    // 选中股票在 market 内
    if let Intent::PlaceLimit { code, .. } = &ints[0] {
        assert!(mv.stocks.contains_key(code));
    }
}

#[test]
fn retail_does_not_emit_an_unfunded_buy_intent() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mv = stock_with_history("600101", vec![1_050, 1_020, 1_000]);
    let own = SelfView {
        cash: Money::ZERO,
        positions: BTreeMap::new(),
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.0)).is_empty());
}

#[test]
fn retail_random_sell_without_sellable_shares_is_a_noop() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1_000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.75)).is_empty());
}

#[test]
fn retail_random_sell_selects_an_actually_sellable_holding() {
    let held = StockCode("600102".to_string());
    let mut stocks = one_stock_view(1_000).stocks;
    stocks.insert(
        held.clone(),
        StockView {
            best_bid: Some(Money::from_cents(999)),
            best_ask: Some(Money::from_cents(1_001)),
            last_price: Money::from_cents(1_000),
            recent_prices: vec![Money::from_cents(1_000)],
            recent_market_minute_prices: vec![],
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        },
    );
    let market = MarketView {
        stocks,
        tick: 0,
        market_minute: 0,
    };
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: [(
            held.clone(),
            PositionView {
                qty: 100,
                sellable_qty: 100,
                cost_price: Some(Money::from_cents(900)),
            },
        )]
        .into(),
    };
    let mut strategy = ZiNoiseStrategy::new(1.0, 100, 0.0, 1).unwrap();
    let mut rng = SeqRng {
        vals: vec![0.0, 0.0, 0.75],
        idx: 0,
        u32s: vec![0, 0],
        uidx: 0,
    };

    let intents = strategy.decide(&market, &own, &mut rng);
    assert!(matches!(
        &intents[..],
        [Intent::PlaceLimit {
            code,
            side: Side::Sell,
            qty: 100,
            ..
        }] if code == &held
    ));
}

#[test]
fn zi_noise_chase_trend_buys_on_uptrend() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap(); // chase_prob=1
    let mv = {
        let mut stocks = BTreeMap::new();
        stocks.insert(
            StockCode("600101".to_string()),
            StockView {
                best_bid: Some(Money::from_cents(999)),
                best_ask: Some(Money::from_cents(1001)),
                last_price: Money::from_cents(1050),
                recent_prices: vec![
                    Money::from_cents(1000),
                    Money::from_cents(1020),
                    Money::from_cents(1050),
                ], // 上升
                recent_market_minute_prices: vec![
                    Money::from_cents(1000),
                    Money::from_cents(1020),
                    Money::from_cents(1050),
                ], // 完整交易分钟的上升
                relative_volume: 1.0,
                order_book_imbalance: 0.0,
            },
        );
        MarketView {
            stocks,
            tick: 0,
            market_minute: 0,
        }
    };
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn retail_without_position_tries_to_buy_a_falling_stock_at_the_best_ask() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mv = stock_with_history("600101", vec![1_050, 1_020, 1_000]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.0));

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Buy,
            price,
            qty: 100,
            ..
        }] if *price == Money::from_cents(1_001)
    ));
}

#[test]
fn retail_ignores_a_tick_only_price_move_when_evaluating_trend() {
    let mut strategy = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    // tick 级序列看似上涨，但最近完成的市场分钟收盘未动；不能据此追涨。
    let mut market = stock_with_history("600101", vec![1_000, 1_020, 1_050]);
    market
        .stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .recent_market_minute_prices = vec![
        Money::from_cents(1_000),
        Money::from_cents(1_000),
        Money::from_cents(1_000),
    ];
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    assert!(strategy
        .decide(&market, &own, &mut SeqRng::new_f64(0.5))
        .is_empty());
}

#[test]
fn retail_sells_at_the_best_bid_after_its_dip_buy_keeps_losing() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mv = stock_with_history("600101", vec![1_050, 1_020, 1_000]);
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(1_100)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions,
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.0));

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            price,
            qty: 100,
            ..
        }] if *price == Money::from_cents(999)
    ));
}

#[test]
fn retail_cannot_panic_sell_a_same_day_dip_buy() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mv = stock_with_history("600101", vec![1_050, 1_020, 1_000]);
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 0,
            cost_price: Some(Money::from_cents(1_100)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions,
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.0));

    assert!(
        intents.is_empty(),
        "A 股当日抄底仓位受 T+1 约束，不能立即恐慌卖出"
    );
}

#[test]
fn retail_does_not_stop_out_a_shallow_loss_and_may_keep_buying_the_dip() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mv = stock_with_history("600101", vec![1_050, 1_020, 1_000]);
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(1_030)),
        },
    );

    let intents = s.decide(
        &mv,
        &SelfView {
            cash: Money::from_cents(1_000_000),
            positions,
        },
        &mut SeqRng::new_f64(0.0),
    );

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }]
    ));
}

#[test]
fn ask_side_imbalance_can_turn_a_shallow_loss_into_a_stop_loss() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mut mv = stock_with_history("600101", vec![1_040, 1_020, 1_000]);
    mv.stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .order_book_imbalance = -1.0;
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(1_045)),
        },
    );

    let intents = s.decide(
        &mv,
        &SelfView {
            cash: Money::from_cents(1_000_000),
            positions,
        },
        &mut SeqRng::new_f64(0.0),
    );

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            ..
        }]
    ));
}

#[test]
fn retail_requires_volume_confirmation_to_chase_a_rebound() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mut mv = stock_with_history("600101", vec![1_000, 1_020, 1_050]);
    mv.stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .relative_volume = 0.20;
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.0)).is_empty());

    mv.stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .relative_volume = 1.0;
    assert!(matches!(
        s.decide(&mv, &own, &mut SeqRng::new_f64(0.0)).as_slice(),
        [Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }]
    ));
}

#[test]
fn retail_takes_profit_into_a_rising_market() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap();
    let mv = stock_with_history("600101", vec![1_000, 1_020, 1_050]);
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(900)),
        },
    );

    let intents = s.decide(
        &mv,
        &SelfView {
            cash: Money::from_cents(1_000_000),
            positions,
        },
        &mut SeqRng::new_f64(0.0),
    );

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            price,
            ..
        }] if *price == Money::from_cents(1_049)
    ));
}

use engine::strategy::{PositionView, TargetPolicy};

/// 显式目标价机构的测试壳（共同 V 删除后的等价入口）：decide 直接委托
/// 数据驱动内核 decide_data——与删除前 ValueStrategy::decide 同一条路径。
struct ValueStrategy {
    policy: TargetPolicy,
    margin: f64,
    order: u32,
}

impl ValueStrategy {
    fn new(policy: TargetPolicy, margin: f64, order: u32) -> Result<Self, String> {
        if !(0.0..1.0).contains(&margin) {
            return Err(format!("margin {margin} not in [0,1)"));
        }
        if order == 0 {
            return Err("order_size must be > 0".to_string());
        }
        Ok(Self {
            policy,
            margin,
            order,
        })
    }

    fn decide(&self, market: &MarketView, own: &SelfView, rng: &mut impl Rng) -> Vec<Intent> {
        let data =
            engine::strategy::StrategyData::inst(self.policy.clone(), self.margin, self.order);
        engine::decide_data(&data, market, own, rng)
    }
}

#[test]
fn value_buys_when_undervalued() {
    // 显式目标价 1000（旧 TrackV{bias:0} 在 V=1000 的等价形式），margin=0.05 → 买阈 950。last=900 < 950 → 买
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100).unwrap();
    let mv = one_stock_view(900); // last=900（目标价 1000）
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn value_strategy_uses_a_lower_best_ask_for_a_small_probe_when_last_trade_is_stale() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 400).unwrap();
    let mut mv = one_stock_view(1_100);
    let stock = mv.stocks.get_mut(&StockCode("600101".to_string())).unwrap();
    stock.best_ask = Some(Money::from_cents(940));
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Buy,
            price,
            qty: 100,
            ..
        }] if *price == Money::from_cents(940)
    ));
}

#[test]
fn value_strategy_adds_a_larger_tranche_as_the_ask_falls_further_below_value() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 400).unwrap();
    let mut mv = one_stock_view(1_100);
    let stock = mv.stocks.get_mut(&StockCode("600101".to_string())).unwrap();
    stock.best_ask = Some(Money::from_cents(850));
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Buy,
            price,
            qty: 200,
            ..
        }] if *price == Money::from_cents(850)
    ));
}

#[test]
fn value_strategy_stops_adding_when_one_stock_exceeds_its_risk_budget() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 400).unwrap();
    let mut mv = one_stock_view(900);
    mv.stocks.insert(
        StockCode("600102".to_string()),
        StockView {
            best_bid: Some(Money::from_cents(999)),
            best_ask: Some(Money::from_cents(1_001)),
            last_price: Money::from_cents(1_000),
            recent_prices: vec![Money::from_cents(1_000)],
            recent_market_minute_prices: vec![],
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        },
    );
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 3_000,
            sellable_qty: 3_000,
            cost_price: Some(Money::from_cents(900)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions,
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn value_strategy_never_expands_a_sub_lot_plan_into_a_board_lot() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 50).unwrap();
    let mv = one_stock_view(900);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));

    assert!(
        intents.is_empty(),
        "不足一手的机构计划量必须放弃下单，不能被策略静默放大"
    );
}

#[test]
fn value_strategy_rounds_a_non_board_lot_tranche_down_without_exceeding_the_plan() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 150).unwrap();
    let mv = one_stock_view(900);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }]
    ));
}

#[test]
fn value_strategy_does_not_submit_a_partial_sub_lot_sell() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 50).unwrap();
    let mv = one_stock_view(1_100);
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 1_000,
            sellable_qty: 1_000,
            cost_price: Some(Money::from_cents(900)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions,
    };

    let intents = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));

    assert!(intents.is_empty());
}

#[test]
fn value_strategy_rounds_a_partial_sell_down_but_allows_selling_the_full_odd_lot() {
    let mv = one_stock_view(1_100);

    let partial =
        ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 150).unwrap();
    let mut large_position = BTreeMap::new();
    large_position.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 1_000,
            sellable_qty: 1_000,
            cost_price: Some(Money::from_cents(900)),
        },
    );
    let partial_intents = partial.decide(
        &mv,
        &SelfView {
            cash: Money::from_cents(1_000_000),
            positions: large_position,
        },
        &mut SeqRng::new_f64(0.5),
    );
    assert!(matches!(
        partial_intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            qty: 100,
            ..
        }]
    ));

    let liquidate =
        ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 150).unwrap();
    let mut odd_lot_position = BTreeMap::new();
    odd_lot_position.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 150,
            sellable_qty: 150,
            cost_price: Some(Money::from_cents(900)),
        },
    );
    let liquidation_intents = liquidate.decide(
        &mv,
        &SelfView {
            cash: Money::from_cents(1_000_000),
            positions: odd_lot_position,
        },
        &mut SeqRng::new_f64(0.5),
    );
    assert!(matches!(
        liquidation_intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            qty: 150,
            ..
        }]
    ));
}

#[test]
fn value_strategy_keeps_a_full_board_lot_when_the_position_has_an_odd_lot_remainder() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100).unwrap();
    let mv = one_stock_view(1_100);
    let mut positions = BTreeMap::new();
    positions.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 250,
            sellable_qty: 250,
            cost_price: Some(Money::from_cents(900)),
        },
    );

    let intents = s.decide(
        &mv,
        &SelfView {
            cash: Money::from_cents(1_000_000),
            positions,
        },
        &mut SeqRng::new_f64(0.5),
    );

    assert!(matches!(
        intents.as_slice(),
        [Intent::PlaceLimit {
            side: Side::Sell,
            qty: 100,
            ..
        }]
    ));
}

#[test]
fn value_strategy_sell_quantity_is_the_largest_valid_quantity_within_its_plan() {
    let mv = one_stock_view(1_100);
    for planned in 1..=250 {
        for sellable in 1..=250 {
            let s =
                ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, planned)
                    .unwrap();
            let mut positions = BTreeMap::new();
            positions.insert(
                StockCode("600101".to_string()),
                PositionView {
                    qty: sellable,
                    sellable_qty: sellable,
                    cost_price: Some(Money::from_cents(900)),
                },
            );
            let intents = s.decide(
                &mv,
                &SelfView {
                    cash: Money::from_cents(1_000_000),
                    positions,
                },
                &mut SeqRng::new_f64(0.5),
            );
            let actual = match intents.as_slice() {
                [] => 0,
                [Intent::PlaceLimit {
                    side: Side::Sell,
                    qty,
                    ..
                }] => *qty,
                unexpected => panic!("unexpected institution sell intents: {unexpected:?}"),
            };
            let requested = planned.min(sellable);
            let expected = (1..=requested)
                .rev()
                .find(|qty| qty % 100 == 0 || qty % 100 == sellable % 100)
                .unwrap_or(0);
            assert_eq!(
                actual, expected,
                "planned={planned}, sellable={sellable} must use the largest valid A-share quantity"
            );
        }
    }
}

#[test]
fn value_no_action_when_in_band() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100).unwrap();
    let mv = one_stock_view(1000); // last=1000 在 [950,1050] 带内
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn value_sells_when_overvalued_and_has_position() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100).unwrap();
    let mv = one_stock_view(1100); // last=1100>1050 → 卖
    let mut pos = BTreeMap::new();
    pos.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(1000)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: pos,
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Sell,
            ..
        }
    )));
}

#[test]
fn value_no_sell_without_position() {
    let s = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100).unwrap();
    let mv = one_stock_view(1100);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    }; // 无持仓
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn value_target_policies_differ() {
    // Fixed(800) vs Fixed(1100)（旧 TrackV{bias:0.1} 在 V=1000 的等价形式）→ 两个不同目标价
    let s_fixed =
        ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(800)), 0.01, 100).unwrap();
    let s_high_target =
        ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_100)), 0.01, 100).unwrap();
    let mv = one_stock_view(900);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    // Fixed target=800, band [792,808]; last=900 > 808 → 应卖但无持仓 → 无动作
    assert!(s_fixed
        .decide(&mv, &own, &mut SeqRng::new_f64(0.5))
        .is_empty());
    // 高目标价 1100, band [1089,1111]; last=900 < 1089 → 买
    let ints = s_high_target.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn drift_up_uses_authoritative_market_minutes_so_reconstructed_strategy_does_not_diverge() {
    let policy = TargetPolicy::DriftUp {
        rate: 0.001,
        base: Money::from_cents(1_000),
    };
    let uninterrupted = ValueStrategy::new(policy.clone(), 0.0, 100).unwrap();
    let reconstructed = ValueStrategy::new(policy, 0.0, 100).unwrap();
    let mut market = one_stock_view(1_005);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    for market_minute in 0..9 {
        // 观察调度可能让同一市场分钟内发生不同次数的 decide；DriftUp 必须不受它影响。
        market.tick = market_minute * 17;
        market.market_minute = market_minute;
        uninterrupted.decide(&market, &own, &mut SeqRng::new_f64(0.5));
    }
    market.tick = 9_999;
    market.market_minute = 9;
    let continued = uninterrupted.decide(&market, &own, &mut SeqRng::new_f64(0.5));
    let after_restore = reconstructed.decide(&market, &own, &mut SeqRng::new_f64(0.5));

    assert_eq!(
        serde_json::to_value(&continued).unwrap(),
        serde_json::to_value(&after_restore).unwrap(),
        "同一权威市场分钟的 DriftUp 判断不得依赖未入存档的调用次数"
    );
    assert!(continued.iter().any(|intent| matches!(
        intent,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn drift_up_first_decision_uses_one_elapsed_market_minute() {
    let strategy = ValueStrategy::new(
        TargetPolicy::DriftUp {
            rate: 0.01,
            base: Money::from_cents(1_000),
        },
        0.0,
        100,
    )
    .unwrap();
    let market = one_stock_view(1_005);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let intents = strategy.decide(&market, &own, &mut SeqRng::new_f64(0.5));

    assert!(intents.iter().any(|intent| matches!(
        intent,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn drift_up_ignores_tick_density_within_the_same_market_minute() {
    let policy = TargetPolicy::DriftUp {
        rate: 0.01,
        base: Money::from_cents(1_000),
    };
    let sparse = ValueStrategy::new(policy.clone(), 0.0, 100).unwrap();
    let dense = ValueStrategy::new(policy, 0.0, 100).unwrap();
    let mut sparse_market = one_stock_view(1_005);
    let mut dense_market = sparse_market.clone();
    sparse_market.tick = 1;
    dense_market.tick = 10_000;
    sparse_market.market_minute = 0;
    dense_market.market_minute = 0;
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let sparse_intents = sparse.decide(&sparse_market, &own, &mut SeqRng::new_f64(0.5));
    let dense_intents = dense.decide(&dense_market, &own, &mut SeqRng::new_f64(0.5));

    assert_eq!(
        serde_json::to_value(&sparse_intents).unwrap(),
        serde_json::to_value(&dense_intents).unwrap(),
        "同一市场分钟内的宿主 tick 密度不得改变 DriftUp 的目标价"
    );
}

#[test]
fn drift_up_saturates_max_market_minute_and_matches_data_path() {
    let policy = TargetPolicy::DriftUp {
        rate: 0.01,
        base: Money::from_cents(1_000),
    };
    let legacy = ValueStrategy::new(policy.clone(), 0.0, 100).unwrap();
    let data = StrategyData::inst(policy, 0.0, 100);
    let mut market = one_stock_view(1_005);
    market.tick = 0;
    market.market_minute = u64::MAX;
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    let legacy_intents = legacy.decide(&market, &own, &mut SeqRng::new_f64(0.5));
    let data_intents = decide_data(&data, &market, &own, &mut SeqRng::new_f64(0.5));

    assert_eq!(
        serde_json::to_value(&legacy_intents).unwrap(),
        serde_json::to_value(&data_intents).unwrap()
    );
    assert!(legacy_intents.iter().any(|intent| matches!(
        intent,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn value_rejects_invalid_params() {
    assert!(ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), -0.1, 100).is_err());
    // margin<0
    assert!(ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), 0.05, 0).is_err());
    // order_size=0
}

#[test]
fn zi_noise_rejects_invalid_params() {
    assert!(ZiNoiseStrategy::new(1.5, 100, 0.0, 1).is_err()); // arrival_rate>1
    assert!(ZiNoiseStrategy::new(0.5, 0, 0.0, 1).is_err()); // order_size_mean=0
    assert!(ZiNoiseStrategy::new(0.5, 100, 0.0, 0).is_err()); // tick_cents=0
                                                              // 顺便确认合法参数 + StrategyError 变体可达（避免 use 未被检查）。
    let ok = ZiNoiseStrategy::new(0.5, 100, 0.1, 1);
    assert!(ok.is_ok());
    assert!(matches!(
        ZiNoiseStrategy::new(1.5, 100, 0.0, 1).err(),
        Some(StrategyError::InvalidParam { .. })
    ));
}

use engine::strategy::MomentumStrategy;

/// 构造单股 MarketView，其 recent_prices = hist（用于游资动量趋势检测）。
fn stock_with_history(code: &str, hist: Vec<i64>) -> MarketView {
    let mut stocks = BTreeMap::new();
    let last = *hist.last().unwrap_or(&1000);
    stocks.insert(
        StockCode(code.to_string()),
        StockView {
            best_bid: Some(Money::from_cents(last - 1)),
            best_ask: Some(Money::from_cents(last + 1)),
            last_price: Money::from_cents(last),
            recent_prices: hist.iter().copied().map(Money::from_cents).collect(),
            recent_market_minute_prices: hist.into_iter().map(Money::from_cents).collect(),
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        },
    );
    MarketView {
        stocks,
        tick: 0,
        market_minute: 0,
    }
}

#[test]
fn momentum_buys_on_uptrend() {
    // hist[1000,1020,1050]：lookback=3, change=(1050-1000)/1000=+5% > 2% → 买
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1000, 1020, 1050]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

#[test]
fn momentum_uses_completed_market_minutes_instead_of_tick_samples() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    // tick 级序列看起来上涨 5%，但同一段完整交易分钟收盘保持不变；游资不得把
    // 宿主的 tick 密度误当作市场时间并据此追涨。
    let mut mv = stock_with_history("600101", vec![1_000, 1_020, 1_050]);
    mv.stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .recent_market_minute_prices = vec![
        Money::from_cents(1_000),
        Money::from_cents(1_000),
        Money::from_cents(1_000),
    ];
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_waits_for_volume_before_chasing_an_uptrend() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mut mv = stock_with_history("600101", vec![1_000, 1_020, 1_050]);
    mv.stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .relative_volume = 0.20;
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_does_not_chase_into_a_heavily_ask_skewed_book() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mut mv = stock_with_history("600101", vec![1_000, 1_020, 1_050]);
    mv.stocks
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .order_book_imbalance = -0.80;
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_sells_on_downtrend_with_position() {
    // hist[1050,1020,1000]：change=(1000-1050)/1050≈-4.76% < -2% + 持仓 → 卖
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1050, 1020, 1000]);
    let mut pos = BTreeMap::new();
    pos.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(1020)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: pos,
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Sell,
            ..
        }
    )));
}

#[test]
fn momentum_no_action_on_flat() {
    // hist[1000,1005,1003]：change=(1003-1000)/1000≈+0.3% < 2% → 空
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1000, 1005, 1003]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_no_sell_without_position() {
    // 下跌但无持仓 → 不动作（不超卖）。
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1050, 1020, 1000]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_rejects_invalid_params() {
    assert!(MomentumStrategy::new(1, 0.02, 100).is_err()); // lookback<2
    assert!(MomentumStrategy::new(3, -0.1, 100).is_err()); // threshold<0
    assert!(MomentumStrategy::new(3, 0.02, 0).is_err()); // order_size=0
}

use engine::account::AccountKind;
use engine::strategy::{
    HotParams, InstParams, RetailParams, RetailStyle, StrategyFactory, StrategyParams,
};

/// 合法 StrategyParams 样本（群体基准参数，供工厂构造测试复用）。
fn sample_params() -> StrategyParams {
    StrategyParams {
        retail: RetailParams {
            arrival_rate: 0.5,
            order_size_mean: 100,
            chase_prob: 0.2,
            tick_cents: 1,
        },
        inst: InstParams {
            margin: 0.05,
            order_size: 200,
        },
        hot: HotParams {
            lookback: 3,
            trend_threshold: 0.02,
            order_size: 150,
        },
    }
}

#[test]
fn factory_builds_retail() {
    let p = sample_params();
    let s = StrategyFactory::build(AccountKind::Retail, &p, &mut SeqRng::new_f64(0.5)).unwrap();
    assert!(s.is_some());
}

#[test]
fn factory_samples_individual_daily_attention_rates_within_kind_ranges() {
    let params = sample_params();
    let mut profile_rng = engine::session::SplitMix64::new(0xA77E_7710);
    let probability =
        |observations_per_day: f64| 1.0 - (-observations_per_day / 15_300.0_f64).exp();

    for (kind, minimum, maximum) in [
        (AccountKind::Retail, probability(0.1), probability(100.0)),
        (AccountKind::Inst, probability(10.0), probability(100.0)),
        (AccountKind::Hot, probability(80.0), probability(300.0)),
    ] {
        let probabilities: Vec<f64> = (0..32)
            .map(|_| {
                StrategyFactory::build(kind, &params, &mut profile_rng)
                    .unwrap()
                    .unwrap()
                    .base_observation_probability()
            })
            .collect();

        assert!(probabilities
            .iter()
            .all(|probability| (*probability >= minimum) && (*probability <= maximum)));
        assert!(
            probabilities.windows(2).any(|pair| pair[0] != pair[1]),
            "{kind:?} 应形成不同的个体注意力"
        );
    }
}

#[test]
fn factory_creates_multiple_named_retail_behavior_styles() {
    let params = sample_params();
    let mut rng = engine::session::SplitMix64::new(0x0057_A1E5);
    let styles: std::collections::BTreeSet<RetailStyle> = (0..512)
        .map(|_| {
            StrategyFactory::build(AccountKind::Retail, &params, &mut rng)
                .unwrap()
                .unwrap()
                .retail_style()
                .unwrap()
        })
        .collect();

    assert_eq!(styles.len(), 6);
}

#[test]
fn five_institution_accounts_receive_five_distinct_named_styles() {
    use engine::strategy::InstitutionStyle;

    let params = sample_params();
    let mut rng = engine::session::SplitMix64::new(0x1A57_1700);
    let styles: std::collections::BTreeSet<InstitutionStyle> = (0..5)
        .map(|ordinal| {
            StrategyFactory::build_for_market_day_with_ordinal(
                AccountKind::Inst,
                &params,
                15_300,
                ordinal,
                &mut rng,
            )
            .unwrap()
            .unwrap()
            .institution_style()
            .unwrap()
        })
        .collect();

    assert_eq!(styles.len(), 5);
}

#[test]
fn active_trader_institution_keeps_its_identity_but_uses_the_momentum_strategy_family() {
    use engine::strategy::StrategyFamily;

    let params = sample_params();
    let mut rng = engine::session::SplitMix64::new(0xA01_5A01);
    let strategy = StrategyFactory::build_for_market_day_with_ordinal(
        AccountKind::Inst,
        &params,
        15_300,
        4,
        &mut rng,
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        strategy.institution_style(),
        Some(engine::strategy::InstitutionStyle::ActiveTrader)
    );
    assert_eq!(strategy.strategy_family(), StrategyFamily::Momentum);
    assert!(
        strategy.belief_chain_params().is_none(),
        "机构身份本身不得让动量策略进入信念决策链"
    );
    assert!(
        !strategy.uses_parent_order_execution(),
        "动量机构应走普通工作报价，而不是价值机构的母单执行"
    );
}

#[test]
fn two_hot_money_accounts_receive_momentum_and_reversal_styles() {
    use engine::strategy::HotStyle;

    let params = sample_params();
    let mut rng = engine::session::SplitMix64::new(0xA07_5700);
    let styles: std::collections::BTreeSet<HotStyle> = (0..2)
        .map(|ordinal| {
            StrategyFactory::build_for_market_day_with_ordinal(
                AccountKind::Hot,
                &params,
                15_300,
                ordinal,
                &mut rng,
            )
            .unwrap()
            .unwrap()
            .hot_style()
            .unwrap()
        })
        .collect();

    assert_eq!(styles, [HotStyle::Momentum, HotStyle::Reversal].into());
}

#[test]
fn reversal_hot_money_buys_a_volume_confirmed_fall_instead_of_joining_the_selloff() {
    let params = sample_params();
    let mut profile_rng = engine::session::SplitMix64::new(0xA07_5EED);
    let mut strategy = StrategyFactory::build_for_market_day_with_ordinal(
        AccountKind::Hot,
        &params,
        15_300,
        1,
        &mut profile_rng,
    )
    .unwrap()
    .unwrap();
    let mut market = stock_with_history("600101", vec![1_000, 950, 900]);
    market
        .stocks
        .values_mut()
        .for_each(|stock| stock.relative_volume = 2.0);
    let own = SelfView {
        cash: Money::from_cents(1_000_000_000),
        positions: BTreeMap::new(),
    };

    let intents = strategy.decide(&market, &own, &mut SeqRng::new_f64(0.5));

    assert!(intents.iter().any(|intent| matches!(
        intent,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
    assert!(!intents.iter().any(|intent| matches!(
        intent,
        Intent::PlaceLimit {
            side: Side::Sell,
            ..
        }
    )));
}

#[test]
fn reversal_hot_money_ignores_a_tick_only_selloff() {
    let params = sample_params();
    let mut profile_rng = engine::session::SplitMix64::new(0xA07_5EED);
    let mut strategy = StrategyFactory::build_for_market_day_with_ordinal(
        AccountKind::Hot,
        &params,
        15_300,
        1,
        &mut profile_rng,
    )
    .unwrap()
    .unwrap();
    let mut market = stock_with_history("600101", vec![1_000, 950, 900]);
    market.stocks.values_mut().for_each(|stock| {
        stock.relative_volume = 2.0;
        stock.recent_market_minute_prices = vec![
            Money::from_cents(1_000),
            Money::from_cents(1_000),
            Money::from_cents(1_000),
        ];
    });
    let own = SelfView {
        cash: Money::from_cents(1_000_000_000),
        positions: BTreeMap::new(),
    };

    assert!(strategy
        .decide(&market, &own, &mut SeqRng::new_f64(0.5))
        .is_empty());
}

#[test]
fn factory_samples_heterogeneous_board_lot_order_sizes_for_institutions() {
    let mut params = sample_params();
    params.inst.order_size = 2_000;
    let mut profile_rng = engine::session::SplitMix64::new(0x0D3E_512E);
    let mut quantities = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let strategy = StrategyFactory::build(AccountKind::Inst, &params, &mut profile_rng)
            .unwrap()
            .unwrap();
        // 信念机构的单笔规模经链参数暴露（方向由决策链驱动，不再经 decide）。
        let chain = strategy
            .belief_chain_params()
            .expect("默认序号机构是信念风格，应暴露链参数");
        let qty = chain.order_size;
        assert_eq!(qty % 100, 0, "机构订单必须保持 100 股整数倍");
        assert!((1_200..=2_800).contains(&qty));
        quantities.insert(qty);
    }

    assert!(
        quantities.len() >= 5,
        "群体基准 2000 股不应让所有机构使用同一个订单尺寸：{quantities:?}"
    );
}

#[test]
fn factory_samples_multiple_order_sizes_for_default_retail_center() {
    let mut params = sample_params();
    params.retail.arrival_rate = 1.0;
    params.retail.chase_prob = 1.0;
    params.retail.order_size_mean = 300;
    let mut profile_rng = engine::session::SplitMix64::new(0x2E7A_1100);
    let mut quantities = std::collections::BTreeSet::new();
    let mut market = stock_with_history("600101", vec![1_000, 900]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000_000),
        positions: BTreeMap::new(),
    };

    for _ in 0..64 {
        let mut strategy = StrategyFactory::build(AccountKind::Retail, &params, &mut profile_rng)
            .unwrap()
            .unwrap();
        market.tick = 0;
        for intent in strategy.decide(&market, &own, &mut SeqRng::new_f64(0.0)) {
            if let Intent::PlaceLimit {
                side: Side::Buy,
                qty,
                ..
            } = intent
            {
                quantities.insert(qty);
            }
        }
    }

    assert_eq!(quantities, [200, 300, 400].into());
}

#[test]
fn factory_samples_multiple_order_sizes_for_default_hot_center() {
    let mut params = sample_params();
    params.hot.order_size = 1_000;
    let mut profile_rng = engine::session::SplitMix64::new(0xA07_512E);
    let mut quantities = std::collections::BTreeSet::new();
    let mut market = stock_with_history("600101", vec![1_000, 1_050, 1_100]);
    market
        .stocks
        .values_mut()
        .for_each(|stock| stock.relative_volume = 2.0);
    let own = SelfView {
        cash: Money::from_cents(1_000_000_000),
        positions: BTreeMap::new(),
    };

    for _ in 0..64 {
        let mut strategy = StrategyFactory::build(AccountKind::Hot, &params, &mut profile_rng)
            .unwrap()
            .unwrap();
        market.tick = 0;
        for intent in strategy.decide(&market, &own, &mut SeqRng::new_f64(0.5)) {
            if let Intent::PlaceLimit {
                side: Side::Buy,
                qty,
                ..
            } = intent
            {
                quantities.insert(qty);
            }
        }
    }

    assert!(
        quantities.len() >= 7,
        "默认游资尺寸应形成多档：{quantities:?}"
    );
    assert!(quantities.iter().all(|qty| qty % 100 == 0));
    assert!(quantities.iter().all(|qty| (600..=1_400).contains(qty)));
}

#[test]
fn factory_rejects_an_order_size_center_whose_full_range_cannot_fit_u32() {
    let mut params = sample_params();
    params.inst.order_size = u32::MAX;

    let error = StrategyFactory::build(
        AccountKind::Inst,
        &params,
        &mut engine::session::SplitMix64::new(7),
    )
    .err()
    .expect("无法表达完整 140% 上界的配置必须显式失败");

    assert!(error.to_string().contains("60%-140%"));
}

#[test]
fn factory_creates_persistent_retail_threshold_diversity() {
    let mut params = sample_params();
    params.retail.arrival_rate = 1.0;
    params.retail.chase_prob = 1.0;
    let mv = stock_with_history("600101", vec![1_000, 975, 950]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let mut profile_rng = engine::session::SplitMix64::new(0xB3A4_7102);
    let mut buyers = 0;
    let mut observers = 0;

    for _ in 0..64 {
        let mut strategy = StrategyFactory::build(AccountKind::Retail, &params, &mut profile_rng)
            .unwrap()
            .unwrap();
        if strategy
            .decide(&mv, &own, &mut SeqRng::new_f64(0.0))
            .is_empty()
        {
            observers += 1;
        } else {
            buyers += 1;
        }
    }

    assert!(buyers > 0, "5% 下跌应触发一部分散户抄底");
    assert!(observers > 0, "5% 下跌不应让所有散户同步抄底");
}

#[test]
fn deeper_fall_triggers_more_retail_stop_losses() {
    let mut params = sample_params();
    params.retail.arrival_rate = 1.0;
    params.retail.chase_prob = 1.0;
    let shallow = stock_with_history("600101", vec![1_000, 980, 960]);
    let deep = stock_with_history("600101", vec![1_000, 950, 900]);
    let position = |last_cost| {
        let mut positions = BTreeMap::new();
        positions.insert(
            StockCode("600101".to_string()),
            PositionView {
                qty: 100,
                sellable_qty: 100,
                cost_price: Some(Money::from_cents(last_cost)),
            },
        );
        SelfView {
            cash: Money::from_cents(1_000_000),
            positions,
        }
    };
    let own = position(1_000);
    let mut shallow_profiles = engine::session::SplitMix64::new(0x5109_1055);
    let mut deep_profiles = engine::session::SplitMix64::new(0x5109_1055);
    let mut shallow_sellers = 0;
    let mut deep_sellers = 0;

    for _ in 0..128 {
        let mut shallow_strategy =
            StrategyFactory::build(AccountKind::Retail, &params, &mut shallow_profiles)
                .unwrap()
                .unwrap();
        let mut deep_strategy =
            StrategyFactory::build(AccountKind::Retail, &params, &mut deep_profiles)
                .unwrap()
                .unwrap();
        shallow_sellers += shallow_strategy
            .decide(&shallow, &own, &mut SeqRng::new_f64(0.0))
            .iter()
            .filter(|intent| {
                matches!(
                    intent,
                    Intent::PlaceLimit {
                        side: Side::Sell,
                        ..
                    }
                )
            })
            .count();
        deep_sellers += deep_strategy
            .decide(&deep, &own, &mut SeqRng::new_f64(0.0))
            .iter()
            .filter(|intent| {
                matches!(
                    intent,
                    Intent::PlaceLimit {
                        side: Side::Sell,
                        ..
                    }
                )
            })
            .count();
    }

    assert!(shallow_sellers > 0, "4% 下跌应先触发少数低阈值止损者");
    assert!(
        deep_sellers > shallow_sellers,
        "跌幅从 4% 扩大到 10% 时，应有更多个体阈值被击穿"
    );
}

#[test]
fn deeper_fall_attracts_more_retail_dip_buyers() {
    let mut params = sample_params();
    params.retail.arrival_rate = 1.0;
    params.retail.chase_prob = 1.0;
    let shallow = stock_with_history("600101", vec![1_000, 990, 980]);
    let deep = stock_with_history("600101", vec![1_000, 960, 930]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let mut shallow_profiles = engine::session::SplitMix64::new(0xD1B0_7700);
    let mut deep_profiles = engine::session::SplitMix64::new(0xD1B0_7700);
    let mut shallow_buyers = 0;
    let mut deep_buyers = 0;

    for _ in 0..128 {
        let mut shallow_strategy =
            StrategyFactory::build(AccountKind::Retail, &params, &mut shallow_profiles)
                .unwrap()
                .unwrap();
        let mut deep_strategy =
            StrategyFactory::build(AccountKind::Retail, &params, &mut deep_profiles)
                .unwrap()
                .unwrap();
        shallow_buyers += shallow_strategy
            .decide(&shallow, &own, &mut SeqRng::new_f64(0.0))
            .iter()
            .filter(|intent| {
                matches!(
                    intent,
                    Intent::PlaceLimit {
                        side: Side::Buy,
                        ..
                    }
                )
            })
            .count();
        deep_buyers += deep_strategy
            .decide(&deep, &own, &mut SeqRng::new_f64(0.0))
            .iter()
            .filter(|intent| {
                matches!(
                    intent,
                    Intent::PlaceLimit {
                        side: Side::Buy,
                        ..
                    }
                )
            })
            .count();
    }

    assert!(shallow_buyers > 0, "2% 下跌应吸引少数敏感抄底者");
    assert!(
        deep_buyers > shallow_buyers,
        "跌幅从 2% 扩大到 7% 时，应吸引更多不同阈值的抄底者"
    );
}

#[test]
fn factory_player_returns_none() {
    let p = sample_params();
    assert!(
        StrategyFactory::build(AccountKind::Player, &p, &mut SeqRng::new_f64(0.5))
            .unwrap()
            .is_none()
    );
}

#[test]
fn factory_builds_each_kind() {
    let p = sample_params();
    assert!(
        StrategyFactory::build(AccountKind::Retail, &p, &mut SeqRng::new_f64(0.5))
            .unwrap()
            .is_some()
    );
    assert!(
        StrategyFactory::build(AccountKind::Inst, &p, &mut SeqRng::new_f64(0.5))
            .unwrap()
            .is_some()
    );
    assert!(
        StrategyFactory::build(AccountKind::Hot, &p, &mut SeqRng::new_f64(0.5))
            .unwrap()
            .is_some()
    );
}

#[test]
fn factory_returns_strategy_error_for_invalid_parameters() {
    let mut params = sample_params();
    params.retail.arrival_rate = -0.1;

    assert!(
        StrategyFactory::build(AccountKind::Retail, &params, &mut SeqRng::new_f64(0.5)).is_err()
    );
}

#[test]
fn reexport_from_crate_root() {
    use engine::{
        BeliefInstitutionStrategy, Intent, MarketView, MomentumStrategy, PositionView, SelfView,
        StockView, StrategyError, StrategyFactory, StrategyParams, ZiNoiseStrategy,
    };
    // 三策略均可从 crate 根直接构造。
    let _ = ZiNoiseStrategy::new(0.5, 100, 0.1, 1).unwrap();
    let _ = BeliefInstitutionStrategy::new(0.05, 100).unwrap();
    let _ = MomentumStrategy::new(3, 0.02, 100).unwrap();
    // 工厂 + 参数 + 目标价策略 + 错误类型可见。
    let _: StrategyParams = sample_params();
    let _: Intent = Intent::PlaceMarket {
        code: StockCode("x".to_string()),
        side: engine::Side::Buy,
        qty: 1,
    };
    // 视图类型可见（各构造一个实例，确保 re-export 命名可达）。
    let _mv = MarketView {
        stocks: std::collections::BTreeMap::new(),
        tick: 0,
        market_minute: 0,
    };
    let _sv = SelfView {
        cash: Money::from_cents(0),
        positions: std::collections::BTreeMap::new(),
    };
    let _stv = StockView {
        best_bid: None,
        best_ask: None,
        last_price: Money::from_cents(0),
        recent_prices: vec![],
        recent_market_minute_prices: vec![],
        relative_volume: 1.0,
        order_book_imbalance: 0.0,
    };
    let _pv = PositionView {
        qty: 0,
        sellable_qty: 0,
        cost_price: None,
    };
    // StrategyError 变体可达。
    let _err: StrategyError = StrategyError::InvalidParam {
        param: "x",
        reason: "test".to_string(),
    };
    // 工厂可调用（Player → None）。
    let p = sample_params();
    assert!(
        StrategyFactory::build(AccountKind::Player, &p, &mut SeqRng::new_f64(0.5))
            .unwrap()
            .is_none()
    );
}

// ─── 数据驱动策略（ADR-0006 数据化改造，为 GPU 化铺路）──────────────────────────
use engine::strategy::{decide_data, StrategyData};

/// 数据驱动：StrategyData 可 serde 往返（未来塞进 GPU StorageBuffer 的前提）。
#[test]
fn strategy_data_serde_roundtrip() {
    let d = StrategyData {
        kind: AccountKind::Retail,
        arrival_rate: 0.5,
        order_size_mean: 100,
        chase_prob: 0.2,
        tick_cents: 1,
        dip_threshold: 0.02,
        stop_loss_threshold: 0.05,
        take_profit_threshold: 0.08,
        volume_confirmation: 0.60,
        max_stock_fraction: 0.35,
        base_observation_probability: 0.25,
        margin: 0.05,
        order_size: 200,
        target_policy: TargetPolicy::Fixed(Money::from_cents(1_000)),
        lookback: 3,
        trend_threshold: 0.02,
    };
    let j = serde_json::to_value(&d).unwrap();
    let back: StrategyData = serde_json::from_value(j).unwrap();
    assert_eq!(back.kind, AccountKind::Retail);
    assert_eq!(back.arrival_rate, 0.5);
    assert_eq!(back.order_size_mean, 100);
}

/// 数据驱动 decide 与旧 ZiNoise 路径行为一致（arrival_rate=0 → 不动作）。
#[test]
fn decide_data_retail_no_action_when_no_arrival() {
    let d = StrategyData::retail(0.0, 100, 0.0, 1);
    let mv = one_stock_view(1000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(decide_data(&d, &mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn decide_data_evaluates_only_after_the_session_scheduler_dispatches_it() {
    let mut data = StrategyData::retail(1.0, 100, 0.0, 1);
    data.base_observation_probability = 0.01;
    let mut market = one_stock_view(1_000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };

    market.tick = 9_999;
    assert!(!decide_data(&data, &market, &own, &mut SeqRng::new_f64(0.3)).is_empty());
}

/// 数据驱动 decide 散户买入分支与旧路径一致。
#[test]
fn decide_data_retail_buys() {
    let d = StrategyData::retail(1.0, 100, 0.0, 1);
    let mv = one_stock_view(1000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = decide_data(&d, &mv, &own, &mut SeqRng::new_f64(0.3)); // 0.3<0.5 → 买
    assert_eq!(ints.len(), 1);
    assert!(matches!(
        ints[0],
        Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }
    ));
}

/// 数据驱动机构买入分支（低估）与旧路径一致。
#[test]
fn decide_data_inst_buys_when_undervalued() {
    let d = StrategyData::inst(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 200);
    let mv = one_stock_view(900);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = decide_data(&d, &mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

/// 数据驱动游资追涨买入与旧路径一致。
#[test]
fn decide_data_hot_buys_on_uptrend() {
    let d = StrategyData::hot(3, 0.02, 150);
    let mv = stock_with_history("600101", vec![1000, 1020, 1050]);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = decide_data(&d, &mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(
        i,
        Intent::PlaceLimit {
            side: Side::Buy,
            ..
        }
    )));
}

/// 数据驱动：玩家账户恒不动作。
#[test]
fn decide_data_player_is_noop() {
    let mut d = StrategyData::retail(1.0, 100, 1.0, 1);
    d.kind = AccountKind::Player;
    let mv = one_stock_view(1000);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(decide_data(&d, &mv, &own, &mut SeqRng::new_f64(0.0)).is_empty());
}

/// 散户必须覆盖**全部**股票（修复「只有一只股票有成交」的回归断言）。
///
/// 旧实现恒取 `first_key_value()`（字典序最小的 000812）→ 全部散户订单集中一只。
/// 修正后散户均匀随机选股：用足够大的真随机源跑很多轮，5 只股票**每一只**都应被选中过。
/// 若回归到 first_key_value，本测试只有 000812 会被命中 → 失败。
#[test]
fn retail_covers_all_stocks_not_just_first() {
    use engine::strategy::{RetailParams, StrategyParams};
    use std::collections::HashSet;

    // 5 只股票的 MarketView（code 故意涵盖各种前缀，首键字典序为 000812）。
    let mut stocks = BTreeMap::new();
    for code in ["000812", "002156", "300260", "600101", "600610"] {
        stocks.insert(
            StockCode(code.to_string()),
            StockView {
                best_bid: Some(Money::from_cents(999)),
                best_ask: Some(Money::from_cents(1001)),
                last_price: Money::from_cents(1000),
                recent_prices: vec![Money::from_cents(1000)],
                recent_market_minute_prices: vec![],
                relative_volume: 1.0,
                order_book_imbalance: 0.0,
            },
        );
    }
    let mv = MarketView {
        stocks,
        tick: 0,
        market_minute: 0,
    };
    let own = SelfView {
        cash: Money::from_cents(1_000_000_000),
        positions: BTreeMap::new(),
    };

    // arrival_rate=1 + chase_prob=0 → 每 tick 必到达、必走随机买卖分支（每轮选一只股）。
    let p = StrategyParams {
        retail: RetailParams {
            arrival_rate: 1.0,
            order_size_mean: 100,
            chase_prob: 0.0,
            tick_cents: 1,
        },
        inst: InstParams {
            margin: 0.05,
            order_size: 200,
        },
        hot: HotParams {
            lookback: 3,
            trend_threshold: 0.02,
            order_size: 150,
        },
    };
    let mut s = StrategyFactory::build(AccountKind::Retail, &p, &mut SeqRng::new_f64(0.5))
        .unwrap()
        .unwrap();

    // 用种子化确定性 RNG 跑 2000 轮：5 只股票每只期望 ~400 次，远超 0。
    // 确定性 → 失败可复现（铁律三）。
    let mut rng = engine::session::SplitMix64::new(0xC1A0DE570C11);
    let mut seen: HashSet<String> = HashSet::new();
    for _ in 0..2000 {
        for it in s.decide(&mv, &own, &mut rng) {
            if let Intent::PlaceLimit { code, .. } = it {
                seen.insert(code.0);
            }
        }
    }
    let all: HashSet<String> = ["000812", "002156", "300260", "600101", "600610"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        seen, all,
        "散户未覆盖全部股票（seen={:?}）——回归到「只交易首键」的 bug",
        seen
    );
}

/// 数据驱动 decide_data 与旧 trait 路径**逐 Intent 等价**（同种子同输出，铁律三）。
/// 这是改造正确性的核心断言：任何 kind 在相同 (data, market, own, rng) 下产出相同 Intents。
#[test]
fn decide_data_matches_legacy_trait_path() {
    let mv = one_stock_view(900);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    // 机构：旧 ValueStrategy vs 新 StrategyData。
    let legacy =
        ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100).unwrap();
    let legacy_intents = legacy.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    let data = StrategyData::inst(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100);
    let data_intents = decide_data(&data, &mv, &own, &mut SeqRng::new_f64(0.5));
    assert_eq!(
        serde_json::to_string(&legacy_intents).unwrap(),
        serde_json::to_string(&data_intents).unwrap()
    );
}
