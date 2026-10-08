use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    decode_save_slot, GameSession, NpcSetup, SaveDecodeLimits, SecurityCategory, SessionError,
    SessionSetup, StockExchange, StockSpec,
};
use engine::{AccountId, Intent, Side};

#[macro_use]
#[path = "../test-support/simple_company.rs"]
mod simple_company_fixture;

fn setup(retail_count: u32) -> SessionSetup {
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600101"]),
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 1_000_000,
        }],
        npcs: NpcSetup {
            retail_count,
            inst_count: 1,
            hot_count: 1,
            retail_cash_median: Money::from_cents(10_000_000),
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
        ticks_per_day: 10,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: engine::FloatAllocation::random(),
        start_date: engine::CivilDate::from_iso("2030-01-07").expect("fixture date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
        flat_withholding_bp: Some(1000),
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: engine::Money::from_cents(100),
        auto_corporate_foundation: false,
    }
}

fn large_save() -> (GameSession, engine::SaveSlot) {
    let session = GameSession::new(setup(20_000), 39).expect("large fixture constructs");
    let save = session.save().expect("healthy save");
    assert_eq!(save.snapshot.accounts.len(), 20_003);
    (session, save)
}

#[test]
fn restore_rejects_deleted_account_state_without_mutating_the_source_session() {
    let (session, mut save) = large_save();
    let before =
        serde_json::to_vec(&session.save().expect("healthy save")).expect("source save serializes");
    save.retail_experience.remove(&AccountId(10_000));

    assert!(matches!(
        GameSession::restore(&save),
        Err(SessionError::InvalidSave(message)) if message.contains("retail experience account set")
    ));
    assert_eq!(
        serde_json::to_vec(&session.save().expect("healthy save")).unwrap(),
        before
    );
}

#[test]
fn restore_rejects_tampered_report_reference_without_mutating_the_source_session() {
    let session = GameSession::new(setup(1), 39).expect("fixture constructs");
    let mut save = session.save().expect("healthy save");
    let account = *save
        .information_states
        .keys()
        .next()
        .expect("institution information state exists");
    let company = engine::company::CompanyId("C-600101".to_string());
    let report = *save
        .public_library
        .reports_for_company(
            &company,
            save.public_library
                .latest_published_instant()
                .expect("seeded history contains public reports"),
        )
        .last()
        .expect("seeded company history contains a public report");
    // This tests reference validation, not the institution's random attention
    // schedule. Record a real, already-public report before corrupting its id.
    save.information_states
        .get_mut(&account)
        .unwrap()
        .record_acquisition(
            account,
            &save.public_library,
            report.id,
            report.published_at,
        )
        .expect("institution acquires an already-public report");
    let session = GameSession::restore(&save).expect("acquired-report fixture restores");
    let before =
        serde_json::to_vec(&session.save().expect("healthy save")).expect("source save serializes");
    let mut value: serde_json::Value = serde_json::from_slice(&before).expect("source JSON parses");
    let account = value["information_states"]
        .as_object()
        .and_then(|states| states.keys().next())
        .expect("institution information state exists")
        .clone();
    let company = value["information_states"][&account]["companies"]
        .as_object()
        .and_then(|companies| companies.keys().next())
        .expect("institution acquired a company report")
        .clone();
    let records = value["information_states"][&account]["companies"][&company]
        .as_array_mut()
        .expect("acquisition records are an array");
    let record = records
        .last_mut()
        .expect("institution acquired at least one report");
    record["id"] = serde_json::json!(9_999_999_u64);
    let bytes = serde_json::to_vec(&value).expect("tampered JSON serializes");
    let decoded = decode_save_slot(&bytes, &SaveDecodeLimits::default()).expect("shape decodes");

    assert!(matches!(
        GameSession::restore(&decoded),
        Err(SessionError::InvalidSave(message)) if message.contains("missing from the library")
    ));
    assert_eq!(
        serde_json::to_vec(&session.save().expect("healthy save")).unwrap(),
        before
    );
}

#[test]
fn decode_rejects_configured_body_limit_without_constructing_a_partial_save() {
    let (_, save) = large_save();
    let bytes = serde_json::to_vec(&save).expect("large save serializes");
    let limit = SaveDecodeLimits {
        max_total_bytes: bytes.len() - 1,
    };

    assert!(matches!(
        decode_save_slot(&bytes, &limit),
        Err(SessionError::ResourceLimit(message)) if message.contains("decode limit")
    ));
}

#[test]
fn company_collection_decodes_without_quota_and_restore_checks_stock_mapping() {
    const COMPANY_COUNT: usize = 257;
    let session = GameSession::new(setup(1), 39).expect("small fixture constructs");
    let mut save = session.save().expect("healthy save");
    let stock = &save.setup.stocks[0];
    let start = save.civil_clock.current_date;
    let stock_code = stock.code.clone();
    let total_shares = stock.total_shares;
    let companies: Vec<_> = (0..COMPANY_COUNT)
        .map(|index| engine::company::CompanySpec {
            id: engine::company::CompanyId(format!("collection-{index:03}")),
            name: format!("独立规模测试公司{index}"),
            industry: engine::company::IndustryId("industrial".into()),
            kind: engine::company::CompanyKind::Industrial,
            listed_stock: (index == 0).then(|| stock_code.clone()),
            issued_shares: total_shares,
            group_parent: None,
        })
        .collect();
    let engine::company::config::CompanySystemConfig::Simple(mut config) = simple_company_fixture!(engine; codes = companies.iter().map(|company| company.id.0.clone()))
    else {
        panic!("规模测试必须使用明确的 Simple fixture");
    };
    config.prehistory_periods = 1;
    for (parameters, company) in config.companies.iter_mut().zip(&companies) {
        parameters.company = company.id.clone();
    }
    let config = engine::company::config::CompanySystemConfig::Simple(config);
    let system = engine::company::CompanySystem::create(companies, config, start, 39)
        .expect("独立 Simple 公司系统不因公司数量设置配额");
    let encoded = serde_json::to_vec(&system).unwrap();
    let restored: engine::company::CompanySystem =
        serde_json::from_slice(&encoded).expect("257 家公司结构必须可解码");
    assert_eq!(restored, system);
    let mut conflicting = serde_json::to_value(&system).unwrap();
    for issuer in conflicting["issuers"].as_object_mut().unwrap().values_mut() {
        issuer["listed_stock"] = serde_json::to_value(&stock_code).unwrap();
    }
    let error = serde_json::from_value::<engine::company::CompanySystem>(conflicting).unwrap_err();
    assert!(
        error.to_string().contains("duplicate issuer stock"),
        "{error}"
    );
    let mut mismatched = save
        .company_system
        .issuers()
        .iter()
        .map(|(_, issuer)| issuer.clone())
        .collect::<Vec<_>>();
    mismatched[0].issued_shares += 1;
    save.company_system = engine::company::CompanySystem::create(
        mismatched,
        save.setup.company_system.clone(),
        start,
        39,
    )
    .unwrap();

    let bytes = serde_json::to_vec(&save).expect("company collection serializes");
    let decoded = decode_save_slot(&bytes, &SaveDecodeLimits::default())
        .expect("company count alone must not prevent decoding");
    assert!(matches!(
        GameSession::restore(&decoded),
        Err(SessionError::InvalidSave(message))
            if message == "公司系统发行人身份与股票配置不一致"
    ));
}

#[test]
fn restore_rejects_duplicate_order_id_without_mutating_the_source_session() {
    let mut session = GameSession::new(setup(1), 39).expect("fixture constructs");
    let code = StockCode("600101".to_string());
    for _ in 0..2 {
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: engine::LimitPrice::Fixed(Money::from_cents(1_000)),
                    qty: 100,
                },
            )
            .expect("intent queues");
    }
    session.step().expect("healthy step");
    let before =
        serde_json::to_vec(&session.save().expect("healthy save")).expect("source save serializes");
    let mut save = session.save().expect("healthy save");
    GameSession::restore(&save).expect("two distinct resting orders restore");
    let orders = save.resting_orders.get_mut(&code).unwrap();
    assert_eq!(orders.len(), 2, "both player orders must rest");
    assert!(orders.iter().all(|order| order.owner == AccountId(0)));
    let duplicate_id = orders[0].id;
    assert_ne!(orders[1].id, duplicate_id);
    // Keep sequence, depth and reservations valid; only the identity is corrupt.
    orders[1].id = duplicate_id;

    let error = GameSession::restore(&save)
        .err()
        .expect("duplicate order id must be rejected");
    let expected = format!(
        "cannot restore resting orders for {}: duplicate order id: {duplicate_id:?}",
        code.0
    );
    assert!(
        matches!(&error, SessionError::InvalidSave(message) if message == &expected),
        "unexpected duplicate-order rejection: {error:?}"
    );
    assert_eq!(
        serde_json::to_vec(&session.save().expect("healthy save")).unwrap(),
        before
    );
}
