use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use engine::session::protocol::ProtocolSession;
use serde_json::{json, Value};
use tower::ServiceExt;

#[path = "identity_fixture.rs"]
mod identity_fixture;

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

fn market_setup() -> engine::SessionSetup {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 2;
    setup
}

fn correction(setup: engine::SessionSetup) -> Value {
    let session = ProtocolSession::new(setup, 7181).unwrap();
    let original = session
        .game()
        .save()
        .unwrap()
        .public_library
        .save()
        .reports
        .into_iter()
        .next()
        .unwrap();
    json!({
        "operation_id": "host-correction-1", "company": original.company,
        "supersedes": original.id, "reason": "补记前期已经收现收入",
        "entries": [{"source": 900000001, "date": "2030-01-05", "kind": "CashRevenue", "cash_flow": "Operating", "lines": [
            {"account": "1002", "side": "Debit", "amount": "10.00"},
            {"account": "6001", "side": "Credit", "amount": "10.00"}
        ]}]
    })
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
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
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn report_correction_routes_require_owner_generation_and_keep_control_separate() {
    let manager = server::SessionManager::default();
    let setup = market_setup();
    let input = correction(setup.clone());
    let (id, subject, token) = identity_fixture::create_market(&manager, setup.clone(), 7181).await;
    let handles = manager.lookup(&id).unwrap();
    let generation = identity_fixture::generation(&handles, subject).await;
    let app = server::app_router_with_manager(manager.clone());
    let (non_controller, non_controller_token) = identity_fixture::guest(&manager);
    handles
        .join_member_for(generation.parse().unwrap(), non_controller, false)
        .await
        .expect("second identity should join as an own-account member");
    let uri = format!("/api/market/report-corrections?session_id={id}&generation={generation}");
    let unauthenticated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let (member_status, member_view) =
        request(app.clone(), "GET", &uri, Some(&non_controller_token), None).await;
    assert_eq!(member_status, StatusCode::OK);
    assert_eq!(member_view, json!({"generation":generation, "value":{"pending":[],"completed":{}}}));
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&non_controller_token),
            Some(json!({"session_id":id,"generation":generation,"request":input}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "market membership allows reading shared reports, not controlling their corrections"
    );
    assert_eq!(
        request(
            app.clone(),
            "DELETE",
            "/api/market/report-corrections",
            Some(&non_controller_token),
            Some(json!({"session_id":id,"generation":generation,"operation_id":"host-correction-1"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, initial) = request(app.clone(), "GET", &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        initial,
        json!({"generation":generation, "value":{"pending":[],"completed":{}}})
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("{uri}&accountId=0"),
            Some(&token),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for generation in ["0", "01", "18446744073709551616"] {
        assert!(!request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"request":input}))
        )
        .await
        .0
        .is_success());
    }
    for _ in 0..2 {
        let (status, ack) = request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"request":input})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ack, json!({"generation":generation,"value":null}));
    }
    let pending = request(app.clone(), "GET", &uri, Some(&token), None).await.1;
    assert_eq!(pending["value"]["pending"], json!([input]));
    assert_eq!(pending["value"]["completed"], json!({}));
    assert_eq!(handles.snapshot().await.unwrap().tick, 0);
    let mut conflict = input.clone();
    conflict["reason"] = json!("不同载荷");
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"request":conflict}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            app.clone(),
            "DELETE",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"operation_id":"host-correction-1"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(app.clone(), "GET", &uri, Some(&token), None).await.1,
        initial
    );
    assert_eq!(
        request(
            app.clone(),
            "DELETE",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"operation_id":"host-correction-1"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut completed = ProtocolSession::new(setup, 7181).unwrap();
    completed.end_civil_day_update().unwrap();
    handles.restore(completed.save().unwrap()).await.unwrap();
    assert_eq!(
        request(app.clone(), "GET", &uri, Some(&token), None).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            app,
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"request":input}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    manager.remove(&id).unwrap().shutdown().await.unwrap();
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn report_correction_completion_is_public_day_end_fact_and_survives_restore() {
    let manager = server::SessionManager::default();
    let setup = market_setup();
    let input = correction(setup.clone());
    let (id, subject, token) = identity_fixture::create_market(&manager, setup, 7181).await;
    let handles = manager.lookup(&id).unwrap();
    let generation = identity_fixture::generation(&handles, subject).await;
    let app = server::app_router_with_manager(manager.clone());
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"request":input}))
        )
        .await
        .0,
        StatusCode::OK
    );
    handles.advance_civil_day(1).await.unwrap();
    let uri = format!("/api/market/report-corrections?session_id={id}&generation={generation}");
    let status = request(app.clone(), "GET", &uri, Some(&token), None).await.1;
    assert_eq!(status["value"]["pending"], json!([]));
    assert_eq!(
        status["value"]["completed"]["host-correction-1"]["request"],
        input
    );
    assert!(
        !status["value"]["completed"]["host-correction-1"]["publications"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let saved = handles.save(1, None).await.unwrap();
    handles.restore(saved).await.unwrap();
    let restored = request(
        app.clone(),
        "GET",
        &format!("/api/market/report-corrections?session_id={id}&generation=2"),
        Some(&token),
        None,
    )
    .await
    .1;
    assert_eq!(restored["value"], status["value"]);
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":"2","request":input}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app,
            "DELETE",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":"2","operation_id":"host-correction-1"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    manager.remove(&id).unwrap().shutdown().await.unwrap();
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn report_correction_failure_keeps_actor_available_for_public_cancel_and_retry() {
    let manager = server::SessionManager::with_base_ms(1);
    let setup = market_setup();
    let mut invalid = correction(setup.clone());
    invalid["company"] = json!("不存在的经营公司");
    let (id, subject, token) = identity_fixture::create_market(&manager, setup, 7181).await;
    let handles = manager.lookup(&id).unwrap();
    let generation = identity_fixture::generation(&handles, subject).await;
    let app = server::app_router_with_manager(manager.clone());
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"request":invalid}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut events = handles.subscribe_events();
    handles.set_running(true).await.unwrap();
    let failure = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    let failure = failure.failure.unwrap();
    assert_eq!(failure.code, "REPORT_CORRECTION_REJECTED");
    assert!(failure.recoverable);
    let uri = format!("/api/market/report-corrections?session_id={id}&generation={generation}");
    assert_eq!(
        request(app.clone(), "GET", &uri, Some(&token), None).await.1["value"]["pending"],
        json!([invalid])
    );
    assert_eq!(
        request(
            app,
            "DELETE",
            "/api/market/report-corrections",
            Some(&token),
            Some(json!({"session_id":id,"generation":generation,"operation_id":"host-correction-1"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let civil = handles.advance_civil_day(1).await.unwrap();
    assert_eq!(civil.civil_date, "2030-01-06");
    manager.remove(&id).unwrap().shutdown().await.unwrap();
}
