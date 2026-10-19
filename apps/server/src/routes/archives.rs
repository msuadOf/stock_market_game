use super::{
    api_error, authorization_token, authorized_market_controller, invalid_json_response, AppState,
};
use axum::extract::{rejection::JsonRejection, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveQuery {
    session_id: String,
    slot_id: Option<String>,
    generation: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveName {
    session_id: String,
    generation: String,
    slot_id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveSelect {
    session_id: String,
    generation: String,
    slot_id: String,
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/archives", axum::routing::get(list).delete(delete))
        .route("/api/archives/load", axum::routing::get(load))
        .route("/api/archives/select", axum::routing::post(select))
        .route("/api/archives/rename", axum::routing::post(rename))
        .route("/api/archives/copy", axum::routing::post(copy))
}

fn storage_error(error: native_store::StoreError) -> Response {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "ARCHIVE_STORAGE_FAILED", format!("日终存档 SQLite 操作失败，上一份有效档不受影响：{error}；请检查文件权限、磁盘空间并反馈错误详情"))
}

async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Response {
    if let Err(response) =
        authorized_market_controller(&state, &query.session_id, authorization_token(&headers)).await
    {
        return *response;
    }
    match state.manager.database().list() {
        Ok(metadata) => Json(metadata).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn load(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Response {
    if let Err(response) =
        authorized_market_controller(&state, &query.session_id, authorization_token(&headers)).await
    {
        return *response;
    }
    let database = state.manager.database();
    let slot_id = match query.slot_id {
        Some(slot_id) => slot_id,
        None => match database.selection() {
            Ok(native_store::ArchiveSelection::Selected(slot_id)) => slot_id,
            Ok(
                native_store::ArchiveSelection::Cleared
                | native_store::ArchiveSelection::Uninitialized,
            ) => return Json(serde_json::Value::Null).into_response(),
            Err(error) => return storage_error(error),
        },
    };
    match database.load(&slot_id) {
        Ok(Some(slot)) => Json(slot).into_response(),
        Ok(None) => api_error(
            StatusCode::NOT_FOUND,
            "ARCHIVE_NOT_FOUND",
            format!("日终存档槽 {slot_id} 不存在"),
        ),
        Err(error) => storage_error(error),
    }
}

async fn select(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ArchiveSelect>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match super::parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .select_archive(generation, subject, body.slot_id)
        .await
    {
        Ok(selected) => Json(selected).into_response(),
        Err(crate::actor::SendCommandError::Rejected(reason))
            if reason.contains("STALE_SESSION_GENERATION") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "ARCHIVE_SELECTION_FAILED",
            format!("市场已读档，但启动槽选择保存失败，旧选择保留；可重试选择：{error}"),
        ),
    }
}

async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Response {
    let (handles, subject) = match authorized_market_controller(
        &state,
        &query.session_id,
        authorization_token(&headers),
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return *response,
    };
    let Some(slot_id) = query.slot_id else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "ARCHIVE_SLOT_REQUIRED",
            "删除必须明确提供 slot_id",
        );
    };
    let Some(generation) = query.generation else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "ARCHIVE_GENERATION_REQUIRED",
            "删除必须提供当前 generation",
        );
    };
    let generation = match super::parse_host_parity_generation(&generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.delete_archive(generation, subject, slot_id).await {
        Ok(()) => Json(serde_json::Value::Null).into_response(),
        Err(crate::actor::SendCommandError::Rejected(reason))
            if reason.contains("STALE_SESSION_GENERATION") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "ARCHIVE_DELETE_FAILED",
            error.to_string(),
        ),
    }
}

async fn rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ArchiveName>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    mutate_metadata(state, headers, body, false).await
}

async fn copy(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ArchiveName>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    mutate_metadata(state, headers, body, true).await
}

async fn mutate_metadata(
    state: AppState,
    headers: HeaderMap,
    body: ArchiveName,
    copy: bool,
) -> Response {
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match super::parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .mutate_archive_metadata(generation, subject, body.slot_id, body.name, copy)
        .await
    {
        Ok(metadata) => Json(metadata).into_response(),
        Err(crate::actor::SendCommandError::Rejected(reason))
            if reason.contains("STALE_SESSION_GENERATION") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "ARCHIVE_METADATA_FAILED",
            error.to_string(),
        ),
    }
}
