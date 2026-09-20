use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange, StockSpec,
};
#[path = "company_operations/fixtures.rs"]
mod four_industry_fixtures;

const SEED: u64 = 39;
const SERVER_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

#[test]
fn four_industry_fixture_catalog_remains_constructible() {
    let start = four_industry_fixtures::d(four_industry_fixtures::START);
    let _ = four_industry_fixtures::industrial_b(start);
    let _ = four_industry_fixtures::industrial_broke(start);
    let _ = four_industry_fixtures::quiet_params();
    let _ = four_industry_fixtures::four_company_config(
        39,
        four_industry_fixtures::quiet_params(),
        start,
    );
    let _ = four_industry_fixtures::two_industrial_config(
        39,
        four_industry_fixtures::quiet_params(),
        start,
    );
}

fn stock(code: &str, price_cents: i64, category: SecurityCategory, total_shares: u64) -> StockSpec {
    StockSpec {
        code: StockCode(code.to_string()),
        exchange: if code.starts_with('6') {
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

fn default_five_stock_setup(retail_count: u32, ticks_per_day: u64) -> SessionSetup {
    SessionSetup {
        stocks: vec![
            stock("600101", 1_120, SecurityCategory::MainBoard, 8_928_571_429),
            stock("002156", 2_735, SecurityCategory::MainBoard, 2_925_045_704),
            stock("300260", 3_680, SecurityCategory::ChiNext, 815_217_391),
            stock("600610", 755, SecurityCategory::MainBoard, 1_059_602_649),
            stock("000812", 285, SecurityCategory::StMainBoard, 1_052_631_579),
        ],
        npcs: NpcSetup {
            retail_count,
            inst_count: 5,
            hot_count: 2,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
                tick_cents: 1,
            },
            inst: engine::InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: engine::HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: engine::FloatAllocation::ByKind {
            retail: 0.45,
            inst: 0.53,
            hot: 0.02,
        },
        start_date: engine::CivilDate::from_iso("2030-01-07").expect("fixture date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V2.to_string(),
    }
}

fn settle_natural_days(session: &mut GameSession, days: u32, ticks_per_day: u64) {
    for _ in 0..days {
        if matches!(
            session.civil_clock().phase(),
            engine::session::CivilPhase::IntradayTrading
        ) {
            for _ in 0..ticks_per_day {
                session.step().expect("healthy step");
            }
        }
        session.end_civil_day().expect("natural day settles");
    }
}

#[test]
#[ignore = "20k default-five-stock 90-natural-day cross-quarter report and plan retention gate"]
fn twenty_thousand_accounts_cross_quarter_preserves_reports_and_plans() {
    let setup = default_five_stock_setup(20_000, 10);
    let ticks_per_day = setup.ticks_per_day;
    let mut uninterrupted = GameSession::new(setup, SEED).expect("large session constructs");
    settle_natural_days(&mut uninterrupted, 45, ticks_per_day);
    let midpoint = uninterrupted.save().expect("healthy save");
    let midpoint_json = serde_json::to_vec(&midpoint).expect("midpoint save serializes");
    let decoded =
        engine::decode_save_slot(&midpoint_json, &Default::default()).expect("save decodes");
    let mut restored = GameSession::restore(&decoded).expect("save restores");
    settle_natural_days(&mut uninterrupted, 45, ticks_per_day);
    settle_natural_days(&mut restored, 45, ticks_per_day);
    let uninterrupted_save = uninterrupted.save().expect("healthy save");
    let restored_save = restored.save().expect("healthy save");

    assert_eq!(uninterrupted_save.snapshot.accounts.len(), 20_008);
    assert!(
        uninterrupted_save.plans.plan_ids().next().is_some(),
        "cross-quarter decision-chain plans must remain authoritative"
    );
    assert_eq!(
        uninterrupted_save.npc_attention,
        restored_save.npc_attention
    );
    assert_eq!(
        uninterrupted_save.retail_experience,
        restored_save.retail_experience
    );
    assert_eq!(
        uninterrupted_save.public_library.save(),
        restored_save.public_library.save()
    );
    assert_eq!(uninterrupted_save.plans, restored_save.plans);
    assert_eq!(
        serde_json::to_vec(&uninterrupted_save).unwrap(),
        serde_json::to_vec(&restored_save).unwrap()
    );
    eprintln!(
        "company_scale_resource_measurement scenario=twenty_thousand_cross_quarter accounts=20008 natural_days=90 reports={} plans={} save_bytes={} server_body_limit_bytes={SERVER_BODY_LIMIT_BYTES} server_body_fit={}",
        uninterrupted_save.public_library.report_count(),
        uninterrupted_save.plans.plan_ids().count(),
        serde_json::to_vec(&uninterrupted_save).unwrap().len(),
        serde_json::to_vec(&uninterrupted_save).unwrap().len() <= SERVER_BODY_LIMIT_BYTES,
    );
}

#[test]
#[ignore = "100k high-attention multi-plan save peak gate"]
fn one_hundred_thousand_high_attention_multi_plan_save_peak() {
    let mut uninterrupted = GameSession::new(default_five_stock_setup(100_000, 10), SEED)
        .expect("large session constructs");
    for _ in 0..10 {
        uninterrupted.step().expect("healthy step");
    }
    let save = uninterrupted.save().expect("healthy save");
    let started = std::time::Instant::now();
    let bytes = serde_json::to_vec(&save).expect("high-attention save serializes");
    let serialization_elapsed = started.elapsed();
    let restore_started = std::time::Instant::now();
    let decoded = engine::decode_save_slot(&bytes, &Default::default()).expect("save decodes");
    let mut restored = GameSession::restore(&decoded).expect("save restores");
    let restore_elapsed = restore_started.elapsed();

    assert_eq!(
        uninterrupted
            .save()
            .expect("healthy save")
            .npc_attention
            .len(),
        100_007
    );
    assert!(
        uninterrupted
            .save()
            .expect("healthy save")
            .plans
            .plan_ids()
            .count()
            >= 5,
        "the real high-attention day must retain multiple authoritative plans"
    );
    let uninterrupted_events = uninterrupted.step().expect("healthy step");
    let restored_events = restored.step().expect("healthy step");
    assert_eq!(
        serde_json::to_vec(&uninterrupted_events).unwrap(),
        serde_json::to_vec(&restored_events).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&uninterrupted.save().expect("healthy save")).unwrap(),
        serde_json::to_vec(&restored.save().expect("healthy save")).unwrap()
    );
    eprintln!(
        "company_scale_resource_measurement scenario=one_hundred_thousand_high_attention_multi_plan accounts=100008 attention_accounts=100007 plans={} save_bytes={} server_body_limit_bytes={SERVER_BODY_LIMIT_BYTES} server_body_fit={} serialize_ms={} restore_ms={}",
        uninterrupted.save().expect("healthy save").plans.plan_ids().count(),
        bytes.len(),
        bytes.len() <= SERVER_BODY_LIMIT_BYTES,
        serialization_elapsed.as_millis(),
        restore_elapsed.as_millis(),
    );
}

#[test]
#[ignore = "four-industry small-account ten-year archive cost gate"]
fn four_industry_ten_year_archive_preserves_each_industry_state() {
    let start = four_industry_fixtures::d("2030-01-01");
    let config = engine::company::operations::CompanyOperationsConfig {
        seed: SEED,
        shock_params: engine::company::events::ShockParams::default_v1(),
        companies: vec![
            four_industry_fixtures::industrial_a(start),
            four_industry_fixtures::bank_c(start),
            four_industry_fixtures::insurance_c(start),
            four_industry_fixtures::real_estate_c(start),
        ],
    };
    let mut operations = engine::company::operations::CompanyOperations::new(config, start)
        .expect("four industry operations construct");
    let started = std::time::Instant::now();
    let mut date = start;
    for _ in 0..3_652 {
        operations
            .advance_civil_day(date)
            .expect("four industry archive day advances");
        date = date.next().expect("archive date remains valid");
    }
    let bytes = serde_json::to_vec(&operations).expect("archive serializes");
    let mut restored: engine::company::operations::CompanyOperations =
        serde_json::from_slice(&bytes).expect("archive decodes");

    assert_eq!(operations, restored);
    let uninterrupted_day = operations
        .advance_civil_day(date)
        .expect("archive source continues after checkpoint");
    let restored_day = restored
        .advance_civil_day(date)
        .expect("restored archive continues after checkpoint");
    assert_eq!(uninterrupted_day, restored_day);
    assert_eq!(operations, restored);
    let value = serde_json::to_value(&restored).expect("archive value serializes");
    assert_eq!(value["companies"].as_object().unwrap().len(), 4);
    eprintln!(
        "company_scale_resource_measurement scenario=four_industry_ten_year_archive companies=4 natural_days=3652 archive_bytes={} advance_ms={}",
        bytes.len(),
        started.elapsed().as_millis(),
    );
}
