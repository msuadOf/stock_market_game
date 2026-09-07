use engine::{
    AccountId, AuctionOrderSnap, Event, FloatAllocation, GameConfig, GameSession, HotParams,
    InstParams, Intent, Money, NpcSetup, RetailParams, SessionSetup, Side, StockCode, StockSpec,
    StrategyParams, TradingPhase, VParams,
};

fn auction_setup(auction_ticks: u64) -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("AUCTION".to_string()),
            initial_price: Money::from_cents(10_000),
            limit_pct: 0.10,
            v_initial: Money::from_cents(10_000),
            tick: Money::from_cents(1),
            float_shares: 10_000,
        }],
        npcs: NpcSetup {
            retail_count: 1,
            inst_count: 0,
            hot_count: 0,
            cash_per_npc: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(10_000),
            mean_reversion: 0.0,
            volatility: 0.0,
        },
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 1,
                chase_prob: 0.0,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 1,
            },
            hot: HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 1,
            },
        },
        player_cash: Money::from_cents(10_000_000),
        ticks_per_day: 10,
        auction_ticks,
        history_len: 10,
        t1_enabled: false,
        float_allocation: FloatAllocation::Random,
    }
}

fn web_default_auction_setup() -> SessionSetup {
    let stock = |code: &str, initial_price: i64, limit_pct: f64| StockSpec {
        code: StockCode(code.to_string()),
        initial_price: Money::from_cents(initial_price),
        limit_pct,
        v_initial: Money::from_cents(initial_price),
        tick: Money::from_cents(1),
        float_shares: 1_000_000,
    };
    SessionSetup {
        stocks: vec![
            stock("600101", 1_120, 0.10),
            stock("002156", 2_735, 0.10),
            stock("300260", 3_680, 0.10),
            stock("600610", 755, 0.10),
            stock("000812", 285, 0.05),
        ],
        npcs: NpcSetup {
            retail_count: 3,
            inst_count: 2,
            hot_count: 1,
            cash_per_npc: Money::from_cents(1_000_000_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(1_120),
            mean_reversion: 0.5,
            volatility: 0.02,
        },
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.3,
                order_size_mean: 200,
                chase_prob: 0.4,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 2_000,
            },
            hot: HotParams {
                lookback: 20,
                trend_threshold: 0.03,
                order_size: 1_000,
            },
        },
        player_cash: Money::from_cents(1_000_000_000),
        ticks_per_day: 15_300,
        auction_ticks: 900,
        history_len: 20,
        t1_enabled: false,
        float_allocation: FloatAllocation::Random,
    }
}

#[test]
fn web_default_session_keeps_real_auction_volume_after_day_one() {
    let mut session = GameSession::new(web_default_auction_setup(), 42).unwrap();
    let target = StockCode("002156".to_string());
    let mut completed_volumes = Vec::new();

    for _ in 0..(15_300 * 3) {
        for event in session.step() {
            if let Event::AuctionCompleted {
                code,
                matched_volume,
                ..
            } = event
            {
                if code == target {
                    completed_volumes.push(matched_volume);
                }
            }
        }
    }

    assert_eq!(completed_volumes.len(), 3);
    assert!(
        completed_volumes.iter().all(|volume| *volume > 0),
        "default NPC auction volume must remain non-zero across days, got {completed_volumes:?}"
    );
}

fn restored_with_orders(orders: Vec<AuctionOrderSnap>, auction_ticks: u64) -> GameSession {
    restored_with_previous_close(orders, auction_ticks, 10_000)
}

fn restored_with_previous_close(
    orders: Vec<AuctionOrderSnap>,
    auction_ticks: u64,
    previous_close: i64,
) -> GameSession {
    let session = GameSession::new(auction_setup(auction_ticks), 99).unwrap();
    let mut save = session.save();
    let market = save
        .snapshot
        .markets
        .get_mut(&StockCode("AUCTION".to_string()))
        .unwrap();
    market.last_close = Money::from_cents(previous_close);
    market.last_price = Money::from_cents(previous_close);
    save.auction_orders
        .insert(StockCode("AUCTION".to_string()), orders);
    save.next_order_id = 100;
    GameSession::restore(&save).unwrap()
}

fn order(owner: u64, side: Side, limit: i64, qty: u32, arrival_seq: u64) -> AuctionOrderSnap {
    AuctionOrderSnap {
        owner: AccountId(owner),
        side,
        limit: Money::from_cents(limit),
        qty,
        arrival_seq,
    }
}

#[test]
fn clearing_price_uses_deterministic_low_price_tie_break() {
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 10_200, 10, 1),
            order(1, Side::Sell, 9_800, 10, 2),
        ],
        2,
    );

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: Some(price),
            matched_volume: 10,
            imbalance: 0,
            ..
        } if *price == Money::from_cents(9_800)
    )));
}

#[test]
fn clearing_price_priority_is_volume_then_imbalance_then_previous_close() {
    let orders = vec![
        order(0, Side::Buy, 10_200, 10, 1),
        order(0, Side::Buy, 10_100, 10, 2),
        order(1, Side::Sell, 9_900, 10, 3),
        order(1, Side::Sell, 10_000, 10, 4),
        order(1, Side::Sell, 10_100, 5, 5),
    ];
    let mut session = restored_with_previous_close(orders, 2, 10_100);

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: Some(price),
            matched_volume: 20,
            imbalance: 0,
            ..
        } if *price == Money::from_cents(10_000)
    )));
}

#[test]
fn no_crossing_orders_publish_no_fake_indicative_price() {
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 9_900, 10, 1),
            order(1, Side::Sell, 10_100, 10, 2),
        ],
        2,
    );

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: None,
            matched_volume: 0,
            ..
        }
    )));
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::Trade { .. })));
}

#[test]
fn auction_only_trades_once_on_its_last_tick() {
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 10_200, 10, 1),
            order(1, Side::Sell, 10_100, 10, 2),
        ],
        2,
    );

    let first = session.step();
    assert!(first
        .iter()
        .all(|event| !matches!(event, Event::Trade { .. })));
    assert_eq!(session.snapshot().phase, TradingPhase::CallAuction);

    let second = session.step();
    assert_eq!(
        second
            .iter()
            .filter(|event| matches!(event, Event::Trade { .. }))
            .count(),
        1
    );
    assert!(second.iter().any(|event| matches!(
        event,
        Event::AuctionCompleted {
            opening_price: Some(price),
            matched_volume: 10,
            tick: 2,
            ..
        } if *price == Money::from_cents(10_100)
    )));
    assert_eq!(session.snapshot().phase, TradingPhase::Continuous);
}

#[test]
fn auction_trade_sets_real_open_and_volume_in_active_daily_candle() {
    let code = StockCode("AUCTION".to_string());
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 10_300, 25, 1),
            order(1, Side::Sell, 10_200, 25, 2),
        ],
        1,
    );

    let events = session.step();
    let snapshot = session.snapshot();
    let candle = &snapshot.active_daily_candles[&code];

    assert_eq!(
        snapshot.markets[&code].last_price,
        Money::from_cents(10_200)
    );
    assert_eq!(candle.open, Money::from_cents(10_200));
    assert_eq!(candle.high, Money::from_cents(10_200));
    assert_eq!(candle.low, Money::from_cents(10_200));
    assert_eq!(candle.close, Money::from_cents(10_200));
    assert_eq!(candle.volume, 25);
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::PriceTick { .. })));
}

#[test]
fn restoring_mid_auction_preserves_deterministic_completion() {
    let code = StockCode("AUCTION".to_string());
    let mut uninterrupted = GameSession::new(auction_setup(3), 123).unwrap();
    uninterrupted
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(10_100),
                qty: 15,
            },
        )
        .unwrap();
    uninterrupted.step();
    let save = uninterrupted.save();
    let mut restored = GameSession::restore(&save).unwrap();

    let expected: Vec<String> = uninterrupted
        .step()
        .into_iter()
        .chain(uninterrupted.step())
        .map(|event| format!("{event:?}"))
        .collect();
    let actual: Vec<String> = restored
        .step()
        .into_iter()
        .chain(restored.step())
        .map(|event| format!("{event:?}"))
        .collect();

    assert_eq!(actual, expected);
    assert_eq!(
        serde_json::to_value(restored.snapshot().markets).unwrap(),
        serde_json::to_value(uninterrupted.snapshot().markets).unwrap()
    );
}

#[test]
fn auction_is_deterministic_for_the_same_seed_and_intents() {
    let mut first = GameSession::new(auction_setup(2), 456).unwrap();
    let mut second = GameSession::new(auction_setup(2), 456).unwrap();
    let intent = Intent::PlaceLimit {
        code: StockCode("AUCTION".to_string()),
        side: Side::Buy,
        price: Money::from_cents(10_100),
        qty: 10,
    };
    first
        .enqueue_player_intent(AccountId(0), intent.clone())
        .unwrap();
    second.enqueue_player_intent(AccountId(0), intent).unwrap();

    for _ in 0..2 {
        assert_eq!(
            format!("{:?}", first.step()),
            format!("{:?}", second.step())
        );
    }
}

#[test]
fn auction_event_json_matches_frontend_contract() {
    let event = Event::AuctionTick {
        seq: 4,
        tick: 2,
        code: StockCode("AUCTION".to_string()),
        indicative_price: Some(Money::from_cents(10_123)),
        matched_volume: 50,
        imbalance: 7,
    };

    let json = serde_json::to_value(event).unwrap();
    let body = &json["AuctionTick"];
    assert_eq!(body["seq"], 4);
    assert_eq!(body["tick"], 2);
    assert_eq!(body["code"], "AUCTION");
    assert_eq!(body["indicative_price"], 10_123);
    assert_eq!(body["matched_volume"], 50);
    assert_eq!(body["imbalance"], 7);
}

#[test]
fn auction_reserves_cash_across_multiple_orders() {
    let mut setup = auction_setup(2);
    setup.player_cash = Money::from_cents(10_500);
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 9).unwrap();
    for _ in 0..2 {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: Money::from_cents(10_000),
                    qty: 1,
                },
            )
            .unwrap();
    }

    let events = session.step();

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                Event::IntentRejected {
                    account: AccountId(0),
                    reason: engine::RejectionReason::InsufficientCash,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn cancel_during_auction_is_explicitly_rejected() {
    let mut session = GameSession::new(auction_setup(2), 9).unwrap();
    let code = StockCode("AUCTION".to_string());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code,
                id: engine::OrderId(1),
            },
        )
        .unwrap();

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: engine::RejectionReason::AuctionOrderNotCancelable,
            ..
        }
    )));
}
