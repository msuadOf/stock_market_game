#![cfg(not(feature = "simulation-diagnostics"))]

use engine::{
    AccountId, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcDecisionDiagnostics, NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode,
    StockExchange, StockSpec, StrategyParams,
};

fn setup() -> SessionSetup {
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
            retail_count: 0,
            inst_count: 0,
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
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V2.to_owned(),
    }
}

#[test]
fn release_without_diagnostic_feature_keeps_normal_session_available() {
    let mut session = GameSession::new(setup(), 7).unwrap();
    assert!(!session.step().expect("healthy step").is_empty());
    assert_eq!(
        session.npc_decision_diagnostics(AccountId(1)),
        NpcDecisionDiagnostics::Unsupported
    );

    let saved = session.save().expect("healthy save");
    let saved_json = serde_json::to_value(saved).unwrap();
    assert!(saved_json.get("npc_decision_traces").is_none());
    assert!(!serde_json::to_string(&session.snapshot())
        .unwrap()
        .contains("npc_decision_traces"));
}

#[test]
fn unsupported_diagnostic_query_keeps_seeded_events_and_save_bytes_identical() {
    // Given: two same-seed release sessions without the diagnostic collector feature.
    let mut plain = GameSession::new(setup(), 29).unwrap();
    let mut queried = GameSession::new(setup(), 29).unwrap();

    // When: one path performs the only supported release diagnostic query between ticks.
    for _ in 0..12 {
        let plain_events = plain.step().expect("healthy step");
        let queried_events = queried.step().expect("healthy step");
        assert_eq!(
            queried.npc_decision_diagnostics(AccountId(1)),
            NpcDecisionDiagnostics::Unsupported
        );
        assert_eq!(plain_events, queried_events);
    }

    // Then: no hidden diagnostic RNG/state perturbs the fixed seeded replay.
    assert_eq!(
        serde_json::to_vec(&plain.save().expect("healthy save")).unwrap(),
        serde_json::to_vec(&queried.save().expect("healthy save")).unwrap()
    );
}
