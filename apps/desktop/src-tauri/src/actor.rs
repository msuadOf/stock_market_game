//! actor-per-session（ADR-0005 §5，桌面端版）：每 session 一个 tokio task 独占 `GameSession`。
//!
//! 与 `apps/server/src/actor.rs` 同源，差异仅在「事件出口」：
//! - server：每 tick 产 Event[] → `broadcast` 给 N 个 WS 订阅者。
//! - **desktop**：每 tick 产 Event[] → `app.emit("engine-event", payload)` 单播给前端窗口。
//!
//! 设计要点（无锁、契合 engine `Send`）：
//! - **无共享可变状态、无锁**：`GameSession` 由 actor task 独占 own，外部经 **mpsc 命令**交互。
//! - **命令通道**（`tokio::sync::mpsc`）：投递 `SessionCommand`（入队意图 / 取快照 / 改速），
//!   每条带 `oneshot` 回执 → 调用方拿 `Result`。engine 失败显式上抛，绝不静默吞（铁律二）。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 当前单玩家模式：意图固定路由给玩家 `AccountId(0)`（见 `enqueue`）。

#[cfg(test)]
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "host-parity")]
use engine::session::protocol::CivilUpdate;
use engine::session::protocol::{EngineUpdate, PausePreferences, ProtocolSession, TickFrame};
use engine::{
    calendar::CivilDate,
    company::{PublicReportPage, PublicReportQuery, PublicReportSummary},
    session::HistoricalStockData, AccountId, Intent, SaveSlot, SessionError, SessionSetup, Snapshot, StockCode,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::{mpsc, oneshot};

use crate::EngineEventPayload;
mod failure;
#[cfg(test)]
mod fatal_tests;
#[cfg(test)]
mod protocol_tests;

/// 倍速基准：1x 时一个 tick 的间隔毫秒数。
/// 与前端 BASE_INTERVAL_MS(1000) 对齐：1x 时一个 tick = 游戏世界 1 秒。
pub const BASE_TICK_MS: u64 = 1000;

const FASTEST_BATCH_BUDGET: Duration = Duration::from_millis(14);
const FASTEST_BATCH_MAX_STEPS: usize = 100_000;
const UI_PUBLISH_INTERVAL: Duration = Duration::from_millis(16);
const SPEED_SAMPLE_MIN_DURATION: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RequestedSpeed {
    Fixed { multiplier: f64 },
    Fastest,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpeedMetrics {
    pub requested: RequestedSpeed,
    pub actual_multiplier: Option<f64>,
    pub sample_duration_ms: u64,
    pub sample_ticks: u64,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreResult {
    pub snapshot: Snapshot,
    pub timeline_id: String,
    pub generation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerationResponse<T> {
    pub generation: String,
    pub value: T,
}

fn diagnostic_generation_response(
    requested_generation: u64,
    current_generation: u64,
    read: impl FnOnce() -> engine::NpcDecisionDiagnostics,
) -> Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SessionError> {
    if requested_generation != current_generation {
        return Err(SessionError::InvalidSave(format!(
            "stale session generation {requested_generation}; current generation is {current_generation}"
        )));
    }
    Ok(GenerationResponse {
        generation: requested_generation.to_string(),
        value: read(),
    })
}

struct SpeedMeter {
    started_at: std::time::Instant,
    started_tick: u64,
    actual_multiplier: Option<f64>,
    sample_duration_ms: u64,
    sample_ticks: u64,
}

impl SpeedMeter {
    fn new(tick: u64) -> Self {
        Self {
            started_at: std::time::Instant::now(),
            started_tick: tick,
            actual_multiplier: None,
            sample_duration_ms: 0,
            sample_ticks: 0,
        }
    }

    fn reset(&mut self, tick: u64) {
        self.started_at = std::time::Instant::now();
        self.started_tick = tick;
        self.actual_multiplier = None;
        self.sample_duration_ms = 0;
        self.sample_ticks = 0;
    }

    fn mark_paused(&mut self, tick: u64) {
        self.reset(tick);
        self.actual_multiplier = Some(0.0);
    }

    fn refresh(&mut self, tick: u64) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.started_at);
        if elapsed < SPEED_SAMPLE_MIN_DURATION {
            return;
        }
        let ticks = tick.saturating_sub(self.started_tick);
        self.actual_multiplier = Some(ticks as f64 / elapsed.as_secs_f64());
        self.sample_duration_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        self.sample_ticks = ticks;
        self.started_at = now;
        self.started_tick = tick;
    }
}

/// actor 独占的宿主节奏状态；引擎事务与调度资源仍归 SessionActor。
struct DesktopPacing {
    running: bool,
    fastest: bool,
    requested_speed: RequestedSpeed,
    tick_interval: Duration,
    base_ms: u64,
    speed_meter: SpeedMeter,
}

impl DesktopPacing {
    fn new(base_ms: u64, tick: u64) -> Self {
        let mut speed_meter = SpeedMeter::new(tick);
        speed_meter.mark_paused(tick);
        Self {
            running: false,
            fastest: false,
            requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
            tick_interval: Duration::from_millis(base_ms),
            base_ms,
            speed_meter,
        }
    }

    fn apply_speed(&mut self, speed: f64, tick: u64) -> bool {
        if speed == f64::INFINITY {
            self.fastest = true;
            self.requested_speed = RequestedSpeed::Fastest;
            self.reset_speed_meter(tick);
            return true;
        }
        // 非法内部值由 actor 显式记录；节奏对象保持原状态。
        if !speed.is_finite() || speed <= 0.0 {
            return false;
        }
        self.fastest = false;
        self.requested_speed = RequestedSpeed::Fixed { multiplier: speed };
        self.reset_speed_meter(tick);
        self.tick_interval = fixed_tick_interval(self.base_ms, speed);
        true
    }

    fn set_running(&mut self, running: bool, tick: u64) {
        if self.running != running {
            self.running = running;
            self.reset_speed_meter(tick);
        }
    }

    fn pause_at_civil_boundary(&mut self, tick: u64) {
        self.running = false;
        self.reset_speed_meter(tick);
    }

    // fatal 停止保留已有采样，避免改变原先只写 running 的故障语义。
    fn stop_after_failure(&mut self) {
        self.running = false;
    }

    fn reset_after_restore(&mut self, tick: u64) {
        self.reset_speed_meter(tick);
    }

    fn reset_speed_meter(&mut self, tick: u64) {
        if self.running {
            self.speed_meter.reset(tick);
        } else {
            self.speed_meter.mark_paused(tick);
        }
    }

    fn refresh_metrics(&mut self, tick: u64) -> SpeedMetrics {
        self.speed_meter.refresh(tick);
        SpeedMetrics {
            requested: self.requested_speed.clone(),
            actual_multiplier: self.speed_meter.actual_multiplier,
            sample_duration_ms: self.speed_meter.sample_duration_ms,
            sample_ticks: self.speed_meter.sample_ticks,
            running: self.running,
        }
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn is_fastest(&self) -> bool {
        self.fastest
    }

    fn tick_interval(&self) -> Duration {
        self.tick_interval
    }
}

/// 发给 actor 的命令；控制回执表示 actor 已应用，不只是进入 mpsc。
///
/// `reply` 用 `Result` 而非裸值：engine 失败（`SessionError`）显式上抛，绝不静默吞（铁律二）。
#[derive(Debug)]
pub enum SessionCommand {
    /// 入队玩家意图（当前固定玩家 0）。Ok=已入队，Err=engine 拒绝。
    Enqueue {
        player_id: AccountId,
        intent: Intent,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 取完整快照。
    Snapshot { reply: oneshot::Sender<Snapshot> },
    PlayerWorkingOrders {
        generation: u64,
        reply: oneshot::Sender<Result<GenerationResponse<serde_json::Value>, SessionError>>,
    },
    StockHistory {
        generation: u64,
        code: StockCode,
        reply: oneshot::Sender<Result<GenerationResponse<HistoricalStockData>, SessionError>>,
    },
    QueryBaseline {
        generation: u64,
        reply: oneshot::Sender<Result<RestoreResult, SessionError>>,
    },
    /// 取不含 360 日历史的轻量运行快照，供高倍率跨日同步。
    RuntimeSnapshot { reply: oneshot::Sender<Snapshot> },
    CivilDate {
        generation: u64,
        reply: oneshot::Sender<Result<GenerationResponse<CivilDate>, SessionError>>,
    },
    PublicReports {
        generation: u64,
        query: PublicReportQuery,
        reply: oneshot::Sender<Result<GenerationResponse<PublicReportPage>, SessionError>>,
    },
    PublicReportById {
        generation: u64,
        id: String,
        reply: oneshot::Sender<Result<GenerationResponse<PublicReportSummary>, SessionError>>,
    },
    NpcDecisionDiagnostics {
        generation: u64,
        account: AccountId,
        reply: oneshot::Sender<
            Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SessionError>,
        >,
    },
    /// 读取设定速度与最近完成的实际 tick/现实秒采样。
    SpeedMetrics {
        reply: oneshot::Sender<SpeedMetrics>,
    },
    /// 生成可持久化存档。actor 独占会话，因此读取与 step 严格串行。
    Save {
        generation: u64,
        candidate: Option<engine::session::protocol::SaveCandidateKey>,
        reply: oneshot::Sender<Result<SaveSlot, SessionError>>,
    },
    /// 原子恢复存档：只有完整校验和重建成功后才替换当前会话。
    Restore {
        generation: u64,
        slot: Box<SaveSlot>,
        reply: oneshot::Sender<Result<RestoreResult, SessionError>>,
    },
    #[cfg(feature = "host-parity")]
    AdvanceCivilDay {
        generation: u64,
        reply: oneshot::Sender<Result<CivilUpdate, SessionError>>,
    },
    #[cfg(feature = "host-parity")]
    Step {
        generation: u64,
        reply: oneshot::Sender<Result<EngineUpdate, SessionError>>,
    },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed {
        speed: f64,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 暂停或恢复步进，保留会话状态与事件订阅。
    SetRunning {
        running: bool,
        reply: oneshot::Sender<()>,
    },
    SetPausePreferences {
        preferences: PausePreferences,
        reply: oneshot::Sender<()>,
    },
    /// 永久结束 actor；用于页面卸载/应用退出释放资源。
    Shutdown { reply: oneshot::Sender<()> },
}

/// 一个 session 的对外句柄：命令发送端（克隆廉价）。
/// `SessionManager` 持 `Arc<SessionHandles>`，Tauri command 经 manager 取克隆与 actor 通信。
/// 原始命令发送端不可由外部访问，命令必须通过受控方法提交。
/// ```compile_fail,E0616
/// fn bypass(handles: &stock_market_game_lib::actor::SessionHandles) {
///     let _ = handles.cmd_tx.clone();
/// }
/// ```
#[derive(Clone)]
pub struct SessionHandles {
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
}

impl SessionHandles {
    /// 便捷：入队玩家意图（固定 `AccountId(0)`）。把 `oneshot` 收发封装成 `Result`。
    pub async fn enqueue(&self, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(AccountId(0), intent).await
    }

    /// 入队指定玩家意图（联机多账户预留，当前由 player 0 调用）。
    pub async fn enqueue_as(
        &self,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Enqueue {
                player_id,
                intent,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// 取完整快照。actor 关闭 → `ActorGone`（绝不静默返回空快照）。
    pub async fn snapshot(&self) -> Result<Snapshot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Snapshot { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn player_working_orders(
        &self,
        generation: u64,
    ) -> Result<GenerationResponse<serde_json::Value>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PlayerWorkingOrders {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn query_stock_history(
        &self,
        generation: u64,
        code: StockCode,
    ) -> Result<GenerationResponse<HistoricalStockData>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(SessionCommand::StockHistory { generation, code, reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn query_baseline(&self, generation: u64) -> Result<RestoreResult, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::QueryBaseline {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// 取轻量运行快照，避免跨日反复复制全部历史 K 线。
    pub async fn runtime_snapshot(&self) -> Result<Snapshot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::RuntimeSnapshot { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn civil_date(
        &self,
        generation: u64,
    ) -> Result<GenerationResponse<CivilDate>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CivilDate {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn public_reports(
        &self,
        generation: u64,
        query: PublicReportQuery,
    ) -> Result<GenerationResponse<PublicReportPage>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReports {
                generation,
                query,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn public_report_by_id(
        &self,
        generation: u64,
        id: String,
    ) -> Result<GenerationResponse<PublicReportSummary>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReportById {
                generation,
                id,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn npc_decision_diagnostics(
        &self,
        generation: u64,
        account: AccountId,
    ) -> Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::NpcDecisionDiagnostics {
                generation,
                account,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn speed_metrics(&self) -> Result<SpeedMetrics, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SpeedMetrics { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    /// Captures only the latest completed day when no key is supplied, or the exact
    /// completed-day candidate named by a CivilUpdate key; it never synthesizes a save.
    pub async fn save(
        &self,
        generation: u64,
        candidate: Option<engine::session::protocol::SaveCandidateKey>,
    ) -> Result<SaveSlot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Save {
                generation,
                candidate,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn restore(
        &self,
        generation: u64,
        slot: SaveSlot,
    ) -> Result<RestoreResult, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Restore {
                generation,
                slot: Box::new(slot),
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    #[cfg(feature = "host-parity")]
    pub async fn advance_civil_day(
        &self,
        generation: u64,
    ) -> Result<CivilUpdate, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::AdvanceCivilDay {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    #[cfg(feature = "host-parity")]
    pub async fn step(&self, generation: u64) -> Result<EngineUpdate, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Step {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// 改变倍速并等待 actor 应用；actor 已关闭则 `ActorGone`。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied
            .await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn set_running(&self, running: bool) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetRunning { running, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn shutdown(&self) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Shutdown { reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn set_pause_preferences(
        &self,
        preferences: PausePreferences,
    ) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetPausePreferences { preferences, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied.await.map_err(|_| SendCommandError::ActorGone)
    }
}

/// 命令投递失败（actor 已退出 / engine 拒绝意图）。显式可见，不静默吞。
#[derive(Debug, thiserror::Error)]
pub enum SendCommandError {
    /// actor task 已退出（session 被 drop 或 panic）。
    #[error("会话不存在或已退出（命令通道关闭）")]
    ActorGone,
    /// engine 拒绝意图（如未知玩家；单玩家正常路径不应触发，但显式上抛）。
    #[error("引擎拒绝指令：{0}")]
    Rejected(String),
}

/// Session 注册表：`DashMap<session_id, Arc<SessionHandles>>` 不便引入（桌面端无 dashmap 依赖），
/// 这里用 `tokio::sync::Mutex<HashMap>` 替代——写少（仅 create），读多但无热路径竞争，足够。
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<SessionHandles>>>>,
    base_ms: u64,
}

impl SessionManager {
    /// 默认基准（`BASE_TICK_MS`）。
    pub fn default_base() -> Self {
        Self {
            sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            base_ms: BASE_TICK_MS,
        }
    }

    /// 创建新 session：构造 `GameSession` → 建 mpsc → spawn actor task → 注册。
    ///
    /// 失败显式返回 `SessionError`（构造非法参数），绝不静默吞（铁律二）。
    /// `app` 传入供 actor `emit` 事件给前端。
    pub async fn new_session<R: Runtime>(
        &self,
        setup: SessionSetup,
        seed: u64,
        app: AppHandle<R>,
    ) -> Result<String, SessionError> {
        let game = ProtocolSession::new(setup, seed)?;
        let session_id = uuid::Uuid::new_v4().to_string();

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let handles = Arc::new(SessionHandles { cmd_tx });

        // 异步锁：Tauri command 在 Tokio runtime 内调用，禁止 blocking_lock 导致 panic。
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), handles.clone());

        let actor = SessionActor {
            #[cfg(test)]
            injected_step_failure: None,
            pacing: DesktopPacing::new(self.base_ms, game.tick()),
            game,
            cmd_rx,
            session_id: session_id.clone(),
            app,
            last_fixed_publish: Instant::now(),
            timeline_id: session_id.clone(),
            generation: 1,
            pause_preferences: PausePreferences::default(),
        };
        tokio::spawn(actor.run());
        Ok(session_id)
    }

    /// 查询 session 句柄（克隆 `Arc<SessionHandles>`）。未知返回 `None`（不静默）。
    pub async fn lookup(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions.lock().await.get(session_id).map(Arc::clone)
    }

    /// 从注册表移除会话并返回最后一个管理句柄，供 shutdown 命令结束 actor。
    pub async fn remove(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions.lock().await.remove(session_id)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::default_base()
    }
}

/// actor：独占 `GameSession` 的 tokio task。命令经 `cmd_rx`，事件经 `app.emit`。
struct SessionActor<R: Runtime> {
    #[cfg(test)]
    injected_step_failure: Option<(usize, engine::session::StepFatal)>,
    pacing: DesktopPacing,
    game: ProtocolSession,
    cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    session_id: String,
    /// Tauri 应用句柄：emit 事件给前端窗口。
    app: AppHandle<R>,
    last_fixed_publish: Instant,
    /// 每次成功读档都会更换；前端据此拒绝晚到的旧时间线 IPC。
    timeline_id: String,
    generation: u64,
    pause_preferences: PausePreferences,
}

#[cfg(test)]
struct ActorHarness {
    app: tauri::App<tauri::test::MockRuntime>,
    actor: SessionActor<tauri::test::MockRuntime>,
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    events_rx: Option<mpsc::UnboundedReceiver<String>>,
    failures_rx: Option<mpsc::UnboundedReceiver<String>>,
}

#[cfg(test)]
impl ActorHarness {
    fn new_protocol_actor(
        game: ProtocolSession,
        fastest: bool,
        pause_preferences: PausePreferences,
        session_id: &str,
        timeline_id: &str,
    ) -> Self {
        let app = tauri::test::mock_app();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let actor = SessionActor {
            injected_step_failure: None,
            pacing: DesktopPacing {
                running: true,
                fastest,
                requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
                tick_interval: Duration::from_millis(1),
                base_ms: 1,
                speed_meter: SpeedMeter::new(game.tick()),
            },
            game,
            cmd_rx,
            session_id: session_id.to_owned(),
            app: app.handle().clone(),
            last_fixed_publish: Instant::now(),
            timeline_id: timeline_id.to_owned(),
            generation: 1,
            pause_preferences,
        };
        Self {
            app,
            actor,
            cmd_tx,
            events_rx: None,
            failures_rx: None,
        }
    }

    fn subscribe_engine_events(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        if self.events_rx.is_none() {
            self.events_rx = Some(self.subscribe(crate::ENGINE_EVENT_NAME));
        }
        self.events_rx
            .as_mut()
            .expect("engine-event 订阅必须已建立")
    }

    fn subscribe_failures(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        if self.failures_rx.is_none() {
            self.failures_rx = Some(self.subscribe("engine-failure"));
        }
        self.failures_rx
            .as_mut()
            .expect("engine-failure 订阅必须已建立")
    }

    fn subscribe(&self, event_name: &str) -> mpsc::UnboundedReceiver<String> {
        use tauri::Listener;

        let (sender, receiver) = mpsc::unbounded_channel();
        self.app.listen(event_name, move |event| {
            sender.send(event.payload().to_owned()).unwrap();
        });
        receiver
    }
}

impl<R: Runtime> SessionActor<R> {
    fn step_game(&mut self) -> Result<TickFrame, engine::session::StepFatal> {
        #[cfg(test)]
        if let Some((remaining, error)) = &mut self.injected_step_failure {
            if *remaining == 0 {
                return Err(error.clone());
            }
            *remaining -= 1;
        }
        self.game.step_frame()
    }
    /// 主循环：`select!` 同时等命令与 interval tick。
    ///
    /// - 命令到达 → 处理（Enqueue→入队、Snapshot→回快照、SetSpeed→改 interval）。
    /// - interval 到 → `step()` → `app.emit("engine-event", ...)`。
    /// - `cmd_rx` 关闭（所有句柄 drop）→ 退出，task 结束。
    async fn run(mut self) {
        let mut interval = self.fresh_interval();

        loop {
            tokio::select! {
                biased; // 优先消费命令，避免被高频 step 饿死控制路径。

                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(c) => {
                            let pacing_changed = matches!(c, SessionCommand::SetSpeed { .. } | SessionCommand::SetRunning { .. });
                            let shutting_down = matches!(c, SessionCommand::Shutdown { .. });
                            self.handle_command(c).await;
                            if shutting_down {
                                break;
                            }
                            if pacing_changed {
                                interval = self.fresh_interval();
                            }
                        }
                        None => break,
                    }
                }
                _ = interval.tick(), if !self.pacing.is_fastest() && self.pacing.is_running() => {
                    self.tick_and_emit().await;
                    if self.cmd_rx.is_closed() { break; }
                }
                _ = tokio::task::yield_now(), if self.pacing.is_fastest() && self.pacing.is_running() => {
                    self.run_fastest_batch().await;
                    if self.cmd_rx.is_closed() { break; }
                }
            }
        }
    }

    /// 构造固定倍率计时器（Skip 积压补发）。
    fn fresh_interval(&self) -> tokio::time::Interval {
        let period = self.pacing.tick_interval().max(UI_PUBLISH_INTERVAL);
        let mut i = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        i
    }

    /// 推进一个 tick 并把产出的事件 emit 给前端。
    ///
    /// emit 失败（窗口已关闭等）不算致命——actor 继续；下一 tick 自然不再有消费者。
    /// 这里不 panic：游戏循环与 UI 解耦，UI 关闭应允许循环自然结束（cmd_tx drop 后退出）。
    async fn tick_and_emit(&mut self) {
        let interval = self.pacing.tick_interval();
        let due = (self.last_fixed_publish.elapsed().as_nanos() / interval.as_nanos())
            .clamp(1, FASTEST_BATCH_MAX_STEPS as u128) as usize;
        let before = self.game.tick();
        self.run_cycle(due);
        let completed = self.game.tick().saturating_sub(before);
        self.last_fixed_publish += interval.saturating_mul(completed as u32);
    }

    fn run_cycle(&mut self, limit: usize) {
        if self.cmd_rx.is_closed() {
            return;
        }
        let checkpoint = match self.game.checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                self.stop_after_step_failure(error);
                return;
            }
        };
        let started = std::time::Instant::now();
        let mut frames = Vec::new();
        let mut steps = 0;
        while steps < limit && {
            #[cfg(test)]
            let injecting = self.injected_step_failure.is_some();
            #[cfg(not(test))]
            let injecting = false;
            injecting || started.elapsed() < FASTEST_BATCH_BUDGET
        } {
            // Process a command received during tick n before starting tick n+1.
            if limit > 1 && !self.cmd_rx.is_empty() {
                break;
            }
            match self.game.civil_day_ready() {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    self.rollback_cycle(checkpoint, error);
                    return;
                }
            }
            let frame = match self.step_game() {
                Ok(frame) => frame,
                Err(error) => {
                    self.rollback_cycle(checkpoint, error.into());
                    return;
                }
            };
            frames.push(frame);
            steps += 1;
        }
        if let Err(error) = self.publish_protocol_cycle(frames) {
            self.rollback_cycle(checkpoint, error);
            return;
        }
        self.pacing.refresh_metrics(self.game.tick());
    }

    async fn run_fastest_batch(&mut self) {
        self.run_cycle(FASTEST_BATCH_MAX_STEPS);
    }

    fn rollback_cycle(
        &mut self,
        checkpoint: engine::session::protocol::ProtocolCheckpoint,
        error: SessionError,
    ) {
        self.game.rollback(checkpoint);
        self.stop_after_host_failure(failure::HostFailure::civil(error));
    }

    fn publish_protocol_cycle(&mut self, frames: Vec<TickFrame>) -> Result<(), SessionError> {
        let batch = if frames.is_empty() {
            None
        } else {
            Some(self.game.tick_batch(frames)?)
        };
        let updates = self.prepare_civil_updates()?;
        if let Some(batch) = batch {
            self.emit_update(EngineUpdate::TickBatch(Box::new(batch)));
        }
        for update in updates {
            let pause = self.pause_preferences.pauses(&update);
            self.emit_update(EngineUpdate::CivilUpdate(Box::new(update)));
            if pause {
                self.pacing.pause_at_civil_boundary(self.game.tick());
            }
        }
        Ok(())
    }

    fn emit_update(&mut self, update: EngineUpdate) {
        let payload = EngineEventPayload {
            session_id: self.session_id.clone(),
            timeline_id: self.timeline_id.clone(),
            update,
        };
        // emit 同步；payload 序列化失败仅在结构不可序列化时（engine::Event 始终可序列化），属不变量。
        if let Err(e) = self.app.emit(crate::ENGINE_EVENT_NAME, payload) {
            eprintln!(
                "[session {}] emit 事件失败（前端可能已关闭）：{e}",
                self.session_id
            );
        }
    }

    /// 处理单条命令。
    async fn handle_command(&mut self, cmd: SessionCommand) {
        if self.cmd_rx.is_closed() {
            return;
        }
        match cmd {
            SessionCommand::Enqueue {
                player_id,
                intent,
                reply,
            } => {
                let res = self.game.enqueue_player_intent(player_id, intent);
                // reply 失败仅说明调用方已放弃等待——不属错误。
                let _ = reply.send(res);
            }
            SessionCommand::Snapshot { reply } => {
                let snap = self.game.snapshot();
                let _ = reply.send(snap);
            }
            SessionCommand::PlayerWorkingOrders { generation, reply } => {
                let orders =
                    serde_json::to_value(self.game.player_working_orders()).map_err(|error| {
                        SessionError::InvalidSave(format!(
                            "serialize player working orders: {error}"
                        ))
                    });
                let result = self.generation_response(generation, orders);
                let _ = reply.send(result);
            }
            SessionCommand::StockHistory { generation, code, reply } => {
                let result = if generation != self.generation {
                    self.generation_response(generation, Err(SessionError::InvalidSave(format!(
                        "stale session generation {generation}; current generation is {}",
                        self.generation
                    ))))
                } else {
                    let queried = self.game.query_stock_history(AccountId(0), &code);
                    self.generation_response(generation, queried)
                };
                let _ = reply.send(result);
            }
            SessionCommand::QueryBaseline { generation, reply } => {
                let result = if generation != self.generation {
                    Err(SessionError::InvalidSave(format!(
                        "stale session generation {generation}; current generation is {}",
                        self.generation
                    )))
                } else {
                    self.game.prepare_public_baseline();
                    Ok(RestoreResult {
                        snapshot: self.game.snapshot(),
                        timeline_id: self.timeline_id.clone(),
                        generation: self.generation.to_string(),
                    })
                };
                let _ = reply.send(result);
            }
            SessionCommand::RuntimeSnapshot { reply } => {
                let snap = self.game.runtime_snapshot();
                let _ = reply.send(snap);
            }
            SessionCommand::CivilDate { generation, reply } => {
                let _ =
                    reply.send(self.generation_response(generation, Ok(self.game.civil_date())));
            }
            SessionCommand::PublicReports {
                generation,
                query,
                reply,
            } => {
                let _ = reply.send(
                    self.generation_response(generation, self.game.query_public_reports(&query)),
                );
            }
            SessionCommand::PublicReportById {
                generation,
                id,
                reply,
            } => {
                let _ = reply
                    .send(self.generation_response(generation, self.game.public_report_by_id(id)));
            }
            SessionCommand::NpcDecisionDiagnostics {
                generation,
                account,
                reply,
            } => {
                let _ = reply.send(diagnostic_generation_response(
                    generation,
                    self.generation,
                    || self.game.npc_decision_diagnostics(account),
                ));
            }
            SessionCommand::SpeedMetrics { reply } => {
                let metrics = self.pacing.refresh_metrics(self.game.tick());
                let _ = reply.send(metrics);
            }
            SessionCommand::Save {
                generation,
                candidate,
                reply,
            } => {
                let result = self.generation_response(generation, Ok(())).and_then(|_| {
                    if let Some(key) = candidate.as_ref() {
                        self.game.save_candidate(key)
                    } else {
                        self.game.save()
                    }
                });
                let _ = reply.send(result);
            }
            SessionCommand::Restore {
                generation,
                slot,
                reply,
            } => match self.restore(generation, slot) {
                Ok(restored) => {
                    let _ = reply.send(Ok(restored));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            #[cfg(feature = "host-parity")]
            SessionCommand::AdvanceCivilDay { generation, reply } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map(|_| ())
                    .and_then(|()| self.game.end_civil_day_update());
                if let Ok(report) = &result {
                    self.emit_update(EngineUpdate::CivilUpdate(Box::new(report.clone())));
                    if self.pause_preferences.pauses(report) {
                        self.pacing.pause_at_civil_boundary(self.game.tick());
                    }
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.stop_after_step_failure(fatal.clone());
                }
                let _ = reply.send(result);
            }
            #[cfg(feature = "host-parity")]
            SessionCommand::Step { generation, reply } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .and_then(|_| self.game.civil_day_ready())
                    .and_then(|ready| {
                        if ready {
                            return Err(SessionError::InvalidSave(
                                "civil day barrier must be published before stepping".into(),
                            ));
                        }
                        self.game
                            .step_frame()
                            .and_then(|frame| self.game.tick_batch(vec![frame]))
                            .map(|batch| EngineUpdate::TickBatch(Box::new(batch)))
                            .map_err(SessionError::from)
                    });
                if let Ok(events) = &result {
                    self.emit_update(events.clone());
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.stop_after_step_failure(fatal.clone());
                }
                let _ = reply.send(result);
            }
            SessionCommand::SetSpeed { speed, reply } => {
                let result = self.apply_speed(speed);
                let _ = reply.send(result);
            }
            SessionCommand::SetRunning { running, reply } => {
                self.pacing.set_running(running, self.game.tick());
                self.last_fixed_publish = Instant::now();
                let _ = reply.send(());
            }
            SessionCommand::SetPausePreferences { preferences, reply } => {
                self.pause_preferences = preferences;
                let _ = reply.send(());
            }
            SessionCommand::Shutdown { reply } => {
                let _ = reply.send(());
            }
        }
    }

    /// 应用新倍速：固定倍率使用精确 Duration；Fastest 切到 CPU 时间片 tight-loop。
    fn apply_speed(&mut self, speed: f64) -> Result<(), SessionError> {
        if !self.pacing.apply_speed(speed, self.game.tick()) {
            return Err(SessionError::InvalidSave(format!("非法 speed: {speed}")));
        }
        if !self.pacing.is_fastest() {
            self.last_fixed_publish = Instant::now();
        }
        Ok(())
    }

    fn generation_response<T>(
        &self,
        generation: u64,
        value: Result<T, SessionError>,
    ) -> Result<GenerationResponse<T>, SessionError> {
        if generation != self.generation {
            return Err(SessionError::InvalidSave(format!(
                "stale session generation {generation}; current generation is {}",
                self.generation
            )));
        }
        value.map(|value| GenerationResponse {
            generation: generation.to_string(),
            value,
        })
    }

    fn restore(
        &mut self,
        generation: u64,
        slot: Box<SaveSlot>,
    ) -> Result<RestoreResult, SessionError> {
        self.generation_response(generation, Ok(()))?;
        let restored = ProtocolSession::restore(&slot)?;
        let next_generation = self.generation.checked_add(1).ok_or_else(|| {
            SessionError::InvalidSave(
                "session generation exhausted; create a new session".to_owned(),
            )
        })?;
        self.game = restored;
        self.game.prepare_public_baseline();
        self.timeline_id = uuid::Uuid::new_v4().to_string();
        self.generation = next_generation;
        self.pacing.reset_after_restore(self.game.tick());
        Ok(RestoreResult {
            snapshot: self.game.snapshot(),
            timeline_id: self.timeline_id.clone(),
            generation: self.generation.to_string(),
        })
    }

    fn prepare_civil_updates(
        &mut self,
    ) -> Result<Vec<engine::session::protocol::CivilUpdate>, SessionError> {
        let mut updates = Vec::new();
        while self.game.civil_day_ready()? {
            let update = self.game.end_civil_day_update()?;
            let pause = self.pause_preferences.pauses(&update);
            updates.push(update);
            if pause {
                break;
            }
        }
        Ok(updates)
    }
}

#[cfg(test)]
fn take_publish_batch<T>(
    pending: &mut Vec<T>,
    incoming: Vec<T>,
    elapsed: Duration,
) -> Option<Vec<T>> {
    pending.extend(incoming);
    (elapsed >= UI_PUBLISH_INTERVAL && !pending.is_empty()).then(|| std::mem::take(pending))
}

#[cfg(test)]
fn compact_fastest_events(
    events: Vec<engine::Event>,
    ticks_per_day: u64,
    auction_ticks: u64,
) -> Vec<engine::Event> {
    if events.is_empty() {
        return events;
    }
    let mut keep = HashSet::new();
    let mut trade_indices = Vec::new();
    let mut active_minute_ticks: HashMap<(engine::StockCode, u64), usize> = HashMap::new();
    let mut auction_slot_ticks: HashMap<(engine::StockCode, u64), usize> = HashMap::new();
    let safe_ticks_per_day = ticks_per_day.max(1);

    for (index, event) in events.iter().enumerate() {
        match event {
            engine::Event::DayBoundary { .. } => {
                keep.insert(index);
                active_minute_ticks.clear();
                auction_slot_ticks.clear();
            }
            engine::Event::AuctionTick { tick, code, .. } => {
                let day_tick = (tick.saturating_sub(1)) % safe_ticks_per_day;
                auction_slot_ticks.insert((code.clone(), day_tick / 6), index);
            }
            engine::Event::PriceTick { tick, code, .. } => {
                let day_tick = (tick.saturating_sub(1)) % safe_ticks_per_day;
                active_minute_ticks.insert(
                    (code.clone(), day_tick.saturating_sub(auction_ticks) / 60),
                    index,
                );
            }
            engine::Event::Trade { .. } => trade_indices.push(index),
            _ => {
                keep.insert(index);
            }
        }
    }
    for index in trade_indices.into_iter().rev().take(100) {
        keep.insert(index);
    }
    keep.extend(active_minute_ticks.into_values());
    keep.extend(auction_slot_ticks.into_values());

    events
        .into_iter()
        .enumerate()
        .filter_map(|(index, event)| keep.contains(&index).then_some(event))
        .collect()
}

fn fixed_tick_interval(base_ms: u64, speed: f64) -> Duration {
    let seconds = (base_ms as f64 / 1000.0 / speed).max(0.000_001);
    Duration::from_secs_f64(seconds)
}

#[cfg(test)]
mod tests {
    use super::{
        compact_fastest_events, diagnostic_generation_response, fixed_tick_interval,
        take_publish_batch, SessionManager, SpeedMeter,
    };
    use engine::{
        AccountId, DailyCandle, DailyTradeStats, Event, FloatAllocation, GameConfig, HotParams,
        InstParams, Money, NpcDecisionDiagnostics, NpcSetup, RetailParams, SecurityCategory,
        SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    pub(super) fn diagnostic_setup() -> SessionSetup {
        SessionSetup {
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![StockSpec {
                code: StockCode("600101".to_owned()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 100_000,
                float_shares: 100_000,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 1,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.8,
                    order_size_mean: 200,
                    chase_prob: 0.4,
                },
                inst: InstParams {
                    margin: 0.02,
                    order_size: 500,
                },
                hot: HotParams {
                    lookback: 3,
                    trend_threshold: 0.01,
                    order_size: 300,
                },
            },
            ticks_per_day: 30,
            auction_ticks: 0,
            closing_auction_ticks: 0,
            history_len: 20,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
            start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        }
    }

    #[test]
    fn stale_diagnostics_generation_does_not_invoke_private_reader() {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reads);

        let result = diagnostic_generation_response(0, 1, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            NpcDecisionDiagnostics::Unsupported
        });

        assert!(result.is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn diagnostics_reject_stale_generation_without_private_records() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();

        let result = handles.npc_decision_diagnostics(0, AccountId(1)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[cfg(not(feature = "simulation-diagnostics"))]
    async fn release_diagnostics_returns_unsupported_without_records() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();

        let response = handles
            .npc_decision_diagnostics(1, AccountId(1))
            .await
            .unwrap();

        assert_eq!(response.value, NpcDecisionDiagnostics::Unsupported);
        assert!(serde_json::to_value(response.value)
            .unwrap()
            .get("records")
            .is_none());
    }

    #[tokio::test]
    #[cfg(feature = "simulation-diagnostics")]
    async fn feature_diagnostics_returns_supported_records_for_current_generation() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();

        let response = handles
            .npc_decision_diagnostics(1, AccountId(1))
            .await
            .unwrap();

        assert!(
            matches!(response.value, NpcDecisionDiagnostics::Supported { records } if records.is_empty())
        );
    }

    #[test]
    fn fixed_speed_intervals_keep_fractional_milliseconds() {
        assert_eq!(fixed_tick_interval(1000, 60.0).as_nanos(), 16_666_667);
        assert_eq!(fixed_tick_interval(1000, 180.0).as_nanos(), 5_555_556);
        assert_eq!(fixed_tick_interval(1000, 720.0).as_nanos(), 1_388_889);
    }

    #[test]
    fn fixed_high_speed_ticks_cross_ipc_as_one_sixteen_ms_batch() {
        let mut pending = Vec::new();
        for tick in 1..=11 {
            assert_eq!(
                take_publish_batch(&mut pending, vec![tick], Duration::from_millis(15)),
                None
            );
        }
        assert_eq!(
            take_publish_batch(&mut pending, vec![12], Duration::from_millis(16)),
            Some((1..=12).collect())
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn desktop_speed_meter_reports_authoritative_ticks_per_real_second() {
        let mut meter = SpeedMeter::new(10);
        meter.started_at -= Duration::from_secs(2);

        meter.refresh(12);

        assert_eq!(meter.sample_ticks, 2);
        assert!(meter.sample_duration_ms >= 2_000);
        let actual = meter.actual_multiplier.expect("两秒窗口应产生实际倍率");
        assert!(
            (actual - 1.0).abs() < 0.01,
            "实际倍率应约为 1x，收到 {actual}"
        );
    }

    #[test]
    fn pacing_keeps_speed_mode_interval_and_pause_metrics_consistent() {
        let mut pacing = super::DesktopPacing::new(1_000, 10);
        assert!(!pacing.is_running());
        assert_eq!(pacing.refresh_metrics(10).actual_multiplier, Some(0.0));

        pacing.set_running(true, 10);
        pacing.apply_speed(60.0, 10);
        let fixed_interval = pacing.tick_interval();
        assert!(!pacing.is_fastest());
        assert_eq!(
            pacing.refresh_metrics(10).requested,
            super::RequestedSpeed::Fixed { multiplier: 60.0 }
        );
        pacing.apply_speed(f64::INFINITY, 11);
        assert!(pacing.is_fastest());
        assert_eq!(pacing.tick_interval(), fixed_interval);
        assert_eq!(
            pacing.refresh_metrics(11).requested,
            super::RequestedSpeed::Fastest
        );
        pacing.apply_speed(2.0, 12);
        assert!(!pacing.is_fastest());
        assert_eq!(pacing.tick_interval(), Duration::from_millis(500));

        pacing.speed_meter.started_at -= Duration::from_secs(2);
        let sampled = pacing.refresh_metrics(16);
        assert_eq!(sampled.sample_ticks, 4);
        pacing.set_running(true, 16);
        assert_eq!(pacing.refresh_metrics(16), sampled);
        pacing.set_running(false, 16);
        let paused = pacing.refresh_metrics(16);
        assert!(!paused.running);
        assert_eq!(paused.actual_multiplier, Some(0.0));
        assert_eq!(paused.sample_ticks, 0);
        pacing.set_running(true, 16);
        assert_eq!(pacing.refresh_metrics(16).actual_multiplier, None);
    }

    #[test]
    fn pacing_restore_resets_sampling_but_fatal_stop_preserves_it() {
        let mut pacing = super::DesktopPacing::new(1_000, 10);
        pacing.set_running(true, 10);
        pacing.speed_meter.started_at -= Duration::from_secs(2);
        let sampled = pacing.refresh_metrics(14);
        pacing.stop_after_failure();
        let stopped = pacing.refresh_metrics(14);
        assert!(!stopped.running);
        assert_eq!(stopped.actual_multiplier, sampled.actual_multiplier);
        assert_eq!(stopped.sample_ticks, sampled.sample_ticks);
        pacing.reset_after_restore(3);
        let restored = pacing.refresh_metrics(3);
        assert_eq!(restored.actual_multiplier, Some(0.0));
        assert_eq!(restored.sample_ticks, 0);
        pacing.set_running(true, 3);
        pacing.reset_after_restore(1);
        assert_eq!(pacing.refresh_metrics(1).actual_multiplier, None);
        assert_eq!(pacing.speed_meter.started_tick, 1);
    }

    #[test]
    fn desktop_pacing_rejects_invalid_internal_speed_without_changing_state() {
        let mut pacing = super::DesktopPacing::new(1_000, 10);
        pacing.set_running(true, 10);
        pacing.apply_speed(2.0, 10);
        let before = pacing.refresh_metrics(10);
        for invalid in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY] {
            assert!(!pacing.apply_speed(invalid, 11));
            assert_eq!(pacing.refresh_metrics(10), before);
            assert_eq!(pacing.tick_interval(), Duration::from_millis(500));
        }
        pacing.apply_speed(1e12, 10);
        assert_eq!(pacing.tick_interval(), Duration::from_micros(1));
    }

    #[test]
    fn fastest_compaction_keeps_boundaries_and_latest_tick_per_minute() {
        let price_tick = |seq, tick| Event::PriceTick {
            seq,
            tick,
            code: StockCode("AAA".into()),
            last_price: Money::from_cents(1_000_000_000),
            daily_candle: DailyCandle {
                time: 0,
                open: Money::from_cents(1_000_000_000),
                high: Money::from_cents(1_000_000_000),
                low: Money::from_cents(1_000_000_000),
                close: Money::from_cents(1_000_000_000),
                volume: 9_007_200,
                trade_stats: Some(DailyTradeStats {
                    turnover_cents: 9_007_200_000_000_000,
                    trade_count: 7,
                }),
            },
            bids: Vec::new(),
            asks: Vec::new(),
        };
        let boundary = Event::DayBoundary {
            seq: 3,
            day: 1,
            closed_daily_candles: Default::default(),
        };
        let compacted = compact_fastest_events(
            vec![
                price_tick(1, 1),
                price_tick(2, 59),
                boundary,
                price_tick(4, 61),
            ],
            14_400,
            0,
        );
        assert_eq!(compacted.len(), 2);
        assert!(matches!(compacted[0], Event::DayBoundary { seq: 3, .. }));
        let Event::PriceTick { daily_candle, .. } = &compacted[1] else {
            panic!("最后一个压缩事件应为 PriceTick");
        };
        let stats = daily_candle
            .trade_stats
            .as_ref()
            .expect("桌面 IPC 压缩必须保留累计成交统计");
        assert_eq!(stats.turnover_cents, 9_007_200_000_000_000);
        assert_eq!(stats.trade_count, 7);
        let json = serde_json::to_value(&compacted[1]).unwrap();
        assert_eq!(
            json["PriceTick"]["daily_candle"]["trade_stats"]["turnover_cents"],
            "9007200000000000"
        );
    }

    #[test]
    fn fastest_compaction_keeps_six_second_auction_slots_and_completion() {
        let auction_tick = |seq, tick| Event::AuctionTick {
            seq,
            tick,
            phase: engine::TradingPhase::CallAuction,
            code: StockCode("AAA".into()),
            indicative_price: Some(Money::from_cents(100)),
            matched_volume: seq,
            imbalance: 0,
        };
        let completed = Event::AuctionCompleted {
            seq: 4,
            tick: 900,
            phase: engine::TradingPhase::CallAuction,
            code: StockCode("AAA".into()),
            clearing_price: Some(Money::from_cents(100)),
            matched_volume: 3,
        };
        let compacted = compact_fastest_events(
            vec![
                auction_tick(1, 1),
                auction_tick(2, 6),
                auction_tick(3, 7),
                completed,
            ],
            15_300,
            900,
        );
        assert_eq!(compacted.len(), 3);
        assert!(matches!(compacted[0], Event::AuctionTick { seq: 2, .. }));
        assert!(matches!(compacted[1], Event::AuctionTick { seq: 3, .. }));
        assert!(matches!(
            compacted[2],
            Event::AuctionCompleted { seq: 4, .. }
        ));
    }
}
