//! 桌面端 lib：Tauri 2 命令 + engine 直连（actor-per-session）。
//!
//! 设计与 `apps/server/src/actor.rs` 同源（ADR-0005 §5），针对单机桌面端裁剪：
//! - **无共享可变状态、无锁**：每个 `GameSession` 由独立 tokio task 独占 own；
//!   外部（Tauri command）一律经 **mpsc 命令通道** 与之交互。
//! - **事件出口**：actor 每 tick `step()` 产出 `Event[]`，经 `app.emit("engine-event", ...)`
//!   推给前端（区别于 server 的 broadcast —— 桌面端单窗口，事件直接 emit）。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 单玩家 v1：意图固定路由给玩家 `AccountId(0)`（与 web-wasm / server 一致）。
//!
//! 防御式（铁律二）：所有失败显式返回 `Result<_, String>`，绝不静默吞错。
//! Tauri command 的 `Err(String)` 会被前端 `invoke` 的 Promise reject 接住 → 显式展示。

pub mod actor;

use std::sync::Arc;

use actor::{SendCommandError, SessionManager};
use engine::{Intent, SessionError, SessionSetup, Snapshot};
use serde::Serialize;
use tauri::{AppHandle, State};

/// 前端监听的事件名（`@tauri-apps/api/event` 的 `listen("engine-event", ...)`）。
pub const ENGINE_EVENT_NAME: &str = "engine-event";

/// emit 给前端的 payload：一次 step 产出的全部 Event（数组，保留 seq 顺序）。
///
/// `Event` 自身 `serde::Serialize`（外部标签），序列化形态与 web-wasm / server 完全一致，
/// 前端 `types/engine.ts` 的 `EngineEvent` 直接复用，无需二次适配。
#[derive(Debug, Clone, Serialize)]
pub struct EngineEventPayload {
    /// 会话 ID（前端可据此区分，当前单会话恒为 create_session 返回值）。
    pub session_id: String,
    /// 本批事件。
    pub events: Vec<engine::Event>,
}

// ── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 创建会话：构造 `GameSession` → 建 mpsc → spawn actor task → 注册。
/// 返回 session_id（前端后续命令携带）。
///
/// 失败：engine 构造非法（`SessionError`）→ 显式 `Err(String)`，绝不静默（铁律二）。
#[tauri::command]
async fn create_session(
    app: AppHandle,
    state: State<'_, DesktopState>,
    setup: SessionSetup,
    seed: u64,
) -> Result<String, String> {
    let manager = state.manager.clone();
    let session_id = manager
        .new_session(setup, seed, app)
        .map_err(map_session_error)?;
    Ok(session_id)
}

/// 入队玩家意图（v1 固定玩家 `AccountId(0)`）。
#[tauri::command]
async fn enqueue(
    state: State<'_, DesktopState>,
    session_id: String,
    intent: Intent,
) -> Result<(), String> {
    let handles = lookup_handles(&state, &session_id)?;
    handles.enqueue(intent).await.map_err(map_send_error)
}

/// 取完整快照（首次连 / 重连 / 存档）。
#[tauri::command]
async fn snapshot(
    state: State<'_, DesktopState>,
    session_id: String,
) -> Result<Snapshot, String> {
    let handles = lookup_handles(&state, &session_id)?;
    handles.snapshot().await.map_err(map_send_error)
}

/// 改变步进倍速（仅调整 interval，不立即 step）。fire-and-forget 经 mpsc 保证顺序。
#[tauri::command]
async fn set_speed(
    state: State<'_, DesktopState>,
    session_id: String,
    speed: f64,
) -> Result<(), String> {
    let handles = lookup_handles(&state, &session_id)?;
    handles.set_speed(speed).await.map_err(map_send_error)
}

// ── 桥接 helper ─────────────────────────────────────────────────────────────

/// 取 session 句柄；不存在 → 显式 `ActorGone`（不静默返回空）。
fn lookup_handles(
    state: &State<'_, DesktopState>,
    session_id: &str,
) -> Result<Arc<actor::SessionHandles>, SendCommandError> {
    state
        .manager
        .lookup(session_id)
        .ok_or(SendCommandError::ActorGone)
}

/// `SessionError` → 前端可读字符串（保留原 message，便于复现）。
fn map_session_error(e: SessionError) -> String {
    format!("创建会话失败：{e}")
}

/// `SendCommandError` → 前端可读字符串。
fn map_send_error(e: SendCommandError) -> String {
    format!("会话指令失败：{e}")
}

/// `From<SendCommandError> for String`：让 Tauri command 内 `?` 能直接把命令投递失败
/// 转成前端可见字符串（复用 `map_send_error` 文案），无需在每个 call-site 写 `.map_err`。
impl From<SendCommandError> for String {
    fn from(e: SendCommandError) -> Self {
        map_send_error(e)
    }
}

/// 进程级共享状态：会话注册表（Tauri `.manage` 注入）。
#[derive(Default)]
pub struct DesktopState {
    pub(crate) manager: SessionManager,
}

/// Tauri 应用入口（main.rs 调用一次）。
///
/// 注册命令 + 注入状态。失败时 panic（防御式：Tauri 启动失败属不可恢复，应显式崩溃而非静默）。
pub fn run() {
    tauri::Builder::default()
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            create_session,
            enqueue,
            snapshot,
            set_speed,
        ])
        .setup(|_app| {
            // 预留：可在此读取 CLI 参数 / 初始化单例资源。当前无额外初始化。
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败（见上方错误）—— 不可恢复，显式崩溃。");
}
