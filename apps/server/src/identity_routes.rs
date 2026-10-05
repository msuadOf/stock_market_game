use crate::identity::{IdentityError, IdentityService, IdentitySubject};
use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

pub fn identity_router(service: IdentityService) -> Router {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/guest", post(guest))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(service)
}

pub fn authenticated_subject(
    service: &IdentityService,
    headers: &HeaderMap,
) -> Result<IdentitySubject, IdentityError> {
    service.authenticate(bearer_token(headers)?)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, IdentityError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(IdentityError::Unauthorized)
}

fn error_response(error: IdentityError) -> Response {
    let (status, code) = match &error {
        IdentityError::InvalidInput => (StatusCode::BAD_REQUEST, "INVALID_AUTH_INPUT"),
        IdentityError::UsernameExists => (StatusCode::CONFLICT, "USERNAME_EXISTS"),
        IdentityError::InvalidLogin => (StatusCode::UNAUTHORIZED, "INVALID_LOGIN"),
        IdentityError::Unauthorized => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
        IdentityError::Storage => (StatusCode::INTERNAL_SERVER_ERROR, "IDENTITY_STORAGE_FAILED"),
        IdentityError::Cryptography => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "IDENTITY_CRYPTOGRAPHY_FAILED",
        ),
    };
    (status, Json(serde_json::json!({"code":code,"message":error.to_string(),"where":"Server.identity","recoverable":"请重试；若持续失败请反馈错误码"}))).into_response()
}

fn invalid_json() -> Response {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code":"INVALID_AUTH_JSON","message":"身份请求必须包含当前接口规定的 JSON 字段与类型，不接受额外字段","where":"Server.identity_routes","recoverable":"请检查请求或反馈客户端版本"}))).into_response()
}

async fn register(
    State(service): State<IdentityService>,
    body: Result<Json<PasswordBody>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return invalid_json();
    };
    match tokio::task::spawn_blocking(move || service.register(&body.username, &body.password))
        .await
    {
        Ok(Ok(result)) => (
            StatusCode::CREATED,
            [(header::CACHE_CONTROL, "no-store")],
            Json(result),
        )
            .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => error_response(IdentityError::Cryptography),
    }
}

async fn login(
    State(service): State<IdentityService>,
    body: Result<Json<PasswordBody>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return invalid_json();
    };
    match tokio::task::spawn_blocking(move || service.login(&body.username, &body.password)).await {
        Ok(Ok(result)) => ([(header::CACHE_CONTROL, "no-store")], Json(result)).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => error_response(IdentityError::Cryptography),
    }
}

async fn guest(
    State(service): State<IdentityService>,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if body.is_err() {
        return invalid_json();
    }
    match service.guest() {
        Ok(result) => (
            StatusCode::CREATED,
            [(header::CACHE_CONTROL, "no-store")],
            Json(result),
        )
            .into_response(),
        Err(error) => error_response(error),
    }
}

async fn logout(
    State(service): State<IdentityService>,
    headers: HeaderMap,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if body.is_err() {
        return invalid_json();
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error_response(error),
    };
    match service.logout(token) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => error_response(error),
    }
}

async fn me(State(service): State<IdentityService>, headers: HeaderMap) -> Response {
    match authenticated_subject(&service, &headers) {
        Ok(subject) => ([(header::CACHE_CONTROL, "no-store")], Json(subject)).into_response(),
        Err(error) => error_response(error),
    }
}
