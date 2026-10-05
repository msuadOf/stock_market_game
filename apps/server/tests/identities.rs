use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use native_store::NativeDatabase;
use serde_json::{json, Value};
use server::identity::{IdentityService, IdentitySubject};
use server::identity_routes::identity_router;
use tower::ServiceExt;

async fn request(
    service: IdentityService,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = identity_router(service)
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

fn service() -> IdentityService {
    IdentityService::new(NativeDatabase::open_in_memory().unwrap())
}

#[tokio::test]
async fn register_login_password_checks_and_logout_preserve_subject() {
    let service = service();
    let (status, registration) = request(
        service.clone(),
        "/api/auth/register",
        json!({"username":"alice", "password":"correct-password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let subject: IdentitySubject = serde_json::from_value(registration["subject"].clone()).unwrap();
    let original_token = registration["token"].as_str().unwrap();
    assert_eq!(service.authenticate(original_token).unwrap(), subject);
    assert_eq!(
        request(
            service.clone(),
            "/api/auth/login",
            json!({"username":"alice", "password":"wrong-password"}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, login) = request(
        service.clone(),
        "/api/auth/login",
        json!({"username":"alice", "password":"correct-password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["subject"], registration["subject"]);
    assert_ne!(login["token"], registration["token"]);
    assert_eq!(
        request(
            service.clone(),
            "/api/auth/logout",
            json!({}),
            Some(original_token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(service.authenticate(original_token).is_err());
    assert_eq!(
        service
            .authenticate(login["token"].as_str().unwrap())
            .unwrap(),
        subject
    );
}

#[tokio::test]
async fn strict_payload_and_errors_never_echo_password_or_tokens() {
    let service = service();
    for payload in [
        json!({"username":"alice","password":"super-secret-password","account_id":7}),
        json!({"username":"alice","password":123}),
    ] {
        let (status, value) = request(service.clone(), "/api/auth/register", payload, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!value.to_string().contains("super-secret-password"));
    }
    let (status, value) = request(
        service.clone(),
        "/api/auth/guest",
        json!({"subject_id":"forged"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!value.to_string().contains("forged"));
    let (status, value) = request(
        service,
        "/api/auth/logout",
        json!({}),
        Some("secret-bearer-value"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!value.to_string().contains("secret-bearer-value"));
}

#[tokio::test]
async fn anonymous_credentials_survive_database_reopen_and_are_distinct() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!("identity-{}.sqlite", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let service = IdentityService::new(NativeDatabase::open(&path).unwrap());
    let first = request(service.clone(), "/api/auth/guest", json!({}), None)
        .await
        .1;
    let second = request(service.clone(), "/api/auth/guest", json!({}), None)
        .await
        .1;
    assert_ne!(first["subject"], second["subject"]);
    let token = first["token"].as_str().unwrap();
    let subject = service.authenticate(token).unwrap();
    assert_eq!(subject.username, None);
    drop(service);
    let database_bytes = std::fs::read(&path).unwrap();
    assert!(!database_bytes
        .windows(token.len())
        .any(|window| window == token.as_bytes()));
    let reopened = IdentityService::new(NativeDatabase::open(&path).unwrap());
    assert_eq!(reopened.authenticate(token).unwrap(), subject);
    assert_ne!(
        reopened
            .authenticate(second["token"].as_str().unwrap())
            .unwrap(),
        subject
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn passwords_have_distinct_argon2id_salts_and_duplicates_do_not_replace_accounts() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let service = IdentityService::new(database.clone());
    let first = request(
        service.clone(),
        "/api/auth/register",
        json!({"username":"alice","password":"same-safe-password"}),
        None,
    )
    .await
    .1;
    let second = request(
        service.clone(),
        "/api/auth/register",
        json!({"username":"bob","password":"same-safe-password"}),
        None,
    )
    .await
    .1;
    let first_record = database.subject_by_username("alice").unwrap().unwrap();
    let second_record = database.subject_by_username("bob").unwrap().unwrap();
    let first_hash = first_record.password_hash.as_ref().unwrap();
    let second_hash = second_record.password_hash.as_ref().unwrap();
    assert!(first_hash.starts_with("$argon2id$"));
    assert!(second_hash.starts_with("$argon2id$"));
    assert_ne!(first_hash, second_hash);
    assert!(!first_hash.contains("same-safe-password"));
    assert_eq!(
        request(
            service.clone(),
            "/api/auth/register",
            json!({"username":"alice","password":"replacement-password"}),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        service
            .authenticate(first["token"].as_str().unwrap())
            .unwrap()
            .subject_id,
        first_record.subject_id
    );
    assert_eq!(
        service
            .authenticate(second["token"].as_str().unwrap())
            .unwrap()
            .subject_id,
        second_record.subject_id
    );
}

#[tokio::test]
async fn real_application_router_exposes_authenticated_self_only() {
    let manager = server::SessionManager::default();
    let service = IdentityService::new(manager.database());
    let identity = service.guest().unwrap();
    let application = server::app_router_with_manager(manager);
    let response = application
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header("authorization", format!("Bearer {}", identity.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body, serde_json::to_value(identity.subject).unwrap());
    assert!(body.get("token").is_none());
    assert!(body.get("password_hash").is_none());
    let response = application
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn registered_password_verification_survives_database_reopen_without_plaintext() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!(
            "registered-identity-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let service = IdentityService::new(NativeDatabase::open(&path).unwrap());
    let registration = request(
        service.clone(),
        "/api/auth/register",
        json!({"username":"alice","password":"password-not-for-disk"}),
        None,
    )
    .await
    .1;
    drop(service);
    let database_bytes = std::fs::read(&path).unwrap();
    assert!(!database_bytes
        .windows("password-not-for-disk".len())
        .any(|window| window == b"password-not-for-disk"));
    let reopened = IdentityService::new(NativeDatabase::open(&path).unwrap());
    let (status, login) = request(
        reopened.clone(),
        "/api/auth/login",
        json!({"username":"alice","password":"password-not-for-disk"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["subject"], registration["subject"]);
    assert_eq!(
        reopened
            .authenticate(registration["token"].as_str().unwrap())
            .unwrap(),
        reopened
            .authenticate(login["token"].as_str().unwrap())
            .unwrap()
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
