//! HTTP + WS 路由（与前端 RemoteHost 严格对齐）。
//!
//! 契约（见任务详情 / ADR-0005 §6 双通道）：
//! - POST /api/new     body {setup, seed}        -> 200 {session_id} | 400
//! - POST /api/intent  body {session_id, intent} -> 200 | 404 | 400
//! - GET  /api/snapshot?session_id=..           -> 200 Snapshot | 404
//! - POST /api/speed   body {session_id, speed}  -> 200 | 404 | 400
//! - GET  /api/speed?session_id=..              -> 200 SpeedMetrics | 404
//! - POST /api/running body {session_id, running}-> 200 | 404
//! - POST /api/save | /api/load                  -> 存档/原子恢复
//! - DELETE /api/session?session_id=..           -> 停止并删除会话
//! - WS   /ws?session_id=..&delivery=..           -> Bearer 鉴权后先发 public baseline，再按 push/pull 交付 PublisherFrame
//!
//! engine 类型经 serde_json 跨界（server 是 Rust，engine 作 rlib 依赖，无 TS）。
//! 错误处理（铁律二）：未知 session → 404（不静默 200）；非法 body/构造 → 400；
//! engine 失败透传文案，绝不静默吞。

use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use futures_util::{SinkExt, StreamExt};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::actor::{NewSessionError, SendCommandError, SessionManager, MAX_SPEED_MULTIPLIER};
use crate::publisher::{ClientFrameBuffer, FrameBufferError, PublisherFrame};

const CLIENT_PUSH_INTERVAL: Duration = Duration::from_millis(16);
// The JSON envelope wraps one engine save. Leave room for its session identity
// while allowing every save accepted by the engine decoder through this route.
pub const MAX_LOAD_BODY_BYTES: usize = engine::MAX_SAVE_DECODE_BYTES + 1024 * 1024;
const MAX_NESTED_SAVE_DEPTH: usize = 64;

/// 路由共享状态：单一 `SessionManager`（actor 各自独占 GameSession，manager 仅持消息端点）。
#[derive(Clone)]
pub struct AppState {
    pub manager: SessionManager,
}

fn authorized_session(
    state: &AppState,
    session_id: &str,
    token: Option<&str>,
) -> Result<Arc<crate::actor::SessionHandles>, Box<Response>> {
    let Some(token) = token.filter(|token| !token.is_empty()) else {
        return Err(Box::new(api_error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "missing session token",
        )));
    };
    let Some(handles) = state.manager.lookup(session_id) else {
        return Err(Box::new(api_error(
            StatusCode::NOT_FOUND,
            "UNKNOWN_SESSION",
            "unknown session",
        )));
    };
    // Sessions are currently local single-player authorities. The opaque token is checked
    // against the actor handle before any report query, so another session cannot probe IDs.
    if handles.session_token != token {
        return Err(Box::new(api_error(
            StatusCode::FORBIDDEN,
            "SESSION_FORBIDDEN",
            "session token does not authorize this session",
        )));
    }
    Ok(handles)
}

fn authorization_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

/// /api/new 请求体。u64 以十进制字符串跨 JSON，避免 JavaScript Number 精度丢失。
#[derive(Debug, Deserialize)]
pub struct NewSessionBody {
    pub setup: engine::SessionSetup,
    pub seed: String,
}

/// /api/new 200 响应体。
#[derive(Debug, Serialize)]
pub struct NewSessionResp {
    pub session_id: String,
    pub session_token: String,
}

#[derive(Debug, Serialize)]
struct ApiError {
    code: &'static str,
    message: String,
}

fn api_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (
        status,
        Json(ApiError {
            code,
            message: message.into(),
        }),
    )
        .into_response()
}

fn invalid_json_response(error: JsonRejection) -> Response {
    api_error(StatusCode::BAD_REQUEST, "INVALID_JSON", error.body_text())
}

/// /api/intent 请求体。
#[derive(Debug, Deserialize)]
pub struct IntentBody {
    pub session_id: String,
    pub intent: engine::Intent,
}

/// /api/speed 请求体。
#[derive(Debug, Deserialize)]
pub struct SpeedBody {
    pub session_id: String,
    speed: SpeedValue,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SpeedValue {
    Multiplier(f64),
    Mode(String),
}

impl SpeedValue {
    fn multiplier(self) -> Result<f64, String> {
        match self {
            Self::Multiplier(value) if value <= MAX_SPEED_MULTIPLIER => Ok(value),
            Self::Multiplier(value) => Err(format!(
                "speed multiplier {value} exceeds maximum {MAX_SPEED_MULTIPLIER}"
            )),
            // Fastest 是 actor 内部专用哨兵：进入有界 CPU 时间片 tight-loop，
            // 不把“最快”伪装成某个固定倍率。
            Self::Mode(mode) if mode == "Fastest" => Ok(f64::INFINITY),
            Self::Mode(mode) => Err(format!("unknown speed mode: {mode}")),
        }
    }
}

#[cfg(test)]
mod speed_value_tests {
    use super::SpeedValue;

    #[test]
    fn fastest_maps_to_unbounded_actor_mode() {
        let multiplier = SpeedValue::Mode("Fastest".to_string())
            .multiplier()
            .expect("Fastest 应是合法速度模式");

        assert_eq!(multiplier, f64::INFINITY);
    }
}

#[derive(Debug, Deserialize)]
pub struct RunningBody {
    pub session_id: String,
    pub running: bool,
}

#[derive(Debug, Deserialize)]
pub struct PausePreferencesBody {
    pub session_id: String,
    pub generation: String,
    pub preferences: engine::session::protocol::PausePreferences,
}

pub async fn api_pause_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<PausePreferencesBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let handles = match authorized_session(&state, &body.session_id, authorization_token(&headers))
    {
        Ok(handles) => handles,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .set_pause_preferences(generation, body.preferences)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SendCommandError::Rejected(reason)) => api_error(
            StatusCode::BAD_REQUEST,
            "PAUSE_PREFERENCES_REJECTED",
            reason,
        ),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            error.to_string(),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct HostParityAdvanceCivilDayBody {
    pub session_id: String,
    pub generation: String,
}

#[cfg(feature = "host-parity")]
#[derive(Debug, Deserialize)]
pub struct HostParityStepBody {
    pub session_id: String,
    pub generation: String,
}

fn parse_host_parity_generation(value: &str) -> Result<u64, Box<Response>> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Box::new(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_GENERATION",
            "generation must be a canonical decimal u64",
        )));
    }
    value.parse().map_err(|_| {
        Box::new(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_GENERATION",
            "generation must be a canonical decimal u64",
        ))
    })
}

#[derive(Debug, Deserialize)]
struct RestoreEnvelope {
    pub session_id: String,
    pub slot: Box<serde_json::value::RawValue>,
}

struct SaveDepthLimit {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for SaveDepthLimit {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SaveDepthVisitor { depth: self.depth })
    }
}

struct SaveDepthVisitor {
    depth: usize,
}

impl<'de> Visitor<'de> for SaveDepthVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a save JSON value with bounded nesting depth")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| serde::de::Error::custom("save nesting depth overflowed"))?;
        if depth > MAX_NESTED_SAVE_DEPTH {
            return Err(serde::de::Error::custom(format!(
                "save nesting depth exceeds {MAX_NESTED_SAVE_DEPTH}"
            )));
        }
        while sequence
            .next_element_seed(SaveDepthLimit { depth })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| serde::de::Error::custom("save nesting depth overflowed"))?;
        if depth > MAX_NESTED_SAVE_DEPTH {
            return Err(serde::de::Error::custom(format!(
                "save nesting depth exceeds {MAX_NESTED_SAVE_DEPTH}"
            )));
        }
        while map.next_key::<IgnoredAny>()?.is_some() {
            map.next_value_seed(SaveDepthLimit { depth })?;
        }
        Ok(())
    }
}

fn preflight_save_depth(json: &[u8]) -> Result<(), String> {
    let mut deserializer = serde_json::Deserializer::from_slice(json);
    SaveDepthLimit { depth: 0 }
        .deserialize(&mut deserializer)
        .and_then(|()| deserializer.end())
        .map_err(|error| error.to_string())
}

/// /api/snapshot / /ws 共用的 query 参数。
#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PublicReportQueryParams {
    pub session_id: String,
    pub cursor: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct NpcDiagnosticsQueryParams {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub session_id: String,
    #[serde(default)]
    pub delivery: DeliveryMode,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    #[default]
    Push,
    Pull,
}

#[derive(Debug, Deserialize)]
enum ClientCommand {
    GetFrame {},
    Resync {},
    SubmitIntent {
        request_id: u64,
        intent: engine::Intent,
    },
}

/// POST /api/new：构造 session → spawn actor → 返回 session_id。
///
/// - 反序列化失败 / engine 构造失败 → 400（带原因文案）。
pub async fn api_new(
    State(state): State<AppState>,
    body: Result<Json<NewSessionBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let seed = match body.seed.parse::<u64>() {
        Ok(seed) => seed,
        Err(error) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_SEED",
                format!("seed must be a decimal integer in 0..=u64::MAX: {error}"),
            );
        }
    };
    match state.manager.new_session(body.setup, seed) {
        Ok(id) => {
            info!(session = %id, "new session created");
            let Some(handles) = state.manager.lookup(&id) else {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ACTOR_GONE",
                    "session actor was not registered",
                );
            };
            (
                StatusCode::OK,
                Json(NewSessionResp {
                    session_id: id,
                    session_token: handles.session_token.clone(),
                }),
            )
                .into_response()
        }
        Err(NewSessionError::InvalidSetup(e)) => {
            warn!(error = %e, "new_session rejected");
            api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_SETUP",
                format!("invalid setup: {e}"),
            )
        }
        Err(NewSessionError::Capacity { max }) => {
            warn!(max, "new_session rejected because capacity was reached");
            api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "SESSION_CAPACITY_REACHED",
                format!("server permits at most {max} active sessions"),
            )
        }
    }
}

pub async fn api_public_report_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(company_id): Path<String>,
    Query(params): Query<PublicReportQueryParams>,
) -> Response {
    let handles =
        match authorized_session(&state, &params.session_id, authorization_token(&headers)) {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let query = engine::company::PublicReportQuery {
        company_id,
        cursor: params.cursor,
        page_size: params.limit,
    };
    match handles.public_report_page(query).await {
        Ok(page) => (StatusCode::OK, Json(page)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => public_query_error(reason),
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("report query cannot validate speed")
        }
    }
}

pub async fn api_public_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((company_id, report_id)): Path<(String, String)>,
    Query(params): Query<PublicReportQueryParams>,
) -> Response {
    let handles =
        match authorized_session(&state, &params.session_id, authorization_token(&headers)) {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    match handles.public_report(report_id).await {
        Ok(report) if report.company_id == company_id => {
            (StatusCode::OK, Json(report)).into_response()
        }
        Ok(_) => api_error(
            StatusCode::NOT_FOUND,
            "REPORT_NOT_VISIBLE",
            "report is not visible for this company",
        ),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => public_query_error(reason),
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("report query cannot validate speed")
        }
    }
}

pub async fn api_npc_decision_diagnostics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account): Path<u64>,
    Query(params): Query<NpcDiagnosticsQueryParams>,
) -> Response {
    let handles =
        match authorized_session(&state, &params.session_id, authorization_token(&headers)) {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    match handles
        .npc_decision_diagnostics(params.generation, engine::AccountId(account))
        .await
    {
        Ok((generation, result)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "generation": generation.to_string(),
                "diagnostics": result,
            })),
        )
            .into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "DIAGNOSTICS_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("diagnostic query cannot validate speed")
        }
    }
}

fn public_query_error(reason: String) -> Response {
    if reason.contains("page size") {
        api_error(StatusCode::BAD_REQUEST, "INVALID_REPORT_LIMIT", reason)
    } else if reason.contains("cursor") {
        api_error(StatusCode::BAD_REQUEST, "INVALID_REPORT_CURSOR", reason)
    } else if reason.contains("no publication") || reason.contains("early read") {
        api_error(StatusCode::NOT_FOUND, "REPORT_NOT_VISIBLE", reason)
    } else {
        api_error(
            StatusCode::BAD_REQUEST,
            "PUBLIC_REPORT_QUERY_REJECTED",
            reason,
        )
    }
}

/// POST /api/intent：入队玩家意图（当前固定 player 0）。
///
/// - 未知 session → 404；engine 拒绝/actor 关闭 → 400/500；成功 → 200。
pub async fn api_intent(
    State(state): State<AppState>,
    body: Result<Json<IntentBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.enqueue(body.intent).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %body.session_id, "intent: actor gone");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ACTOR_GONE",
                "session actor gone",
            )
        }
        Err(SendCommandError::Rejected(reason)) => {
            warn!(session = %body.session_id, "intent rejected by engine");
            api_error(StatusCode::BAD_REQUEST, "INTENT_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("enqueue cannot validate speed"),
    }
}

/// GET /api/snapshot：取完整快照。
///
/// - 未知 session → 404；成功 → 200 JSON Snapshot。
pub async fn api_snapshot(
    State(state): State<AppState>,
    Query(q): Query<SessionQuery>,
) -> Response {
    let Some(handles) = state.manager.lookup(&q.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.snapshot().await {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %q.session_id, "snapshot: actor gone");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ACTOR_GONE",
                "session actor gone",
            )
        }
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "SNAPSHOT_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("snapshot cannot validate speed"),
    }
}

/// POST /api/save：在 actor 内生成一致存档。存档含隐藏 V，只应由会话所有者持久化。
pub async fn api_save(
    State(state): State<AppState>,
    body: Result<Json<SessionQuery>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.save().await {
        Ok(slot) => (StatusCode::OK, Json(slot)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "SAVE_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("save cannot validate speed"),
    }
}

/// POST /api/load：完整校验通过后原子替换 actor 会话，失败保留原状态。
pub async fn api_load(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() > MAX_LOAD_BODY_BYTES {
        return api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "SAVE_BODY_TOO_LARGE",
            format!("save request exceeds {MAX_LOAD_BODY_BYTES} bytes"),
        );
    }
    let body: RestoreEnvelope = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_JSON",
                format!("invalid restore envelope: {error}"),
            );
        }
    };
    if let Err(message) = preflight_save_depth(body.slot.get().as_bytes()) {
        return api_error(StatusCode::BAD_REQUEST, "SAVE_RESOURCE_LIMIT", message);
    }
    let decode_limits = engine::SaveDecodeLimits {
        max_total_bytes: engine::MAX_SAVE_DECODE_BYTES,
    };
    let slot = match engine::decode_save_slot(body.slot.get().as_bytes(), &decode_limits) {
        Ok(slot) => slot,
        Err(engine::SessionError::ResourceLimit(message)) => {
            return api_error(StatusCode::BAD_REQUEST, "SAVE_RESOURCE_LIMIT", message);
        }
        Err(error) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SAVE", error.to_string()),
    };
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.restore(slot).await {
        Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "INVALID_SAVE", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("load cannot validate speed"),
    }
}

/// POST /api/speed：改变倍速。
///
/// - 未知 session → 404；非法 speed → 400，绝不伪装成成功。
pub async fn api_speed(
    State(state): State<AppState>,
    body: Result<Json<SpeedBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    let speed = match body.speed.multiplier() {
        Ok(speed) => speed,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SPEED", message),
    };
    match handles.set_speed(speed).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %body.session_id, "speed: actor gone");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ACTOR_GONE",
                "session actor gone",
            )
        }
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "SPEED_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(speed)) => api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_SPEED",
            format!(
                "speed must be Fastest or a finite multiplier in (0, {MAX_SPEED_MULTIPLIER}], got {speed}"
            ),
        ),
    }
}

/// GET /api/speed：读取服务端权威的设定速度与最近完成采样窗口中的实际倍率。
pub async fn api_speed_metrics(
    State(state): State<AppState>,
    Query(q): Query<SessionQuery>,
) -> Response {
    let Some(handles) = state.manager.lookup(&q.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.speed_metrics().await {
        Ok(metrics) => (StatusCode::OK, Json(metrics)).into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %q.session_id, "speed metrics: actor gone");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ACTOR_GONE",
                "session actor gone",
            )
        }
        Err(SendCommandError::Rejected(_)) => {
            unreachable!("speed metrics cannot be rejected")
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("speed metrics cannot validate speed")
        }
    }
}

/// POST /api/running：显式暂停/恢复远程会话，隐藏页面不会继续消耗服务端 CPU。
pub async fn api_running(
    State(state): State<AppState>,
    body: Result<Json<RunningBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.set_running(body.running).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "RUNNING_STATE_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("running state cannot validate speed")
        }
    }
}

#[cfg(feature = "host-parity")]
pub async fn api_host_parity_advance_civil_day(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<HostParityAdvanceCivilDayBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let handles = match authorized_session(&state, &body.session_id, authorization_token(&headers))
    {
        Ok(handles) => handles,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.advance_civil_day(generation).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "CIVIL_DAY_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("civil-day command cannot validate speed")
        }
    }
}

#[cfg(feature = "host-parity")]
pub async fn api_host_parity_step(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<HostParityStepBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let handles = match authorized_session(&state, &body.session_id, authorization_token(&headers))
    {
        Ok(handles) => handles,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.step(generation).await {
        Ok(events) => (StatusCode::OK, Json(events)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "STEP_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("step command cannot validate speed")
        }
    }
}

/// DELETE /api/session：从 manager 移除并确认 actor 已停止。
pub async fn api_delete_session(
    State(state): State<AppState>,
    Query(q): Query<SessionQuery>,
) -> Response {
    let Some(handles) = state.manager.remove(&q.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.shutdown().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SendCommandError::ActorGone) => StatusCode::NO_CONTENT.into_response(),
        Err(SendCommandError::Rejected(reason)) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "SHUTDOWN_REJECTED",
            reason,
        ),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("shutdown cannot validate speed"),
    }
}

/// WS /ws：握手 → 先发完整 Snapshot 对齐基线 → 按客户端选择推送或拉取 PublisherFrame。
///
/// - 缺 token / 未知 session → 拒绝（当前握手仅校验 token 存在性）。
/// - 心跳：~30s 后端发 Ping；客户端不回则由 tungstenite/代理超时清理（ADR-0005 §6）。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let handles = match authorized_session(&state, &q.session_id, authorization_token(&headers)) {
        Ok(handles) => handles,
        Err(response) => return *response,
    };
    // handles 已是 Arc<SessionHandles>；clone 一份 event_tx 给 select 循环，handles 给取基线快照。
    let event_tx = handles.event_tx.clone();
    ws.on_upgrade(move |socket| run_ws(socket, event_tx, handles, q.delivery))
}

/// WS 连接主循环。
///
/// 1. 先经 actor 取完整 Snapshot，序列化 JSON 发给客户端（对齐基线）。
/// 2. 订阅更新 broadcast，把 Event[] + 可选权威运行快照作为一个 JSON 帧推出。
/// 3. 同时读客户端消息（仅作存活/pong 探测；当前不处理客户端业务消息）。
/// 4. 30s 心跳：发 Ping。
async fn run_ws(
    socket: axum::extract::ws::WebSocket,
    event_tx: tokio::sync::broadcast::Sender<crate::actor::EngineUpdate>,
    handles: Arc<crate::actor::SessionHandles>,
    delivery: DeliveryMode,
) {
    let (mut sender, mut receiver) = socket.split();

    // 先订阅再取基线，消除 snapshot 与 subscribe 之间丢事件的竞态；基线 seq 之前的
    // 缓冲事件在后续读取时跳过。
    let mut rx = event_tx.subscribe();
    let mut baseline_seq;
    let mut baseline_tick;
    let mut timeline_generation;
    let initial_failure;

    // 1. 对齐基线：发完整 Snapshot JSON。
    match handles.public_baseline().await {
        Ok(baseline) => {
            baseline_seq = baseline.snapshot.seq;
            baseline_tick = baseline.snapshot.tick;
            timeline_generation = baseline.timeline_generation;
            initial_failure = baseline.failure.clone();
            match send_baseline(&mut sender, baseline).await {
                true => {}
                false => {
                    warn!("ws: failed to send baseline snapshot; closing");
                    return;
                }
            }
        }
        Err(SendCommandError::ActorGone) => {
            warn!("ws: actor gone before public baseline");
            return;
        }
        Err(SendCommandError::Rejected(_)) => unreachable!("public baseline cannot be rejected"),
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("public baseline cannot validate speed")
        }
    }

    // 3/4. 心跳 interval + 事件/消息 select。
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = heartbeat.tick().await; // 跳过首个立即到期。
    let mut publisher = match ClientFrameBuffer::new(handles.ticks_per_day, handles.auction_ticks) {
        Ok(buffer) => buffer,
        Err(error) => {
            error!(%error, "ws: invalid publisher configuration");
            return;
        }
    };
    let mut push_clock = tokio::time::interval(CLIENT_PUSH_INTERVAL);
    push_clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = push_clock.tick().await;
    let mut awaiting_resync = initial_failure.is_some();
    let mut delivered_failure = initial_failure;

    loop {
        tokio::select! {
            // 事件到达 → 推 JSON。
            ev = rx.recv() => {
                match ev {
                    Ok(update) => {
                        if let Some(failure) = update.failure {
                            if delivered_failure.as_ref() == Some(&failure) {
                                awaiting_resync = true;
                                continue;
                            }
                            if !send_host_failure(&mut sender, failure.clone()).await { break; }
                            delivered_failure = Some(failure);
                            awaiting_resync = true;
                            continue;
                        }
                        if update.timeline_generation != timeline_generation {
                            publisher.clear();
                            awaiting_resync = true;
                            if !send_resync_required(&mut sender, "timeline_changed", None).await { break; }
                            continue;
                        }
                        if awaiting_resync { continue; }
                        let Some(protocol) = &update.update else { continue; };
                        let covered = match protocol {
                            engine::session::protocol::EngineUpdate::TickBatch(batch) =>
                                batch.frames.last().is_some_and(|frame| frame.tick <= baseline_tick && frame.seq_to <= baseline_seq),
                            engine::session::protocol::EngineUpdate::CivilUpdate(civil) =>
                                civil.tick <= baseline_tick && civil.seq_to <= baseline_seq,
                        };
                        if covered {
                            continue;
                        }
                        if let Err(error) = publisher.push(update.clone()) {
                            match error {
                                FrameBufferError::MetadataTransition => {
                                    if let Some(frame) = publisher.take() {
                                        if !send_publisher_frame(&mut sender, frame).await { break; }
                                    }
                                    if let Err(error) = publisher.push(update) {
                                        error!(%error, "ws: publisher rejected metadata segment");
                                        break;
                                    }
                                }
                                FrameBufferError::BufferCapacityExceeded { limit } => {
                                    if delivery == DeliveryMode::Push {
                                        // Backlog pressure must not discard a complete update
                                        // that has not yet reached this push client.
                                        let mut delivered = true;
                                        while let Some(frame) = publisher.take() {
                                            if !send_publisher_frame(&mut sender, frame).await {
                                                delivered = false;
                                                break;
                                            }
                                        }
                                        if !delivered { break; }
                                        if let Err(error) = publisher.push(update) {
                                            error!(%error, "ws: publisher rejected update after flushing backlog");
                                            break;
                                        }
                                    } else {
                                        warn!(limit, "ws: pull publisher buffer full; client must re-sync");
                                        publisher.clear();
                                        awaiting_resync = true;
                                        if !send_resync_required(&mut sender, "publisher_buffer_capacity", None).await {
                                            break;
                                        }
                                    }
                                }
                                other => {
                                    error!(error = %other, "ws: publisher rejected engine update");
                                    if !send_gateway_error(&mut sender, None, "PUBLISHER_SEQUENCE_ERROR", other.to_string()).await {
                                        break;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // 慢消费者必须立即重新拉快照；显式协议消息避免客户端只看到 seq 缺口。
                        warn!(missed = n, "ws: lagged, client should re-sync via snapshot");
                        publisher.clear();
                        awaiting_resync = true;
                        if !send_resync_required(&mut sender, "event_stream_lagged", Some(n)).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("ws: event stream closed (actor exited)");
                        break;
                    }
                }
            }
            _ = push_clock.tick(), if delivery == DeliveryMode::Push && !awaiting_resync => {
                if let Some(frame) = publisher.take() {
                    if !send_publisher_frame(&mut sender, frame).await {
                        break;
                    }
                }
            }
            // 心跳：30s 发 Ping（防中间设备杀空闲连接）。
            _ = heartbeat.tick() => {
                if sender.send(axum::extract::ws::Message::Ping(Vec::new())).await.is_err() {
                    debug!("ws: heartbeat ping failed; closing");
                    break;
                }
            }
            // 读客户端消息（Pong/Close/其它）：仅作存活探测与礼貌关闭，不解析业务消息。
            msg = receiver.next() => {
                match msg {
                    Some(Ok(m)) => {
                        if matches!(m, axum::extract::ws::Message::Close(_)) {
                            debug!("ws: client sent close");
                            break;
                        }
                        if let axum::extract::ws::Message::Text(text) = m {
                            let command = match serde_json::from_str::<ClientCommand>(&text) {
                                Ok(command) => command,
                                Err(error) => {
                                    if !send_gateway_error(&mut sender, None, "INVALID_CLIENT_COMMAND", format!("客户端命令不是合法协议消息：{error}")).await {
                                        break;
                                    }
                                    continue;
                                }
                            };
                            match command {
                                ClientCommand::Resync {} => {
                                    rx = event_tx.subscribe();
                                    publisher.clear();
                                    match handles.public_baseline().await {
                                        Ok(baseline) => {
                                            baseline_seq = baseline.snapshot.seq;
                                            baseline_tick = baseline.snapshot.tick;
                                            timeline_generation = baseline.timeline_generation;
                                            let latched_failure = baseline.failure.clone();
                                            if !send_baseline(&mut sender, baseline).await { break; }
                                            awaiting_resync = latched_failure.is_some();
                                            delivered_failure = latched_failure;
                                        }
                                        Err(error) => {
                                            if !send_gateway_error(&mut sender, None, "RESYNC_FAILED", error.to_string()).await { break; }
                                        }
                                    }
                                }
                                ClientCommand::GetFrame {} => {
                                    if awaiting_resync {
                                        if !send_gateway_error(&mut sender, None, "RESYNC_REQUIRED", "send Resync before requesting frames").await { break; }
                                        continue;
                                    }
                                    if delivery != DeliveryMode::Pull {
                                        if !send_gateway_error(&mut sender, None, "WRONG_DELIVERY_MODE", "GetFrame 只允许用于 pull 模式").await {
                                            break;
                                        }
                                    } else if let Some(frame) = publisher.take() {
                                        if !send_publisher_frame(&mut sender, frame).await {
                                            break;
                                        }
                                    } else {
                                        let empty = serde_json::json!({ "FrameEmpty": {} }).to_string();
                                        if sender.send(axum::extract::ws::Message::Text(empty)).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                                ClientCommand::SubmitIntent { request_id, intent } => {
                                    match handles.enqueue(intent).await {
                                        Ok(()) => {
                                            let queued = serde_json::json!({ "CommandQueued": { "request_id": request_id } }).to_string();
                                            if sender.send(axum::extract::ws::Message::Text(queued)).await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(error) => {
                                            if !send_gateway_error(&mut sender, Some(request_id), "INTENT_QUEUE_REJECTED", error.to_string()).await {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "ws: receive error; closing");
                        break;
                    }
                    None => {
                        debug!("ws: client stream ended");
                        break;
                    }
                }
            }
        }
    }
    debug!("ws run loop exited");
}

async fn send_publisher_frame(
    sender: &mut futures_util::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    frame: PublisherFrame,
) -> bool {
    match serde_json::to_string(&serde_json::json!({ "PublisherFrame": frame })) {
        Ok(json) => sender
            .send(axum::extract::ws::Message::Text(json))
            .await
            .is_ok(),
        Err(error) => {
            error!(%error, "ws: serialize publisher frame failed");
            false
        }
    }
}

async fn send_baseline(
    sender: &mut futures_util::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    baseline: crate::actor::PublicBaseline,
) -> bool {
    match baseline_wire_messages(baseline) {
        Ok(messages) => {
            for message in messages {
                if sender
                    .send(axum::extract::ws::Message::Text(message))
                    .await
                    .is_err()
                {
                    return false;
                }
            }
            true
        }
        Err(error) => {
            error!(%error, "ws: serialize public baseline failed");
            false
        }
    }
}

fn baseline_wire_messages(
    baseline: crate::actor::PublicBaseline,
) -> Result<Vec<String>, serde_json::Error> {
    let failure = baseline.failure.clone();
    let mut messages = vec![serde_json::to_string(
        &serde_json::json!({ "Baseline": baseline }),
    )?];
    if let Some(failure) = failure {
        messages.push(serde_json::to_string(
            &serde_json::json!({ "HostFailure": failure }),
        )?);
    }
    Ok(messages)
}

#[cfg(test)]
mod baseline_failure_tests {
    use super::baseline_wire_messages;
    use crate::actor::{HostFailure, PublicBaseline, PublicBaselineSnapshot};

    #[test]
    fn latched_failure_is_sent_after_an_unchanged_baseline() {
        let messages = baseline_wire_messages(PublicBaseline {
            timeline_generation: 3,
            snapshot: PublicBaselineSnapshot {
                seq: 4,
                tick: 5,
                day: 1,
                phase: engine::TradingPhase::Continuous,
                markets: Default::default(),
                accounts: Default::default(),
                daily_candles: Default::default(),
                active_daily_candles: Default::default(),
            },
            civil_date: "2030-01-02".into(),
            public_revision: 6,
            public_report_ids: Vec::new(),
            failure: Some(HostFailure {
                code: "STEP_FATAL",
                message: "invariant violation at server.step: receipt chain broke".into(),
            }),
        })
        .unwrap();

        assert_eq!(messages.len(), 2);
        let baseline: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert!(baseline["Baseline"].get("failure").is_none());
        let failure: serde_json::Value = serde_json::from_str(&messages[1]).unwrap();
        assert_eq!(failure["HostFailure"]["code"], "STEP_FATAL");
        assert_eq!(
            failure["HostFailure"]["message"],
            "invariant violation at server.step: receipt chain broke"
        );
    }
}

async fn send_host_failure(
    sender: &mut futures_util::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    failure: crate::actor::HostFailure,
) -> bool {
    sender
        .send(axum::extract::ws::Message::Text(
            serde_json::json!({ "HostFailure": failure }).to_string(),
        ))
        .await
        .is_ok()
}

async fn send_gateway_error(
    sender: &mut futures_util::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    request_id: Option<u64>,
    code: &'static str,
    message: impl Into<String>,
) -> bool {
    let json = serde_json::json!({
        "GatewayError": {
            "request_id": request_id,
            "code": code,
            "message": message.into(),
        }
    })
    .to_string();
    sender
        .send(axum::extract::ws::Message::Text(json))
        .await
        .is_ok()
}

async fn send_resync_required(
    sender: &mut futures_util::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    reason: &'static str,
    missed: Option<u64>,
) -> bool {
    let json = serde_json::json!({
        "ResyncRequired": {
            "reason": reason,
            "missed": missed,
        }
    })
    .to_string();
    sender
        .send(axum::extract::ws::Message::Text(json))
        .await
        .is_ok()
}
