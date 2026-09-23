//! Task 28: real `GameSession` accounting-information-plan-matching scenarios.
//!
//! This target deliberately uses the session's public test surface. It never fabricates a
//! `Trade`, candle, fill, or liquidity: the orderbook emits every asserted trade.

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange,
    StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};
use serde::Deserialize;

mod constraints;
mod controlled;
mod controlled_execution;
mod controlled_experience;
mod lifecycle;
mod matching;
mod restore;

pub(crate) const SEED: u64 = 0x28_C0FFEE;
pub(crate) const TICKS_PER_DAY: u64 = 60;

#[derive(Deserialize)]
pub(crate) struct ScenarioFixture {
    pub start_date: String,
    pub matching: MatchingFixture,
}

#[derive(Deserialize)]
pub(crate) struct MatchingFixture {
    pub price_cents: i64,
    pub seller_shares: u32,
    pub buyer_target_shares: u32,
}

pub(crate) fn fixture() -> ScenarioFixture {
    serde_json::from_str(include_str!(
        "../fixtures/company-model/task-28-scenario.json"
    ))
    .expect("task-28 fixture must be structurally valid")
}

pub(crate) fn code(value: &str) -> StockCode {
    StockCode(value.to_owned())
}

fn stock(
    value: &str,
    price_cents: i64,
    category: SecurityCategory,
    total_shares: u64,
) -> StockSpec {
    StockSpec {
        code: code(value),
        exchange: if value.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(price_cents),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares,
        float_shares: 1_000_000,
    }
}

pub(crate) fn setup(start_date: &str) -> SessionSetup {
    SessionSetup {
        stocks: vec![
            stock("600101", 1_120, SecurityCategory::MainBoard, 8_928_571_429),
            stock("002156", 2_735, SecurityCategory::MainBoard, 2_925_045_704),
            stock("300260", 3_680, SecurityCategory::ChiNext, 815_217_391),
            stock("600610", 755, SecurityCategory::MainBoard, 1_059_602_649),
            stock("000812", 285, SecurityCategory::StMainBoard, 1_052_631_579),
        ],
        npcs: NpcSetup {
            retail_count: 12,
            inst_count: 10,
            hot_count: 4,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.4,
                order_size_mean: 200,
                chase_prob: 0.3,
                tick_cents: 2,
            },
            inst: InstParams {
                margin: 0.03,
                order_size: 5_000,
            },
            hot: HotParams {
                lookback: 10,
                trend_threshold: 0.02,
                order_size: 1_000,
            },
        },
        ticks_per_day: TICKS_PER_DAY,
        auction_ticks: 6,
        closing_auction_ticks: 3,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.4,
            inst: 0.5,
            hot: 0.1,
        },
        start_date: engine::CivilDate::from_iso(start_date).expect("fixture civil date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V1.to_owned(),
    }
}

pub(crate) fn session(start_date: &str) -> GameSession {
    GameSession::new(setup(start_date), SEED).expect("task-28 fixture must assemble")
}

pub(crate) fn fixture_session() -> GameSession {
    session(&fixture().start_date)
}

pub(crate) fn run_trading_day(session: &mut GameSession) -> Vec<engine::session::Event> {
    let mut events = Vec::new();
    for _ in 0..TICKS_PER_DAY {
        events.extend(session.step().expect("healthy step"));
    }
    events
}
