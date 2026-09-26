#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::json;
use server::{app_router_with_manager, SessionManager};
use tower::ServiceExt;

#[tokio::test]
async fn pause_preferences_requires_owner_and_current_canonical_generation() {
    let manager = SessionManager::default();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let id = manager.new_session(setup, 1).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let app = app_router_with_manager(manager.clone());
    for (session, token, generation, expected, code) in [
        (
            id.as_str(),
            None,
            "1",
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
        ),
        (
            &id,
            Some("invalid-session-token"),
            "1",
            StatusCode::FORBIDDEN,
            "SESSION_FORBIDDEN",
        ),
        (
            &id,
            Some(handles.session_token.as_str()),
            "01",
            StatusCode::BAD_REQUEST,
            "INVALID_GENERATION",
        ),
        (
            &id,
            Some(handles.session_token.as_str()),
            "+1",
            StatusCode::BAD_REQUEST,
            "INVALID_GENERATION",
        ),
        (
            &id,
            Some(handles.session_token.as_str()),
            "0",
            StatusCode::BAD_REQUEST,
            "PAUSE_PREFERENCES_REJECTED",
        ),
        (
            "absent",
            Some(handles.session_token.as_str()),
            "1",
            StatusCode::NOT_FOUND,
            "UNKNOWN_SESSION",
        ),
        (
            &id,
            Some(handles.session_token.as_str()),
            "1",
            StatusCode::NO_CONTENT,
            "",
        ),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/pause-preferences")
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let body = json!({"session_id": session, "generation": generation,
            "preferences": {"pause_after_close": true, "pause_before_open": false}});

        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            expected,
            "generation={generation}, token={token:?}"
        );
        if expected != StatusCode::NO_CONTENT {
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["code"], code);
        }
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/pause-preferences")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    manager.remove(&id).unwrap().shutdown().await.unwrap();
}
