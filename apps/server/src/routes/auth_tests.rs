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
        "ticks_per_day":10,"auction_ticks":0,"closing_auction_ticks":0,"history_len":5,"report_frequency":"Quarterly","company_operations":null,"groups":[],"t1_enabled":true,"float_allocation":{"between_kinds":"Random","within_kind":"Random"},"simulation_policy_id":engine::SIMULATION_POLICY_ID
    })
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let generated_credential = if method == "POST" && uri == "/api/new" && token.is_none() {
        let guest = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/guest")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(
            &to_bytes(guest.into_body(), 1 << 20).await.unwrap(),
        )
        .unwrap();
        value["token"].as_str().unwrap().to_owned()
    } else {
        String::new()
    };
    let effective_token = token.or_else(|| {
        (!generated_credential.is_empty()).then_some(generated_credential.as_str())
    });
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    if let Some(token) = effective_token {
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
    let mut body = if bytes.is_empty() || !is_json {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    if !generated_credential.is_empty() && body.is_object() {
        body["test_credential"] = json!(generated_credential);
    }
    (status, body)
}

async fn guest_token(app: axum::Router) -> String {
    let (status, body) = request(app, "POST", "/api/auth/guest", Some(json!({})), None).await;
    assert_eq!(status, StatusCode::CREATED);
    body["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn initial_allocation_route_requires_authentication_and_current_generation() {
    let app = crate::app_router();
    let mut market = setup();
    market["stocks"][0]["float_shares"] = json!(7);
    let (status, created) = request(
        app.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":market,"seed":"17"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = created["session_id"].as_str().unwrap();
    let generation = created["generation"].as_str().unwrap();
    let token = created["test_credential"].as_str().unwrap();
    let uri = format!("/api/initial-allocation?session_id={session_id}&generation={generation}");
    assert_eq!(
        request(app.clone(), "GET", &uri, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let unjoined_token = guest_token(app.clone()).await;
    let (unjoined_status, unjoined_response) =
        request(app.clone(), "GET", &uri, None, Some(&unjoined_token)).await;
    assert_eq!(unjoined_status, StatusCode::OK);
    assert_eq!(unjoined_response["generation"], generation);
    let (status, response) = request(app.clone(), "GET", &uri, None, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["generation"], generation);
    assert_eq!(response["data"]["stocks"][0]["float_shares"], 7);
    assert_eq!(response["data"]["stocks"][0]["unallocated_shares"], 7);
    assert_eq!(
        response["data"]["stocks"][0]["categories"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let stale = format!("/api/initial-allocation?session_id={session_id}&generation=2");
    assert_eq!(
        request(app.clone(), "GET", &stale, None, Some(token))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("{uri}&account=0"),
            None,
            Some(token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            app,
            "DELETE",
            &format!("/api/session?session_id={session_id}&generation={generation}"),
            None,
            Some(token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn current_minute_history_is_authenticated_public_and_generation_scoped() {
    let app = crate::app_router();
    let mut current_setup = setup();
    current_setup["start_date"] = json!("2030-01-02");
    let (status, created) = request(app.clone(), "POST", "/api/new", Some(json!({"setup":current_setup,"seed":"23"})), None).await;
    assert_eq!(status, StatusCode::OK);
    let body = json!({"session_id":created["session_id"],"generation":created["generation"],"query":{"code":"600101"}});
    let token = created["test_credential"].as_str().unwrap();
    assert_eq!(request(app.clone(), "POST", "/api/current-minute-history", Some(body.clone()), None).await.0, StatusCode::UNAUTHORIZED);
    let (status, response) = request(app.clone(), "POST", "/api/current-minute-history", Some(body.clone()), Some(token)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["response"]["live"], true);
    assert_eq!(response["response"]["date"], "2030-01-02");
    assert_eq!(response["response"]["bars"], json!([]));
    let (_, guest) = request(app.clone(), "POST", "/api/auth/guest", Some(json!({})), None).await;
    let (status, public) = request(app.clone(), "POST", "/api/current-minute-history", Some(body.clone()), Some(guest["token"].as_str().unwrap())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public, response);
    let mut stale = body.clone(); stale["generation"] = json!("0");
    assert_eq!(request(app.clone(), "POST", "/api/current-minute-history", Some(stale), Some(token)).await.0, StatusCode::CONFLICT);
    let mut injected = body; injected["query"]["account"] = json!("0");
    assert_eq!(request(app, "POST", "/api/current-minute-history", Some(injected), Some(token)).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn retained_market_history_auth_generation_and_public_nonmember_query() {
    let app = crate::app_router();
    let mut current_setup = setup();
    current_setup["start_date"] = json!("2030-01-02");
    let (status, created) = request(app.clone(), "POST", "/api/new", Some(json!({"setup":current_setup,"seed":"23"})), None).await;
    assert_eq!(status, StatusCode::OK);
    let body = json!({"session_id":created["session_id"],"generation":created["generation"],"query":{"code":"600101","date_from":"2030-01-02","date_to":"2030-01-03","after":null,"page_size":2}});
    let token = created["test_credential"].as_str().unwrap();
    assert_eq!(request(app.clone(), "POST", "/api/market-history", Some(body.clone()), None).await.0, StatusCode::UNAUTHORIZED);
    let (status, response) = request(app.clone(), "POST", "/api/market-history", Some(body.clone()), Some(token)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["page"]["entries"].as_array().unwrap().len(), 2);
    assert_eq!(response["page"]["entries"][0]["availability"], "NotEnded");
    let (_, guest) = request(app.clone(), "POST", "/api/auth/guest", Some(json!({})), None).await;
    let (status, public) = request(app.clone(), "POST", "/api/market-history", Some(body.clone()), Some(guest["token"].as_str().unwrap())).await;
    assert_eq!(status, StatusCode::OK, "{public}");
    assert_eq!(public, response, "未入场身份可以读取公开历史，不冒充account0经历");
    let mut stale = body.clone();
    stale["generation"] = json!("0");
    assert_eq!(request(app.clone(), "POST", "/api/market-history", Some(stale), Some(token)).await.0, StatusCode::CONFLICT);
    let mut injected = body.clone();
    injected["query"]["account"] = json!("0");
    assert_eq!(request(app.clone(), "POST", "/api/market-history", Some(injected), Some(token)).await.0, StatusCode::BAD_REQUEST);
    let mut incomplete = body;
    incomplete["query"].as_object_mut().unwrap().remove("after");
    assert_eq!(request(app, "POST", "/api/market-history", Some(incomplete), Some(token)).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn intraday_average_and_personal_trade_history_are_authenticated_current_generation_queries()
{
    let app = crate::app_router();
    let mut current_setup = setup();
    current_setup["report_frequency"] = json!("Quarterly");
    current_setup["start_date"] = json!("2030-01-02");
    let (status, created) = request(
        app.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":current_setup,"seed":"23"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = created["session_id"].as_str().unwrap();
    let generation = created["generation"].as_str().unwrap();
    let token = created["test_credential"].as_str().unwrap();
    let history_uri =
        format!("/api/personal-trade-confirmations?session_id={session_id}&generation={generation}");
    assert_eq!(
        request(app.clone(), "GET", &history_uri, None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, history) = request(app.clone(), "GET", &history_uri, None, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["generation"], generation);
    assert_eq!(history["confirmations"], json!([]));
    let date_query = json!({ "date_from": "2030-01-02", "date_to": "2030-01-04", "code": null, "side": null, "before_receipt": null, "as_of_receipt": null, "page_size": 100 });
    let body = json!({ "session_id": session_id, "generation": generation, "query": date_query });
    assert_eq!(request(app.clone(), "POST", "/api/personal-trade-history", Some(body.clone()), None).await.0, StatusCode::UNAUTHORIZED);
    let (status, page) = request(app.clone(), "POST", "/api/personal-trade-history", Some(body.clone()), Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["page"]["request"], date_query);
    assert_eq!(page["page"]["confirmations"], json!([]));
    let mut untrusted = body.clone();
    untrusted["query"]["account"] = json!(1);
    assert_eq!(request(app.clone(), "POST", "/api/personal-trade-history", Some(untrusted), Some(token)).await.0, StatusCode::BAD_REQUEST);
    let mut incomplete = body;
    incomplete["query"].as_object_mut().unwrap().remove("side");
    assert_eq!(request(app.clone(), "POST", "/api/personal-trade-history", Some(incomplete), Some(token)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("{history_uri}&account=1"),
            None,
            Some(token),
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "账户由宿主授权主体绑定，不接受客户端选择他人账户"
    );
    let (status, average) = request(
        app.clone(),
        "POST",
        "/api/intraday-average",
        Some(json!({
            "session_id":session_id,
            "generation":generation,
            "turnover_cents":"260000",
            "trade_count":2,
            "volume_shares":300
        })),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(average["turnover_cents"], "260000");
    assert_eq!(average["volume_shares"], 300);
    let curve_body = json!({ "session_id": session_id, "generation": generation, "series_key": "600000:1:0", "samples": [
        { "turnover_cents": "100000", "trade_count": 1, "volume_shares": 100 },
        { "turnover_cents": "260000", "trade_count": 2, "volume_shares": 300 }
    ] });
    let (curve_status, curve) = request(
        app.clone(),
        "POST",
        "/api/intraday-average-curve",
        Some(curve_body.clone()),
        Some(token),
    )
    .await;
    assert_eq!(curve_status, StatusCode::OK);
    assert_eq!(curve["series_key"], "600000:1:0");
    assert_eq!(curve["generation"], generation);
    assert_eq!(curve["results"][1]["turnover_cents"], "260000");
    let mut numeric_generation = curve_body;
    numeric_generation["generation"] = json!(1);
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/intraday-average-curve",
            Some(numeric_generation),
            Some(token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for cursor in ["00", "18446744073709551616", "-1"] {
        assert_eq!(
            request(
                app.clone(),
                "GET",
                &format!("{history_uri}&before_receipt={cursor}"),
                None,
                Some(token)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/intraday-average",
            Some(json!({
                "session_id":session_id,
                "generation":"2",
                "turnover_cents":"260000",
                "trade_count":2,
                "volume_shares":300
            })),
            Some(token),
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn session_private_http_routes_require_the_matching_bearer_token() {
    let app = crate::app_router();
    let mut current_setup = setup();
    current_setup["start_date"] = json!("2030-01-02");
    let (created_status, created) = request(
        app.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":current_setup,"seed":"17"})),
        None,
    )
    .await;
    assert_eq!(created_status, StatusCode::OK);
    let session_id = created["session_id"].as_str().unwrap();
    assert!(created.get("session_token").is_none());
    let generation = created["generation"].as_str().unwrap();
    let credential = created["test_credential"].as_str().unwrap();
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
            json!({"session_id":session_id,"generation":generation,"intent":intent}),
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
            json!({"session_id":session_id,"generation":generation,"slot":{}}),
        ),
        (
            "POST",
            "/api/speed",
            json!({"session_id":session_id,"generation":generation,"speed":1.0}),
        ),
        (
            "GET",
            &format!("/api/speed?session_id={session_id}"),
            Value::Null,
        ),
        (
            "POST",
            "/api/running",
            json!({"session_id":session_id,"generation":generation,"running":false}),
        ),
        (
            "DELETE",
            &format!("/api/session?session_id={session_id}&generation={generation}"),
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
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(error["code"], "IDENTITY_AUTHENTICATION_FAILED", "{method} {uri}");
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
    assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
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
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(app.clone(), "GET", &snapshot_uri, None, Some(credential))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("/api/stock-history?session_id={session_id}&generation={generation}&code=600101"),
            None,
            Some(credential),
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!(
                "/api/stock-history?session_id={session_id}&generation={generation}&code=600101&account=1"
            ),
            None,
            Some(credential),
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
            Some(json!({"session_id":session_id,"generation":generation,"intent":intent})),
            Some(credential)
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
            Some(json!({"session_id":session_id,"generation":generation,"running":true})),
            Some(credential)
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
            Some(credential)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (save_status, save_error) = request(
        app.clone(),
        "POST",
        "/api/save",
        Some(json!({"session_id":session_id,"generation":generation,"candidate":null})),
        Some(credential),
    )
    .await;
    assert_eq!(save_status, StatusCode::BAD_REQUEST);
    assert_eq!(save_error["code"], "SAVE_REJECTED");
    let (load_status, load_error) = request(
        app.clone(),
        "POST",
        "/api/load",
        Some(json!({"session_id":session_id,"generation":generation,"slot":{}})),
        Some(credential),
    )
    .await;
    assert_eq!(load_status, StatusCode::BAD_REQUEST);
    assert_eq!(load_error["code"], "INVALID_SAVE");
    assert_eq!(
        request(
            app.clone(),
            "DELETE",
            &format!("/api/session?session_id={session_id}&generation={generation}"),
            None,
            Some(credential)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}
