use super::{command_builder, parse_generation, SpeedRequest};
use engine::{
    FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcDecisionDiagnostics, NpcSetup,
    RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
};
use serde_json::{json, Value};
include!("../../../../packages/engine/test-support/simple_company.rs");

#[tokio::test]
async fn company_read_ipc_registers_six_queries_and_preserves_typed_rejections() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut setup = diagnostic_setup();
    setup.npcs.inst_count = 0;
    setup.stocks[0].float_shares = 0;
    setup.history_len = 1;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({"setup":setup,"seed":"7"}),
    )
    .unwrap();
    for query in [
        json!({"kind":"CompanyPeriodExplanation","data":{"company":0,"period_end":"2029-12-31"}}),
        json!({"kind":"AccountDividendTaxStatus"}),
        json!({"kind":"DividendTaxOutstandingViews"}),
        json!({"kind":"OwnerFlatWithholdingReceipts"}),
        json!({"kind":"CompanyCapabilities","data":{"company":0}}),
        json!({"kind":"OwnerRightsOfferings"}),
    ] {
        let response = invoke_json(
            &webview,
            "company_read_query",
            json!({"sessionId":session_id,"generation":"1","query":query}),
        )
        .unwrap();
        assert_eq!(response["generation"], "1");
        assert_eq!(response["owner"], "0");
        assert_eq!(response["value"]["kind"], query["kind"]);
        println!("COMPANY_READ_TAURI_FIXTURE {response}");
    }
    let error = invoke_json(&webview,"company_read_query",json!({"sessionId":session_id,"generation":"1","query":{"kind":"CompanyPeriodExplanation","data":{"company":0,"period_end":"2030-01-31"}}})).unwrap_err();
    assert_eq!(error["kind"], "Query");
    assert_eq!(error["data"]["kind"], "Company");
    assert_eq!(error["data"]["data"]["classification"], "BusinessCondition");
    assert!(error["data"]["data"]["reason"]
        .as_str()
        .unwrap()
        .contains("尚未结算"));
    println!("COMPANY_READ_TAURI_FAILURE {error}");
    let error = invoke_json(&webview,"company_read_query",json!({"sessionId":session_id,"generation":"999","query":{"kind":"AccountDividendTaxStatus"}})).unwrap_err();
    assert_eq!(error["kind"], "Session");
    invoke_json(&webview, "stop_session", json!({"sessionId":session_id})).unwrap();
}
#[cfg(all(feature = "simulation-diagnostics", debug_assertions))]
use std::time::Duration;
use tauri::{
    ipc::{CallbackFn, InvokeBody},
    test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY},
    webview::InvokeRequest,
    WebviewWindow, WebviewWindowBuilder,
};

pub(super) fn diagnostic_setup() -> SessionSetup {
    SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
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
            retail_count: 0,
            inst_count: 1,
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
        start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
    }
}

fn ipc_request(command: &str, body: Value) -> InvokeRequest {
    InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_owned(),
    }
}

pub(super) fn invoke_json(
    webview: &WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(webview, ipc_request(command, body))
        .map(|response| response.deserialize::<Value>().unwrap())
}

#[tokio::test]
async fn enqueue_ipc_requires_current_canonical_generation() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut setup = diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    let mut completed = engine::session::protocol::ProtocolSession::new(setup.clone(), 7).unwrap();
    completed.end_civil_day_update().unwrap();
    let stock_id = completed.stock_id("600101").unwrap();
    assert_eq!(completed.stock_catalog().code(stock_id), Some("600101"));
    let slot = completed.save().unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({ "setup": setup, "seed": "7" }),
    )
    .unwrap();
    invoke_json(
        &webview,
        "restore_session",
        json!({ "sessionId": session_id, "generation": "1", "slot": slot }),
    )
    .unwrap();
    let intent = json!({ "PlaceLimit": { "code": stock_id, "side": "Buy", "price": { "Fixed": "1000" }, "qty": 100 } });
    for generation in [
        Some(json!("1")),
        None,
        Some(json!("01")),
        Some(json!(1)),
        Some(json!("18446744073709551616")),
    ] {
        let mut body = json!({ "sessionId": session_id, "intent": intent });
        if let Some(generation) = generation {
            body["generation"] = generation;
        }
        assert!(
            invoke_json(&webview, "enqueue", body).is_err(),
            "旧 generation、缺失或非法 generation 必须在 intake 前拒绝"
        );
    }
    invoke_json(
        &webview,
        "enqueue",
        json!({ "sessionId": session_id, "generation": "2", "intent": intent }),
    )
    .unwrap();
    invoke_json(&webview, "stop_session", json!({ "sessionId": session_id })).unwrap();
}

#[tokio::test]
async fn save_generation_rejects_same_date_seq_edited_assets_after_restore() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut setup = diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    let mut game = engine::session::protocol::ProtocolSession::new(setup.clone(), 7).unwrap();
    game.end_civil_day_update().unwrap();
    let slot = game.save().unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({ "setup": setup, "seed": "7" }),
    )
    .unwrap();
    invoke_json(
        &webview,
        "restore_session",
        json!({ "sessionId": session_id, "generation": "1", "slot": slot }),
    )
    .unwrap();
    let mut edited = slot.clone();
    let account = edited
        .snapshot
        .accounts
        .get_mut(&engine::AccountId(0))
        .unwrap();
    account.cash = account.cash.add(Money::from_cents(100)).unwrap();
    invoke_json(
        &webview,
        "restore_session",
        json!({ "sessionId": session_id, "generation": "2", "slot": edited }),
    )
    .unwrap();
    for candidate in [
        json!({ "seq": slot.snapshot.seq, "settledDate": "2030-01-05" }),
        Value::Null,
    ] {
        let result = invoke_json(
            &webview,
            "save_session",
            json!({ "sessionId": session_id, "generation": "2", "candidate": candidate }),
        );
        assert!(result.is_err(), "stale save must not expose edited assets");
    }
    let saved = invoke_json(
        &webview,
        "save_session",
        json!({ "sessionId": session_id, "generation": "3" }),
    )
    .unwrap();
    assert_eq!(saved, serde_json::to_value(edited).unwrap());
}

#[test]
fn desktop_speed_protocol_accepts_fixed_and_fastest_json() {
    let fixed: SpeedRequest = serde_json::from_str(r#"{"Fixed":360}"#).unwrap();
    assert!(matches!(fixed, SpeedRequest::Fixed(value) if value == 360.0));
    let fastest: SpeedRequest = serde_json::from_str(r#""Fastest""#).unwrap();
    assert!(matches!(fastest, SpeedRequest::Fastest));
}

#[test]
fn generation_protocol_requires_canonical_lossless_decimal_u64() {
    assert_eq!(parse_generation(u64::MAX.to_string()), Ok(u64::MAX));
    for malformed in ["", "+1", " 1", "01", "1 ", "18446744073709551616"] {
        assert!(
            parse_generation(malformed.to_owned()).is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn release_diagnostics_result_has_no_private_trace_field() {
    let value = serde_json::to_value(NpcDecisionDiagnostics::Unsupported).unwrap();
    assert_eq!(value, json!({ "kind": "unsupported" }));
    assert!(value.get("records").is_none());
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn host_parity_ipc_advances_one_closed_civil_day_through_the_actor() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({ "setup": diagnostic_setup(), "seed": "7" }),
    )
    .unwrap()
    .as_str()
    .unwrap()
    .to_owned();

    let report = invoke_json(
        &webview,
        "host_parity_advance_civil_day",
        json!({ "sessionId": session_id, "generation": "1" }),
    )
    .unwrap();
    assert!(report["events"].as_array().is_some_and(|events| events
        .iter()
        .any(|event| event.get("CivilDateAdvanced").is_some())));
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn host_parity_ipc_steps_a_normal_market_day_before_civil_settlement() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut setup = diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({ "setup": setup, "seed": "7" }),
    )
    .unwrap()
    .as_str()
    .unwrap()
    .to_owned();

    for _ in 0..30 {
        assert!(invoke_json(
            &webview,
            "host_parity_step",
            json!({ "sessionId": session_id, "generation": "1" }),
        )
        .is_ok());
    }
    let report = invoke_json(
        &webview,
        "host_parity_advance_civil_day",
        json!({ "sessionId": session_id, "generation": "1" }),
    )
    .unwrap();

    assert!(report["events"].as_array().is_some_and(|events| events
        .iter()
        .any(|event| event.get("CivilDateAdvanced").is_some())));
}

#[tokio::test]
async fn diagnostics_ipc_rejects_malformed_account_and_stale_generation_before_returning_data() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({ "setup": diagnostic_setup(), "seed": "7" }),
    )
    .unwrap()
    .as_str()
    .unwrap()
    .to_owned();

    let malformed = invoke_json(
        &webview,
        "npc_decision_diagnostics",
        json!({ "sessionId": session_id, "generation": "1", "account": "unsafe" }),
    )
    .unwrap_err();
    assert!(malformed.to_string().contains("account"));

    assert_eq!(
        invoke_json(
            &webview,
            "set_speed",
            json!({ "sessionId": session_id, "speed": { "Fixed": 720 } }),
        ),
        Ok(Value::Null)
    );
    assert_eq!(
        invoke_json(
            &webview,
            "resume_session",
            json!({ "sessionId": session_id }),
        ),
        Ok(Value::Null)
    );
    #[cfg(all(feature = "simulation-diagnostics", debug_assertions))]
    {
        let current = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let current = invoke_json(
                    &webview,
                    "npc_decision_diagnostics",
                    json!({ "sessionId": session_id, "generation": "1", "account": 1 }),
                )
                .unwrap();
                if current["value"]["records"]
                    .as_array()
                    .is_some_and(|records| !records.is_empty())
                {
                    break current;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("NPC decision diagnostics did not record an account decision within 2 seconds");
        assert_eq!(current["generation"], "1");
        assert!(current["value"]["records"]
            .as_array()
            .is_some_and(|records| {
                !records.is_empty() && records.len() <= engine::MAX_NPC_DECISION_TRACE_RECORDS
            }));
        let stale = invoke_json(
            &webview,
            "npc_decision_diagnostics",
            json!({ "sessionId": session_id, "generation": "0", "account": 1 }),
        )
        .unwrap_err();
        assert!(stale.to_string().contains("stale session generation 0"));
    }
    #[cfg(not(all(feature = "simulation-diagnostics", debug_assertions)))]
    for generation in ["1", "0"] {
        let disabled = invoke_json(
            &webview,
            "npc_decision_diagnostics",
            json!({ "sessionId": session_id, "generation": generation, "account": 1 }),
        )
        .unwrap_err();
        assert_eq!(
            disabled,
            json!("NPC 决策诊断仅在启用开发诊断的非 release 桌面端可用")
        );
    }

    let stopped = invoke_json(&webview, "stop_session", json!({ "sessionId": session_id }));
    assert_eq!(stopped, Ok(Value::Null));
}

#[test]
fn catalog_ipc_error_keeps_structured_fault_and_existing_string_shape() {
    use crate::{map_baseline_error, BaselineIpcError};
    let leaf = engine::session::CompanyCatalogViewError::UnknownListedStock {
        company: engine::company::CompanyId(7),
    };
    let error =
        crate::actor::BaselineRequestError::Baseline(crate::actor::BaselineError::Catalog(leaf));
    let value = serde_json::to_value(map_baseline_error(
        error,
        "restore",
        "desktop.actor.restore",
    ))
    .unwrap();
    assert_eq!(value["code"], "CATALOG_BASELINE_FAILED");
    assert_eq!(
        value["context"]["fault"],
        serde_json::json!({"code":3019,"context":[7]})
    );
    assert_eq!(
        serde_json::to_value(BaselineIpcError::Existing("旧错误".to_owned())).unwrap(),
        serde_json::json!("旧错误")
    );
}
