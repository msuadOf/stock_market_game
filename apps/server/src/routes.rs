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
//! - WS   /ws?session_id=..&token=..             -> 先发 Snapshot，再推 EngineUpdate 批次
//!
//! engine 类型经 serde_json 跨界（server 是 Rust，engine 作 rlib 依赖，无 TS）。
//! 错误处理（铁律二）：未知 session → 404（不静默 200）；非法 body/构造 → 400；
//! engine 失败透传文案，绝不静默吞。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::actor::{NewSessionError, SendCommandError, SessionManager, MAX_SPEED_MULTIPLIER};

const MAX_SERVER_STOCKS: usize = 1_000;
const MAX_SERVER_NPCS: u64 = 100_000;
const MAX_SERVER_HISTORY_LEN: usize = 10_000;
const MAX_SERVER_MARKET_HISTORY_CELLS: usize = 2_000_000;
const MAX_SERVER_DECISIONS_PER_TICK: u64 = 100_000;
const MAX_SERVER_DECISIONS_PER_SECOND: u64 = 10_000_000;
const MAX_SERVER_SAVED_ORDERS: usize = engine::MAX_OPEN_ORDERS;
const MAX_SERVER_ORDERS_PER_ACCOUNT: usize = engine::MAX_OPEN_ORDERS_PER_ACCOUNT;
const MAX_SERVER_PENDING_INTENTS: usize = engine::MAX_PENDING_PLAYER_INTENTS;

fn validate_server_setup_budget(setup: &engine::SessionSetup) -> Result<(), String> {
    let stock_count = setup.stocks.len();
    let npc_count = u64::from(setup.npcs.retail_count)
        .checked_add(u64::from(setup.npcs.inst_count))
        .and_then(|total| total.checked_add(u64::from(setup.npcs.hot_count)))
        .ok_or_else(|| "NPC count overflowed the server budget calculation".to_string())?;
    let history_cells = stock_count
        .checked_mul(setup.history_len)
        .ok_or_else(|| "stock_count × history_len overflowed".to_string())?;
    let decisions_per_tick = npc_count
        .checked_mul(stock_count as u64)
        .ok_or_else(|| "NPC count × stock count overflowed".to_string())?;
    let decisions_per_second = decisions_per_tick
        .checked_mul(MAX_SPEED_MULTIPLIER as u64)
        .ok_or_else(|| "maximum-speed strategy work overflowed".to_string())?;

    if stock_count > MAX_SERVER_STOCKS
        || npc_count > MAX_SERVER_NPCS
        || setup.history_len > MAX_SERVER_HISTORY_LEN
        || history_cells > MAX_SERVER_MARKET_HISTORY_CELLS
        || decisions_per_tick > MAX_SERVER_DECISIONS_PER_TICK
        || decisions_per_second > MAX_SERVER_DECISIONS_PER_SECOND
    {
        return Err(format!(
            "setup exceeds server limits: stocks={stock_count}/{MAX_SERVER_STOCKS}, npcs={npc_count}/{MAX_SERVER_NPCS}, history_len={}/{MAX_SERVER_HISTORY_LEN}, history_cells={history_cells}/{MAX_SERVER_MARKET_HISTORY_CELLS}, decisions_per_tick={decisions_per_tick}/{MAX_SERVER_DECISIONS_PER_TICK}, decisions_per_second_at_max_speed={decisions_per_second}/{MAX_SERVER_DECISIONS_PER_SECOND}",
            setup.history_len,
        ));
    }
    Ok(())
}

fn validate_server_save_budget(slot: &engine::SaveSlot) -> Result<(), String> {
    validate_server_setup_budget(&slot.setup)?;
    if slot.pending_player.len() > MAX_SERVER_PENDING_INTENTS {
        return Err(format!(
            "save exceeds pending intent limit: {}/{}",
            slot.pending_player.len(),
            MAX_SERVER_PENDING_INTENTS
        ));
    }
    let mut total_orders = 0_usize;
    let mut per_account = BTreeMap::<engine::AccountId, usize>::new();
    for owner in slot
        .auction_orders
        .values()
        .flatten()
        .map(|order| order.owner)
        .chain(
            slot.resting_orders
                .values()
                .flatten()
                .map(|order| order.owner),
        )
    {
        total_orders = total_orders
            .checked_add(1)
            .ok_or_else(|| "saved order count overflowed".to_string())?;
        let count = per_account.entry(owner).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| "saved per-account order count overflowed".to_string())?;
        if *count > MAX_SERVER_ORDERS_PER_ACCOUNT {
            return Err(format!(
                "save exceeds per-account order limit for account {}: {}/{}",
                owner.0, count, MAX_SERVER_ORDERS_PER_ACCOUNT
            ));
        }
    }
    if total_orders > MAX_SERVER_SAVED_ORDERS {
        return Err(format!(
            "save exceeds total order limit: {total_orders}/{MAX_SERVER_SAVED_ORDERS}"
        ));
    }
    Ok(())
}

/// 路由共享状态：单一 `SessionManager`（actor 各自独占 GameSession，manager 仅持消息端点）。
#[derive(Clone)]
pub struct AppState {
    pub manager: SessionManager,
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
pub struct RestoreBody {
    pub session_id: String,
    pub slot: engine::SaveSlot,
}

/// /api/snapshot / /ws 共用的 query 参数。
#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    pub session_id: String,
}

/// /ws 的 query（token 当前仅做存在性校验，联机鉴权日后接 ADR-0005 §5.4）。
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub session_id: String,
    pub token: String,
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
    if let Err(message) = validate_server_setup_budget(&body.setup) {
        return api_error(StatusCode::BAD_REQUEST, "SETUP_RESOURCE_LIMIT", message);
    }
    match state.manager.new_session(body.setup, seed) {
        Ok(id) => {
            info!(session = %id, "new session created");
            (StatusCode::OK, Json(NewSessionResp { session_id: id })).into_response()
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
        Ok(slot) => match validate_server_save_budget(&slot) {
            Ok(()) => (StatusCode::OK, Json(slot)).into_response(),
            Err(message) => api_error(StatusCode::BAD_REQUEST, "SAVE_RESOURCE_LIMIT", message),
        },
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
pub async fn api_load(
    State(state): State<AppState>,
    body: Result<Json<RestoreBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return invalid_json_response(error),
    };
    if let Err(message) = validate_server_save_budget(&body.slot) {
        return api_error(StatusCode::BAD_REQUEST, "SAVE_RESOURCE_LIMIT", message);
    }
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return api_error(StatusCode::NOT_FOUND, "UNKNOWN_SESSION", "unknown session");
    };
    match handles.restore(body.slot).await {
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

/// WS /ws：握手 → 先发完整 Snapshot 对齐基线 → 持续推 EngineUpdate JSON 批次。
///
/// - 缺 token / 未知 session → 拒绝（当前握手仅校验 token 存在性）。
/// - 心跳：~30s 后端发 Ping；客户端不回则由 tungstenite/代理超时清理（ADR-0005 §6）。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(state): State<AppState>,
) -> Response {
    // 当前鉴权边界：token 非空即放行（联机鉴权日后接）。
    if q.token.is_empty() {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    }
    let Some(handles) = state.manager.lookup(&q.session_id) else {
        return (StatusCode::NOT_FOUND, "unknown session").into_response();
    };
    // handles 已是 Arc<SessionHandles>；clone 一份 event_tx 给 select 循环，handles 给取基线快照。
    let event_tx = handles.event_tx.clone();
    ws.on_upgrade(move |socket| run_ws(socket, event_tx, handles))
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
) {
    let (mut sender, mut receiver) = socket.split();

    // 先订阅再取基线，消除 snapshot 与 subscribe 之间丢事件的竞态；基线 seq 之前的
    // 缓冲事件在后续读取时跳过。
    let mut rx = event_tx.subscribe();
    let baseline_seq;

    // 1. 对齐基线：发完整 Snapshot JSON。
    match handles.snapshot().await {
        Ok(snap) => {
            baseline_seq = snap.seq;
            match serde_json::to_string(&snap) {
                Ok(json) => {
                    if sender
                        .send(axum::extract::ws::Message::Text(json))
                        .await
                        .is_err()
                    {
                        warn!("ws: failed to send baseline snapshot; closing");
                        return;
                    }
                }
                Err(e) => {
                    error!(error = %e, "ws: serialize snapshot failed");
                    return;
                }
            }
        }
        Err(SendCommandError::ActorGone) => {
            warn!("ws: actor gone before baseline snapshot");
            return;
        }
        Err(SendCommandError::Rejected(_)) => {
            warn!("ws: snapshot rejected");
            return;
        }
        Err(SendCommandError::InvalidSpeed(_)) => unreachable!("snapshot cannot validate speed"),
    }

    // 3/4. 心跳 interval + 事件/消息 select。
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = heartbeat.tick().await; // 跳过首个立即到期。

    loop {
        tokio::select! {
            // 事件到达 → 推 JSON。
            ev = rx.recv() => {
                match ev {
                    Ok(mut update) => {
                        update.events.retain(|event| event.seq() > baseline_seq);
                        if update.events.is_empty() {
                            continue;
                        }
                        match serde_json::to_string(&serde_json::json!({ "EngineUpdate": update })) {
                            Ok(json) => {
                                if sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                                    debug!("ws: send engine update failed; client likely disconnected");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "ws: serialize engine update failed; closing connection");
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // 慢消费者必须立即重新拉快照；显式协议消息避免客户端只看到 seq 缺口。
                        warn!(missed = n, "ws: lagged, client should re-sync via snapshot");
                        let message = serde_json::json!({
                            "ResyncRequired": {
                                "reason": "event_stream_lagged",
                                "missed": n,
                            }
                        })
                        .to_string();
                        if sender.send(axum::extract::ws::Message::Text(message)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("ws: event stream closed (actor exited)");
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
                        // Ping/Pong/Binary/Text 均忽略（服务端单向推送）；tungstenite 自动回 Ping 的 Pong。
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
