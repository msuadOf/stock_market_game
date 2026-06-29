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
//! 单玩家 v1：意图固定路由给玩家 `AccountId(0)`（见 `enqueue`）。

use std::sync::Arc;
use std::time::Duration;

use engine::{AccountId, GameSession, Intent, SessionError, SessionSetup, Snapshot};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

use crate::EngineEventPayload;

/// 倍速基准：1x 时一个 tick 的间隔毫秒数。
/// 与前端 BASE_INTERVAL_MS(600) 对齐——桌面端走「真实时间」尺度，1x ≈ 0.6s/tick。
pub const BASE_TICK_MS: u64 = 600;

/// 命令通道容量：意图/查询短小，32 足够积压；满了 `await` 背压，绝不静默丢命令。
const COMMAND_CHANNEL_CAPACITY: usize = 32;

/// 发给 actor 的命令。每条都自带 `oneshot` 回执（`SetSpeed` 除外：fire-and-forget）。
///
/// `reply` 用 `Result` 而非裸值：engine 失败（`SessionError`）显式上抛，绝不静默吞（铁律二）。
#[derive(Debug)]
pub enum SessionCommand {
    /// 入队玩家意图（v1 固定玩家 0）。Ok=已入队，Err=engine 拒绝。
    Enqueue {
        player_id: AccountId,
        intent: Intent,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 取完整快照。
    Snapshot {
        reply: oneshot::Sender<Snapshot>,
    },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed { speed: f64 },
}

/// 一个 session 的对外句柄：命令发送端（克隆廉价）。
/// `SessionManager` 持 `Arc<SessionHandles>`，Tauri command 经 manager 取克隆与 actor 通信。
#[derive(Clone)]
pub struct SessionHandles {
    pub cmd_tx: mpsc::Sender<SessionCommand>,
}

impl SessionHandles {
    /// 便捷：入队玩家意图（v1 固定 `AccountId(0)`）。把 `oneshot` 收发封装成 `Result`。
    pub async fn enqueue(&self, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(AccountId(0), intent).await
    }

    /// 入队指定玩家意图（联机多账户预留，v1 仍固定 player 0 调用）。
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
            .map_err(|_| SendCommandError::Rejected)
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

    /// 改变倍速。fire-and-forget 经 mpsc 保证顺序；actor 已关闭则 `ActorGone`（不静默）。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed })
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
    /// engine 拒绝意图（如未知玩家；v1 不应触发，但显式上抛）。
    #[error("意图被引擎拒绝")]
    Rejected,
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
    pub fn new_session(
        &self,
        setup: SessionSetup,
        seed: u64,
        app: AppHandle,
    ) -> Result<String, SessionError> {
        let game = GameSession::new(setup, seed)?;
        let session_id = uuid::Uuid::new_v4().to_string();

        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let handles = Arc::new(SessionHandles { cmd_tx });

        // 同步锁：注册表操作瞬时完成，不持锁跨 await。
        self.sessions
            .blocking_lock()
            .insert(session_id.clone(), handles.clone());

        let actor = SessionActor {
            game,
            cmd_rx,
            interval_ms: self.base_ms,
            base_ms: self.base_ms,
            session_id: session_id.clone(),
            app,
        };
        tokio::spawn(actor.run());
        Ok(session_id)
    }

    /// 查询 session 句柄（克隆 `Arc<SessionHandles>`）。未知返回 `None`（不静默）。
    pub fn lookup(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions
            .blocking_lock()
            .get(session_id)
            .map(Arc::clone)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::default_base()
    }
}

/// actor：独占 `GameSession` 的 tokio task。命令经 `cmd_rx`，事件经 `app.emit`。
struct SessionActor {
    game: GameSession,
    cmd_rx: mpsc::Receiver<SessionCommand>,
    /// 当前 interval（毫秒）。`base_ms / speed`。
    interval_ms: u64,
    base_ms: u64,
    session_id: String,
    /// Tauri 应用句柄：emit 事件给前端窗口。
    app: AppHandle,
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
                            let speed_changed = matches!(c, SessionCommand::SetSpeed { .. });
                            self.handle_command(c).await;
                            if speed_changed {
                                interval = self.fresh_interval();
                            }
                        }
                        None => break,
                    }
                }
                _ = interval.tick() => {
                    self.tick_and_emit().await;
                }
            }
        }
    }

    /// 构造按当前 `interval_ms` 计时的新 `tokio::time::Interval`（Skip 积压补发）。
    fn fresh_interval(&self) -> tokio::time::Interval {
        let mut i = tokio::time::interval(self.tick_duration());
        i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        i
    }

    /// 推进一个 tick 并把产出的事件 emit 给前端。
    ///
    /// emit 失败（窗口已关闭等）不算致命——actor 继续；下一 tick 自然不再有消费者。
    /// 这里不 panic：游戏循环与 UI 解耦，UI 关闭应允许循环自然结束（cmd_tx drop 后退出）。
    async fn tick_and_emit(&mut self) {
        let events = self.game.step();
        if events.is_empty() {
            return;
        }
        let payload = EngineEventPayload {
            session_id: self.session_id.clone(),
            events,
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
            SessionCommand::SetSpeed { speed } => {
                self.apply_speed(speed);
            }
        }
    }

    /// 应用新倍速：重算 `interval_ms`。`run()` 处理完后重建 interval。
    fn apply_speed(&mut self, speed: f64) {
        // 防御：speed 非正/非有限 → 拒绝（保持原速），不静默用 0 导致除零/死循环。
        if !speed.is_finite() || speed <= 0.0 {
            eprintln!(
                "[session {}] 非法速度被忽略（须为有限正数）：{speed}",
                self.session_id
            );
            return;
        }
        let new_ms = ((self.base_ms as f64) / speed).max(1.0) as u64;
        self.interval_ms = new_ms;
    }

    /// 当前 interval 对应的 `Duration`（至少 1ms）。
    fn tick_duration(&self) -> Duration {
        Duration::from_millis(self.interval_ms.max(1))
    }
}
