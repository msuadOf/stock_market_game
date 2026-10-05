//! HTTP + WS 路由（与前端 RemoteHost 严格对齐）。
//!
//! 契约（见任务详情 / ADR-0005 §6 双通道）：
//! - POST /api/new     body {setup, seed}        -> 200 共享市场 context | 401 | 409 | 400
//! - POST /api/intent  body {session_id, generation, intent} -> 200 | 401 | 403 | 400
//! - GET  /api/snapshot?session_id=..           -> 200 Snapshot | 401 | 403
//! - POST /api/speed   body {session_id, generation, speed}  -> 200 | 401 | 403 | 400
//! - GET  /api/speed?session_id=..              -> 200 SpeedMetrics | 401 | 403
//! - POST /api/running body {session_id, generation, running}-> 204 | 401 | 403
//! - POST /api/save | /api/load                  -> 存档/原子恢复 | 401 | 403 | 400
//! - DELETE /api/session?session_id=..&generation=.. -> 市场控制鉴权后停止并删除整个活动市场
//! - WS   /ws?session_id=..&delivery=..           -> Bearer 鉴权后先发 public baseline，再按 push/pull 交付 PublisherFrame
//!
//! engine 类型经 serde_json 跨界（server 是 Rust，engine 作 rlib 依赖，无 TS）。
//! 错误处理（铁律二）：登录凭据缺失或无效 → 401，市场不存在、本人资金成员或独立控制能力不足 → 403；
//! 非法 body/构造 → 400，engine 失败透传文案，绝不静默吞。

use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
#[path = "routes/auth_tests.rs"]
mod auth_tests;

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
mod archives;

pub(crate) fn archive_router() -> axum::Router<AppState> {
    archives::router()
}
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

/// 本人金融接口必须由身份 credential 解析主体，再校验当前市场中的资金成员关系。
async fn authorized_session(
    state: &AppState,
    session_id: &str,
    token: Option<&str>,
) -> Result<
    (
        Arc<crate::actor::SessionHandles>,
        engine::session::OpaqueSubjectId,
    ),
    Box<Response>,
> {
    let Some(token) = token.filter(|token| !token.is_empty()) else {
        return Err(Box::new(api_error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "缺少登录凭据，请先登录",
        )));
    };
    let subject = request_subject(state, Some(token))?;
    let Some(handles) = state.manager.lookup(session_id) else {
        return Err(session_forbidden());
    };
    handles
        .resolve_member(subject.clone())
        .await
        .map_err(|error| {
            Box::new(api_error(
                StatusCode::FORBIDDEN,
                "MARKET_MEMBERSHIP_REQUIRED",
                error.to_string(),
            ))
        })?;
    Ok((handles, subject))
}

async fn authorized_market(
    state: &AppState,
    session_id: &str,
    token: Option<&str>,
) -> Result<
    (
        Arc<crate::actor::SessionHandles>,
        engine::session::OpaqueSubjectId,
    ),
    Box<Response>,
> {
    let subject = request_subject(state, token)?;
    let handles = state
        .manager
        .lookup(session_id)
        .ok_or_else(session_forbidden)?;
    Ok((handles, subject))
}

pub(crate) async fn authorized_market_controller(
    state: &AppState,
    session_id: &str,
    token: Option<&str>,
) -> Result<
    (
        Arc<crate::actor::SessionHandles>,
        engine::session::OpaqueSubjectId,
    ),
    Box<Response>,
> {
    let subject = request_subject(state, token)?;
    let handles = state
        .manager
        .lookup(session_id)
        .ok_or_else(session_forbidden)?;
    handles
        .require_control(subject.clone())
        .await
        .map_err(|error| {
            Box::new(api_error(
                StatusCode::FORBIDDEN,
                "MARKET_CONTROL_FORBIDDEN",
                error.to_string(),
            ))
        })?;
    Ok((handles, subject))
}

fn session_forbidden() -> Box<Response> {
    Box::new(api_error(
        StatusCode::FORBIDDEN,
        "SESSION_FORBIDDEN",
        "当前市场不存在或不允许访问，请重新查询市场列表",
    ))
}

fn authorization_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

fn websocket_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = authorization_token(headers) {
        return Some(token.to_owned());
    }
    let offered = headers.get("sec-websocket-protocol")?.to_str().ok()?;
    let mut credentials = offered
        .split(',')
        .map(str::trim)
        .filter_map(|protocol| protocol.strip_prefix("stock-game.auth."));
    let encoded = credentials.next()?;
    if credentials.next().is_some() || encoded.is_empty() || encoded.len() % 2 != 0 {
        return None;
    }
    let bytes = encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            Some((high * 16 + low) as u8)
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

/// /api/new 请求体。u64 以十进制字符串跨 JSON，避免 JavaScript Number 精度丢失。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewSessionBody {
    pub setup: engine::SessionSetup,
    pub seed: String,
}

/// /api/new 200 响应体。
#[derive(Debug, Serialize)]
pub struct NewSessionResp {
    pub session_id: String,
    pub setup: engine::SessionSetup,
    pub seed: String,
    pub resumed: bool,
    pub generation: String,
    pub member: Option<engine::session::MarketMembership>,
    pub can_control: bool,
    pub needs_rejoin: bool,
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
#[serde(deny_unknown_fields)]
pub struct IntentBody {
    pub session_id: String,
    pub generation: String,
    pub intent: engine::Intent,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitReportCorrectionBody {
    pub session_id: String,
    pub generation: String,
    pub request: engine::CompanyReportCorrection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelReportCorrectionBody {
    pub session_id: String,
    pub generation: String,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportCorrectionsQuery {
    pub session_id: String,
    pub generation: String,
}

fn report_correction_response<T: Serialize>(
    result: Result<(u64, T), SendCommandError>,
) -> Response {
    match result {
        Ok((generation, value)) => (
            StatusCode::OK,
            Json(serde_json::json!({"generation":generation.to_string(),"value":value})),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => api_error(
            StatusCode::BAD_REQUEST,
            "REPORT_CORRECTION_REJECTED",
            reason,
        ),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("report correction cannot validate speed")
        }
    }
}

pub async fn api_submit_report_correction(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<SubmitReportCorrectionBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    report_correction_response(
        handles
            .submit_report_correction(generation, body.request)
            .await,
    )
}

pub async fn api_cancel_report_correction(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CancelReportCorrectionBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    report_correction_response(
        handles
            .cancel_report_correction(generation, body.operation_id)
            .await,
    )
}

pub async fn api_report_corrections(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ReportCorrectionsQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REPORT_CORRECTION_QUERY",
                error.body_text(),
            )
        }
    };
    let (handles, subject) =
        match authorized_market(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&query.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    report_correction_response(handles.query_report_corrections(generation).await)
}

/// /api/speed 请求体。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedBody {
    pub session_id: String,
    pub generation: String,
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
#[serde(deny_unknown_fields)]
pub struct RunningBody {
    pub session_id: String,
    pub generation: String,
    pub running: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .set_pause_preferences_for(subject.clone(), generation, body.preferences)
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
#[serde(deny_unknown_fields)]
struct RestoreEnvelope {
    pub session_id: String,
    pub generation: String,
    pub archive_slot_id: Option<String>,
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
#[serde(deny_unknown_fields)]
pub struct SessionQuery {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerWorkingOrdersQuery {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockHistoryQuery {
    pub session_id: String,
    pub generation: u64,
    pub code: engine::StockCode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialAllocationQuery {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalTradeConfirmationsQuery {
    pub session_id: String,
    pub generation: u64,
    pub before_receipt: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalTradeHistoryBody {
    session_id: String,
    generation: String,
    query: engine::session::PersonalTradeHistoryRequest,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketHistoryBody {
    session_id: String,
    generation: String,
    query: engine::session::MarketHistoryRequest,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentMinuteHistoryBody {
    session_id: String,
    generation: String,
    query: engine::session::CurrentMinuteHistoryRequest,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculateIntradayAverageBody {
    session_id: String,
    generation: String,
    turnover_cents: String,
    trade_count: u64,
    volume_shares: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculateIntradayAverageCurveBody {
    session_id: String,
    generation: String,
    series_key: String,
    samples: Vec<engine::IntradayAverageInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    pub session_id: String,
    pub generation: String,
    pub candidate: Option<engine::session::protocol::SaveCandidateKey>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicReportQueryParams {
    pub session_id: String,
    pub cursor: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicReportAvailabilityParams {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcDiagnosticsQueryParams {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
enum ClientCommand {
    GetFrame {},
    Resync {},
    SubmitIntent {
        request_id: u64,
        generation: String,
        intent: engine::Intent,
    },
}

/// POST /api/new：构造 session → spawn actor → 返回 session_id。
///
/// - 反序列化失败 / engine 构造失败 → 400（带原因文案）。
pub async fn api_new(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<NewSessionBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let subject = match request_subject(&state, authorization_token(&headers)) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let seed = match parse_host_parity_generation(&body.seed) {
        Ok(seed) => seed,
        Err(_) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_SEED",
                "seed 必须是规范十进制 u64 字符串",
            );
        }
    };
    match state
        .manager
        .new_shared_session(body.setup, seed, subject.clone())
    {
        Ok((_, false)) => api_error(
            StatusCode::CONFLICT,
            "MARKET_ALREADY_EXISTS",
            "Server 已有共享市场，请查看市场列表并明确加入，不能为另一个主体另开市场",
        ),
        Ok((id, true)) => {
            info!(session = %id, "new session created");
            let Some(handles) = state.manager.lookup(&id) else {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ACTOR_GONE",
                    "session actor was not registered",
                );
            };
            market_context_response(id, handles, subject).await
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

fn request_subject(
    state: &AppState,
    token: Option<&str>,
) -> Result<engine::session::OpaqueSubjectId, Box<Response>> {
    let token = token.ok_or_else(|| {
        Box::new(api_error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "缺少登录凭据，请先登录",
        ))
    })?;
    let identity = crate::identity::IdentityService::new(state.manager.database())
        .authenticate(token)
        .map_err(|error| {
            let status = if matches!(
                error,
                crate::identity::IdentityError::Storage
                    | crate::identity::IdentityError::Cryptography
            ) {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::UNAUTHORIZED
            };
            Box::new(api_error(
                status,
                "IDENTITY_AUTHENTICATION_FAILED",
                error.to_string(),
            ))
        })?;
    engine::session::OpaqueSubjectId::new(identity.subject_id).map_err(|_| {
        Box::new(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IDENTITY_INVALID",
            "登录主体异常，请反馈 Server.identity",
        ))
    })
}

async fn market_context_response(
    id: String,
    handles: Arc<crate::actor::SessionHandles>,
    subject: engine::session::OpaqueSubjectId,
) -> Response {
    let context = match handles.market_context_for(subject).await {
        Ok(context) => context,
        Err(error) => {
            return api_error(
                StatusCode::FORBIDDEN,
                "MARKET_CONTEXT_FAILED",
                error.to_string(),
            )
        }
    };
    Json(NewSessionResp {
        session_id: id,
        setup: context.setup,
        seed: context.seed,
        resumed: handles.startup_resumed,
        generation: context.generation,
        member: context.member,
        can_control: context.can_control,
        needs_rejoin: context.needs_rejoin,
    })
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinMarketBody {
    pub session_id: String,
    pub generation: String,
    pub confirmed_rejoin: bool,
}

pub async fn api_markets(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match request_subject(&state, authorization_token(&headers)) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let mut markets = Vec::new();
    for id in state.manager.market_ids() {
        let Some(handles) = state.manager.lookup(&id) else {
            continue;
        };
        let context = match handles.market_context_for(subject.clone()).await {
            Ok(context) => context,
            Err(error) => {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "MARKET_CONTEXT_FAILED",
                    error.to_string(),
                )
            }
        };
        markets.push(serde_json::json!({"session_id":id,"setup":context.setup,"seed":context.seed,"resumed":handles.startup_resumed,"generation":context.generation,"member":context.member,"can_control":context.can_control,"needs_rejoin":context.needs_rejoin}));
    }
    Json(serde_json::json!({"markets":markets})).into_response()
}

pub async fn api_market_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SessionQuery>,
) -> Response {
    let (handles, subject) =
        match authorized_market(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    market_context_response(query.session_id, handles, subject).await
}

pub async fn api_join_market(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<JoinMarketBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let subject = match request_subject(&state, authorization_token(&headers)) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return *session_forbidden();
    };
    match handles
        .join_member_for(generation, subject.clone(), body.confirmed_rejoin)
        .await
    {
        Ok(_) => market_context_response(body.session_id, handles, subject).await,
        Err(error) => api_error(
            StatusCode::CONFLICT,
            "MARKET_JOIN_REJECTED",
            error.to_string(),
        ),
    }
}

pub async fn api_public_report_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(company_id): Path<String>,
    Query(params): Query<PublicReportQueryParams>,
) -> Response {
    let (handles, subject) =
        match authorized_market(&state, &params.session_id, authorization_token(&headers)).await {
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

pub async fn api_public_report_availability(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(company_id): Path<String>,
    Query(params): Query<PublicReportAvailabilityParams>,
    Json(query): Json<engine::company::PublicReportAvailabilityQuery>,
) -> Response {
    if query.company_id != company_id {
        return api_error(StatusCode::BAD_REQUEST, "COMPANY_QUERY_MISMATCH", "company_id path 与请求内容不一致");
    }
    let (handles, _) = match authorized_market(&state, &params.session_id, authorization_token(&headers)).await {
        Ok(handles) => handles,
        Err(response) => return *response,
    };
    match handles.public_report_availability(query).await {
        Ok(availability) => (StatusCode::OK, Json(availability)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "session actor gone"),
        Err(SendCommandError::Rejected(reason)) => public_query_error(reason),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("availability query cannot validate speed"),
    }
}

pub async fn api_public_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((company_id, report_id)): Path<(String, String)>,
    Query(params): Query<PublicReportQueryParams>,
) -> Response {
    let (handles, subject) =
        match authorized_market(&state, &params.session_id, authorization_token(&headers)).await {
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
    if !cfg!(all(feature = "simulation-diagnostics", debug_assertions)) {
        return api_error(
            StatusCode::NOT_FOUND,
            "DIAGNOSTICS_DISABLED",
            "NPC 决策诊断仅在启用开发诊断的非 release 服务端可用",
        );
    }
    let (handles, subject) = match authorized_market_controller(
        &state,
        &params.session_id,
        authorization_token(&headers),
    )
    .await
    {
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
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "DIAGNOSTICS_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("diagnostic query cannot validate speed")
        }
    }
}

pub async fn api_host_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SessionQuery>,
) -> Response {
    let subject = match request_subject(&state, authorization_token(&headers)) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let Some(handles) = state.manager.lookup(&query.session_id) else {
        return *session_forbidden();
    };
    match handles.market_context_for(subject).await {
        Ok(context) => {
            (StatusCode::OK, Json(serde_json::json!({
        "npcDecisionDiagnostics": cfg!(all(feature = "simulation-diagnostics", debug_assertions)),
        "indicatorCapabilities": {
            "intradayAverage": true,
            "macd": true,
            "priceKdj": true,
            "candleKdj": true
        },
        "personalTradeHistory": true,
        "marketControl": context.can_control,
        "ownAccountTrading": context.member.is_some(),
        "persistence": "remote",
    }))).into_response()
        }
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "MARKET_CONTEXT_FAILED", error.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionCashBody {
    pub session_id: String,
    pub generation: String,
    pub admission_cash: engine::Money,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetMarketBody {
    pub session_id: String,
    pub generation: String,
    pub setup: engine::SessionSetup,
    pub seed: String,
}

pub async fn api_reset_market(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ResetMarketBody>, JsonRejection>,
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
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    let seed = match parse_host_parity_generation(&body.seed) {
        Ok(seed) => seed,
        Err(_) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_SEED",
                "seed 必须是规范十进制 u64 字符串",
            )
        }
    };
    match handles
        .reset_for(generation, subject.clone(), body.setup, seed)
        .await
    {
        Ok(_) => market_context_response(body.session_id, handles, subject).await,
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "MARKET_RESET_REJECTED",
            error.to_string(),
        ),
    }
}

pub async fn api_admission_cash(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<AdmissionCashBody>, JsonRejection>,
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
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .set_admission_cash_for(generation, subject, body.admission_cash)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "ADMISSION_CASH_REJECTED",
            error.to_string(),
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CalculateIndicatorsBody {
    session_id: String,
    prices: Vec<f64>,
    #[serde(default)]
    candles: Vec<engine::indicators::OhlcBar>,
}

pub(crate) async fn api_calculate_indicators(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CalculateIndicatorsBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    if let Err(response) =
        authorized_market(&state, &body.session_id, authorization_token(&headers)).await
    {
        return *response;
    }
    match engine::indicators::calculate_indicators(&body.prices, &body.candles) {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INDICATOR_INPUT",
            error.to_string(),
        ),
    }
}

pub async fn api_calculate_intraday_average(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CalculateIntradayAverageBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market(&state, &body.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let turnover_cents = match engine::parse_turnover_cents(&body.turnover_cents) {
        Ok(value) => value,
        _ => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_INTRADAY_AVERAGE",
                "turnover_cents 必须是规范非负分字符串",
            )
        }
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let result = handles
        .calculate_intraday_average(
            generation,
            engine::session::DailyTradeStats {
                turnover_cents,
                trade_count: body.trade_count,
            },
            body.volume_shares,
        )
        .await;
    match result {
        Ok((_generation, Some(average))) => (StatusCode::OK, Json(average)).into_response(),
        Ok((_generation, None)) => (StatusCode::OK, Json(serde_json::Value::Null)).into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "INVALID_INTRADAY_AVERAGE", reason)
        }
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("VWAP query cannot validate speed"),
    }
}

pub async fn api_calculate_intraday_average_curve(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CalculateIntradayAverageCurveBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    if body.series_key.is_empty() || body.series_key.len() > 256 || body.samples.len() > 600 {
        return api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INTRADAY_AVERAGE",
            "VWAP curve series_key 或样本数无效",
        );
    }
    let (handles, subject) =
        match authorized_market(&state, &body.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let results = match handles
        .calculate_intraday_average_curve(generation, body.samples)
        .await
    {
        Ok((actual_generation, values)) if actual_generation == generation => values,
        Ok(_) => {
            return api_error(
                StatusCode::CONFLICT,
                "STALE_SESSION_GENERATION",
                "VWAP curve generation changed during calculation",
            )
        }
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            return api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => {
            return api_error(StatusCode::BAD_REQUEST, "INVALID_INTRADAY_AVERAGE", reason)
        }
        Err(SendCommandError::ActorGone) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ACTOR_GONE",
                "session actor gone",
            )
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("VWAP curve query cannot validate speed")
        }
    };
    (StatusCode::OK, Json(serde_json::json!({ "generation": body.generation.to_string(), "series_key": body.series_key, "results": results }))).into_response()
}

pub async fn api_personal_trade_confirmations(
    State(state): State<AppState>,
    Query(query): Query<PersonalTradeConfirmationsQuery>,
    headers: HeaderMap,
) -> Response {
    let (handles, subject) =
        match authorized_session(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let before_receipt = match query
        .before_receipt
        .map(|value| {
            if value.is_empty()
                || value.len() > 20
                || !value.bytes().all(|byte| byte.is_ascii_digit())
                || (value.len() > 1 && value.starts_with('0'))
            {
                return Err("交割单 before_receipt 必须是规范u64字符串".to_owned());
            }
            value.parse::<u64>().map_err(|error| error.to_string())
        })
        .transpose()
    {
        Ok(value) => value,
        Err(error) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "PERSONAL_TRADE_HISTORY_REJECTED",
                error,
            )
        }
    };
    match handles.confirmations_for(query.generation, subject.clone(), before_receipt).await {
        Ok((generation, confirmations)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "confirmations": confirmations })),
        ).into_response(),
        Err(SendCommandError::Rejected(reason)) if reason.starts_with("STALE_SESSION_GENERATION:") => {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "PERSONAL_TRADE_HISTORY_REJECTED", reason),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "session actor gone"),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("trade history query cannot validate speed"),
    }
}

pub async fn api_personal_trade_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<PersonalTradeHistoryBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body { Ok(value) => value, Err(error) => return invalid_json_response(error) };
    let (handles, subject) = match authorized_session(&state, &body.session_id, authorization_token(&headers)).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match handles.trade_history_for(generation, subject, body.query).await {
        Ok((generation, page)) => (StatusCode::OK, Json(serde_json::json!({ "generation": generation.to_string(), "page": page }))).into_response(),
        Err(SendCommandError::Rejected(reason)) if reason.starts_with("STALE_SESSION_GENERATION:") => api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason),
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "PERSONAL_TRADE_HISTORY_REJECTED", reason),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "session actor gone"),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("本人日期查询不设置speed"),
    }
}

pub async fn api_market_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<MarketHistoryBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body { Ok(value) => value, Err(error) => return invalid_json_response(error) };
    let (handles, subject) = match authorized_market(&state, &body.session_id, authorization_token(&headers)).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) { Ok(value) => value, Err(response) => return *response };
    match handles.market_history_for(generation, subject, body.query).await {
        Ok((generation, page)) => (StatusCode::OK, Json(serde_json::json!({ "generation": generation.to_string(), "page": page }))).into_response(),
        Err(SendCommandError::Rejected(reason)) if reason.starts_with("STALE_SESSION_GENERATION:") => api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason),
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "MARKET_HISTORY_REJECTED", reason),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "session actor gone"),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("公开量价查询不设置speed"),
    }
}

pub async fn api_current_minute_history(State(state): State<AppState>, headers: HeaderMap, body: Result<Json<CurrentMinuteHistoryBody>, JsonRejection>) -> Response {
    let Json(body) = match body { Ok(value) => value, Err(error) => return invalid_json_response(error) };
    let (handles, subject) = match authorized_market(&state, &body.session_id, authorization_token(&headers)).await { Ok(value) => value, Err(response) => return *response };
    let generation = match parse_host_parity_generation(&body.generation) { Ok(value) => value, Err(response) => return *response };
    match handles.current_minute_history_for(generation, subject, body.query).await {
        Ok((generation, response)) => (StatusCode::OK, Json(serde_json::json!({ "generation": generation.to_string(), "response": response }))).into_response(),
        Err(SendCommandError::Rejected(reason)) if reason.starts_with("STALE_SESSION_GENERATION:") => api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason),
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "CURRENT_MINUTE_HISTORY_REJECTED", reason),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "session actor gone"),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("当前分钟查询不设置speed"),
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

/// POST /api/intent：从认证主体解析本人 AccountId 后入队意图。
///
/// - 缺 token → 401；未知 session 或错误 token → 403；engine 拒绝/actor 关闭 → 400/500。
pub async fn api_intent(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<IntentBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market(&state, &body.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .enqueue_for(generation, subject.clone(), body.intent)
        .await
    {
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
/// - 缺 token → 401；未知 session 或错误 token → 403；成功 → 200 JSON Snapshot。
pub async fn api_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionQuery>,
) -> Response {
    let (handles, subject) =
        match authorized_session(&state, &q.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    match handles.snapshot_for(subject.clone()).await {
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

/// GET /api/player-working-orders：只读查询当前玩家的真实活动委托，不生成存档。
pub async fn api_player_working_orders(
    State(state): State<AppState>,
    Query(query): Query<PlayerWorkingOrdersQuery>,
    headers: HeaderMap,
) -> Response {
    let (handles, subject) =
        match authorized_session(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    match handles
        .working_orders_for(query.generation, subject.clone())
        .await
    {
        Ok((generation, orders)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "orders": orders })),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "WORKING_ORDERS_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("order query cannot validate speed"),
    }
}

/// GET /api/stock-history：由玩家显式查询单只股票历史，不参与 baseline 或快照生成。
pub async fn api_stock_history(
    State(state): State<AppState>,
    Query(query): Query<StockHistoryQuery>,
    headers: HeaderMap,
) -> Response {
    let (handles, subject) =
        match authorized_session(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    match handles
        .history_for(query.generation, subject.clone(), query.code)
        .await
    {
        Ok((generation, data)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "data": data })),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "STOCK_HISTORY_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("stock history query cannot validate speed")
        }
    }
}

pub async fn api_initial_allocation(
    State(state): State<AppState>,
    Query(query): Query<InitialAllocationQuery>,
    headers: HeaderMap,
) -> Response {
    let (handles, subject) =
        match authorized_market(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    match handles.initial_allocation(query.generation).await {
        Ok((generation, data)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "data": data })),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) => api_error(
            StatusCode::BAD_REQUEST,
            "INITIAL_ALLOCATION_REJECTED",
            reason,
        ),
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("initial allocation query cannot validate speed")
        }
    }
}

/// POST /api/save：在 actor 内取得一致日终存档，完整市场事实只允许独立控制能力持有者导出。
/// 缺 token 返回 401；未知 session 或错误 token 返回 403。
pub async fn api_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<SaveRequest>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.save(generation, body.candidate).await {
        Ok(slot) => (StatusCode::OK, Json(slot)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason)) if reason.contains("STALE_SESSION_GENERATION:") => {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "SAVE_REJECTED", reason)
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("save cannot validate speed"),
    }
}

/// POST /api/load：完整校验通过后原子替换 actor 会话，失败保留原状态。
/// 缺 token 返回 401；未知 session 或错误 token 返回 403。
pub async fn api_load(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
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
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
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
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .restore_archive_for(generation, subject.clone(), slot, body.archive_slot_id)
        .await
    {
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
/// - 缺 token → 401；未知 session 或错误 token → 403；非法 speed → 400。
pub async fn api_speed(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<SpeedBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let speed = match body.speed.multiplier() {
        Ok(speed) => speed,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SPEED", message),
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.set_speed_for(generation, subject.clone(), speed).await {
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
    headers: HeaderMap,
    Query(q): Query<SessionQuery>,
) -> Response {
    let (handles, subject) =
        match authorized_market(&state, &q.session_id, authorization_token(&headers)).await {
            Ok(handles) => handles,
            Err(response) => return *response,
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
/// 缺 token 返回 401；未知 session 或错误 token 返回 403。
pub async fn api_running(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<RunningBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .set_running_for(generation, subject.clone(), body.running)
        .await
    {
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
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
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
    let (handles, subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
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

/// DELETE /api/session：授权后从 manager 移除并确认 actor 已停止。
/// 缺 token 返回 401；未知 session 或错误 token 返回 403。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteSessionQuery {
    pub session_id: String,
    pub generation: String,
}

pub async fn api_delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeleteSessionQuery>,
) -> Response {
    let (handles, subject) =
        match authorized_market_controller(&state, &q.session_id, authorization_token(&headers))
            .await
        {
            Ok(handles) => handles,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&q.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.shutdown_for(generation, subject).await {
        Ok(()) => {
            state.manager.remove(&q.session_id);
            StatusCode::NO_CONTENT.into_response()
        }
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
/// - 缺 token → 401；未知 session 或错误 token → 403。
/// - 浏览器凭据通过 auth subprotocol 发送，只协商公开 stock-game，不回显凭据。
/// - 心跳：~30s 后端发 Ping；10s 内未收到匹配 Pong 则关闭连接。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let token = websocket_token(&headers);
    let (handles, subject) = match authorized_market(&state, &q.session_id, token.as_deref()).await
    {
        Ok(handles) => handles,
        Err(response) => return *response,
    };
    let identities = crate::identity::IdentityService::new(state.manager.database());
    let Some(token) = token else {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "缺少登录凭据");
    };
    ws.protocols(["stock-game"])
        .on_upgrade(move |socket| run_ws(socket, handles, q.delivery, subject, identities, token))
}

#[derive(Default)]
struct WsHeartbeat {
    sequence: u64,
    outstanding: Option<(Vec<u8>, tokio::time::Instant)>,
}

impl WsHeartbeat {
    fn ping(&mut self, now: tokio::time::Instant) -> Option<Vec<u8>> {
        if self.outstanding.is_some() {
            return None;
        }
        self.sequence = self.sequence.wrapping_add(1);
        let payload = self.sequence.to_be_bytes().to_vec();
        self.outstanding = Some((payload.clone(), now + Duration::from_secs(10)));
        Some(payload)
    }

    fn pong(&mut self, payload: &[u8]) {
        if self
            .outstanding
            .as_ref()
            .is_some_and(|(expected, deadline)| {
                expected == payload && tokio::time::Instant::now() < *deadline
            })
        {
            self.outstanding = None;
        }
    }

    fn deadline(&self) -> Option<tokio::time::Instant> {
        self.outstanding.as_ref().map(|(_, deadline)| *deadline)
    }
}

#[cfg(test)]
mod heartbeat_tests {
    use super::*;

    #[test]
    fn matching_pong_clears_only_the_current_ping_deadline() {
        let now = tokio::time::Instant::now();
        let mut heartbeat = WsHeartbeat::default();
        let first = heartbeat.ping(now).unwrap();
        assert_eq!(heartbeat.deadline(), Some(now + Duration::from_secs(10)));
        heartbeat.pong(b"wrong");
        assert!(heartbeat.deadline().is_some());
        heartbeat.pong(&first);
        assert!(heartbeat.deadline().is_none());
        let second = heartbeat.ping(now + Duration::from_secs(30)).unwrap();
        heartbeat.pong(&first);
        assert_eq!(heartbeat.deadline(), Some(now + Duration::from_secs(40)));
        heartbeat.pong(&second);
        assert!(heartbeat.deadline().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn pong_deadline_cancels_a_connection_blocked_inside_an_await() {
        struct DropMarker(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker = DropMarker(dropped.clone());
        let connection = async move {
            let _marker = marker;
            std::future::pending::<()>().await;
        };
        let started = tokio::time::Instant::now();
        let (_sender, mut receiver) =
            tokio::sync::watch::channel(Some(started + Duration::from_secs(10)));
        supervise_ws_connection(connection, &mut receiver).await;
        assert_eq!(
            tokio::time::Instant::now() - started,
            Duration::from_secs(10)
        );
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn a_pong_arriving_at_the_deadline_cannot_reopen_the_connection() {
        let mut heartbeat = WsHeartbeat::default();
        let payload = heartbeat.ping(tokio::time::Instant::now()).unwrap();
        let deadline = heartbeat.deadline();
        tokio::time::advance(Duration::from_secs(10)).await;
        heartbeat.pong(&payload);
        assert_eq!(heartbeat.deadline(), deadline);
    }

    #[tokio::test(start_paused = true)]
    async fn an_expired_deadline_prevents_connection_poll_from_renewing_it() {
        let polled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = polled.clone();
        let (sender, mut receiver) = tokio::sync::watch::channel(Some(tokio::time::Instant::now()));
        let connection = async move {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            sender.send_replace(Some(tokio::time::Instant::now() + Duration::from_secs(30)));
            std::future::pending::<()>().await;
        };
        supervise_ws_connection(connection, &mut receiver).await;
        assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn an_outstanding_ping_cannot_be_replaced_after_runtime_delay() {
        let mut heartbeat = WsHeartbeat::default();
        heartbeat.ping(tokio::time::Instant::now()).unwrap();
        let deadline = heartbeat.deadline();
        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(heartbeat.ping(tokio::time::Instant::now()).is_none());
        assert_eq!(heartbeat.deadline(), deadline);
    }
}

#[cfg(test)]
mod public_report_availability_route_tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn rejects_company_path_mismatch_before_session_lookup() {
        let app = crate::app_router();
        let response = app.oneshot(Request::builder()
            .method("POST")
            .uri("/api/companies/C-path/reports/availability?session_id=unused")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"company_id":"C-body","period_end":"2030-03-31","kind":"Quarter","scope":{"Standalone":{"entity_id":"C-body"}}}"#))
            .unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "COMPANY_QUERY_MISMATCH");
    }
}

/// 每条 WS 连接独占的 publisher 协议状态；socket I/O 仍由 run_ws 执行。
struct WsPublisherConnection {
    delivery: DeliveryMode,
    baseline_seq: u64,
    baseline_tick: u64,
    timeline_generation: u64,
    publisher: ClientFrameBuffer,
    awaiting_resync: bool,
    delivered_failure: Option<crate::actor::HostFailure>,
}

enum PublisherAction {
    Buffered,
    Ignored,
    HostFailure(Box<crate::actor::HostFailure>),
    ResyncRequired {
        reason: &'static str,
        capacity: Option<usize>,
    },
    Flush {
        update: Box<crate::actor::EngineUpdate>,
        all: bool,
    },
    Rejected(FrameBufferError),
}

enum FrameRequest {
    ResyncRequired,
    WrongDeliveryMode,
    Frame(Box<PublisherFrame>),
    Empty,
}

impl WsPublisherConnection {
    fn new(
        delivery: DeliveryMode,
        cursor: (u64, u64, u64),
        failure: Option<crate::actor::HostFailure>,
        ticks_per_day: u64,
        auction_ticks: u64,
    ) -> Result<Self, FrameBufferError> {
        Ok(Self {
            delivery,
            baseline_seq: cursor.0,
            baseline_tick: cursor.1,
            timeline_generation: cursor.2,
            publisher: ClientFrameBuffer::new(ticks_per_day, auction_ticks)?,
            awaiting_resync: failure.is_some(),
            delivered_failure: failure,
        })
    }

    fn ingest(&mut self, update: crate::actor::EngineUpdate) -> PublisherAction {
        if let Some(failure) = &update.failure {
            if self.delivered_failure.as_ref() == Some(failure) {
                self.awaiting_resync = true;
                return PublisherAction::Ignored;
            }
            return PublisherAction::HostFailure(Box::new(failure.clone()));
        }
        if update.timeline_generation != self.timeline_generation {
            self.require_resync();
            return PublisherAction::ResyncRequired {
                reason: "timeline_changed",
                capacity: None,
            };
        }
        if self.awaiting_resync {
            return PublisherAction::Ignored;
        }
        let Some(protocol) = &update.update else {
            return PublisherAction::Ignored;
        };
        let covered = match protocol {
            engine::session::protocol::EngineUpdate::TickBatch(batch) => {
                batch.frames.last().is_some_and(|frame| {
                    frame.tick <= self.baseline_tick && frame.seq_to <= self.baseline_seq
                })
            }
            engine::session::protocol::EngineUpdate::CivilUpdate(civil) => {
                civil.tick <= self.baseline_tick && civil.seq_to <= self.baseline_seq
            }
        };
        if covered {
            return PublisherAction::Ignored;
        }
        match self.publisher.push(update.clone()) {
            Ok(()) => PublisherAction::Buffered,
            Err(error) => self.buffer_error(update, error),
        }
    }

    fn buffer_error(
        &mut self,
        update: crate::actor::EngineUpdate,
        error: FrameBufferError,
    ) -> PublisherAction {
        match error {
            FrameBufferError::MetadataTransition => PublisherAction::Flush {
                update: Box::new(update),
                all: false,
            },
            FrameBufferError::BufferCapacityExceeded { limit }
                if self.delivery == DeliveryMode::Pull =>
            {
                self.require_resync();
                PublisherAction::ResyncRequired {
                    reason: "publisher_buffer_capacity",
                    capacity: Some(limit),
                }
            }
            FrameBufferError::BufferCapacityExceeded { .. } => PublisherAction::Flush {
                update: Box::new(update),
                all: true,
            },
            other => PublisherAction::Rejected(other),
        }
    }

    fn failure_delivered(&mut self, failure: crate::actor::HostFailure) {
        self.delivered_failure = Some(failure);
        self.awaiting_resync = true;
    }

    fn require_resync(&mut self) {
        self.publisher.clear();
        self.awaiting_resync = true;
    }

    fn prepare_resync(&mut self) {
        self.publisher.clear();
    }

    fn install_baseline_cursor(&mut self, baseline: &crate::actor::PublicBaseline) {
        self.baseline_seq = baseline.snapshot.seq;
        self.baseline_tick = baseline.snapshot.tick;
        self.timeline_generation = baseline.timeline_generation;
    }

    fn baseline_delivered(&mut self, failure: Option<crate::actor::HostFailure>) {
        self.awaiting_resync = failure.is_some();
        self.delivered_failure = failure;
    }

    fn push_ready(&self) -> bool {
        self.delivery == DeliveryMode::Push && !self.awaiting_resync
    }

    fn take_push_frame(&mut self) -> Option<PublisherFrame> {
        if self.push_ready() {
            self.publisher.take()
        } else {
            None
        }
    }

    fn take_flush_frame(&mut self) -> Option<PublisherFrame> {
        self.publisher.take()
    }

    fn accept_after_flush(
        &mut self,
        update: crate::actor::EngineUpdate,
    ) -> Result<(), FrameBufferError> {
        self.publisher.push(update)
    }

    fn request_frame(&mut self) -> FrameRequest {
        if self.awaiting_resync {
            return FrameRequest::ResyncRequired;
        }
        if self.delivery != DeliveryMode::Pull {
            return FrameRequest::WrongDeliveryMode;
        }
        match self.publisher.take() {
            Some(frame) => FrameRequest::Frame(Box::new(frame)),
            None => FrameRequest::Empty,
        }
    }
}

/// WS 连接主循环。
///
/// 1. 先经 actor 取完整 Snapshot，序列化 JSON 发给客户端（对齐基线）。
/// 2. 订阅更新 broadcast，把 Event[] + 可选权威运行快照作为一个 JSON 帧推出。
/// 3. 同时读客户端消息（仅作存活/pong 探测；当前不处理客户端业务消息）。
/// 4. 30s 心跳：发 Ping。
async fn run_ws(
    socket: axum::extract::ws::WebSocket,
    handles: Arc<crate::actor::SessionHandles>,
    delivery: DeliveryMode,
    subject: engine::session::OpaqueSubjectId,
    identities: crate::identity::IdentityService,
    token: String,
) {
    let (deadline_sender, mut deadline_receiver) = tokio::sync::watch::channel(None);
    let connection = run_ws_connection(
        socket,
        handles,
        delivery,
        deadline_sender,
        subject,
        identities,
        token,
    );
    supervise_ws_connection(connection, &mut deadline_receiver).await;
}

async fn supervise_ws_connection(
    connection: impl std::future::Future<Output = ()>,
    deadline_receiver: &mut tokio::sync::watch::Receiver<Option<tokio::time::Instant>>,
) {
    tokio::pin!(connection);
    loop {
        let deadline = *deadline_receiver.borrow_and_update();
        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            debug!("ws: expired pong deadline; refusing to poll connection");
            break;
        }
        tokio::select! {
            _ = &mut connection => break,
            changed = deadline_receiver.changed() => {
                if changed.is_err() { break; }
            }
            _ = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                debug!("ws: pong deadline expired; dropping connection transport");
                break;
            }
        }
    }
}

async fn run_ws_connection(
    socket: axum::extract::ws::WebSocket,
    handles: Arc<crate::actor::SessionHandles>,
    delivery: DeliveryMode,
    deadline_sender: tokio::sync::watch::Sender<Option<tokio::time::Instant>>,
    subject: engine::session::OpaqueSubjectId,
    identities: crate::identity::IdentityService,
    token: String,
) {
    let (mut sender, mut receiver) = socket.split();

    // 先订阅再取基线，消除 snapshot 与 subscribe 之间丢事件的竞态；基线 seq 之前的
    // 缓冲事件在后续读取时跳过。
    let mut rx = handles.subscribe_events();
    let initial_baseline;
    let mut account;

    // 1. 对齐基线：发完整 Snapshot JSON。
    match handles.public_baseline_for(subject.clone()).await {
        Ok(baseline) => {
            account = baseline.snapshot.accounts.keys().next().copied();
            initial_baseline = (
                (
                    baseline.snapshot.seq,
                    baseline.snapshot.tick,
                    baseline.timeline_generation,
                ),
                baseline.failure.clone(),
            );
            match send_baseline(&mut sender, baseline, &identities, &token).await {
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
        Err(SendCommandError::Rejected(reason)) => {
            let _ =
                send_gateway_error(&mut sender, None, "MARKET_MEMBERSHIP_REQUIRED", reason).await;
            return;
        }
        Err(SendCommandError::InvalidSpeed(_)) => {
            unreachable!("public baseline cannot validate speed")
        }
    }

    // 3/4. 心跳 interval + 事件/消息 select。
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = heartbeat.tick().await; // 跳过首个立即到期。
    let mut heartbeat_state = WsHeartbeat::default();
    let mut connection = match WsPublisherConnection::new(
        delivery,
        initial_baseline.0,
        initial_baseline.1,
        handles.ticks_per_day,
        handles.auction_ticks,
    ) {
        Ok(buffer) => buffer,
        Err(error) => {
            error!(%error, "ws: invalid publisher configuration");
            return;
        }
    };
    let mut push_clock = tokio::time::interval(CLIENT_PUSH_INTERVAL);
    push_clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = push_clock.tick().await;

    loop {
        tokio::select! {
            // 事件到达 → 推 JSON。
            ev = rx.recv() => {
                match ev {
                    Ok(update) => {
                        if let Err(error) = identities.authenticate(&token) {
                            let _ = send_gateway_error(&mut sender, None, "IDENTITY_AUTHENTICATION_FAILED", error.to_string()).await;
                            break;
                        }
                        if update.timeline_generation != connection.timeline_generation {
                            connection.require_resync();
                            if !send_resync_required(&mut sender, "timeline_changed", None).await { break; }
                            continue;
                        }
                        match connection.ingest(update.for_member_account(account)) {
                            PublisherAction::Buffered | PublisherAction::Ignored => {}
                            PublisherAction::HostFailure(failure) => {
                                if !send_host_failure(&mut sender, (*failure).clone()).await { break; }
                                connection.failure_delivered(*failure);
                            }
                            PublisherAction::ResyncRequired { reason, capacity } => {
                                if let Some(limit) = capacity {
                                    warn!(limit, "ws: pull publisher buffer full; client must re-sync");
                                }
                                if !send_resync_required(&mut sender, reason, None).await { break; }
                            }
                            PublisherAction::Flush { update, all } => {
                                let mut delivered = true;
                                while let Some(frame) = connection.take_flush_frame() {
                                    if !send_publisher_frame(&mut sender, frame, &identities, &token).await {
                                        delivered = false;
                                        break;
                                    }
                                    if !all { break; }
                                }
                                if !delivered { break; }
                                if let Err(error) = connection.accept_after_flush(*update) {
                                    if all {
                                        error!(%error, "ws: publisher rejected update after flushing backlog");
                                    } else {
                                        error!(%error, "ws: publisher rejected metadata segment");
                                    }
                                    break;
                                }
                            }
                            PublisherAction::Rejected(error) => {
                                error!(%error, "ws: publisher rejected engine update");
                                if !send_gateway_error(&mut sender, None, "PUBLISHER_SEQUENCE_ERROR", error.to_string()).await { break; }
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // 慢消费者必须立即重新拉快照；显式协议消息避免客户端只看到 seq 缺口。
                        warn!(missed = n, "ws: lagged, client should re-sync via snapshot");
                        connection.require_resync();
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
            _ = push_clock.tick(), if connection.push_ready() => {
                if let Some(frame) = connection.take_push_frame() {
                    if !send_publisher_frame(&mut sender, frame, &identities, &token).await {
                        break;
                    }
                }
            }
            // 心跳：30s 发 Ping（防中间设备杀空闲连接）。
            _ = heartbeat.tick() => {
                if let Err(error) = identities.authenticate(&token) {
                    let _ = send_gateway_error(&mut sender, None, "IDENTITY_AUTHENTICATION_FAILED", error.to_string()).await;
                    break;
                }
                let Some(payload) = heartbeat_state.ping(tokio::time::Instant::now()) else {
                    debug!("ws: previous ping remains unanswered; closing");
                    break;
                };
                deadline_sender.send_replace(heartbeat_state.deadline());
                if sender.send(axum::extract::ws::Message::Ping(payload)).await.is_err() {
                    debug!("ws: heartbeat ping failed; closing");
                    break;
                }
            }
            // 读客户端消息（Pong/Close/其它）：仅作存活探测与礼貌关闭，不解析业务消息。
            msg = receiver.next() => {
                match msg {
                    Some(Ok(m)) => {
                        if let axum::extract::ws::Message::Pong(payload) = &m {
                            heartbeat_state.pong(payload);
                            deadline_sender.send_replace(heartbeat_state.deadline());
                        }
                        if matches!(m, axum::extract::ws::Message::Close(_)) {
                            debug!("ws: client sent close");
                            break;
                        }
                        if let axum::extract::ws::Message::Text(text) = m {
                            if let Err(error) = identities.authenticate(&token) {
                                let _ = send_gateway_error(&mut sender, None, "IDENTITY_AUTHENTICATION_FAILED", error.to_string()).await;
                                break;
                            }
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
                                    rx = handles.subscribe_events();
                                    connection.prepare_resync();
                                    match handles.public_baseline_for(subject.clone()).await {
                                        Ok(baseline) => {
                                            account = baseline.snapshot.accounts.keys().next().copied();
                                            connection.install_baseline_cursor(&baseline);
                                            let latched_failure = baseline.failure.clone();
                                            if !send_baseline(&mut sender, baseline, &identities, &token).await { break; }
                                            connection.baseline_delivered(latched_failure);
                                        }
                                        Err(error) => {
                                            if !send_gateway_error(&mut sender, None, "RESYNC_FAILED", error.to_string()).await { break; }
                                        }
                                    }
                                }
                                ClientCommand::GetFrame {} => {
                                    match connection.request_frame() {
                                        FrameRequest::ResyncRequired => {
                                            if !send_gateway_error(&mut sender, None, "RESYNC_REQUIRED", "send Resync before requesting frames").await { break; }
                                        }
                                        FrameRequest::WrongDeliveryMode => {
                                            if !send_gateway_error(&mut sender, None, "WRONG_DELIVERY_MODE", "GetFrame 只允许用于 pull 模式").await { break; }
                                        }
                                        FrameRequest::Frame(frame) => {
                                            if !send_publisher_frame(&mut sender, *frame, &identities, &token).await { break; }
                                        }
                                        FrameRequest::Empty => {
                                            let empty = serde_json::json!({ "FrameEmpty": {} }).to_string();
                                            if sender.send(axum::extract::ws::Message::Text(empty)).await.is_err() { break; }
                                        }
                                    }
                                }
                                ClientCommand::SubmitIntent { request_id, generation, intent } => {
                                    let generation = match parse_host_parity_generation(&generation) {
                                        Ok(generation) => generation,
                                        Err(_) => {
                                            if !send_gateway_error(&mut sender, Some(request_id), "INVALID_CLIENT_COMMAND", "generation 必须是 canonical decimal u64").await { break; }
                                            continue;
                                        }
                                    };
                                    match handles.enqueue_for(generation, subject.clone(), intent).await {
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
    identities: &crate::identity::IdentityService,
    token: &str,
) -> bool {
    match authenticated_frame_json(frame, identities, token) {
        Ok(json) => {
            if let Err(error) = identities.authenticate(token) {
                let _ =
                    send_gateway_error(sender, None, "FRAME_DELIVERY_REJECTED", error.to_string())
                        .await;
                return false;
            }
            sender
                .send(axum::extract::ws::Message::Text(json))
                .await
                .is_ok()
        }
        Err(error) => {
            let _ = send_gateway_error(sender, None, "FRAME_DELIVERY_REJECTED", error).await;
            false
        }
    }
}

fn authenticated_frame_json(
    frame: PublisherFrame,
    identities: &crate::identity::IdentityService,
    token: &str,
) -> Result<String, String> {
    identities
        .authenticate(token)
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&serde_json::json!({ "PublisherFrame": frame }))
        .map_err(|error| error.to_string())
}

async fn send_baseline(
    sender: &mut futures_util::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    baseline: crate::actor::PublicBaseline,
    identities: &crate::identity::IdentityService,
    token: &str,
) -> bool {
    match baseline_wire_messages(baseline) {
        Ok(messages) => {
            for message in messages {
                if let Err(error) = identities.authenticate(token) {
                    let _ = send_gateway_error(
                        sender,
                        None,
                        "BASELINE_DELIVERY_REJECTED",
                        error.to_string(),
                    )
                    .await;
                    return false;
                }
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
            failure: Some(HostFailure::step(
                &engine::session::StepFatal::InvariantViolation {
                    location: "server.step".into(),
                    description: "receipt chain broke".into(),
                },
            )),
        })
        .unwrap();

        assert_eq!(messages.len(), 2);
        let baseline: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert!(baseline["Baseline"].get("failure").is_none());
        let failure: serde_json::Value = serde_json::from_str(&messages[1]).unwrap();
        assert_eq!(failure["HostFailure"]["code"], "STEP_FATAL");
        assert_eq!(
            failure["HostFailure"]["message"],
            "引擎不变量校验失败（原始详情已脱敏）"
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

#[cfg(test)]
mod publisher_connection_tests {
    #[test]
    fn cached_private_frames_are_not_serialized_after_credential_logout() {
        let database = native_store::NativeDatabase::open_in_memory().unwrap();
        let identities = crate::identity::IdentityService::new(database);
        let login = identities.guest().unwrap();
        let mut connection = connection(DeliveryMode::Push);
        assert!(matches!(
            connection.ingest(update(6, 4)),
            PublisherAction::Buffered
        ));
        identities.logout(&login.token).unwrap();
        let cached = connection
            .take_push_frame()
            .expect("logout 前缓存的 frame 仍实际存在");
        let rejected = super::authenticated_frame_json(cached, &identities, &login.token);
        assert!(rejected.is_err(), "撤销凭据不得序列化或交付缓存 frame");
    }
    use super::*;
    use crate::actor::{EngineUpdate, HostFailure, PublicBaseline, PublicBaselineSnapshot};

    fn baseline(failure: Option<HostFailure>) -> PublicBaseline {
        PublicBaseline {
            timeline_generation: 7,
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
            failure,
        }
    }

    fn connection(delivery: DeliveryMode) -> WsPublisherConnection {
        let baseline = baseline(None);
        WsPublisherConnection::new(
            delivery,
            (
                baseline.snapshot.seq,
                baseline.snapshot.tick,
                baseline.timeline_generation,
            ),
            baseline.failure,
            120,
            0,
        )
        .unwrap()
    }

    fn failure() -> HostFailure {
        HostFailure::step(&engine::session::StepFatal::InvariantViolation {
            location: "server.step".into(),
            description: "receipt chain broke".into(),
        })
    }

    fn update(tick: u64, seq: u64) -> EngineUpdate {
        use engine::session::protocol::{TickBatch, TickFrame};
        EngineUpdate {
            timeline_generation: 7,
            update: Some(engine::session::protocol::EngineUpdate::TickBatch(
                Box::new(TickBatch {
                    frames: vec![TickFrame {
                        tick,
                        seq_from: seq,
                        seq_to: seq,
                        facts: Vec::new(),
                        events: Vec::new(),
                        timeseries_payload: Default::default(),
                    }],
                    runtime_snapshot: None,
                    runtime_delta: None,
                }),
            )),
            civil_date: "2030-01-02".into(),
            public_revision: 6,
            failure: None,
        }
    }

    fn frame_tick(frame: PublisherFrame) -> u64 {
        let Some(engine::session::protocol::EngineUpdate::TickBatch(batch)) = frame.update else {
            panic!("fixture 应为 TickBatch")
        };
        batch.frames[0].tick
    }

    #[test]
    fn failure_precedes_generation_and_resync_and_latches_after_delivery() {
        let mut connection = connection(DeliveryMode::Pull);
        connection.require_resync();
        let mut failed = update(6, 4);
        failed.timeline_generation = 6;
        failed.failure = Some(failure());
        let PublisherAction::HostFailure(delivered) = connection.ingest(failed.clone()) else {
            panic!("故障必须先于旧 timeline 和 resync barrier")
        };
        assert!(connection.delivered_failure.is_none());
        connection.failure_delivered(*delivered);
        assert!(matches!(
            connection.ingest(failed),
            PublisherAction::Ignored
        ));
        assert!(matches!(
            connection.request_frame(),
            FrameRequest::ResyncRequired
        ));
    }

    #[test]
    fn covered_updates_timeline_changes_and_lag_keep_frame_barrier() {
        let mut connection = connection(DeliveryMode::Pull);
        assert!(matches!(
            connection.ingest(update(5, 4)),
            PublisherAction::Ignored
        ));
        assert!(matches!(connection.request_frame(), FrameRequest::Empty));
        assert!(matches!(
            connection.ingest(update(6, 4)),
            PublisherAction::Buffered
        ));
        let mut replacement = update(7, 4);
        replacement.timeline_generation = 8;
        assert!(matches!(
            connection.ingest(replacement),
            PublisherAction::ResyncRequired {
                reason: "timeline_changed",
                ..
            }
        ));
        assert!(connection.take_push_frame().is_none());
        assert!(matches!(
            connection.request_frame(),
            FrameRequest::ResyncRequired
        ));
        connection.baseline_delivered(None);
        connection.require_resync();
        assert!(matches!(
            connection.request_frame(),
            FrameRequest::ResyncRequired
        ));
    }

    #[test]
    fn resync_preparation_clears_buffer_without_mutating_failed_baseline_state() {
        let mut connection = connection(DeliveryMode::Pull);
        assert!(matches!(
            connection.ingest(update(6, 4)),
            PublisherAction::Buffered
        ));
        connection.failure_delivered(failure());
        let before = (
            connection.baseline_seq,
            connection.baseline_tick,
            connection.timeline_generation,
            connection.awaiting_resync,
            connection.delivered_failure.clone(),
        );
        connection.prepare_resync();
        assert!(connection.take_flush_frame().is_none());
        assert_eq!(
            (
                connection.baseline_seq,
                connection.baseline_tick,
                connection.timeline_generation,
                connection.awaiting_resync,
                connection.delivered_failure.clone()
            ),
            before
        );
        let mut fresh = baseline(None);
        fresh.timeline_generation += 1;
        connection.install_baseline_cursor(&fresh);
        assert!(connection.awaiting_resync);
        connection.baseline_delivered(None);
        assert!(!connection.awaiting_resync);
        assert!(connection.delivered_failure.is_none());
    }

    #[test]
    fn flush_actions_leave_incoming_update_unaccepted_until_send_succeeds() {
        for (error, expected_all) in [
            (FrameBufferError::MetadataTransition, false),
            (FrameBufferError::BufferCapacityExceeded { limit: 1 }, true),
        ] {
            let mut connection = connection(DeliveryMode::Push);
            connection.ingest(update(6, 4));
            let PublisherAction::Flush {
                update: incoming,
                all,
            } = connection.buffer_error(update(7, 4), error)
            else {
                panic!("Push 应返回待发送动作")
            };
            assert_eq!(
                all, expected_all,
                "MetadataTransition 应仅 flush 一帧，Push capacity 应 flush 全部 backlog"
            );
            let frame = connection.take_flush_frame().unwrap();
            assert_eq!(frame_tick(frame), 6);
            assert!(
                connection.take_flush_frame().is_none(),
                "发送失败前不得接受后继 update"
            );
            connection.accept_after_flush(*incoming).unwrap();
            assert_eq!(frame_tick(connection.take_push_frame().unwrap()), 7);
        }
    }

    #[test]
    fn pull_capacity_clears_backlog_and_get_frame_rejects_push_mode() {
        let mut pull = connection(DeliveryMode::Pull);
        pull.ingest(update(6, 4));
        assert!(matches!(
            pull.buffer_error(
                update(7, 4),
                FrameBufferError::BufferCapacityExceeded { limit: 1 }
            ),
            PublisherAction::ResyncRequired {
                reason: "publisher_buffer_capacity",
                ..
            }
        ));
        assert!(pull.take_flush_frame().is_none());
        assert!(matches!(pull.request_frame(), FrameRequest::ResyncRequired));
        let mut push = connection(DeliveryMode::Push);
        assert!(matches!(
            push.request_frame(),
            FrameRequest::WrongDeliveryMode
        ));
    }
}
