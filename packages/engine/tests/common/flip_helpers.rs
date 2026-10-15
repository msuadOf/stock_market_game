//! 翻转迁移公共构造（双路径驱动器）。
#![allow(dead_code)]

use engine::money::Money;
use engine::runtime::drive::RuntimeDriver;
use engine::session::{NpcSetup, SessionSetup};
use engine::{Event, FloatAllocation, GameSession, SecurityCategory, StockCode, StockExchange, StockSpec, SIMULATION_POLICY_ID};

pub fn setup_1stock() -> SessionSetup {
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
pub fn runtime_events(ticks: u64, seed: u64) -> (GameSession, Vec<Event>) {
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
pub fn pipeline_events(ticks: u64, seed: u64) -> (GameSession, Vec<Event>) {
    let mut session = GameSession::new(setup_1stock(), seed).expect("会话构造");
    let mut all = Vec::new();
    for _ in 0..ticks {
        let frame = session.step_frame().expect("旧管线 tick");
        all.extend(frame.facts.into_iter().map(|f| f.event));
    }
    (session, all)
}

