//! Q01 表示取证工具：完整复用两个受控 fixture 的生产调用与业务 guards。
//! 单独链接 675ac4c 或当前 engine，捕获原字节，不读取失败断言或生成预期值。

mod replay {
    use engine::account::StockCode;
    use engine::config::GameConfig;
    use engine::money::Money;
    use engine::session::{
        Event, FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup,
        StockExchange, StockSpec,
    };
    use engine::strategy::{HotParams, InstParams, Intent, RetailParams, StrategyParams};
    use engine::{AccountId, LimitPrice, Side};

    const REPLAY_SEED: u64 = 0x5EED_2026_0903;
    const TICKS_PER_DAY: u64 = 240;
    const REPLAY_DAYS: u64 = 3;

    fn replay_setup() -> SessionSetup {
        let first = StockCode("600888".to_string());
        let second = StockCode("600889".to_string());
        SessionSetup {
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![
                StockSpec {
                    code: first.clone(),
                    exchange: StockExchange::Shanghai,
                    initial_price: Money::from_cents(1_000),
                    category: SecurityCategory::MainBoard,
                    limit_pct: 0.10,
                    tick: Money::from_cents(1),
                    total_shares: 10_000_000,
                    float_shares: 40_000,
                },
                StockSpec {
                    code: second.clone(),
                    exchange: StockExchange::Shanghai,
                    initial_price: Money::from_cents(2_350),
                    category: SecurityCategory::MainBoard,
                    limit_pct: 0.10,
                    tick: Money::from_cents(1),
                    total_shares: 8_000_000,
                    float_shares: 40_000,
                },
            ],
            npcs: NpcSetup {
                retail_count: 12,
                inst_count: 2,
                hot_count: 2,
                retail_cash_median: Money::from_cents(5_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.40,
                    order_size_mean: 200,
                    chase_prob: 0.30,
                },
                inst: InstParams {
                    margin: 0.05,
                    order_size: 500,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 300,
                },
            },
            ticks_per_day: TICKS_PER_DAY,
            auction_ticks: 15,
            closing_auction_ticks: 6,
            history_len: 10,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
            start_date: engine::CivilDate::from_iso("2030-01-02").unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        }
    }

    struct ReplayCapture {
        events_bytes: Vec<u8>,
        save_mid_bytes: Vec<u8>,
        save_end_bytes: Vec<u8>,
    }

    fn run_replay(seed: u64) -> ReplayCapture {
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| run_replay_serial(seed))
    }

    fn run_replay_serial(seed: u64) -> ReplayCapture {
        let initial = GameSession::new(replay_setup(), seed).expect("replay setup must be valid");
        let mut initial_save = initial.save().expect("healthy initial save");
        let initial_cash: i128 = initial_save
            .snapshot
            .accounts
            .values()
            .map(|account| i128::from(account.cash.cents()))
            .sum();
        for stock in &initial_save.setup.stocks {
            let donor = initial_save
                .snapshot
                .accounts
                .iter_mut()
                .filter(|(owner, _)| **owner != AccountId(0))
                .find_map(|(_, account)| {
                    account
                        .positions
                        .get_mut(&stock.code)
                        .filter(|position| position.qty >= 1_000)
                })
                .expect("initial float must supply the player fixture inventory");
            assert_eq!(donor.t1_locked, 0);
            let invested_cents = stock.initial_price.cents() * 1_000;
            donor.qty -= 1_000;
            donor.invested_cents -= invested_cents;
            initial_save
                .snapshot
                .accounts
                .get_mut(&AccountId(0))
                .expect("initial player exists")
                .positions
                .insert(
                    stock.code.clone(),
                    engine::PositionSnap {
                        qty: 1_000,
                        t1_locked: 0,
                        invested_cents,
                        recovered_cents: 0,
                    },
                );
        }
        let mut session =
            GameSession::restore(&initial_save).expect("initial inventory must restore");
        assert_eq!(
            initial_save
                .snapshot
                .accounts
                .keys()
                .map(|owner| {
                    i128::from(
                        session
                            .account(*owner)
                            .expect("restored account exists")
                            .cash()
                            .cents(),
                    )
                })
                .sum::<i128>(),
            initial_cash,
            "initial inventory redistribution must not inject cash"
        );
        let mut events: Vec<Event> = Vec::new();
        let mut save_mid_bytes = Vec::new();
        for tick_index in 0..(TICKS_PER_DAY * REPLAY_DAYS) {
            if tick_index == TICKS_PER_DAY {
                save_mid_bytes = serde_json::to_vec(&session.save().expect("healthy save"))
                    .expect("mid-scenario authoritative save must serialize");
            }
            if tick_index == 0 {
                for stock in &session
                    .save()
                    .expect("healthy setup projection")
                    .setup
                    .stocks
                {
                    let price = session.snapshot().markets[&stock.code].last_close;
                    session
                        .enqueue_player_intent(
                            AccountId(0),
                            Intent::PlaceLimit {
                                code: stock.code.clone(),
                                side: Side::Sell,
                                price: LimitPrice::Fixed(price),
                                qty: 1_000,
                            },
                        )
                        .expect("real player order must enqueue");
                }
            }
            let step_events = session.step().expect("healthy step");
            for event in &step_events {
                if let Event::Trade {
                    code,
                    qty,
                    maker,
                    taker,
                    ..
                } = event
                {
                    let buyer = if *maker == AccountId(0) {
                        *taker
                    } else if *taker == AccountId(0) {
                        *maker
                    } else {
                        continue;
                    };
                    assert!(
                        session
                            .account(buyer)
                            .expect("real buyer exists")
                            .position(code)
                            .expect("real purchase creates a position")
                            .t1_locked()
                            >= *qty,
                        "the real NPC purchase must be T+1 locked on its trade day"
                    );
                }
            }
            events.extend(step_events);
            if (tick_index + 1) % TICKS_PER_DAY == 0 {
                events.extend(
                    session
                        .end_civil_day()
                        .expect("healthy civil day end")
                        .events,
                );
            }
        }
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Trade { qty, .. } if *qty > 0)),
            "seed discrimination requires genuine trades, not only empty price ticks"
        );
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Trade { qty, maker, taker, .. }
                if *qty > 0 && ((*maker == AccountId(0)) != (*taker == AccountId(0)))
        )));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::DayBoundary { .. }))
                .count(),
            REPLAY_DAYS as usize
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::AuctionCompleted { .. }))
                .count(),
            (REPLAY_DAYS * 2 * 2) as usize
        );
        let save = session.save().expect("healthy save");
        assert!(
            save.snapshot
                .accounts
                .values()
                .map(|account| i128::from(account.cash.cents()))
                .sum::<i128>()
                < initial_cash,
            "genuine trades must deduct fees rather than inject cash"
        );
        assert_eq!(save.snapshot.tick, TICKS_PER_DAY * REPLAY_DAYS);
        assert_eq!(u64::from(session.day()), REPLAY_DAYS);
        assert_eq!(
            save.civil_clock.current_date,
            engine::CivilDate::from_iso("2030-01-05").unwrap()
        );
        for stock in &save.setup.stocks {
            assert_eq!(
                save.snapshot
                    .accounts
                    .values()
                    .map(|account| account
                        .positions
                        .get(&stock.code)
                        .map_or(0u64, |position| u64::from(position.qty)))
                    .sum::<u64>(),
                u64::from(stock.float_shares)
            );
        }
        let save_end_bytes =
            serde_json::to_vec(&save).expect("end-of-scenario save must serialize");
        let events_bytes = serde_json::to_vec(&events).expect("event stream must serialize");
        ReplayCapture {
            events_bytes,
            save_mid_bytes,
            save_end_bytes,
        }
    }

    pub fn capture(path: &str, seed: u64) {
        let capture = run_replay(seed);
        for (name, bytes) in [
            ("events", capture.events_bytes),
            ("mid", capture.save_mid_bytes),
            ("end", capture.save_end_bytes),
        ] {
            std::fs::write(format!("{path}/replay-{seed}-{name}.json"), bytes).unwrap();
        }
    }
}
mod step {
    use engine::{
        CivilDate, Event, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
        NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange,
        StockSpec, StrategyParams,
    };

    fn setup(auction_ticks: u64) -> Result<SessionSetup, Box<dyn std::error::Error>> {
        Ok(SessionSetup {
            company_operations: None,
            groups: Vec::new(),
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
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        })
    }

    fn complete_projection(auction_ticks: u64, seed: u64, ticks: u64) -> Vec<Vec<u8>> {
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

    pub fn capture(path: &str, auction: u64) {
        for (tick, bytes) in complete_projection(auction, 42, 20).iter().enumerate() {
            std::fs::write(
                format!("{path}/step-auction-{auction}-tick-{:02}.json", tick + 1),
                bytes,
            )
            .unwrap();
        }
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("capture output directory");
    let fixture = args.next().expect("fixture name");
    match fixture.as_str() {
        "replay" => replay::capture(&path, 0x5EED_2026_0903),
        "perturbed" => replay::capture(&path, 0x5EED_2026_0903 ^ 0x9E37_79B9),
        _ => step::capture(&path, fixture.parse().expect("auction ticks")),
    }
}
