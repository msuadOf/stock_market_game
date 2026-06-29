//! actor-per-session（ADR-0005 §5）：每 session 一个 tokio task 独占 `GameSession`。
//!
//! 设计要点（无锁、契合 engine `Send`）：
//! - **无共享可变状态、无锁**：`GameSession` 由 actor task 独占 own，外部一律经**消息**与之交互。
//! - **命令通道**（`tokio::sync::mpsc`）：外部投递 `SessionCommand`（入队意图 / 取快照 / 改速）。
//! - **事件广播**（`tokio::sync::broadcast`）：actor 每 tick `step()` 产 `Event[]`，逐条 broadcast，
//!   订阅者（WS 连接等）各自消费。`Event` 自带单调 `seq`（断线重连按 seq 续传）。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 单玩家 v1：意图固定路由给玩家 `AccountId(0)`（见 `enqueue`）。

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use engine::{AccountId, Event, GameSession, Intent, SessionError, SessionSetup, Snapshot};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, info, warn};

/// 倍速基准：1x 时一个 tick 的间隔毫秒数（与前端 RemoteHost 对齐的「真实时间」尺度）。
/// 取 1000ms（1 秒一 tick）作为可感知默认；speed=N → interval = BASE_TICK_MS / N。
pub const BASE_TICK_MS: u64 = 1000;

/// 广播事件通道容量：留足缓冲以应对慢消费者短时积压；超过则 broadcast 丢旧（lagged），
/// 订阅者靠 `seq` 检测缺口后拉快照对齐（ADR-0005 §6）。
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// 命令通道容量：意图/查询短小，32 足够积压；满了显式 `await` 背压，绝不静默丢命令。
const COMMAND_CHANNEL_CAPACITY: usize = 32;

/// 发给 actor 的命令。每条命令都自带 `oneshot` 回执通道——actor 处理后 `reply`，调用方拿 `Result`。
///
/// `reply` 用 `Result<...>` 而非裸值：engine 失败（`SessionError`）显式上抛，绝不静默吞（铁律二）。
#[derive(Debug)]
pub enum SessionCommand {
    /// 入队玩家意图（v1 固定玩家 0）。Ok=已入队，Err=未知玩家（不应发生，账户恒存在）。
    Enqueue {
        player_id: AccountId,
        intent: Intent,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 取完整快照。Ok=快照值。
    Snapshot { reply: oneshot::Sender<Snapshot> },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed { speed: f64 },
}

/// 一个 session 的对外句柄：命令发送端 + 事件广播端。
///
/// 克隆廉价（`mpsc::Sender` / `broadcast::Sender` 均可 clone）。`SessionManager` 持有
/// `Arc<SessionHandles>`，路由层与 WS 连接经 manager 取一份克隆与 actor 通信。
#[derive(Clone)]
pub struct SessionHandles {
    pub cmd_tx: mpsc::Sender<SessionCommand>,
    pub event_tx: broadcast::Sender<Event>,
}

impl SessionHandles {
    /// 便捷：入队玩家意图（v1 固定 `AccountId(0)`）。把 `oneshot` 收发封装成 `Result` 返回。
    ///
    /// 失败两种：actor 已退出（通道关闭）→ `SendCommandError`；engine 拒绝 → `SessionError`。
    /// 两者都显式上抛，不静默。
    pub async fn enqueue(&self, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(AccountId(0), intent).await
    }

    /// 入队指定玩家的意图（联机多账户预留接口，v1 仍固定 player 0 调用）。
    pub async fn enqueue_as(
        &self,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Enqueue { player_id, intent, reply: tx })
            .await
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|_| SendCommandError::Rejected)
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

    /// 改变倍速。fire-and-forget 经 mpsc 保证顺序（在 Enqueue/Snapshot 之后生效），
    /// 但若 actor 已关闭则返回 `ActorGone`（不静默）。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed })
            .await
            .map_err(|_| SendCommandError::ActorGone)
    }
}

/// 命令投递失败（actor task 已退出 / engine 拒绝意图）。显式可见，不静默吞。
#[derive(Debug, thiserror::Error)]
pub enum SendCommandError {
    /// actor task 已退出（session 被 drop 或 panic）。
    #[error("session actor gone (channel closed)")]
    ActorGone,
    /// engine 拒绝意图（如未知玩家；v1 不应触发，但显式上抛）。
    #[error("intent rejected by engine")]
    Rejected,
}

/// Session 注册表：`DashMap<session_id, Arc<SessionHandles>>`。
///
/// `DashMap` 内部分片锁、读多写少场景高效；actor 状态本身不存于此（actor task own），
/// 这里只存消息端点——无共享可变游戏状态。
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Arc<SessionHandles>>>,
    base_ms: u64,
}

impl SessionManager {
    /// 默认基准（`BASE_TICK_MS`）。
    pub fn default_base() -> Self {
        Self { sessions: Arc::new(DashMap::new()), base_ms: BASE_TICK_MS }
    }

    /// 测试用：自定义 base_ms（很小的值可加速 step 以便断言事件）。
    pub fn with_base_ms(base_ms: u64) -> Self {
        Self { sessions: Arc::new(DashMap::new()), base_ms }
    }

    /// 创建新 session：构造 `GameSession` → 建 mpsc+broadcast → spawn actor task → 注册。
    ///
    /// 失败显式返回 `SessionError`（构造非法参数），绝不静默吞（铁律二）。
    pub fn new_session(&self, setup: SessionSetup, seed: u64) -> Result<String, SessionError> {
        let game = GameSession::new(setup, seed)?;
        let session_id = uuid::Uuid::new_v4().to_string();

        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let (event_tx, _event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let handles = Arc::new(SessionHandles { cmd_tx, event_tx });
        self.sessions.insert(session_id.clone(), handles.clone());

        let actor = SessionActor {
            game,
            cmd_rx,
            event_tx: handles.event_tx.clone(),
            interval_ms: self.base_ms,
            base_ms: self.base_ms,
            session_id: session_id.clone(),
        };
        tokio::spawn(actor.run());
        Ok(session_id)
    }

    /// 查询 session 句柄（克隆 `Arc<SessionHandles>`）。未知返回 `None`（不静默）。
    pub fn lookup(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions.get(session_id).map(|r| Arc::clone(&r))
    }

    /// 删除 session（预留：断开清理 / TTL）。返回是否曾存在。
    #[allow(dead_code)]
    pub fn remove(&self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::default_base()
    }
}

/// actor：独占 `GameSession` 的 tokio task。命令经 `cmd_rx`，事件经 `event_tx`。
struct SessionActor {
    game: GameSession,
    cmd_rx: mpsc::Receiver<SessionCommand>,
    event_tx: broadcast::Sender<Event>,
    /// 当前 interval（毫秒）。`base_ms / speed`。
    interval_ms: u64,
    base_ms: u64,
    session_id: String,
}

impl SessionActor {
    /// 主循环：`select!` 同时等命令与 interval tick。
    ///
    /// - 命令到达 → 处理（Enqueue→入队、Snapshot→回快照、SetSpeed→改 interval）。
    /// - interval 到 → `step()` → 逐条 `broadcast` Event。
    /// - `cmd_rx` 关闭（所有句柄 drop）→ 退出，task 结束。
    ///
    /// 倍速变更后下一轮重建 interval（`tokio::time::Interval` 不支持改周期，只能重建）。
    async fn run(mut self) {
        info!(session = %self.session_id, interval_ms = self.interval_ms, "session actor started");

        // 初次 interval：MissedTickBehavior::Skip，提速追赶不补发积压 tick（避免 burst）。
        let mut interval = self.fresh_interval();
        // 跳过首个「立即到期」tick：开局不立刻 step，留给客户端连 WS 对齐基线。
        // 事件本身带 seq，断线重连靠 snapshot+seq 续传，不依赖开局时序。
        let _ = interval.tick().await;

        loop {
            tokio::select! {
                biased; // 优先消费命令，避免被高频 step 饿死控制路径。

                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(c) => {
                            let speed_changed = matches!(c, SessionCommand::SetSpeed { .. });
                            self.handle_command(c).await;
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
                _ = interval.tick() => {
                    self.tick_and_broadcast().await;
                }
            }
        }
    }

    /// 构造一个按当前 `interval_ms` 计时的新 `tokio::time::Interval`（Skip 积压补发）。
    fn fresh_interval(&self) -> tokio::time::Interval {
        let mut i = tokio::time::interval(self.tick_duration());
        i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        i
    }

    /// 推进一个 tick 并把产出的事件广播出去。
    async fn tick_and_broadcast(&mut self) {
        let events = self.game.step();
        if events.is_empty() {
            return;
        }
        let n = events.len();
        for ev in events {
            // 广播失败仅意味着无订阅者（或全部 lagged 后已丢弃）——不属错误，debug 记录即可。
            if self.event_tx.send(ev).is_err() {
                debug!(session = %self.session_id, "no subscribers for event (dropped)");
            }
        }
        debug!(session = %self.session_id, events = n, tick = self.game.tick(), "broadcast events");
    }

    /// 处理单条命令。
    async fn handle_command(&mut self, cmd: SessionCommand) {
        match cmd {
            SessionCommand::Enqueue { player_id, intent, reply } => {
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
            SessionCommand::SetSpeed { speed } => {
                self.apply_speed(speed);
            }
        }
    }

    /// 应用新倍速：重算 `interval_ms`。`run()` 在处理完 `SetSpeed` 后会重建 tokio interval。
    fn apply_speed(&mut self, speed: f64) {
        // 防御：speed 非正/非有限 → 拒绝（保持原速），不静默用 0 导致除零/死循环。
        if !speed.is_finite() || speed <= 0.0 {
            warn!(session = %self.session_id, speed, "invalid speed ignored (must be finite >0)");
            return;
        }
        let new_ms = ((self.base_ms as f64) / speed).max(1.0) as u64;
        debug!(session = %self.session_id, old_ms = self.interval_ms, new_ms, speed, "speed changed");
        self.interval_ms = new_ms;
    }

    /// 当前 interval 对应的 `Duration`（至少 1ms）。
    fn tick_duration(&self) -> Duration {
        Duration::from_millis(self.interval_ms.max(1))
    }
}
