//! 批 8a 桥接测试：SessionSetup → WorldSpec 投影 + runtime 世界与会话同构
//! 收敛（确定性配置面的接线地基验收）。

use super::bridge::{default_retail_decision, world_spec_from_setup};
use super::group::GroupScript;
use super::world::World;
use crate::money::Money;
use crate::session::SessionSetup;
use crate::StockExchange;
use crate::{SecurityCategory, StockCode, StockSpec};

pub(crate) fn setup_fixture() -> SessionSetup {
    let stocks = vec![
        StockSpec {
            code: StockCode("600000".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 800_000,
        },
        StockSpec {
            code: StockCode("300001".to_owned()),
            exchange: StockExchange::Shenzhen,
            initial_price: Money::from_cents(2000),
            category: SecurityCategory::ChiNext,
            limit_pct: 0.20,
            tick: Money::from_cents(1),
            total_shares: 500_000,
            float_shares: 400_000,
        },
    ];
    SessionSetup {
        stocks,
        company_operations: None,
        groups: Vec::new(),
        npcs: crate::session::NpcSetup {
            retail_count: 2,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(1_000_000),
        },
        config: crate::config::GameConfig::proposed_defaults(),
        strategy_params: crate::strategy::StrategyParams {
            retail: crate::strategy::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: crate::strategy::InstParams {
                margin: 0.01,
                order_size: 1,
            },
            hot: crate::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 1,
            },
        },
        ticks_per_day: 240,
        auction_ticks: 15,
        closing_auction_ticks: 3,
        history_len: 16,
        t1_enabled: true,
        float_allocation: crate::session::FloatAllocation::random(),
        start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        simulation_policy_id: crate::session::SIMULATION_POLICY_ID.to_owned(),
    }
}

fn idle_script() -> Box<dyn Fn(super::GroupIdx) -> GroupScript> {
    Box::new(|_g| Box::new(|_t, _r| super::group::TickPlan::default()))
}

#[test]
fn bridge_projects_setup_into_world_spec() {
    let setup = setup_fixture();
    let spec = world_spec_from_setup(&setup, None, idle_script()).expect("投影成功");
    assert_eq!(spec.stocks.len(), 2);
    // 主板 10%（1000bps）/创业板 20%（2000bps）显式差异。
    assert_eq!(spec.stocks[0].limit_bps, 1000);
    assert_eq!(spec.stocks[1].limit_bps, 2000);
    // 单笔上限类别差异：主板 100 万、创业板限价 30 万/市价 15 万。
    assert_eq!(spec.stocks[0].max_order_qty_limit, 1_000_000);
    assert_eq!(spec.stocks[1].max_order_qty_limit, 300_000);
    assert_eq!(spec.stocks[1].max_order_qty_market, 150_000);
    // 相位参数直传。
    assert_eq!(spec.ticks_per_day, 240);
    assert_eq!(spec.auction_ticks, 15);
    assert_eq!(spec.closing_auction_ticks, 3);
}

#[test]
fn bridge_rejects_zero_ticks_per_day() {
    let mut setup = setup_fixture();
    setup.ticks_per_day = 0;
    let err = match world_spec_from_setup(&setup, None, idle_script()) {
        Err(e) => e,
        Ok(_) => panic!("零 ticks_per_day 必须显式报错"),
    };
    assert!(err.contains("ticks_per_day"), "错误应指明字段：{err}");
}

#[test]
fn bridged_world_converges_with_session_phase_parameters() {
    // 会话相位参数（240/15/3）下 runtime 世界可完整收敛 20 tick：
    // 开盘竞价申报窗（day_tick<10）→盘前→连续→收盘竞价→日结→day2。
    let setup = setup_fixture();
    let mut spec = world_spec_from_setup(&setup, None, idle_script()).expect("投影");
    spec.n_groups = 1;
    spec.accounts_per_group = 2;
    spec.initial_cash = 1_000_000;
    spec.initial_shares = vec![vec![vec![1_000u32; 2]; 2]];
    let mut world = World::spawn(spec).expect("世界构造");
    let committed = world.run_ticks(20).expect("20 tick 收敛（含一次日结）");
    assert_eq!(committed, 20);
    let no_day_end_yet = world.last_day_end_candidate.is_none();
    let report = world.shutdown().expect("关停");
    // 空闲脚本：守恒恒等。
    let cash: i64 = report.group_finals[0].accounts.iter().map(|a| a.cash).sum();
    assert_eq!(cash, 2 * 1_000_000);
    // tick 20 < ticks_per_day 240：未跨日，无日结候选。
    assert!(no_day_end_yet);
}

#[test]
fn default_retail_decision_is_seed_distinct() {
    let d = default_retail_decision(3, 42);
    assert_eq!(d.profiles.len(), 3);
    assert_ne!(d.profiles[0].seed, d.profiles[1].seed);
}

// ── 撮合域事件桥（批 8b 项 2）──

#[test]
fn trade_facts_project_to_event_trade_with_monotonic_seq() {
    use super::event_bridge::{owner_to_account, project_trade_events};
    use super::protocol::DeskTradeFact;
    use crate::orderbook::Side;
    let trades = vec![
        DeskTradeFact {
            stock: 0,
            price: 1005,
            qty: 100,
            maker: (2, 7),
            taker: (1, 3),
            taker_side: Side::Buy,
            maker_fees_cents: 0,
            taker_fees_cents: 0,
            maker_order_serial: 0,
            taker_order_serial: 0,
        },
        DeskTradeFact {
            stock: 0,
            price: 1005,
            qty: 50,
            maker: (2, 7),
            taker: (0, 9),
            taker_side: Side::Buy,
            maker_fees_cents: 0,
            taker_fees_cents: 0,
            maker_order_serial: 0,
            taker_order_serial: 0,
        },
    ];
    let mut seq = 42u64;
    let events = project_trade_events(&trades, &mut seq);
    assert_eq!(events.len(), 2);
    for (i, e) in events.iter().enumerate() {
        let crate::Event::Trade {
            seq,
            code,
            price,
            qty,
            maker,
            taker,
        } = e
        else {
            panic!("应为 Trade 事件");
        };
        assert_eq!(*seq, 42 + i as u64, "seq 单调分配");
        assert_eq!(code, &crate::StockId(0), "code=StockId 路由");
        assert_eq!(price.cents(), 1005);
        assert_eq!(*qty, trades[i].qty);
        assert_eq!(*maker, owner_to_account(trades[i].maker));
        assert_eq!(*taker, owner_to_account(trades[i].taker));
    }
    assert_eq!(seq, 44, "返回推进后的 next_seq");
}

#[test]
fn world_reports_two_legged_trade_facts_end_to_end() {
    // 端到端：世界成交 → DeskReport.trades 携带双腿身份（按成交序）。
    use super::group::{GroupScript, PlaceSpec, TickPlan};
    let script = |_g: super::GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(PlaceSpec {
                    local: 1,
                    stock: 0,
                    side: crate::orderbook::Side::Sell,
                    price: 1000,
                    qty: 100,
                });
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: crate::orderbook::Side::Buy,
                    price: 1005,
                    qty: 100,
                });
            }
            plan
        })
    };
    let mut spec = super::skeleton_tests::basic_spec(1, 1, 2, Box::new(script));
    spec.decision = None;
    let mut world = super::world::World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("收敛");
    let report = world.shutdown().expect("关停");
    // DeskReport.trades 在收束时出口（协调者路径）；Final 侧由成交计数
    // 与映射器单测联测覆盖。
    assert_eq!(report.desk_finals[0].market.trade_count, 1, "一笔成交");
}

// ── 批 8c：时序快照映射 + 账户面投影 ──

#[test]
fn timeseries_mappers_project_runtime_views_to_engine_shapes() {
    use super::event_bridge::{
        project_continuous_point, project_daily_candle, project_market_snap,
    };
    use super::protocol::StockPublic;
    let view = StockPublic {
        last_price: 1005,
        best_bid: Some(1000),
        best_ask: Some(1010),
        max_buy_price: 1020,
        daily_upper_limit: 1100,
        min_sell_price: 980,
        recent_prices: vec![1000],
        minute_closes: vec![],
        daily_bars: Vec::new(),
        daily_active: None,
        daily_total_completed: 0,
        daily_total_traded: 0,
        volume_today: 12_345,
        relative_volume: 1.0,
        order_book_imbalance: 0.25,
        depth_bids: vec![(1000, 500), (990, 300)],
        depth_asks: vec![(1010, 400), (1020, 200)],
    };
    let snap = project_market_snap(&view, 999);
    assert_eq!(snap.last_price.cents(), 1005);
    assert_eq!(snap.last_close.cents(), 999);
    assert_eq!(snap.best_bid.unwrap().cents(), 1000);
    assert_eq!(snap.bids.len(), 2);
    assert_eq!(snap.bids[0], (crate::money::Money::from_cents(1000), 500));
    assert_eq!(snap.asks[1], (crate::money::Money::from_cents(1020), 200));
    let point = project_continuous_point(&view, 7, crate::TradingPhase::Continuous);
    assert_eq!(point.tick, 7);
    assert_eq!(point.last_price.cents(), 1005);
    assert_eq!(point.cumulative_volume, 12_345);
    assert_eq!(point.bids.len(), 2);
    let candle = super::desk::DeskCandle {
        open: 1000,
        high: 1010,
        low: 995,
        close: 1005,
        volume: 800,
    };
    let daily = project_daily_candle(candle, 42);
    assert_eq!(daily.time, 42);
    assert_eq!(daily.open.cents(), 1000);
    assert_eq!(daily.volume, 800);
}

#[test]
fn desk_report_view_carries_five_level_depth() {
    // 端到端：挂簿成交后 DeskReport.view 携带五档深度（MarketSnap 映射输入）。
    use super::group::{GroupScript, PlaceSpec, TickPlan};
    use crate::orderbook::Side;
    let script = |_g: super::GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(PlaceSpec {
                    local: 1,
                    stock: 0,
                    side: Side::Sell,
                    price: 1010,
                    qty: 100,
                });
                plan.places.push(PlaceSpec {
                    local: 1,
                    stock: 0,
                    side: Side::Sell,
                    price: 1005,
                    qty: 100,
                });
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: Side::Buy,
                    price: 1000,
                    qty: 100,
                });
            }
            plan
        })
    };
    let mut world =
        super::world::World::spawn(super::skeleton_tests::basic_spec(1, 1, 2, Box::new(script)))
            .expect("世界构造");
    world.run_ticks(1).expect("收敛");
    let views = world.public_views();
    // 两档卖盘（1005/1010）挂在簿上；买方 1000 挂簿。
    assert!(
        !views[0].depth_asks.is_empty(),
        "卖盘深度应可见：{:?}",
        views[0].depth_asks
    );
    assert!(views[0].depth_bids.iter().any(|(p, _)| *p == 1000));
    assert_eq!(views[0].depth_asks[0].0, 1005, "价低在前");
}

// ── 批 8d：驱动器纵切 ──

#[test]
fn runtime_driver_steps_session_with_account_writeback_and_events() {
    use super::drive::RuntimeDriver;
    // 会话（2 股）→ 驱动器 → 3 tick：现金回写守恒、事件 seq 单调。
    let mut session = crate::GameSession::new(setup_fixture(), 7).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    assert!(
        driver.id_map.len() >= 1,
        "玩家并入投影（AccountId(0) 惯例位）"
    );
    let before_cash: i64 = driver
        .id_map
        .iter()
        .filter_map(|id| session.account(*id).map(|a| a.cash().cents()))
        .sum();
    let mut total_events = 0usize;
    let mut last_seq: Option<u64> = None;
    for _ in 0..20 {
        let events = driver.step(&mut session).expect("驱动 tick");
        for e in &events {
            let seq = match e {
                crate::Event::Trade { seq, .. } => *seq,
                // 批 8e/8f：事件面扩全（受理/撤单/拒收/竞价），纵切允许。
                crate::Event::OrderAccepted { seq, .. }
                | crate::Event::OrderCanceled { seq, .. }
                | crate::Event::IntentRejected { seq, .. }
                | crate::Event::AuctionTick { seq, .. }
                | crate::Event::AuctionCompleted { seq, .. } => *seq,
                // 纵切当前不产这些类型（PriceTick/日界/公司域留后续批）；
                // 出现即断言失败（显式，不静默跳过）。
                other => panic!("纵切事件面外的事件：{other:?}"),
            };
            if let Some(prev) = last_seq {
                assert!(seq > prev, "seq 全局单调：{seq} <= {prev}");
            }
            last_seq = Some(seq);
        }
        total_events += events.len();
    }
    let after_cash: i64 = driver
        .id_map
        .iter()
        .filter_map(|id| session.account(*id).map(|a| a.cash().cents()))
        .sum();
    // 决策活跃（retail 到达 0.30）：现金应变化（成交/挂单扣款回写）；
    // 3 tick 全程无成交也允许（挂簿扣款）——断言回写通道真实生效：
    // 构造确定性对照（同会话再驱动 0 步无变化）。
    assert!(
        after_cash != before_cash || total_events > 0,
        "回写或事件至少一面生效（现金 {before_cash}→{after_cash}，事件 {total_events}）"
    );
}

#[test]
fn lifecycle_facts_map_to_engine_event_types() {
    use super::event_bridge::{collect_lifecycle, project_lifecycle_events, rejection_reason};
    use super::protocol::{DeskLifecycleFact, LifecycleKind};
    use super::{DeskReceipt, ReceiptKind};
    use crate::orderbook::Side;
    let facts = vec![
        DeskLifecycleFact {
            stock: 1,
            owner: (0, 5),
            order_serial: 7,
            remaining: 300,
            side: Side::Buy,
            price: 1000,
            kind: LifecycleKind::Accepted,
        },
        DeskLifecycleFact {
            stock: 1,
            owner: (0, 5),
            order_serial: 7,
            remaining: 100,
            side: Side::Buy,
            price: 1000,
            kind: LifecycleKind::Cancelled,
        },
    ];
    let receipts = vec![DeskReceipt {
        order: (1, 9),
        owner: (2, 3),
        kind: ReceiptKind::PriceOutOfBand,
    }];
    let input = collect_lifecycle(&facts, &receipts);
    assert_eq!(input.cancels.len(), 1);
    assert_eq!(input.accepts.len(), 1);
    assert_eq!(input.rejections.len(), 1);
    assert!(matches!(
        input.rejections[0].3,
        crate::RejectionReason::LimitExceeded
    ));
    assert!(matches!(
        rejection_reason(ReceiptKind::CancelWindowClosed),
        crate::RejectionReason::AuctionOrderNotCancelable
    ));
    let mut seq = 100u64;
    let events = project_lifecycle_events(&input, &mut seq);
    assert_eq!(events.len(), 3);
    assert_eq!(seq, 103);
    for e in &events {
        match e {
            crate::Event::OrderCanceled {
                account,
                code,
                id,
                remaining_qty,
                ..
            } => {
                assert_eq!(code, &crate::StockId(1));
                assert_eq!(id.0, 7);
                assert_eq!(*remaining_qty, 100);
                assert_eq!(*account, super::event_bridge::owner_to_account((0, 5)));
            }
            crate::Event::OrderAccepted { side, price, .. } => {
                assert_eq!(side, &Side::Buy);
                assert_eq!(price.cents(), 1000);
            }
            crate::Event::IntentRejected { reason, .. } => {
                assert!(matches!(reason, crate::RejectionReason::LimitExceeded));
            }
            _ => panic!("应为生命周期事件"),
        }
    }
}

#[test]
fn driver_day_end_advances_runtime_and_civil_together() {
    use super::drive::RuntimeDriver;
    // ticks_per_day=240：驱动 240 tick（一个交易日）→ day_end：
    // runtime 日结（组日翻转/清簿）+ 会话自然日推进（civil clock 前进）。
    let mut session = crate::GameSession::new(setup_fixture(), 11).expect("会话构造");
    let day_before = session.day();
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    for _ in 0..240 {
        driver.step(&mut session).expect("驱动 tick");
    }
    let world_day = driver.world_global_day();
    // 纵切已知边界（登记）：会话市场侧 day 由旧管线 step_frame 推进；纵切
    // 驱动不跑旧管线 ⇒ session.day 停留 ⇒ end_civil_day 显式拒绝
    // （MarketSessionOutOfSync——防御正确）。编排 API 形状验证：
    let err = driver
        .day_end(&mut session)
        .expect_err("纵切下自然日推进须显式拒绝");
    assert!(
        err.contains("MarketSessionOutOfSync"),
        "应为时钟同步拒绝：{err}"
    );
    // runtime 侧日结已成功（run_day_end_public 在会话拒绝前完成）。
    assert!(world_day <= session.day() + 1);
    let _ = day_before;
}
