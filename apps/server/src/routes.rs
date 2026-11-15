//! HTTP + WS 路由（与前端 RemoteHost 严格对齐）。
//!
//! 契约（见任务详情 / ADR-0005 §6 双通道）：
//! - POST /api/new     body {setup, seed}        -> 200 {session_id} | 400
//! - POST /api/intent  body {session_id, generation, intent} -> 200 | 401 | 403 | 400
//! - GET  /api/snapshot?session_id=..           -> 200 Snapshot | 401 | 403
//! - POST /api/speed   body {session_id, speed}  -> 200 | 401 | 403 | 400
//! - GET  /api/speed?session_id=..              -> 200 SpeedMetrics | 401 | 403
//! - POST /api/running body {session_id, running}-> 204 | 401 | 403
//! - POST /api/save | /api/load                  -> 存档/原子恢复 | 401 | 403 | 400
//! - DELETE /api/session?session_id=..           -> Bearer 鉴权后停止并删除会话
//! - WS   /ws?session_id=..&delivery=..           -> Bearer 鉴权后先发 public baseline，再按 push/pull 交付 PublisherFrame
//!
//! engine 类型经 serde_json 跨界（server 是 Rust，engine 作 rlib 依赖，无 TS）。
//! 错误处理（铁律二）：会话私有路由缺 token → 401，携带 token 但会话未知或 token 不匹配 → 403；
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
use crate::publisher::PublisherFrame;
use engine::session::publication::{ObserverPump, PumpRead, Read as PublicationRead};
// The JSON envelope wraps one engine save. Leave room for its session identity
// while allowing every save accepted by the engine decoder through this route.
pub const MAX_LOAD_BODY_BYTES: usize = engine::MAX_SAVE_DECODE_BYTES + 1024 * 1024;
const MAX_NESTED_SAVE_DEPTH: usize = 64;

/// 路由共享状态：单一 `SessionManager`（actor 各自独占 GameSession，manager 仅持消息端点）。
#[derive(Clone)]
pub struct AppState {
    pub manager: SessionManager,
}

/// 缺 token 返回 401；携带 token 但 session 不存在或 token 不匹配时统一返回 403，避免探测 session ID。
/// B3 远程登录：Bearer 头解析为身份 credential，经 IdentityService 认证
/// 得认证主体（OpaqueSubjectId）。缺凭据/无效凭据 → 401；存储/密码学
/// 异常 → 500（不与凭据错误混淆）。
fn request_subject(
    state: &AppState,
    token: Option<&str>,
) -> Result<engine::session::OpaqueSubjectId, Box<Response>> {
    let token = token.filter(|token| !token.is_empty()).ok_or_else(|| {
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

/// 本人金融接口：身份解析主体 + 当前市场的资金成员关系校验。
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
    let subject = request_subject(state, token)?;
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

/// 市场级接口：身份解析主体（不要求成员——列表/加入/上下文用）。
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

/// 控制面接口：身份解析主体 + 市场控制权校验。
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
    let (handles, subject) = authorized_market(state, session_id, token).await?;
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
        "session token does not authorize this session",
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
pub struct NewSessionBody {
    pub setup: engine::SessionSetup,
    pub seed: String,
}

/// /api/new 200 响应体（B3 共享市场上下文——18725d07 契约）。
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
pub struct IntentBody {
    pub session_id: String,
    pub generation: String,
    pub intent: engine::Intent,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReadBody {
    pub session_id: String,
    pub generation: String,
    pub query: engine::session::protocol::CompanyReadQuery,
}

/// 本人公司查询；账户在 actor 内按当前 subject 再次解析，不接受客户端选择。
pub async fn api_company_read_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CompanyReadBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_session(&state, &body.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.company_read_query_for(generation, subject, body.query.clone()).await {
        Ok((generation, owner, Ok(value))) => Json(serde_json::json!({"generation": generation.to_string(), "owner": owner, "value": value})).into_response(),
        Ok((_, _, Err(error))) => {
            warn!(session = %body.session_id, query = ?body.query, error = ?error, "公司只读查询被领域拒绝");
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code": "COMPANY_READ_REJECTED", "failure": error.public_failure()}))).into_response()
        }
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "会话 actor 已退出，请同步有效会话后重试"),
        Err(SendCommandError::Rejected(reason)) if reason == "STALE_SESSION_GENERATION" => api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason),
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "COMPANY_READ_REJECTED", reason),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("公司查询不校验倍速"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscribeRightsOfferingBody {
    pub session_id: String,
    pub generation: String,
    pub event_id: String,
    pub requested_shares: String,
}

pub async fn api_subscribe_rights_offering(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<SubscribeRightsOfferingBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let requested_shares = match parse_subscription_shares(&body.requested_shares) {
        Some(value) => value,
        None => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUESTED_SHARES",
                "requested_shares 必须是规范的 0..=u64::MAX 十进制字符串",
            )
        }
    };
    let (handles, subject) =
        match authorized_session(&state, &body.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match handles
        .member_request(
            subject,
            crate::actor::MemberRequest::SubscribeRightsOffering {
                generation,
                event_id: body.event_id,
                requested_shares,
            },
        )
        .await
    {
        Ok(crate::actor::MemberResponse::RightsSubscription(generation, Ok(receipt))) => {
            Json(serde_json::json!({"generation":generation.to_string(),"receipt":receipt}))
                .into_response()
        }
        Ok(crate::actor::MemberResponse::RightsSubscription(_, Err(error))) => {
            warn!(session = %body.session_id, error = ?error, "配股认购被领域拒绝");
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code":"RIGHTS_SUBSCRIPTION_REJECTED","failure":error.public_failure()}))).into_response()
        }
        Ok(crate::actor::MemberResponse::CompanyWriteFatal(failure)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(failure)).into_response()
        }
        Ok(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "COMPANY_WRITE_RESPONSE_INVALID",
            "公司写命令响应类型错误，请反馈",
        ),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "会话 actor 已退出，请同步有效会话后重试",
        ),
        Err(SendCommandError::Rejected(reason)) if reason == "STALE_SESSION_GENERATION" => {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => api_error(
            StatusCode::BAD_REQUEST,
            "RIGHTS_SUBSCRIPTION_REJECTED",
            reason,
        ),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("公司写命令不校验倍速"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposePlayerCompanyActionBody {
    pub session_id: String,
    pub generation: String,
    pub proposal: engine::session::PlayerCompanyProposal,
}

pub async fn api_propose_player_company_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ProposePlayerCompanyActionBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let (handles, subject) =
        match authorized_session(&state, &body.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .member_request(
            subject,
            crate::actor::MemberRequest::ProposePlayerCompanyAction {
                generation,
                proposal: body.proposal,
            },
        )
        .await
    {
        Ok(crate::actor::MemberResponse::CompanyProposal(generation, Ok(value))) => {
            Json(serde_json::json!({"generation":generation.to_string(),"receipt":value}))
                .into_response()
        }
        Ok(crate::actor::MemberResponse::CompanyProposal(_, Err(error))) => {
            warn!(session = %body.session_id, error = ?error, "公司写命令被领域拒绝");
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code":"PLAYER_COMPANY_ACTION_REJECTED","failure":error.public_failure()}))).into_response()
        }
        Ok(crate::actor::MemberResponse::CompanyWriteFatal(failure)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(failure)).into_response()
        }
        Ok(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "COMPANY_WRITE_RESPONSE_INVALID",
            "公司写命令响应类型错误，请反馈",
        ),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "会话 actor 已退出，请同步有效会话后重试",
        ),
        Err(SendCommandError::Rejected(reason)) if reason == "STALE_SESSION_GENERATION" => {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => api_error(
            StatusCode::BAD_REQUEST,
            "PLAYER_COMPANY_ACTION_REJECTED",
            reason,
        ),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("公司写命令不校验倍速"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnqueueReportCorrectionBody {
    pub session_id: String,
    pub generation: String,
    pub request: engine::session::CompanyReportCorrection,
}

pub async fn api_enqueue_report_correction(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<EnqueueReportCorrectionBody>, JsonRejection>,
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
    match handles.member_request(subject, crate::actor::MemberRequest::EnqueueReportCorrection { generation, request: body.request }).await {
        Ok(crate::actor::MemberResponse::ReportCorrection(generation, Ok(value))) => Json(serde_json::json!({"generation":generation.to_string(),"operation_id":value.operation_id})).into_response(),
        Ok(crate::actor::MemberResponse::ReportCorrection(_, Err(error))) => {
            warn!(session = %body.session_id, error = ?error, "公司写命令被领域拒绝");
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code":"REPORT_CORRECTION_REJECTED","failure":error.public_failure()}))).into_response()
        }
        Ok(crate::actor::MemberResponse::CompanyWriteFatal(failure)) =>
            (StatusCode::INTERNAL_SERVER_ERROR, Json(failure)).into_response(),
        Ok(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "COMPANY_WRITE_RESPONSE_INVALID", "公司写命令响应类型错误，请反馈"),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "会话 actor 已退出，请同步有效会话后重试"),
        Err(SendCommandError::Rejected(reason)) if reason == "STALE_SESSION_GENERATION" => api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason),
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "REPORT_CORRECTION_REJECTED", reason),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("公司写命令不校验倍速"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelReportCorrectionBody {
    pub session_id: String,
    pub generation: String,
    pub operation_id: String,
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
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.member_request(subject, crate::actor::MemberRequest::CancelReportCorrection { generation, operation_id: body.operation_id }).await {
        Ok(crate::actor::MemberResponse::ReportCorrection(generation, Ok(value))) => Json(serde_json::json!({"generation":generation.to_string(),"operation_id":value.operation_id})).into_response(),
        Ok(crate::actor::MemberResponse::ReportCorrection(_, Err(error))) => {
            warn!(session = %body.session_id, error = ?error, "公司写命令被领域拒绝");
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code":"REPORT_CORRECTION_REJECTED","failure":error.public_failure()}))).into_response()
        }
        Ok(crate::actor::MemberResponse::CompanyWriteFatal(failure)) =>
            (StatusCode::INTERNAL_SERVER_ERROR, Json(failure)).into_response(),
        Ok(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "COMPANY_WRITE_RESPONSE_INVALID", "公司写命令响应类型错误，请反馈"),
        Err(SendCommandError::ActorGone) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "ACTOR_GONE", "会话 actor 已退出，请同步有效会话后重试"),
        Err(SendCommandError::Rejected(reason)) if reason == "STALE_SESSION_GENERATION" => api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason),
        Err(SendCommandError::Rejected(reason)) => api_error(StatusCode::BAD_REQUEST, "REPORT_CORRECTION_REJECTED", reason),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("公司写命令不校验倍速"),
    }
}

/// /api/speed 请求体。
#[derive(Debug, Deserialize)]
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
pub struct RunningBody {
    pub session_id: String,
    pub generation: String,
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
        .set_pause_preferences_for(generation, subject, body.preferences)
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

fn parse_subscription_shares(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
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
pub struct DeleteSessionQuery {
    pub session_id: String,
    pub generation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionQuery {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PlayerWorkingOrdersQuery {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockHistoryQuery {
    pub session_id: String,
    pub generation: u64,
    pub code: engine::StockId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialAllocationQuery {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    pub session_id: String,
    pub generation: String,
    pub candidate: Option<engine::session::protocol::SaveCandidateKey>,
}

#[derive(Debug, Deserialize)]
pub struct PublicReportQueryParams {
    pub session_id: String,
    /// 报告 id 的数值形式（公开页 next_cursor 为其十进制文本）。
    pub cursor: Option<u32>,
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
    #[serde(default)]
    pub observer_fencing: bool,
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
    CaptureObserver {},
    FenceObserver {
        capture_id: String,
        timeline_generation: String,
        publication_ordinal: String,
    },
    ReleaseObserver {
        capture_id: String,
    },
    SubmitIntent {
        request_id: u64,
        generation: String,
        intent: engine::Intent,
    },
}

/// POST /api/new：身份鉴权 → 创建共享市场（单市场全局）→ 返回市场上下文。
///
/// - 缺/无效凭据 → 401；已有市场 → 409（列出并加入，不另开）；非法 body → 400。
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
        Err(NewSessionError::Text(message)) => {
            warn!(error = %message, "new_session rejected (server domain)");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SESSION_SETUP_FAILED",
                message,
            )
        }
    }
}

/// 市场上下文应答（创建/加入/查询共用的 200 形态）。
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetMarketBody {
    pub session_id: String,
    pub generation: String,
    pub setup: engine::SessionSetup,
    pub seed: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionCashBody {
    pub session_id: String,
    pub generation: String,
    pub admission_cash: engine::Money,
}

/// POST /api/market/reset：控制权重置市场（交易时钟配置必须一致）。
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

/// POST /api/market/admission-cash：控制权调整入场资金。
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

/// GET /api/markets：身份鉴权后的市场列表（每市场附本人上下文）。
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
        markets.push(serde_json::json!({
            "session_id": id,
            "setup": context.setup,
            "seed": context.seed,
            "resumed": handles.startup_resumed,
            "generation": context.generation,
            "member": context.member,
            "can_control": context.can_control,
            "needs_rejoin": context.needs_rejoin,
        }));
    }
    Json(serde_json::json!({ "markets": markets })).into_response()
}

/// GET /api/market/context：本人市场上下文。
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

/// POST /api/markets/join：加入市场（确认重加入后按当前配置发一次入场资金）。
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
    Path(company_id): Path<u32>,
    Query(params): Query<PublicReportQueryParams>,
) -> Response {
    let (handles, _) =
        match authorized_market(&state, &params.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
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
    Path((company_id, report_id)): Path<(u32, String)>,
    Query(params): Query<PublicReportQueryParams>,
) -> Response {
    let (handles, _) =
        match authorized_market(&state, &params.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
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
    // 复核建议 4：NPC 决策细节属控制面诊断——鉴权升级为控制权
    // （authorized_market_controller，18725d07 契约）。
    let (handles, _) = match authorized_market_controller(
        &state,
        &params.session_id,
        authorization_token(&headers),
    )
    .await
    {
        Ok(context) => context,
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
            if reason.contains("fault[1290]")
                || reason.starts_with("STALE_SESSION_GENERATION:") =>
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
    match authorized_market(&state, &query.session_id, authorization_token(&headers)).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({
            "npcDecisionDiagnostics": cfg!(all(feature = "simulation-diagnostics", debug_assertions)),
        }))).into_response(),
        Err(response) => *response,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostQueryBody {
    session_id: String,
    generation: String,
    query: crate::actor::HostQuery,
}

pub(crate) async fn api_host_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<HostQueryBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    let private = matches!(
        &body.query,
        crate::actor::HostQuery::PersonalTradeHistory(_)
            | crate::actor::HostQuery::PersonalTradeConfirmations(_)
    );
    let authorization = if private {
        authorized_session(&state, &body.session_id, authorization_token(&headers)).await
    } else {
        authorized_market(&state, &body.session_id, authorization_token(&headers)).await
    };
    let (handles, subject) = match authorization {
        Ok(context) => context,
        Err(response) => return *response,
    };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match handles.host_query(generation, subject, body.query).await {
        Ok((generation, value)) => (
            StatusCode::OK,
            Json(serde_json::json!({"generation": generation.to_string(), "value": value})),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason)) if reason.contains("STALE_SESSION_GENERATION") => {
            api_error(StatusCode::CONFLICT, "STALE_SESSION_GENERATION", reason)
        }
        Err(SendCommandError::Rejected(reason)) => {
            api_error(StatusCode::BAD_REQUEST, "HOST_QUERY_REJECTED", reason)
        }
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("查询不修改速度"),
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
    match engine::indicators::calculate_indicators(&body.prices, &body.candles, None) {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INDICATOR_INPUT",
            error.to_string(),
        ),
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
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles.enqueue_for(generation, subject, body.intent).await {
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
            Ok(context) => context,
            Err(response) => return *response,
        };
    match handles.snapshot_for(subject).await {
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
    // 复核建议 2：成员鉴权（authorized_session——非成员不得读取在途单）。
    // 读面仍是 main 的账户 0 单人形态（PlayerWorkingOrder 无 owner 字段，
    // 多成员本人过滤形态待 W3 读面多玩家化；actor 侧 MemberRequest::
    // WorkingOrders 对远程多成员已显式拒绝）。
    let (handles, _) =
        match authorized_session(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    match handles.player_working_orders(query.generation).await {
        Ok((generation, orders)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "orders": orders })),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.contains("fault[1290]")
                || reason.starts_with("STALE_SESSION_GENERATION:") =>
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
    // 复核建议 2：本人股票历史——成员鉴权 + history_for（按成员账户查询
    // 与记账 read_count；此前 authorized_market+账户 0 通道使非成员可读
    // 且经历账本记在账户 0 上越权）。
    let (handles, subject) =
        match authorized_session(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    match handles
        .history_for(query.generation, subject, query.code)
        .await
    {
        Ok((generation, data)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "data": data })),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.contains("fault[1290]")
                || reason.starts_with("STALE_SESSION_GENERATION:") =>
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
    let (handles, _) =
        match authorized_market(&state, &query.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
            Err(response) => return *response,
        };
    match handles.initial_allocation(query.generation).await {
        Ok((generation, data)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "generation": generation.to_string(), "data": data })),
        )
            .into_response(),
        Err(SendCommandError::Rejected(reason))
            if reason.contains("fault[1290]")
                || reason.starts_with("STALE_SESSION_GENERATION:") =>
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

/// POST /api/save：在 actor 内生成一致存档。存档包含会话私有事实，须由持有会话 token 的客户端保存。
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
    let (handles, _) =
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
    match handles.save(generation, body.candidate).await {
        Ok(slot) => (StatusCode::OK, Json(slot)).into_response(),
        Err(SendCommandError::ActorGone) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ACTOR_GONE",
            "session actor gone",
        ),
        Err(SendCommandError::Rejected(reason))
            if reason.contains("fault[1290]")
                || reason.starts_with("STALE_SESSION_GENERATION:") =>
        {
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
    let (handles, _subject) =
        match authorized_market_controller(&state, &body.session_id, authorization_token(&headers))
            .await
        {
            Ok(context) => context,
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
        Err(engine::SessionError::ResourceLimit(fault)) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "SAVE_RESOURCE_LIMIT",
                fault.to_string(),
            );
        }
        Err(error) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SAVE", error.to_string()),
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
            Ok(context) => context,
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
    match handles.set_speed_for(generation, subject, speed).await {
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
    let (handles, _) =
        match authorized_market(&state, &q.session_id, authorization_token(&headers)).await {
            Ok(context) => context,
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
            Ok(context) => context,
            Err(response) => return *response,
        };
    let generation = match parse_host_parity_generation(&body.generation) {
        Ok(generation) => generation,
        Err(response) => return *response,
    };
    match handles
        .set_running_for(generation, subject, body.running)
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
            Ok(context) => context,
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
            Ok(context) => context,
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
pub async fn api_delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeleteSessionQuery>,
) -> Response {
    let (handles, subject) =
        match authorized_market_controller(&state, &q.session_id, authorization_token(&headers))
            .await
        {
            Ok(context) => context,
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
    // 保留已认证主体用于附着和重同步；内容由权威生成的受限观察端投影。
    let (handles, subject) = match authorized_market(&state, &q.session_id, token.as_deref()).await
    {
        Ok(context) => context,
        Err(response) => return *response,
    };
    ws.protocols(["stock-game"])
        .on_upgrade(move |socket| run_ws(socket, handles, subject, q.delivery, q.observer_fencing))
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

/// WebSocket 只驱动已注册的共享观察端，不缓存另一份行情队列。
struct WsPublisherConnection {
    delivery: DeliveryMode,
    timeline_generation: u64,
    pump: ObserverPump<crate::actor::ScopedEngineUpdate>,
    initial_orders: Option<Vec<engine::session::protocol::PlayerWorkingOrder>>,
    awaiting_resync: bool,
    delivered_failure: Option<crate::actor::HostFailure>,
}

enum FrameAction {
    Frame {
        ordinal: u64,
        update: PublisherFrame,
    },
    Failure(crate::actor::HostFailure),
    Resync(&'static str),
    Ignored,
    Empty,
    Closed,
}

impl WsPublisherConnection {
    fn attach(
        delivery: DeliveryMode,
        attachment: crate::actor::ObservationAttachment,
    ) -> (Self, crate::actor::PublicBaseline) {
        let crate::actor::ObservationAttachment {
            baseline,
            observer,
            initial_orders,
        } = attachment;
        let connection = Self {
            delivery,
            timeline_generation: baseline.timeline_generation,
            pump: ObserverPump::new(
                observer,
                engine::session::publication::OBSERVER_IN_FLIGHT_LIMIT,
            ),
            initial_orders,
            awaiting_resync: baseline.failure.is_some(),
            delivered_failure: baseline.failure.clone(),
        };
        (connection, baseline)
    }

    fn poll(&mut self) -> FrameAction {
        match self.pump.poll() {
            PumpRead::Ready(PublicationRead::Item { ordinal, value }) => {
                let update = value.into_projected(&mut self.initial_orders);
                self.classify(ordinal, update)
            }
            PumpRead::Ready(PublicationRead::NeedsBaseline { .. }) => {
                self.awaiting_resync = true;
                FrameAction::Resync("event_stream_lagged")
            }
            PumpRead::Ready(PublicationRead::Empty) | PumpRead::CreditBlocked => FrameAction::Empty,
            PumpRead::Ready(PublicationRead::Closed) => FrameAction::Closed,
        }
    }

    fn classify(&mut self, ordinal: u64, update: PublisherFrame) -> FrameAction {
        if let Some(failure) = update.failure.as_ref() {
            if self
                .delivered_failure
                .as_ref()
                .is_some_and(|first| !first.recoverable || first == failure)
            {
                return FrameAction::Ignored;
            }
            return FrameAction::Failure(failure.clone());
        }
        if update.timeline_generation != self.timeline_generation {
            self.awaiting_resync = true;
            return FrameAction::Resync("timeline_changed");
        }
        if self.awaiting_resync || update.update.is_none() {
            return FrameAction::Ignored;
        }
        FrameAction::Frame { ordinal, update }
    }

    fn failure_delivered(&mut self, failure: crate::actor::HostFailure) {
        if self
            .delivered_failure
            .as_ref()
            .is_some_and(|first| !first.recoverable)
        {
            return;
        }
        self.awaiting_resync = !failure.is_correction_prepare_rejected(self.timeline_generation);
        self.delivered_failure = Some(failure);
    }

    async fn wait_push_or_barrier(&mut self) -> FrameAction {
        loop {
            let epoch = self.pump.signal().load();
            if self.pump.should_poll(self.delivery == DeliveryMode::Push) {
                match self.poll() {
                    FrameAction::Empty => {}
                    FrameAction::Ignored => continue,
                    action => return action,
                }
            }
            self.pump.signal().wait_for_change(epoch).await;
        }
    }

    async fn deliver(&mut self, sender: &mut WsSender, action: FrameAction) -> bool {
        match action {
            FrameAction::Frame { ordinal, update } => {
                if let Err(error) = self.pump.record_sent(ordinal) {
                    warn!(?error, "ws: invalid shared publisher ordinal");
                    return false;
                }
                if !send_publisher_frame(sender, &update).await {
                    return false;
                }
                match self.pump.acknowledge(ordinal) {
                    Ok(true) => true,
                    result => {
                        warn!(
                            ?result,
                            "ws: publisher transport did not confirm its ordinal"
                        );
                        false
                    }
                }
            }
            FrameAction::Failure(failure) => {
                if !send_host_failure(sender, failure.clone()).await {
                    return false;
                }
                self.failure_delivered(failure);
                true
            }
            FrameAction::Resync(reason) => send_resync_required(sender, reason, None).await,
            FrameAction::Ignored => true,
            FrameAction::Closed => false,
            FrameAction::Empty => sender
                .send(axum::extract::ws::Message::Text(
                    serde_json::json!({ "FrameEmpty": {} }).to_string(),
                ))
                .await
                .is_ok(),
        }
    }
}

/// WS 切面是当前连接的租约；每条新观察流使用自己的 0 号基线。
#[derive(Default)]
struct ObserverCutState {
    counter: u64,
    held: Option<(u64, u64, bool)>,
}

impl ObserverCutState {
    fn begin(&mut self, generation: u64) -> u64 {
        self.counter = self.counter.checked_add(1).expect("观察切面序号溢出");
        self.held = Some((self.counter, generation, false));
        self.counter
    }

    fn matches(&self, id: u64, generation: u64, ordinal: u64) -> bool {
        ordinal == 0
            && self
                .held
                .is_some_and(|(current, expected, _)| (current, expected) == (id, generation))
    }

    fn accept(&mut self, id: u64, generation: u64, ordinal: u64, endpoint_accepted: bool) -> bool {
        if !self.matches(id, generation, ordinal) || !endpoint_accepted {
            return false;
        }
        self.held = Some((id, generation, true));
        true
    }

    fn release(&mut self, id: u64) -> bool {
        if !self
            .held
            .is_some_and(|(current, _, accepted)| current == id && accepted)
        {
            return false;
        }
        self.held = None;
        true
    }
}

#[cfg(test)]
#[test]
fn observer_cut_requires_current_accepted_ticket_before_release() {
    let mut cut = ObserverCutState::default();
    let first = cut.begin(4);
    let second = cut.begin(4);
    assert!(!cut.accept(first, 4, 0, true), "旧切面不能覆盖当前围栏");
    assert!(!cut.accept(second, 3, 0, true), "错误世代不能续接");
    assert!(!cut.accept(second, 4, 1, true), "新观察流在 0 号基线取样");
    assert!(!cut.accept(second, 4, 0, false), "溢出后必须重取切面");
    assert!(!cut.release(second), "未确认的切面不能放行");
    assert!(cut.accept(second, 4, 0, true));
    assert!(!cut.release(first), "晚到释放不能越过新围栏");
    assert!(cut.release(second));
    assert!(!cut.release(second));
}

/// WS 本身由独立 I/O 任务执行；市场推进从不等待此 socket。
async fn run_ws(
    socket: axum::extract::ws::WebSocket,
    handles: Arc<crate::actor::SessionHandles>,
    subject: engine::session::OpaqueSubjectId,
    delivery: DeliveryMode,
    advertising_observer_fence: bool,
) {
    let (deadline_sender, mut deadline_receiver) = tokio::sync::watch::channel(None);
    let connection = run_ws_connection(
        socket,
        handles,
        subject,
        delivery,
        advertising_observer_fence,
        deadline_sender,
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
    subject: engine::session::OpaqueSubjectId,
    delivery: DeliveryMode,
    advertising_observer_fence: bool,
    deadline_sender: tokio::sync::watch::Sender<Option<tokio::time::Instant>>,
) {
    let (mut sender, mut receiver) = socket.split();

    let mut connection = match handles.observe_for(subject.clone()).await {
        Ok(attachment) => {
            let (connection, baseline) = WsPublisherConnection::attach(delivery, attachment);
            let failed = baseline.failure.is_some();
            if !send_ws_baseline(&mut sender, baseline, advertising_observer_fence).await || failed
            {
                return;
            }
            connection
        }
        Err(crate::actor::PublicBaselineError::Catalog(error)) => {
            let failure = crate::actor::HostFailure::catalog(
                &error,
                "public_baseline",
                "server.actor.public_baseline",
            );
            let _ = send_host_failure(&mut sender, failure).await;
            return;
        }
        Err(crate::actor::PublicBaselineError::Command(error)) => {
            warn!(%error, "ws: scoped observation attachment failed");
            return;
        }
    };
    let mut observer_cut = ObserverCutState::default();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = heartbeat.tick().await; // 跳过首个立即到期。
    let mut heartbeat_state = WsHeartbeat::default();

    loop {
        tokio::select! {
            action = connection.wait_push_or_barrier(), if !connection.awaiting_resync && observer_cut.held.is_none() => {
                if !connection.deliver(&mut sender, action).await { break; }
            }
            // 心跳：30s 发 Ping（防中间设备杀空闲连接）。
            _ = heartbeat.tick() => {
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
                                    match handles.observe_for(subject.clone()).await {
                                        Ok(attachment) => {
                                            let (replacement, baseline) = WsPublisherConnection::attach(delivery, attachment);
                                            if !send_baseline(&mut sender, baseline).await { break; }
                                            connection = replacement;
                                            observer_cut.held = None;
                                        }
                                        Err(crate::actor::PublicBaselineError::Catalog(error)) => {
                                            let failure = crate::actor::HostFailure::catalog(&error, "public_baseline", "server.actor.public_baseline");
                                            let _ = send_host_failure(&mut sender, failure).await;
                                            break;
                                        }
                                        Err(crate::actor::PublicBaselineError::Command(error)) => {
                                            if !send_gateway_error(&mut sender, None, "RESYNC_FAILED", error.to_string()).await { break; }
                                        }
                                    }
                                }
                                ClientCommand::CaptureObserver {} => {
                                    match handles.observe_for(subject.clone()).await {
                                        Ok(attachment) => {
                                            let (replacement, baseline) = WsPublisherConnection::attach(delivery, attachment);
                                            if baseline.failure.is_some() {
                                                let _ = send_baseline(&mut sender, baseline).await;
                                                break;
                                            }
                                            let id = observer_cut.begin(baseline.timeline_generation);
                                            connection = replacement;
                                            if !send_observer_cut(&mut sender, &baseline, id).await { break; }
                                        }
                                        Err(crate::actor::PublicBaselineError::Catalog(error)) => {
                                            let failure = crate::actor::HostFailure::catalog(&error, "public_baseline", "server.actor.public_baseline");
                                            let _ = send_host_failure(&mut sender, failure).await;
                                            break;
                                        }
                                        Err(crate::actor::PublicBaselineError::Command(error)) => {
                                            if !send_gateway_error(&mut sender, None, "RESYNC_FAILED", error.to_string()).await { break; }
                                        }
                                    }
                                }
                                ClientCommand::FenceObserver { capture_id, timeline_generation, publication_ordinal } => {
                                    let accepted = match (
                                        parse_host_parity_generation(&capture_id),
                                        parse_host_parity_generation(&timeline_generation),
                                        parse_host_parity_generation(&publication_ordinal),
                                    ) {
                                        (Ok(id), Ok(generation), Ok(ordinal)) if observer_cut.matches(id, generation, ordinal) => {
                                            let ready = !connection.pump.signal().is_closed() && connection.pump.resume_after(ordinal).is_ok();
                                            observer_cut.accept(id, generation, ordinal, ready)
                                        }
                                        _ => false,
                                    };
                                    let response = serde_json::json!({ "ObserverFenced": { "capture_id": capture_id, "accepted": accepted } });
                                    if sender.send(axum::extract::ws::Message::Text(response.to_string())).await.is_err() { break; }
                                }
                                ClientCommand::ReleaseObserver { capture_id } => {
                                    let released = parse_host_parity_generation(&capture_id)
                                        .is_ok_and(|id| observer_cut.release(id));
                                    if !released && !send_gateway_error(&mut sender, None, "OBSERVER_CUT_STALE", "无已接受的同代观察切面").await { break; }
                                }
                                ClientCommand::GetFrame {} => {
                                    if observer_cut.held.is_some() || connection.awaiting_resync {
                                        if !send_gateway_error(&mut sender, None, "RESYNC_REQUIRED", "send Resync before requesting frames").await { break; }
                                    } else if delivery != DeliveryMode::Pull {
                                        if !send_gateway_error(&mut sender, None, "WRONG_DELIVERY_MODE", "GetFrame 只允许用于 pull 模式").await { break; }
                                    } else {
                                        let action = loop {
                                            match connection.poll() {
                                                FrameAction::Ignored => continue,
                                                action => break action,
                                            }
                                        };
                                        if !connection.deliver(&mut sender, action).await { break; }
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
                                    match handles.enqueue(generation, intent).await {
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

type WsSender =
    futures_util::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>;

async fn send_publisher_frame(sender: &mut WsSender, frame: &PublisherFrame) -> bool {
    #[derive(Serialize)]
    enum Wire<'a> {
        PublisherFrame(&'a PublisherFrame),
    }
    match serde_json::to_string(&Wire::PublisherFrame(frame)) {
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

async fn send_observer_cut(
    sender: &mut WsSender,
    baseline: &crate::actor::PublicBaseline,
    capture_id: u64,
) -> bool {
    #[derive(Serialize)]
    struct Cut<'a> {
        #[serde(flatten)]
        baseline: &'a crate::actor::PublicBaseline,
        publication_ordinal: &'static str,
        capture_id: String,
    }
    #[derive(Serialize)]
    enum Wire<'a> {
        ObserverCut(Cut<'a>),
    }
    let response = Wire::ObserverCut(Cut {
        baseline,
        publication_ordinal: "0",
        capture_id: capture_id.to_string(),
    });
    match serde_json::to_string(&response) {
        Ok(json) => sender
            .send(axum::extract::ws::Message::Text(json))
            .await
            .is_ok(),
        Err(error) => {
            error!(%error, "ws: serialize observer cut failed");
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
    send_ws_baseline(sender, baseline, false).await
}

async fn send_ws_baseline(
    sender: &mut WsSender,
    baseline: crate::actor::PublicBaseline,
    observer_fencing: bool,
) -> bool {
    match baseline_wire_messages_inner(baseline, observer_fencing) {
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

#[cfg(test)]
fn baseline_wire_messages(
    baseline: crate::actor::PublicBaseline,
) -> Result<Vec<String>, serde_json::Error> {
    baseline_wire_messages_inner(baseline, false)
}

fn baseline_wire_messages_inner(
    baseline: crate::actor::PublicBaseline,
    observer_fencing: bool,
) -> Result<Vec<String>, serde_json::Error> {
    if let Some(failure) = &baseline.failure {
        return Ok(vec![serde_json::to_string(
            &serde_json::json!({ "HostFailure": failure }),
        )?]);
    }
    if observer_fencing {
        #[derive(Serialize)]
        struct BaselineWithCapability {
            #[serde(flatten)]
            baseline: crate::actor::PublicBaseline,
            observer_fencing: bool,
        }
        #[derive(Serialize)]
        enum Wire {
            Baseline(BaselineWithCapability),
        }
        return Ok(vec![serde_json::to_string(&Wire::Baseline(
            BaselineWithCapability {
                baseline,
                observer_fencing,
            },
        ))?]);
    }
    Ok(vec![serde_json::to_string(
        &serde_json::json!({ "Baseline": baseline }),
    )?])
}

#[cfg(test)]
mod baseline_failure_tests {
    use super::baseline_wire_messages;
    use crate::actor::{HostFailure, PublicBaseline, PublicBaselineSnapshot};

    #[test]
    fn latched_failure_is_sent_without_installing_a_healthy_wire_baseline() {
        let messages = baseline_wire_messages(PublicBaseline {
            stock_catalog: Vec::new(),
            company_catalog: Vec::new(),
            timeline_generation: 3,
            snapshot: PublicBaselineSnapshot {
                seq: 4,
                tick: 5,
                day: 1,
                phase: engine::TradingPhase::Continuous,
                mirror_origin: engine::RuntimeMirrorOrigin {
                    authority_epoch: 1,
                    install_sequence: 4,
                    source_tick: 5,
                    source_generation: 0,
                    day_end_stage: engine::RuntimeMirrorDayEndStage::Tick,
                    non_tick_revision: 0,
                    mirror_phase: engine::TradingPhase::Continuous,
                },
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

        assert_eq!(
            messages.len(),
            1,
            "terminal baseline 不得先作为健康 wire baseline 交付"
        );
        let failure: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert!(failure.get("Baseline").is_none());
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
    use super::*;
    use crate::actor::{EngineUpdate, HostFailure};
    use engine::session::publication::channel;
    use std::num::NonZeroUsize;

    fn connection(delivery: DeliveryMode) -> WsPublisherConnection {
        let (_, observer) = channel(NonZeroUsize::MIN);
        WsPublisherConnection {
            delivery,
            timeline_generation: 7,
            pump: ObserverPump::new(observer, NonZeroUsize::MIN),
            initial_orders: Some(Vec::new()),
            awaiting_resync: false,
            delivered_failure: None,
        }
    }

    fn failure() -> HostFailure {
        HostFailure::step(&engine::session::StepFatal::InvariantViolation {
            location: "server.step".into(),
            description: "receipt chain broke".into(),
        })
    }

    fn update(tick: u64) -> EngineUpdate {
        use engine::session::protocol::{TickBatch, TickFrame};
        EngineUpdate {
            timeline_generation: 7,
            update: Some(engine::session::protocol::EngineUpdate::TickBatch(
                Box::new(TickBatch {
                    frames: vec![TickFrame {
                        tick,
                        seq_from: 4,
                        seq_to: 4,
                        facts: Vec::new(),
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

    #[test]
    fn same_fact_cursor_can_have_two_distinct_observation_ordinals() {
        let mut connection = connection(DeliveryMode::Push);
        assert!(matches!(
            connection.classify(1, update(6)),
            FrameAction::Frame { ordinal: 1, .. }
        ));
        assert!(matches!(
            connection.classify(2, update(6)),
            FrameAction::Frame { ordinal: 2, .. }
        ));
    }

    #[test]
    fn changed_timeline_gates_later_frames_until_fresh_attach() {
        let mut connection = connection(DeliveryMode::Pull);
        let mut replacement = update(6);
        replacement.timeline_generation = 8;
        assert!(matches!(
            connection.classify(1, replacement),
            FrameAction::Resync("timeline_changed")
        ));
        assert!(connection.awaiting_resync);
        assert!(matches!(
            connection.classify(2, update(6)),
            FrameAction::Ignored
        ));
    }

    #[test]
    fn failure_precedes_generation_checks_and_latches_after_delivery() {
        let mut connection = connection(DeliveryMode::Pull);
        connection.awaiting_resync = true;
        let mut failed = update(6);
        failed.timeline_generation = 6;
        failed.failure = Some(failure());
        let FrameAction::Failure(delivered) = connection.classify(1, failed.clone()) else {
            panic!("首个故障必须先于旧 timeline 检查");
        };
        assert!(connection.delivered_failure.is_none());
        connection.failure_delivered(delivered);
        assert!(matches!(
            connection.classify(2, failed),
            FrameAction::Ignored
        ));
        assert!(connection.awaiting_resync);
    }

    #[test]
    fn prepare_rejection_preserves_current_timeline_unless_a_fatal_was_delivered() {
        for delivery in [DeliveryMode::Push, DeliveryMode::Pull] {
            let mut connection = connection(delivery);
            let rejected = HostFailure::correction_prepare_rejection_fixture();
            connection.failure_delivered(rejected.clone());
            assert!(!connection.awaiting_resync);
            assert!(matches!(
                connection.classify(2, update(7)),
                FrameAction::Frame { ordinal: 2, .. }
            ));
            let fatal = failure();
            connection.failure_delivered(fatal.clone());
            connection.failure_delivered(rejected);
            assert!(connection.awaiting_resync);
            assert_eq!(connection.delivered_failure, Some(fatal));
        }
    }
}

#[cfg(test)]
#[path = "routes/company_write_tests.rs"]
mod company_write_tests;
