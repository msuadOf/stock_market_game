//! actor-per-session（ADR-0005 §5）：每 session 一个 tokio task 独占 `GameSession`。
//!
//! 设计要点（无锁、契合 engine `Send`）：
//! - **无共享可变状态、无锁**：`GameSession` 由 actor task 独占 own，外部一律经**消息**与之交互。
//! - **命令通道**（`tokio::sync::mpsc`）：外部投递 `SessionCommand`（入队意图 / 取快照 / 改速）。
//! - **更新广播**（`tokio::sync::broadcast`）：actor 每轮把 `Event[]` 与必要的权威运行快照
//!   作为一个原子批次 broadcast；WS 只是传输适配器，与 Worker/Tauri 的应用层语义一致。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 当前单玩家模式：意图固定路由给玩家 `AccountId(0)`（见 `enqueue`）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
#[cfg(feature = "host-parity")]
use engine::session::protocol::CivilUpdate;
use engine::session::protocol::{
    EngineUpdate as ProtocolUpdate, PausePreferences, ProtocolSession,
};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup, Snapshot};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot, Semaphore};
use tracing::{debug, info, warn};

/// 倍速基准：1x 时一个 tick 的间隔毫秒数（与前端 RemoteHost 对齐的「真实时间」尺度）。
/// 取 1000ms（1 秒一 tick）作为可感知默认；speed=N → interval = BASE_TICK_MS / N。
pub const BASE_TICK_MS: u64 = 1000;
pub const MAX_SPEED_MULTIPLIER: f64 = BASE_TICK_MS as f64;
const FASTEST_BATCH_BUDGET: Duration = Duration::from_millis(14);
const FASTEST_BATCH_MAX_STEPS: usize = 100_000;
const SPEED_SAMPLE_MIN_DURATION: Duration = Duration::from_millis(500);

fn fixed_tick_duration(base_ms: u64, speed: f64) -> Duration {
    debug_assert!(speed.is_finite() && speed > 0.0);
    Duration::from_secs_f64((base_ms as f64 / 1_000.0) / speed).max(Duration::from_millis(1))
}

fn fastest_permits_for_workers(worker_count: usize) -> usize {
    worker_count.saturating_sub(1).max(1)
}

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

/// 三种部署共用的应用层更新单元。远程 WS 只负责把它序列化传输。
#[derive(Debug, Clone, Serialize)]
pub struct EngineUpdate {
    pub timeline_generation: u64,
    pub update: Option<ProtocolUpdate>,
    pub civil_date: String,
    pub public_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<HostFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostFailure {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicBaseline {
    pub timeline_generation: u64,
    pub snapshot: PublicBaselineSnapshot,
    pub civil_date: String,
    pub public_revision: u64,
    pub public_report_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicBaselineSnapshot {
    pub seq: u64,
    pub tick: u64,
    pub day: u32,
    pub phase: engine::TradingPhase,
    pub markets: std::collections::BTreeMap<engine::StockCode, engine::MarketSnap>,
    pub accounts: std::collections::BTreeMap<AccountId, engine::AccountSnap>,
    pub daily_candles: std::collections::BTreeMap<engine::StockCode, Vec<engine::DailyCandle>>,
    pub active_daily_candles: std::collections::BTreeMap<engine::StockCode, engine::DailyCandle>,
}

impl From<Snapshot> for PublicBaselineSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        let player_account = snapshot
            .accounts
            .into_iter()
            .filter(|(account_id, _)| *account_id == AccountId(0))
            .collect();
        Self {
            seq: snapshot.seq,
            tick: snapshot.tick,
            day: snapshot.day,
            phase: snapshot.phase,
            markets: snapshot.markets,
            accounts: player_account,
            daily_candles: snapshot.daily_candles,
            active_daily_candles: snapshot.active_daily_candles,
        }
    }
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

/// 广播事件通道容量：留足缓冲以应对慢消费者短时积压；超过则 broadcast 丢旧（lagged），
/// 订阅者靠 `seq` 检测缺口后拉快照对齐（ADR-0005 §6）。
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// 命令通道容量：意图/查询短小，32 足够积压；满了显式 `await` 背压，绝不静默丢命令。
const COMMAND_CHANNEL_CAPACITY: usize = 32;
pub const DEFAULT_MAX_SESSIONS: usize = 256;

/// 发给 actor 的命令。每条命令都自带 `oneshot` 回执通道——actor 处理后 `reply`，调用方拿 `Result`。
///
/// `reply` 用 `Result<...>` 而非裸值：engine 失败（`SessionError`）显式上抛，绝不静默吞（铁律二）。
#[derive(Debug)]
pub enum SessionCommand {
    /// 入队玩家意图（当前固定玩家 0）。Ok=已入队，Err=未知玩家（不应发生，账户恒存在）。
    Enqueue {
        player_id: AccountId,
        intent: Intent,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 取完整快照。Ok=快照值。
    Snapshot { reply: oneshot::Sender<Snapshot> },
    PublicBaseline {
        reply: oneshot::Sender<PublicBaseline>,
    },
    SpeedMetrics {
        reply: oneshot::Sender<SpeedMetrics>,
    },
    Save {
        reply: oneshot::Sender<Result<SaveSlot, engine::session::StepFatal>>,
    },
    Restore {
        slot: Box<SaveSlot>,
        reply: oneshot::Sender<Result<Snapshot, SessionError>>,
    },
    #[cfg(feature = "host-parity")]
    AdvanceCivilDay {
        generation: u64,
        reply: oneshot::Sender<Result<CivilUpdate, SessionError>>,
    },
    #[cfg(feature = "host-parity")]
    Step {
        generation: u64,
        reply: oneshot::Sender<Result<ProtocolUpdate, SessionError>>,
    },
    PublicReportPage {
        query: engine::company::PublicReportQuery,
        reply: oneshot::Sender<Result<engine::company::PublicReportPage, SessionError>>,
    },
    PublicReport {
        id: String,
        reply: oneshot::Sender<Result<engine::company::PublicReportSummary, SessionError>>,
    },
    NpcDecisionDiagnostics {
        generation: u64,
        account: AccountId,
        reply: oneshot::Sender<engine::NpcDecisionDiagnostics>,
    },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed {
        speed: f64,
        reply: oneshot::Sender<()>,
    },
    SetRunning {
        running: bool,
        reply: oneshot::Sender<()>,
    },
    SetPausePreferences {
        generation: u64,
        preferences: PausePreferences,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 永久停止 actor。会话必须先从 manager 移除，避免新请求继续取得句柄。
    Shutdown { reply: oneshot::Sender<()> },
}

#[cfg(test)]
mod fatal_tests;

#[cfg(test)]
mod interval_tests {
    use super::{fastest_permits_for_workers, fixed_tick_duration, SpeedMeter};
    use std::time::Duration;

    #[test]
    fn high_speed_interval_preserves_fractional_milliseconds() {
        assert_eq!(
            fixed_tick_duration(1_000, 720.0),
            Duration::from_secs_f64(1.0 / 720.0)
        );
        assert_eq!(fixed_tick_duration(0, 1.0), Duration::from_millis(1));
    }

    #[test]
    fn fastest_budget_reserves_one_runtime_worker_when_possible() {
        assert_eq!(fastest_permits_for_workers(1), 1);
        assert_eq!(fastest_permits_for_workers(2), 1);
        assert_eq!(fastest_permits_for_workers(8), 7);
    }

    #[test]
    fn speed_meter_reports_authoritative_ticks_per_real_second() {
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
}

/// 一个 session 的对外句柄：命令发送端 + 事件广播端。
///
/// 克隆廉价（`mpsc::Sender` / `broadcast::Sender` 均可 clone）。`SessionManager` 持有
/// `Arc<SessionHandles>`，路由层与 WS 连接经 manager 取一份克隆与 actor 通信。
#[derive(Clone)]
pub struct SessionHandles {
    pub cmd_tx: mpsc::Sender<SessionCommand>,
    pub event_tx: broadcast::Sender<EngineUpdate>,
    pub ticks_per_day: u64,
    pub auction_ticks: u64,
    pub closing_auction_ticks: u64,
    pub session_token: String,
}

impl SessionHandles {
    /// 便捷：入队玩家意图（固定 `AccountId(0)`）。把 `oneshot` 收发封装成 `Result` 返回。
    ///
    /// 失败两种：actor 已退出（通道关闭）→ `SendCommandError`；engine 拒绝 → `SessionError`。
    /// 两者都显式上抛，不静默。
    pub async fn enqueue(&self, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(AccountId(0), intent).await
    }

    /// 入队指定玩家的意图（联机多账户预留接口，当前由 player 0 调用）。
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
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// 取完整快照。actor 关闭时返回 `ActorGone`（绝不静默返回空快照）。
    pub async fn snapshot(&self) -> Result<Snapshot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Snapshot { reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn public_baseline(&self) -> Result<PublicBaseline, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicBaseline { reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn speed_metrics(&self) -> Result<SpeedMetrics, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SpeedMetrics { reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn save(&self) -> Result<SaveSlot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Save { reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn restore(&self, slot: SaveSlot) -> Result<Snapshot, SendCommandError> {
        if slot.setup.ticks_per_day != self.ticks_per_day
            || slot.setup.auction_ticks != self.auction_ticks
            || slot.setup.closing_auction_ticks != self.closing_auction_ticks
        {
            return Err(SendCommandError::Rejected(format!(
                "存档交易时钟配置与当前会话不一致：当前 ticks_per_day={}, auction_ticks={}, closing_auction_ticks={}；存档 ticks_per_day={}, auction_ticks={}, closing_auction_ticks={}",
                self.ticks_per_day,
                self.auction_ticks,
                self.closing_auction_ticks,
                slot.setup.ticks_per_day,
                slot.setup.auction_ticks,
                slot.setup.closing_auction_ticks,
            )));
        }
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Restore {
                slot: Box::new(slot),
                reply: tx,
            })
            .await
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
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    #[cfg(feature = "host-parity")]
    pub async fn step(&self, generation: u64) -> Result<ProtocolUpdate, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Step {
                generation,
                reply: tx,
            })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn public_report_page(
        &self,
        query: engine::company::PublicReportQuery,
    ) -> Result<engine::company::PublicReportPage, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReportPage { query, reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn public_report(
        &self,
        id: String,
    ) -> Result<engine::company::PublicReportSummary, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReport { id, reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn npc_decision_diagnostics(
        &self,
        generation: u64,
        account: AccountId,
    ) -> Result<(u64, engine::NpcDecisionDiagnostics), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::NpcDecisionDiagnostics {
                generation,
                account,
                reply: tx,
            })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map(|result| (generation, result))
            .map_err(|_| SendCommandError::ActorGone)
    }

    /// 改变倍速。fire-and-forget 经 mpsc 保证顺序（在 Enqueue/Snapshot 之后生效），
    /// 但若 actor 已关闭则返回 `ActorGone`（不静默）。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        if speed != f64::INFINITY
            && (!speed.is_finite() || speed <= 0.0 || speed > MAX_SPEED_MULTIPLIER)
        {
            return Err(SendCommandError::InvalidSpeed(speed));
        }
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed, reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn shutdown(&self) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Shutdown { reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn set_running(&self, running: bool) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetRunning { running, reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn set_pause_preferences(
        &self,
        generation: u64,
        preferences: PausePreferences,
    ) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetPausePreferences {
                generation,
                preferences,
                reply: tx,
            })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }
}

/// 命令投递失败（actor task 已退出 / engine 拒绝意图）。显式可见，不静默吞。
#[derive(Debug, thiserror::Error)]
pub enum SendCommandError {
    /// actor task 已退出（session 被 drop 或 panic）。
    #[error("session actor gone (channel closed)")]
    ActorGone,
    /// engine 拒绝意图（如未知玩家；单玩家正常路径不应触发，但显式上抛）。
    #[error("engine rejected command: {0}")]
    Rejected(String),
    #[error(
        "speed must be Fastest (+Infinity) or a supported finite positive multiplier, got {0}"
    )]
    InvalidSpeed(f64),
}

#[derive(Debug, thiserror::Error)]
pub enum NewSessionError {
    #[error(transparent)]
    InvalidSetup(#[from] SessionError),
    #[error("session capacity reached (maximum {max})")]
    Capacity { max: usize },
}

/// Session 注册表：`DashMap<session_id, Arc<SessionHandles>>`。
///
/// `DashMap` 内部分片锁、读多写少场景高效；actor 状态本身不存于此（actor task own），
/// 这里只存消息端点——无共享可变游戏状态。
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Arc<SessionHandles>>>,
    active_count: Arc<AtomicUsize>,
    fastest_budget: Arc<Semaphore>,
    base_ms: u64,
    max_sessions: usize,
}

impl SessionManager {
    /// 默认基准（`BASE_TICK_MS`）。
    pub fn default_base() -> Self {
        Self::with_limits(BASE_TICK_MS, DEFAULT_MAX_SESSIONS)
    }

    /// 测试用：自定义 base_ms（很小的值可加速 step 以便断言事件）。
    pub fn with_base_ms(base_ms: u64) -> Self {
        Self::with_limits(base_ms, DEFAULT_MAX_SESSIONS)
    }

    pub fn with_limits(base_ms: u64, max_sessions: usize) -> Self {
        assert!(max_sessions > 0, "max_sessions must be greater than zero");
        let runtime_workers = tokio::runtime::Handle::try_current()
            .map(|handle| handle.metrics().num_workers())
            .unwrap_or_else(|error| {
                warn!(%error, "SessionManager created outside a Tokio runtime; Fastest limited to one concurrent batch");
                1
            });
        // Fastest 是持续计算负载；最多占用 N-1 个 Tokio worker，至少仍允许一个会话运行。
        // 多核机器保留一核处理 HTTP/WS、定时器和控制命令，避免合法多会话拖死整个服务。
        let fastest_permits = fastest_permits_for_workers(runtime_workers);
        Self {
            sessions: Arc::new(DashMap::new()),
            active_count: Arc::new(AtomicUsize::new(0)),
            fastest_budget: Arc::new(Semaphore::new(fastest_permits)),
            base_ms,
            max_sessions,
        }
    }

    /// 创建新 session：构造 `GameSession` → 建 mpsc+broadcast → spawn actor task → 注册。
    ///
    /// 失败显式返回 `SessionError`（构造非法参数），绝不静默吞（铁律二）。
    pub fn new_session(&self, setup: SessionSetup, seed: u64) -> Result<String, NewSessionError> {
        self.active_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < self.max_sessions).then_some(current + 1)
            })
            .map_err(|_| NewSessionError::Capacity {
                max: self.max_sessions,
            })?;
        let ticks_per_day = setup.ticks_per_day;
        let auction_ticks = setup.auction_ticks;
        let closing_auction_ticks = setup.closing_auction_ticks;
        let game = match ProtocolSession::new(setup, seed) {
            Ok(game) => game,
            Err(error) => {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(error.into());
            }
        };
        let session_id = uuid::Uuid::new_v4().to_string();

        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let (event_tx, _event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let handles = Arc::new(SessionHandles {
            cmd_tx,
            event_tx,
            ticks_per_day,
            auction_ticks,
            closing_auction_ticks,
            session_token: uuid::Uuid::new_v4().to_string(),
        });
        self.sessions.insert(session_id.clone(), handles.clone());

        let mut speed_meter = SpeedMeter::new(game.tick());
        speed_meter.mark_paused(game.tick());
        let actor = SessionActor {
            #[cfg(test)]
            injected_step_failure: None,
            speed_meter,
            game,
            cmd_rx,
            event_tx: handles.event_tx.clone(),
            tick_interval: Duration::from_millis(self.base_ms),
            base_ms: self.base_ms,
            session_id: session_id.clone(),
            running: false,
            fastest: false,
            requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
            fastest_budget: Arc::clone(&self.fastest_budget),
            public_revision: 0,
            timeline_generation: 1,
            pause_preferences: PausePreferences::default(),
            fatal_failure: None,
        };
        tokio::spawn(actor.run());
        Ok(session_id)
    }

    /// 查询 session 句柄（克隆 `Arc<SessionHandles>`）。未知返回 `None`（不静默）。
    pub fn lookup(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions.get(session_id).map(|r| Arc::clone(&r))
    }

    /// 从注册表原子移除 session。调用方随后应发送 `Shutdown` 释放 actor。
    pub fn remove(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        let removed = self.sessions.remove(session_id).map(|(_, handles)| handles);
        if removed.is_some() {
            self.active_count.fetch_sub(1, Ordering::AcqRel);
        }
        removed
    }

    pub fn active_session_count(&self) -> usize {
        self.active_count.load(Ordering::Acquire)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::default_base()
    }
}

/// actor：独占 `GameSession` 的 tokio task。命令经 `cmd_rx`，事件经 `event_tx`。
struct SessionActor {
    #[cfg(test)]
    injected_step_failure: Option<(usize, engine::session::StepFatal)>,
    speed_meter: SpeedMeter,
    game: ProtocolSession,
    cmd_rx: mpsc::Receiver<SessionCommand>,
    event_tx: broadcast::Sender<EngineUpdate>,
    /// 当前 tick 周期；保留亚毫秒精度，避免 360x/720x 被整数毫秒截断。
    tick_interval: Duration,
    base_ms: u64,
    session_id: String,
    running: bool,
    fastest: bool,
    requested_speed: RequestedSpeed,
    fastest_budget: Arc<Semaphore>,
    public_revision: u64,
    timeline_generation: u64,
    pause_preferences: PausePreferences,
    fatal_failure: Option<HostFailure>,
}

impl SessionActor {
    fn step_game(
        &mut self,
    ) -> Result<engine::session::protocol::TickFrame, engine::session::StepFatal> {
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
    /// - interval 到 → `step()` → 原子 `broadcast` 一个 EngineUpdate 批次。
    /// - `cmd_rx` 关闭（所有句柄 drop）→ 退出，task 结束。
    ///
    /// 倍速变更后下一轮重建 interval（`tokio::time::Interval` 不支持改周期，只能重建）。
    async fn run(mut self) {
        info!(session = %self.session_id, interval = ?self.tick_interval, "session actor started");

        // 初次 interval：MissedTickBehavior::Skip，提速追赶不补发积压 tick（避免 burst）。
        let mut interval = self.fresh_interval();
        // 跳过首个「立即到期」tick：开局不立刻 step，留给客户端连 WS 对齐基线。
        // 事件本身带 seq，断线重连靠 snapshot+seq 续传，不依赖开局时序。
        let _ = interval.tick().await;

        loop {
            tokio::select! {
                biased;

                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(c) => {
                            let speed_changed = matches!(c, SessionCommand::SetSpeed { .. });
                            let shutting_down = matches!(c, SessionCommand::Shutdown { .. });
                            self.handle_command(c).await;
                            if shutting_down {
                                break;
                            }
                            if speed_changed {
                                // 重建 interval 使新周期立即生效。
                                interval = self.fresh_interval();
                            }
                        }
                        None => {
                            info!(session = %self.session_id, "all handles dropped; actor exiting");
                            break;
                        }
                    }
                }
                _ = interval.tick(), if !self.fastest && self.running => {
                    self.tick_and_broadcast().await;
                }
                permit = Arc::clone(&self.fastest_budget).acquire_owned(), if self.fastest && self.running => {
                    match permit {
                        Ok(_permit) => self.run_fastest_batch(),
                        Err(_) => {
                            warn!(session = %self.session_id, "fastest CPU budget closed; actor exiting");
                            break;
                        }
                    }
                    tokio::task::yield_now().await;
                }
            }
        }
    }

    /// 构造一个按当前 tick 周期计时的新 `tokio::time::Interval`（Skip 积压补发）。
    fn fresh_interval(&self) -> tokio::time::Interval {
        let mut i = tokio::time::interval(self.tick_duration());
        i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        i
    }

    /// 推进一个 tick 并把产出的事件广播出去。
    async fn tick_and_broadcast(&mut self) {
        self.run_protocol_batch(1);
    }

    /// “最快”不使用固定 interval；每轮尽可能推进一个受控 CPU 时间片，随后由
    /// `yield_now()` 把执行权交还 Tokio，使暂停、调速、下单和快照命令不会饿死。
    fn run_fastest_batch(&mut self) {
        self.run_protocol_batch(FASTEST_BATCH_MAX_STEPS);
    }

    fn run_protocol_batch(&mut self, limit: usize) {
        if self.fatal_failure.is_some() {
            return;
        }
        let checkpoint = match self.game.checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                self.running = false;
                self.broadcast_failure("STEP_FATAL", error.to_string());
                self.fatal_failure = Some(HostFailure {
                    code: "STEP_FATAL",
                    message: error.to_string(),
                });
                return;
            }
        };
        let revision_before = self.public_revision;
        let started = std::time::Instant::now();
        let mut frames = Vec::new();
        for _ in 0..limit {
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
            #[cfg(test)]
            let injecting = self.injected_step_failure.is_some();
            #[cfg(not(test))]
            let injecting = false;
            if !injecting && started.elapsed() >= FASTEST_BATCH_BUDGET {
                break;
            }
        }
        let tick_update = if frames.is_empty() {
            None
        } else {
            match self.game.tick_batch(frames) {
                Ok(batch) => Some(ProtocolUpdate::TickBatch(batch)),
                Err(error) => {
                    self.rollback_cycle(checkpoint, error.into());
                    return;
                }
            }
        };
        let tick_date = self.game.civil_date().to_iso();
        let civil_updates = match self.prepare_civil_updates() {
            Ok(updates) => updates,
            Err(error) => {
                self.public_revision = revision_before;
                self.rollback_cycle(checkpoint, error);
                return;
            }
        };
        if let Some(update) = tick_update {
            self.broadcast_at(update, tick_date);
        }
        for update in civil_updates {
            let pause = self.pause_preferences.pauses(&update);
            self.public_revision = self.public_revision.saturating_add(1);
            let date = update.civil_date.clone();
            self.broadcast_at(ProtocolUpdate::CivilUpdate(Box::new(update)), date);
            if pause {
                self.running = false;
                self.reset_speed_meter();
            }
        }
        self.speed_meter.refresh(self.game.tick());
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

    fn rollback_cycle(
        &mut self,
        checkpoint: engine::session::protocol::ProtocolCheckpoint,
        error: SessionError,
    ) {
        let (code, mut message) = match error {
            SessionError::Step(fatal) => ("STEP_FATAL", fatal.to_string()),
            other => ("CIVIL_DAY_SETTLEMENT_FAILED", other.to_string()),
        };
        if let Err(rollback) = self.game.rollback(checkpoint) {
            message = format!("{message}; actor rollback failed: {rollback}");
        }
        self.running = false;
        self.fatal_failure = Some(HostFailure {
            code,
            message: message.clone(),
        });
        self.broadcast_failure(code, message);
    }

    #[cfg(feature = "host-parity")]
    fn broadcast_update(&self, protocol: ProtocolUpdate) {
        self.broadcast_at(protocol, self.game.civil_date().to_iso());
    }

    fn broadcast_at(&self, protocol: ProtocolUpdate, civil_date: String) {
        let update = EngineUpdate {
            timeline_generation: self.timeline_generation,
            update: Some(protocol),
            civil_date,
            public_revision: self.public_revision,
            failure: None,
        };
        // 一个 CPU 时间片只占广播通道的一个槽位，避免 Fastest 按事件数量击穿缓冲。
        if self.event_tx.send(update).is_err() {
            debug!(session = %self.session_id, "no subscribers for engine update (dropped)");
        }
        debug!(session = %self.session_id, tick = self.game.tick(), "broadcast engine update");
    }

    fn broadcast_failure(&self, code: &'static str, message: String) {
        let update = EngineUpdate {
            timeline_generation: self.timeline_generation,
            update: None,
            civil_date: self.game.civil_date().to_iso(),
            public_revision: self.public_revision,
            failure: Some(HostFailure { code, message }),
        };
        let _ = self.event_tx.send(update);
    }

    fn broadcast_timeline_reset(&self) {
        let update = EngineUpdate {
            timeline_generation: self.timeline_generation,
            update: None,
            civil_date: self.game.civil_date().to_iso(),
            public_revision: self.public_revision,
            failure: None,
        };
        let _ = self.event_tx.send(update);
    }

    /// 处理单条命令。
    async fn handle_command(&mut self, cmd: SessionCommand) {
        match cmd {
            SessionCommand::Enqueue {
                player_id,
                intent,
                reply,
            } => {
                let res = self.game.enqueue_player_intent(player_id, intent);
                if let Err(e) = &res {
                    // engine 入队失败（未知玩家等）——显式上抛，不静默吞（铁律二）。
                    warn!(session = %self.session_id, error = %e, "enqueue_player_intent failed");
                }
                // reply 失败仅说明调用方已放弃等待——不属错误。
                let _ = reply.send(res);
            }
            SessionCommand::Snapshot { reply } => {
                let snap = self.game.snapshot();
                let _ = reply.send(snap);
            }
            SessionCommand::PublicBaseline { reply } => {
                let _ = reply.send(PublicBaseline {
                    timeline_generation: self.timeline_generation,
                    snapshot: self.game.snapshot().into(),
                    civil_date: self.game.civil_date().to_iso(),
                    public_revision: self.public_revision,
                    public_report_ids: self
                        .game
                        .publication_ids()
                        .into_iter()
                        .map(|id| id.value().to_string())
                        .collect(),
                });
            }
            SessionCommand::SpeedMetrics { reply } => {
                self.speed_meter.refresh(self.game.tick());
                let _ = reply.send(SpeedMetrics {
                    requested: self.requested_speed.clone(),
                    actual_multiplier: self.speed_meter.actual_multiplier,
                    sample_duration_ms: self.speed_meter.sample_duration_ms,
                    sample_ticks: self.speed_meter.sample_ticks,
                    running: self.running,
                });
            }
            SessionCommand::Save { reply } => {
                let _ = reply.send(self.game.save());
            }
            SessionCommand::Restore { slot, reply } => match ProtocolSession::restore(&slot) {
                Ok(restored) => {
                    self.game = restored;
                    self.timeline_generation = self.timeline_generation.saturating_add(1);
                    self.public_revision = self.public_revision.saturating_add(1);
                    self.reset_speed_meter();
                    self.broadcast_timeline_reset();
                    let _ = reply.send(Ok(self.game.snapshot()));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            #[cfg(feature = "host-parity")]
            SessionCommand::AdvanceCivilDay { generation, reply } => {
                let result = if generation == self.timeline_generation {
                    self.game.end_civil_day_update()
                } else {
                    Err(SessionError::InvalidSave(format!(
                        "stale session generation {generation}; current generation is {}",
                        self.timeline_generation
                    )))
                };
                if let Ok(report) = &result {
                    self.public_revision = self.public_revision.saturating_add(1);
                    self.broadcast_update(ProtocolUpdate::CivilUpdate(Box::new(report.clone())));
                    if self.pause_preferences.pauses(report) {
                        self.running = false;
                        self.reset_speed_meter();
                    }
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.running = false;
                    self.broadcast_failure("STEP_FATAL", fatal.to_string());
                }
                let _ = reply.send(result);
            }
            #[cfg(feature = "host-parity")]
            SessionCommand::Step { generation, reply } => {
                let result = if generation == self.timeline_generation {
                    self.game.civil_day_ready().and_then(|ready| {
                        if ready {
                            return Err(SessionError::InvalidSave(
                                "civil day barrier must be published before stepping".into(),
                            ));
                        }
                        self.game
                            .step_frame()
                            .and_then(|frame| self.game.tick_batch(vec![frame]))
                            .map(ProtocolUpdate::TickBatch)
                            .map_err(SessionError::from)
                    })
                } else {
                    Err(SessionError::InvalidSave(format!(
                        "stale session generation {generation}; current generation is {}",
                        self.timeline_generation
                    )))
                };
                if let Ok(events) = &result {
                    self.broadcast_update(events.clone());
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.running = false;
                    self.broadcast_failure("STEP_FATAL", fatal.to_string());
                }
                let _ = reply.send(result);
            }
            SessionCommand::PublicReportPage { query, reply } => {
                let _ = reply.send(self.game.query_public_reports(&query));
            }
            SessionCommand::PublicReport { id, reply } => {
                let _ = reply.send(self.game.public_report_by_id(id));
            }
            SessionCommand::NpcDecisionDiagnostics {
                generation,
                account,
                reply,
            } => {
                let result = if generation == self.timeline_generation {
                    self.game.npc_decision_diagnostics(account)
                } else {
                    engine::NpcDecisionDiagnostics::Unsupported
                };
                let _ = reply.send(result);
            }
            SessionCommand::SetSpeed { speed, reply } => {
                self.apply_speed(speed);
                let _ = reply.send(());
            }
            SessionCommand::SetRunning { running, reply } => {
                if self.fatal_failure.is_none() && self.running != running {
                    self.running = running;
                    self.reset_speed_meter();
                }
                let _ = reply.send(());
            }
            SessionCommand::SetPausePreferences {
                generation,
                preferences,
                reply,
            } => {
                let result = if generation == self.timeline_generation {
                    self.pause_preferences = preferences;
                    Ok(())
                } else {
                    Err(SessionError::InvalidSave(
                        "stale timeline generation".into(),
                    ))
                };
                let _ = reply.send(result);
            }
            SessionCommand::Shutdown { reply } => {
                let _ = reply.send(());
            }
        }
    }

    /// 应用新倍速：重算 tick 周期。`run()` 在处理完 `SetSpeed` 后会重建 tokio interval。
    fn apply_speed(&mut self, speed: f64) {
        if speed == f64::INFINITY {
            self.fastest = true;
            self.requested_speed = RequestedSpeed::Fastest;
            self.reset_speed_meter();
            debug!(session = %self.session_id, "speed changed to fastest");
            return;
        }
        debug_assert!(speed.is_finite() && speed > 0.0 && speed <= MAX_SPEED_MULTIPLIER);
        self.fastest = false;
        self.requested_speed = RequestedSpeed::Fixed { multiplier: speed };
        self.reset_speed_meter();
        let new_interval = fixed_tick_duration(self.base_ms, speed);
        debug!(session = %self.session_id, old_interval = ?self.tick_interval, ?new_interval, speed, "speed changed");
        self.tick_interval = new_interval;
    }

    fn reset_speed_meter(&mut self) {
        if self.running {
            self.speed_meter.reset(self.game.tick());
        } else {
            self.speed_meter.mark_paused(self.game.tick());
        }
    }

    /// 当前 interval 对应的 `Duration`（至少 1ms）。
    fn tick_duration(&self) -> Duration {
        self.tick_interval
    }
}
