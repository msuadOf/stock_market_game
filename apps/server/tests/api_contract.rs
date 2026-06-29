//! WS-5 后端契约集成测试（与前端 RemoteHost 严格对齐）。
//!
//! 契约（见任务详情）：
//! - POST /api/new     body {setup, seed}        -> 200 {session_id} | 400
//! - POST /api/intent  body {session_id, intent} -> 200 | 404(未知session) | 400
//! - GET  /api/snapshot?session_id=..           -> 200 Snapshot | 404
//! - POST /api/speed   body {session_id, speed}  -> 200
//! - WS   /ws?session_id=..&token=..            -> 先发完整 Snapshot，再持续推 Event[]（各带 seq）
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
            "initial_price": 1000,
            "limit_pct": 0.10,
            "v_initial": 1000,
            "tick": 1,
            "float_shares": 0
        }],
        "npcs": {
            "retail_count": 2,
            "inst_count": 1,
            "hot_count": 1,
            "cash_per_npc": 10_000_000
        },
        "config": engine::GameConfig::proposed_defaults(),
        "v_params": {
            "long_run_mean": 1000,
            "mean_reversion": 0.5,
            "volatility": 0.0
        },
        "strategy_params": {
            "retail": { "arrival_rate": 0.5, "order_size_mean": 100, "chase_prob": 0.2, "tick_cents": 1 },
            "inst":   { "margin": 0.05, "order_size": 200 },
            "hot":    { "lookback": 3, "trend_threshold": 0.02, "order_size": 150 }
        },
        "player_cash": 10_000_000,
        "ticks_per_day": 10,
        "history_len": 5,
        "t1_enabled": false,
        "float_allocation": "Random"
    })
}

// 验证 engine serde 表示与 JSON 形态一致（早失败：若 engine 改了 serde 表示，这里先红）。
#[test]
fn engine_setup_roundtrips_json() {
    let v = sample_setup_json();
    let setup: engine::SessionSetup = serde_json::from_value(v).expect("JSON 应可反序列化为 SessionSetup");
    let s = engine::GameSession::new(setup, 42).expect("应可构造 GameSession");
    assert_eq!(s.market_count(), 1);
    assert_eq!(s.account_count(), 5);
}

#[tokio::test]
async fn healthz_still_ok() {
    // 回归：/healthz 不被新路由破坏。
    let res = app_router()
        .oneshot(Request::builder().uri("/healthz").body(axum::body::Body::empty()).unwrap())
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
    let bytes = to_bytes(res.into_body(), 1 << 20).await.expect("读取 body 失败");
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn new_session_returns_200_with_id() {
    let (status, body) = new_session(app_router(), json!({ "setup": sample_setup_json(), "seed": 42 })).await;
    assert_eq!(status, StatusCode::OK, "/api/new 合法 body 应 200: {body}");
    let id = body.get("session_id").and_then(|v| v.as_str()).expect("应返回 session_id 字符串");
    assert!(!id.is_empty(), "session_id 非空");
}

#[tokio::test]
async fn new_session_rejects_invalid_setup_with_400() {
    // 空 stocks -> engine::GameSession::new 返回 InvalidSetup -> 400（铁律二：不静默）。
    let mut bad = sample_setup_json();
    bad["stocks"] = json!([]);
    let (status, body) = new_session(app_router(), json!({ "setup": bad, "seed": 42 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "非法 setup 应 400: {body}");
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

// --- GET /api/snapshot ---

#[tokio::test]
async fn snapshot_unknown_session_returns_404() {
    let res = app_router()
        .oneshot(Request::builder().uri("/api/snapshot?session_id=does-not-exist").body(axum::body::Body::empty()).unwrap())
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "未知 session 应 404");
}

#[tokio::test]
async fn snapshot_returns_snapshot_json() {
    // 跨请求共享同一 manager：用 app_router_with_manager 而非 app_router（后者每次新 manager）。
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(app.clone(), json!({ "setup": sample_setup_json(), "seed": 42 })).await;
    let id = body["session_id"].as_str().unwrap().to_string();

    // GET /api/snapshot 对刚创建的 session → 200 + Snapshot JSON。
    let res = app
        .oneshot(Request::builder().uri(format!("/api/snapshot?session_id={id}")).body(axum::body::Body::empty()).unwrap())
        .await
        .expect("请求未返回响应");
    assert_eq!(res.status(), StatusCode::OK, "已知 session 取 snapshot 应 200");
    let bytes = to_bytes(res.into_body(), 1 << 20).await.expect("读取 body 失败");
    let snap: Value = serde_json::from_slice(&bytes).expect("body 应为 Snapshot JSON");
    assert_eq!(snap["markets"].as_object().map(|m| m.len()), Some(1), "快照含 1 个 market");
    assert_eq!(snap["accounts"].as_object().map(|m| m.len()), Some(5), "快照含 5 个账户");
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
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "未知 session 下单应 404");
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
    let (_, body) = new_session(app.clone(), json!({ "setup": sample_setup_json(), "seed": 42 })).await;
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
async fn speed_known_session_returns_200() {
    use server::{app_router_with_manager, SessionManager};
    let app = app_router_with_manager(SessionManager::default());
    let (_, body) = new_session(app.clone(), json!({ "setup": sample_setup_json(), "seed": 42 })).await;
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
