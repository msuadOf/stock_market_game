use engine::{CivilDate, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams};
use engine::session::protocol::ProtocolSession;

fn setup() -> SessionSetup {
    SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("000812".into()),
            exchange: StockExchange::Shenzhen,
            initial_price: Money::from_cents(885),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 1_000_000,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 1,
            hot_count: 0,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams { arrival_rate: 0.0, order_size_mean: 200, chase_prob: 0.0 },
            inst: InstParams { margin: 0.02, order_size: 500 },
            hot: HotParams { lookback: 3, trend_threshold: 0.01, order_size: 300 },
        },
        ticks_per_day: 3,
        auction_ticks: 1,
        closing_auction_ticks: 1,
        history_len: 1,
        t1_enabled: true,
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V2.into(),
    }
}

#[test]
fn closing_checkpoint_and_completed_public_day_end_have_distinct_contracts() {
    let mut session = ProtocolSession::new(setup(), 4).unwrap();
    session.step_frame().unwrap();
    session.step_frame().unwrap();
    let checkpoint = session.game().save().unwrap();
    assert_eq!(session.game().phase(), engine::TradingPhase::ClosingAuction);
    assert_eq!(checkpoint.parent_orders.len(), 1);
    let parent = checkpoint.parent_orders.values().next().unwrap().values().next().unwrap();
    eprintln!("tick={} phase={:?} parent={:?}", checkpoint.snapshot.tick, session.game().phase(), parent);
    assert_eq!(parent.expires_market_minute, 240);
    assert!(parent.filled_qty < parent.target_qty);
    assert!(matches!(GameSession::restore(&checkpoint),
        Err(engine::SessionError::InvalidSave(reason)) if reason.contains("violates execution-plan invariants")));
    session.step_frame().unwrap();
    session.end_civil_day_update().unwrap();
    let completed = session.save().unwrap();
    assert!(completed.parent_orders.is_empty());
    assert!(completed.resting_orders.values().all(Vec::is_empty));
    assert!(completed.auction_orders.values().all(Vec::is_empty));
    let restored = ProtocolSession::restore(&completed).unwrap();
    assert_eq!(serde_json::to_value(restored.save().unwrap()).unwrap(), serde_json::to_value(completed).unwrap());
}
