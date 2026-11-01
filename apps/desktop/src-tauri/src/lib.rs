//! 桌面端 lib：Tauri 2 命令 + engine 直连（actor-per-session）。
//!
//! 设计与 `apps/server/src/actor.rs` 同源（ADR-0005 §5），针对单机桌面端裁剪：
//! - **无共享可变状态、无锁**：每个 `GameSession` 由独立 tokio task 独占 own；
//!   外部（Tauri command）一律经 **mpsc 命令通道** 与之交互。
//! - **事件出口**：actor 每 tick `step()` 产出 `Event[]`，经 `app.emit("engine-event", ...)`
//!   推给前端（区别于 server 的 broadcast —— 桌面端单窗口，事件直接 emit）。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 当前单玩家模式：意图固定路由给玩家 `AccountId(0)`（与 web-wasm / server 一致）。
//!
//! 防御式（铁律二）：所有失败显式返回 `Result<_, String>`，绝不静默吞错。
//! Tauri command 的 `Err(String)` 会被前端 `invoke` 的 Promise reject 接住 → 显式展示。

pub mod actor;

use std::sync::Arc;

use actor::{SendCommandError, SessionManager};
#[cfg(feature = "host-parity")]
use engine::session::protocol::CivilUpdate;
use engine::{
    calendar::CivilDate,
    company::{PublicReportPage, PublicReportQuery, PublicReportSummary},
    AccountId, Intent, NpcDecisionDiagnostics, SaveSlot, SessionError, SessionSetup, Snapshot,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State};

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
    /// 成功读档后更换；用于丢弃另一条 IPC 路径上晚到的旧时间线事件。
    pub timeline_id: String,
    pub update: engine::session::protocol::EngineUpdate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilities {
    pub npc_decision_diagnostics: bool,
}

#[tauri::command]
fn host_capabilities() -> HostCapabilities {
    HostCapabilities {
        npc_decision_diagnostics: cfg!(all(feature = "simulation-diagnostics", debug_assertions)),
    }
}

#[tauri::command]
fn calculate_indicators(
    prices: Vec<f64>,
    candles: Vec<engine::indicators::OhlcBar>,
) -> Result<engine::indicators::IndicatorResults, String> {
    engine::indicators::calculate_indicators(&prices, &candles, None)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn calculate_intraday_average(
    input: engine::IntradayAverageInput,
) -> Result<Option<engine::IntradayAverage>, String> {
    engine::calculate_intraday_average_from_input(&input).map_err(|error| error.to_string())
}

#[tauri::command]
fn calculate_intraday_average_curve(
    series_key: String,
    samples: Vec<engine::IntradayAverageInput>,
) -> Result<serde_json::Value, String> {
    if series_key.is_empty() || series_key.chars().count() > 256 || samples.len() > 600 {
        return Err("VWAP 曲线 series_key 或样本数无效".to_owned());
    }
    let results =
        engine::calculate_intraday_average_curve(&samples).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({ "series_key": series_key, "results": results }))
}

// ── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 创建会话：构造 `GameSession` → 建 mpsc → spawn actor task → 注册。
/// 返回 session_id（前端后续命令携带）。
///
/// 失败：engine 构造非法（`SessionError`）→ 显式 `Err(String)`，绝不静默（铁律二）。
#[tauri::command]
async fn create_session<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DesktopState>,
    setup: SessionSetup,
    seed: String,
) -> Result<String, String> {
    let seed = seed
        .parse::<u64>()
        .map_err(|error| format!("随机种子必须是 0..=u64::MAX 的十进制整数：{error}"))?;
    let manager = state.manager.clone();
    let session_id = manager
        .new_session(setup, seed, app)
        .await
        .map_err(map_session_error)?;
    Ok(session_id)
}

/// 入队玩家意图（固定玩家 `AccountId(0)`）。
#[tauri::command]
async fn enqueue(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    intent: Intent,
) -> Result<(), String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .enqueue(parse_generation(generation)?, intent)
        .await
        .map_err(map_send_error)
}

/// 取完整快照（首次连 / 重连 / 存档）。
#[tauri::command]
async fn snapshot(state: State<'_, DesktopState>, session_id: String) -> Result<Snapshot, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.snapshot().await.map_err(map_send_error)
}

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum BaselineIpcError {
    Catalog(actor::HostFailure),
    Existing(String),
}

impl From<String> for BaselineIpcError {
    fn from(error: String) -> Self {
        Self::Existing(error)
    }
}

fn map_baseline_error(
    error: actor::BaselineRequestError,
    operation: &'static str,
    location: &'static str,
) -> BaselineIpcError {
    match error {
        actor::BaselineRequestError::Baseline(actor::BaselineError::Catalog(error)) => {
            BaselineIpcError::Catalog(actor::HostFailure::catalog(&error, operation, location))
        }
        actor::BaselineRequestError::Baseline(actor::BaselineError::Session(error)) => {
            BaselineIpcError::Existing(map_send_error(SendCommandError::Rejected(
                error.to_string(),
            )))
        }
        actor::BaselineRequestError::Command(error) => {
            BaselineIpcError::Existing(map_send_error(error))
        }
    }
}

#[tauri::command]
async fn engine_baseline(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::RestoreResult, BaselineIpcError> {
    let handles = lookup_handles(&state, &session_id)
        .await
        .map_err(map_send_error)?;
    handles
        .query_baseline(parse_generation(generation)?)
        .await
        .map_err(|error| {
            map_baseline_error(error, "query_baseline", "desktop.actor.query_baseline")
        })
}

#[tauri::command]
async fn player_working_orders(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::GenerationResponse<serde_json::Value>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .player_working_orders(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn host_history_query(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    query: actor::HistoryQuery,
) -> Result<actor::GenerationResponse<serde_json::Value>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .host_history_query(parse_generation(generation)?, query)
        .await
}

#[tauri::command]
async fn query_stock_history(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    code: engine::StockId,
) -> Result<actor::GenerationResponse<engine::session::HistoricalStockData>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .query_stock_history(parse_generation(generation)?, code)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn initial_allocation(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::GenerationResponse<engine::session::InitialAllocation>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .initial_allocation(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
}

/// 取不含历史日 K 的轻量运行快照（跨日 UI 同步）。
#[tauri::command]
async fn runtime_snapshot(
    state: State<'_, DesktopState>,
    session_id: String,
) -> Result<Snapshot, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.runtime_snapshot().await.map_err(map_send_error)
}

#[tauri::command]
async fn civil_date(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::GenerationResponse<CivilDate>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .civil_date(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
enum CompanyReadIpcError {
    Session { message: String },
    Query(engine::session::protocol::CompanyReadFailure),
}

#[tauri::command]
async fn company_read_query(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    query: engine::session::protocol::CompanyReadQuery,
) -> Result<actor::CompanyReadResponse, CompanyReadIpcError> {
    let handles = lookup_handles(&state, &session_id).await.map_err(|error| {
        CompanyReadIpcError::Session {
            message: error.to_string(),
        }
    })?;
    let generation =
        parse_generation(generation).map_err(|message| CompanyReadIpcError::Session { message })?;
    handles
        .company_read_query(generation, query)
        .await
        .map_err(|error| match error {
            actor::CompanyReadHostError::Session { message } => {
                CompanyReadIpcError::Session { message }
            }
            actor::CompanyReadHostError::Query(error) => {
                eprintln!("desktop.company_read_query 领域拒绝：{error:?}");
                CompanyReadIpcError::Query(error.public_failure())
            }
        })
}

#[tauri::command]
async fn subscribe_rights_offering(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    event_id: String,
    requested_shares: String,
) -> Result<actor::RightsSubscriptionResponse, actor::CompanyWriteHostFailure> {
    let requested_shares = parse_subscription_shares(&requested_shares)
        .map_err(|message| actor::CompanyWriteHostFailure::Session { message })?;
    let handles = lookup_handles(&state, &session_id).await.map_err(|error| {
        actor::CompanyWriteHostFailure::Session {
            message: error.to_string(),
        }
    })?;
    let generation = parse_generation(generation)
        .map_err(|message| actor::CompanyWriteHostFailure::Session { message })?;
    handles
        .subscribe_rights_offering(generation, event_id, requested_shares)
        .await
        .map_err(|error| {
            eprintln!("desktop.subscribe_rights_offering 领域拒绝：{error:?}");
            error.public_failure()
        })
}

#[tauri::command]
async fn propose_player_company_action(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    proposal: engine::session::PlayerCompanyProposal,
) -> Result<actor::CompanyProposalResponse, actor::CompanyWriteHostFailure> {
    let handles = lookup_handles(&state, &session_id).await.map_err(|error| {
        actor::CompanyWriteHostFailure::Session {
            message: error.to_string(),
        }
    })?;
    let generation = parse_generation(generation)
        .map_err(|message| actor::CompanyWriteHostFailure::Session { message })?;
    handles
        .propose_player_company_action(generation, proposal)
        .await
        .map_err(|error| {
            eprintln!("desktop.propose_player_company_action 领域拒绝：{error:?}");
            error.public_failure()
        })
}

#[tauri::command]
async fn enqueue_report_correction(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    request: engine::session::CompanyReportCorrection,
) -> Result<actor::ReportCorrectionResponse, actor::CompanyWriteHostFailure> {
    let handles = lookup_handles(&state, &session_id).await.map_err(|error| {
        actor::CompanyWriteHostFailure::Session {
            message: error.to_string(),
        }
    })?;
    let generation = parse_generation(generation)
        .map_err(|message| actor::CompanyWriteHostFailure::Session { message })?;
    handles
        .enqueue_report_correction(generation, request)
        .await
        .map_err(|error| {
            eprintln!("desktop.enqueue_report_correction 领域拒绝：{error:?}");
            error.public_failure()
        })
}

#[tauri::command]
async fn cancel_report_correction(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    operation_id: String,
) -> Result<actor::ReportCorrectionResponse, actor::CompanyWriteHostFailure> {
    let handles = lookup_handles(&state, &session_id).await.map_err(|error| {
        actor::CompanyWriteHostFailure::Session {
            message: error.to_string(),
        }
    })?;
    let generation = parse_generation(generation)
        .map_err(|message| actor::CompanyWriteHostFailure::Session { message })?;
    handles
        .cancel_report_correction(generation, operation_id)
        .await
        .map_err(|error| {
            eprintln!("desktop.cancel_report_correction 领域拒绝：{error:?}");
            error.public_failure()
        })
}

#[tauri::command]
async fn public_reports(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    query: PublicReportQuery,
) -> Result<actor::GenerationResponse<PublicReportPage>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .public_reports(parse_generation(generation)?, query)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn public_report_by_id(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    id: String,
) -> Result<actor::GenerationResponse<PublicReportSummary>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .public_report_by_id(parse_generation(generation)?, id)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn npc_decision_diagnostics(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    account: u64,
) -> Result<actor::GenerationResponse<NpcDecisionDiagnostics>, String> {
    if !cfg!(all(feature = "simulation-diagnostics", debug_assertions)) {
        return Err("NPC 决策诊断仅在启用开发诊断的非 release 桌面端可用".into());
    }
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .npc_decision_diagnostics(parse_generation(generation)?, AccountId(account))
        .await
        .map_err(map_send_error)
}

/// 读取桌面 actor 权威的设定速度与最近实际 tick/现实秒采样。
#[tauri::command]
async fn speed_metrics(
    state: State<'_, DesktopState>,
    session_id: String,
) -> Result<actor::SpeedMetrics, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.speed_metrics().await.map_err(map_send_error)
}

/// 在 actor 内串行生成存档，避免与正在执行的 step 形成撕裂状态。
#[tauri::command]
async fn save_session(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    candidate: Option<engine::session::protocol::SaveCandidateKey>,
) -> Result<SaveSlot, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .save(parse_generation(generation)?, candidate)
        .await
        .map_err(map_send_error)
}

/// 原子恢复存档；校验失败时 actor 保留原会话，前端可继续运行或修正文件。
#[tauri::command]
async fn restore_session(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    slot: SaveSlot,
) -> Result<actor::RestoreResult, BaselineIpcError> {
    let handles = lookup_handles(&state, &session_id)
        .await
        .map_err(map_send_error)?;
    handles
        .restore(parse_generation(generation)?, slot)
        .await
        .map_err(|error| map_baseline_error(error, "restore", "desktop.actor.restore"))
}

#[cfg(feature = "host-parity")]
#[tauri::command]
async fn host_parity_advance_civil_day(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<CivilUpdate, String> {
    lookup_handles(&state, &session_id)
        .await?
        .advance_civil_day(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
}

#[cfg(feature = "host-parity")]
#[tauri::command]
async fn host_parity_step(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<engine::session::protocol::EngineUpdate, String> {
    lookup_handles(&state, &session_id)
        .await?
        .step(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
}

/// 改变步进倍速（仅调整 interval，不立即 step），等待 actor 应用确认。
#[derive(Debug, Deserialize)]
enum SpeedRequest {
    Fixed(f64),
    Fastest,
}

#[tauri::command]
async fn set_speed(
    state: State<'_, DesktopState>,
    session_id: String,
    speed: SpeedRequest,
) -> Result<(), String> {
    let handles = lookup_handles(&state, &session_id).await?;
    let multiplier = match speed {
        SpeedRequest::Fixed(value) if value.is_finite() && value > 0.0 => value,
        SpeedRequest::Fixed(value) => {
            return Err(format!("非法速度倍率：{value}（必须为有限正数）"));
        }
        SpeedRequest::Fastest => f64::INFINITY,
    };
    handles.set_speed(multiplier).await.map_err(map_send_error)
}

#[tauri::command]
async fn pause_session(state: State<'_, DesktopState>, session_id: String) -> Result<(), String> {
    lookup_handles(&state, &session_id)
        .await?
        .set_running(false)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn set_pause_preferences(
    state: State<'_, DesktopState>,
    session_id: String,
    preferences: engine::session::protocol::PausePreferences,
) -> Result<(), String> {
    lookup_handles(&state, &session_id)
        .await?
        .set_pause_preferences(preferences)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn resume_session(state: State<'_, DesktopState>, session_id: String) -> Result<(), String> {
    lookup_handles(&state, &session_id)
        .await?
        .set_running(true)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn stop_session(state: State<'_, DesktopState>, session_id: String) -> Result<(), String> {
    let handles = state
        .manager
        .remove(&session_id)
        .await
        .ok_or_else(|| map_send_error(SendCommandError::ActorGone))?;
    handles.shutdown().await.map_err(map_send_error)
}

// ── 桥接 helper ─────────────────────────────────────────────────────────────

/// 取 session 句柄；不存在 → 显式 `ActorGone`（不静默返回空）。
async fn lookup_handles(
    state: &State<'_, DesktopState>,
    session_id: &str,
) -> Result<Arc<actor::SessionHandles>, SendCommandError> {
    state
        .manager
        .lookup(session_id)
        .await
        .ok_or(SendCommandError::ActorGone)
}

fn parse_subscription_shares(value: &str) -> Result<u64, String> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("认购股数必须是规范的 0..=u64::MAX 十进制字符串".into());
    }
    value
        .parse()
        .map_err(|_| "认购股数必须是规范的 0..=u64::MAX 十进制字符串".into())
}

fn parse_generation(generation: String) -> Result<u64, String> {
    if generation.is_empty()
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
        || (generation.len() > 1 && generation.starts_with('0'))
    {
        return Err("会话世代必须是规范的 0..=u64::MAX 十进制整数".to_owned());
    }
    generation
        .parse::<u64>()
        .map_err(|error| format!("会话世代必须是 0..=u64::MAX 的十进制整数：{error}"))
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

#[cfg(not(feature = "host-parity"))]
fn command_builder<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            create_session,
            enqueue,
            snapshot,
            engine_baseline,
            host_capabilities,
            calculate_indicators,
            calculate_intraday_average,
            calculate_intraday_average_curve,
            player_working_orders,
            query_stock_history,
            host_history_query,
            initial_allocation,
            runtime_snapshot,
            civil_date,
            public_reports,
            company_read_query,
            subscribe_rights_offering,
            propose_player_company_action,
            enqueue_report_correction,
            cancel_report_correction,
            public_report_by_id,
            npc_decision_diagnostics,
            speed_metrics,
            save_session,
            restore_session,
            set_speed,
            pause_session,
            set_pause_preferences,
            resume_session,
            stop_session,
        ])
}

#[cfg(feature = "host-parity")]
fn command_builder<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            create_session,
            enqueue,
            snapshot,
            engine_baseline,
            host_capabilities,
            calculate_indicators,
            calculate_intraday_average,
            calculate_intraday_average_curve,
            player_working_orders,
            query_stock_history,
            host_history_query,
            initial_allocation,
            runtime_snapshot,
            civil_date,
            public_reports,
            company_read_query,
            subscribe_rights_offering,
            propose_player_company_action,
            enqueue_report_correction,
            cancel_report_correction,
            public_report_by_id,
            npc_decision_diagnostics,
            speed_metrics,
            save_session,
            restore_session,
            host_parity_advance_civil_day,
            host_parity_step,
            set_speed,
            pause_session,
            set_pause_preferences,
            resume_session,
            stop_session,
        ])
}

/// Tauri 应用入口（main.rs 调用一次）。
///
/// 注册命令 + 注入状态。失败时 panic（防御式：Tauri 启动失败属不可恢复，应显式崩溃而非静默）。
pub fn run() {
    command_builder(tauri::Builder::default())
        .setup(|_app| {
            // 预留：可在此读取 CLI 参数 / 初始化单例资源。当前无额外初始化。
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败（见上方错误）—— 不可恢复，显式崩溃。");
}

#[cfg(test)]
mod lib_tests;

#[cfg(test)]
mod company_write_tests;
