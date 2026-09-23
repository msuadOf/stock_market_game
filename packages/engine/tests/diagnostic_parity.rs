#![cfg(feature = "simulation-diagnostics")]

#[cfg(test)]
use engine::{AccountId, GameSession};
use engine::{
    FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams,
    SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};

pub fn setup() -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600101".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 100_000,
            float_shares: 100_000,
        }],
        npcs: NpcSetup {
            retail_count: 4,
            inst_count: 4,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V1.to_owned(),
    }
}

#[test]
fn trace_queries_are_read_only_and_keep_per_npc_history_bounded() {
    let mut plain = GameSession::new(setup(), 7).unwrap();
    let mut traced = GameSession::new(setup(), 7).unwrap();

    for _ in 0..129 {
        let plain_events = plain.step().expect("healthy step");
        let traced_events = traced.step().expect("healthy step");
        let _ = traced.npc_decision_trace(AccountId(5));
        assert_eq!(plain_events, traced_events);
    }

    assert_eq!(
        serde_json::to_vec(&plain.save().expect("healthy save")).unwrap(),
        serde_json::to_vec(&traced.save().expect("healthy save")).unwrap()
    );
    let trace = traced.npc_decision_trace(AccountId(5)).unwrap();
    assert!(trace.len() <= 128);
    assert!(trace.iter().all(|record| record.account == AccountId(5)));
}
