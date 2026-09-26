use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    decode_save_slot, GameSession, NpcSetup, SaveDecodeLimits, SecurityCategory, SessionError,
    SessionSetup, StockExchange, StockSpec,
};
use engine::{AccountId, Intent, Side};

#[path = "company_operations/fixtures.rs"]
#[allow(dead_code)] // This collection test only needs the industrial opening fixture.
mod company_fixtures;

fn setup(retail_count: u32) -> SessionSetup {
    SessionSetup {
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
        ticks_per_day: 10,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: engine::FloatAllocation::Random,
        start_date: engine::CivilDate::from_iso("2030-01-07").expect("fixture date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V2.to_string(),
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
    let (mut session, _) = large_save();
    for _ in 0..10 {
        session.step().expect("healthy step");
    }
    session
        .end_civil_day()
        .expect("completed civil day settles");
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
    let mut template = company_fixtures::industrial_a(start);
    template.spec.listed_stock = Some(stock.code.clone());
    template.spec.issued_shares = stock.total_shares;
    // Only opening books are needed to exercise collection decoding. Generating two
    // years of company history here would add unrelated work to this short test.
    let companies = (0..COMPANY_COUNT)
        .map(|index| {
            let mut company = template.clone();
            company.spec.id = engine::company::CompanyId(format!("collection-{index:03}"));
            company
        })
        .collect();
    save.company_operations = engine::company::CompanyOperations::new(
        engine::company::CompanyOperationsConfig {
            seed: 39,
            shock_params: company_fixtures::quiet_params(),
            companies,
        },
        start,
    )
    .expect("independent opening books construct");

    let bytes = serde_json::to_vec(&save).expect("company collection serializes");
    let decoded = decode_save_slot(&bytes, &SaveDecodeLimits::default())
        .expect("company count alone must not prevent decoding");
    // The collection is structurally decodable, but these companies deliberately
    // share one listed stock. Removing the quota must preserve this actual error.
    assert!(matches!(
        GameSession::restore(&decoded),
        Err(SessionError::InvalidSave(message))
            if message.contains("share count or mapping conflicts with setup stock")
    ));
}

#[test]
fn restore_rejects_duplicate_order_book_sequence_without_mutating_the_source_session() {
    let mut session = GameSession::new(setup(1), 39).expect("fixture constructs");
    let code = StockCode("600101".to_string());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .expect("intent queues");
    session.step().expect("healthy step");
    let before =
        serde_json::to_vec(&session.save().expect("healthy save")).expect("source save serializes");
    let mut save = session.save().expect("healthy save");
    let order = save.resting_orders[&code][0].clone();
    save.resting_orders.get_mut(&code).unwrap().push(order);
    save.snapshot.markets.get_mut(&code).unwrap().bids[0].1 = 200;

    assert!(matches!(
        GameSession::restore(&save),
        Err(SessionError::InvalidSave(message)) if message.contains("duplicate saved order id")
    ));
    assert_eq!(
        serde_json::to_vec(&session.save().expect("healthy save")).unwrap(),
        before
    );
}
