//! HTTP + WS 路由（与前端 RemoteHost 严格对齐）。
//!
//! 契约（见任务详情 / ADR-0005 §6 双通道）：
//! - POST /api/new     body {setup, seed}        -> 200 {session_id} | 400
//! - POST /api/intent  body {session_id, intent} -> 200 | 404 | 400
//! - GET  /api/snapshot?session_id=..           -> 200 Snapshot | 404
//! - POST /api/speed   body {session_id, speed}  -> 200 | 404
//! - WS   /ws?session_id=..&token=..            -> 先发完整 Snapshot 对齐基线，再持续推 Event[]
//!
//! engine 类型经 serde_json 跨界（server 是 Rust，engine 作 rlib 依赖，无 TS）。
//! 错误处理（铁律二）：未知 session → 404（不静默 200）；非法 body/构造 → 400；
//! engine 失败透传文案，绝不静默吞。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::actor::{SendCommandError, SessionManager};

/// 路由共享状态：单一 `SessionManager`（actor 各自独占 GameSession，manager 仅持消息端点）。
#[derive(Clone)]
pub struct AppState {
    pub manager: SessionManager,
}

/// /api/new 请求体。`seed` 是 u64（前端可能用 BigInt，JSON number 在 u64 范围内可无损表达）。
#[derive(Debug, Deserialize)]
pub struct NewSessionBody {
    pub setup: engine::SessionSetup,
    pub seed: u64,
}

/// /api/new 200 响应体。
#[derive(Debug, Serialize)]
pub struct NewSessionResp {
    pub session_id: String,
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
    pub speed: f64,
}

/// /api/snapshot / /ws 共用的 query 参数。
#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    pub session_id: String,
}

/// /ws 的 query（多一个 token；v1 token 仅做存在性校验，联机鉴权日后接 ADR-0005 §5.4）。
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
    Json(body): Json<NewSessionBody>,
) -> Response {
    match state.manager.new_session(body.setup, body.seed) {
        Ok(id) => {
            info!(session = %id, "new session created");
            (StatusCode::OK, Json(NewSessionResp { session_id: id })).into_response()
        }
        Err(e) => {
            warn!(error = %e, "new_session rejected");
            (StatusCode::BAD_REQUEST, format!("invalid setup: {e}")).into_response()
        }
    }
}

/// POST /api/intent：入队玩家意图（v1 固定 player 0）。
///
/// - 未知 session → 404；engine 拒绝/actor 关闭 → 400/500；成功 → 200。
pub async fn api_intent(
    State(state): State<AppState>,
    Json(body): Json<IntentBody>,
) -> Response {
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return (StatusCode::NOT_FOUND, "unknown session").into_response();
    };
    match handles.enqueue(body.intent).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %body.session_id, "intent: actor gone");
            (StatusCode::INTERNAL_SERVER_ERROR, "session actor gone").into_response()
        }
        Err(SendCommandError::Rejected) => {
            warn!(session = %body.session_id, "intent rejected by engine");
            (StatusCode::BAD_REQUEST, "intent rejected").into_response()
        }
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
        return (StatusCode::NOT_FOUND, "unknown session").into_response();
    };
    match handles.snapshot().await {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %q.session_id, "snapshot: actor gone");
            (StatusCode::INTERNAL_SERVER_ERROR, "session actor gone").into_response()
        }
        Err(SendCommandError::Rejected) => (StatusCode::BAD_REQUEST, "rejected").into_response(),
    }
}

/// POST /api/speed：改变倍速。
///
/// - 未知 session → 404；非法 speed（非正/非有限）由 actor 忽略并 warn（保持原速），仍 200。
pub async fn api_speed(
    State(state): State<AppState>,
    Json(body): Json<SpeedBody>,
) -> Response {
    let Some(handles) = state.manager.lookup(&body.session_id) else {
        return (StatusCode::NOT_FOUND, "unknown session").into_response();
    };
    match handles.set_speed(body.speed).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(SendCommandError::ActorGone) => {
            error!(session = %body.session_id, "speed: actor gone");
            (StatusCode::INTERNAL_SERVER_ERROR, "session actor gone").into_response()
        }
        Err(SendCommandError::Rejected) => (StatusCode::BAD_REQUEST, "rejected").into_response(),
    }
}

/// WS /ws：握手 → 先发完整 Snapshot 对齐基线 → 持续推 Event[] JSON。
///
/// - 缺 token / 未知 session → 拒绝（前端契约要求握手鉴权，v1 仅校验存在性）。
/// - 心跳：~30s 后端发 Ping；客户端不回则由 tungstenite/代理超时清理（ADR-0005 §6）。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(state): State<AppState>,
) -> Response {
    // v1 鉴权：token 非空即放行（联机鉴权日后接）。
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
/// 2. 订阅事件 broadcast，逐条把 Event 序列化 JSON 推出（各带 seq）。
/// 3. 同时读客户端消息（仅作存活/pong 探测；v1 不处理客户端业务消息）。
/// 4. 30s 心跳：发 Ping。
async fn run_ws(
    socket: axum::extract::ws::WebSocket,
    event_tx: tokio::sync::broadcast::Sender<engine::Event>,
    handles: Arc<crate::actor::SessionHandles>,
) {
    let (mut sender, mut receiver) = socket.split();

    // 1. 对齐基线：发完整 Snapshot JSON。
    match handles.snapshot().await {
        Ok(snap) => {
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
        Err(SendCommandError::Rejected) => {
            warn!("ws: snapshot rejected");
            return;
        }
    }

    // 2. 订阅事件流。
    let mut rx = event_tx.subscribe();

    // 3/4. 心跳 interval + 事件/消息 select。
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = heartbeat.tick().await; // 跳过首个立即到期。

    loop {
        tokio::select! {
            // 事件到达 → 推 JSON。
            ev = rx.recv() => {
                match ev {
                    Ok(event) => {
                        match serde_json::to_string(&event) {
                            Ok(json) => {
                                if sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                                    debug!("ws: send event failed; client likely disconnected");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "ws: serialize event failed (skipping one event)");
                                // 不静默丢弃：记 error 后继续（单个事件序列化失败不应杀连接）。
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // 慢消费者丢事件：靠 snapshot+seq 对齐（ADR-0005 §6）。warn 可见，不杀连接。
                        warn!(missed = n, "ws: lagged, client should re-sync via snapshot");
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
            // 读客户端消息（Pong/Close/其它）：仅作存活探测与礼貌关闭，v1 不解析业务消息。
            msg = receiver.next() => {
                match msg {
                    Some(Ok(m)) => {
                        if matches!(m, axum::extract::ws::Message::Close(_)) {
                            debug!("ws: client sent close");
                            break;
                        }
                        // Ping/Pong/Binary/Text 均忽略（v1 单向推送）；tungstenite 自动回 Ping 的 Pong。
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
