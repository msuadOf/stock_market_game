use engine::{
    AccountId, AuctionOrderSnap, Event, FloatAllocation, GameConfig, GameSession, HotParams,
    InstParams, Intent, Money, NpcSetup, RetailParams, SecurityCategory, SessionSetup, Side,
    StockCode, StockExchange, StockSpec, StrategyParams, TradingPhase, VParams,
};

fn auction_setup(auction_ticks: u64) -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600000".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(10_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            v_initial: Money::from_cents(10_000),
            tick: Money::from_cents(1),
            total_shares: 10_000,
            float_shares: 10_000,
        }],
        npcs: NpcSetup {
            retail_count: 1,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(10_000),
            mean_reversion: 0.0,
            volatility: 0.0,
        },
        fundamental_value_means: [(StockCode("600000".to_string()), Money::from_cents(10_000))]
            .into(),
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
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 10,
        auction_ticks,
        closing_auction_ticks: 0,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
    }
}

#[test]
fn closing_auction_has_a_distinct_phase_at_the_end_of_the_trading_day() {
    let mut setup = auction_setup(0);
    setup.closing_auction_ticks = 2;
    let mut session = GameSession::new(setup, 1).unwrap();

    for _ in 0..8 {
        session.step();
    }

    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
}

#[test]
fn closing_auction_accepts_limit_orders_then_expires_an_unmatched_remainder_at_day_end() {
    let code = StockCode("600000".to_string());
    let mut setup = auction_setup(0);
    setup.closing_auction_ticks = 2;
    let mut session = GameSession::new(setup, 2).unwrap();
    for _ in 0..8 {
        session.step();
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(9_900),
                qty: 100,
            },
        )
        .unwrap();

    let first = session.step();
    assert!(first.iter().any(|event| matches!(
        event,
        Event::AuctionTick { phase: TradingPhase::ClosingAuction, code: event_code, .. } if event_code == &code
    )));

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: engine::OrderId(1),
            },
        )
        .unwrap();

    let final_tick = session.step();
    assert!(final_tick.iter().any(|event| matches!(
        event,
        Event::AuctionCompleted {
            phase: TradingPhase::ClosingAuction,
            code: event_code,
            matched_volume: 0,
            ..
        } if event_code == &code
    )));
    assert!(final_tick.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: engine::RejectionReason::AuctionOrderNotCancelable,
            ..
        }
    )));
    assert!(final_tick.iter().any(|event| matches!(
        event,
        Event::OrderCanceled { account, code: event_code, remaining_qty: 100, .. }
            if *account == AccountId(0) && event_code == &code
    )));
    assert_eq!(session.snapshot().day, 1);
}

fn web_default_auction_setup() -> SessionSetup {
    let stock = |code: &str, initial_price: i64, category: SecurityCategory| StockSpec {
        code: StockCode(code.to_string()),
        exchange: if code.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(initial_price),
        category,
        limit_pct: category.limit_pct(),
        v_initial: Money::from_cents(initial_price),
        tick: Money::from_cents(1),
        total_shares: 1_000_000,
        float_shares: 1_000_000,
    };
    SessionSetup {
        stocks: vec![
            stock("600101", 1_120, SecurityCategory::MainBoard),
            stock("002156", 2_735, SecurityCategory::MainBoard),
            stock("300260", 3_680, SecurityCategory::ChiNext),
            stock("600610", 755, SecurityCategory::MainBoard),
            stock("000812", 285, SecurityCategory::StMainBoard),
        ],
        npcs: NpcSetup {
            // 与 Web 默认 10 倍 NPC 群体保持一致；该测试验证的是默认局，而非缩小样本局。
            retail_count: 30,
            inst_count: 20,
            hot_count: 10,
            retail_cash_median: Money::from_cents(1_000_000_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(1_120),
            mean_reversion: 0.5,
            volatility: 0.02,
        },
        fundamental_value_means: [
            (StockCode("600101".to_string()), Money::from_cents(1_120)),
            (StockCode("002156".to_string()), Money::from_cents(2_735)),
            (StockCode("300260".to_string()), Money::from_cents(3_680)),
            (StockCode("600610".to_string()), Money::from_cents(755)),
            (StockCode("000812".to_string()), Money::from_cents(285)),
        ]
        .into(),
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
        ticks_per_day: 15_300,
        auction_ticks: 900,
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
    }
}

#[test]
fn web_default_session_keeps_real_auction_activity_without_forcing_every_day_to_trade() {
    // 保留默认策略比例，把交易日等比例缩短到 1/10，避免用数万次 step 验证同一日界行为。
    let mut setup = web_default_auction_setup();
    setup.ticks_per_day = 1_530;
    setup.auction_ticks = 90;
    let ticks_per_day = setup.ticks_per_day;
    let mut session = GameSession::new(setup, 42).unwrap();
    let target = StockCode("002156".to_string());
    let mut completed_volumes = Vec::new();

    for _ in 0..(ticks_per_day * 3) {
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
    let active_days = completed_volumes
        .iter()
        .filter(|volume| **volume > 0)
        .count();
    assert!(
        active_days >= 2,
        "default NPC auction activity must persist beyond one day, while an uncrossed day may correctly have zero volume; got {completed_volumes:?}"
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
        .get_mut(&StockCode("600000".to_string()))
        .unwrap();
    market.last_close = Money::from_cents(previous_close);
    market.last_price = Money::from_cents(previous_close);
    save.auction_orders
        .insert(StockCode("600000".to_string()), orders);
    save.next_order_id = 100;
    GameSession::restore(&save).unwrap()
}

fn restored_on_exchange(
    orders: Vec<AuctionOrderSnap>,
    auction_ticks: u64,
    previous_close: i64,
    exchange: StockExchange,
) -> GameSession {
    let mut setup = auction_setup(auction_ticks);
    let code = StockCode(
        match exchange {
            StockExchange::Shanghai => "600000",
            StockExchange::Shenzhen => "000001",
        }
        .to_string(),
    );
    setup.stocks[0].code = code.clone();
    setup.stocks[0].exchange = exchange;
    setup.fundamental_value_means = [(code.clone(), Money::from_cents(10_000))].into();
    let session = GameSession::new(setup, 99).unwrap();
    let mut save = session.save();
    let market = save.snapshot.markets.get_mut(&code).unwrap();
    market.last_close = Money::from_cents(previous_close);
    market.last_price = Money::from_cents(previous_close);
    save.auction_orders.insert(code, orders);
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
fn shanghai_clearing_price_uses_the_midpoint_of_remaining_candidates() {
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 10_200, 200, 1),
            order(1, Side::Sell, 9_800, 200, 2),
        ],
        2,
    );

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: Some(price),
            matched_volume: 200,
            imbalance: 0,
            ..
        } if *price == Money::from_cents(10_000)
    )));
}

#[test]
fn shanghai_midpoint_is_rounded_half_up_to_the_price_tick() {
    let mut session = restored_on_exchange(
        vec![
            order(0, Side::Buy, 10_001, 200, 1),
            order(1, Side::Sell, 10_000, 200, 2),
        ],
        2,
        10_000,
        StockExchange::Shanghai,
    );

    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: Some(price),
            matched_volume: 200,
            ..
        } if *price == Money::from_cents(10_001)
    )));
}

#[test]
fn shanghai_and_shenzhen_apply_their_own_final_auction_tie_breaks() {
    let orders = vec![
        order(0, Side::Buy, 10_200, 300, 1),
        order(0, Side::Buy, 10_100, 100, 2),
        order(1, Side::Sell, 9_900, 100, 3),
        order(1, Side::Sell, 10_000, 300, 4),
        order(1, Side::Sell, 10_100, 100, 5),
    ];
    let mut shanghai = restored_on_exchange(orders.clone(), 2, 10_000, StockExchange::Shanghai);
    let mut shenzhen = restored_on_exchange(orders, 2, 10_000, StockExchange::Shenzhen);

    let indicative = |events: Vec<Event>| {
        events.into_iter().find_map(|event| match event {
            Event::AuctionTick {
                indicative_price, ..
            } => indicative_price,
            _ => None,
        })
    };
    assert_eq!(indicative(shanghai.step()), Some(Money::from_cents(10_000)));
    assert_eq!(indicative(shenzhen.step()), Some(Money::from_cents(10_100)));
}

#[test]
fn shenzhen_final_tie_break_chooses_the_candidate_nearest_previous_close() {
    let mut session = restored_on_exchange(
        vec![
            order(0, Side::Buy, 10_200, 200, 1),
            order(1, Side::Sell, 9_800, 200, 2),
        ],
        2,
        10_100,
        StockExchange::Shenzhen,
    );

    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: Some(price),
            matched_volume: 200,
            ..
        } if *price == Money::from_cents(10_200)
    )));
}

#[test]
fn shanghai_clearing_price_prioritizes_volume_then_unmatched_quantity() {
    let orders = vec![
        order(0, Side::Buy, 10_200, 200, 1),
        order(0, Side::Buy, 10_100, 200, 2),
        order(1, Side::Sell, 9_900, 200, 3),
        order(1, Side::Sell, 10_000, 200, 4),
        order(1, Side::Sell, 10_100, 100, 5),
    ];
    let mut session = restored_with_previous_close(orders, 2, 10_100);

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            indicative_price: Some(price),
            matched_volume: 400,
            imbalance: 0,
            ..
        } if *price == Money::from_cents(10_000)
    )));
}

#[test]
fn no_crossing_orders_publish_no_fake_indicative_price() {
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 9_900, 200, 1),
            order(1, Side::Sell, 10_100, 200, 2),
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
            order(0, Side::Buy, 10_200, 200, 1),
            order(1, Side::Sell, 10_100, 200, 2),
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
            clearing_price: Some(price),
            matched_volume: 200,
            tick: 2,
            ..
        } if *price == Money::from_cents(10_150)
    )));
    let indicative_price = second.iter().find_map(|event| match event {
        Event::AuctionTick {
            indicative_price, ..
        } => *indicative_price,
        _ => None,
    });
    let clearing_price = second.iter().find_map(|event| match event {
        Event::AuctionCompleted { clearing_price, .. } => *clearing_price,
        _ => None,
    });
    assert_eq!(indicative_price, clearing_price);
    assert_eq!(session.snapshot().phase, TradingPhase::Continuous);
}

#[test]
fn auction_trade_sets_real_open_and_volume_in_active_daily_candle() {
    let code = StockCode("600000".to_string());
    let mut session = restored_with_orders(
        vec![
            order(0, Side::Buy, 10_300, 500, 1),
            order(1, Side::Sell, 10_200, 500, 2),
        ],
        1,
    );

    let events = session.step();
    let snapshot = session.snapshot();
    let candle = &snapshot.active_daily_candles[&code];

    assert_eq!(
        snapshot.markets[&code].last_price,
        Money::from_cents(10_250)
    );
    assert_eq!(candle.open, Money::from_cents(10_250));
    assert_eq!(candle.high, Money::from_cents(10_250));
    assert_eq!(candle.low, Money::from_cents(10_250));
    assert_eq!(candle.close, Money::from_cents(10_250));
    assert_eq!(candle.volume, 500);
    let stats = candle.trade_stats.as_ref().unwrap();
    assert_eq!(stats.turnover_cents, 5_125_000);
    assert_eq!(stats.trade_count, 1);
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::PriceTick { .. })));
}

#[test]
fn preopen_save_requires_every_market_candle_after_auction_completion() {
    let code = StockCode("600000".to_string());
    let idle_code = StockCode("000001".to_string());
    let mut setup = auction_setup(3);
    let mut idle_stock = setup.stocks[0].clone();
    idle_stock.code = idle_code.clone();
    idle_stock.exchange = StockExchange::Shenzhen;
    setup.stocks.push(idle_stock);
    setup
        .fundamental_value_means
        .insert(idle_code.clone(), Money::from_cents(10_000));
    let mut save = GameSession::new(setup, 99).unwrap().save();
    save.auction_orders.insert(
        code.clone(),
        vec![
            order(0, Side::Buy, 10_300, 500, 1),
            order(1, Side::Sell, 10_200, 500, 2),
        ],
    );
    save.next_order_id = 100;
    let mut session = GameSession::restore(&save).unwrap();
    session.step();
    session.step();
    assert_eq!(session.snapshot().phase, TradingPhase::PreOpen);

    let complete = session.save();
    let restored = GameSession::restore(&complete).unwrap();

    assert_eq!(restored.snapshot().active_daily_candles[&code].volume, 500);
    assert_eq!(
        restored.snapshot().active_daily_candles[&code]
            .trade_stats
            .as_ref()
            .unwrap()
            .turnover_cents,
        5_125_000
    );
    assert_eq!(
        restored.snapshot().active_daily_candles[&code]
            .trade_stats
            .as_ref()
            .unwrap()
            .trade_count,
        1
    );
    assert_eq!(
        restored.snapshot().active_daily_candles[&idle_code].volume,
        0
    );

    let mut incomplete = complete;
    incomplete.snapshot.active_daily_candles.remove(&idle_code);
    assert!(matches!(
        GameSession::restore(&incomplete),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("active-candle")
    ));
}

#[test]
fn restoring_mid_auction_preserves_deterministic_completion() {
    let code = StockCode("600000".to_string());
    let mut uninterrupted = GameSession::new(auction_setup(3), 123).unwrap();
    uninterrupted
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(10_100),
                qty: 300,
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
        code: StockCode("600000".to_string()),
        side: Side::Buy,
        price: Money::from_cents(10_100),
        qty: 200,
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
        phase: TradingPhase::CallAuction,
        code: StockCode("600000".to_string()),
        indicative_price: Some(Money::from_cents(10_123)),
        matched_volume: 50,
        imbalance: 7,
    };

    let json = serde_json::to_value(event).unwrap();
    let body = &json["AuctionTick"];
    assert_eq!(body["seq"], 4);
    assert_eq!(body["tick"], 2);
    assert_eq!(body["code"], "600000");
    assert_eq!(body["indicative_price"], 10_123);
    assert_eq!(body["matched_volume"], 50);
    assert_eq!(body["imbalance"], 7);
}

#[test]
fn auction_reserves_cash_across_multiple_orders() {
    let mut setup = auction_setup(2);
    setup.config.starting_cash = Money::from_cents(1_000_510);
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
                    qty: 100,
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
fn auction_reserves_sellable_shares_across_orders_and_restore() {
    let code = StockCode("600000".to_string());
    let session = GameSession::new(auction_setup(3), 9).unwrap();
    let mut save = session.save();
    save.snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            engine::PositionSnap {
                qty: 100,
                t1_locked: 0,
                invested_cents: 1_000_000,
                recovered_cents: 0,
            },
        );
    let mut session = GameSession::restore(&save).unwrap();
    for _ in 0..2 {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: Money::from_cents(10_000),
                    qty: 100,
                },
            )
            .unwrap();
    }

    let events = session.step();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::OrderAccepted { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                Event::IntentRejected {
                    account: AccountId(0),
                    reason: engine::RejectionReason::InsufficientShares,
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        session.snapshot().accounts[&AccountId(0)].reserved_sell_qty[&code],
        100
    );

    let mut restored = GameSession::restore(&session.save()).unwrap();
    restored
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(10_000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(restored.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account: AccountId(0),
            reason: engine::RejectionReason::InsufficientShares,
            ..
        }
    )));
}

#[test]
fn cancel_during_auction_is_explicitly_rejected() {
    let mut session = GameSession::new(auction_setup(2), 9).unwrap();
    let code = StockCode("600000".to_string());
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

#[test]
fn auction_order_can_be_canceled_during_the_first_third() {
    let mut session = GameSession::new(auction_setup(3), 9).unwrap();
    let code = StockCode("600000".to_string());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(10_000),
                qty: 100,
            },
        )
        .unwrap();
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
        Event::OrderAccepted {
            id: engine::OrderId(1),
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::OrderCanceled {
            id: engine::OrderId(1),
            remaining_qty: 100,
            ..
        }
    )));
}

#[test]
fn orders_are_rejected_during_the_0925_to_0930_preopen_window() {
    let mut session = GameSession::new(auction_setup(3), 9).unwrap();
    let code = StockCode("600000".to_string());

    session.step();
    session.step();
    assert_eq!(session.snapshot().phase, TradingPhase::PreOpen);

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(10_000),
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: engine::RejectionReason::AuctionOrderEntryClosed,
            ..
        }
    )));
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::OrderAccepted { .. })));
    assert_eq!(session.snapshot().phase, TradingPhase::Continuous);
}

#[test]
fn unmatched_auction_limit_order_enters_the_continuous_book() {
    let code = StockCode("600000".to_string());
    let mut session = restored_with_orders(vec![order(0, Side::Buy, 9_900, 100, 1)], 3);

    session.step();
    let completed = session.step();

    assert!(completed.iter().any(|event| matches!(
        event,
        Event::AuctionCompleted {
            clearing_price: None,
            matched_volume: 0,
            tick: 2,
            ..
        }
    )));
    assert_eq!(session.snapshot().phase, TradingPhase::PreOpen);
    assert_eq!(
        session.snapshot().markets[&code].best_bid,
        Some(Money::from_cents(9_900))
    );
    let save = session.save();
    assert!(save.auction_orders.is_empty());
    let resting = &save.resting_orders[&code];
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].original_qty, 100);
    assert_eq!(resting[0].filled_qty, 0);
    assert_eq!(resting[0].qty, 100);
}
