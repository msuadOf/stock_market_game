use engine::{
    AccountId, AuctionOrderSnap, Event, FloatAllocation, GameConfig, GameSession, HotParams,
    InstParams, Intent, Money, NpcSetup, RetailParams, SecurityCategory, SessionSetup, Side,
    StockCode, StockExchange, StockSpec, StrategyParams, TradingPhase,
};

/// 场景配置与 seed 由 fixture 拥有；运行中的 Session 和断言仍归各测试。
struct AuctionFixture {
    setup: SessionSetup,
    seed: u64,
}

impl AuctionFixture {
    fn quiet(auction_ticks: u64, seed: u64) -> Self {
        let setup = SessionSetup {
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![StockSpec {
                code: StockCode("600000".to_string()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(10_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
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
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
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
            start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        };
        Self { setup, seed }
    }
    fn representative(seed: u64) -> Self {
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
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 1_000_000,
        };
        let setup = SessionSetup {
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![
                stock("600101", 1_120, SecurityCategory::MainBoard),
                stock("002156", 2_735, SecurityCategory::MainBoard),
                stock("300260", 3_680, SecurityCategory::ChiNext),
                stock("600610", 755, SecurityCategory::MainBoard),
                stock("000812", 285, SecurityCategory::StMainBoard),
            ],
            npcs: NpcSetup {
                retail_count: 30,
                inst_count: 20,
                hot_count: 10,
                retail_cash_median: Money::from_cents(1_000_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.3,
                    order_size_mean: 200,
                    chase_prob: 0.4,
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
            ticks_per_day: 10,
            auction_ticks: 2,
            closing_auction_ticks: 0,
            history_len: 20,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
            start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        };
        Self { setup, seed }
    }
    fn quiet_on_exchange(auction_ticks: u64, seed: u64, exchange: StockExchange) -> Self {
        let mut fixture = Self::quiet(auction_ticks, seed);
        fixture.setup.stocks[0].code = StockCode(
            match exchange {
                StockExchange::Shanghai => "600000",
                StockExchange::Shenzhen => "000001",
            }
            .to_string(),
        );
        fixture.setup.stocks[0].exchange = exchange;
        fixture
    }

    fn retain_stock(&mut self, code: &StockCode) {
        self.setup.stocks.retain(|stock| &stock.code == code);
        assert_eq!(self.setup.stocks.len(), 1, "fixture stock must exist");
    }

    fn start_session(&self) -> GameSession {
        GameSession::new(self.setup.clone(), self.seed).expect("auction fixture must be valid")
    }

    /// 使用场景首只证券身份注入竞价单，并同步 order id 与 runtime envelope。
    fn save_with_orders(
        &self,
        orders: Vec<AuctionOrderSnap>,
        previous_close: i64,
    ) -> engine::SaveSlot {
        let code = self.setup.stocks[0].code.clone();
        let mut save = self.start_session().save().expect("healthy save");
        let market = save
            .snapshot
            .markets
            .get_mut(&code)
            .expect("fixture market exists");
        market.last_close = Money::from_cents(previous_close);
        market.last_price = Money::from_cents(previous_close);
        save.auction_orders.insert(code, orders);
        save.next_order_id = 100;
        let mut envelopes = Vec::new();
        for (stock, orders) in &save.auction_orders {
            for order in orders {
                envelopes.push(engine::SavedLiveEnvelope {
                    key: engine::SavedEnvelopeKey {
                        account: order.owner,
                        stock: stock.clone(),
                        order: engine::OrderId(order.order_id),
                        side: order.side,
                    },
                    charged: engine::SavedFeeComponents::default(),
                });
            }
        }
        envelopes.sort_by(|left, right| left.key.cmp(&right.key));

        save.runtime_state.live_envelopes = envelopes;

        save
    }

    fn restore_with_orders(
        &self,
        orders: Vec<AuctionOrderSnap>,
        previous_close: i64,
    ) -> GameSession {
        GameSession::restore(&self.save_with_orders(orders, previous_close))
            .expect("injected auction save must restore")
    }
}

#[test]
fn auction_fixture_preserves_exchange_identity_and_order_envelope_keys() {
    for (exchange, code) in [
        (StockExchange::Shanghai, "600000"),
        (StockExchange::Shenzhen, "000001"),
    ] {
        let fixture = AuctionFixture::quiet_on_exchange(2, 99, exchange);
        assert_eq!(fixture.setup.stocks[0].exchange, exchange);
        assert_eq!(fixture.setup.stocks[0].code, StockCode(code.to_string()));
    }
    let fixture = AuctionFixture::quiet_on_exchange(2, 99, StockExchange::Shenzhen);
    let save = fixture.save_with_orders(
        vec![
            order(1, Side::Sell, 10_000, 100, 2),
            order(0, Side::Buy, 10_000, 100, 1),
        ],
        10_100,
    );
    let code = StockCode("000001".to_string());
    assert_eq!(
        save.snapshot.markets[&code].last_close,
        Money::from_cents(10_100)
    );
    let keys = save
        .runtime_state
        .live_envelopes
        .iter()
        .map(|envelope| envelope.key.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        vec![
            engine::SavedEnvelopeKey {
                account: AccountId(0),
                stock: code.clone(),
                order: engine::OrderId(1),
                side: Side::Buy
            },
            engine::SavedEnvelopeKey {
                account: AccountId(1),
                stock: code,
                order: engine::OrderId(2),
                side: Side::Sell
            },
        ]
    );
    assert_eq!(save.next_order_id, 100);
}

#[test]
fn closing_auction_has_a_distinct_phase_at_the_end_of_the_trading_day() {
    let mut fixture = AuctionFixture::quiet(0, 1);
    fixture.setup.closing_auction_ticks = 2;
    let mut session = fixture.start_session();

    for _ in 0..8 {
        session.step().expect("healthy step");
    }

    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
}

#[test]
fn closing_auction_accepts_limit_orders_then_expires_an_unmatched_remainder_at_day_end() {
    let code = StockCode("600000".to_string());
    let mut fixture = AuctionFixture::quiet(0, 2);
    fixture.setup.closing_auction_ticks = 2;
    let mut session = fixture.start_session();
    for _ in 0..8 {
        session.step().expect("healthy step");
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(9_900)),
                qty: 100,
            },
        )
        .unwrap();

    let first = session.step().expect("healthy step");
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

    let final_tick = session.step().expect("healthy step");
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

#[test]
fn single_stock_npc_population_keeps_real_auction_activity_across_days() {
    let mut fixture = AuctionFixture::representative(42);
    let target = StockCode("002156".to_string());
    fixture.retain_stock(&target);
    let ticks_per_day = fixture.setup.ticks_per_day;
    let mut session = fixture.start_session();
    let mut completed_volumes = Vec::new();

    for _ in 0..(ticks_per_day * 3) {
        for event in session.step().expect("healthy step") {
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
        "single-stock NPC auction activity must persist beyond one day, while an uncrossed day may correctly have zero volume; got {completed_volumes:?}"
    );
}

#[test]
fn multi_stock_npc_auctions_report_only_real_trades_across_days() {
    let fixture = AuctionFixture::representative(42);
    let codes = fixture
        .setup
        .stocks
        .iter()
        .map(|stock| stock.code.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let ticks_per_day = fixture.setup.ticks_per_day;
    let auction_ticks = fixture.setup.auction_ticks;
    let mut session = fixture.start_session();
    let mut completed_volumes = Vec::new();
    let mut zero_volume_completions = 0;

    for tick in 0..(ticks_per_day * 3) {
        let phase = session.phase();
        let events = session.step().expect("healthy step");
        let mut completed = std::collections::BTreeMap::new();
        for event in &events {
            if let Event::AuctionCompleted {
                code,
                phase: completed_phase,
                tick: completed_tick,
                clearing_price,
                matched_volume,
                ..
            } = event
            {
                assert_eq!(*completed_phase, TradingPhase::CallAuction);
                assert_eq!(*completed_phase, phase);
                assert_eq!(*completed_tick, tick + 1);
                assert!(completed
                    .insert(code.clone(), (*clearing_price, *matched_volume))
                    .is_none());
            }
        }
        if tick % ticks_per_day != auction_ticks - 1 {
            assert!(completed.is_empty());
            if phase == TradingPhase::CallAuction {
                assert!(!events
                    .iter()
                    .any(|event| matches!(event, Event::Trade { .. })));
            }
            continue;
        }

        assert_eq!(
            completed
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            codes
        );
        for code in &codes {
            let (clearing_price, matched_volume) = completed.get(code).unwrap();
            let mut real_volume = 0_u64;
            for event in &events {
                if let Event::Trade {
                    code: traded_code,
                    price,
                    qty,
                    maker,
                    taker,
                    ..
                } = event
                {
                    assert!(
                        codes.contains(traded_code),
                        "unknown traded stock {traded_code:?}"
                    );
                    if traded_code == code {
                        assert!(*qty > 0);
                        assert!(maker.0 > 0 && taker.0 > 0);
                        assert_eq!(Some(*price), *clearing_price);
                        real_volume += u64::from(*qty);
                    }
                }
            }
            assert_eq!(*matched_volume, real_volume, "tick {tick}, stock {code:?}");
            assert_eq!(clearing_price.is_some(), *matched_volume > 0);
            if *matched_volume == 0 {
                zero_volume_completions += 1;
            }
        }
        completed_volumes.push(completed.values().map(|(_, volume)| volume).sum::<u64>());
    }

    assert_eq!(completed_volumes.len(), 3);
    assert!(
        completed_volumes
            .iter()
            .filter(|volume| **volume > 0)
            .count()
            >= 2
    );
    assert!(zero_volume_completions > 0);
}

fn order(owner: u64, side: Side, limit: i64, qty: u32, order_id: u64) -> AuctionOrderSnap {
    AuctionOrderSnap {
        owner: AccountId(owner),
        side,
        limit: Money::from_cents(limit),
        qty,
        order_id,
    }
}

#[test]
fn auction_save_order_uses_order_id_and_rejects_the_old_field() {
    let saved = serde_json::to_value(order(0, Side::Buy, 10_000, 100, 7)).unwrap();
    assert_eq!(saved["order_id"], 7);
    assert!(saved.get("arrival_seq").is_none());

    let mut old = saved.clone();
    old.as_object_mut().unwrap().remove("order_id");
    old["arrival_seq"] = serde_json::json!(7);
    assert!(serde_json::from_value::<AuctionOrderSnap>(old).is_err());

    let mut mixed = saved;
    mixed["arrival_seq"] = serde_json::json!(7);
    assert!(serde_json::from_value::<AuctionOrderSnap>(mixed).is_err());
}

#[test]
fn shanghai_clearing_price_uses_the_midpoint_of_remaining_candidates() {
    let mut session = AuctionFixture::quiet(2, 99).restore_with_orders(
        vec![
            order(0, Side::Buy, 10_200, 200, 1),
            order(1, Side::Sell, 9_800, 200, 2),
        ],
        10_000,
    );

    let events = session.step().expect("healthy step");

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
    let mut session = AuctionFixture::quiet_on_exchange(2, 99, StockExchange::Shanghai)
        .restore_with_orders(
            vec![
                order(0, Side::Buy, 10_001, 200, 1),
                order(1, Side::Sell, 10_000, 200, 2),
            ],
            10_000,
        );

    assert!(session
        .step()
        .expect("healthy step")
        .iter()
        .any(|event| matches!(
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
    let mut shanghai = AuctionFixture::quiet_on_exchange(2, 99, StockExchange::Shanghai)
        .restore_with_orders(orders.clone(), 10_000);
    let mut shenzhen = AuctionFixture::quiet_on_exchange(2, 99, StockExchange::Shenzhen)
        .restore_with_orders(orders, 10_000);

    let indicative = |events: Vec<Event>| {
        events.into_iter().find_map(|event| match event {
            Event::AuctionTick {
                indicative_price, ..
            } => indicative_price,
            _ => None,
        })
    };
    assert_eq!(
        indicative(shanghai.step().expect("healthy step")),
        Some(Money::from_cents(10_000))
    );
    assert_eq!(
        indicative(shenzhen.step().expect("healthy step")),
        Some(Money::from_cents(10_100))
    );
}

#[test]
fn shenzhen_final_tie_break_chooses_the_candidate_nearest_previous_close() {
    let mut session = AuctionFixture::quiet_on_exchange(2, 99, StockExchange::Shenzhen)
        .restore_with_orders(
            vec![
                order(0, Side::Buy, 10_200, 200, 1),
                order(1, Side::Sell, 9_800, 200, 2),
            ],
            10_100,
        );

    assert!(session
        .step()
        .expect("healthy step")
        .iter()
        .any(|event| matches!(
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
    let mut session = AuctionFixture::quiet(2, 99).restore_with_orders(orders, 10_100);

    let events = session.step().expect("healthy step");

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
    let mut session = AuctionFixture::quiet(2, 99).restore_with_orders(
        vec![
            order(0, Side::Buy, 9_900, 200, 1),
            order(1, Side::Sell, 10_100, 200, 2),
        ],
        10_000,
    );

    let events = session.step().expect("healthy step");

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
    let mut session = AuctionFixture::quiet(2, 99).restore_with_orders(
        vec![
            order(0, Side::Buy, 10_200, 200, 1),
            order(1, Side::Sell, 10_100, 200, 2),
        ],
        10_000,
    );

    let first = session.step().expect("healthy step");
    assert!(first
        .iter()
        .all(|event| !matches!(event, Event::Trade { .. })));
    assert_eq!(session.snapshot().phase, TradingPhase::CallAuction);

    let second = session.step().expect("healthy step");
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
    let mut session = AuctionFixture::quiet(1, 99).restore_with_orders(
        vec![
            order(0, Side::Buy, 10_300, 500, 1),
            order(1, Side::Sell, 10_200, 500, 2),
        ],
        10_000,
    );

    let events = session.step().expect("healthy step");
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
    let mut fixture = AuctionFixture::quiet(3, 99);
    let mut idle_stock = fixture.setup.stocks[0].clone();
    idle_stock.code = idle_code.clone();
    idle_stock.exchange = StockExchange::Shenzhen;
    fixture.setup.stocks.push(idle_stock);
    let save = fixture.save_with_orders(
        vec![
            order(0, Side::Buy, 10_300, 500, 1),
            order(1, Side::Sell, 10_200, 500, 2),
        ],
        10_000,
    );
    let mut session = GameSession::restore(&save).unwrap();
    session.step().expect("healthy step");
    session.step().expect("healthy step");
    assert_eq!(session.snapshot().phase, TradingPhase::PreOpen);

    let complete = session.save().expect("healthy save");
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
    let mut uninterrupted = AuctionFixture::quiet(3, 123).start_session();
    uninterrupted
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(10_100)),
                qty: 300,
            },
        )
        .unwrap();
    uninterrupted.step().expect("healthy step");
    let save = uninterrupted.save().expect("healthy save");
    let mut restored = GameSession::restore(&save).unwrap();

    let expected: Vec<String> = uninterrupted
        .step()
        .expect("healthy step")
        .into_iter()
        .chain(uninterrupted.step().expect("healthy step"))
        .map(|event| format!("{event:?}"))
        .collect();
    let actual: Vec<String> = restored
        .step()
        .expect("healthy step")
        .into_iter()
        .chain(restored.step().expect("healthy step"))
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
    let mut first = AuctionFixture::quiet(2, 456).start_session();
    let mut second = AuctionFixture::quiet(2, 456).start_session();
    let intent = Intent::PlaceLimit {
        code: StockCode("600000".to_string()),
        side: Side::Buy,
        price: engine::LimitPrice::Fixed(Money::from_cents(10_100)),
        qty: 200,
    };
    first
        .enqueue_player_intent(AccountId(0), intent.clone())
        .unwrap();
    second.enqueue_player_intent(AccountId(0), intent).unwrap();

    for _ in 0..2 {
        assert_eq!(
            format!("{:?}", first.step().expect("healthy step")),
            format!("{:?}", second.step().expect("healthy step"))
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
    let mut fixture = AuctionFixture::quiet(2, 9);
    fixture.setup.config.starting_cash = Money::from_cents(1_000_510);
    let code = fixture.setup.stocks[0].code.clone();
    let mut session = fixture.start_session();
    for _ in 0..2 {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: engine::LimitPrice::Fixed(Money::from_cents(10_000)),
                    qty: 100,
                },
            )
            .unwrap();
    }

    let events = session.step().expect("healthy step");

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
    let session = AuctionFixture::quiet(3, 9).start_session();
    let mut save = session.save().expect("healthy save");
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
                    price: engine::LimitPrice::Fixed(Money::from_cents(10_000)),
                    qty: 100,
                },
            )
            .unwrap();
    }

    let events = session.step().expect("healthy step");
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

    let mut restored = GameSession::restore(&session.save().expect("healthy save")).unwrap();
    restored
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: engine::LimitPrice::Fixed(Money::from_cents(10_000)),
                qty: 100,
            },
        )
        .unwrap();
    assert!(restored
        .step()
        .expect("healthy step")
        .iter()
        .any(|event| matches!(
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
    let mut session = AuctionFixture::quiet(2, 9).start_session();
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

    let events = session.step().expect("healthy step");

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
    let mut session = AuctionFixture::quiet(6, 9).start_session();
    let code = StockCode("600000".to_string());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(10_000)),
                qty: 100,
            },
        )
        .unwrap();

    let placed = session.step().expect("healthy step");

    assert!(placed.iter().any(|event| matches!(
        event,
        Event::OrderAccepted {
            id: engine::OrderId(1),
            ..
        }
    )));
    let placed_save = session.save().expect("healthy save");
    let placed_orders = &placed_save.auction_orders[&code];
    assert_eq!(placed_orders.len(), 1);
    assert_eq!(placed_orders[0].order_id, 1);
    assert_eq!(placed_orders[0].qty, 100);

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: engine::OrderId(1),
            },
        )
        .unwrap();

    let events = session.step().expect("healthy step");

    assert!(events.iter().any(|event| matches!(
        event,
        Event::OrderCanceled {
            id: engine::OrderId(1),
            remaining_qty: 100,
            ..
        }
    )));
    let canceled_save = session.save().expect("healthy save");
    assert!(!canceled_save.auction_orders.contains_key(&code));
    assert_eq!(
        GameSession::restore(&canceled_save)
            .unwrap()
            .snapshot()
            .accounts[&AccountId(0)]
            .reserved_cash,
        Money::ZERO
    );
}

#[test]
fn same_tick_auction_place_and_cancel_releases_the_order() {
    let mut session = AuctionFixture::quiet(6, 9).start_session();
    let code = StockCode("600000".to_string());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(10_000)),
                qty: 100,
            },
        )
        .unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: engine::OrderId(1),
            },
        )
        .unwrap();

    let events = session.step().expect("healthy step");

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
    let save = session.save().expect("healthy save");
    assert!(!save.auction_orders.contains_key(&code));
    assert_eq!(
        GameSession::restore(&save).unwrap().snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO
    );
}

#[test]
fn orders_are_rejected_during_the_0925_to_0930_preopen_window() {
    let mut session = AuctionFixture::quiet(3, 9).start_session();
    let code = StockCode("600000".to_string());

    session.step().expect("healthy step");
    session.step().expect("healthy step");
    assert_eq!(session.snapshot().phase, TradingPhase::PreOpen);

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(10_000)),
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step().expect("healthy step");

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
    let mut session = AuctionFixture::quiet(3, 99)
        .restore_with_orders(vec![order(0, Side::Buy, 9_900, 100, 1)], 10_000);

    session.step().expect("healthy step");
    let completed = session.step().expect("healthy step");

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
    let save = session.save().expect("healthy save");
    assert!(save.auction_orders.is_empty());
    let resting = &save.resting_orders[&code];
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].original_qty, 100);
    assert_eq!(resting[0].filled_qty, 0);
    assert_eq!(resting[0].qty, 100);
}

#[test]
fn symbolic_highest_opening_quote_keeps_its_resolved_price_after_restore_and_rollover() {
    let code = StockCode("600000".to_owned());
    let mut session = AuctionFixture::quiet(3, 401).start_session();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Highest,
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step().unwrap();
    assert!(events.iter().any(|event| matches!(event,
        Event::OrderAccepted { price, .. } if *price == Money::from_cents(11_000))));
    let save = session.save().unwrap();
    assert_eq!(
        save.auction_orders[&code][0].limit,
        Money::from_cents(11_000)
    );
    let mut restored = GameSession::restore(&save).unwrap();
    for _ in 0..5 {
        if restored.phase() == TradingPhase::Continuous {
            break;
        }
        restored.step().unwrap();
    }
    assert_eq!(restored.phase(), TradingPhase::Continuous);
    assert_eq!(
        restored.snapshot().markets[&code].bids,
        vec![(Money::from_cents(11_000), 100)]
    );
}

#[test]
fn symbolic_lowest_closing_quote_expires_and_releases_its_cash_at_day_end() {
    let code = StockCode("600000".to_owned());
    let mut fixture = AuctionFixture::quiet(0, 402);
    fixture.setup.closing_auction_ticks = 2;
    let mut session = fixture.start_session();
    for _ in 0..8 {
        session.step().unwrap();
    }
    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Lowest,
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step().unwrap();
    assert!(events.iter().any(|event| matches!(event,
        Event::OrderAccepted { price, .. } if *price == Money::from_cents(9_000))));
    assert!(session.snapshot().accounts[&AccountId(0)].reserved_cash > Money::ZERO);
    let mut restored = GameSession::restore(&session.save().unwrap()).unwrap();
    restored.step().unwrap();
    assert_eq!(
        restored.snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO
    );
    assert!(restored.snapshot().markets[&code].bids.is_empty());
}
