use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use engine::account::StockCode;
use engine::money::Money;
use engine::strategy::Intent;
use engine::Side;
use serde_json::{json, Value};
use tower::ServiceExt;

fn setup() -> Value {
    json!({
        "stocks": [{"code":"600101","exchange":"Shanghai","category":"MainBoard","initial_price":"1000","limit_pct":0.10,"tick":"1","total_shares":"10000000","float_shares":0}],
        "npcs": {"retail_count":0,"inst_count":0,"hot_count":0,"retail_cash_median":"10000000"},
        "config": engine::GameConfig::proposed_defaults(),
        "strategy_params": {"retail":{"arrival_rate":0.5,"order_size_mean":100,"chase_prob":0.2},"inst":{"margin":0.05,"order_size":200},"hot":{"lookback":3,"trend_threshold":0.02,"order_size":200}},
        "ticks_per_day":10,"auction_ticks":0,"closing_auction_ticks":0,"history_len":5,"t1_enabled":true,"float_allocation":{"between_kinds":"Random","within_kind":"Random"},"simulation_policy_id":engine::SIMULATION_POLICY_ID
    })
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let is_json = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok());
    let is_json = is_json.is_some_and(|value| value.starts_with("application/json"));
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let body = if bytes.is_empty() || !is_json {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

#[tokio::test]
async fn initial_allocation_route_requires_owner_and_current_generation() {
    let app = crate::app_router();
    let mut market = setup();
    market["stocks"][0]["float_shares"] = json!(7);
    let (status, created) = request(app.clone(), "POST", "/api/new", Some(json!({"setup":market,"seed":"17"})), None).await;
    assert_eq!(status, StatusCode::OK);
    let session_id = created["session_id"].as_str().unwrap();
    let token = created["session_token"].as_str().unwrap();
    let uri = format!("/api/initial-allocation?session_id={session_id}&generation=1");
    assert_eq!(request(app.clone(), "GET", &uri, None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(request(app.clone(), "GET", &uri, None, Some("other-player")).await.0, StatusCode::FORBIDDEN);
    let (status, response) = request(app.clone(), "GET", &uri, None, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["generation"], "1");
    assert_eq!(response["data"]["stocks"][0]["float_shares"], 7);
    assert_eq!(response["data"]["stocks"][0]["unallocated_shares"], 7);
    assert_eq!(response["data"]["stocks"][0]["categories"].as_array().unwrap().len(), 3);
    let stale = format!("/api/initial-allocation?session_id={session_id}&generation=2");
    assert_eq!(request(app.clone(), "GET", &stale, None, Some(token)).await.0, StatusCode::CONFLICT);
    assert_eq!(request(app.clone(), "GET", &format!("{uri}&account=0"), None, Some(token)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(request(app, "DELETE", &format!("/api/session?session_id={session_id}"), None, Some(token)).await.0, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn session_private_http_routes_require_the_matching_bearer_token() {
    let app = crate::app_router();
    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/new")
                .header("content-type", "application/json")
                .body(Body::from(json!({"setup":setup(),"seed":"17"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(created.into_body(), 1 << 20).await.unwrap()).unwrap();
    let session_id = created["session_id"].as_str().unwrap();
    let session_token = created["session_token"].as_str().unwrap();
    let intent = serde_json::to_value(Intent::PlaceLimit {
        code: StockCode("600101".to_owned()),
        side: Side::Buy,
        price: engine::LimitPrice::Fixed(Money::from_cents(1000)),
        qty: 100,
    })
    .unwrap();
    assert_eq!(
        request(app.clone(), "GET", "/healthz", None, None).await.0,
        StatusCode::OK
    );
    let cases = [
        (
            "POST",
            "/api/intent",
            json!({"session_id":session_id,"generation":"1","intent":intent}),
        ),
        (
            "GET",
            &format!("/api/snapshot?session_id={session_id}"),
            Value::Null,
        ),
        (
            "GET",
            &format!("/api/stock-history?session_id={session_id}&generation=0&code=600101"),
            Value::Null,
        ),
        (
            "POST",
            "/api/save",
            json!({"session_id":session_id,"generation":"0","candidate":null}),
        ),
        (
            "POST",
            "/api/load",
            json!({"session_id":session_id,"slot":{}}),
        ),
        (
            "POST",
            "/api/speed",
            json!({"session_id":session_id,"speed":1.0}),
        ),
        (
            "GET",
            &format!("/api/speed?session_id={session_id}"),
            Value::Null,
        ),
        (
            "POST",
            "/api/running",
            json!({"session_id":session_id,"running":false}),
        ),
        (
            "DELETE",
            &format!("/api/session?session_id={session_id}"),
            Value::Null,
        ),
    ];
    for (method, uri, body) in cases {
        assert_eq!(
            request(
                app.clone(),
                method,
                uri,
                (!body.is_null()).then_some(body.clone()),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} must require session token"
        );
        let (status, error) = request(
            app.clone(),
            method,
            uri,
            (!body.is_null()).then_some(body),
            Some("incorrect-token"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} must reject an incorrect session token"
        );
        assert_eq!(error["code"], "SESSION_FORBIDDEN", "{method} {uri}");
        assert_eq!(
            error["message"], "session token does not authorize this session",
            "{method} {uri}"
        );
    }
    let snapshot_uri = format!("/api/snapshot?session_id={session_id}");
    let (unknown_status, unknown_error) = request(
        app.clone(),
        "GET",
        "/api/snapshot?session_id=not-a-session",
        None,
        Some("incorrect-token"),
    )
    .await;
    let (wrong_status, wrong_error) = request(
        app.clone(),
        "GET",
        &snapshot_uri,
        None,
        Some("incorrect-token"),
    )
    .await;
    assert_eq!(unknown_status, StatusCode::FORBIDDEN);
    assert_eq!(wrong_status, StatusCode::FORBIDDEN);
    assert_eq!(unknown_error["code"], wrong_error["code"]);
    assert_eq!(unknown_error["message"], wrong_error["message"]);
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &snapshot_uri,
            None,
            Some("another-session-token")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(app.clone(), "GET", &snapshot_uri, None, Some(session_token))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("/api/stock-history?session_id={session_id}&generation=1&code=600101"),
            None,
            Some(session_token),
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("/api/stock-history?session_id={session_id}&generation=1&code=600101&account=1"),
            None,
            Some(session_token),
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/intent",
            Some(json!({"session_id":session_id,"generation":"1","intent":intent})),
            Some(session_token)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/running",
            Some(json!({"session_id":session_id,"running":true})),
            Some(session_token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("/api/speed?session_id={session_id}"),
            None,
            Some(session_token)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (save_status, save_error) = request(
        app.clone(),
        "POST",
        "/api/save",
        Some(json!({"session_id":session_id,"generation":"1","candidate":null})),
        Some(session_token),
    )
    .await;
    assert_eq!(save_status, StatusCode::BAD_REQUEST);
    assert_eq!(save_error["code"], "SAVE_REJECTED");
    let (load_status, load_error) = request(
        app.clone(),
        "POST",
        "/api/load",
        Some(json!({"session_id":session_id,"slot":{}})),
        Some(session_token),
    )
    .await;
    assert_eq!(load_status, StatusCode::BAD_REQUEST);
    assert_eq!(load_error["code"], "INVALID_SAVE");
    assert_eq!(
        request(
            app.clone(),
            "DELETE",
            &format!("/api/session?session_id={session_id}"),
            None,
            Some(session_token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}
