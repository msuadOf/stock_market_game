//! WS-5 后端契约集成测试（与前端 RemoteHost 严格对齐）。
//!
//! 契约（见任务详情）：
//! - POST /api/new     body {setup, seed}        -> 200 {session_id} | 400
//! - POST /api/intent  body {session_id, intent} -> 200 | 404(未知session) | 400
//! - GET  /api/snapshot?session_id=..           -> 200 Snapshot | 404
//! - POST /api/speed   body {session_id, speed}  -> 200
//! - GET  /api/speed?session_id=..              -> 200 SpeedMetrics | 404
//! - WS   /ws?session_id=..                       -> Bearer 鉴权后先发 public baseline，再持续推 PublisherFrame
//!
//! 复用 engine 既有 serde 类型（server 是 Rust，engine 作 rlib 依赖，无 TS）。
//! 这里直接构造一个合法 SessionSetup JSON（与 engine/tests/session.rs 的 sample_setup 等价）。

use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use engine::account::StockCode;
use engine::money::Money;
use engine::strategy::Intent;
use engine::Side;
use serde_json::{json, Value};
use server::app_router;
use tower::ServiceExt;

/// 构造一个合法的最小 SessionSetup JSON（对齐 engine/tests/session.rs sample_setup）。
fn sample_setup_json() -> Value {
    json!({
        "stocks": [{
            "code": "600101",
            "exchange": "Shanghai",
            "category": "MainBoard",
            "initial_price": 1000,
            "limit_pct": 0.10,
            "tick": 1,
            "total_shares": "10000000",
            "float_shares": 0
        }],
        "npcs": {
            "retail_count": 2,
            "inst_count": 1,
            "hot_count": 1,
            "retail_cash_median": 10_000_000
        },
        "config": engine::GameConfig::proposed_defaults(),
        "strategy_params": {
            "retail": { "arrival_rate": 0.5, "order_size_mean": 100, "chase_prob": 0.2, "tick_cents": 1 },
            "inst":   { "margin": 0.05, "order_size": 200 },
            "hot":    { "lookback": 3, "trend_threshold": 0.02, "order_size": 200 }
        },
        "ticks_per_day": 10,
        "auction_ticks": 0,
        "closing_auction_ticks": 0,
        "history_len": 5,
        "t1_enabled": true,
        "float_allocation": "Random",
        "simulation_policy_id": engine::SIMULATION_POLICY_ID_V1
    })
}

// 验证 engine serde 表示与 JSON 形态一致（早失败：若 engine 改了 serde 表示，这里先红）。
#[test]
fn engine_setup_roundtrips_json() {
    let v = sample_setup_json();
    let setup: engine::SessionSetup =
        serde_json::from_value(v).expect("JSON 应可反序列化为 SessionSetup");
    let s = engine::GameSession::new(setup, 42).expect("应可构造 GameSession");
    assert_eq!(s.market_count(), 1);
    assert_eq!(s.account_count(), 5);
}

#[tokio::test]
async fn healthz_still_ok() {
    // 回归：/healthz 不被新路由破坏。
    let res = app_router()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::OK);
}

// --- POST /api/new ---

async fn new_session(app: axum::Router, body: Value) -> (StatusCode, Value) {
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/new")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("读取 body 失败");
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn new_session_credentials(app: axum::Router) -> (String, String) {
    let (status, body) =
        new_session(app, json!({ "setup": sample_setup_json(), "seed": "42" })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "session creation must succeed: {body}"
    );
    let session_id = body["session_id"]
        .as_str()
        .expect("new session returns its id")
        .to_owned();
    let session_token = body["session_token"]
        .as_str()
        .expect("new session returns its session token")
        .to_owned();
    (session_id, session_token)
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn host_parity_civil_day_route_requires_bearer_authentication_and_uses_the_actor() {
    let app = app_router();
    let (session_id, session_token) = new_session_credentials(app.clone()).await;
    let request_body = json!({ "session_id": session_id, "generation": "1" });

    let missing_auth = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/host-parity/advance-civil-day")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    let (missing_auth_status, _) = response_json(missing_auth).await;
    assert_eq!(missing_auth_status, StatusCode::UNAUTHORIZED);

    let success = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/host-parity/advance-civil-day")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {session_token}"))
                .body(axum::body::Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    let (success_status, report) = response_json(success).await;
    assert_eq!(success_status, StatusCode::OK);
    assert!(report["events"].as_array().is_some_and(|events| events
        .iter()
        .any(|event| event.get("CivilDateAdvanced").is_some())));
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn host_parity_civil_day_route_rejects_a_stale_timeline_generation() {
    let app = app_router();
    let (session_id, session_token) = new_session_credentials(app.clone()).await;
    let request = Request::builder()
        .method("POST")
        .uri("/api/host-parity/advance-civil-day")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {session_token}"))
        .body(axum::body::Body::from(
            json!({ "session_id": session_id, "generation": "0" }).to_string(),
        ))
        .unwrap();

    let response = app
        .oneshot(request)
        .await
        .expect("request must return a response");
    let (status, error) = response_json(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "CIVIL_DAY_REJECTED");
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn host_parity_step_route_completes_a_normal_market_day_before_civil_settlement() {
    let app = app_router();
    let mut setup = sample_setup_json();
    setup["start_date"] = json!("2030-01-02");
    let (new_status, new_body) =
        new_session(app.clone(), json!({ "setup": setup, "seed": "42" })).await;
    assert_eq!(new_status, StatusCode::OK);
    let session_id = new_body["session_id"].as_str().unwrap().to_owned();
    let session_token = new_body["session_token"].as_str().unwrap().to_owned();
    let step_body = json!({ "session_id": session_id, "generation": "1" });

    for _ in 0..10 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/host-parity/step")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {session_token}"))
                    .body(axum::body::Body::from(step_body.to_string()))
                    .unwrap(),
            )
            .await
            .expect("step must return a response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let settle = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/host-parity/advance-civil-day")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {session_token}"))
                .body(axum::body::Body::from(step_body.to_string()))
                .unwrap(),
        )
        .await
        .expect("settlement must return a response");
    let (status, report) = response_json(settle).await;

    assert_eq!(status, StatusCode::OK);
    assert!(report["events"].as_array().is_some_and(|events| events
        .iter()
        .any(|event| event.get("CivilDateAdvanced").is_some())));
}

async fn response_json(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let value = serde_json::from_slice(&body).expect("response body should be JSON");
    (status, value)
}

#[tokio::test]
async fn new_session_returns_200_with_id() {
    let (status, body) = new_session(
        app_router(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "/api/new 合法 body 应 200: {body}");
    let id = body
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("应返回 session_id 字符串");
    assert!(!id.is_empty(), "session_id 非空");
}

#[tokio::test]
async fn new_session_rejects_invalid_setup_with_400() {
    // 空 stocks -> engine::GameSession::new 返回 InvalidSetup -> 400（铁律二：不静默）。
    let mut bad = sample_setup_json();
    bad["stocks"] = json!([]);
    let (status, body) = new_session(app_router(), json!({ "setup": bad, "seed": "42" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "非法 setup 应 400: {body}");
}

#[tokio::test]
async fn new_session_rejects_setup_that_exceeds_server_resource_budget() {
    let mut bad = sample_setup_json();
    bad["history_len"] = json!(1_000_001);

    let (status, body) = new_session(app_router(), json!({ "setup": bad, "seed": "42" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "SETUP_RESOURCE_LIMIT");
}

#[tokio::test]
async fn new_session_rejects_excessive_strategy_work_at_maximum_speed() {
    let mut bad = sample_setup_json();
    bad["npcs"]["retail_count"] = json!(60_000);
    bad["npcs"]["inst_count"] = json!(0);
    bad["npcs"]["hot_count"] = json!(0);
    let mut second_stock = bad["stocks"][0].clone();
    second_stock["code"] = json!("600102");
    bad["stocks"].as_array_mut().unwrap().push(second_stock);

    let (status, body) = new_session(app_router(), json!({ "setup": bad, "seed": "42" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "SETUP_RESOURCE_LIMIT");
}

#[tokio::test]
async fn new_session_rejects_implicit_exchange_or_category_with_400() {
    for required_field in ["exchange", "category"] {
        let mut bad = sample_setup_json();
        bad["stocks"][0]
            .as_object_mut()
            .unwrap()
            .remove(required_field);
        let (status, body) = new_session(app_router(), json!({ "setup": bad, "seed": "42" })).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "缺少 {required_field} 的新配置应 400: {body}"
        );
    }
}

#[tokio::test]
async fn new_session_rejects_malformed_json_with_400() {
    // 非 JSON body -> 反序列化失败 -> 400。
    let res = app_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/new")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(&b"{not json"[..]))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn new_session_requires_a_decimal_string_seed() {
    let (status, body) = new_session(
        app_router(),
        json!({ "setup": sample_setup_json(), "seed": 42 }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "JSON number seed must be rejected: {body}"
    );
}

// --- GET /api/snapshot ---

#[tokio::test]
async fn snapshot_unknown_session_returns_404() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .uri("/api/snapshot?session_id=does-not-exist")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "未知 session 应 404");
}

#[tokio::test]
async fn snapshot_returns_snapshot_json() {
    // 跨请求共享同一 manager：用 app_router_with_manager 而非 app_router（后者每次新 manager）。
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap().to_string();

    // GET /api/snapshot 对刚创建的 session → 200 + Snapshot JSON。
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/snapshot?session_id={id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "已知 session 取 snapshot 应 200"
    );
    let bytes = to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("读取 body 失败");
    let snap: Value = serde_json::from_slice(&bytes).expect("body 应为 Snapshot JSON");
    assert_eq!(
        snap["markets"].as_object().map(|m| m.len()),
        Some(1),
        "快照含 1 个 market"
    );
    assert_eq!(
        snap["accounts"].as_object().map(|m| m.len()),
        Some(1),
        "客户端快照只含玩家账户，不泄露 NPC 私有资产"
    );
    assert_eq!(
        snap["daily_candles"]["600101"].as_array().map(Vec::len),
        Some(360),
        "HTTP 连接快照应同步 Rust 生成的 360 日日 K"
    );
}

#[tokio::test]
async fn public_report_page_requires_the_owning_session_token() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token) = new_session_credentials(app.clone()).await;
    let uri = format!("/api/companies/C-600101/reports?session_id={session_id}");

    let (status, body) = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "UNAUTHORIZED");

    let (status, body) = response_json(
        app.oneshot(
            Request::builder()
                .uri(&uri)
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "published page should be readable: {body}"
    );
    assert!(body["reports"].is_array());
    for field in ["period", "approved_date", "published_date"] {
        let date = body["reports"][0][field]
            .as_str()
            .expect("public report date must be a string");
        assert_eq!(date.len(), 10, "{field} must be YYYY-MM-DD: {date}");
        assert_eq!(&date[4..5], "-", "{field} must be YYYY-MM-DD: {date}");
        assert_eq!(&date[7..8], "-", "{field} must be YYYY-MM-DD: {date}");
    }
    println!(
        "server public report dates: period={}, approved_date={}, published_date={}",
        body["reports"][0]["period"],
        body["reports"][0]["approved_date"],
        body["reports"][0]["published_date"]
    );
    assert!(body.get("books").is_none());
    assert!(body.get("journal").is_none());
    assert!(body.get("npc_information_state").is_none());
}

#[tokio::test]
#[cfg(not(feature = "simulation-diagnostics"))]
async fn npc_diagnostics_requires_auth_and_release_returns_no_records() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token) = new_session_credentials(app.clone()).await;
    let uri = format!("/api/diagnostics/npc/1?session_id={session_id}&generation=1");

    let (status, body) = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "UNAUTHORIZED");

    let (status, body) = response_json(
        app.oneshot(
            Request::builder()
                .uri(&uri)
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "generation": "1", "diagnostics": { "kind": "unsupported" } })
    );
}

#[tokio::test]
async fn npc_diagnostics_stale_generation_returns_unsupported_without_records() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token) = new_session_credentials(app.clone()).await;
    let uri = format!("/api/diagnostics/npc/1?session_id={session_id}&generation=0");
    let (status, body) = response_json(
        app.oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["diagnostics"], json!({ "kind": "unsupported" }));
    assert!(body["diagnostics"].get("records").is_none());
}

#[cfg(feature = "simulation-diagnostics")]
#[tokio::test]
async fn npc_diagnostics_feature_returns_supported_records_for_authenticated_current_session() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token) = new_session_credentials(app.clone()).await;
    let uri = format!("/api/diagnostics/npc/1?session_id={session_id}&generation=1");
    let (status, body) = response_json(
        app.oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["diagnostics"]["kind"], "supported");
    assert!(body["diagnostics"]["records"].is_array());
}

#[tokio::test]
async fn public_report_routes_reject_invalid_pagination_and_cross_session_credentials() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (first_id, first_token) = new_session_credentials(app.clone()).await;
    let (_, second_token) = new_session_credentials(app.clone()).await;
    let base = format!("/api/companies/C-600101/reports?session_id={first_id}");

    let (status, body) = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(&base)
                    .header("authorization", format!("Bearer {second_token}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "SESSION_FORBIDDEN");

    let (status, body) = response_json(
        app.oneshot(
            Request::builder()
                .uri(format!("{base}&limit=101"))
                .header("authorization", format!("Bearer {first_token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_REPORT_LIMIT");
}

#[tokio::test]
async fn public_report_by_id_rejects_unknown_and_company_mismatched_reports() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token) = new_session_credentials(app.clone()).await;
    let base = format!("?session_id={session_id}");

    let (status, body) = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/companies/C-600101/reports/999999{base}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "REPORT_NOT_VISIBLE");

    let first_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/companies/C-600101/reports{base}"))
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    let (_, page) = response_json(first_page).await;
    let report_id = page["reports"][0]["id"]
        .as_str()
        .expect("fixture must provide a public report");
    let (status, body) = response_json(
        app.oneshot(
            Request::builder()
                .uri(format!("/api/companies/C-002156/reports/{report_id}{base}"))
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("request must return a response"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "REPORT_NOT_VISIBLE");
}

#[tokio::test]
async fn load_rejects_corrupt_and_oversized_raw_bodies_before_actor_replacement() {
    let manager = server::SessionManager::default();
    let app = server::app_router_with_manager(manager.clone());
    let (session_id, _) = new_session_credentials(app.clone()).await;
    let handles = manager.lookup(&session_id).expect("session must exist");
    let before = serde_json::to_vec(&handles.save().await.expect("save must work")).unwrap();

    let corrupt = json!({ "session_id": session_id, "slot": { "bad": true } });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(corrupt.to_string()))
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    let (status, body) = response_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_SAVE");
    assert_eq!(
        serde_json::to_vec(&handles.save().await.unwrap()).unwrap(),
        before
    );

    let oversized = vec![b'x'; server::routes::MAX_LOAD_BODY_BYTES + 1];
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(oversized))
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        serde_json::to_vec(&handles.save().await.unwrap()).unwrap(),
        before
    );
}

#[tokio::test]
async fn load_rejects_an_oversized_nested_collection_before_actor_replacement() {
    // Given: a live actor and a syntactically valid restore envelope whose slot contains a
    // deliberately excessive nested collection.
    let manager = server::SessionManager::default();
    let app = server::app_router_with_manager(manager.clone());
    let (session_id, _) = new_session_credentials(app.clone()).await;
    let handles = manager.lookup(&session_id).expect("session must exist");
    let before = serde_json::to_vec(&handles.save().await.expect("save must work")).unwrap();
    let nested_items = "0,".repeat(100_001);
    let body = format!(
        r#"{{"session_id":"{session_id}","slot":{{"untrusted_nested":[{nested_items}0]}}}}"#
    );

    // When: the server receives the oversized nested collection.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    let (status, body) = response_json(response).await;

    // Then: it is rejected before domain restore and the actor state remains unchanged.
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "SAVE_RESOURCE_LIMIT");
    assert_eq!(
        serde_json::to_vec(&handles.save().await.expect("save must still work")).unwrap(),
        before
    );
}

#[tokio::test]
async fn load_rejects_excessive_nesting_before_actor_replacement() {
    // Given: a live actor and a restore envelope with depth beyond the parser gate.
    let manager = server::SessionManager::default();
    let app = server::app_router_with_manager(manager.clone());
    let (session_id, _) = new_session_credentials(app.clone()).await;
    let handles = manager.lookup(&session_id).expect("session must exist");
    let before = serde_json::to_vec(&handles.save().await.expect("save must work")).unwrap();
    let nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let body = format!(r#"{{"session_id":"{session_id}","slot":{nested}}}"#);

    // When: the server receives the deeply nested but syntactically valid input.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .expect("request must return a response");
    let (status, body) = response_json(response).await;

    // Then: resource rejection occurs without replacing the authoritative actor state.
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "SAVE_RESOURCE_LIMIT");
    assert_eq!(
        serde_json::to_vec(&handles.save().await.expect("save must still work")).unwrap(),
        before
    );
}

// --- POST /api/intent ---

fn player_buy_intent() -> Value {
    serde_json::to_value(&Intent::PlaceLimit {
        code: StockCode("600101".to_string()),
        side: Side::Buy,
        price: Money::from_cents(1000),
        qty: 100,
    })
    .expect("Intent 序列化")
}

#[tokio::test]
async fn intent_unknown_session_returns_404() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/intent")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": "nope", "intent": player_buy_intent() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "未知 session 下单应 404"
    );
}

#[tokio::test]
async fn intent_malformed_body_returns_400() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/intent")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(&b"not json"[..]))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn intent_known_session_returns_200() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/intent")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "intent": player_buy_intent() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::OK, "已知 session 下单应 200");
}

// --- POST /api/speed ---

#[tokio::test]
async fn speed_unknown_session_returns_404() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": "nope", "speed": 2.0 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn speed_metrics_unknown_session_returns_404() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .uri("/api/speed?session_id=does-not-exist")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn speed_known_session_returns_200() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "speed": 4.0 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::OK, "已知 session 改速应 200");
}

#[tokio::test]
async fn invalid_speed_is_rejected_instead_of_returning_false_success() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "speed": 0.0 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn excessive_numeric_speed_is_rejected() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "speed": 1e100 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn load_rejects_over_budget_setup_without_replacing_session() {
    use server::{app_router_with_manager, SessionManager};
    let manager = SessionManager::default();
    let setup: engine::SessionSetup = serde_json::from_value(sample_setup_json()).unwrap();
    let id = manager.new_session(setup.clone(), 42).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let before = handles.snapshot().await.unwrap();
    let mut slot = engine::GameSession::new(setup, 42)
        .unwrap()
        .save()
        .expect("healthy save");
    slot.setup.history_len = 10_001;
    let app = app_router_with_manager(manager);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&json!({ "session_id": id, "slot": slot })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(handles.snapshot().await.unwrap().tick, before.tick);
}

#[tokio::test]
async fn load_rejects_excessive_saved_pending_intents_without_replacing_session() {
    use server::{app_router_with_manager, SessionManager};
    let manager = SessionManager::default();
    let setup: engine::SessionSetup = serde_json::from_value(sample_setup_json()).unwrap();
    let id = manager.new_session(setup.clone(), 42).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let before = handles.snapshot().await.unwrap();
    let mut slot = engine::GameSession::new(setup, 42)
        .unwrap()
        .save()
        .expect("healthy save");
    slot.pending_player = (0..5_001)
        .map(|_| {
            (
                engine::AccountId(0),
                Intent::PlaceLimit {
                    code: StockCode("600101".to_string()),
                    side: Side::Buy,
                    price: Money::from_cents(1_000),
                    qty: 100,
                },
            )
        })
        .collect();
    let app = app_router_with_manager(manager);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&json!({ "session_id": id, "slot": slot })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(handles.snapshot().await.unwrap().tick, before.tick);
}

#[tokio::test]
async fn fastest_speed_mode_is_accepted() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "speed": "Fastest" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn speed_metrics_reports_requested_mode_and_actual_sampling_fields() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap();

    let set_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "speed": "Fastest" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/speed?session_id={id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["requested"], json!({ "mode": "fastest" }));
    assert_eq!(body["running"], false);
    assert_eq!(body["actual_multiplier"], 0.0, "暂停时改速仍应明确报告 0x");
    assert!(body["sample_duration_ms"].is_u64());
    assert!(body["sample_ticks"].is_u64());
}

#[tokio::test]
async fn delete_session_stops_and_removes_it() {
    use server::{app_router_with_manager, SessionManager};
    let manager = SessionManager::default();
    let app = app_router_with_manager(manager.clone());
    let (_, body) = new_session(
        app.clone(),
        json!({ "setup": sample_setup_json(), "seed": "42" }),
    )
    .await;
    let id = body["session_id"].as_str().unwrap();
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/session?session_id={id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(manager.lookup(id).is_none());
    assert_eq!(manager.active_session_count(), 0);

    let second = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/session?session_id={id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::NOT_FOUND);
}

// --- CORS（tower-http，允许前端跨域；ADR-0005 §6 联机前提） ---

/// CORS 预检 OPTIONS 应返回带 `Access-Control-Allow-Origin` 的响应。
///
/// 前端（不同 origin）发跨域请求前先发 OPTIONS 预检；若服务端无 CORS 头，
/// 浏览器会拦截实际请求。这里验证路由层挂了 `CorsLayer`。
#[tokio::test]
async fn cors_preflight_returns_allow_origin_header() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/new")
                .header("origin", "http://localhost:5173")
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    let allow_origin = res
        .headers()
        .get("access-control-allow-origin")
        .expect("OPTIONS 预检响应必须含 Access-Control-Allow-Origin（CORS 未挂载？）");
    assert!(
        !allow_origin.is_empty(),
        "Access-Control-Allow-Origin 不得为空（CORS 层未正确配置）"
    );
}

/// CORS 实际（非预检）请求也应带 allow-origin（前端跨域 fetch 落地）。
#[tokio::test]
async fn cors_actual_response_carries_allow_origin() {
    let res = app_router()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .header("origin", "http://localhost:5173")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get("access-control-allow-origin").is_some(),
        "带 origin 的实际请求响应也应带 Access-Control-Allow-Origin"
    );
}
