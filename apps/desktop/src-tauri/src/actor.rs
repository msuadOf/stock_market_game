//! actor-per-session（ADR-0005 §5，桌面端版）：每 session 一个 tokio task 独占 `GameSession`。
//!
//! 与 `apps/server/src/actor.rs` 同源，差异仅在「事件出口」：
//! - server：每 tick 产 Event[] → `broadcast` 给 N 个 WS 订阅者。
//! - **desktop**：每 tick 产 Event[] → `app.emit("engine-event", payload)` 单播给前端窗口。
//!
//! 设计要点（市场状态独占、契合 engine `Send`）：
//! - `GameSession` 由 actor task 独占；Player 与 NPC 共用每会话 ingress，短生命周期锁不覆盖 step。
//! - Player 在 generation 检查后直接登记 ingress；控制命令（快照 / 改速 / restore）走 mpsc，
//!   每条带 `oneshot` 回执 → 调用方拿 `Result`。engine 失败显式上抛，绝不静默吞（铁律二）。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 当前单玩家模式：意图固定路由给玩家 `AccountId(0)`（见 `enqueue`）。

use std::collections::VecDeque;
#[cfg(test)]
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[cfg(feature = "host-parity")]
use engine::session::protocol::CivilUpdate;
use engine::session::protocol::{EngineUpdate, PausePreferences, ProtocolSession, TickFrame};
use engine::{
    calendar::CivilDate,
    company::{PublicReportPage, PublicReportQuery, PublicReportSummary},
    session::HistoricalStockData,
    AccountId, Intent, SaveSlot, SessionError, SessionSetup, Snapshot,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::{mpsc, oneshot};

use crate::EngineEventPayload;
mod failure;
pub(crate) use failure::HostFailure;
#[cfg(test)]
mod correction_recovery_tests;
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
const MAX_PENDING_TICKS: usize = 8;
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
    pub stock_catalog: Vec<engine::session::StockIdentityEntry>,
    pub company_catalog: Vec<engine::session::CompanyCatalogEntry>,
    pub civil_date: CivilDate,
    pub timeline_id: String,
    pub generation: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmationQuery {
    #[serde(deserialize_with = "required_cursor")]
    before_receipt: Option<String>,
}

fn required_cursor<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    <Option<String> as serde::Deserialize>::deserialize(deserializer)
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum HistoryQuery {
    MarketHistory(engine::session::MarketHistoryRequest),
    CurrentMinuteHistory(engine::session::CurrentMinuteHistoryRequest),
    PersonalTradeHistory(engine::session::PersonalTradeHistoryRequest),
    PersonalTradeConfirmations(ConfirmationQuery),
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerationResponse<T> {
    pub generation: String,
    pub value: T,
}

#[derive(Debug, Serialize)]
pub struct CompanyReadResponse {
    pub generation: String,
    pub owner: AccountId,
    pub value: engine::session::protocol::CompanyReadResult,
}

#[derive(Debug)]
pub enum CompanyReadHostError {
    Session { message: String },
    Query(engine::session::protocol::CompanyReadError),
}

fn diagnostic_generation_response(
    requested_generation: u64,
    current_generation: u64,
    read: impl FnOnce() -> engine::NpcDecisionDiagnostics,
) -> Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SessionError> {
    if requested_generation != current_generation {
        // 1310：世代过期；ctx[0]=请求世代，ctx[1]=当前世代。
        return Err(SessionError::InvalidSave(
            engine::error::ValidationFault::new(
                1310,
                vec![requested_generation, current_generation],
            ),
        ));
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

#[derive(Debug, thiserror::Error)]
pub enum CompanyWriteHostError {
    #[error("公司写命令宿主失败：{message}")]
    Session { message: String },
    #[error("{0}")]
    Command(#[from] engine::session::protocol::CompanyWriteError),
    #[error("公司写命令已触发引擎致命错误：{0}")]
    Fatal(#[source] engine::session::StepFatal),
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum CompanyWriteHostFailure {
    Command(engine::session::protocol::CompanyWriteFailure),
    Fatal(HostFailure),
    Session { message: String },
}

impl CompanyWriteHostError {
    pub(crate) fn public_failure(&self) -> CompanyWriteHostFailure {
        match self {
            Self::Session { message } => CompanyWriteHostFailure::Session {
                message: message.clone(),
            },
            Self::Command(error) => CompanyWriteHostFailure::Command(error.public_failure()),
            Self::Fatal(fatal) => CompanyWriteHostFailure::Fatal(HostFailure::step(fatal.clone())),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RightsSubscriptionResponse {
    pub generation: String,
    pub receipt: engine::session::RightsSubscriptionReceipt,
}

#[derive(Debug, Serialize)]
pub struct CompanyProposalResponse {
    pub generation: String,
    pub receipt: engine::session::PlayerCompanyCommandReceipt,
}

#[derive(Debug, Serialize)]
pub struct ReportCorrectionResponse {
    pub generation: String,
    pub operation_id: String,
}

/// 发给 actor 的命令；控制回执表示 actor 已应用，不只是进入 mpsc。
///
/// `reply` 用 `Result` 而非裸值：engine 失败（`SessionError`）显式上抛，绝不静默吞（铁律二）。
#[derive(Debug)]
pub enum SessionCommand {
    SubscribeRightsOffering {
        generation: u64,
        event_id: String,
        requested_shares: u64,
        reply: oneshot::Sender<Result<RightsSubscriptionResponse, CompanyWriteHostError>>,
    },
    ProposePlayerCompanyAction {
        generation: u64,
        proposal: engine::session::PlayerCompanyProposal,
        reply: oneshot::Sender<Result<CompanyProposalResponse, CompanyWriteHostError>>,
    },
    EnqueueReportCorrection {
        generation: u64,
        request: engine::session::CompanyReportCorrection,
        reply: oneshot::Sender<Result<ReportCorrectionResponse, CompanyWriteHostError>>,
    },
    CancelReportCorrection {
        generation: u64,
        operation_id: String,
        reply: oneshot::Sender<Result<ReportCorrectionResponse, CompanyWriteHostError>>,
    },
    CompanyRead {
        generation: u64,
        query: engine::session::protocol::CompanyReadQuery,
        reply: oneshot::Sender<Result<CompanyReadResponse, CompanyReadHostError>>,
    },
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
    HostHistoryQuery {
        generation: u64,
        query: HistoryQuery,
        reply: oneshot::Sender<Result<GenerationResponse<serde_json::Value>, String>>,
    },
    StockHistory {
        generation: u64,
        code: engine::StockId,
        reply: oneshot::Sender<Result<GenerationResponse<HistoricalStockData>, SessionError>>,
    },
    InitialAllocation {
        generation: u64,
        reply: oneshot::Sender<
            Result<GenerationResponse<engine::session::InitialAllocation>, SessionError>,
        >,
    },
    QueryBaseline {
        generation: u64,
        reply: oneshot::Sender<Result<RestoreResult, BaselineError>>,
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
        reply: oneshot::Sender<Result<RestoreResult, BaselineError>>,
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
    ingress: Arc<RwLock<(u64, engine::SharedSessionIngress)>>,
}

impl SessionHandles {
    pub async fn subscribe_rights_offering(
        &self,
        generation: u64,
        event_id: String,
        requested_shares: u64,
    ) -> Result<RightsSubscriptionResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SubscribeRightsOffering {
                generation,
                event_id,
                requested_shares,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn propose_player_company_action(
        &self,
        generation: u64,
        proposal: engine::session::PlayerCompanyProposal,
    ) -> Result<CompanyProposalResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ProposePlayerCompanyAction {
                generation,
                proposal,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn enqueue_report_correction(
        &self,
        generation: u64,
        request: engine::session::CompanyReportCorrection,
    ) -> Result<ReportCorrectionResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::EnqueueReportCorrection {
                generation,
                request,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn cancel_report_correction(
        &self,
        generation: u64,
        operation_id: String,
    ) -> Result<ReportCorrectionResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CancelReportCorrection {
                generation,
                operation_id,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn company_read_query(
        &self,
        generation: u64,
        query: engine::session::protocol::CompanyReadQuery,
    ) -> Result<CompanyReadResponse, CompanyReadHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CompanyRead {
                generation,
                query,
                reply,
            })
            .map_err(|error| CompanyReadHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyReadHostError::Session {
                message: error.to_string(),
            })?
    }
    /// 便捷：入队玩家意图（固定 `AccountId(0)`），立即登记到共享 ingress。
    pub async fn enqueue(&self, generation: u64, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(generation, AccountId(0), intent).await
    }

    /// 入队指定玩家意图（联机多账户预留，当前由 player 0 调用）。
    pub async fn enqueue_as(
        &self,
        generation: u64,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SendCommandError> {
        if self.cmd_tx.is_closed() {
            return Err(SendCommandError::ActorGone);
        }
        let ingress = self.ingress.read().map_err(|error| {
            SendCommandError::Rejected(format!("session ingress lifecycle lock poisoned: {error}"))
        })?;
        if generation != ingress.0 {
            return Err(SendCommandError::Rejected(format!(
                "stale session generation {generation}; current generation is {}",
                ingress.0
            )));
        }
        ingress
            .1
            .enqueue_player_intent(player_id, intent)
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

    pub async fn host_history_query(
        &self,
        generation: u64,
        query: HistoryQuery,
    ) -> Result<GenerationResponse<serde_json::Value>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::HostHistoryQuery {
                generation,
                query,
                reply: tx,
            })
            .map_err(|_| "actor 已关闭".to_owned())?;
        rx.await.map_err(|_| "actor 已关闭".to_owned())?
    }

    pub async fn query_stock_history(
        &self,
        generation: u64,
        code: engine::StockId,
    ) -> Result<GenerationResponse<HistoricalStockData>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::StockHistory {
                generation,
                code,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn initial_allocation(
        &self,
        generation: u64,
    ) -> Result<GenerationResponse<engine::session::InitialAllocation>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::InitialAllocation {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn query_baseline(
        &self,
        generation: u64,
    ) -> Result<RestoreResult, BaselineRequestError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::QueryBaseline {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(BaselineRequestError::Baseline)
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
            .map_err(|error| SendCommandError::Rejected(failure::save_error_message(error)))
    }

    pub async fn restore(
        &self,
        generation: u64,
        slot: SaveSlot,
    ) -> Result<RestoreResult, BaselineRequestError> {
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
            .map_err(BaselineRequestError::Baseline)
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
pub enum BaselineError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("公开公司身份目录构造失败")]
    Catalog(#[from] engine::session::CompanyCatalogViewError),
}

#[derive(Debug, thiserror::Error)]
pub enum BaselineRequestError {
    #[error(transparent)]
    Command(#[from] SendCommandError),
    #[error(transparent)]
    Baseline(#[from] BaselineError),
}

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
        let ingress = Arc::new(RwLock::new((1, game.shared_ingress())));
        let handles = Arc::new(SessionHandles {
            cmd_tx,
            ingress: Arc::clone(&ingress),
        });

        // 异步锁：Tauri command 在 Tokio runtime 内调用，禁止 blocking_lock 导致 panic。
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), handles.clone());

        let actor = SessionActor {
            ingress,
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
            consumer: None,
            pending_ticks: 0,
            fixed_batches: VecDeque::new(),
            fixed_frames: VecDeque::new(),
            consumer_notify: Arc::new(tokio::sync::Notify::new()),
            #[cfg(test)]
            consumer_gate: None,
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

#[cfg(test)]
#[derive(Default)]
struct StepFailureCapture {
    saved: Option<serde_json::Value>,
    origin: Option<engine::RuntimeMirrorOrigin>,
    business: Option<engine::session::StateHash>,
    frames: Vec<engine::session::protocol::TickFrame>,
}

struct TickConsumer {
    input: Option<std::sync::mpsc::Sender<engine::session::protocol::DeferredTick>>,
    output: std::sync::mpsc::Receiver<
        Result<engine::session::protocol::ConsumedTick, engine::session::StepFatal>,
    >,
    worker: Option<std::thread::JoinHandle<()>>,
}

fn consumer_failure(description: impl Into<String>) -> engine::session::StepFatal {
    engine::session::StepFatal::InvariantViolation {
        location: "desktop.actor.tick_consumer".into(),
        description: description.into(),
    }
}

struct ConsumerWakeOnExit {
    sender: Option<
        std::sync::mpsc::Sender<
            Result<engine::session::protocol::ConsumedTick, engine::session::StepFatal>,
        >,
    >,
    notify: Arc<tokio::sync::Notify>,
}

impl Drop for ConsumerWakeOnExit {
    fn drop(&mut self) {
        self.sender.take();
        self.notify.notify_one();
    }
}

impl Drop for TickConsumer {
    fn drop(&mut self) {
        self.input.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                eprintln!("desktop tick consumer 线程发生 panic");
            }
        }
    }
}

/// actor：独占 `GameSession` 的 tokio task。命令经 `cmd_rx`，事件经 `app.emit`。
struct SessionActor<R: Runtime> {
    ingress: Arc<RwLock<(u64, engine::SharedSessionIngress)>>,
    #[cfg(test)]
    injected_step_failure: Option<(
        usize,
        engine::session::StepFatal,
        Arc<std::sync::Mutex<StepFailureCapture>>,
    )>,
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
    consumer: Option<TickConsumer>,
    pending_ticks: usize,
    fixed_batches: VecDeque<(usize, bool)>,
    fixed_frames: VecDeque<TickFrame>,
    consumer_notify: Arc<tokio::sync::Notify>,
    #[cfg(test)]
    consumer_gate: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
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
            ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
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
            consumer: None,
            pending_ticks: 0,
            fixed_batches: VecDeque::new(),
            fixed_frames: VecDeque::new(),
            consumer_notify: Arc::new(tokio::sync::Notify::new()),
            consumer_gate: None,
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
    fn start_consumer(&mut self) -> Result<(), engine::session::StepFatal> {
        if self.consumer.is_some() {
            return Ok(());
        }
        let (input, receiver) =
            std::sync::mpsc::channel::<engine::session::protocol::DeferredTick>();
        let (sender, output) = std::sync::mpsc::channel();
        let notify = Arc::clone(&self.consumer_notify);
        #[cfg(test)]
        let gate = self.consumer_gate.take();
        let worker = std::thread::Builder::new()
            .name("desktop-tick-consumer".into())
            .spawn(move || {
                let wake_on_exit = ConsumerWakeOnExit {
                    sender: Some(sender),
                    notify: Arc::clone(&notify),
                };
                #[cfg(test)]
                let mut gate = gate;
                for deferred in receiver {
                    #[cfg(test)]
                    if let Some((entered, release)) = gate.take() {
                        entered.send(()).expect("测试必须接收消费者进入通知");
                        release.recv().expect("测试必须释放消费者");
                    }
                    let result = deferred.consume();
                    let failed = result.is_err();
                    let sent = wake_on_exit
                        .sender
                        .as_ref()
                        .expect("消费者输出必须存在")
                        .send(result)
                        .is_ok();
                    notify.notify_one();
                    if !sent || failed {
                        break;
                    }
                }
            })
            .map_err(|error| consumer_failure(format!("无法启动外围消费者：{error}")))?;
        self.consumer = Some(TickConsumer {
            input: Some(input),
            output,
            worker: Some(worker),
        });
        Ok(())
    }

    fn collect_consumed(&mut self, wait: bool) -> Result<(), engine::session::StepFatal> {
        let grouped = !self.fixed_batches.is_empty();
        let mut frames = Vec::new();
        while self.pending_ticks > 0 {
            let consumer = self
                .consumer
                .as_ref()
                .expect("有待消费 tick 时必须存在消费者");
            let received = if wait {
                consumer
                    .output
                    .recv()
                    .map_err(|error| consumer_failure(format!("外围消费者退出：{error}")))
            } else {
                match consumer.output.try_recv() {
                    Ok(result) => Ok(result),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(error) => Err(consumer_failure(format!("外围消费者退出：{error}"))),
                }
            };
            self.pending_ticks -= 1;
            match received
                .and_then(|result| result)
                .and_then(|consumed| self.game.record_consumed_tick(consumed))
            {
                Ok(frame) => {
                    #[cfg(test)]
                    if let Some((_, _, capture)) = &self.injected_step_failure {
                        capture.lock().unwrap().frames.push(frame.clone());
                    }
                    if grouped {
                        self.queue_consumed_frames(vec![frame])?;
                    } else {
                        frames.push(frame);
                    }
                }
                Err(error) => {
                    self.pending_ticks = 0;
                    if !frames.is_empty() {
                        self.publish_protocol_frames(frames)
                            .map_err(|failure| consumer_failure(failure.message))?;
                    }
                    self.fixed_batches.clear();
                    let prefix: Vec<_> = self.fixed_frames.drain(..).collect();
                    if !prefix.is_empty() {
                        self.publish_protocol_frames(prefix)
                            .map_err(|failure| consumer_failure(failure.message))?;
                    }
                    return Err(error);
                }
            }
        }
        if !frames.is_empty() {
            self.publish_protocol_frames(frames)
                .map_err(|failure| consumer_failure(failure.message))?;
        }
        Ok(())
    }

    fn queue_consumed_frames(
        &mut self,
        frames: Vec<TickFrame>,
    ) -> Result<(), engine::session::StepFatal> {
        if self.fixed_batches.is_empty() {
            if !frames.is_empty() {
                self.publish_protocol_frames(frames)
                    .map_err(|failure| consumer_failure(failure.message))?;
            }
            return Ok(());
        }
        self.fixed_frames.extend(frames);
        while let Some(&(count, closed)) = self.fixed_batches.front() {
            if !closed || self.fixed_frames.len() < count {
                break;
            }
            self.fixed_batches.pop_front();
            let batch: Vec<_> = self.fixed_frames.drain(..count).collect();
            if !batch.is_empty() {
                self.publish_protocol_frames(batch)
                    .map_err(|failure| consumer_failure(failure.message))?;
            }
        }
        Ok(())
    }

    fn drain_consumed(&mut self) -> bool {
        if let Err(error) = self.collect_consumed(true) {
            self.stop_after_step_failure(error);
            return false;
        }
        true
    }
    /// 主循环：`select!` 同时等命令与 interval tick。
    ///
    /// - 命令到达 → 处理（Enqueue→入队、Snapshot→回快照、SetSpeed→改 interval）。
    /// - interval 到 → `step()` → `app.emit("engine-event", ...)`。
    /// - `cmd_rx` 关闭（所有句柄 drop）→ 退出，task 结束。
    async fn run(mut self) {
        let mut interval = self.fresh_interval();
        let consumer_notify = Arc::clone(&self.consumer_notify);

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
                _ = consumer_notify.notified(), if self.pending_ticks > 0 => {
                    if let Err(error) = self.collect_consumed(false) { self.stop_after_step_failure(error); }
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
        self.drain_consumed();
        self.consumer.take();
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
        let fixed = !self.pacing.is_fastest() && limit > 0;
        if fixed {
            self.fixed_batches.push_back((0, false));
        }
        self.run_cycle_inner(limit, fixed);
        if fixed {
            self.finish_fixed_batch();
        }
    }

    fn finish_fixed_batch(&mut self) {
        if let Some(batch) = self.fixed_batches.back_mut() {
            batch.1 = true;
        }
        if let Err(error) = self.queue_consumed_frames(Vec::new()) {
            self.stop_after_step_failure(error);
        }
    }

    fn run_cycle_inner(&mut self, limit: usize, fixed: bool) {
        if self.cmd_rx.is_closed() {
            return;
        }
        if let Err(error) = self.collect_consumed(false) {
            self.stop_after_step_failure(error);
            return;
        }
        if let Err(error) = self.game.require_healthy() {
            self.stop_after_step_failure(error);
            return;
        }
        let started = Instant::now();
        let mut steps = 0;
        while steps < limit && {
            #[cfg(test)]
            let injecting = self.injected_step_failure.is_some();
            #[cfg(not(test))]
            let injecting = false;
            injecting || started.elapsed() < FASTEST_BATCH_BUDGET
        } {
            if limit > 1 && !self.cmd_rx.is_empty() {
                break;
            }
            match self.game.civil_day_ready() {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    if fixed {
                        self.finish_fixed_batch();
                    }
                    if !self.drain_consumed() {
                        return;
                    }
                    self.stop_cycle_with_unpublished_frames(Vec::new(), error);
                    return;
                }
            }
            #[cfg(test)]
            if let Some((remaining, error, capture)) = &mut self.injected_step_failure {
                if *remaining == 0 {
                    let error = error.clone();
                    let capture = Arc::clone(capture);
                    if fixed {
                        self.finish_fixed_batch();
                    }
                    if !self.drain_consumed() {
                        return;
                    }
                    let mut observed = capture.lock().unwrap();
                    observed.saved =
                        Some(serde_json::to_value(self.game.game().save().unwrap()).unwrap());
                    observed.origin = Some(self.game.runtime_mirror_origin());
                    observed.business = Some(self.game.business_state_hash().unwrap());
                    drop(observed);
                    self.stop_after_step_failure(error);
                    return;
                }
                *remaining -= 1;
            }
            if self.pending_ticks == MAX_PENDING_TICKS {
                if !self.drain_consumed() || self.cmd_rx.is_closed() {
                    return;
                }
            }
            if let Err(error) = self.start_consumer() {
                self.stop_after_step_failure(error);
                return;
            }
            let deferred = match self.game.advance_core_tick() {
                Ok(deferred) => deferred,
                Err(error) => {
                    if fixed {
                        self.finish_fixed_batch();
                    }
                    let pending = self.pending_ticks;
                    let mut failure = HostFailure::step(error);
                    if let Err(publication_error) = self.collect_consumed(true) {
                        eprintln!(
                            "[session {}] fatal 后成功前缀无法交付：{publication_error:?}",
                            self.session_id
                        );
                        failure.message.push_str(&format!("；先前 {pending} 个已提交 tick 的外围结果未能全部交付，内存事实保留供诊断"));
                    }
                    self.stop_after_host_failure(failure);
                    return;
                }
            };
            let consumer = self.consumer.as_ref().unwrap();
            if let Err(error) = consumer.input.as_ref().unwrap().send(deferred) {
                let mut failure =
                    HostFailure::step(consumer_failure(format!("无法提交已完成 tick：{error}")));
                failure.message.push_str(&format!(
                    "；tick {} 已提交但未交付，内存事实保留供诊断，请重新打开有效日终档或新局",
                    self.game.tick()
                ));
                if fixed {
                    if let Some(batch) = self.fixed_batches.back_mut() {
                        batch.1 = true;
                    }
                }
                let prefix_result = self
                    .queue_consumed_frames(Vec::new())
                    .and_then(|_| self.collect_consumed(true));
                if let Err(prefix_error) = prefix_result {
                    eprintln!(
                        "[session {}] 发送失败后的成功前缀无法交付：{prefix_error:?}",
                        self.session_id
                    );
                    failure.message.push_str("；先前已提交前缀未能全部交付");
                }
                self.stop_after_host_failure(failure);
                return;
            }
            self.pending_ticks += 1;
            if fixed {
                self.fixed_batches
                    .back_mut()
                    .expect("固定倍率批次必须存在")
                    .0 += 1;
            }
            steps += 1;
        }
        if let Err(error) = self.collect_consumed(false) {
            self.stop_after_step_failure(error);
            return;
        }
        match self.game.civil_day_ready() {
            Ok(true) => {
                if fixed {
                    self.finish_fixed_batch();
                }
                if !self.drain_consumed() || self.cmd_rx.is_closed() {
                    return;
                }
                if let Err(failure) = self.prepare_civil_updates() {
                    self.stop_after_host_failure(*failure);
                    return;
                }
            }
            Ok(false) => {}
            Err(error) => {
                self.stop_cycle_with_unpublished_frames(Vec::new(), error);
                return;
            }
        }
        self.pacing.refresh_metrics(self.game.tick());
    }

    async fn run_fastest_batch(&mut self) {
        self.run_cycle(FASTEST_BATCH_MAX_STEPS);
    }

    fn describe_unpublished_frames(failure: &mut HostFailure, range: Option<(u64, u64, u64, u64)>) {
        if let Some((tick_from, tick_to, seq_from, seq_to)) = range {
            failure.message.push_str(&format!("；本批已提交但尚未交付的成功前缀：tick_from={tick_from}, tick_to={tick_to}, seq_from={seq_from}, seq_to={seq_to}；内存事实仍保留供诊断，请重新打开有效日终档或新局"));
        }
    }

    fn stop_cycle_with_unpublished_frames(&mut self, frames: Vec<TickFrame>, error: SessionError) {
        let mut failure = failure::HostFailure::civil(error);
        if !frames.is_empty() {
            let range = frames
                .first()
                .zip(frames.last())
                .map(|(first, last)| (first.tick, last.tick, first.seq_from, last.seq_to));
            match self.game.tick_batch(frames) {
                Ok(batch) => self.emit_update(EngineUpdate::TickBatch(Box::new(batch))),
                Err(publication_error) => {
                    eprintln!("[session {}] fatal 后成功前缀无法通过健康门，保留原首错和未交付范围：{publication_error:?}", self.session_id);
                    Self::describe_unpublished_frames(&mut failure, range);
                }
            }
        }
        self.stop_after_host_failure(failure);
    }

    fn publish_protocol_frames(&mut self, frames: Vec<TickFrame>) -> Result<(), Box<HostFailure>> {
        if !frames.is_empty() {
            let range = frames
                .first()
                .zip(frames.last())
                .map(|(first, last)| (first.tick, last.tick, first.seq_from, last.seq_to));
            match self.game.tick_batch(frames) {
                Ok(batch) => self.emit_update(EngineUpdate::TickBatch(Box::new(batch))),
                Err(error) => {
                    let mut failure = failure::HostFailure::step(error);
                    Self::describe_unpublished_frames(&mut failure, range);
                    return Err(Box::new(failure));
                }
            }
        }
        Ok(())
    }

    fn publish_civil_update(&mut self, update: engine::session::protocol::CivilUpdate) {
        let pause = self.pause_preferences.pauses(&update);
        self.emit_update(EngineUpdate::CivilUpdate(Box::new(update)));
        if pause {
            self.pacing.pause_at_civil_boundary(self.game.tick());
        }
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

    fn finish_company_write<T>(
        &mut self,
        result: Result<T, CompanyWriteHostError>,
    ) -> Result<T, CompanyWriteHostError> {
        // generation 拒绝先于领域命令，也不能因当前 Session 已 poison 而改写其结果。
        if matches!(&result, Err(CompanyWriteHostError::Session { .. })) {
            return result;
        }
        let fatal = self
            .game
            .require_healthy()
            .err()
            .or_else(|| match result.as_ref().err() {
                Some(CompanyWriteHostError::Command(error)) => error.step_fatal().cloned(),
                _ => None,
            });
        if let Some(fatal) = fatal {
            if let Err(error) = &result {
                eprintln!(
                    "[session {}] 公司写命令触发引擎致命错误：{error:?}",
                    self.session_id
                );
            }
            self.stop_after_step_failure(fatal.clone());
            return Err(CompanyWriteHostError::Fatal(fatal));
        }
        result
    }

    /// 处理单条命令。
    async fn handle_command(&mut self, cmd: SessionCommand) {
        if self.cmd_rx.is_closed() {
            return;
        }
        if matches!(
            &cmd,
            SessionCommand::SetRunning { running: false, .. } | SessionCommand::Shutdown { .. }
        ) {
            self.pacing.set_running(false, self.game.tick());
        }
        if !self.drain_consumed() || self.cmd_rx.is_closed() {
            return;
        }
        if matches!(
            &cmd,
            SessionCommand::Snapshot { .. }
                | SessionCommand::RuntimeSnapshot { .. }
                | SessionCommand::QueryBaseline { .. }
                | SessionCommand::PlayerWorkingOrders { .. }
        ) {
            if let Err(error) = self.game.flush_now() {
                self.stop_after_step_failure(error);
                return;
            }
        }
        match cmd {
            SessionCommand::SubscribeRightsOffering {
                generation,
                event_id,
                requested_shares,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        self.game
                            .require_healthy()
                            .map_err(CompanyWriteHostError::Fatal)
                    })
                    .and_then(|_| {
                        self.game
                            .subscribe_rights_offering(AccountId(0), &event_id, requested_shares)
                            .map(|receipt| RightsSubscriptionResponse {
                                generation: self.generation.to_string(),
                                receipt,
                            })
                            .map_err(CompanyWriteHostError::Command)
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::ProposePlayerCompanyAction {
                generation,
                proposal,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        self.game
                            .propose_player_company_action(AccountId(0), proposal)
                            .map(|receipt| CompanyProposalResponse {
                                generation: self.generation.to_string(),
                                receipt,
                            })
                            .map_err(|error| CompanyWriteHostError::Command(error.into()))
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::EnqueueReportCorrection {
                generation,
                request,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        let operation_id = request.operation_id.clone();
                        let epoch = self.game.game().report_correction_epoch();
                        self.game
                            .enqueue_report_correction(&epoch, request)
                            .map(|_| ReportCorrectionResponse {
                                generation: self.generation.to_string(),
                                operation_id,
                            })
                            .map_err(|error| CompanyWriteHostError::Command(error.into()))
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::CancelReportCorrection {
                generation,
                operation_id,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        let epoch = self.game.game().report_correction_epoch();
                        self.game
                            .cancel_report_correction(&epoch, &operation_id)
                            .map(|_| ReportCorrectionResponse {
                                generation: self.generation.to_string(),
                                operation_id,
                            })
                            .map_err(|error| CompanyWriteHostError::Command(error.into()))
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::CompanyRead {
                generation,
                query,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyReadHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        let owner = AccountId(0);
                        self.game
                            .company_read_query(owner, query)
                            .map(|value| CompanyReadResponse {
                                generation: self.generation.to_string(),
                                owner,
                                value,
                            })
                            .map_err(CompanyReadHostError::Query)
                    });
                let _ = reply.send(result);
            }
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
                    serde_json::to_value(self.game.player_working_orders()).map_err(|_| {
                        // 1312：玩家在途委托序列化失败（边界日志承载细节）。
                        SessionError::InvalidSave(engine::error::ValidationFault::bare(1312))
                    });
                let result = self.generation_response(generation, orders);
                let _ = reply.send(result);
            }
            SessionCommand::HostHistoryQuery {
                generation,
                query,
                reply,
            } => {
                let result = (|| -> Result<GenerationResponse<serde_json::Value>, String> {
                    if generation != self.generation {
                        return Err("STALE_SESSION_GENERATION".into());
                    }
                    self.game.require_healthy().map_err(|e| e.to_string())?;
                    let value = match query {
                        HistoryQuery::MarketHistory(query) => serde_json::to_value(
                            self.game
                                .query_market_history_for(AccountId(0), &query)
                                .map_err(|e| e.to_string())?,
                        ),
                        HistoryQuery::CurrentMinuteHistory(query) => serde_json::to_value(
                            self.game
                                .query_current_minute_history_for(AccountId(0), &query)
                                .map_err(|e| e.to_string())?,
                        ),
                        HistoryQuery::PersonalTradeHistory(query) => serde_json::to_value(
                            self.game
                                .query_personal_trade_history(AccountId(0), query)
                                .map_err(|e| e.to_string())?,
                        ),
                        HistoryQuery::PersonalTradeConfirmations(query) => {
                            let cursor = query.before_receipt;
                            let cursor = cursor
                                .map(|s| {
                                    if s.is_empty()
                                        || (s.len() > 1 && s.starts_with('0'))
                                        || !s.bytes().all(|b| b.is_ascii_digit())
                                    {
                                        return Err("交割单游标无效".to_owned());
                                    }
                                    s.parse::<u64>().map_err(|e| e.to_string())
                                })
                                .transpose()?;
                            serde_json::to_value(
                                self.game
                                    .personal_trade_confirmations_page(AccountId(0), cursor),
                            )
                        }
                    }
                    .map_err(|e| e.to_string())?;
                    Ok(GenerationResponse {
                        generation: generation.to_string(),
                        value,
                    })
                })();
                let _ = reply.send(result);
            }
            SessionCommand::StockHistory {
                generation,
                code,
                reply,
            } => {
                let result = if generation != self.generation {
                    self.generation_response(
                        generation,
                        Err(SessionError::InvalidSave(
                            engine::error::ValidationFault::new(
                                1310,
                                vec![generation, self.generation],
                            ),
                        )),
                    )
                } else {
                    let queried = self.game.query_stock_history(AccountId(0), &code);
                    self.generation_response(generation, queried)
                };
                let _ = reply.send(result);
            }
            SessionCommand::InitialAllocation { generation, reply } => {
                let result = self.generation_response(generation, self.game.initial_allocation());
                let _ = reply.send(result);
            }
            SessionCommand::QueryBaseline { generation, reply } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(BaselineError::Session)
                    .and_then(|_| {
                        self.baseline_candidate("query_baseline", "desktop.actor.query_baseline")
                    });
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
                if let Err(error) = self.game.shared_ingress().close() {
                    self.stop_after_host_failure(failure::HostFailure::civil(error));
                }
                let _ = reply.send(());
            }
        }
    }

    /// 应用新倍速：固定倍率使用精确 Duration；Fastest 切到 CPU 时间片 tight-loop。
    fn apply_speed(&mut self, speed: f64) -> Result<(), SessionError> {
        if !self.pacing.apply_speed(speed, self.game.tick()) {
            // 1311：speed 档位非法；ctx[0]=speed 位模式（f64）。
            return Err(SessionError::InvalidSave(
                engine::error::ValidationFault::new(1311, vec![speed.to_bits()]),
            ));
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
            // 1310：世代过期。
            return Err(SessionError::InvalidSave(
                engine::error::ValidationFault::new(1310, vec![generation, self.generation]),
            ));
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
    ) -> Result<RestoreResult, BaselineError> {
        self.generation_response(generation, Ok(()))?;
        let restored = ProtocolSession::restore(&slot)?;
        let next_generation = self.generation.checked_add(1).ok_or_else(|| {
            // 1313：世代计数耗尽。
            SessionError::InvalidSave(engine::error::ValidationFault::bare(1313))
        })?;
        {
            let mut ingress = self.ingress.write().map_err(|_| {
                // 1314：ingress 生命周期锁中毒。
                SessionError::ResourceLimit(engine::error::ValidationFault::bare(1314))
            })?;
            ingress.1.close()?;
            *ingress = (next_generation, restored.shared_ingress());
        }
        self.game = restored;
        self.game.prepare_public_baseline();
        self.timeline_id = uuid::Uuid::new_v4().to_string();
        self.generation = next_generation;
        self.pacing.reset_after_restore(self.game.tick());
        self.baseline_candidate("restore", "desktop.actor.restore")
    }

    fn baseline_candidate(
        &mut self,
        operation: &'static str,
        location: &'static str,
    ) -> Result<RestoreResult, BaselineError> {
        let catalog = self.game.company_catalog_view();
        self.finish_catalog_baseline(catalog, operation, location)
    }

    fn finish_catalog_baseline(
        &mut self,
        catalog: Result<
            Vec<engine::session::CompanyCatalogEntry>,
            engine::session::CompanyCatalogViewError,
        >,
        operation: &'static str,
        location: &'static str,
    ) -> Result<RestoreResult, BaselineError> {
        let company_catalog = match catalog {
            Ok(catalog) => catalog,
            Err(error) => {
                self.stop_after_host_failure(failure::HostFailure::catalog(
                    &error, operation, location,
                ));
                return Err(BaselineError::Catalog(error));
            }
        };
        self.game.prepare_public_baseline();
        Ok(RestoreResult {
            snapshot: self.game.snapshot(),
            stock_catalog: self.game.stock_catalog_view(),
            company_catalog,
            civil_date: self.game.civil_date(),
            timeline_id: self.timeline_id.clone(),
            generation: self.generation.to_string(),
        })
    }

    fn prepare_civil_updates(&mut self) -> Result<(), Box<HostFailure>> {
        while self
            .game
            .civil_day_ready()
            .map_err(|error| Box::new(failure::HostFailure::civil(error)))?
        {
            let date = self.game.civil_date();
            let update = match self.game.end_civil_day_update_classified() {
                Ok(update) => update,
                Err(failure) => {
                    let prepare_rejected = failure.recovery()
                        == engine::session::protocol::CivilRecovery::CorrectionPrepareRejected
                        && !self.cmd_rx.is_closed()
                        && self.game.require_healthy().is_ok()
                        && self.game.civil_date() == date
                        && self.game.civil_day_ready().is_ok_and(|ready| ready);
                    let error = failure.into_error();
                    return Err(Box::new(if prepare_rejected {
                        failure::HostFailure::correction_prepare_rejected(error)
                    } else {
                        failure::HostFailure::civil(error)
                    }));
                }
            };
            let pause = self.pause_preferences.pauses(&update);
            // 已成功日结立即交付；下一自然日失败不能丢弃先前的完整 CivilUpdate。
            self.publish_civil_update(update);
            if pause {
                break;
            }
        }
        Ok(())
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
    let mut active_minute_ticks: HashMap<(engine::StockId, u64), usize> = HashMap::new();
    let mut auction_slot_ticks: HashMap<(engine::StockId, u64), usize> = HashMap::new();
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
                auction_slot_ticks.insert((*code, day_tick / 6), index);
            }
            engine::Event::PriceTick { tick, code, .. } => {
                let day_tick = (tick.saturating_sub(1)) % safe_ticks_per_day;
                active_minute_ticks
                    .insert((*code, day_tick.saturating_sub(auction_ticks) / 60), index);
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
            company_system: None,
            dividend_tax_mode: None,
            flat_withholding_bp: None,
            rights_offering_enabled: false,
            issuer_repurchase_enabled: false,
            par_value_per_share: None,
            auto_corporate_foundation: false,
            report_frequency: None,
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
            float_allocation: FloatAllocation::random(),
            start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
            runtime_sync_policy: Default::default(),
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
    async fn initial_allocation_queries_actual_npc_holdings_before_start() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();
        assert!(handles.initial_allocation(0).await.is_err());
        let allocation = handles.initial_allocation(1).await.unwrap();
        assert_eq!(allocation.generation, "1");
        assert_eq!(allocation.value.stocks[0].categories[1].shares, 100_000);
        assert_eq!(allocation.value.stocks[0].categories[1].zero_holders, 0);
        assert_eq!(allocation.value.stocks[0].unallocated_shares, 0);
        assert_eq!(
            handles.initial_allocation(1).await.unwrap().value,
            allocation.value
        );
        handles.shutdown().await.unwrap();
        manager.remove(&session_id).await.unwrap();
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
            code: engine::StockId(0),
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
            code: engine::StockId(0),
            indicative_price: Some(Money::from_cents(100)),
            matched_volume: seq,
            imbalance: 0,
        };
        let completed = Event::AuctionCompleted {
            seq: 4,
            tick: 900,
            phase: engine::TradingPhase::CallAuction,
            code: engine::StockId(0),
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
