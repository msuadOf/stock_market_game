//! 真实成交广播 后端契约集成测试（与前端 RemoteHost 严格对齐）。
//!
//! 验证的宿主契约：
//! - POST /api/new     body {setup, seed}        -> 200 market context | 400
//! - POST /api/intent  body {session_id, generation, intent} -> Bearer 鉴权后 200 | 400
//! - GET  /api/snapshot?session_id=..           -> Bearer 鉴权后 200 Snapshot
//! - POST /api/speed   body {session_id, speed}  -> 200
//! - GET  /api/speed?session_id=..              -> Bearer 鉴权后 200 SpeedMetrics
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
use server::{app_router, app_router_with_manager, SessionManager};
use tower::ServiceExt;

/// 构造一个合法的最小 SessionSetup JSON（对齐 engine/tests/session.rs sample_setup）。
fn sample_setup_json() -> Value {
    json!({
        "stocks": [{
            "code": "600101",
            "exchange": "Shanghai",
            "category": "MainBoard",
            "initial_price": "1000",
            "limit_pct": 0.10,
            "tick": "1",
            "total_shares": "10000000",
            "float_shares": 0
        }],
        "npcs": {
            "retail_count": 2,
            "inst_count": 1,
            "hot_count": 1,
            "retail_cash_median": "10000000"
        },
        "config": engine::GameConfig::proposed_defaults(),
        "strategy_params": {
            "retail": { "arrival_rate": 0.5, "order_size_mean": 100, "chase_prob": 0.2 },
            "inst":   { "margin": 0.05, "order_size": 200 },
            "hot":    { "lookback": 3, "trend_threshold": 0.02, "order_size": 200 }
        },
        "ticks_per_day": 10,
        "auction_ticks": 0,
        "closing_auction_ticks": 0,
        "history_len": 5,
        "start_date": "2030-01-02",
        "report_frequency": "Quarterly",
        "company_operations": null,
        "groups": [],
        "t1_enabled": true,
        "float_allocation": { "between_kinds": "Random", "within_kind": "Random" },
        "simulation_policy_id": engine::SIMULATION_POLICY_ID
    })
}

// 私有路由：无凭据 401；未知 session 与错误 token 均返回 403，防止会话探测。
// --- POST /api/new ---

async fn new_session(app: axum::Router, body: Value) -> (StatusCode, Value) {
    let guest = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/guest")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .expect("guest request must return a response");
    assert_eq!(guest.status(), StatusCode::CREATED);
    let guest_bytes = to_bytes(guest.into_body(), 1 << 20).await.unwrap();
    let guest_body: Value = serde_json::from_slice(&guest_bytes).unwrap();
    let credential = guest_body["token"].as_str().unwrap().to_owned();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/new")
                .header("authorization", format!("Bearer {credential}"))
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
    let mut body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if body.is_object() {
        body["test_credential"] = json!(credential);
    }
    (status, body)
}

fn guest_app() -> (axum::Router, String) {
    let manager = SessionManager::default();
    let identity = server::identity::IdentityService::new(manager.database())
        .guest()
        .expect("guest identity should be created");
    (app_router_with_manager(manager), identity.token)
}

async fn new_session_credentials(app: axum::Router) -> (String, String, String) {
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
    assert!(body.get("session_token").is_none(), "/api/new must not return a session token");
    let session_token = body["test_credential"]
        .as_str()
        .expect("new session returns its session token")
        .to_owned();
    let generation = body["generation"]
        .as_str()
        .expect("new session returns its context generation")
        .to_owned();
    (session_id, session_token, generation)
}

fn closed_day_setup_json() -> Value {
    let mut setup = sample_setup_json();
    // Saturday settles into Sunday without market ticks or next-opening NPC
    // requests, providing a public day-end archive rather than an intraday checkpoint.
    setup["start_date"] = json!("2030-01-05");
    setup
}

fn closed_day_save(
    setup: engine::SessionSetup,
    seed: u64,
) -> Result<engine::SaveSlot, engine::SessionError> {
    let mut session = engine::session::protocol::ProtocolSession::new(setup, seed)?;
    session.end_civil_day_update()?;
    session.save()
}

#[tokio::test]
async fn save_generation_rejects_same_date_seq_edited_assets_after_restore() {
    let fixture = ApiTestSession::new_settled(closed_day_setup_json(), 42)
        .await
        .unwrap();
    let session_id = &fixture.session_id;
    let token = &fixture.session_token;
    let handles = &fixture.handles;
    let app = fixture.app.clone();
    let baseline = handles.public_baseline().await.unwrap();
    let mut slot = handles.save(2, None).await.unwrap();
    let candidate = json!({ "seq": slot.snapshot.seq, "settledDate": "2030-01-05" });
    let account = slot
        .snapshot
        .accounts
        .get_mut(&engine::AccountId(0))
        .unwrap();
    account.cash = account.cash.add(Money::from_cents(100)).unwrap();
    handles.restore(slot).await.unwrap();
    for key in [Some(candidate), None] {
        let mut body = json!({ "session_id": session_id, "generation": baseline.timeline_generation.to_string() });
        if let Some(candidate) = key {
            body["candidate"] = candidate;
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/save")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, error) = response_json(response).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "stale save must not expose edited assets"
        );
        assert_eq!(error["code"], "STALE_SESSION_GENERATION");
    }
    let saved = handles
        .save(
            baseline.timeline_generation + 1,
            Some(engine::session::protocol::SaveCandidateKey {
                seq: baseline.snapshot.seq,
                settled_date: engine::CivilDate::from_iso("2030-01-05").unwrap(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        saved.snapshot.accounts[&engine::AccountId(0)].cash,
        handles.snapshot().await.unwrap().accounts[&engine::AccountId(0)].cash
    );
    fixture.shutdown().await;
}

/// Router、SessionManager 与会话凭据来自同一次 fixture 初始化。
struct ApiTestSession {
    app: axum::Router,
    manager: server::SessionManager,
    session_id: String,
    session_token: String,
    generation: String,
    handles: std::sync::Arc<server::SessionHandles>,
}

impl ApiTestSession {
    async fn new_settled(setup_json: Value, seed: u64) -> Result<Self, String> {
        let setup: engine::SessionSetup = serde_json::from_value(setup_json.clone())
            .map_err(|error| format!("ApiTestSession setup 解码失败：{error}"))?;
        let manager = server::SessionManager::default();
        let app = server::app_router_with_manager(manager.clone());
        let (status, body) = new_session(
            app.clone(),
            json!({ "setup": setup_json, "seed": seed.to_string() }),
        )
        .await;
        if status != StatusCode::OK {
            return Err(format!("ApiTestSession 创建失败（{status}）：{body}"));
        }
        let session_id = body["session_id"]
            .as_str()
            .ok_or_else(|| format!("ApiTestSession 创建响应缺少 session_id：{body}"))?
            .to_owned();
        let handles = manager
            .lookup(&session_id)
            .ok_or_else(|| format!("ApiTestSession 注册表中找不到 session_id {session_id}"))?;
        let session_token = body["test_credential"]
            .as_str()
            .ok_or_else(|| format!("ApiTestSession test credential 缺失：{body}"))?
            .to_owned();
        let identity = server::identity::IdentityService::new(manager.database())
            .authenticate(&session_token)
            .map_err(|error| format!("ApiTestSession credential 无效：{error}"))?;
        let subject = engine::session::OpaqueSubjectId::new(identity.subject_id)
            .map_err(|error| format!("ApiTestSession subject 无效：{error}"))?;
        let context = handles
            .market_context_for(subject)
            .await
            .map_err(|error| format!("ApiTestSession context 查询失败：{error}"))?;
        let mut fixture = Self {
            app,
            manager,
            session_id,
            session_token,
            generation: context.generation,
            handles,
        };
        let slot = match closed_day_save(setup, seed) {
            Ok(slot) => slot,
            Err(error) => {
                let reason = format!("ApiTestSession 日终 fixture 构造失败：{error}");
                fixture.shutdown().await;
                return Err(reason);
            }
        };
        if let Err(error) = fixture.handles.restore(slot).await {
            let reason = format!("ApiTestSession 日终恢复失败：{error}");
            fixture.shutdown().await;
            return Err(reason);
        }
        let identity = server::identity::IdentityService::new(fixture.manager.database())
            .authenticate(&fixture.session_token)
            .map_err(|error| format!("ApiTestSession credential 无效：{error}"))?;
        let subject = engine::session::OpaqueSubjectId::new(identity.subject_id)
            .map_err(|error| format!("ApiTestSession subject 无效：{error}"))?;
        fixture.generation = fixture
            .handles
            .market_context_for(subject)
            .await
            .map_err(|error| format!("ApiTestSession context 查询失败：{error}"))?
            .generation;
        Ok(fixture)
    }

    async fn shutdown(self) {
        self.manager
            .remove(&self.session_id)
            .expect("fixture session 应仍在注册表中");
        self.handles
            .shutdown()
            .await
            .expect("fixture actor 应正常停止");
    }
}

#[tokio::test]
async fn settled_api_fixture_reports_creation_failure_with_context() {
    let mut invalid = closed_day_setup_json();
    invalid["stocks"] = json!([]);
    let error = match ApiTestSession::new_settled(invalid, 42).await {
        Ok(fixture) => {
            fixture.shutdown().await;
            panic!("非法 setup 不应创建 fixture")
        }
        Err(error) => error,
    };
    assert!(error.contains("ApiTestSession 创建失败"));
    assert!(error.contains("400"));
    assert!(error.contains("INVALID_SETUP"));
}

#[tokio::test]
async fn settled_api_fixture_reports_unsettled_day_with_context() {
    let mut setup = closed_day_setup_json();
    setup["start_date"] = json!("2030-01-02");
    let error = match ApiTestSession::new_settled(setup, 42).await {
        Ok(fixture) => {
            fixture.shutdown().await;
            panic!("日内 fixture 不应冒充日终档")
        }
        Err(error) => error,
    };
    assert!(error.contains("ApiTestSession 日终 fixture 构造失败"));
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn host_parity_civil_day_route_requires_bearer_authentication_and_uses_the_actor() {
    let app = app_router();
    let (session_id, session_token, generation) = new_session_credentials(app.clone()).await;
    let request_body = json!({ "session_id": session_id, "generation": generation });

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
    let (session_id, session_token, _generation) = new_session_credentials(app.clone()).await;
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
    let session_token = new_body["test_credential"].as_str().unwrap().to_owned();
    let generation = new_body["generation"].as_str().unwrap();
    let step_body = json!({ "session_id": session_id, "generation": generation });

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

async fn assert_successful_intent(response: axum::response::Response, message: &str) {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("intent response body should be readable");
    if status != StatusCode::OK {
        let error: Value = serde_json::from_slice(&body)
            .expect("non-200 intent response should contain a JSON error");
        panic!("{message}: {error}");
    }
    assert!(body.is_empty(), "successful intent acknowledgment should be empty");
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
async fn legal_history_window_can_be_created_saved_and_restored() {
    let mut setup = closed_day_setup_json();
    // This is a history window, not a market-size quota. One stock and four NPCs
    // exercise the real routes without constructing a large world.
    setup["history_len"] = json!(10_001);
    let fixture = ApiTestSession::new_settled(setup, 42).await.unwrap();
    let id = &fixture.session_id;
    let token = &fixture.session_token;
    let app = fixture.app.clone();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/save")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation": fixture.generation }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("save request must return");
    let (status, saved) = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "valid market must save: {saved}");
    assert_eq!(saved["setup"]["history_len"], 10_001);

    let response = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation": fixture.generation, "slot": saved }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("restore request must return");
    let (status, restored) = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "own save must restore: {restored}");
    let handles = &fixture.handles;
    let saved_again = serde_json::to_value(handles.save(3, None).await.expect("restored save"))
        .expect("save serializes");
    assert_eq!(saved_again, saved, "restore must preserve the whole market");
    fixture.shutdown().await;
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
async fn snapshot_unknown_session_with_token_is_forbidden() {
    let (app, token) = guest_app();
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .uri("/api/snapshot?session_id=does-not-exist")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "未知 session 不得泄露存在性"
    );
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
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");

    // GET /api/snapshot 对刚创建的 session → 200 + Snapshot JSON。
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
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
    let (session_id, token, initial_generation) = new_session_credentials(app.clone()).await;
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
#[cfg(not(all(feature = "simulation-diagnostics", debug_assertions)))]
async fn disabled_npc_diagnostics_returns_no_records_regardless_of_credentials() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token, generation) = new_session_credentials(app.clone()).await;
    let uri = format!("/api/diagnostics/npc/1?session_id={session_id}&generation={generation}");
    for credential in [None, Some(token.as_str()), Some("invalid-session-token")] {
        let mut request = Request::builder().uri(&uri);
        if let Some(credential) = credential {
            request = request.header("authorization", format!("Bearer {credential}"));
        }
        let (status, body) = response_json(
            app.clone()
                .oneshot(request.body(axum::body::Body::empty()).unwrap())
                .await
                .expect("request must return a response"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], "DIAGNOSTICS_DISABLED");
        assert!(body.get("diagnostics").is_none());
    }
}

#[tokio::test]
async fn npc_diagnostics_stale_generation_never_returns_records() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token, _generation) = new_session_credentials(app.clone()).await;
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
    if cfg!(all(feature = "simulation-diagnostics", debug_assertions)) {
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "STALE_SESSION_GENERATION");
    } else {
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], "DIAGNOSTICS_DISABLED");
    }
    assert!(body.get("diagnostics").is_none());
}

#[cfg(all(feature = "simulation-diagnostics", debug_assertions))]
#[tokio::test]
async fn npc_diagnostics_feature_returns_supported_records_for_authenticated_current_session() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token, generation) = new_session_credentials(app.clone()).await;
    let uri = format!("/api/diagnostics/npc/1?session_id={session_id}&generation={generation}");
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
async fn public_report_routes_reject_invalid_pagination_and_credentials() {
    let app = server::app_router_with_manager(server::SessionManager::default());
    let (session_id, token, _generation) = new_session_credentials(app.clone()).await;
    let base = format!("/api/companies/C-600101/reports?session_id={session_id}");

    let (status, body) = response_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(&base)
                    .header("authorization", "Bearer invalid-session-token")
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
                .uri(format!("{base}&limit=101"))
                .header("authorization", format!("Bearer {token}"))
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
    let (session_id, token, _generation) = new_session_credentials(app.clone()).await;
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
async fn load_rejects_corrupt_body_before_actor_replacement() {
    let fixture = ApiTestSession::new_settled(closed_day_setup_json(), 42)
        .await
        .unwrap();
    let session_id = &fixture.session_id;
    let token = &fixture.session_token;
    let handles = &fixture.handles;
    let app = fixture.app.clone();
    let before = serde_json::to_vec(&handles.save(2, None).await.expect("save must work")).unwrap();
    let baseline_before = serde_json::to_value(handles.public_baseline().await.unwrap()).unwrap();

    let corrupt = json!({ "session_id": session_id, "generation": fixture.generation, "slot": { "bad": true } });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
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
        serde_json::to_vec(&handles.save(2, None).await.unwrap()).unwrap(),
        before
    );

    const { assert!(server::routes::MAX_LOAD_BODY_BYTES > engine::MAX_SAVE_DECODE_BYTES) };
    assert_eq!(
        serde_json::to_value(handles.public_baseline().await.unwrap()).unwrap(),
        baseline_before,
        "rejected load must preserve the live actor state and timeline, not only its archive"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn load_rejects_excessive_nesting_before_actor_replacement() {
    // Given: a live actor and a restore envelope with depth beyond the parser gate.
    let fixture = ApiTestSession::new_settled(closed_day_setup_json(), 42)
        .await
        .unwrap();
    let session_id = &fixture.session_id;
    let token = &fixture.session_token;
    let handles = &fixture.handles;
    let app = fixture.app.clone();
    let before = serde_json::to_vec(&handles.save(2, None).await.expect("save must work")).unwrap();
    let baseline_before = serde_json::to_value(handles.public_baseline().await.unwrap()).unwrap();
    let nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let body = format!(r#"{{"session_id":"{session_id}","generation":"{}","slot":{nested}}}"#, fixture.generation);

    // When: the server receives the deeply nested but syntactically valid input.
    let response = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
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
        serde_json::to_vec(&handles.save(2, None).await.expect("save must still work")).unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(handles.public_baseline().await.unwrap()).unwrap(),
        baseline_before,
        "rejected load must preserve the live actor state and timeline, not only its archive"
    );
    fixture.shutdown().await;
}

// --- POST /api/intent ---

fn player_buy_intent() -> Value {
    serde_json::to_value(&Intent::PlaceLimit {
        code: StockCode("600101".to_string()),
        side: Side::Buy,
        price: engine::LimitPrice::Fixed(Money::from_cents(1000)),
        qty: 100,
    })
    .expect("Intent 序列化")
}

#[tokio::test]
async fn intent_requires_current_canonical_generation() {
    let manager = server::SessionManager::default();
    let app = server::app_router_with_manager(manager.clone());
    let (session_id, token, initial_generation) = new_session_credentials(app.clone()).await;
    let mut setup: engine::SessionSetup = serde_json::from_value(sample_setup_json()).unwrap();
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    let expected_admission_cash = serde_json::to_value(setup.config.starting_cash).unwrap();
    let mut completed = engine::session::protocol::ProtocolSession::new(setup, 42).unwrap();
    completed.end_civil_day_update().unwrap();
    completed.end_civil_day_update().unwrap();
    assert_eq!(completed.civil_date().to_iso(), "2030-01-07");
    manager
        .lookup(&session_id)
        .unwrap()
        .restore(completed.save().unwrap())
        .await
        .unwrap();
    let identity = server::identity::IdentityService::new(manager.database())
        .authenticate(&token)
        .unwrap();
    let subject = engine::session::OpaqueSubjectId::new(identity.subject_id).unwrap();
    let current_generation = manager
        .lookup(&session_id)
        .unwrap()
        .market_context_for(subject.clone())
        .await
        .unwrap()
        .generation;
    for generation in [
        Some(json!(initial_generation)),
        None,
        Some(json!("01")),
        Some(json!(1)),
        Some(json!("18446744073709551616")),
    ] {
        let mut body = json!({ "session_id": session_id, "intent": player_buy_intent() });
        if let Some(generation) = generation {
            body["generation"] = generation;
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/intent")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "旧 generation、缺失或非法 generation 必须在 intake 前拒绝"
        );
    }
    let context = manager
        .lookup(&session_id)
        .unwrap()
        .market_context_for(subject.clone())
        .await
        .unwrap();
    assert!(context.member.is_none(), "restore fixture must begin without membership");
    let response = app
        .clone()
        .oneshot(Request::builder().method("POST").uri("/api/intent").header("authorization", format!("Bearer {token}")).header("content-type", "application/json").body(axum::body::Body::from(json!({ "session_id": session_id, "generation": current_generation, "intent": player_buy_intent() }).to_string())).unwrap())
        .await
        .unwrap();
    let (status, error) = response_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "INTENT_REJECTED");

    let join = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/markets/join")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(json!({
                    "session_id": session_id,
                    "generation": current_generation,
                    "confirmed_rejoin": true
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (join_status, joined) = response_json(join).await;
    assert_eq!(join_status, StatusCode::OK, "真实 join route 应完成重新加入：{joined}");
    assert!(joined["member"].is_object(), "join route 应返回本人 membership");
    let joined_account_id = joined["member"]["account_id"].as_str().unwrap();
    assert_eq!(
        joined["member"]["admission_funding"]["external_cash"],
        expected_admission_cash,
        "真实 join route 应按市场当前设置记录 AdmissionFunding"
    );
    let join_generation = joined["generation"].as_str().unwrap();

    let repeated_join = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/markets/join")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(json!({
                    "session_id": session_id,
                    "generation": join_generation,
                    "confirmed_rejoin": true
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (repeat_status, repeated) = response_json(repeated_join).await;
    assert_eq!(repeat_status, StatusCode::OK, "重复 join 应保持当前成员关系：{repeated}");
    assert_eq!(repeated["member"]["account_id"], joined_account_id);
    assert_eq!(
        repeated["member"]["admission_funding"]["external_cash"],
        expected_admission_cash,
        "重复 join 不得再次改变 AdmissionFunding"
    );
    let current_context = manager
        .lookup(&session_id)
        .unwrap()
        .market_context_for(subject)
        .await
        .unwrap();
    assert_eq!(
        current_context.member.as_ref().unwrap().account_id.0.to_string(),
        joined_account_id,
        "current context must retain the actual account allocated by join"
    );
    let current_generation = current_context.generation.to_string();
    let response = app.oneshot(Request::builder().method("POST").uri("/api/intent").header("authorization", format!("Bearer {token}")).header("content-type", "application/json").body(axum::body::Body::from(json!({ "session_id": session_id, "generation": current_generation, "intent": player_buy_intent() }).to_string())).unwrap()).await.unwrap();
    assert_successful_intent(response, "valid current-generation intent rejected").await;
    manager
        .remove(&session_id)
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn intent_unknown_session_with_token_is_forbidden() {
    let (app, token) = guest_app();
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/intent")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": "nope", "generation": "1", "intent": player_buy_intent() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "未知 session 下单不得泄露存在性"
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
    let generation = body["generation"].as_str().unwrap();
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");

    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/intent")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation": generation, "intent": player_buy_intent() })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_successful_intent(res, "已知 session 下单应 200").await;
}

// --- POST /api/speed ---

#[tokio::test]
async fn speed_unknown_session_with_token_is_forbidden() {
    let (app, token) = guest_app();
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": "nope", "generation":"1", "speed": 2.0 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn speed_metrics_unknown_session_with_token_is_forbidden() {
    let (app, token) = guest_app();
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .uri("/api/speed?session_id=does-not-exist")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
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
    let generation = body["generation"].as_str().unwrap();
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");

    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation":generation, "speed": 4.0 }).to_string(),
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
    let generation = body["generation"].as_str().unwrap();
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation":generation, "speed": 0.0 }).to_string(),
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
    let generation = body["generation"].as_str().unwrap();
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation":generation, "speed": 1e100 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn large_pending_player_queue_survives_engine_save_restore_exactly() {
    let setup: engine::SessionSetup = serde_json::from_value(closed_day_setup_json()).unwrap();
    let mut slot = closed_day_save(setup, 42).expect("日终 fixture 应合法");
    const REQUEST_COUNT: usize = 5_001;
    let mut producer = engine::GameSession::restore(&slot).unwrap();
    for _ in 0..REQUEST_COUNT {
        producer
            .enqueue_player_intent(
                engine::AccountId(0),
                Intent::PlaceLimit {
                    code: StockCode("600101".to_string()),
                    side: Side::Buy,
                    price: engine::LimitPrice::Fixed(Money::from_cents(1_000)),
                    qty: 100,
                },
            )
            .unwrap();
    }
    slot = producer.save().unwrap();
    assert_eq!(slot.pending_player.len(), REQUEST_COUNT);

    // Low-level engine checkpoints retain pending input and its order. Public
    // ProtocolSession restore has a stricter day-level save contract (covered
    // separately below).
    let restored = engine::GameSession::restore(&slot).expect("engine checkpoint restores");
    let roundtrip = restored.save().expect("engine checkpoint saves");
    assert_eq!(roundtrip.pending_player.len(), REQUEST_COUNT);
    assert_eq!(
        serde_json::to_value(&roundtrip.pending_player).unwrap(),
        serde_json::to_value(&slot.pending_player).unwrap(),
        "all 5,001 pending requests must retain their exact order and payloads"
    );
    assert_eq!(
        serde_json::to_value(roundtrip).unwrap(),
        serde_json::to_value(slot).unwrap(),
        "engine save/restore must preserve the complete checkpoint"
    );
}

#[tokio::test]
async fn public_load_rejects_pending_player_queue_without_replacing_actor() {
    let fixture = ApiTestSession::new_settled(closed_day_setup_json(), 42)
        .await
        .unwrap();
    let id = &fixture.session_id;
    let handles = &fixture.handles;
    let app = fixture.app.clone();
    let before = serde_json::to_value(handles.save(2, None).await.unwrap()).unwrap();
    let baseline_before = serde_json::to_value(handles.public_baseline().await.unwrap()).unwrap();

    let mut candidate = serde_json::from_value::<engine::SaveSlot>(before.clone()).unwrap();
    const REQUEST_COUNT: usize = 5_001;
    let mut producer = engine::GameSession::restore(&candidate).unwrap();
    for _ in 0..REQUEST_COUNT {
        producer
            .enqueue_player_intent(
                engine::AccountId(0),
                Intent::PlaceLimit {
                    code: StockCode("600101".to_string()),
                    side: Side::Buy,
                    price: engine::LimitPrice::Fixed(Money::from_cents(1_000)),
                    qty: 100,
                },
            )
            .unwrap();
    }
    candidate = producer.save().unwrap();
    assert_eq!(candidate.pending_player.len(), REQUEST_COUNT);

    let response = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {}", fixture.session_token))
                .method("POST")
                .uri("/api/load")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_string(&json!({ "session_id": id, "generation": fixture.generation, "slot": candidate })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .expect("load request must return a response");
    let (status, error) = response_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "INVALID_SAVE");
    assert!(error["message"]
        .as_str()
        .is_some_and(|message| message.contains("待处理输入")));
    assert_eq!(
        serde_json::to_value(handles.save(2, None).await.unwrap()).unwrap(),
        before,
        "rejected public load must leave the actor's settled save unchanged"
    );
    assert_eq!(
        serde_json::to_value(handles.public_baseline().await.unwrap()).unwrap(),
        baseline_before,
        "rejected load must preserve the live actor state and timeline, not only its archive"
    );
    fixture.shutdown().await;
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
    let generation = body["generation"].as_str().unwrap();
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");
    let res = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation":generation, "speed": "Fastest" }).to_string(),
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
    let generation = body["generation"].as_str().unwrap();
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");

    let set_response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("POST")
                .uri("/api/speed")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "session_id": id, "generation":generation, "speed": "Fastest" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
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
    let token = body["test_credential"]
        .as_str()
        .expect("session token required");
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .header("authorization", format!("Bearer {token}"))
                .method("DELETE")
                .uri(format!("/api/session?session_id={id}&generation={}", body["generation"].as_str().unwrap()))
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
                .header("authorization", format!("Bearer {token}"))
                .method("DELETE")
                .uri(format!("/api/session?session_id={id}&generation={}", body["generation"].as_str().unwrap()))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::FORBIDDEN);
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
