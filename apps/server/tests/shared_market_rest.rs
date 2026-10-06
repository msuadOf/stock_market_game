#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use server::{identity::IdentityService, SessionManager};
use tower::ServiceExt;

async fn request(
    application: axum::Router,
    method: &str,
    uri: &str,
    payload: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if payload.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = application
        .oneshot(
            builder
                .body(payload.map_or_else(Body::empty, |payload| Body::from(payload.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let is_json = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else if is_json {
        serde_json::from_slice(&bytes).unwrap()
    } else {
        Value::String(String::from_utf8(bytes.to_vec()).unwrap())
    };
    (status, value)
}

#[tokio::test]
async fn public_guests_join_one_market_and_cannot_control_or_read_another_account() {
    let manager = SessionManager::default();
    let identities = IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let visitor = identities.guest().unwrap();
    let application = server::app_router_with_manager(manager.clone());
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let payload = json!({"setup":setup,"seed":"7"});
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/new",
            Some(payload.clone()),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, created) = request(
        application.clone(),
        "POST",
        "/api/new",
        Some(payload.clone()),
        Some(&creator.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(created.get("session_token").is_none());
    assert_eq!(created["setup"], serde_json::to_value(setup).unwrap());
    let id = created["session_id"].as_str().unwrap();
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/new",
            Some(payload),
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(manager.active_session_count(), 1);
    let join = json!({"session_id":id,"generation":"1","confirmed_rejoin":false});
    let (status, joined) = request(
        application.clone(),
        "POST",
        "/api/markets/join",
        Some(join.clone()),
        Some(&visitor.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second_join = request(
        application.clone(),
        "POST",
        "/api/markets/join",
        Some(join),
        Some(&visitor.token),
    )
    .await;
    assert_eq!(second_join.0, StatusCode::OK);
    assert_eq!(second_join.1["member"], joined["member"]);
    let snapshot_uri = format!("/api/snapshot?session_id={id}");
    let visitor_snapshot = request(
        application.clone(),
        "GET",
        &snapshot_uri,
        None,
        Some(&visitor.token),
    )
    .await;
    assert_eq!(visitor_snapshot.0, StatusCode::OK);
    let accounts = visitor_snapshot.1["accounts"].as_object().unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(accounts.contains_key(joined["member"]["account_id"].to_string().trim_matches('"')));
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/running",
            Some(json!({"session_id":id,"generation":"1","running":true})),
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/save",
            Some(json!({"session_id":id,"generation":"1","candidate":null})),
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            application.clone(),
            "GET",
            &format!("{snapshot_uri}&account_id=0"),
            None,
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    manager.remove(id).unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_reset_is_global_generation_guarded_and_requires_external_control() {
    let manager = SessionManager::default();
    let identities = IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let visitor = identities.guest().unwrap();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let application = server::app_router_with_manager(manager.clone());
    let created = request(
        application.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":setup,"seed":"7"})),
        Some(&creator.token),
    )
    .await
    .1;
    let id = created["session_id"].as_str().unwrap();
    let reset = json!({"session_id":id,"generation":"1","setup":setup,"seed":"11"});
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/market/reset",
            Some(reset.clone()),
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let restarted = request(
        application.clone(),
        "POST",
        "/api/market/reset",
        Some(reset.clone()),
        Some(&creator.token),
    )
    .await;
    assert_eq!(restarted.0, StatusCode::OK);
    assert_eq!(restarted.1["session_id"], id);
    assert_eq!(restarted.1["generation"], "2");
    assert_eq!(restarted.1["seed"], "11");
    assert_eq!(restarted.1["can_control"], true);
    assert_eq!(manager.active_session_count(), 1);
    assert_eq!(
        request(
            application,
            "POST",
            "/api/market/reset",
            Some(reset),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    manager.remove(id).unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn market_context_is_authenticated_and_never_auto_joins_or_grants_control() {
    let manager = SessionManager::default();
    let identities = IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let observer = identities.guest().unwrap();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let application = server::app_router_with_manager(manager.clone());
    let created = request(
        application.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":setup,"seed":"7"})),
        Some(&creator.token),
    )
    .await
    .1;
    let id = created["session_id"].as_str().unwrap();
    let uri = format!("/api/market/context?session_id={id}");
    assert_eq!(
        request(application.clone(), "GET", &uri, None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let own = request(application.clone(), "GET", &uri, None, Some(&creator.token)).await;
    assert_eq!(own.0, StatusCode::OK);
    assert_eq!(own.1, created);
    let public = request(
        application.clone(),
        "GET",
        &uri,
        None,
        Some(&observer.token),
    )
    .await;
    assert_eq!(public.0, StatusCode::OK);
    assert!(public.1["member"].is_null());
    assert_eq!(public.1["can_control"], false);
    assert_eq!(public.1["needs_rejoin"], true);
    assert_eq!(
        request(
            application,
            "GET",
            &format!("{uri}&account_id=0"),
            None,
            Some(&observer.token)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    manager.remove(id).unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn archive_metadata_mutations_require_current_generation_and_independent_control() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    let mut producer = engine::session::protocol::ProtocolSession::new(setup.clone(), 19).unwrap();
    let update = producer.end_civil_day_update().unwrap();
    let candidate = native_store::DayEndCandidate::capture(
        &producer,
        &engine::session::protocol::SaveCandidateKey {
            seq: update.seq_to,
            settled_date: update.boundary.settled_date,
        },
    )
    .unwrap();
    let manager = SessionManager::default();
    let identities = IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let visitor = identities.guest().unwrap();
    native_store::ArchiveWriter::activate(manager.database(), "manual")
        .unwrap()
        .save_day_end(&candidate)
        .unwrap();
    let application = server::app_router_with_manager(manager.clone());
    let created = request(
        application.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":setup,"seed":"19"})),
        Some(&creator.token),
    )
    .await
    .1;
    let id = created["session_id"].as_str().unwrap();
    let mutation = json!({"session_id":id,"slot_id":"manual","name":"已重命名","generation":"1"});
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/archives/rename",
            Some(mutation.clone()),
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut stale = mutation.clone();
    stale["generation"] = json!("0");
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/archives/rename",
            Some(stale.clone()),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/archives/copy",
            Some(stale),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/archives/rename",
            Some(mutation.clone()),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        manager
            .database()
            .list()
            .unwrap()
            .iter()
            .find(|entry| entry.slot_id == "manual")
            .unwrap()
            .name,
        "已重命名"
    );
    assert_eq!(
        request(
            application,
            "POST",
            "/api/archives/copy",
            Some(mutation),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(manager.database().list().unwrap().len(), 2);
    manager.remove(id).unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn loading_an_economic_archive_preserves_external_control_and_requires_explicit_rejoin() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    let mut producer = engine::session::protocol::ProtocolSession::new(setup.clone(), 19).unwrap();
    let update = producer.end_civil_day_update().unwrap();
    let slot = producer
        .save_candidate(&engine::session::protocol::SaveCandidateKey {
            seq: update.seq_to,
            settled_date: update.boundary.settled_date,
        })
        .unwrap();
    let manager = SessionManager::default();
    let identities = IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let visitor = identities.guest().unwrap();
    let application = server::app_router_with_manager(manager.clone());
    let created = request(
        application.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":setup,"seed":"19"})),
        Some(&creator.token),
    )
    .await
    .1;
    let id = created["session_id"].as_str().unwrap();
    let loaded = request(
        application.clone(),
        "POST",
        "/api/load",
        Some(json!({"session_id":id,"generation":"1","slot":slot})),
        Some(&creator.token),
    )
    .await;
    assert_eq!(loaded.0, StatusCode::OK);
    let markets = request(
        application.clone(),
        "GET",
        "/api/markets",
        None,
        Some(&creator.token),
    )
    .await
    .1;
    assert_eq!(markets["markets"][0]["can_control"], true);
    assert_eq!(markets["markets"][0]["needs_rejoin"], true);
    assert!(markets["markets"][0]["member"].is_null());
    assert_eq!(markets["markets"][0]["generation"], "2");
    let running = json!({"session_id":id,"generation":"2","running":false});
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/running",
            Some(running.clone()),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/running",
            Some(running),
            Some(&visitor.token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/markets/join",
            Some(json!({"session_id":id,"generation":"1","confirmed_rejoin":true})),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            application.clone(),
            "POST",
            "/api/markets/join",
            Some(json!({"session_id":id,"generation":"2","confirmed_rejoin":false})),
            Some(&creator.token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let join = json!({"session_id":id,"generation":"2","confirmed_rejoin":true});
    let first = request(
        application.clone(),
        "POST",
        "/api/markets/join",
        Some(join.clone()),
        Some(&creator.token),
    )
    .await;
    let repeated = request(
        application,
        "POST",
        "/api/markets/join",
        Some(join),
        Some(&creator.token),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["member"], repeated.1["member"]);
    assert_eq!(first.1["can_control"], true);
    manager.remove(id).unwrap().shutdown().await.unwrap();
}
