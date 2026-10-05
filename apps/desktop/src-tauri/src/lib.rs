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
    company::{PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportPage, PublicReportQuery, PublicReportSummary},
    AccountId, Intent, NpcDecisionDiagnostics, SaveSlot, SessionError, SessionSetup, Snapshot,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

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
    pub persistence: &'static str,
    pub npc_decision_diagnostics: bool,
    pub indicator_capabilities: IndicatorCapabilities,
    pub personal_trade_history: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorCapabilities {
    pub intraday_average: bool,
    pub macd: bool,
    pub price_kdj: bool,
    pub candle_kdj: bool,
}

#[tauri::command]
fn host_capabilities() -> HostCapabilities {
    HostCapabilities {
        persistence: "native",
        npc_decision_diagnostics: cfg!(all(feature = "simulation-diagnostics", debug_assertions)),
        indicator_capabilities: IndicatorCapabilities {
            intraday_average: true,
            macd: true,
            price_kdj: true,
            candle_kdj: true,
        },
        personal_trade_history: true,
    }
}

#[tauri::command]
fn calculate_indicators(
    prices: Vec<f64>,
    candles: Vec<engine::indicators::OhlcBar>,
) -> Result<engine::indicators::IndicatorResults, String> {
    engine::indicators::calculate_indicators(&prices, &candles).map_err(|error| error.to_string())
}

#[tauri::command]
fn calculate_intraday_average(
    turnover_cents: String,
    trade_count: u64,
    volume_shares: u64,
) -> Result<Option<engine::IntradayAverage>, String> {
    let turnover_cents = engine::parse_turnover_cents(&turnover_cents)
        .map_err(|error| format!("turnover_cents 无效：{error}"))?;
    let stats = engine::DailyTradeStats { turnover_cents, trade_count };
    engine::calculate_intraday_average(Some(&stats), volume_shares).map_err(|error| error.to_string())
}

#[tauri::command]
fn calculate_intraday_average_curve(
    series_key: String,
    samples: Vec<engine::IntradayAverageInput>,
) -> Result<IntradayAverageCurveResponse, String> {
    if series_key.is_empty() || series_key.len() > 256 { return Err("VWAP curve series_key 长度无效".to_owned()); }
    if samples.len() > 600 { return Err("VWAP curve 样本数不能超过 600".to_owned()); }
    let results = engine::calculate_intraday_average_curve(&samples).map_err(|error| error.to_string())?;
    Ok(IntradayAverageCurveResponse { series_key, results })
}

#[derive(serde::Serialize)]
struct IntradayAverageCurveResponse {
    series_key: String,
    results: Vec<Option<engine::IntradayAverage>>,
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
    resume_archive: bool,
) -> Result<actor::SessionCreation, String> {
    let seed = seed
        .parse::<u64>()
        .map_err(|error| format!("随机种子必须是 0..=u64::MAX 的十进制整数：{error}"))?;
    let manager = state.manager.clone();
    let creation = manager
        .new_session_with_archive(setup, seed, app, resume_archive)
        .await
        .map_err(map_session_error)?;
    Ok(creation)
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

#[tauri::command]
async fn submit_report_correction(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    request: engine::CompanyReportCorrection,
) -> Result<actor::GenerationResponse<()>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .submit_report_correction(parse_generation(generation)?, request)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn cancel_report_correction(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    operation_id: String,
) -> Result<actor::GenerationResponse<()>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .cancel_report_correction(parse_generation(generation)?, operation_id)
        .await
        .map_err(map_send_error)
}

#[tauri::command]
async fn query_report_corrections(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::GenerationResponse<engine::ReportCorrectionStatus>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .query_report_corrections(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
}

/// 取完整快照（首次连 / 重连 / 存档）。
#[tauri::command]
async fn snapshot(state: State<'_, DesktopState>, session_id: String) -> Result<Snapshot, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.snapshot().await.map_err(map_send_error)
}

#[tauri::command]
async fn engine_baseline(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::RestoreResult, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .query_baseline(parse_generation(generation)?)
        .await
        .map_err(map_send_error)
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
async fn query_stock_history(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    code: engine::StockCode,
) -> Result<actor::GenerationResponse<engine::session::HistoricalStockData>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.query_stock_history(parse_generation(generation)?, code).await.map_err(map_send_error)
}

#[tauri::command]
async fn personal_trade_confirmations(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    before_receipt: Option<String>,
) -> Result<serde_json::Value, String> {
    let before_receipt = before_receipt.map(|value| {
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) || (value.len() > 1 && value.starts_with('0')) {
            return Err("交割单 before_receipt 必须是规范u64字符串".to_owned());
        }
        value.parse::<u64>().map_err(|error| error.to_string())
    }).transpose()?;
    let handles = lookup_handles(&state, &session_id).await?;
    let response = handles
        .personal_trade_confirmations(parse_generation(generation)?, before_receipt)
        .await
        .map_err(map_send_error)?;
    Ok(serde_json::json!({ "generation": response.generation, "confirmations": response.value }))
}

#[tauri::command]
async fn personal_trade_history(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    query: engine::session::PersonalTradeHistoryRequest,
) -> Result<serde_json::Value, String> {
    let response = lookup_handles(&state, &session_id).await?
        .personal_trade_history(parse_generation(generation)?, query).await.map_err(map_send_error)?;
    Ok(serde_json::json!({ "generation": response.generation, "page": response.value }))
}

#[tauri::command]
async fn market_history(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    query: engine::session::MarketHistoryRequest,
) -> Result<serde_json::Value, String> {
    let response = lookup_handles(&state, &session_id).await?
        .market_history(parse_generation(generation)?, query).await.map_err(map_send_error)?;
    Ok(serde_json::json!({ "generation": response.generation, "page": response.value }))
}

#[tauri::command]
async fn current_minute_history(state: State<'_, DesktopState>, session_id: String, generation: String, query: engine::session::CurrentMinuteHistoryRequest) -> Result<serde_json::Value, String> {
    let response = lookup_handles(&state, &session_id).await?
        .current_minute_history(parse_generation(generation)?, query).await.map_err(map_send_error)?;
    Ok(serde_json::json!({ "generation": response.generation, "response": response.value }))
}

#[tauri::command]
async fn initial_allocation(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
) -> Result<actor::GenerationResponse<engine::session::InitialAllocation>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.initial_allocation(parse_generation(generation)?).await.map_err(map_send_error)
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
async fn public_report_availability(
    state: State<'_, DesktopState>,
    session_id: String,
    generation: String,
    query: PublicReportAvailabilityQuery,
) -> Result<actor::GenerationResponse<PublicReportAvailability>, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles.public_report_availability(parse_generation(generation)?, query).await.map_err(map_send_error)
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
    archive_slot_id: Option<String>,
) -> Result<actor::RestoreResult, String> {
    let handles = lookup_handles(&state, &session_id).await?;
    handles
        .restore_archive(parse_generation(generation)?, slot, archive_slot_id)
        .await
        .map_err(map_send_error)
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

#[tauri::command]
async fn archive_list(state: State<'_, DesktopState>, session_id: String) -> Result<Vec<native_store::ArchiveMetadata>, String> {
    lookup_handles(&state, &session_id).await?;
    state.manager.database().list().map_err(|error| error.to_string())
}

#[tauri::command]
async fn archive_load(state: State<'_, DesktopState>, session_id: String, slot_id: Option<String>) -> Result<Option<SaveSlot>, String> {
    lookup_handles(&state, &session_id).await?;
    let database = state.manager.database();
    let slot_id = match slot_id {
        Some(slot_id) => slot_id,
        None => match database.selection().map_err(|error| error.to_string())? {
            native_store::ArchiveSelection::Selected(slot_id) => slot_id,
            native_store::ArchiveSelection::Cleared | native_store::ArchiveSelection::Uninitialized => return Ok(None),
        },
    };
    database.load(&slot_id).map_err(|error| error.to_string())?.map(Some).ok_or_else(|| format!("日终存档槽 {slot_id} 不存在"))
}

#[tauri::command]
async fn archive_select(state: State<'_, DesktopState>, session_id: String, generation: String, slot_id: String) -> Result<bool, String> {
    lookup_handles(&state, &session_id).await?.select_archive(parse_generation(generation)?, slot_id).await.map_err(map_send_error)
}

#[tauri::command]
async fn archive_rename(state: State<'_, DesktopState>, session_id: String, generation: String, slot_id: String, name: String) -> Result<(), String> {
    match lookup_handles(&state, &session_id).await?.mutate_archive_metadata(parse_generation(generation)?, slot_id, name, false).await.map_err(map_send_error)? {
        None => Ok(()),
        Some(_) => Err("存档重命名返回了复制结果；请反馈 Native 协议错误".into()),
    }
}

#[tauri::command]
async fn archive_delete(state: State<'_, DesktopState>, session_id: String, generation: String, slot_id: String) -> Result<(), String> {
    lookup_handles(&state, &session_id).await?.delete_archive(parse_generation(generation)?, slot_id).await.map_err(map_send_error)
}

#[tauri::command]
async fn archive_copy(state: State<'_, DesktopState>, session_id: String, generation: String, slot_id: String, name: String) -> Result<native_store::ArchiveMetadata, String> {
    lookup_handles(&state, &session_id).await?.mutate_archive_metadata(parse_generation(generation)?, slot_id, name, true).await.map_err(map_send_error)?.ok_or_else(|| "存档复制未返回槽元数据；请反馈 Native 协议错误".into())
}

#[cfg(test)]
fn command_builder<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    command_builder_base(builder).manage(DesktopState::default())
}

#[cfg(not(feature = "host-parity"))]
fn command_builder_base<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            create_session,
            archive_list,
            archive_load,
            archive_select,
            archive_rename,
            archive_delete,
            archive_copy,
            submit_report_correction,
            cancel_report_correction,
            query_report_corrections,
            enqueue,
            snapshot,
            engine_baseline,
            host_capabilities,
            calculate_indicators,
            calculate_intraday_average,
            calculate_intraday_average_curve,
            player_working_orders,
            query_stock_history,
            personal_trade_confirmations,
            personal_trade_history,
            market_history,
            current_minute_history,
            initial_allocation,
            runtime_snapshot,
            civil_date,
            public_reports,
            public_report_availability,
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
fn command_builder_base<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            create_session,
            archive_list,
            archive_load,
            archive_select,
            archive_rename,
            archive_delete,
            archive_copy,
            submit_report_correction,
            cancel_report_correction,
            query_report_corrections,
            enqueue,
            snapshot,
            engine_baseline,
            host_capabilities,
            calculate_indicators,
            calculate_intraday_average,
            calculate_intraday_average_curve,
            player_working_orders,
            query_stock_history,
            personal_trade_confirmations,
            personal_trade_history,
            market_history,
            current_minute_history,
            initial_allocation,
            runtime_snapshot,
            civil_date,
            public_reports,
            public_report_availability,
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
    command_builder_base(tauri::Builder::default())
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("stock-market-game.sqlite");
            let database = native_store::NativeDatabase::open(&path)?;
            if !app.manage(DesktopState { manager: SessionManager::with_database(database) }) {
                return Err("DesktopState 重复初始化；拒绝替换正在运行的市场".into());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败（见上方错误）—— 不可恢复，显式崩溃。");
}

#[cfg(test)]
mod lib_tests;
