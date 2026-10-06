include!("../test-support/simple_company.rs");

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange, StockSpec,
};
use engine::MAX_SAVE_DECODE_BYTES;
#[path = "company_operations/fixtures.rs"]
mod four_industry_fixtures;

const SEED: u64 = 39;

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
        company_system: simple_company_fixture!(engine; ["600101", "002156", "300260", "600610", "000812"]),
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
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: engine::FloatAllocation::class_percentages(0.45, 0.53, 0.02, engine::WithinKindDistribution::Random),
        start_date: engine::CivilDate::from_iso("2030-01-07").expect("fixture date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
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

fn restore_byte_identically(bytes: &[u8], boundary: &str) -> GameSession {
    let decoded = engine::decode_save_slot(bytes, &Default::default())
        .unwrap_or_else(|error| panic!("{boundary}: save decodes: {error}"));
    let restored = GameSession::restore(&decoded)
        .unwrap_or_else(|error| panic!("{boundary}: save restores: {error}"));
    let restored_bytes = serde_json::to_vec(&restored.save().expect("restored save is healthy"))
        .expect("restored save serializes");
    assert!(
        bytes == restored_bytes,
        "{boundary}: restore followed by immediate resave must preserve every byte"
    );
    restored
}

#[test]
fn short_checkpoint_preserves_bytes_and_rejects_unknown_plan_references() {
    // 单股、两个 tick 足以生成真实计划，避免短引用校验重复构建五股历史。
    let mut setup = default_five_stock_setup(2, 2);
    setup.stocks.truncate(1);
    setup.company_system = simple_company_fixture!(engine; codes = setup.stocks.iter().map(|stock| stock.code.0.as_str()));
    let mut session = GameSession::new(setup, SEED).expect("representative session constructs");
    settle_natural_days(&mut session, 1, 2);
    let save = session.save().expect("healthy save");
    let bytes = serde_json::to_vec(&save).expect("save serializes");
    restore_byte_identically(&bytes, "representative checkpoint");
    let plan_id = save
        .plans
        .plan_ids()
        .next()
        .expect("fixture retains a plan");
    let original = serde_json::to_value(&save).expect("save value serializes");

    // 保持 PlanBook 本身可解码，只破坏计划对会话权威账户/股票的引用。
    for (field, value, expected_error) in [
        (
            "account",
            serde_json::to_value(engine::AccountId(999_999)).unwrap(),
            "unknown account",
        ),
        (
            "code",
            serde_json::to_value(StockCode("600999".to_string())).unwrap(),
            "unknown stock",
        ),
    ] {
        let mut tampered = original.clone();
        tampered["plans"]["plans"][plan_id.0.to_string()][field] = value;
        let tampered_bytes = serde_json::to_vec(&tampered).expect("tampered save serializes");
        let decoded = engine::decode_save_slot(&tampered_bytes, &Default::default())
            .expect("plan reference tampering remains structurally decodable");
        assert!(
            matches!(GameSession::restore(&decoded), Err(engine::session::SessionError::InvalidSave(message)) if message.contains(expected_error)),
            "restore must reject plan {field} reference tampering as {expected_error}"
        );
    }
    assert_eq!(
        bytes,
        serde_json::to_vec(&session.save().expect("source save remains healthy")).unwrap(),
        "rejected restores must leave the source checkpoint unchanged"
    );
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
    let mut restored = restore_byte_identically(&midpoint_json, "45-natural-day checkpoint");
    settle_natural_days(&mut uninterrupted, 45, ticks_per_day);
    settle_natural_days(&mut restored, 45, ticks_per_day);
    let uninterrupted_save = uninterrupted.save().expect("healthy save");
    let restored_save = restored.save().expect("healthy save");

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
    // ADR-0017/0018：自由调度可改变后续 PlanId 分配；逐实例验证已发生事实
    // 的完整恢复，不能用两个未来时间线的整档相等强加交易受理顺序。
    let final_date = engine::CivilDate::from_iso("2030-04-07").unwrap();
    let settled_date = engine::CivilDate::from_iso("2030-04-06").unwrap();
    let published_through = engine::CivilInstant::from_hms(settled_date, 18, 0, 0).unwrap();
    for (boundary, save) in [
        (
            "uninterrupted 90-natural-day checkpoint",
            &uninterrupted_save,
        ),
        ("restored 90-natural-day checkpoint", &restored_save),
    ] {
        assert_eq!(save.snapshot.accounts.len(), 20_008, "{boundary}");
        assert_eq!(save.civil_clock.current_date, final_date, "{boundary}");
        assert_eq!(
            save.civil_clock.settled_through,
            Some(settled_date),
            "{boundary}"
        );
        assert!(
            save.plans.plan_ids().next().is_some(),
            "{boundary}: cross-quarter plans must remain authoritative"
        );
        assert!(save.plans.plan_ids().count() >= midpoint.plans.plan_ids().count());
        for plan_id in save.plans.plan_ids() {
            let plan = save.plans.plan(plan_id).expect("plan id resolves");
            assert!(save.snapshot.accounts.contains_key(&plan.account()));
            assert!(save.snapshot.markets.contains_key(plan.code()));
        }
        for plan_id in midpoint.plans.plan_ids() {
            let before = midpoint
                .plans
                .plan(plan_id)
                .expect("midpoint plan resolves");
            let retained = save.plans.plan(plan_id).expect("midpoint plan is retained");
            assert_eq!(retained.account(), before.account());
            assert_eq!(retained.code(), before.code());
            assert_eq!(retained.created_trading_day(), before.created_trading_day());
        }
        for report in midpoint.public_library.save().reports {
            assert_eq!(
                save.public_library
                    .report(report.id, published_through)
                    .expect("midpoint report remains published"),
                &report,
                "{boundary}: published report versions must remain immutable"
            );
        }
        for report in save.public_library.save().reports {
            assert!(report.published_at <= published_through);
            assert!(report.approved_at <= report.published_at);
        }
        let bytes = serde_json::to_vec(save).expect("final save serializes");
        assert!(bytes.len() <= MAX_SAVE_DECODE_BYTES, "{boundary}");
        // save 校验完整 envelope ledger；decode/restore 再执行跨层权威引用校验。
        restore_byte_identically(&bytes, boundary);
    }
    eprintln!(
        "company_scale_resource_measurement scenario=twenty_thousand_cross_quarter accounts=20008 natural_days=90 reports={} plans={} save_bytes={} decode_limit_bytes={MAX_SAVE_DECODE_BYTES} decode_limit_fit={}",
        uninterrupted_save.public_library.report_count(),
        uninterrupted_save.plans.plan_ids().count(),
        serde_json::to_vec(&uninterrupted_save).unwrap().len(),
        serde_json::to_vec(&uninterrupted_save).unwrap().len() <= MAX_SAVE_DECODE_BYTES,
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
        "company_scale_resource_measurement scenario=one_hundred_thousand_high_attention_multi_plan accounts=100008 attention_accounts=100007 plans={} save_bytes={} decode_limit_bytes={MAX_SAVE_DECODE_BYTES} decode_limit_fit={} serialize_ms={} restore_ms={}",
        uninterrupted.save().expect("healthy save").plans.plan_ids().count(),
        bytes.len(),
        bytes.len() <= MAX_SAVE_DECODE_BYTES,
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
        shock_params: engine::company::events::ShockParams::current_default_parameters(),
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
