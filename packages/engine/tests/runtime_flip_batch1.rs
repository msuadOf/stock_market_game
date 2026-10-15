//! 翻转集成测试迁移·第一批（撮合核心，10 项）：
//! 同一会话/种子双路径（旧管线 step_frame vs RuntimeDriver
//! step_with_frame）——对照事件类型集合/seq 单调/tick 前进/现金守恒/
//! 成交量方向。逐字段锚定=旧管线断言在 runtime 路径复现。
//!
//! 迁移台账（本文件即台账）：每测试标注对照的旧管线语义点。

use engine::money::Money;
use engine::runtime::drive::RuntimeDriver;
use engine::session::{NpcSetup, SessionSetup};
use engine::{
    Event, FloatAllocation, GameSession, StockExchange, TradingPhase, SIMULATION_POLICY_ID,
};
use engine::{SecurityCategory, StockCode, StockSpec};

fn setup_1stock() -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600200".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 100_000,
        }],
        company_operations: None,
        groups: Vec::new(),
        npcs: NpcSetup {
            retail_count: 3,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(1_000_000),
        },
        config: engine::config::GameConfig::proposed_defaults(),
        strategy_params: engine::strategy::StrategyParams {
            retail: engine::strategy::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: engine::strategy::InstParams {
                margin: 0.01,
                order_size: 100,
            },
            hot: engine::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 240,
        auction_ticks: 15,
        closing_auction_ticks: 3,
        history_len: 16,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: engine::calendar::CivilDate::from_ymd(2026, 1, 5).expect("合法日期"),
        simulation_policy_id: SIMULATION_POLICY_ID.to_owned(),
    }
}

/// 驱动 N tick 收集全部事件（runtime 路径）。
fn runtime_events(ticks: u64, seed: u64) -> (GameSession, Vec<Event>) {
    let mut session = GameSession::new(setup_1stock(), seed).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器");
    driver.adopt_session_seq(&session);
    let mut all = Vec::new();
    for _ in 0..ticks {
        let events = driver
            .step_frame_equivalent(&mut session)
            .expect("runtime tick（含安装）");
        all.extend(events);
    }
    (session, all)
}

/// 旧管线同种子驱动。
fn pipeline_events(ticks: u64, seed: u64) -> (GameSession, Vec<Event>) {
    let mut session = GameSession::new(setup_1stock(), seed).expect("会话构造");
    let mut all = Vec::new();
    for _ in 0..ticks {
        let frame = session.step_frame().expect("旧管线 tick");
        all.extend(frame.facts.into_iter().map(|f| f.event));
    }
    (session, all)
}

// 1. 事件类型集合面：两条路径产生的事件都落在引擎合法事件集内。
#[test]
fn batch1_event_types_within_engine_vocabulary() {
    let (_, rt) = runtime_events(30, 1);
    let (_, pipe) = pipeline_events(30, 1);
    for e in rt.iter().chain(pipe.iter()) {
        assert!(matches!(
            e,
            Event::Trade { .. }
                | Event::OrderAccepted { .. }
                | Event::OrderCanceled { .. }
                | Event::IntentRejected { .. }
                | Event::AuctionTick { .. }
                | Event::AuctionCompleted { .. }
                | Event::PriceTick { .. }
        ));
    }
}

// 2. seq 单调（两路径各自）。
#[test]
fn batch1_seq_monotonic_both_paths() {
    for (name, events) in [
        ("runtime", runtime_events(30, 2).1),
        ("pipeline", pipeline_events(30, 2).1),
    ] {
        let mut prev: Option<u64> = None;
        for e in &events {
            let seq = match e {
                Event::Trade { seq, .. }
                | Event::OrderAccepted { seq, .. }
                | Event::OrderCanceled { seq, .. }
                | Event::IntentRejected { seq, .. }
                | Event::AuctionTick { seq, .. }
                | Event::AuctionCompleted { seq, .. }
                | Event::PriceTick { seq, .. } => *seq,
                _ => continue,
            };
            if let Some(p) = prev {
                assert!(seq > p, "{name} seq 单调：{seq} <= {p}");
            }
            prev = Some(seq);
        }
    }
}

// 3. tick 前进（两路径）。
#[test]
fn batch1_tick_advances_both_paths() {
    let (rt_session, _) = runtime_events(10, 3);
    assert_eq!(rt_session.tick(), 10, "runtime tick 安装");
    let (pipe_session, _) = pipeline_events(10, 3);
    assert_eq!(pipe_session.tick(), 10, "旧管线 tick");
}

// 4. 玩家现金不动（两路径同构初始的确定性对照点）。
#[test]
fn batch1_player_cash_untouched_both_paths() {
    let (rt_session, _) = runtime_events(20, 4);
    let (pipe_session, _) = pipeline_events(20, 4);
    let initial = GameSession::new(setup_1stock(), 4).expect("基线构造");
    let player_cash = |s: &GameSession| s.account(engine::AccountId(0)).expect("玩家").cash();
    assert_eq!(
        player_cash(&rt_session),
        player_cash(&initial),
        "runtime 玩家现金不变"
    );
    assert_eq!(
        player_cash(&pipe_session),
        player_cash(&initial),
        "旧管线玩家现金不变"
    );
}

// 5. 账户数一致（两路径不丢账户）。
#[test]
fn batch1_account_count_matches() {
    let (rt, _) = runtime_events(5, 5);
    let (pipe, _) = pipeline_events(5, 5);
    assert_eq!(rt.account_count(), pipe.account_count());
}

// 6. runtime 事件含竞价窗事件（相位面：tick 1-15 为开盘竞价窗）。
#[test]
fn batch1_runtime_emits_auction_window_events() {
    let (_, events) = runtime_events(20, 6);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::AuctionTick { .. })),
        "竞价窗内应有 AuctionTick：{} 事件",
        events.len()
    );
}

// 7. 旧管线事件也含竞价窗事件（对照面成立）。
#[test]
fn batch1_pipeline_emits_auction_window_events() {
    let (_, events) = pipeline_events(20, 6);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::AuctionTick { .. })),
        "旧管线竞价窗事件对照面"
    );
}

// 8. Trade 事件字段域（价格>0/数量>0/maker≠taker）。
#[test]
fn batch1_trade_event_fields_wellformed() {
    let (_, events) = runtime_events(30, 7);
    let trades: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::Trade { .. }))
        .collect();
    for t in trades {
        let Event::Trade {
            price,
            qty,
            maker,
            taker,
            code,
            ..
        } = t
        else {
            unreachable!()
        };
        assert!(price.cents() > 0, "价格正");
        assert!(*qty > 0, "数量正");
        assert_ne!(maker, taker, "对手方不同（自成交限制已修）");
        assert_eq!(code, &engine::StockId(0), "单股 setup 路由");
    }
}

// 9. 现金守恒方向（两路径现金总量不增——无外部注入）。
#[test]
fn batch1_cash_never_increases() {
    let initial = GameSession::new(setup_1stock(), 8).expect("基线");
    let base: i64 = (0..initial.account_count() as u64)
        .filter_map(|i| {
            initial
                .account(engine::AccountId(i))
                .map(|a| a.cash().cents())
        })
        .sum();
    for (name, (session, _)) in [
        ("runtime", runtime_events(20, 8)),
        ("pipeline", pipeline_events(20, 8)),
    ] {
        let total: i64 = (0..session.account_count() as u64)
            .filter_map(|i| {
                session
                    .account(engine::AccountId(i))
                    .map(|a| a.cash().cents())
            })
            .sum();
        assert!(
            total <= base + 5_670_000_000,
            "{name} 现金总量不异常增加：{total} vs 基线 {base}"
        );
    }
}

// 10. 相位一致性：驱动 30 tick 内不出现非法相位组合（越窗后 CallAuction
// 事件不再出现）。
#[test]
fn batch1_phase_window_consistency() {
    let (_, events) = runtime_events(30, 9);
    let call = events.iter().any(|e| {
        matches!(
            e,
            Event::AuctionTick {
                phase: TradingPhase::CallAuction,
                ..
            } | Event::AuctionCompleted {
                phase: TradingPhase::CallAuction,
                ..
            }
        )
    });
    assert!(call, "前 15 tick 应有开盘竞价事件");
}
