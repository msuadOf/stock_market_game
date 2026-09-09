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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine::{AccountId, GameSession, Intent, SaveSlot, SessionError, SessionSetup, Snapshot};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

use crate::EngineEventPayload;

/// 倍速基准：1x 时一个 tick 的间隔毫秒数。
/// 与前端 BASE_INTERVAL_MS(1000) 对齐：1x 时一个 tick = 游戏世界 1 秒。
pub const BASE_TICK_MS: u64 = 1000;

/// 命令通道容量：意图/查询短小，32 足够积压；满了 `await` 背压，绝不静默丢命令。
const COMMAND_CHANNEL_CAPACITY: usize = 32;
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

/// 发给 actor 的命令。每条都自带 `oneshot` 回执（`SetSpeed` 除外：fire-and-forget）。
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
    /// 取不含 360 日历史的轻量运行快照，供高倍率跨日同步。
    RuntimeSnapshot { reply: oneshot::Sender<Snapshot> },
    /// 读取设定速度与最近完成的实际 tick/现实秒采样。
    SpeedMetrics {
        reply: oneshot::Sender<SpeedMetrics>,
    },
    /// 生成可持久化存档。actor 独占会话，因此读取与 step 严格串行。
    Save { reply: oneshot::Sender<SaveSlot> },
    /// 原子恢复存档：只有完整校验和重建成功后才替换当前会话。
    Restore {
        slot: Box<SaveSlot>,
        reply: oneshot::Sender<Result<RestoreResult, SessionError>>,
    },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed { speed: f64 },
    /// 暂停或恢复步进，保留会话状态与事件订阅。
    SetRunning { running: bool },
    /// 永久结束 actor；用于页面卸载/应用退出释放资源。
    Shutdown,
}

/// 一个 session 的对外句柄：命令发送端（克隆廉价）。
/// `SessionManager` 持 `Arc<SessionHandles>`，Tauri command 经 manager 取克隆与 actor 通信。
#[derive(Clone)]
pub struct SessionHandles {
    pub cmd_tx: mpsc::Sender<SessionCommand>,
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
            .await
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
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    /// 取轻量运行快照，避免跨日反复复制全部历史 K 线。
    pub async fn runtime_snapshot(&self) -> Result<Snapshot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::RuntimeSnapshot { reply: tx })
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
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn restore(&self, slot: SaveSlot) -> Result<RestoreResult, SendCommandError> {
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

    /// 改变倍速。fire-and-forget 经 mpsc 保证顺序；actor 已关闭则 `ActorGone`（不静默）。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed })
            .await
            .map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn set_running(&self, running: bool) -> Result<(), SendCommandError> {
        self.cmd_tx
            .send(SessionCommand::SetRunning { running })
            .await
            .map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn shutdown(&self) -> Result<(), SendCommandError> {
        self.cmd_tx
            .send(SessionCommand::Shutdown)
            .await
            .map_err(|_| SendCommandError::ActorGone)
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
    pub async fn new_session(
        &self,
        setup: SessionSetup,
        seed: u64,
        app: AppHandle,
    ) -> Result<String, SessionError> {
        let ticks_per_day = setup.ticks_per_day;
        let auction_ticks = setup.auction_ticks;
        let game = GameSession::new(setup, seed)?;
        let session_id = uuid::Uuid::new_v4().to_string();

        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let handles = Arc::new(SessionHandles { cmd_tx });

        // 异步锁：Tauri command 在 Tokio runtime 内调用，禁止 blocking_lock 导致 panic。
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), handles.clone());

        let mut speed_meter = SpeedMeter::new(game.tick());
        speed_meter.mark_paused(game.tick());
        let actor = SessionActor {
            speed_meter,
            game,
            cmd_rx,
            tick_interval: Duration::from_millis(self.base_ms),
            base_ms: self.base_ms,
            session_id: session_id.clone(),
            app,
            running: false,
            fastest: false,
            requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
            ticks_per_day,
            auction_ticks,
            pending_fixed_events: Vec::new(),
            last_fixed_publish: Instant::now(),
            timeline_id: session_id.clone(),
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
struct SessionActor {
    speed_meter: SpeedMeter,
    game: GameSession,
    cmd_rx: mpsc::Receiver<SessionCommand>,
    /// 固定倍率的精确 tick 周期；使用 Duration 避免 60x/180x 等被整数毫秒截断。
    tick_interval: Duration,
    base_ms: u64,
    session_id: String,
    /// Tauri 应用句柄：emit 事件给前端窗口。
    app: AppHandle,
    running: bool,
    fastest: bool,
    requested_speed: RequestedSpeed,
    ticks_per_day: u64,
    auction_ticks: u64,
    /// 固定高倍速在 Rust 侧聚合到 16ms 再跨 IPC，避免每 tick 唤醒 WebView。
    pending_fixed_events: Vec<engine::Event>,
    last_fixed_publish: Instant,
    /// 每次成功读档都会更换；前端据此拒绝晚到的旧时间线 IPC。
    timeline_id: String,
}

impl SessionActor {
    /// 主循环：`select!` 同时等命令与 interval tick。
    ///
    /// - 命令到达 → 处理（Enqueue→入队、Snapshot→回快照、SetSpeed→改 interval）。
    /// - interval 到 → `step()` → `app.emit("engine-event", ...)`。
    /// - `cmd_rx` 关闭（所有句柄 drop）→ 退出，task 结束。
    async fn run(mut self) {
        let mut interval = self.fresh_interval();
        // 跳过首个「立即到期」tick：开局不立刻 step，留给前端连监听对齐基线。
        // 事件本身带 seq，断线重连靠 snapshot+seq 续传，不依赖开局时序。
        let _ = interval.tick().await;

        loop {
            tokio::select! {
                biased; // 优先消费命令，避免被高频 step 饿死控制路径。

                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(c) => {
                            // 命令与 step 严格排序；先发布命令前已经产生的事件，避免读档、
                            // 暂停或切速让不足 16ms 的尾批次滞留或跨越新基线。
                            self.flush_fixed_events();
                            let speed_changed = matches!(c, SessionCommand::SetSpeed { .. });
                            let shutting_down = matches!(c, SessionCommand::Shutdown);
                            self.handle_command(c).await;
                            if shutting_down {
                                break;
                            }
                            if speed_changed {
                                interval = self.fresh_interval();
                            }
                        }
                        None => break,
                    }
                }
                _ = interval.tick(), if !self.fastest && self.running => {
                    self.tick_and_emit().await;
                }
                _ = tokio::task::yield_now(), if self.fastest && self.running => {
                    self.run_fastest_batch().await;
                }
            }
        }
    }

    /// 构造固定倍率计时器（Skip 积压补发）。
    fn fresh_interval(&self) -> tokio::time::Interval {
        let mut i = tokio::time::interval(self.tick_interval);
        i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        i
    }

    /// 推进一个 tick 并把产出的事件 emit 给前端。
    ///
    /// emit 失败（窗口已关闭等）不算致命——actor 继续；下一 tick 自然不再有消费者。
    /// 这里不 panic：游戏循环与 UI 解耦，UI 关闭应允许循环自然结束（cmd_tx drop 后退出）。
    async fn tick_and_emit(&mut self) {
        let events = self.game.step();
        if let Some(batch) = take_publish_batch(
            &mut self.pending_fixed_events,
            events,
            self.last_fixed_publish.elapsed(),
        ) {
            self.last_fixed_publish = Instant::now();
            self.emit_events(batch);
        }
        self.speed_meter.refresh(self.game.tick());
    }

    fn flush_fixed_events(&mut self) {
        if self.pending_fixed_events.is_empty() {
            return;
        }
        self.last_fixed_publish = Instant::now();
        let events = std::mem::take(&mut self.pending_fixed_events);
        self.emit_events(events);
    }

    /// “最快”不经过定时器：在一个受控 CPU 时间片内 tight-loop，再批量 emit 一次，
    /// 既让处理器全力推进，也避免每 tick 一次 IPC 的消息风暴。
    async fn run_fastest_batch(&mut self) {
        let started = std::time::Instant::now();
        let mut events = Vec::new();
        let mut steps = 0;
        while steps < FASTEST_BATCH_MAX_STEPS && started.elapsed() < FASTEST_BATCH_BUDGET {
            events.extend(self.game.step());
            steps += 1;
        }
        let Some(from_seq) = events.first().map(engine::Event::seq) else {
            return;
        };
        let to_seq = events.last().map(engine::Event::seq).expect("已确认非空");
        self.emit_events_with_coverage(
            compact_fastest_events(events, self.ticks_per_day, self.auction_ticks),
            from_seq,
            to_seq,
        );
        self.speed_meter.refresh(self.game.tick());
    }

    fn emit_events(&mut self, events: Vec<engine::Event>) {
        if events.is_empty() {
            return;
        }
        let from_seq = events.first().map(engine::Event::seq).expect("已确认非空");
        let to_seq = events.last().map(engine::Event::seq).expect("已确认非空");
        self.emit_events_with_coverage(events, from_seq, to_seq);
    }

    fn emit_events_with_coverage(
        &mut self,
        events: Vec<engine::Event>,
        from_seq: u64,
        to_seq: u64,
    ) {
        if events.is_empty() {
            return;
        }
        let runtime_snapshot = events
            .iter()
            .any(|event| {
                matches!(
                    event,
                    engine::Event::Trade { .. }
                        | engine::Event::OrderAccepted { .. }
                        | engine::Event::OrderCanceled { .. }
                        | engine::Event::AuctionCompleted { .. }
                        | engine::Event::DayBoundary { .. }
                )
            })
            .then(|| self.game.runtime_snapshot());
        let payload = EngineEventPayload {
            session_id: self.session_id.clone(),
            timeline_id: self.timeline_id.clone(),
            events,
            from_seq,
            to_seq,
            runtime_snapshot,
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
            SessionCommand::RuntimeSnapshot { reply } => {
                let snap = self.game.runtime_snapshot();
                let _ = reply.send(snap);
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
            SessionCommand::Restore { slot, reply } => match GameSession::restore(&slot) {
                Ok(restored) => {
                    self.game = restored;
                    self.timeline_id = uuid::Uuid::new_v4().to_string();
                    self.reset_speed_meter();
                    let _ = reply.send(Ok(RestoreResult {
                        snapshot: self.game.snapshot(),
                        timeline_id: self.timeline_id.clone(),
                    }));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            SessionCommand::SetSpeed { speed } => {
                self.apply_speed(speed);
            }
            SessionCommand::SetRunning { running } => {
                if self.running != running {
                    self.running = running;
                    self.reset_speed_meter();
                }
            }
            SessionCommand::Shutdown => {}
        }
    }

    /// 应用新倍速：固定倍率使用精确 Duration；Fastest 切到 CPU 时间片 tight-loop。
    fn apply_speed(&mut self, speed: f64) {
        if speed == f64::INFINITY {
            self.fastest = true;
            self.requested_speed = RequestedSpeed::Fastest;
            self.reset_speed_meter();
            return;
        }
        // 防御：除专用 Fastest 哨兵外，非正/非有限值一律拒绝。
        if !speed.is_finite() || speed <= 0.0 {
            eprintln!(
                "[session {}] 非法速度被忽略（须为有限正数）：{speed}",
                self.session_id
            );
            return;
        }
        self.fastest = false;
        self.requested_speed = RequestedSpeed::Fixed { multiplier: speed };
        self.reset_speed_meter();
        self.tick_interval = fixed_tick_interval(self.base_ms, speed);
        self.last_fixed_publish = Instant::now();
    }

    fn reset_speed_meter(&mut self) {
        if self.running {
            self.speed_meter.reset(self.game.tick());
        } else {
            self.speed_meter.mark_paused(self.game.tick());
        }
    }
}

fn take_publish_batch<T>(
    pending: &mut Vec<T>,
    incoming: Vec<T>,
    elapsed: Duration,
) -> Option<Vec<T>> {
    pending.extend(incoming);
    (elapsed >= UI_PUBLISH_INTERVAL && !pending.is_empty()).then(|| std::mem::take(pending))
}

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
    use super::{compact_fastest_events, fixed_tick_interval, take_publish_batch, SpeedMeter};
    use engine::{DailyCandle, Event, Money, StockCode};
    use std::time::Duration;

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
    fn fastest_compaction_keeps_boundaries_and_latest_tick_per_minute() {
        let price_tick = |seq, tick| Event::PriceTick {
            seq,
            tick,
            code: StockCode("AAA".into()),
            last_price: Money::from_cents(100),
            daily_candle: DailyCandle {
                time: 0,
                open: Money::from_cents(100),
                high: Money::from_cents(100),
                low: Money::from_cents(100),
                close: Money::from_cents(100),
                volume: 0,
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
        assert!(matches!(compacted[1], Event::PriceTick { seq: 4, .. }));
    }

    #[test]
    fn fastest_compaction_keeps_six_second_auction_slots_and_completion() {
        let auction_tick = |seq, tick| Event::AuctionTick {
            seq,
            tick,
            code: StockCode("AAA".into()),
            indicative_price: Some(Money::from_cents(100)),
            matched_volume: seq,
            imbalance: 0,
        };
        let completed = Event::AuctionCompleted {
            seq: 4,
            tick: 900,
            code: StockCode("AAA".into()),
            opening_price: Some(Money::from_cents(100)),
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
