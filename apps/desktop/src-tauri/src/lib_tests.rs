use super::{command_builder, parse_generation, SpeedRequest};
use engine::{
    FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcDecisionDiagnostics, NpcSetup,
    RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{
    ipc::{CallbackFn, InvokeBody},
    test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY},
    webview::InvokeRequest,
    WebviewWindow, WebviewWindowBuilder,
};

fn diagnostic_setup() -> SessionSetup {
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

fn invoke_json(
    webview: &WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(webview, ipc_request(command, body))
        .map(|response| response.deserialize::<Value>().unwrap())
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
    tokio::time::sleep(Duration::from_millis(20)).await;

    let current = invoke_json(
        &webview,
        "npc_decision_diagnostics",
        json!({ "sessionId": session_id, "generation": "1", "account": 1 }),
    )
    .unwrap();
    assert_eq!(current["generation"], "1");

    #[cfg(not(feature = "simulation-diagnostics"))]
    assert_eq!(current["value"], json!({ "kind": "unsupported" }));

    #[cfg(feature = "simulation-diagnostics")]
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

    let stopped = invoke_json(&webview, "stop_session", json!({ "sessionId": session_id }));
    assert_eq!(stopped, Ok(Value::Null));
}
