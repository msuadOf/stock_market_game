//! SOURCE-B 真实 Session 选源、公开前史与唯一存档字段的生产边界。
include!("../test-support/simple_company.rs");
use engine::*;
use engine::company::cash_dividend_tax::CashDividendTaxMode;
use engine::information::PublicationSource;
fn plain_setup() -> SessionSetup {
    SessionSetup {
        company_operations: None,
        groups: Vec::new(),
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
        float_allocation: FloatAllocation::random(),
        start_date: engine::CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
    }
}


fn setup() -> SessionSetup {
    let mut setup = plain_setup();
    setup.npcs.retail_count = 0;
    setup.history_len = 1;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup
}

#[test]
fn selected_simple_initial_public_history_comes_from_simple_generated_reports() {
    let session = GameSession::new(setup(), 42).unwrap();
    let saved = session.save().unwrap();
    let now = engine::calendar::CivilInstant::new(session.civil_date(), 0).unwrap();
    let reports = saved.public_library.reports_for_company(&engine::company::CompanyId(0), now);
    assert!(!reports.is_empty(), "真实 Simple 前史应已生成可公开定稿");
    for report in reports {
        assert_eq!(report.source, PublicationSource::SimpleGenerated);
    }
}

#[test]
fn selected_simple_save_contains_only_the_required_selected_company_state() {
    let session = GameSession::new(setup(), 42).unwrap();
    let saved = serde_json::to_value(session.save().unwrap()).unwrap();
    assert!(saved["company_state"]["CompanySystem"].is_object());
    for old in ["company_system", "corporate_actions", "company_operations", "closing_registry", "groups"] {
        assert!(!saved.as_object().unwrap().contains_key(old), "旧双源运行字段 {old}");
    }
}

#[test]
fn selected_legacy_save_contains_only_legacy_company_state_and_original_public_source() {
    let mut setup = plain_setup();
    setup.npcs.retail_count = 0;
    setup.history_len = 1;
    let session = GameSession::new(setup, 42).unwrap();
    let saved = session.save().unwrap();
    let value = serde_json::to_value(&saved).unwrap();
    assert!(value["company_state"]["LegacyOperations"].is_object());
    let now = engine::calendar::CivilInstant::new(session.civil_date(), 0).unwrap();
    let reports = saved.public_library.reports_for_company(&engine::company::CompanyId(0), now);
    assert!(!reports.is_empty());
    assert!(reports.iter().all(|r| r.source == PublicationSource::SimulationAccounting));
}

#[test]
fn selected_simple_rejects_legacy_group_configuration_before_any_assembly() {
    let mut setup = setup();
    setup.groups.push(engine::session::GroupStructure {
        root: engine::company::CompanyId(0), holdings: vec![],
    });
    assert!(GameSession::new(setup, 42).is_err(), "所选 Simple 不得执行或静默忽略 Legacy 集团配置");
}

#[test]
fn selected_simple_real_save_roundtrips_only_one_company_authority() {
    let session = GameSession::new(setup(), 42).unwrap();
    let bytes = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["company_state"]["CompanySystem"].is_object());
    let save: engine::SaveSlot = serde_json::from_slice(&bytes).unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(serde_json::to_vec(&restored.save().unwrap()).unwrap(), bytes);
}
