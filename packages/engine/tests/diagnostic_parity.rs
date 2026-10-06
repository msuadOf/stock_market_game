#![cfg(feature = "simulation-diagnostics")]

#[cfg(test)]
include!("../test-support/simple_company.rs");

use engine::{AccountId, GameSession};
use engine::{
    FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams,
    SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};

pub fn setup() -> SessionSetup {
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600101"]),
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
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::random(),
        start_date: engine::CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    }
}

#[test]
fn trace_queries_are_read_only_and_keep_per_npc_history_bounded() {
    const QUERY_TICKS: u64 = 129;
    let mut setup = setup();
    setup.ticks_per_day = QUERY_TICKS + 1;
    let mut session = GameSession::new(setup, 7).unwrap();
    // 第一个机构是极低频的 DeepValue；选择 Growth 的真实观察节奏覆盖非空查询。
    let account = AccountId(6);

    for _ in 0..QUERY_TICKS {
        session.step().expect("healthy step");
        // seed 不包含并发受理轨迹；在同一已提交时间线上验证查询没有副作用。
        let before = serde_json::to_vec(&session.save().expect("healthy save")).unwrap();
        let trace = session.npc_decision_trace(account);
        assert_eq!(trace, session.npc_decision_trace(account));
        assert_eq!(
            before,
            serde_json::to_vec(&session.save().expect("healthy save")).unwrap()
        );
    }

    assert_eq!(session.tick(), QUERY_TICKS);
    let trace = session
        .npc_decision_trace(account)
        .expect("机构必须产生真实 decision trace");
    assert!(!trace.is_empty());
    assert!(trace.len() <= engine::MAX_NPC_DECISION_TRACE_RECORDS);
    assert!(trace.iter().all(|record| record.account == account));
}
