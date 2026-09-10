//! engine session 模块集成测试（TDD 红绿循环）。
use engine::session::SplitMix64;
use engine::strategy::Rng;

#[test]
fn splitmix64_is_deterministic() {
    let mut a = SplitMix64::new(42);
    let mut b = SplitMix64::new(42);
    for _ in 0..10 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
    assert_ne!(
        SplitMix64::new(7).next_u64(),
        SplitMix64::new(42).next_u64()
    );
}

#[test]
fn splitmix64_next_f64_in_unit_range() {
    let mut r = SplitMix64::new(123);
    for _ in 0..100 {
        let x = r.next_f64();
        assert!((0.0..1.0).contains(&x), "next_f64 out of [0,1): {x}");
    }
}

#[test]
fn splitmix64_next_range_u32_in_range() {
    let mut r = SplitMix64::new(999);
    for _ in 0..100 {
        let v = r.next_range_u32(10, 20);
        assert!((10..20).contains(&v), "out of [10,20): {v}");
    }
    assert_eq!(r.next_range_u32(20, 20), 20); // lo>=hi → lo
}

use engine::account::StockCode;
use engine::money::Money;
use engine::orderbook::AccountId;
use engine::session::{
    Event, NpcSetup, RejectionReason, SecurityCategory, SessionSetup, Snapshot, StockExchange,
    StockSpec, TradingPhase,
};

fn sample_setup() -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            v_initial: Money::from_cents(1000),
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 1,
            hot_count: 1,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        v_params: engine::VParams {
            long_run_mean: Money::from_cents(1000),
            mean_reversion: 0.5,
            volatility: 0.0,
        },
        fundamental_value_means: [(StockCode("600101".to_string()), Money::from_cents(1000))]
            .into(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.5,
                order_size_mean: 100,
                chase_prob: 0.2,
                tick_cents: 1,
            },
            inst: engine::InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: engine::HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day: 10,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: engine::FloatAllocation::Random,
        start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
    }
}

#[test]
fn event_and_setup_construct() {
    let e = Event::IntentRejected {
        seq: 5,
        account: engine::AccountId(1),
        code: StockCode("600101".to_string()),
        reason: RejectionReason::InsufficientCash,
    };
    assert!(matches!(
        e,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    ));
    let snap = Snapshot {
        seq: 0,
        tick: 0,
        day: 0,
        phase: TradingPhase::Continuous,
        markets: Default::default(),
        accounts: Default::default(),
        daily_candles: Default::default(),
        active_daily_candles: Default::default(),
    };
    assert_eq!(snap.seq, 0);
    assert_eq!(sample_setup().stocks.len(), 1);
}

#[test]
fn formal_session_setup_enforces_a_share_baseline_and_category_limits() {
    let mut setup = sample_setup();
    setup.t1_enabled = false;
    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("T+1")
    ));

    let mut setup = sample_setup();
    setup.stocks[0].tick = Money::from_cents(2);
    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("0.01")
    ));

    let mut setup = sample_setup();
    setup.config.stamp_tax_rate = 0.001;
    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("0.0005")
    ));

    let mut setup = sample_setup();
    setup.config.st_limit = 0.05;
    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("st_limit=10%")
    ));

    let mut setup = sample_setup();
    setup.stocks[0].code = StockCode("300101".to_string());
    setup.stocks[0].exchange = StockExchange::Shenzhen;
    setup.stocks[0].category = SecurityCategory::ChiNext;
    setup.fundamental_value_means =
        [(StockCode("300101".to_string()), Money::from_cents(1000))].into();
    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("20%")
    ));
    setup.stocks[0].limit_pct = 0.20;
    assert!(setup.validate().is_ok());

    setup.stocks[0].code = StockCode("000101".to_string());
    setup.stocks[0].category = SecurityCategory::StMainBoard;
    setup.fundamental_value_means =
        [(StockCode("000101".to_string()), Money::from_cents(1000))].into();
    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("10%")
    ));
}

#[test]
fn chinext_enforces_limit_and_market_order_quantity_caps() {
    let mut setup = sample_setup();
    setup.stocks[0].code = StockCode("300101".to_string());
    setup.stocks[0].exchange = StockExchange::Shenzhen;
    setup.fundamental_value_means =
        [(StockCode("300101".to_string()), Money::from_cents(1000))].into();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.config.starting_cash = Money::from_cents(1_000_000_000);
    setup.stocks[0].category = SecurityCategory::ChiNext;
    setup.stocks[0].limit_pct = 0.20;
    let code = StockCode("300101".to_string());

    for (intent, should_reject) in [
        (
            engine::Intent::PlaceLimit {
                code: code.clone(),
                side: engine::Side::Buy,
                price: Money::from_cents(1000),
                qty: 300_000,
            },
            false,
        ),
        (
            engine::Intent::PlaceLimit {
                code: code.clone(),
                side: engine::Side::Buy,
                price: Money::from_cents(1000),
                qty: 300_100,
            },
            true,
        ),
        (
            engine::Intent::PlaceMarket {
                code: code.clone(),
                side: engine::Side::Buy,
                qty: 150_000,
            },
            false,
        ),
        (
            engine::Intent::PlaceMarket {
                code: code.clone(),
                side: engine::Side::Buy,
                qty: 150_100,
            },
            true,
        ),
    ] {
        let mut session = GameSession::new(setup.clone(), 42).unwrap();
        session.enqueue_player_intent(AccountId(0), intent).unwrap();
        let rejected = session.step().iter().any(|event| {
            matches!(
                event,
                Event::IntentRejected {
                    reason: RejectionReason::InvalidQuantity,
                    ..
                }
            )
        });
        assert_eq!(rejected, should_reject);
    }
}

#[test]
fn continuous_limit_orders_obey_102_and_98_percent_price_cages() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(100, 10_000_000);
    session
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code: code.clone(),
                side: engine::Side::Sell,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    session.step();

    session
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code: code.clone(),
                side: engine::Side::Buy,
                price: Money::from_cents(1021),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::PriceCageExceeded,
            ..
        }
    )));

    session
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code,
                side: engine::Side::Buy,
                price: Money::from_cents(1020),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session
        .step()
        .iter()
        .all(|event| !matches!(event, Event::IntentRejected { .. })));

    let mut session = player_session_with_position(200, 10_000_000);
    session
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code: StockCode("600101".to_string()),
                side: engine::Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    session.step();

    session
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code: StockCode("600101".to_string()),
                side: engine::Side::Sell,
                price: Money::from_cents(979),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::PriceCageExceeded,
            ..
        }
    )));

    session
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code: StockCode("600101".to_string()),
                side: engine::Side::Sell,
                price: Money::from_cents(980),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session
        .step()
        .iter()
        .all(|event| !matches!(event, Event::IntentRejected { .. })));
}

#[test]
fn current_setup_requires_authoritative_state_fields() {
    let setup = sample_setup();
    for required_field in ["auction_ticks", "fundamental_value_means"] {
        let mut value = serde_json::to_value(&setup).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove(required_field)
            .unwrap();
        assert!(
            serde_json::from_value::<SessionSetup>(value).is_err(),
            "current setup must reject a missing {required_field}"
        );
    }

    let mut missing_mean = sample_setup();
    missing_mean.fundamental_value_means.clear();
    assert!(matches!(
        GameSession::new(missing_mean, 7),
        Err(engine::SessionError::InvalidSetup(message)) if message.contains("mean market set")
    ));
}

#[test]
fn current_stock_specs_require_explicit_exchange_and_category() {
    let current = serde_json::to_value(&sample_setup().stocks[0]).unwrap();
    for required_field in ["exchange", "category"] {
        let mut missing = current.clone();
        missing
            .as_object_mut()
            .unwrap()
            .remove(required_field)
            .unwrap();
        assert!(
            serde_json::from_value::<StockSpec>(missing).is_err(),
            "current StockSpec must reject a missing {required_field}"
        );
    }
}

#[test]
fn current_save_rejects_invalid_rules_and_unowned_depth() {
    let mut invalid_rules = GameSession::new(sample_setup(), 42).unwrap().save();
    invalid_rules.setup.config.st_limit = 0.05;
    invalid_rules.setup.stocks[0].limit_pct = 0.05;
    assert!(matches!(
        GameSession::restore(&invalid_rules),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("10%")
    ));

    let mut unowned_depth = GameSession::new(sample_setup(), 42).unwrap().save();
    unowned_depth.resting_orders.clear();
    let market = unowned_depth
        .snapshot
        .markets
        .get_mut(&StockCode("600101".to_string()))
        .unwrap();
    market.best_bid = Some(Money::from_cents(1000));
    market.bids = vec![(Money::from_cents(1000), 100)];
    assert!(matches!(
        GameSession::restore(&unowned_depth),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("no restorable order ownership")
    ));
}

#[test]
fn current_save_rejects_non_player_pending_intents_and_inconsistent_market_depth() {
    let mut npc_pending = GameSession::new(sample_setup(), 42).unwrap().save();
    npc_pending.pending_player.push((
        AccountId(1),
        Intent::PlaceMarket {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            qty: 100,
        },
    ));
    assert!(matches!(
        GameSession::restore(&npc_pending),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("player account")
    ));

    let mut inconsistent = GameSession::new(sample_setup(), 42).unwrap().save();
    inconsistent
        .snapshot
        .markets
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .best_bid = Some(Money::from_cents(1_000));
    assert!(matches!(
        GameSession::restore(&inconsistent),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("best price")
    ));
}

#[test]
fn current_save_requires_complete_active_candles_only_after_continuous_trading_starts() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    assert!(GameSession::restore(&session.save()).is_ok());

    session.step();
    let mut missing = session.save();
    missing.snapshot.active_daily_candles.clear();
    assert!(matches!(
        GameSession::restore(&missing),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("active-candle")
    ));

    let mut unexpected = GameSession::new(sample_setup(), 42).unwrap().save();
    unexpected.snapshot.active_daily_candles.insert(
        StockCode("600101".to_string()),
        engine::DailyCandle {
            time: 0,
            open: Money::from_cents(1_000),
            high: Money::from_cents(1_000),
            low: Money::from_cents(1_000),
            close: Money::from_cents(1_000),
            volume: 0,
            trade_stats: Some(engine::DailyTradeStats::default()),
        },
    );
    assert!(matches!(
        GameSession::restore(&unexpected),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("active-candle")
    ));
}
#[test]
fn current_save_json_requires_explicit_stock_fields() {
    let save = GameSession::new(sample_setup(), 42).unwrap().save();
    let current = serde_json::to_value(save).unwrap();
    for required_field in ["exchange", "category"] {
        let mut missing = current.clone();
        missing["setup"]["stocks"][0]
            .as_object_mut()
            .unwrap()
            .remove(required_field)
            .unwrap();
        assert!(
            serde_json::from_value::<engine::SaveSlot>(missing).is_err(),
            "current save must reject a missing {required_field}"
        );
    }

    for required_field in [
        "price_history",
        "market_minute_closes",
        "rng_state",
        "npc_attention",
        "strategy_profiles",
        "resting_orders",
    ] {
        let mut missing = current.clone();
        missing
            .as_object_mut()
            .unwrap()
            .remove(required_field)
            .unwrap();
        assert!(
            serde_json::from_value::<engine::SaveSlot>(missing).is_err(),
            "current save must reject a missing {required_field}"
        );
    }

    let mut obsolete = current;
    obsolete["schema_version"] = serde_json::json!(1);
    assert!(
        serde_json::from_value::<engine::SaveSlot>(obsolete).is_err(),
        "obsolete versioned saves must be rejected"
    );

    let current =
        serde_json::to_value(GameSession::new(sample_setup(), 42).unwrap().save()).unwrap();
    for required_field in ["reserved_cash", "reserved_sell_qty"] {
        let mut missing = current.clone();
        missing["snapshot"]["accounts"]["0"]
            .as_object_mut()
            .unwrap()
            .remove(required_field)
            .unwrap();
        assert!(
            serde_json::from_value::<engine::SaveSlot>(missing).is_err(),
            "current save must reject a missing account {required_field}"
        );
    }
}

#[test]
fn restore_rejects_mismatched_npc_strategy_profile() {
    let mut save = GameSession::new(sample_setup(), 42).unwrap().save();
    let original_retail_style = match save.strategy_profiles[&AccountId(1)] {
        engine::strategy::StrategyProfile::Retail(style) => style,
        ref profile => panic!("account 1 must be retail, got {profile:?}"),
    };
    let different_retail_style = match original_retail_style {
        engine::strategy::RetailStyle::Dormant => engine::strategy::RetailStyle::LongTerm,
        _ => engine::strategy::RetailStyle::Dormant,
    };
    let mut changed_retail_style = save.clone();
    changed_retail_style.strategy_profiles.insert(
        AccountId(1),
        engine::strategy::StrategyProfile::Retail(different_retail_style),
    );
    assert!(matches!(
        GameSession::restore(&changed_retail_style),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("strategy profiles")
    ));

    save.strategy_profiles.insert(
        AccountId(1),
        engine::strategy::StrategyProfile::Institution(
            engine::strategy::InstitutionStyle::DeepValue,
        ),
    );

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("strategy profiles")
    ));
}

#[test]
fn saved_profiles_cover_each_npc_kind_and_rebuild_exactly() {
    let save = GameSession::new(sample_setup(), 42).unwrap().save();
    assert_eq!(save.strategy_profiles.len(), 4);
    assert!(matches!(
        save.strategy_profiles.get(&AccountId(1)),
        Some(engine::strategy::StrategyProfile::Retail(_))
    ));
    assert!(matches!(
        save.strategy_profiles.get(&AccountId(2)),
        Some(engine::strategy::StrategyProfile::Retail(_))
    ));
    assert!(matches!(
        save.strategy_profiles.get(&AccountId(3)),
        Some(engine::strategy::StrategyProfile::Institution(_))
    ));
    assert!(matches!(
        save.strategy_profiles.get(&AccountId(4)),
        Some(engine::strategy::StrategyProfile::Hot(_))
    ));
    assert!(GameSession::restore(&save).is_ok());
}

#[test]
fn restore_rejects_missing_or_extra_npc_strategy_profiles() {
    let save = GameSession::new(sample_setup(), 42).unwrap().save();
    let mut missing = save.clone();
    missing.strategy_profiles.remove(&AccountId(1));
    assert!(matches!(
        GameSession::restore(&missing),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("strategy profile account set")
    ));

    let mut extra = save;
    extra.strategy_profiles.insert(
        AccountId(99),
        engine::strategy::StrategyProfile::Retail(engine::strategy::RetailStyle::Noise),
    );
    assert!(matches!(
        GameSession::restore(&extra),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("strategy profile account set")
    ));
}

#[test]
fn javascript_number_boundaries_reject_unsafe_u64_values() {
    let mut save = GameSession::new(sample_setup(), 42).unwrap().save();
    save.next_order_id = 9_007_199_254_740_992;
    assert!(serde_json::to_value(&save).is_err());

    let mut snapshot = GameSession::new(sample_setup(), 42).unwrap().snapshot();
    snapshot
        .markets
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .bids = vec![(Money::from_cents(1_000), 9_007_199_254_740_992)];
    assert!(serde_json::to_value(&snapshot).is_err());
}

#[test]
fn restore_rejects_missing_deterministic_state() {
    let save = GameSession::new(sample_setup(), 42).unwrap().save();

    let mut missing_history = save.clone();
    missing_history.price_history.clear();
    assert!(matches!(
        GameSession::restore(&missing_history),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("price-history")
    ));

    let mut missing_candles = save;
    missing_candles.snapshot.daily_candles.clear();
    assert!(matches!(
        GameSession::restore(&missing_candles),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("daily-candle")
    ));

    let mut continuous_setup = sample_setup();
    continuous_setup.auction_ticks = 0;
    let mut advanced = GameSession::new(continuous_setup, 42).unwrap();
    advanced.step();
    let mut truncated = advanced.save();
    truncated
        .price_history
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .pop();
    assert!(matches!(
        GameSession::restore(&truncated),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("price history")
    ));

    let mut truncated_candles = advanced.save();
    truncated_candles
        .snapshot
        .daily_candles
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .pop();
    assert!(matches!(
        GameSession::restore(&truncated_candles),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("expected 360")
    ));
}

#[test]
fn setup_rejects_an_exchange_that_conflicts_with_the_stock_code() {
    let mut setup = sample_setup();
    setup.stocks[0].exchange = StockExchange::Shenzhen;

    assert!(matches!(
        setup.validate(),
        Err(engine::SessionError::InvalidSetup(message))
            if message.contains("code belongs")
    ));
}

#[test]
fn session_rejects_auction_that_consumes_the_whole_day() {
    let mut setup = sample_setup();
    setup.auction_ticks = setup.ticks_per_day;

    let error = match GameSession::new(setup, 7) {
        Ok(_) => panic!("auction_ticks == ticks_per_day must be rejected"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("auction_ticks"));
}

#[test]
fn auction_does_not_emit_regular_price_ticks_and_rejects_market_orders() {
    let mut setup = sample_setup();
    setup.auction_ticks = 2;
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 7).unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 10,
            },
        )
        .unwrap();

    let events = session.step();

    assert_eq!(session.snapshot().phase, TradingPhase::CallAuction);
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::PriceTick { .. })));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::AuctionLimitOrderRequired,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AuctionTick {
            code: event_code,
            indicative_price: None,
            matched_volume: 0,
            imbalance: 0,
            ..
        } if event_code == &code
    )));
}

use engine::session::GameSession;

#[test]
fn session_new_constructs_markets_and_accounts() {
    let s = GameSession::new(sample_setup(), 42).unwrap();
    assert_eq!(s.market_count(), 1);
    assert_eq!(s.account_count(), 5); // 玩家1 + retail2 + inst1 + hot1
    assert!(!s.account(AccountId(0)).unwrap().has_strategy()); // 玩家 None
    assert!(s.account(AccountId(1)).unwrap().has_strategy()); // NPC 有
}

#[test]
fn npc_accounts_have_independent_wealth_and_kind_scale() {
    let s = GameSession::new(sample_setup(), 42).unwrap();
    let retail_a = s.account(AccountId(1)).unwrap().cash;
    let retail_b = s.account(AccountId(2)).unwrap().cash;
    let institution = s.account(AccountId(3)).unwrap().cash;
    let hot = s.account(AccountId(4)).unwrap().cash;

    assert_ne!(retail_a, retail_b, "两个自然人散户不应复制同一现金状态");
    assert!(institution > retail_a && institution > retail_b);
    assert!(hot > retail_a && hot > retail_b);
}

fn large_retail_account_setup(retail_count: u32) -> SessionSetup {
    let mut setup = sample_setup();
    // 压缩墙钟粒度但保留“每交易日”观察次数分布，使测试覆盖完整日终压力而不把
    // 15,300 个空秒级 PriceTick 的开销带入常规测试套件。
    setup.ticks_per_day = 300;
    setup.auction_ticks = 0;
    setup.npcs.retail_count = retail_count;
    setup.npcs.inst_count = 5;
    setup.npcs.hot_count = 2;
    setup.npcs.retail_cash_median = Money::from_cents(20_000_000);
    setup.float_allocation = engine::FloatAllocation::ByKind {
        retail: 0.45,
        inst: 0.53,
        hot: 0.02,
    };
    let template = setup.stocks[0].clone();
    setup.stocks = ["600101", "002156", "300260", "600610", "000812"]
        .into_iter()
        .map(|code| {
            let mut stock = template.clone();
            stock.code = StockCode(code.to_string());
            stock.exchange = if code.starts_with('6') {
                StockExchange::Shanghai
            } else {
                StockExchange::Shenzhen
            };
            stock.category = if code.starts_with("300") {
                SecurityCategory::ChiNext
            } else if code == "000812" {
                SecurityCategory::StMainBoard
            } else {
                SecurityCategory::MainBoard
            };
            stock.limit_pct = stock.category.limit_pct();
            stock.total_shares = 100_000_000;
            stock.float_shares = 80_000_000;
            stock
        })
        .collect();
    setup.fundamental_value_means = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), stock.v_initial))
        .collect();
    setup
}

fn twenty_thousand_account_setup() -> SessionSetup {
    large_retail_account_setup(20_000)
}

fn assert_large_population_roundtrip_and_complete_a_full_market_day(retail_count: u32) {
    let setup = large_retail_account_setup(retail_count);
    let ticks_per_day = setup.ticks_per_day;
    // 玩家 1 人 + 散户 + 5 家机构 + 2 个游资。这里刻意只按实际账户数计数，
    // 不能将任何自然人聚合为代表性主体。
    let expected_accounts = usize::try_from(retail_count).unwrap() + 8;
    let mut uninterrupted = GameSession::new(setup.clone(), 42).unwrap();
    assert_eq!(uninterrupted.account_count(), expected_accounts);
    assert_eq!(uninterrupted.snapshot().accounts.len(), 1);

    let initial_save = uninterrupted.save();
    assert_eq!(initial_save.snapshot.accounts.len(), expected_accounts);
    assert_eq!(initial_save.retail_experience.len(), retail_count as usize);
    assert_eq!(
        initial_save.npc_attention.len(),
        expected_accounts - 1,
        "每个 NPC 必须保留自己的可恢复注意力状态"
    );
    let initial_json = serde_json::to_vec(&initial_save).expect("大规模账户存档必须可序列化");
    let same_seed_json = serde_json::to_vec(&GameSession::new(setup, 42).unwrap().save())
        .expect("同 seed 对照存档必须可序列化");
    assert_eq!(initial_json, same_seed_json, "同 seed 必须逐户确定重建");

    let decoded: engine::SaveSlot =
        serde_json::from_slice(&initial_json).expect("完整 JSON 存档必须可反序列化");
    let initial_attention_mismatches: Vec<_> = initial_save
        .npc_attention
        .iter()
        .filter_map(|(id, state)| (decoded.npc_attention.get(id) != Some(state)).then_some(*id))
        .take(8)
        .collect();
    assert!(
        initial_attention_mismatches.is_empty(),
        "注意力基础概率必须无损跨越 JSON 存档边界；前几个差异账户：{initial_attention_mismatches:?}"
    );
    let mut restored = GameSession::restore(&decoded).expect("大规模账户 JSON 存档必须可恢复");
    assert_eq!(
        serde_json::to_value(restored.save()).unwrap(),
        serde_json::to_value(decoded).unwrap(),
        "恢复后逐户资产、库存和注意力状态必须保持一致"
    );

    let mut saw_day_boundary = false;
    let mut resource_limit_rejections = 0_u64;
    for tick in 0..ticks_per_day {
        let uninterrupted_events = uninterrupted.step();
        let restored_events = restored.step();
        assert_eq!(
            events_summary(&restored_events),
            events_summary(&uninterrupted_events),
            "恢复实例必须在第 {tick} 个 tick 重放不中断实例的事件"
        );
        for event in restored_events {
            if matches!(event, engine::Event::DayBoundary { day: 1, .. }) {
                saw_day_boundary = true;
            }
            if matches!(
                event,
                engine::Event::IntentRejected {
                    reason: engine::RejectionReason::ResourceLimitExceeded,
                    ..
                }
            ) {
                resource_limit_rejections += 1;
            }
        }
    }
    assert!(saw_day_boundary, "默认规模必须能完整推进一个交易日");
    assert_eq!(
        resource_limit_rejections, 0,
        "默认规模不应撞上 50,000 全局挂单上限"
    );
    let uninterrupted_final = uninterrupted.save();
    let restored_final = restored.save();
    let uninterrupted_final_json = serde_json::to_value(&uninterrupted_final).unwrap();
    let restored_final_json = serde_json::to_value(&restored_final).unwrap();
    let changed_fields: Vec<_> = [
        "setup",
        "seed",
        "snapshot",
        "auction_orders",
        "resting_orders",
        "price_history",
        "market_minute_closes",
        "rng_state",
        "npc_attention",
        "retail_experience",
        "parent_orders",
        "npc_order_lifecycles",
        "pending_player",
        "next_order_id",
    ]
    .into_iter()
    .filter(|field| uninterrupted_final_json[field] != restored_final_json[field])
    .collect();
    let changed_attention: Vec<_> = restored_final
        .npc_attention
        .iter()
        .filter_map(|(id, restored_state)| {
            (uninterrupted_final.npc_attention.get(id) != Some(restored_state)).then_some(*id)
        })
        .take(8)
        .collect();
    let first_attention_difference = changed_attention.first().map(|id| {
        (
            id,
            uninterrupted_final.npc_attention.get(id),
            restored_final.npc_attention.get(id),
        )
    });
    assert_eq!(
        changed_fields,
        Vec::<&str>::new(),
        "完整交易日后，恢复实例必须与不中断实例逐户保持完全相同的权威状态；前几个注意力差异账户：{changed_attention:?}，首个差异：{first_attention_difference:?}"
    );
    let completed_day_json =
        serde_json::to_vec(&uninterrupted_final).expect("日后完整存档必须可序列化");
    let completed_day_save: engine::SaveSlot =
        serde_json::from_slice(&completed_day_json).expect("日后完整 JSON 存档必须可反序列化");
    let reloaded = GameSession::restore(&completed_day_save).expect("日后完整 JSON 存档必须可恢复");
    assert_eq!(
        serde_json::to_value(reloaded.save()).unwrap(),
        serde_json::to_value(completed_day_save).unwrap(),
        "日界后的存档恢复也必须逐户保持一致"
    );
}

#[test]
fn twenty_thousand_individual_retailers_stay_inside_the_engine_boundary() {
    let setup = twenty_thousand_account_setup();
    let session = GameSession::new(setup, 42).unwrap();
    assert_eq!(session.account_count(), 20_008);
    assert_eq!(session.snapshot().accounts.len(), 1);
    assert_eq!(session.save().snapshot.accounts.len(), 20_008);
}

#[test]
#[ignore = "20,007-account full-day release-mode stress gate"]
fn twenty_thousand_accounts_roundtrip_and_complete_a_full_market_day() {
    assert_large_population_roundtrip_and_complete_a_full_market_day(20_000);
}

#[test]
#[ignore = "50,007-account full-day release-mode stress gate: cargo test -p engine --release --test session fifty_thousand_accounts_roundtrip_and_complete_a_full_market_day -- --ignored --nocapture"]
fn fifty_thousand_accounts_roundtrip_and_complete_a_full_market_day() {
    assert_large_population_roundtrip_and_complete_a_full_market_day(50_000);
}

#[test]
#[ignore = "100,007-account full-day release-mode stress gate: cargo test -p engine --release --test session one_hundred_thousand_accounts_roundtrip_and_complete_a_full_market_day -- --ignored --nocapture"]
fn one_hundred_thousand_accounts_roundtrip_and_complete_a_full_market_day() {
    assert_large_population_roundtrip_and_complete_a_full_market_day(100_000);
}

#[test]
#[ignore = "2 万散户经历状态成本探针：cargo test -p engine --release --test session twenty_thousand_retail_experience_cost_report -- --ignored --nocapture"]
fn twenty_thousand_retail_experience_cost_report() {
    let setup = twenty_thousand_account_setup();
    let ticks_per_day = setup.ticks_per_day;
    let mut session = GameSession::new(setup, 42).expect("2 万散户会话必须可创建");
    let initial_save = session.save();
    let serialization_started = std::time::Instant::now();
    let retail_json =
        serde_json::to_vec(&initial_save.retail_experience).expect("散户经历必须可序列化");
    let full_json = serde_json::to_vec(&initial_save).expect("完整存档必须可序列化");
    let serialization_elapsed = serialization_started.elapsed();
    let trading_day_started = std::time::Instant::now();
    for _ in 0..ticks_per_day {
        session.step();
    }
    let trading_day_elapsed = trading_day_started.elapsed();
    let final_save = session.save();
    assert_eq!(final_save.retail_experience.len(), 20_000);
    assert!(retail_json.len() <= full_json.len());
    eprintln!(
        "retail_experience_cost retail_accounts=20000 experience_bytes={} full_save_bytes={} serialization_ms={} full_day_step_ms={}",
        retail_json.len(),
        full_json.len(),
        serialization_elapsed.as_millis(),
        trading_day_elapsed.as_millis()
    );
}

#[test]
fn session_new_rejects_empty_stocks() {
    let mut setup = sample_setup();
    setup.stocks.clear();
    assert!(GameSession::new(setup, 42).is_err());
}

#[test]
fn session_revalidates_config_deserialized_without_constructor() {
    let mut setup = sample_setup();
    setup.config.lot_size = 0;
    assert!(matches!(
        GameSession::new(setup, 42),
        Err(engine::SessionError::Config(
            engine::ConfigError::InvalidLotSize(0)
        ))
    ));
}

#[test]
fn session_rejects_negative_external_cash_and_duplicate_stock_codes() {
    let mut negative_cash = sample_setup();
    negative_cash.config.starting_cash = Money::from_cents(-1);
    assert!(GameSession::new(negative_cash, 42).is_err());

    let mut duplicates = sample_setup();
    duplicates.stocks.push(duplicates.stocks[0].clone());
    assert!(GameSession::new(duplicates, 42).is_err());
}

#[test]
fn session_rejects_a_float_larger_than_total_shares() {
    let mut setup = sample_setup();
    setup.stocks[0].total_shares = 99;
    setup.stocks[0].float_shares = 100;
    assert!(matches!(
        GameSession::new(setup, 42),
        Err(engine::SessionError::InvalidSetup(reason))
            if reason.contains("float_shares") && reason.contains("total_shares")
    ));
}

#[test]
fn session_rejects_invalid_strategy_params_instead_of_disabling_npcs() {
    let mut setup = sample_setup();
    setup.strategy_params.retail.arrival_rate = -0.1;
    assert!(matches!(
        GameSession::new(setup, 42),
        Err(engine::SessionError::Strategy(_))
    ));
}

#[test]
fn player_snapshot_excludes_private_npc_accounts_but_save_keeps_them() {
    let s = GameSession::new(sample_setup(), 42).unwrap();
    let snap = s.snapshot();
    assert_eq!(snap.seq, 0);
    assert_eq!(snap.markets.len(), 1);
    let ms = snap.markets.get(&StockCode("600101".to_string())).unwrap();
    assert_eq!(ms.last_price.cents(), 1000);
    assert_eq!(ms.fundamental_value, None, "玩家快照不能泄露隐藏基本面 V");
    assert_eq!(
        s.save().snapshot.markets[&StockCode("600101".to_string())].fundamental_value,
        Some(Money::from_cents(1000)),
        "可信存档必须保留恢复所需的 V"
    );
    assert_eq!(snap.accounts.len(), 1);
    assert_eq!(s.save().snapshot.accounts.len(), 5);
    assert_eq!(
        snap.accounts.get(&AccountId(0)).unwrap().cash.cents(),
        10_000_000
    );
}

#[test]
fn new_session_generates_360_authoritative_daily_candles() {
    let setup = sample_setup();
    let code = setup.stocks[0].code.clone();
    let initial_price = setup.stocks[0].initial_price;
    let snapshot = GameSession::new(setup, 42).unwrap().snapshot();
    let candles = snapshot
        .daily_candles
        .get(&code)
        .expect("stock history must exist");

    assert_eq!(candles.len(), 360);
    assert_eq!(candles.last().unwrap().close, initial_price);
    assert!(candles.last().unwrap().time < 0);
    for (index, candle) in candles.iter().enumerate() {
        assert!(candle.low <= candle.open.min(candle.close));
        assert!(candle.high >= candle.open.max(candle.close));
        assert!(candle.low > Money::ZERO);
        assert!(candle.volume > 0);
        if index > 0 {
            assert!(candle.time > candles[index - 1].time);
        }
    }
}

#[test]
fn generated_daily_candles_are_seed_deterministic() {
    let first = GameSession::new(sample_setup(), 42)
        .unwrap()
        .save()
        .snapshot
        .daily_candles;
    let repeated = GameSession::new(sample_setup(), 42)
        .unwrap()
        .snapshot()
        .daily_candles;
    let different = GameSession::new(sample_setup(), 43)
        .unwrap()
        .snapshot()
        .daily_candles;

    assert_eq!(first, repeated);
    assert_ne!(first, different);
}

#[test]
fn day_boundary_commits_engine_owned_daily_candle() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    for _ in 0..10 {
        session.step();
    }
    let snapshot = session.snapshot();
    let code = StockCode("600101".to_string());
    let candles = snapshot.daily_candles.get(&code).unwrap();

    assert_eq!(
        candles.len(),
        360,
        "rolling history keeps the requested window"
    );
    assert_eq!(candles.last().unwrap().time, 0);
    assert!(snapshot.active_daily_candles.is_empty());
}

#[test]
fn restore_rejects_inconsistent_authoritative_daily_trade_statistics() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    session.step();
    let mut save = session.save();
    let candle = save
        .snapshot
        .active_daily_candles
        .get_mut(&StockCode("600101".to_string()))
        .unwrap();
    candle.volume = 100;
    candle.trade_stats = Some(engine::DailyTradeStats::default());

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("trade statistics")
    ));
}

#[test]
fn restore_rejects_daily_turnover_above_the_ohlc_volume_bound() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    session.step();
    let mut save = session.save();
    let candle = save
        .snapshot
        .active_daily_candles
        .get_mut(&StockCode("600101".to_string()))
        .unwrap();
    candle.volume = 100;
    candle.trade_stats = Some(engine::DailyTradeStats {
        turnover_cents: 100_001,
        trade_count: 1,
    });

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("trade statistics")
    ));
}

#[test]
fn restore_rejects_missing_daily_statistics_for_positive_volume() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    session.step();
    let mut save = session.save();
    let candle = save
        .snapshot
        .active_daily_candles
        .get_mut(&StockCode("600101".to_string()))
        .unwrap();
    candle.volume = 100;
    candle.trade_stats = None;

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("trade statistics")
    ));
}

#[test]
fn restore_rejects_daily_statistics_below_a_minimum_that_exceeds_u64() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    session.step();
    let mut save = session.save();
    let candle = save
        .snapshot
        .active_daily_candles
        .get_mut(&StockCode("600101".to_string()))
        .unwrap();
    candle.open = Money::from_cents(i64::MAX);
    candle.high = Money::from_cents(i64::MAX);
    candle.low = Money::from_cents(i64::MAX);
    candle.close = Money::from_cents(i64::MAX);
    candle.volume = 3;
    candle.trade_stats = Some(engine::DailyTradeStats {
        turnover_cents: u64::MAX,
        trade_count: 1,
    });

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("trade statistics")
    ));
}

#[test]
fn restore_accepts_a_wide_ohlc_upper_bound_without_u64_multiplication_overflow() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    session.step();
    let mut save = session.save();
    let candle = save
        .snapshot
        .active_daily_candles
        .get_mut(&StockCode("600101".to_string()))
        .unwrap();
    candle.high = Money::from_cents(i64::MAX);
    candle.volume = 3;
    candle.trade_stats = Some(engine::DailyTradeStats {
        turnover_cents: 3_000,
        trade_count: 3,
    });

    GameSession::restore(&save).expect("可表示的成交额不能因宽 OHLC 上界而被误拒绝");
}

#[test]
fn price_ticks_and_day_boundary_carry_authoritative_daily_candles() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    let code = StockCode("600101".to_string());
    let mut final_tick_candle = None;
    let mut closed_candle = None;

    for _ in 0..10 {
        for event in session.step() {
            match event {
                Event::PriceTick {
                    code: event_code,
                    tick,
                    last_price,
                    daily_candle,
                    ..
                } if event_code == code => {
                    assert_eq!(tick, session.snapshot().tick);
                    assert_eq!(daily_candle.time, 0);
                    assert_eq!(daily_candle.close, last_price);
                    final_tick_candle = Some(daily_candle);
                }
                Event::DayBoundary {
                    closed_daily_candles,
                    ..
                } => {
                    closed_candle = closed_daily_candles.get(&code).cloned();
                }
                _ => {}
            }
        }
    }

    assert_eq!(closed_candle, final_tick_candle);
    assert_eq!(
        session.snapshot().daily_candles[&code].last(),
        closed_candle.as_ref()
    );
}

#[test]
fn runtime_snapshot_keeps_current_day_statistics_without_copying_history() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    session.step();

    let runtime = session.runtime_snapshot();
    assert!(runtime.daily_candles.is_empty());
    assert!(!runtime.active_daily_candles.is_empty());
    assert!(runtime
        .active_daily_candles
        .values()
        .all(|candle| candle.trade_stats.is_some()));
}

#[test]
fn price_ticks_carry_current_top_five_order_book_depth() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    let code = StockCode("600101".to_string());

    let events = session.step();
    let snapshot = session.runtime_snapshot();
    let market = &snapshot.markets[&code];
    let (event_bids, event_asks) = events
        .iter()
        .find_map(|event| match event {
            Event::PriceTick {
                code: event_code,
                bids,
                asks,
                ..
            } if event_code == &code => Some((bids, asks)),
            _ => None,
        })
        .expect("selected stock must emit a PriceTick");

    assert_eq!(
        event_bids,
        &market.bids.iter().take(5).cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        event_asks,
        &market.asks.iter().take(5).cloned().collect::<Vec<_>>()
    );
}

// step() 核心循环测试（决策/路由/结算/V/日界/事件）。

fn seq_of(e: &Event) -> u64 {
    match e {
        Event::Trade { seq, .. }
        | Event::AuctionTick { seq, .. }
        | Event::AuctionCompleted { seq, .. }
        | Event::PriceTick { seq, .. }
        | Event::DayBoundary { seq, .. }
        | Event::IntentRejected { seq, .. }
        | Event::SettlementError { seq, .. }
        | Event::VError { seq, .. }
        | Event::OrderCanceled { seq, .. }
        | Event::OrderAccepted { seq, .. } => *seq,
    }
}
fn events_summary(ev: &[Event]) -> Vec<String> {
    ev.iter()
        .map(|e| match e {
            Event::Trade {
                code, price, qty, ..
            } => format!("T{}:{}:{}", code.0, price.cents(), qty),
            Event::AuctionTick {
                code,
                indicative_price,
                matched_volume,
                ..
            } => format!(
                "A{}:{:?}:{}",
                code.0,
                indicative_price.map(|price| price.cents()),
                matched_volume
            ),
            Event::AuctionCompleted {
                code,
                clearing_price,
                matched_volume,
                ..
            } => format!(
                "C{}:{:?}:{}",
                code.0,
                clearing_price.map(|price| price.cents()),
                matched_volume
            ),
            Event::PriceTick {
                code, last_price, ..
            } => format!("P{}:{}", code.0, last_price.cents()),
            Event::DayBoundary { day, .. } => format!("D{}", day),
            Event::IntentRejected { reason, .. } => format!("R{:?}", reason),
            Event::SettlementError { reason, .. } => format!("S{}", reason),
            Event::VError { reason, .. } => format!("V{}", reason),
            Event::OrderCanceled { id, .. } => format!("X{}", id.0),
            Event::OrderAccepted { id, .. } => format!("O{}", id.0),
        })
        .collect()
}

#[test]
fn step_produces_events_with_monotonic_seq() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let events = s.step();
    assert_eq!(s.tick(), 1);
    assert!(events.iter().any(|e| matches!(e, Event::PriceTick { .. })));
    let mut last = 0u64;
    for e in &events {
        let sq = seq_of(e);
        assert!(sq >= last);
        last = last.max(sq);
    }
}

#[test]
fn step_is_deterministic_same_seed() {
    let mut a = GameSession::new(sample_setup(), 42).unwrap();
    let mut b = GameSession::new(sample_setup(), 42).unwrap();
    for _ in 0..5 {
        assert_eq!(events_summary(&a.step()), events_summary(&b.step()));
    }
}

#[test]
fn step_npc_routes_undervalued_buy_intent() {
    // 机构在随机注意力真正观察到 last<V 后必下买单（ValueStrategy 决策本身无 RNG 依赖）。
    // 说明：在当前撮合/账户语义下，无人持初始仓位 → 卖盘恒空 → 首笔买单只能挂入簿
    // （无对手盘不能成交，见 orderbook "无对手盘时买单应直接挂入簿：无成交"）。
    // 故此处断言「机构低估意图被路由并挂入买盘」(best_bid 出现)，验证 决策→路由→orderbook 链路。
    // 真正的 Trade 需做市商初始仓位/卖盘注入（超出本任务范围，见 honestReport）。
    let mut setup = sample_setup();
    setup.stocks[0].initial_price = Money::from_cents(900);
    setup.stocks[0].v_initial = Money::from_cents(1000);
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 1,
        hot_count: 0,
        retail_cash_median: Money::from_cents(10_000_000),
    };
    let mut s = GameSession::new(setup, 42).unwrap();
    for _ in 0..200 {
        s.step();
        if s.snapshot().markets[&StockCode("600101".to_string())]
            .best_bid
            .is_some()
        {
            break;
        }
    }
    assert_eq!(
        s.snapshot()
            .markets
            .get(&StockCode("600101".to_string()))
            .unwrap()
            .best_bid,
        Some(Money::from_cents(900)),
        "机构低估买单应挂入买盘（best_bid=900）"
    );
}

#[test]
fn npc_reuses_its_working_quote_instead_of_accumulating_duplicates() {
    let mut setup = sample_setup();
    setup.stocks[0].initial_price = Money::from_cents(900);
    setup.stocks[0].v_initial = Money::from_cents(1_000);
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 1,
        hot_count: 0,
        retail_cash_median: Money::from_cents(10_000_000),
    };
    setup.ticks_per_day = 1_000;
    let mut session = GameSession::new(setup, 42).unwrap();

    let mut events = Vec::new();
    for _ in 0..200 {
        events.extend(session.step());
    }
    let save = session.save();
    let orders = save
        .resting_orders
        .get(&StockCode("600101".to_string()))
        .unwrap();

    assert_eq!(orders.len(), 1, "每个方向只应保留一张最新工作委托");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                Event::OrderAccepted {
                    account: AccountId(1),
                    ..
                }
            ))
            .count(),
        1,
        "多次随机观察后的相同报价只能首次入簿"
    );
    assert!(events.iter().all(|event| !matches!(
        event,
        Event::OrderCanceled {
            account: AccountId(1),
            ..
        }
    )));
}

#[test]
fn step_evolves_v() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let before = s
        .save()
        .snapshot
        .markets
        .get(&StockCode("600101".to_string()))
        .unwrap()
        .fundamental_value
        .unwrap()
        .cents();
    s.step();
    let after = s
        .save()
        .snapshot
        .markets
        .get(&StockCode("600101".to_string()))
        .unwrap()
        .fundamental_value
        .unwrap()
        .cents();
    assert_eq!(before, after); // V=mean=1000,volatility=0 → 不变
}

#[test]
fn each_stock_uses_its_own_initial_value_as_long_run_mean() {
    let mut setup = sample_setup();
    setup.stocks.push(StockSpec {
        code: StockCode("600102".to_string()),
        exchange: StockExchange::Shanghai,
        initial_price: Money::from_cents(2000),
        category: SecurityCategory::MainBoard,
        limit_pct: 0.10,
        v_initial: Money::from_cents(2000),
        tick: Money::from_cents(1),
        total_shares: 10_000_000,
        float_shares: 0,
    });
    setup
        .fundamental_value_means
        .insert(StockCode("600102".to_string()), Money::from_cents(2000));
    setup.v_params.long_run_mean = Money::from_cents(1000);
    setup.v_params.mean_reversion = 0.5;
    setup.v_params.volatility = 0.0;
    let mut session = GameSession::new(setup, 42).unwrap();

    session.step();

    assert_eq!(
        session.save().snapshot.markets[&StockCode("600102".to_string())].fundamental_value,
        Some(Money::from_cents(2000))
    );
}

#[test]
fn v_evolution_failure_is_emitted_as_an_event() {
    let mut setup = sample_setup();
    setup.v_params.mean_reversion = 2.0;
    setup.v_params.long_run_mean = Money::from_cents(1);
    setup.v_params.volatility = 0.0;
    setup
        .fundamental_value_means
        .insert(StockCode("600101".to_string()), Money::from_cents(1));
    let mut session = GameSession::new(setup, 42).unwrap();

    let events = session.step();

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::VError { .. })));
}

#[test]
fn step_day_boundary() {
    let mut setup = sample_setup();
    setup.ticks_per_day = 2;
    let mut s = GameSession::new(setup, 42).unwrap();
    s.step();
    let events = s.step();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::DayBoundary { day: 1, .. })));
    assert_eq!(s.day(), 1);
}

use engine::strategy::Intent;
use engine::Side;

#[test]
fn enqueue_player_intent_executes_in_step() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    s.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    s.step();
    // 玩家挂买单 → best_bid 出现
    assert!(s
        .snapshot()
        .markets
        .get(&StockCode("600101".to_string()))
        .unwrap()
        .best_bid
        .is_some());
}

#[test]
fn enqueue_unknown_player_errors() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let r = s.enqueue_player_intent(
        AccountId(999),
        Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    );
    assert!(r.is_err());
}

#[test]
fn pending_player_intents_have_an_explicit_resource_limit() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    let intent = Intent::PlaceMarket {
        code: StockCode("600101".to_string()),
        side: Side::Buy,
        qty: 100,
    };
    for _ in 0..engine::MAX_PENDING_PLAYER_INTENTS {
        session
            .enqueue_player_intent(AccountId(0), intent.clone())
            .unwrap();
    }
    assert!(matches!(
        session.enqueue_player_intent(AccountId(0), intent),
        Err(engine::SessionError::ResourceLimit(message)) if message.contains("pending")
    ));
}

#[test]
fn open_order_limit_rejects_one_more_order_and_cancel_releases_capacity() {
    let code = StockCode("600101".to_string());
    let session = player_session_with_position(0, 1_000_000_000);
    let mut save = session.save();
    let orders: Vec<engine::Order> = (0..engine::MAX_OPEN_ORDERS_PER_ACCOUNT)
        .map(|index| engine::Order {
            id: engine::OrderId(index as u64 + 1),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(0),
            seq: index as u64,
        })
        .collect();
    save.resting_orders.insert(code.clone(), orders);
    save.snapshot.markets.get_mut(&code).unwrap().best_bid = Some(Money::from_cents(1_000));
    save.snapshot.markets.get_mut(&code).unwrap().bids = vec![(
        Money::from_cents(1_000),
        (engine::MAX_OPEN_ORDERS_PER_ACCOUNT as u64) * 100,
    )];
    save.next_order_id = engine::MAX_OPEN_ORDERS_PER_ACCOUNT as u64 + 1;
    let mut session = GameSession::restore(&save).unwrap();

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: engine::RejectionReason::ResourceLimitExceeded,
            ..
        }
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
    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::OrderCanceled {
            id: engine::OrderId(1),
            ..
        }
    )));

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session
        .step()
        .iter()
        .any(|event| matches!(event, Event::OrderAccepted { .. })));
}

#[test]
fn enqueue_rejects_npc_account_instead_of_relabeling_it_as_player_zero() {
    let mut session = GameSession::new(sample_setup(), 42).unwrap();
    let error = session
        .enqueue_player_intent(
            AccountId(1),
            Intent::PlaceLimit {
                code: StockCode("600101".to_string()),
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        engine::SessionError::NotPlayer(AccountId(1))
    ));
}

#[test]
fn step_rejects_insufficient_cash_player_intent() {
    let mut setup = sample_setup();
    setup.config.starting_cash = Money::from_cents(500);
    let mut s = GameSession::new(setup, 42).unwrap();
    s.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    let events = s.step();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::IntentRejected { account, reason, .. }
        if *account == AccountId(0) && *reason == RejectionReason::InsufficientCash
    )));
    // 现金不足以买入 → 余额未变（仍为 500）
    assert_eq!(s.account(AccountId(0)).unwrap().cash.cents(), 500);
}

// seed_float Random 分配（筹码守恒/玩家0/确定性/成本开盘价）。
fn float_setup(float: u32) -> SessionSetup {
    let mut s = sample_setup(); // sample_setup float_shares=0
                                // A listed company always has positive total shares even when the tradable float is zero.
    s.stocks[0].total_shares = u64::from(float).max(1);
    s.stocks[0].float_shares = float;
    s
}

/// 通过已知 NPC id（1..=npc_count）求和持仓的辅助。account_count 含玩家(0)。
fn npc_total_qty(s: &GameSession, code: &StockCode) -> u32 {
    (1..s.account_count() as u64)
        .filter_map(|id| s.account(AccountId(id)))
        .map(|a| a.positions.get(code).map(|p| p.qty).unwrap_or(0))
        .sum()
}

#[test]
fn seed_float_random_conserves_chips() {
    let s = GameSession::new(float_setup(1_000_000), 42).unwrap();
    let total = npc_total_qty(&s, &StockCode("600101".to_string()));
    assert_eq!(total, 1_000_000, "筹码守恒：Σ NPC 持仓 == float_shares");
}

#[test]
fn by_kind_distribution_gives_individual_retailers_sparse_portfolios() {
    let mut setup = sample_setup();
    setup.npcs.retail_count = 100;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.float_allocation = engine::FloatAllocation::ByKind {
        retail: 1.0,
        inst: 0.0,
        hot: 0.0,
    };
    let codes = ["600101", "600102", "600103", "600104", "600105"];
    let template = setup.stocks[0].clone();
    setup.stocks = codes
        .iter()
        .map(|code| {
            let mut stock = template.clone();
            stock.code = StockCode((*code).to_string());
            stock.total_shares = 1_000_000;
            stock.float_shares = 1_000_000;
            stock
        })
        .collect();
    setup.fundamental_value_means = codes
        .iter()
        .map(|code| (StockCode((*code).to_string()), Money::from_cents(1_000)))
        .collect();

    let session = GameSession::new(setup, 0x5CA1_E001).unwrap();
    let position_counts: Vec<usize> = (1..=100)
        .map(|id| session.account(AccountId(id)).unwrap().positions.len())
        .collect();

    assert!(position_counts.iter().any(|count| *count < codes.len()));
    assert!(position_counts.iter().any(|count| *count > 1));
    for code in codes {
        let code = StockCode(code.to_string());
        assert_eq!(npc_total_qty(&session, &code), 1_000_000);
        let holders = (1..=100)
            .filter(|id| {
                session
                    .account(AccountId(*id))
                    .unwrap()
                    .positions
                    .contains_key(&code)
            })
            .count();
        assert!((20..=60).contains(&holders), "{code:?} holders={holders}");
    }
}

#[test]
fn seed_float_player_has_zero() {
    let s = GameSession::new(float_setup(1_000_000), 42).unwrap();
    let player = s.account(AccountId(0)).unwrap();
    assert!(player.positions.is_empty(), "玩家新进场 0 持仓");
}

#[test]
fn seed_float_deterministic() {
    let a = GameSession::new(float_setup(1_000_000), 42).unwrap();
    let b = GameSession::new(float_setup(1_000_000), 42).unwrap();
    for id in 1..a.account_count() as u64 {
        let qa = a
            .account(AccountId(id))
            .unwrap()
            .positions
            .get(&StockCode("600101".to_string()))
            .map(|p| p.qty)
            .unwrap_or(0);
        let qb = b
            .account(AccountId(id))
            .unwrap()
            .positions
            .get(&StockCode("600101".to_string()))
            .map(|p| p.qty)
            .unwrap_or(0);
        assert_eq!(qa, qb, "同种子同分配 (AccountId({}))", id);
    }
}

#[test]
fn seed_float_cost_is_initial_price() {
    let s = GameSession::new(float_setup(1_000_000), 42).unwrap();
    // 任一有持仓的 NPC：cost_price == initial_price(1000分)、t1_locked==0
    let code = StockCode("600101".to_string());
    let holder = (1..s.account_count() as u64)
        .filter_map(|id| s.account(AccountId(id)))
        .find(|a| a.positions.contains_key(&code))
        .expect("至少一个 NPC 应持有仓位");
    let p = holder.positions.get(&code).unwrap();
    assert_eq!(p.cost_price().unwrap().cents(), 1000);
    assert_eq!(p.t1_locked, 0);
    assert_eq!(p.invested_cents, (p.qty as i64) * 1000);
}

#[test]
fn seed_float_zero_float_no_allocation() {
    let s = GameSession::new(float_setup(0), 42).unwrap(); // float_shares=0
    let total: u32 = (0..s.account_count() as u64)
        .filter_map(|id| s.account(AccountId(id)))
        .map(|a| a.positions.values().map(|p| p.qty).sum::<u32>())
        .sum();
    assert_eq!(total, 0, "float_shares==0 不分配（兼容加载存档路径）");
}

// seed_float ByKind 比例分配（比例/缺类守恒/非法比例校验）。

/// 按种类聚合 NPC（id!=0）对某股票的持仓。走公共 account() 访问器（accounts 字段私有）。
fn total_by_kind(s: &GameSession, code: &StockCode, kind: engine::AccountKind) -> u32 {
    (0..s.account_count() as u64)
        .filter_map(|id| s.account(AccountId(id)))
        .filter(|a| a.id.0 != 0 && a.kind == kind)
        .map(|a| a.positions.get(code).map(|p| p.qty).unwrap_or(0))
        .sum()
}

#[test]
fn seed_float_bykind_ratios() {
    let mut s = sample_setup();
    s.stocks[0].total_shares = 1_000_000;
    s.stocks[0].float_shares = 1_000_000;
    s.float_allocation = engine::FloatAllocation::ByKind {
        retail: 0.2,
        inst: 0.5,
        hot: 0.3,
    };
    // sample_setup: retail2, inst1, hot1
    let sess = GameSession::new(s, 42).unwrap();
    let code = StockCode("600101".to_string());
    let retail_total = total_by_kind(&sess, &code, engine::AccountKind::Retail);
    let inst_total = total_by_kind(&sess, &code, engine::AccountKind::Inst);
    let hot_total = total_by_kind(&sess, &code, engine::AccountKind::Hot);
    // 比例 2:5:3，容差（整数取整）±5%
    assert!(
        (retail_total as f64 / 1_000_000.0 - 0.2).abs() < 0.05,
        "retail≈0.2 got {}",
        retail_total
    );
    assert!(
        (inst_total as f64 / 1_000_000.0 - 0.5).abs() < 0.05,
        "inst≈0.5 got {}",
        inst_total
    );
    assert!(
        (hot_total as f64 / 1_000_000.0 - 0.3).abs() < 0.05,
        "hot≈0.3 got {}",
        hot_total
    );
    assert_eq!(retail_total + inst_total + hot_total, 1_000_000); // 守恒
}

#[test]
fn seed_float_bykind_missing_kind_redistributes() {
    let mut s = sample_setup();
    s.stocks[0].total_shares = 1_000_000;
    s.stocks[0].float_shares = 1_000_000;
    s.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 1,
        hot_count: 1,
        retail_cash_median: Money::from_cents(10_000_000),
    };
    s.float_allocation = engine::FloatAllocation::ByKind {
        retail: 0.2,
        inst: 0.5,
        hot: 0.3,
    };
    // retail 0 个 → 其 0.2 分摊给 inst/hot（归一化后 inst:0.5/0.8、hot:0.3/0.8）
    let sess = GameSession::new(s, 42).unwrap();
    let total = npc_total_qty(&sess, &StockCode("600101".to_string()));
    assert_eq!(total, 1_000_000, "缺类仍守恒");
}

#[test]
fn seed_float_bykind_invalid_ratio_rejected() {
    let mut s = sample_setup();
    s.stocks[0].total_shares = 1_000_000;
    s.stocks[0].float_shares = 1_000_000;
    s.float_allocation = engine::FloatAllocation::ByKind {
        retail: -0.1,
        inst: 0.5,
        hot: 0.6,
    };
    assert!(GameSession::new(s, 42).is_err(), "负比例 → InvalidSetup");
}

#[test]
fn seed_float_bykind_rejects_zero_weight_for_every_existing_kind() {
    let mut setup = sample_setup();
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 1_000_000;
    setup.float_allocation = engine::FloatAllocation::ByKind {
        retail: 0.0,
        inst: 0.0,
        hot: 0.0,
    };

    let error = GameSession::new(setup, 42)
        .err()
        .expect("全零有效权重必须失败");
    assert!(error.to_string().contains("existing NPC kinds"));
}

#[test]
fn seed_float_bykind_rejects_weight_assigned_only_to_an_absent_kind() {
    let mut setup = sample_setup();
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 1_000_000;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.float_allocation = engine::FloatAllocation::ByKind {
        retail: 0.0,
        inst: 1.0,
        hot: 0.0,
    };

    let error = GameSession::new(setup, 42)
        .err()
        .expect("只给缺失类别权重必须失败");
    assert!(error.to_string().contains("existing NPC kinds"));
}

#[test]
fn seed_float_bykind_rejects_a_non_finite_sum_of_individually_finite_weights() {
    let mut setup = sample_setup();
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 1_000_000;
    setup.float_allocation = engine::FloatAllocation::ByKind {
        retail: f64::MAX,
        inst: f64::MAX,
        hot: 0.0,
    };

    let error = GameSession::new(setup, 42)
        .err()
        .expect("有限大权重的溢出和必须失败");
    assert!(error.to_string().contains("finite sum"));
}

#[test]
fn formal_session_rejects_zero_total_shares_even_when_float_is_zero() {
    let mut setup = sample_setup();
    setup.stocks[0].total_shares = 0;
    setup.stocks[0].float_shares = 0;

    let error = GameSession::new(setup, 42).err().expect("零总股本必须失败");
    assert!(error.to_string().contains("total_shares must be > 0"));
}

// 初始持仓市场转活集成测试（分配后 step 出 Trade）+ FloatAllocation 导出。

#[test]
fn allocated_market_produces_trades() {
    // 分配流通盘后，NPC 有持仓可卖 → 卖盘有货 → 跑若干 step 出现成交。
    let mut s = sample_setup();
    s.stocks[0].total_shares = 10_000_000;
    s.stocks[0].float_shares = 10_000_000; // 大流通盘，确保 NPC 都有仓
    s.float_allocation = engine::FloatAllocation::ByKind {
        retail: 0.3,
        inst: 0.4,
        hot: 0.3,
    };
    let mut sess = GameSession::new(s, 42).unwrap();
    let mut any_trade = false;
    for _ in 0..5_000 {
        for e in sess.step() {
            if matches!(e, engine::Event::Trade { .. }) {
                any_trade = true;
            }
        }
    }
    assert!(any_trade, "分配流通盘后市场应出现成交（缺口已修）");
}

/// 回归测试（修复「浏览器里只有 ST低价股(000812) 价格在动，其余 4 只不动」）：
/// 复刻前端 DEFAULT_SETUP 的 5 只股票，跑若干 step 后**每一只**都应出现成交。
///
/// 旧 bug：decide_retail 恒取 `first_key_value()`（字典序最小 = "000812"）→ 全部散户
/// 订单集中在这只 → 其余股票无散户流动性、无对手盘、价格不动。修复后散户均匀随机选股，
/// 所有股票都应被交易。确定性（种子固定）→ 失败可复现（铁律三）。
#[test]
fn all_stocks_produce_trades_multistock() {
    use std::collections::HashSet;

    // 复刻 apps/web/src/config/defaults.ts 的 5 只股票（initial_price=v_initial，分）。
    let mk = |code: &str, price: i64, category: SecurityCategory| StockSpec {
        code: StockCode(code.to_string()),
        exchange: if code.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(price),
        category,
        limit_pct: category.limit_pct(),
        v_initial: Money::from_cents(price),
        tick: Money::from_cents(1),
        total_shares: 1_000_000,
        float_shares: 1_000_000,
    };
    let mut setup = sample_setup();
    setup.stocks = vec![
        mk("600101", 1120, SecurityCategory::MainBoard),
        mk("002156", 2735, SecurityCategory::MainBoard),
        mk("300260", 3680, SecurityCategory::ChiNext),
        mk("600610", 755, SecurityCategory::MainBoard),
        mk("000812", 285, SecurityCategory::StMainBoard),
    ];
    setup.fundamental_value_means = [
        (StockCode("600101".to_string()), Money::from_cents(1120)),
        (StockCode("002156".to_string()), Money::from_cents(2735)),
        (StockCode("300260".to_string()), Money::from_cents(3680)),
        (StockCode("600610".to_string()), Money::from_cents(755)),
        (StockCode("000812".to_string()), Money::from_cents(285)),
    ]
    .into();
    // 与 defaults.ts 对齐的 60 NPC 配额 + 策略参数（散户 arrival 0.3、机构 margin 0.02、游资 lookback 20）。
    setup.npcs = NpcSetup {
        retail_count: 30,
        inst_count: 20,
        hot_count: 10,
        retail_cash_median: Money::from_cents(100_000_000),
    };
    setup.strategy_params = engine::StrategyParams {
        retail: engine::RetailParams {
            arrival_rate: 0.3,
            order_size_mean: 200,
            chase_prob: 0.4,
            tick_cents: 1,
        },
        inst: engine::InstParams {
            margin: 0.02,
            order_size: 2_000,
        },
        hot: engine::HotParams {
            lookback: 20,
            trend_threshold: 0.03,
            order_size: 1_000,
        },
    };
    setup.history_len = 20;

    let mut sess = GameSession::new(setup, 42).unwrap();
    let mut traded: HashSet<String> = HashSet::new();
    for _ in 0..400 {
        for e in sess.step() {
            if let engine::Event::Trade { code, .. } = e {
                traded.insert(code.0);
            }
        }
    }
    let all: HashSet<String> = ["600101", "002156", "300260", "600610", "000812"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        traded, all,
        "未全部成交（traded={:?}）——回归到「只有一只股票有成交」的 bug",
        traded
    );
}

#[test]
fn reexport_float_allocation() {
    use engine::FloatAllocation;
    let _: FloatAllocation = FloatAllocation::Random;
    let _: FloatAllocation = FloatAllocation::ByKind {
        retail: 0.2,
        inst: 0.5,
        hot: 0.3,
    };
}

// crate 根 re-export（engine::{GameSession,SessionSetup,SplitMix64,Event,Snapshot,SessionError}）。
#[test]
fn reexport_from_crate_root() {
    use engine::{Event, GameSession, SessionError, SessionSetup, Snapshot, SplitMix64};
    let _ = GameSession::new(sample_setup(), 42).unwrap();
    let _ = SplitMix64::new(1);
    // 确保所有 re-export 符号可命名（编译期校验）。
    let _: Option<SessionError> = None;
    let _: Option<Event> = None;
    let _: Option<SessionSetup> = None;
    let _: Option<Snapshot> = None;
}

#[test]
fn game_session_is_send() {
    // WS-1: GameSession 必须是 Send（含 Box<dyn Strategy + Send + Sync>），
    // 才能进多线程宿主（后端 actor-per-session、Tauri、未来联机）。
    fn assert_send<T: Send>() {}
    let s = GameSession::new(sample_setup(), 42).unwrap();
    assert_send::<GameSession>();
    // 实际跨线程 move + step（证明可在另一线程跑）
    std::thread::spawn(move || {
        let _ = s.tick();
        drop(s);
    })
    .join()
    .unwrap();
}

#[test]
fn snapshot_depth_empty_initially_and_populated_after_order() {
    // float_shares=0 → 无持仓 → step 无成交；初始盘口空
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let code = StockCode("600101".to_string());
    let snap0 = s.snapshot();
    let ms0 = snap0.markets.get(&code).unwrap();
    assert!(ms0.bids.is_empty() && ms0.asks.is_empty(), "初始盘口应为空");
    // 玩家挂买单 600101@10.00×100（无对手盘 → 进买盘）
    s.enqueue_player_intent(
        AccountId(0),
        engine::strategy::Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    s.step();
    let snap1 = s.snapshot();
    let ms1 = snap1.markets.get(&code).unwrap();
    assert!(!ms1.bids.is_empty(), "挂买单后买盘非空");
    // 玩家 1000 买单应在盘口（可能非最优价：ZiNoise NPC 会挂 best_bid+1=1001 抢到买一）。
    assert!(
        ms1.bids.iter().any(|(p, _)| p.cents() == 1000),
        "玩家 1000 买单应在盘口"
    );
}

fn player_session_with_position(qty: u32, cash: i64) -> GameSession {
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.config.starting_cash = Money::from_cents(cash);
    let session = GameSession::new(setup, 42).unwrap();
    let mut save = session.save();
    if qty > 0 {
        save.snapshot
            .accounts
            .get_mut(&AccountId(0))
            .unwrap()
            .positions
            .insert(
                StockCode("600101".to_string()),
                engine::PositionSnap {
                    qty,
                    t1_locked: 0,
                    invested_cents: i64::from(qty) * 1000,
                    recovered_cents: 0,
                },
            );
    }
    GameSession::restore(&save).unwrap()
}

#[test]
fn sell_order_is_rejected_when_cash_cannot_cover_fee_shortfall() {
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.config.starting_cash = Money::ZERO;
    setup.v_params.long_run_mean = Money::from_cents(1);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.stocks[0].v_initial = Money::from_cents(1);
    let code = setup.stocks[0].code.clone();
    let session = GameSession::new(setup, 42).unwrap();
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
                invested_cents: 100,
                recovered_cents: 0,
            },
        );
    let mut session = GameSession::restore(&save).unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1),
                qty: 100,
            },
        )
        .unwrap();

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: engine::RejectionReason::InsufficientCash,
            ..
        }
    )));
    assert!(session.snapshot().markets[&code].asks.is_empty());
}

#[test]
fn sell_order_reserves_fees_for_a_possible_small_partial_fill() {
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.config.starting_cash = Money::ZERO;
    setup.v_params.long_run_mean = Money::from_cents(1);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.stocks[0].v_initial = Money::from_cents(1);
    setup
        .fundamental_value_means
        .insert(setup.stocks[0].code.clone(), Money::from_cents(1));
    let code = setup.stocks[0].code.clone();
    let session = GameSession::new(setup, 42).unwrap();
    let mut save = session.save();
    save.snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            engine::PositionSnap {
                qty: 1_000,
                t1_locked: 0,
                invested_cents: 1_000,
                recovered_cents: 0,
            },
        );
    let mut session = GameSession::restore(&save).unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1),
                qty: 1_000,
            },
        )
        .unwrap();

    let events = session.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));
    assert!(session.snapshot().markets[&code].asks.is_empty());
}

#[test]
fn buy_and_sell_orders_share_one_cash_reservation_budget() {
    let code = StockCode("600101".to_string());
    for first_side in [Side::Buy, Side::Sell] {
        let mut setup = sample_setup();
        setup.npcs = NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        };
        setup.config.starting_cash = Money::from_cents(601);
        setup.v_params.long_run_mean = Money::from_cents(1);
        setup.stocks[0].initial_price = Money::from_cents(1);
        setup.stocks[0].v_initial = Money::from_cents(1);
        setup
            .fundamental_value_means
            .insert(code.clone(), Money::from_cents(1));
        let base = GameSession::new(setup, 42).unwrap();
        let mut save = base.save();
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
                    invested_cents: 100,
                    recovered_cents: 0,
                },
            );
        let mut session = GameSession::restore(&save).unwrap();
        let first_price = Money::from_cents(1);
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: first_side,
                    price: first_price,
                    qty: 100,
                },
            )
            .unwrap();
        assert!(session
            .step()
            .iter()
            .any(|event| matches!(event, Event::OrderAccepted { .. })));

        let second_side = match first_side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let second_price = Money::from_cents(1);
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: second_side,
                    price: second_price,
                    qty: 100,
                },
            )
            .unwrap();

        assert!(session.step().iter().any(|event| matches!(
            event,
            Event::IntentRejected {
                reason: RejectionReason::InsufficientCash,
                ..
            }
        )));
    }
}

fn session_with_resting_sellers(seller_count: u32, player_cash: i64) -> GameSession {
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: seller_count,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.config.starting_cash = Money::from_cents(player_cash);
    let session = GameSession::new(setup, 42).unwrap();
    let mut save = session.save();
    let code = StockCode("600101".to_string());
    let mut orders = Vec::new();
    for offset in 0..seller_count {
        let owner = AccountId(u64::from(offset) + 1);
        save.snapshot
            .accounts
            .get_mut(&owner)
            .unwrap()
            .positions
            .insert(
                code.clone(),
                engine::PositionSnap {
                    qty: 100,
                    t1_locked: 0,
                    invested_cents: 100_000,
                    recovered_cents: 0,
                },
            );
        save.retail_experience
            .get_mut(&owner)
            .unwrap()
            .initialize_holding(
                &code,
                Some(Money::from_cents(1_000)),
                Money::from_cents(1_000),
                0,
            )
            .unwrap();
        orders.push(engine::Order {
            id: engine::OrderId(u64::from(offset) + 1),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner,
            seq: u64::from(offset),
        });
    }
    save.resting_orders.insert(code.clone(), orders);
    // 这些用例只验证既有挂单的结算与费用；避免卖方 NPC 在同一 tick 重新观察后
    // 按策略撤掉测试夹具中的挂单。
    for attention in save.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = save.snapshot.tick + 10;
    }
    let market = save.snapshot.markets.get_mut(&code).unwrap();
    market.best_ask = (seller_count > 0).then_some(Money::from_cents(1_000));
    market.asks = (seller_count > 0)
        .then_some(vec![(
            Money::from_cents(1_000),
            u64::from(seller_count) * 100,
        )])
        .unwrap_or_default();
    save.next_order_id = u64::from(seller_count) + 1;
    GameSession::restore(&save).unwrap()
}

#[test]
fn continuous_multi_fill_charges_one_minimum_commission_per_account_batch() {
    let code = StockCode("600101".to_string());
    let mut session = session_with_resting_sellers(2, 200_502);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 200,
            },
        )
        .unwrap();

    let events = session.step();

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Trade { .. }))
            .count(),
        2
    );
    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::SettlementError { .. })));
    let snapshot = session.snapshot();
    assert_eq!(snapshot.active_daily_candles[&code].volume, 200);
    let stats = snapshot.active_daily_candles[&code]
        .trade_stats
        .as_ref()
        .unwrap();
    assert_eq!(stats.turnover_cents, 200_000);
    assert_eq!(stats.trade_count, 2);
    assert_eq!(session.account(AccountId(0)).unwrap().cash, Money::ZERO);
}

#[test]
fn daily_trade_turnover_uses_a_lossless_decimal_string_in_json() {
    let mut session = session_with_resting_sellers(1, 100_502);
    let code = StockCode("600101".to_string());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    session.step();

    let json = serde_json::to_value(session.snapshot()).unwrap();
    assert_eq!(
        json["active_daily_candles"]["600101"]["trade_stats"]["turnover_cents"],
        serde_json::Value::String("100000".to_string())
    );
    assert_eq!(
        json["active_daily_candles"]["600101"]["trade_stats"]["trade_count"],
        1
    );
}

#[test]
fn partial_fill_never_commits_an_under_reserved_buy_order() {
    let code = StockCode("600101".to_string());
    let mut session = session_with_resting_sellers(1, 200_502);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 200,
            },
        )
        .unwrap();

    session.step();

    let save = session.save();
    assert!(GameSession::restore(&save).is_ok());
}

#[test]
fn resting_maker_buy_split_across_later_takers_stays_fully_reserved() {
    let code = StockCode("600101".to_string());
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 1,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.config.starting_cash = Money::from_cents(500);
    let session = GameSession::new(setup, 42).unwrap();
    let mut save = session.save();
    save.snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            engine::PositionSnap {
                qty: 200,
                t1_locked: 0,
                invested_cents: 200_000,
                recovered_cents: 0,
            },
        );
    // 成交额 200_000 分 + 整张委托一份最低佣金 500 分 + 过户费 2 分。
    // 若错误地按两次 taker 调用重复收最低佣金，第二次成交会失败。
    save.snapshot.accounts.get_mut(&AccountId(1)).unwrap().cash = Money::from_cents(200_502);
    save.resting_orders
        .get_mut(&code)
        .unwrap()
        .push(engine::Order {
            id: engine::OrderId(1),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 200,
            original_qty: 200,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(1),
            seq: 0,
        });
    let market = save.snapshot.markets.get_mut(&code).unwrap();
    market.best_bid = Some(Money::from_cents(1_000));
    market.bids = vec![(Money::from_cents(1_000), 200)];
    save.next_order_id = 2;
    let mut session = GameSession::restore(&save).unwrap();

    for expected_remaining in [100, 0] {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            )
            .unwrap();
        let events = session.step();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Trade { .. }))
                .count(),
            1
        );
        assert!(events
            .iter()
            .all(|event| !matches!(event, Event::SettlementError { .. })));
        assert_eq!(
            session.snapshot().markets[&code]
                .bids
                .first()
                .map_or(0, |(_, qty)| *qty),
            expected_remaining
        );
        assert!(GameSession::restore(&session.save()).is_ok());
    }
    assert_eq!(session.account(AccountId(1)).unwrap().cash, Money::ZERO);
}

#[test]
fn resting_sell_orders_reserve_shares_at_acceptance() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(100, 10_000_000);
    for _ in 0..2 {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: Money::from_cents(1000),
                    qty: 100,
                },
            )
            .unwrap();
    }

    let events = session.step();
    let snapshot = session.snapshot();
    let asks = &snapshot.markets.get(&code).unwrap().asks;
    assert_eq!(asks, &vec![(Money::from_cents(1000), 100)]);
    assert_eq!(
        snapshot.accounts[&AccountId(0)].reserved_sell_qty[&code],
        100
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientShares,
            ..
        }
    )));

    let mut restored = GameSession::restore(&session.save()).unwrap();
    restored
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(restored.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientShares,
            ..
        }
    )));
    assert_eq!(
        restored.snapshot().accounts[&AccountId(0)].reserved_sell_qty[&code],
        100
    );
}

#[test]
fn resting_buy_orders_reserve_cash_at_acceptance() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(0, 100_501);
    for _ in 0..2 {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: Money::from_cents(1000),
                    qty: 100,
                },
            )
            .unwrap();
    }

    let events = session.step();
    let snapshot = session.snapshot();
    let bids = &snapshot.markets.get(&code).unwrap().bids;
    assert_eq!(bids, &vec![(Money::from_cents(1000), 100)]);
    assert_eq!(
        snapshot.accounts[&AccountId(0)].reserved_cash,
        Money::from_cents(100_501)
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));
}

#[test]
fn continuous_cancel_removes_order_and_releases_reserved_cash() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(0, 100_501);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    let accepted = session.step();
    let order_id = accepted
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("resting order must publish its id");

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: order_id,
            },
        )
        .unwrap();
    let canceled = session.step();

    assert!(canceled.iter().any(|event| matches!(
        event,
        Event::OrderCanceled { id, remaining_qty: 100, .. } if *id == order_id
    )));
    assert!(session
        .snapshot()
        .markets
        .get(&code)
        .unwrap()
        .bids
        .is_empty());
    assert_eq!(
        session.snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO
    );

    // 账户现金刚好够一单；若撤单没有释放预留，这张同额委托会被错误拒绝。
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session
        .step()
        .iter()
        .any(|event| matches!(event, Event::OrderAccepted { .. })));
}

#[test]
fn unfilled_market_order_never_rests_in_the_book() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(100, 10_000_000);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Sell,
                qty: 100,
            },
        )
        .unwrap();

    let events = session.step();

    assert!(events
        .iter()
        .all(|event| !matches!(event, Event::OrderAccepted { .. } | Event::Trade { .. })));
    assert!(session
        .snapshot()
        .markets
        .get(&code)
        .unwrap()
        .asks
        .is_empty());
}

#[test]
fn day_boundary_clears_daily_orders_and_releases_reservations() {
    let code = StockCode("600101".to_string());
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.config.starting_cash = Money::from_cents(100_501);
    setup.ticks_per_day = 2;
    let mut session = GameSession::new(setup, 42).unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    session.step();
    assert!(!session
        .snapshot()
        .markets
        .get(&code)
        .unwrap()
        .bids
        .is_empty());

    session.step();
    assert!(session
        .snapshot()
        .markets
        .get(&code)
        .unwrap()
        .bids
        .is_empty());

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(session
        .step()
        .iter()
        .any(|event| matches!(event, Event::OrderAccepted { .. })));
}

#[test]
fn a_share_buy_quantity_must_be_a_board_lot() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(0, 10_000_000);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 99,
            },
        )
        .unwrap();

    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InvalidQuantity,
            ..
        }
    )));
}

#[test]
fn a_share_odd_lot_sell_cannot_split_the_odd_lot_remainder() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(150, 10_000_000);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Sell,
                price: Money::from_cents(1000),
                qty: 25,
            },
        )
        .unwrap();

    assert!(session.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InvalidQuantity,
            ..
        }
    )));
}

#[test]
fn a_share_t1_locked_shares_unlock_at_the_day_boundary() {
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.t1_enabled = true;
    let mut session = GameSession::new(setup, 42).unwrap();
    let code = StockCode("600101".to_string());
    for _ in 0..9 {
        session.step();
    }
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
                t1_locked: 100,
                invested_cents: 100_000,
                recovered_cents: 0,
            },
        );
    let mut restored = GameSession::restore(&save).unwrap();

    restored.step();

    assert_eq!(
        restored.account(AccountId(0)).unwrap().sellable_qty(&code),
        100
    );
}

#[test]
fn save_restore_preserves_pending_player_intents() {
    let mut original = GameSession::new(sample_setup(), 42).unwrap();
    let code = StockCode("600101".to_string());
    original
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();

    let save = original.save();
    assert_eq!(save.pending_player.len(), 1);
    let mut restored = GameSession::restore(&save).unwrap();
    let events = restored.step();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::OrderAccepted { account, code: event_code, .. }
            if *account == AccountId(0) && event_code == &code
    )));
}

#[test]
fn save_restore_preserves_state() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    // 跑几步产生状态
    for _ in 0..20 {
        s.step();
    }
    let saved = s.save();
    // 验证存档有状态
    assert!(saved.snapshot.tick > 0, "tick should be > 0");
    // 恢复
    let s2 = GameSession::restore(&saved).unwrap();
    assert_eq!(s2.tick(), saved.snapshot.tick, "tick restored");
    assert_eq!(s2.day(), saved.snapshot.day, "day restored");
    // 验证账户现金一致
    let snap_acc = saved.snapshot.accounts.get(&AccountId(0)).unwrap();
    let restored_acc = s2.account(AccountId(0)).unwrap();
    assert_eq!(restored_acc.cash, snap_acc.cash, "player cash restored");
}

#[test]
fn save_restore_preserves_resting_orders_and_their_reservations() {
    let code = StockCode("600101".to_string());
    let mut session = player_session_with_position(0, 100_501);
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    let accepted = session.step();
    let original_id = accepted
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();

    let saved = session.save();
    let mut restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        restored.snapshot().markets.get(&code).unwrap().bids,
        vec![(Money::from_cents(1000), 100)]
    );

    restored
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    assert!(restored.step().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));

    restored
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: original_id,
            },
        )
        .unwrap();
    assert!(restored.step().iter().any(|event| matches!(
        event,
        Event::OrderCanceled { id, .. } if *id == original_id
    )));
}

#[test]
fn restore_rejects_corrupted_position_invariants() {
    let session = GameSession::new(sample_setup(), 42).unwrap();
    let mut save = session.save();
    save.snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            StockCode("600101".to_string()),
            engine::PositionSnap {
                qty: 100,
                t1_locked: 101,
                invested_cents: 100_000,
                recovered_cents: 0,
            },
        );

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(_))
    ));
}

#[test]
fn restore_rejects_missing_or_unknown_authoritative_entities() {
    let session = GameSession::new(sample_setup(), 42).unwrap();
    let mut missing_player = session.save();
    missing_player.snapshot.accounts.remove(&AccountId(0));
    assert!(matches!(
        GameSession::restore(&missing_player),
        Err(engine::SessionError::InvalidSave(_))
    ));

    let mut unknown_market = session.save();
    let market = unknown_market
        .snapshot
        .markets
        .values()
        .next()
        .unwrap()
        .clone();
    unknown_market
        .snapshot
        .markets
        .insert(StockCode("UNKNOWN".to_string()), market);
    assert!(matches!(
        GameSession::restore(&unknown_market),
        Err(engine::SessionError::InvalidSave(_))
    ));
}

#[test]
fn restore_rejects_non_positive_prices() {
    let session = GameSession::new(sample_setup(), 42).unwrap();
    let mut bad_price = session.save();
    bad_price
        .snapshot
        .markets
        .get_mut(&StockCode("600101".to_string()))
        .unwrap()
        .last_price = Money::ZERO;
    assert!(matches!(
        GameSession::restore(&bad_price),
        Err(engine::SessionError::InvalidSave(_))
    ));
}

#[test]
fn save_restore_preserves_rng_and_strategy_price_history() {
    let mut original = GameSession::new(sample_setup(), 42).unwrap();
    for _ in 0..7 {
        original.step();
    }
    let saved = original.save();
    assert!(!saved.price_history[&StockCode("600101".to_string())].is_empty());
    assert_ne!(saved.rng_state, 0);
    assert_eq!(saved.npc_attention.len(), 4);
    assert_eq!(
        saved.retail_experience.len(),
        usize::try_from(saved.setup.npcs.retail_count).unwrap()
    );
    assert_eq!(
        saved
            .npc_attention
            .values()
            .map(|state| state.rng_state)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "每个 NPC 必须持有独立注意力随机流"
    );

    let mut restored = GameSession::restore(&saved).unwrap();
    assert_eq!(restored.save().retail_experience, saved.retail_experience);
    for _ in 0..12 {
        assert_eq!(
            serde_json::to_value(original.step()).unwrap(),
            serde_json::to_value(restored.step()).unwrap(),
            "restored session diverged from uninterrupted session"
        );
    }
}

#[test]
fn save_restore_rebuilds_the_active_trader_institution_deterministically() {
    let mut setup = sample_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 5;
    setup.npcs.hot_count = 0;
    let mut original = GameSession::new(setup, 0xA01_5A01).unwrap();
    for _ in 0..25 {
        original.step();
    }
    let saved = original.save();
    assert_eq!(saved.npc_attention.len(), 5);
    let mut restored = GameSession::restore(&saved).unwrap();

    for _ in 0..40 {
        assert_eq!(
            serde_json::to_value(original.step()).unwrap(),
            serde_json::to_value(restored.step()).unwrap(),
            "restore must rebuild ordinal-4 ActiveTrader with the same strategy parameters"
        );
        assert_eq!(
            serde_json::to_value(original.save()).unwrap(),
            serde_json::to_value(restored.save()).unwrap(),
            "active-trader strategy reconstruction must preserve the authoritative continuation"
        );
    }
}

#[test]
fn retail_decision_diagnostics_are_not_authoritative_or_replay_state() {
    let mut original = GameSession::new(sample_setup(), 42).unwrap();
    for _ in 0..20 {
        original.step();
        if !original.last_retail_decisions().is_empty() {
            break;
        }
    }
    assert!(
        !original.last_retail_decisions().is_empty(),
        "test seed must produce at least one real retail decision sample"
    );

    let saved = original.save();
    let mut restored = GameSession::restore(&saved).unwrap();
    assert!(
        restored.last_retail_decisions().is_empty(),
        "a restored authoritative session must not deserialize old diagnostic samples"
    );
    for _ in 0..12 {
        assert_eq!(
            serde_json::to_value(original.step()).unwrap(),
            serde_json::to_value(restored.step()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(original.save()).unwrap(),
            serde_json::to_value(restored.save()).unwrap()
        );
    }
}

#[test]
fn session_persists_canonical_market_minutes_and_restores_their_observations() {
    let mut setup = sample_setup();
    setup.ticks_per_day = 240;
    setup.history_len = 40;
    let mut original = engine::GameSession::new(setup, 42).unwrap();
    for _ in 0..31 {
        original.step();
    }

    let saved = original.save();
    let code = StockCode("600101".to_string());
    let minutes = &saved.market_minute_closes[&code];
    assert_eq!(minutes.len(), 31);
    assert_eq!(minutes.first().unwrap().absolute_trading_minute, 0);
    assert_eq!(minutes.last().unwrap().absolute_trading_minute, 30);

    let before = original.market_price_path_observations().unwrap();
    assert!(before[&code].one_minute.return_ratio.is_some());
    assert!(before[&code].thirty_minute.return_ratio.is_some());
    assert!(before[&code].five_day.return_ratio.is_some());

    let restored = engine::GameSession::restore(&saved).unwrap();
    assert_eq!(
        before,
        restored.market_price_path_observations().unwrap(),
        "restoring a save must preserve every strategy-visible market-time observation"
    );
}

#[test]
fn session_intraday_return_requires_a_real_opening_trade_not_the_zero_volume_placeholder() {
    let mut setup = sample_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 240;
    setup.history_len = 40;
    let mut no_trade = engine::GameSession::new(setup, 42).unwrap();
    no_trade.step();
    let code = StockCode("600101".to_string());

    let unavailable = no_trade.market_price_path_observations().unwrap();
    assert!(
        unavailable[&code].intraday.return_ratio.is_none(),
        "a zero-volume previous-close placeholder is not an opening trade"
    );

    let mut traded_save = no_trade.save();
    let candle = traded_save
        .snapshot
        .active_daily_candles
        .get_mut(&code)
        .unwrap();
    candle.open = Money::from_cents(900);
    candle.low = Money::from_cents(900);
    candle.volume = 200;
    candle.trade_stats = Some(engine::DailyTradeStats {
        turnover_cents: 190_000,
        trade_count: 2,
    });
    let traded = engine::GameSession::restore(&traded_save).unwrap();
    let observation = traded.market_price_path_observations().unwrap();
    assert_eq!(observation[&code].intraday.return_ratio, Some(1.0 / 9.0));
}

#[test]
fn compressed_session_days_still_cover_every_completed_market_minute() {
    let mut session = engine::GameSession::new(sample_setup(), 42).unwrap();
    for _ in 0..3 {
        session.step();
    }
    let code = StockCode("600101".to_string());
    let saved = session.save();
    let minutes = &saved.market_minute_closes[&code];
    assert_eq!(
        minutes
            .iter()
            .map(|sample| sample.absolute_trading_minute)
            .collect::<Vec<_>>(),
        (0..72).collect::<Vec<_>>()
    );
    let observation = session.market_price_path_observations().unwrap();
    assert!(observation[&code].one_minute.return_ratio.is_some());
    assert!(observation[&code].thirty_minute.return_ratio.is_some());
    assert_eq!(observation[&code].thirty_minute.available_span, 30);
}

#[test]
fn restore_rejects_missing_invalid_or_future_market_minute_history() {
    let mut session = engine::GameSession::new(sample_setup(), 42).unwrap();
    session.step();
    let save = session.save();
    let code = StockCode("600101".to_string());

    let mut missing = save.clone();
    missing.market_minute_closes.clear();
    assert!(matches!(
        engine::GameSession::restore(&missing),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("market-minute")
    ));

    let mut duplicate = save.clone();
    let sample = duplicate.market_minute_closes[&code][0];
    duplicate
        .market_minute_closes
        .get_mut(&code)
        .unwrap()
        .push(sample);
    assert!(matches!(
        engine::GameSession::restore(&duplicate),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("strictly increasing")
    ));

    let mut non_positive = save.clone();
    non_positive.market_minute_closes.get_mut(&code).unwrap()[0].close = Money::ZERO;
    assert!(matches!(
        engine::GameSession::restore(&non_positive),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("positive")
    ));

    let mut future = save;
    future
        .market_minute_closes
        .get_mut(&code)
        .unwrap()
        .last_mut()
        .unwrap()
        .absolute_trading_minute = 24;
    assert!(matches!(
        engine::GameSession::restore(&future),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("future")
    ));
}

#[test]
fn restore_rejects_missing_invalid_or_stale_npc_attention_state() {
    let session = GameSession::new(sample_setup(), 42).unwrap();
    let save = session.save();

    let mut missing = save.clone();
    missing.npc_attention.remove(&AccountId(1));
    assert!(matches!(
        GameSession::restore(&missing),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("attention account set")
    ));

    let mut invalid_probability = save.clone();
    invalid_probability
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap()
        .base_probability = f64::NAN;
    assert!(matches!(
        GameSession::restore(&invalid_probability),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("attention probability")
    ));

    let mut changed_profile = save.clone();
    let profile = changed_profile
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap();
    profile.base_probability = (profile.base_probability + 0.01).min(1.0);
    assert!(matches!(
        GameSession::restore(&changed_profile),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("deterministic profile")
    ));

    let mut advanced = GameSession::new(sample_setup(), 42).unwrap();
    advanced.step();
    let mut stale = advanced.save();
    stale
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap()
        .next_attention_candidate_tick = 0;
    assert!(matches!(
        GameSession::restore(&stale),
        Err(engine::SessionError::InvalidSave(message)) if message.contains("precedes snapshot tick")
    ));
}

#[test]
fn restore_rejects_missing_or_corrupt_retail_experience_state() {
    let session = GameSession::new(sample_setup(), 42).unwrap();
    let save = session.save();
    let retail = AccountId(1);

    let mut missing = save.clone();
    missing.retail_experience.remove(&retail);
    assert!(matches!(
        GameSession::restore(&missing),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("experience account set")
    ));

    let mut invalid_equity = save.clone();
    let experience = invalid_equity.retail_experience.get_mut(&retail).unwrap();
    experience.reference_equity = Some(Money::from_cents(10));
    experience.peak_equity = Some(Money::from_cents(9));
    assert!(matches!(
        GameSession::restore(&invalid_equity),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("equity experience references")
    ));

    let held_code = save.snapshot.accounts[&retail]
        .positions
        .keys()
        .next()
        .cloned();
    if let Some(code) = held_code {
        let mut missing_holding = save;
        missing_holding
            .retail_experience
            .get_mut(&retail)
            .unwrap()
            .stocks
            .remove(&code);
        assert!(matches!(
            GameSession::restore(&missing_holding),
            Err(engine::SessionError::InvalidSave(message))
                if message.contains("missing experience for a held stock")
        ));

        let mut invalid_order = session.save();
        let experience = invalid_order.retail_experience.get_mut(&retail).unwrap();
        experience.stocks.insert(
            code,
            engine::RetailStockExperience {
                entry_reference_price: Some(Money::from_cents(1_000)),
                peak_price_since_entry: Some(Money::from_cents(1_000)),
                last_buy_order_id: Some(invalid_order.next_order_id),
                ..engine::RetailStockExperience::default()
            },
        );
        assert!(matches!(
            GameSession::restore(&invalid_order),
            Err(engine::SessionError::InvalidSave(message))
                if message.contains("invalid last buy order id")
        ));
    }
}

#[test]
fn restore_rejects_inconsistent_retail_experience_lifecycles() {
    let code = StockCode("600101".to_string());
    let retail = AccountId(1);
    let held_session = session_with_resting_sellers(1, 200_000);

    let mut adverse_without_buy = held_session.save();
    adverse_without_buy
        .retail_experience
        .get_mut(&retail)
        .unwrap()
        .stocks
        .get_mut(&code)
        .unwrap()
        .adverse_move_recorded = true;
    assert!(matches!(
        GameSession::restore(&adverse_without_buy),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("adversity without a buy identity")
    ));

    let mut peak_below_latest_buy = held_session.save();
    let stock = peak_below_latest_buy
        .retail_experience
        .get_mut(&retail)
        .unwrap()
        .stocks
        .get_mut(&code)
        .unwrap();
    stock.last_buy_price = Some(Money::from_cents(1_100));
    stock.last_buy_order_id = Some(1);
    assert!(matches!(
        GameSession::restore(&peak_below_latest_buy),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("peak below its latest buy")
    ));

    let mut held_without_entry = held_session.save();
    held_without_entry
        .retail_experience
        .get_mut(&retail)
        .unwrap()
        .stocks
        .get_mut(&code)
        .unwrap()
        .entry_reference_price = None;
    assert!(matches!(
        GameSession::restore(&held_without_entry),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("invalid active experience")
    ));

    let empty_session = GameSession::new(sample_setup(), 42).unwrap();
    let mut cooldown_without_sell = empty_session.save();
    cooldown_without_sell
        .retail_experience
        .get_mut(&retail)
        .unwrap()
        .stocks
        .insert(
            code,
            engine::RetailStockExperience {
                cooldown_until_market_minute: Some(engine::POST_EXIT_COOLDOWN_MINUTES),
                ..engine::RetailStockExperience::default()
            },
        );
    assert!(matches!(
        GameSession::restore(&cooldown_without_sell),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("inconsistent post-exit state")
    ));

    let mut observation_with_trade_fields = empty_session.save();
    observation_with_trade_fields.next_order_id = 2;
    observation_with_trade_fields
        .retail_experience
        .get_mut(&retail)
        .unwrap()
        .stocks
        .insert(
            StockCode("600101".to_string()),
            engine::RetailStockExperience {
                last_buy_price: Some(Money::from_cents(1_000)),
                ..engine::RetailStockExperience::default()
            },
        );
    let error = match GameSession::restore(&observation_with_trade_fields) {
        Ok(_) => panic!("observation-only state with a buy identity must be rejected"),
        Err(error) => error,
    };
    assert!(
        matches!(error, engine::SessionError::InvalidSave(ref message) if message.contains("invalid observation-only state")),
        "unexpected restore error: {error}"
    );
}

#[test]
fn restore_rejects_continuous_orders_during_call_auction() {
    let code = StockCode("600101".to_string());
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.auction_ticks = 3;
    let session = GameSession::new(setup, 42).unwrap();
    let mut save = session.save();
    save.resting_orders
        .get_mut(&code)
        .unwrap()
        .push(engine::Order {
            id: engine::OrderId(1),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(0),
            seq: 1,
        });
    save.snapshot.markets.get_mut(&code).unwrap().bids = vec![(Money::from_cents(1000), 100)];
    save.snapshot.markets.get_mut(&code).unwrap().best_bid = Some(Money::from_cents(1000));
    save.next_order_id = 2;

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("call-auction")
    ));
}

#[test]
fn restore_rejects_unreachable_t1_locks() {
    let code = StockCode("600101".to_string());

    let session = GameSession::new(sample_setup(), 42).unwrap();
    let mut t0_save = session.save();
    // 正式 A 股 setup 不允许通过存档把会话改成 T+0。
    t0_save.setup.t1_enabled = false;
    t0_save
        .snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            engine::PositionSnap {
                qty: 100,
                t1_locked: 100,
                invested_cents: 100_000,
                recovered_cents: 0,
            },
        );
    assert!(matches!(
        GameSession::restore(&t0_save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("formal A-share sessions require T+1 settlement")
    ));

    let mut auction_setup = sample_setup();
    auction_setup.t1_enabled = true;
    auction_setup.auction_ticks = 3;
    let session = GameSession::new(auction_setup, 42).unwrap();
    let mut auction_save = session.save();
    auction_save
        .snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(
            code,
            engine::PositionSnap {
                qty: 100,
                t1_locked: 100,
                invested_cents: 100_000,
                recovered_cents: 0,
            },
        );
    assert!(matches!(
        GameSession::restore(&auction_save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("unreachable T+1")
    ));
}

#[test]
fn restore_rejects_split_odd_lot_sell_orders() {
    let code = StockCode("600101".to_string());
    let session = player_session_with_position(150, 10_000_000);
    let mut save = session.save();
    save.resting_orders.insert(
        code.clone(),
        vec![
            engine::Order {
                id: engine::OrderId(1),
                side: Side::Sell,
                price: Money::from_cents(1000),
                qty: 25,
                original_qty: 25,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: AccountId(0),
                seq: 1,
            },
            engine::Order {
                id: engine::OrderId(2),
                side: Side::Sell,
                price: Money::from_cents(1000),
                qty: 25,
                original_qty: 25,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: AccountId(0),
                seq: 2,
            },
        ],
    );
    save.snapshot.markets.get_mut(&code).unwrap().asks = vec![(Money::from_cents(1000), 50)];
    save.snapshot.markets.get_mut(&code).unwrap().best_ask = Some(Money::from_cents(1000));
    save.next_order_id = 3;

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("odd-lot")
    ));
}

#[test]
fn restore_accepts_non_lot_remainders_after_a_real_partial_fill() {
    let code = StockCode("600101".to_string());
    let mut setup = sample_setup();
    setup.npcs = NpcSetup {
        retail_count: 1,
        inst_count: 0,
        hot_count: 0,
        retail_cash_median: Money::ZERO,
    };
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.config.starting_cash = Money::from_cents(10_000_000);
    let base = GameSession::new(setup, 42).unwrap();
    let mut initial_save = base.save();
    initial_save
        .snapshot
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .positions
        .insert(
            code.clone(),
            engine::PositionSnap {
                qty: 25,
                t1_locked: 0,
                invested_cents: 25_000,
                recovered_cents: 0,
            },
        );
    initial_save
        .retail_experience
        .get_mut(&AccountId(1))
        .unwrap()
        .initialize_holding(
            &code,
            Some(Money::from_cents(1_000)),
            Money::from_cents(1_000),
            0,
        )
        .unwrap();
    initial_save.resting_orders.insert(
        code.clone(),
        vec![engine::Order {
            id: engine::OrderId(1),
            side: Side::Sell,
            price: Money::from_cents(1000),
            qty: 25,
            original_qty: 25,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(1),
            seq: 1,
        }],
    );
    initial_save.snapshot.markets.get_mut(&code).unwrap().asks =
        vec![(Money::from_cents(1000), 25)];
    initial_save
        .snapshot
        .markets
        .get_mut(&code)
        .unwrap()
        .best_ask = Some(Money::from_cents(1000));
    initial_save.next_order_id = 2;
    let mut session = GameSession::restore(&initial_save).unwrap();

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step();
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Trade { qty: 25, .. })));

    let generated_save = session.save();
    let remaining = generated_save.resting_orders[&code]
        .iter()
        .find(|order| order.owner == AccountId(0))
        .unwrap();
    assert_eq!(remaining.original_qty, 100);
    assert_eq!(remaining.filled_qty, 25);
    assert_eq!(remaining.qty, 75);
    assert_eq!(remaining.filled_value, Money::from_cents(25_000));
    assert!(GameSession::restore(&generated_save).is_ok());
}

#[test]
fn restore_validates_multiple_partial_sell_orders_independently_of_storage_order() {
    let code = StockCode("600101".to_string());
    let session = player_session_with_position(175, 10_000_000);
    let early_odd_lot = engine::Order {
        id: engine::OrderId(1),
        side: Side::Sell,
        price: Money::from_cents(1000),
        qty: 25,
        original_qty: 50,
        filled_qty: 25,
        filled_value: Money::from_cents(25_000),
        owner: AccountId(0),
        seq: 1,
    };
    let later_board_lot = engine::Order {
        id: engine::OrderId(2),
        side: Side::Sell,
        price: Money::from_cents(1000),
        qty: 150,
        original_qty: 200,
        filled_qty: 50,
        filled_value: Money::from_cents(50_000),
        owner: AccountId(0),
        seq: 2,
    };

    for orders in [
        vec![early_odd_lot.clone(), later_board_lot.clone()],
        vec![later_board_lot.clone(), early_odd_lot.clone()],
    ] {
        let mut save = session.save();
        save.resting_orders.insert(code.clone(), orders);
        let market = save.snapshot.markets.get_mut(&code).unwrap();
        market.best_ask = Some(Money::from_cents(1000));
        market.asks = vec![(Money::from_cents(1000), 175)];
        save.next_order_id = 3;

        assert!(GameSession::restore(&save).is_ok());
    }
}

#[test]
fn restore_rejects_filled_value_without_matching_quantity_progress() {
    let code = StockCode("600101".to_string());
    let session = player_session_with_position(0, 10_000_000);
    let mut save = session.save();
    save.resting_orders.insert(
        code.clone(),
        vec![engine::Order {
            id: engine::OrderId(1),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 1,
            original_qty: 1,
            filled_qty: 0,
            filled_value: Money::from_cents(1),
            owner: AccountId(0),
            seq: 1,
        }],
    );
    save.snapshot.markets.get_mut(&code).unwrap().bids = vec![(Money::from_cents(1000), 1)];
    save.snapshot.markets.get_mut(&code).unwrap().best_bid = Some(Money::from_cents(1000));
    save.next_order_id = 2;

    assert!(matches!(
        GameSession::restore(&save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("invalid saved resting order")
    ));
}

#[test]
fn restore_rejects_partial_fill_values_that_violate_the_limit_price_direction() {
    let code = StockCode("600101".to_string());

    let buy_session = player_session_with_position(0, 10_000_000);
    let mut buy_save = buy_session.save();
    buy_save.resting_orders.insert(
        code.clone(),
        vec![engine::Order {
            id: engine::OrderId(1),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
            original_qty: 200,
            filled_qty: 100,
            filled_value: Money::from_cents(110_000),
            owner: AccountId(0),
            seq: 1,
        }],
    );
    let buy_market = buy_save.snapshot.markets.get_mut(&code).unwrap();
    buy_market.best_bid = Some(Money::from_cents(1000));
    buy_market.bids = vec![(Money::from_cents(1000), 100)];
    buy_save.next_order_id = 2;
    assert!(matches!(
        GameSession::restore(&buy_save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("filled value")
    ));

    let sell_session = player_session_with_position(200, 10_000_000);
    let mut sell_save = sell_session.save();
    sell_save.resting_orders.insert(
        code.clone(),
        vec![engine::Order {
            id: engine::OrderId(1),
            side: Side::Sell,
            price: Money::from_cents(1000),
            qty: 100,
            original_qty: 200,
            filled_qty: 100,
            filled_value: Money::from_cents(90_000),
            owner: AccountId(0),
            seq: 1,
        }],
    );
    let sell_market = sell_save.snapshot.markets.get_mut(&code).unwrap();
    sell_market.best_ask = Some(Money::from_cents(1000));
    sell_market.asks = vec![(Money::from_cents(1000), 100)];
    sell_save.next_order_id = 2;
    assert!(matches!(
        GameSession::restore(&sell_save),
        Err(engine::SessionError::InvalidSave(message))
            if message.contains("filled value")
    ));
}
