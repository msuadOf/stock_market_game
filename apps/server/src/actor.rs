//! actor-per-session（ADR-0005 §5）：每 session 一个 tokio task 独占 `GameSession`。
//!
//! 设计要点（市场状态独占、契合 engine `Send`）：
//! - `GameSession` 由 actor task 独占；Player 与 NPC 共用每会话 ingress，短生命周期锁不覆盖 step。
//! - Player 在鉴权与 generation 检查后直接登记 ingress；控制命令（快照 / 改速 / restore）走 mpsc。
//! - **更新广播**（`tokio::sync::broadcast`）：actor 每轮把 `Event[]` 与必要的权威运行快照
//!   作为一个原子批次 broadcast；WS 只是传输适配器，与 Worker/Tauri 的应用层语义一致。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 当前单玩家模式：意图固定路由给玩家 `AccountId(0)`（见 `enqueue`）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use dashmap::DashMap;
#[cfg(feature = "host-parity")]
use engine::session::protocol::CivilUpdate;
use engine::session::protocol::{
    EngineUpdate as ProtocolUpdate, PausePreferences, ProtocolSession,
};
use engine::session::{MarketMembership, OpaqueSubjectId};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup, Snapshot, StockCode};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HostFailure {
    pub code: &'static str,
    pub message: String,
    pub r#where: String,
    pub cause: Option<Box<FailureCause>>,
    pub context: FailureContext,
    pub recoverable: bool,
    #[serde(rename = "recoveryActions")]
    pub recovery_actions: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FailureContext {
    pub operation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FailureCause {
    code: &'static str,
    message: String,
    cause: Option<Box<FailureCause>>,
}

fn failure_description(error: &(dyn std::error::Error + 'static)) -> (&'static str, &'static str) {
    if let Some(error) = error.downcast_ref::<SessionError>() {
        return match error {
            SessionError::Closing(_) => (
                "SESSION_CLOSING_FAILED",
                "自然日日终封账失败（私有详情已脱敏）",
            ),
            SessionError::InvalidSave(_) => (
                "SESSION_STATE_INVALID",
                "日终存档或协议状态校验失败（原始详情已脱敏）",
            ),
            SessionError::Step(_) => (
                "INVARIANT_VIOLATION",
                "引擎不变量校验失败（原始详情已脱敏）",
            ),
            _ => ("SESSION_OPERATION_FAILED", "会话操作失败（原始详情已脱敏）"),
        };
    }
    if let Some(error) = error.downcast_ref::<engine::accounting::closing::ClosingError>() {
        return match error {
            engine::accounting::closing::ClosingError::Accounting(_) => (
                "CLOSING_ACCOUNTING_FAILED",
                "日终封账的会计处理失败（私有详情已脱敏）",
            ),
            engine::accounting::closing::ClosingError::Report(_) => (
                "CLOSING_REPORT_FAILED",
                "日终封账的报表处理失败（私有详情已脱敏）",
            ),
            _ => (
                "CLOSING_VALIDATION_FAILED",
                "日终封账校验失败（私有详情已脱敏）",
            ),
        };
    }
    if let Some(error) = error.downcast_ref::<engine::accounting::AccountingError>() {
        return match error {
            engine::accounting::AccountingError::AmountOverflow { .. } => (
                "ACCOUNTING_AMOUNT_OVERFLOW",
                "公司会计金额运算溢出（操作数已脱敏）",
            ),
            _ => (
                "ACCOUNTING_VALIDATION_FAILED",
                "公司会计校验失败（私有详情已脱敏）",
            ),
        };
    }
    if error.is::<engine::session::StepFatal>() {
        return (
            "INVARIANT_VIOLATION",
            "引擎不变量校验失败（原始详情已脱敏）",
        );
    }
    ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
}

fn failure_cause(error: &(dyn std::error::Error + 'static)) -> Option<Box<FailureCause>> {
    error.source().map(|source| {
        let (code, message) = failure_description(source);
        Box::new(FailureCause {
            code,
            message: message.to_owned(),
            cause: failure_cause(source),
        })
    })
}

impl FailureContext {
    fn new(operation: &'static str) -> Self {
        Self {
            operation,
            tick: None,
            seq: None,
            day: None,
            generation: None,
        }
    }
}

impl HostFailure {
    pub(crate) fn step(error: &engine::session::StepFatal) -> Self {
        let engine::session::StepFatal::InvariantViolation { location, .. } = error;
        Self {
            code: "STEP_FATAL",
            message: failure_description(error).1.to_owned(),
            r#where: location.clone(),
            cause: failure_cause(error),
            context: FailureContext::new("step"),
            recoverable: false,
            recovery_actions: vec![
                "停止当前会话；重新打开上一份有效日终存档或新局",
                "复制脱敏错误详情反馈",
            ],
        }
    }

    fn civil(error: &SessionError) -> Self {
        match error {
            SessionError::Step(fatal) => Self::step(fatal),
            other => Self {
                code: "CIVIL_DAY_SETTLEMENT_FAILED",
                message: failure_description(other).1.to_owned(),
                r#where: "server.actor.rollback_cycle".into(),
                cause: failure_cause(other),
                context: FailureContext::new("endCivilDay"),
                recoverable: false,
                recovery_actions: vec![
                    "停止当前会话；重新打开上一份有效日终存档或新局",
                    "复制脱敏错误详情反馈",
                ],
            },
        }
    }

    /// B3 远程登录（18725d07 语义）：日终已完成、SQLite 保存失败——上一份
    /// 有效日终档保留，恢复推进后下一日终重试。**可恢复**失败（交易提交
    /// 有效），只广播不致命停机。
    pub(crate) fn archive(error: &str) -> Self {
        Self {
            code: "DAY_END_ARCHIVE_FAILED",
            message: format!("日终已成功完成，但 SQLite 保存失败，上一份有效日终档仍保留：{error}"),
            r#where: "server.actor.save_day_end".into(),
            cause: None,
            context: FailureContext::new("saveDayEnd"),
            recoverable: true,
            recovery_actions: vec![
                "检查 SQLite 文件权限与磁盘空间后恢复推进，在下一日终重试保存",
                "复制存档错误详情反馈",
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicBaseline {
    pub timeline_generation: u64,
    pub snapshot: PublicBaselineSnapshot,
    pub civil_date: String,
    pub public_revision: u64,
    pub public_report_ids: Vec<String>,
    #[serde(skip)]
    pub failure: Option<HostFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicBaselineSnapshot {
    pub seq: u64,
    pub tick: u64,
    pub day: u32,
    pub phase: engine::TradingPhase,
    pub markets: std::collections::BTreeMap<engine::StockId, engine::MarketSnap>,
    pub accounts: std::collections::BTreeMap<AccountId, engine::AccountSnap>,
    pub daily_candles: std::collections::BTreeMap<engine::StockId, Vec<engine::DailyCandle>>,
    pub active_daily_candles: std::collections::BTreeMap<engine::StockId, engine::DailyCandle>,
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

/// actor 独占的宿主节奏状态；引擎事务与调度资源仍归 SessionActor。
struct ServerPacing {
    running: bool,
    fastest: bool,
    requested_speed: RequestedSpeed,
    tick_interval: Duration,
    base_ms: u64,
    speed_meter: SpeedMeter,
}

impl ServerPacing {
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

    fn apply_speed(&mut self, speed: f64, tick: u64) {
        if speed == f64::INFINITY {
            self.fastest = true;
            self.requested_speed = RequestedSpeed::Fastest;
            self.reset_speed_meter(tick);
            return;
        }
        debug_assert!(speed.is_finite() && speed > 0.0 && speed <= MAX_SPEED_MULTIPLIER);
        self.fastest = false;
        self.requested_speed = RequestedSpeed::Fixed { multiplier: speed };
        self.reset_speed_meter(tick);
        self.tick_interval = fixed_tick_duration(self.base_ms, speed);
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

/// 广播事件通道容量：留足缓冲以应对慢消费者短时积压；超过则 broadcast 丢旧（lagged），
/// 订阅者靠 `seq` 检测缺口后拉快照对齐（ADR-0005 §6）。
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// 草稿阶段固定保留两局的容纳能力；当前开发与验收集中于单局市场。
const MAX_SESSIONS: usize = 2;

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
    PlayerWorkingOrders {
        generation: u64,
        reply: oneshot::Sender<Result<(u64, serde_json::Value), SendCommandError>>,
    },
    StockHistory {
        generation: u64,
        code: engine::StockId,
        reply:
            oneshot::Sender<Result<(u64, engine::session::HistoricalStockData), SendCommandError>>,
    },
    InitialAllocation {
        generation: u64,
        reply: oneshot::Sender<Result<(u64, engine::session::InitialAllocation), SendCommandError>>,
    },
    PublicBaseline {
        reply: oneshot::Sender<PublicBaseline>,
    },
    SpeedMetrics {
        reply: oneshot::Sender<SpeedMetrics>,
    },
    Save {
        generation: u64,
        candidate: Option<engine::session::protocol::SaveCandidateKey>,
        reply: oneshot::Sender<Result<SaveSlot, SessionError>>,
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
        reply: oneshot::Sender<Result<(u64, engine::NpcDecisionDiagnostics), SendCommandError>>,
    },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed {
        speed: f64,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    SetRunning {
        running: bool,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    SetPausePreferences {
        generation: u64,
        preferences: PausePreferences,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 永久停止 actor。会话必须先从 manager 移除，避免新请求继续取得句柄。
    Shutdown { reply: oneshot::Sender<()> },
    /// B3 远程登录：认证主体（subject）成员请求——鉴权后的成员/控制面
    /// （加入市场、解析成员、市场上下文、重置/停止、控制命令 Subject 化）。
    Member {
        subject: OpaqueSubjectId,
        request: MemberRequest,
        reply: oneshot::Sender<Result<MemberResponse, SendCommandError>>,
    },
    /// B3 远程登录：归档槽元数据变更（重命名/复制——控制权）。
    ArchiveMetadata {
        subject: OpaqueSubjectId,
        generation: u64,
        slot_id: String,
        name: String,
        copy: bool,
        reply: oneshot::Sender<Result<Option<native_store::ArchiveMetadata>, SendCommandError>>,
    },
    /// B3 远程登录：删除归档槽（活动槽删除后切换到新槽——控制权）。
    DeleteArchive {
        subject: OpaqueSubjectId,
        generation: u64,
        slot_id: String,
        reply: oneshot::Sender<Result<(), SendCommandError>>,
    },
    /// B3 远程登录：选择启动归档槽（控制权）。
    SelectArchive {
        subject: OpaqueSubjectId,
        generation: u64,
        slot_id: String,
        reply: oneshot::Sender<Result<bool, SendCommandError>>,
    },
}

/// 共享市场成员请求（B3 子集——B2 查询面与公司契约面不在本批）。
#[derive(Debug)]
pub enum MemberRequest {
    /// 重置市场（控制权 + 新 setup/seed；交易时钟配置必须与现会话一致）。
    Reset {
        generation: u64,
        setup: Box<SessionSetup>,
        seed: u64,
    },
    /// 本人公开基线（成员账户过滤后的 Snapshot）。
    Baseline,
    /// 市场上下文（成员关系/控制权/世代/setup/seed）。
    Context,
    /// 停止共享市场（控制权 + 关闭 ingress 与命令通道）。
    Shutdown { generation: u64 },
    /// 新局创建者绑定（仅 fresh 初始态一次）。
    BindCreator,
    /// 加入市场（确认重加入后按当前配置发一次入场资金）。
    Join {
        generation: Option<u64>,
        confirmed_rejoin: bool,
    },
    /// 解析成员关系（非成员=需确认重加入）。
    Resolve,
    /// 市场控制权校验。
    Control,
    /// 本人快照。
    Snapshot,
    /// 本人在途委托。
    WorkingOrders { generation: u64 },
    /// 本人股票历史。
    History { generation: u64, code: engine::StockId },
    /// 调整入场资金（控制权）。
    AdmissionCash { generation: u64, cash: engine::Money },
    /// 改速（控制权 Subject 化——取代裸 SetSpeed 的远程形态）。
    SetSpeed { generation: u64, speed: f64 },
    /// 启停（控制权 Subject 化）。
    SetRunning { generation: u64, running: bool },
    /// 暂停偏好（控制权 Subject 化）。
    SetPausePreferences {
        generation: u64,
        preferences: PausePreferences,
    },
}

#[derive(Debug)]
pub enum MemberResponse {
    Baseline(PublicBaseline),
    Context(MarketContext),
    Membership(MarketMembership),
    Control,
    Snapshot(Snapshot),
    Orders(u64, serde_json::Value),
    History(u64, engine::session::HistoricalStockData),
}

/// 市场上下文（远程客户端接入/重连的首要应答体）。
#[derive(Debug, Clone, Serialize)]
pub struct MarketContext {
    pub generation: String,
    pub member: Option<MarketMembership>,
    pub can_control: bool,
    pub needs_rejoin: bool,
    pub setup: SessionSetup,
    pub seed: String,
}

#[cfg(test)]
mod fatal_tests;

#[cfg(test)]
mod interval_tests {
    use super::{
        fixed_tick_duration, gate_deferred_drop_report, SpeedMeter, FASTEST_BATCH_BUDGET,
    };
    use std::time::Duration;

    #[test]
    fn deferred_drop_report_gates_first_diagnostic_only() {
        use engine::session::DeferredDropDegradation;
        let mut reported = false;
        // 无退化 → 不报告、不置位。
        assert_eq!(gate_deferred_drop_report(&mut reported, None), None);
        assert!(!reported);
        // 首个退化（真实 errno 透传）→ 报告并置位。
        assert_eq!(
            gate_deferred_drop_report(
                &mut reported,
                Some(DeferredDropDegradation::SpawnFailed { os_errno: Some(11) })
            ),
            Some(("DEFERRED_DROP_SPAWN_FAILED", Some(11)))
        );
        assert!(reported);
        // 已报告后不再刷屏，即使退化升级。
        assert_eq!(
            gate_deferred_drop_report(
                &mut reported,
                Some(DeferredDropDegradation::UnexpectedDisconnected)
            ),
            None
        );
        // 新 actor（未报告）首见断连 → 对应诊断码、无 errno。
        let mut fresh = false;
        assert_eq!(
            gate_deferred_drop_report(
                &mut fresh,
                Some(DeferredDropDegradation::UnexpectedDisconnected)
            ),
            Some(("DEFERRED_DROP_UNEXPECTED_DISCONNECTED", None))
        );
        // SpawnFailed 无 errno（非 OS 来源失败）也能报告。
        let mut no_errno = false;
        assert_eq!(
            gate_deferred_drop_report(
                &mut no_errno,
                Some(DeferredDropDegradation::SpawnFailed { os_errno: None })
            ),
            Some(("DEFERRED_DROP_SPAWN_FAILED", None))
        );
    }

    #[test]
    fn high_speed_interval_preserves_fractional_milliseconds() {
        assert_eq!(
            fixed_tick_duration(1_000, 720.0),
            Duration::from_secs_f64(1.0 / 720.0)
        );
        assert_eq!(fixed_tick_duration(0, 1.0), Duration::from_millis(1));
    }

    #[test]
    fn fastest_batch_uses_the_cross_host_fourteen_millisecond_response_budget() {
        assert_eq!(FASTEST_BATCH_BUDGET, Duration::from_millis(14));
    }

    #[test]
    fn pacing_keeps_speed_mode_interval_and_pause_metrics_consistent() {
        let mut pacing = super::ServerPacing::new(1_000, 10);
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

        let mut minimal = super::ServerPacing::new(1, 0);
        minimal.apply_speed(super::MAX_SPEED_MULTIPLIER, 0);
        assert_eq!(minimal.tick_interval(), Duration::from_millis(1));
    }

    #[test]
    fn pacing_restore_resets_sampling_but_fatal_stop_preserves_it() {
        let mut pacing = super::ServerPacing::new(1_000, 10);
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
/// 克隆廉价（`mpsc::UnboundedSender` / `broadcast::Sender` 均可 clone）。`SessionManager` 持有
/// `Arc<SessionHandles>`，路由层与 WS 连接经 manager 取一份克隆与 actor 通信。
/// 原始命令发送端不可由外部访问，命令必须通过受控方法提交。
/// ```compile_fail,E0616
/// fn bypass(handles: &server::SessionHandles) {
///     let _ = handles.cmd_tx.clone();
/// }
/// ```
/// 外部订阅者不获得发布端。
/// ```compile_fail,E0616
/// fn publish(handles: &server::SessionHandles) {
///     let _ = handles.event_tx.clone();
/// }
/// ```
#[derive(Clone)]
pub struct SessionHandles {
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    ingress: Arc<RwLock<(u64, engine::SharedSessionIngress)>>,
    /// B3 远程登录：subject → 玩家账户的成员名册（enqueue_for 路由源）。
    members: Arc<RwLock<std::collections::BTreeMap<OpaqueSubjectId, AccountId>>>,
    event_tx: broadcast::Sender<EngineUpdate>,
    pub ticks_per_day: u64,
    pub auction_ticks: u64,
    pub closing_auction_ticks: u64,
    pub session_token: String,
    /// B3 远程登录：启动形态（新局/恢复）与原始 setup/seed——市场列表与
    /// 上下文应答（MarketContext/resumed）消费。
    pub startup_setup: SessionSetup,
    pub startup_seed: u64,
    pub startup_resumed: bool,
}

impl SessionHandles {
    /// 订阅 actor 发布的更新；外部只持有 Receiver，不获得发布写权。
    /// ```no_run
    /// fn subscribe(handles: &server::SessionHandles) {
    ///     let _: tokio::sync::broadcast::Receiver<server::actor::EngineUpdate> =
    ///         handles.subscribe_events();
    /// }
    /// ```
    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineUpdate> {
        self.event_tx.subscribe()
    }

    /// 便捷：入队玩家意图（固定 `AccountId(0)`），立即登记到共享 ingress。
    ///
    /// 失败两种：actor 已退出（通道关闭）→ `SendCommandError`；engine 拒绝 → `SessionError`。
    /// 两者都显式上抛，不静默。
    pub async fn enqueue(&self, generation: u64, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(generation, AccountId(0), intent).await
    }

    /// 入队指定玩家的意图（联机多账户预留接口，当前由 player 0 调用）。
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
                "STALE_SESSION_GENERATION: fault[1290] requested {generation}; current is {}",
                ingress.0
            )));
        }
        ingress
            .1
            .enqueue_player_intent(player_id, intent)
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// B3 远程登录：按认证主体入队意图——subject 先经成员名册解析账户
    /// （非成员显式拒绝，不回退账户 0）。
    pub async fn enqueue_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        intent: Intent,
    ) -> Result<(), SendCommandError> {
        if self.cmd_tx.is_closed() {
            return Err(SendCommandError::ActorGone);
        }
        let (ingress, members) = match (self.ingress.read(), self.members.read()) {
            (Ok(ingress), Ok(members)) => (ingress, members),
            _ => {
                return Err(SendCommandError::Rejected(
                    "session ingress lifecycle lock poisoned".into(),
                ))
            }
        };
        if generation != ingress.0 {
            return Err(SendCommandError::Rejected(format!(
                "STALE_SESSION_GENERATION: fault[1290] requested {generation}; current is {}",
                ingress.0
            )));
        }
        let account = members.get(&subject).copied().ok_or_else(|| {
            SendCommandError::Rejected(
                "当前市场没有该主体的成员关系，请确认重新加入".into(),
            )
        })?;
        ingress
            .1
            .enqueue_player_intent(account, intent)
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// B3 远程登录：成员请求统一入口（鉴权后由路由层调用）。
    pub async fn member_request(
        &self,
        subject: OpaqueSubjectId,
        request: MemberRequest,
    ) -> Result<MemberResponse, SendCommandError> {
        let (reply, receive) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Member {
                subject,
                request,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        receive.await.map_err(|_| SendCommandError::ActorGone)?
    }

    /// 市场上下文（成员关系/控制权/世代）。
    pub async fn market_context_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<MarketContext, SendCommandError> {
        match self.member_request(subject, MemberRequest::Context).await? {
            MemberResponse::Context(context) => Ok(context),
            _ => Err(SendCommandError::Rejected("市场上下文响应类型错误".into())),
        }
    }

    /// 本人公开基线（成员账户过滤后的 Snapshot）。
    pub async fn public_baseline_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<PublicBaseline, SendCommandError> {
        match self.member_request(subject, MemberRequest::Baseline).await? {
            MemberResponse::Baseline(baseline) => Ok(baseline),
            _ => Err(SendCommandError::Rejected("本人基线响应类型错误".into())),
        }
    }

    /// 解析成员关系（非成员报需确认重加入）。
    pub async fn resolve_member(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<MarketMembership, SendCommandError> {
        match self.member_request(subject, MemberRequest::Resolve).await? {
            MemberResponse::Membership(member) => Ok(member),
            _ => Err(SendCommandError::Rejected("市场成员响应类型错误".into())),
        }
    }

    /// 控制权校验（重置/停止/改速等控制面路由共用）。
    pub async fn require_control(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<(), SendCommandError> {
        match self.member_request(subject, MemberRequest::Control).await? {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("市场控制响应类型错误".into())),
        }
    }

    /// 加入市场。
    pub async fn join_member(
        &self,
        subject: OpaqueSubjectId,
        confirmed_rejoin: bool,
    ) -> Result<MarketMembership, SendCommandError> {
        match self
            .member_request(
                subject,
                MemberRequest::Join {
                    generation: None,
                    confirmed_rejoin,
                },
            )
            .await?
        {
            MemberResponse::Membership(member) => Ok(member),
            _ => Err(SendCommandError::Rejected("市场加入响应类型错误".into())),
        }
    }

    /// 带世代校验的加入市场。
    pub async fn join_member_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        confirmed_rejoin: bool,
    ) -> Result<MarketMembership, SendCommandError> {
        match self
            .member_request(
                subject,
                MemberRequest::Join {
                    generation: Some(generation),
                    confirmed_rejoin,
                },
            )
            .await?
        {
            MemberResponse::Membership(member) => Ok(member),
            _ => Err(SendCommandError::Rejected("市场加入响应类型错误".into())),
        }
    }

    /// 重置市场（控制权 + 新 setup/seed；成功返回新世代快照）。
    pub async fn reset_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        setup: SessionSetup,
        seed: u64,
    ) -> Result<Snapshot, SendCommandError> {
        if setup.ticks_per_day != self.ticks_per_day
            || setup.auction_ticks != self.auction_ticks
            || setup.closing_auction_ticks != self.closing_auction_ticks
        {
            return Err(SendCommandError::Rejected(
                "重置市场的交易时钟配置必须与当前共享会话一致".into(),
            ));
        }
        match self
            .member_request(
                subject,
                MemberRequest::Reset {
                    generation,
                    setup: Box::new(setup),
                    seed,
                },
            )
            .await?
        {
            MemberResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(SendCommandError::Rejected("重置市场响应类型错误".into())),
        }
    }

    /// 停止共享市场（控制权）。
    pub async fn shutdown_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
    ) -> Result<(), SendCommandError> {
        self.member_request(subject, MemberRequest::Shutdown { generation })
            .await
            .map(|_| ())
    }

    /// 调整入场资金（控制权）。
    pub async fn set_admission_cash_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        cash: engine::Money,
    ) -> Result<(), SendCommandError> {
        match self
            .member_request(
                subject,
                MemberRequest::AdmissionCash { generation, cash },
            )
            .await?
        {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("入场资金响应类型错误".into())),
        }
    }

    /// 改速（控制权 Subject 化）。
    pub async fn set_speed_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        speed: f64,
    ) -> Result<(), SendCommandError> {
        if !(speed.is_finite() && speed > 0.0 && speed <= MAX_SPEED_MULTIPLIER
            || speed == f64::INFINITY)
        {
            return Err(SendCommandError::Rejected("市场倍速无效".into()));
        }
        match self
            .member_request(subject, MemberRequest::SetSpeed { generation, speed })
            .await?
        {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("市场倍速响应类型错误".into())),
        }
    }

    /// 启停（控制权 Subject 化）。
    pub async fn set_running_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        running: bool,
    ) -> Result<(), SendCommandError> {
        match self
            .member_request(
                subject,
                MemberRequest::SetRunning { generation, running },
            )
            .await?
        {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("市场启停响应类型错误".into())),
        }
    }

    /// 暂停偏好（控制权 Subject 化）。
    pub async fn set_pause_preferences_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        preferences: PausePreferences,
    ) -> Result<(), SendCommandError> {
        match self
            .member_request(
                subject,
                MemberRequest::SetPausePreferences {
                    generation,
                    preferences,
                },
            )
            .await?
        {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("暂停偏好响应类型错误".into())),
        }
    }

    /// 本人快照（成员账户过滤）。
    pub async fn snapshot_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<Snapshot, SendCommandError> {
        match self.member_request(subject, MemberRequest::Snapshot).await? {
            MemberResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(SendCommandError::Rejected("成员快照响应类型错误".into())),
        }
    }

    /// 本人股票历史。
    pub async fn history_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        code: engine::StockId,
    ) -> Result<(u64, engine::session::HistoricalStockData), SendCommandError> {
        match self
            .member_request(subject, MemberRequest::History { generation, code })
            .await?
        {
            MemberResponse::History(generation, data) => Ok((generation, data)),
            _ => Err(SendCommandError::Rejected("成员历史响应类型错误".into())),
        }
    }

    /// 归档槽元数据变更（重命名/复制——控制权）。
    pub async fn mutate_archive_metadata(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot_id: String,
        name: String,
        copy: bool,
    ) -> Result<Option<native_store::ArchiveMetadata>, SendCommandError> {
        let (reply, receive) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ArchiveMetadata {
                subject,
                generation,
                slot_id,
                name,
                copy,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        receive.await.map_err(|_| SendCommandError::ActorGone)?
    }

    /// 删除归档槽（活动槽删除后 writer 切换到新槽——控制权）。
    pub async fn delete_archive(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot_id: String,
    ) -> Result<(), SendCommandError> {
        let (reply, receive) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::DeleteArchive {
                subject,
                generation,
                slot_id,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        receive.await.map_err(|_| SendCommandError::ActorGone)?
    }

    /// 选择启动归档槽（控制权）。
    pub async fn select_archive(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot_id: String,
    ) -> Result<bool, SendCommandError> {
        let (reply, receive) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SelectArchive {
                subject,
                generation,
                slot_id,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        receive.await.map_err(|_| SendCommandError::ActorGone)?
    }

    /// 取完整快照。actor 关闭时返回 `ActorGone`（绝不静默返回空快照）。
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
    ) -> Result<(u64, serde_json::Value), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PlayerWorkingOrders {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub async fn query_stock_history(
        &self,
        generation: u64,
        code: engine::StockId,
    ) -> Result<(u64, engine::session::HistoricalStockData), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::StockHistory {
                generation,
                code,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub async fn initial_allocation(
        &self,
        generation: u64,
    ) -> Result<(u64, engine::session::InitialAllocation), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::InitialAllocation {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub async fn public_baseline(&self) -> Result<PublicBaseline, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicBaseline { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
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
    pub async fn step(&self, generation: u64) -> Result<ProtocolUpdate, SendCommandError> {
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

    pub async fn public_report_page(
        &self,
        query: engine::company::PublicReportQuery,
    ) -> Result<engine::company::PublicReportPage, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReportPage { query, reply: tx })
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
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
    }

    /// 改变倍速，按命令投递顺序处理并等待 actor 确认；关闭时返回 `ActorGone`。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        if speed != f64::INFINITY
            && (!speed.is_finite() || speed <= 0.0 || speed > MAX_SPEED_MULTIPLIER)
        {
            return Err(SendCommandError::InvalidSpeed(speed));
        }
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed, reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn shutdown(&self) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Shutdown { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn set_running(&self, running: bool) -> Result<(), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetRunning { running, reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
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
    /// B3 远程登录：server 域错误（锁损坏/启动期限制/归档激活失败等）——
    /// server 是引擎字符串边界，文本在此层显式携带（引擎域保持 fault 码）。
    #[error("{0}")]
    Text(String),
}

/// Session 注册表：`DashMap<session_id, Arc<SessionHandles>>`。
///
/// `DashMap` 内部分片锁、读多写少场景高效；actor 状态本身不存于此（actor task own），
/// 这里只存消息端点——无共享可变游戏状态。
#[derive(Clone)]
pub struct SessionManager {
    /// B3 远程登录：原生存储（身份 credential/归档槽）。默认构造开内存
    /// 库（测试/无持久化部署）；生产经 `with_database` 绑定文件库。
    database: native_store::NativeDatabase,
    /// 共享市场创建/恢复互斥（单市场全局语义的并发防线）。
    shared_market_gate: Arc<std::sync::Mutex<()>>,
    sessions: Arc<DashMap<String, Arc<SessionHandles>>>,
    active_count: Arc<AtomicUsize>,
    base_ms: u64,
}

impl SessionManager {
    /// 默认基准（`BASE_TICK_MS`）。
    pub fn default_base() -> Self {
        Self::with_base_ms(BASE_TICK_MS)
    }

    /// 绑定持久化原生存储（生产入口——文件 SQLite 独占 writer）。
    pub fn with_database(database: native_store::NativeDatabase) -> Self {
        Self::with_base_ms_and_database(BASE_TICK_MS, database)
    }

    /// 身份服务的存储入口（identity_routes 装配用）。
    pub fn database(&self) -> native_store::NativeDatabase {
        self.database.clone()
    }

    /// 批 9 A1（std-thread 迁移收官）：compute 注入退役——线程模型
    /// 由引擎内部决定（一股一持久线程），CPU 限制走 OS 级
    /// cgroup/taskset（memory/project_std_thread_migration）。
    pub fn with_base_ms(base_ms: u64) -> Self {
        Self::with_base_ms_and_database(
            base_ms,
            native_store::NativeDatabase::open_in_memory()
                .expect("测试 SQLite 初始化失败"),
        )
    }

    fn with_base_ms_and_database(
        base_ms: u64,
        database: native_store::NativeDatabase,
    ) -> Self {
        Self {
            database,
            shared_market_gate: Arc::new(std::sync::Mutex::new(())),
            sessions: Arc::new(DashMap::new()),
            active_count: Arc::new(AtomicUsize::new(0)),
            base_ms,
        }
    }

    /// 创建新 session：构造 `GameSession` → 建 mpsc+broadcast → spawn actor task → 注册。
    ///
    /// 失败显式返回 `SessionError`（构造非法参数），绝不静默吞（铁律二）。
    pub fn new_session(&self, setup: SessionSetup, seed: u64) -> Result<String, NewSessionError> {
        self.new_session_bound(setup, seed, None)
    }

    /// B3 远程登录：创建共享市场（单市场全局；已有市场返回 (id, false)
    /// 不另开）。creator 绑定市场创建者并授予控制权。
    pub fn new_shared_session(
        &self,
        setup: SessionSetup,
        seed: u64,
        creator: OpaqueSubjectId,
    ) -> Result<(String, bool), NewSessionError> {
        let _guard = self
            .shared_market_gate
            .lock()
            .map_err(|error| NewSessionError::Text(format!("共享市场创建锁损坏：{error}")))?;
        if let Some(entry) = self.sessions.iter().next() {
            return Ok((entry.key().clone(), false));
        }
        self.new_session_bound(setup, seed, Some(creator))
            .map(|id| (id, true))
    }

    /// 当前全部市场 id。
    pub fn market_ids(&self) -> Vec<String> {
        self.sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn new_session_bound(
        &self,
        setup: SessionSetup,
        seed: u64,
        creator: Option<OpaqueSubjectId>,
    ) -> Result<String, NewSessionError> {
        self.active_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < MAX_SESSIONS).then_some(current + 1)
            })
            .map_err(|_| NewSessionError::Capacity { max: MAX_SESSIONS })?;
        let mut game = match ProtocolSession::new(setup.clone(), seed) {
            Ok(game) => game,
            Err(error) => {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(error.into());
            }
        };
        if let Some(creator) = &creator {
            if let Err(error) = game.bind_market_creator(creator.clone()) {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(NewSessionError::Text(error.to_string()));
            }
        }
        self.register_game(game, setup, seed, "current", false, creator)
    }

    /// B3 远程登录：从日终档恢复共享市场（只允许启动期无内存市场时）。
    pub fn resume_session(&self, slot_id: &str) -> Result<Option<String>, NewSessionError> {
        let _guard = self
            .shared_market_gate
            .lock()
            .map_err(|error| NewSessionError::Text(format!("共享市场创建锁损坏：{error}")))?;
        if self.active_session_count() != 0 {
            return Err(NewSessionError::Text(
                "只允许在启动且没有内存市场时恢复日终档".into(),
            ));
        }
        let slot = self
            .database
            .load(slot_id)
            .map_err(|error| NewSessionError::Text(error.to_string()))?;
        let Some(slot) = slot else {
            return Ok(None);
        };
        let setup = slot.setup.clone();
        let seed = slot.seed;
        let game = ProtocolSession::restore(&slot)?;
        self.active_count.fetch_add(1, Ordering::AcqRel);
        self.register_game(game, setup, seed, slot_id, true, None)
            .map(Some)
    }

    /// B3 远程登录：按启动槽选择恢复（无选择=不恢复；已清空选择=显式报错
    /// 不自动回退其他档）。
    pub fn resume_selected_session(&self) -> Result<Option<String>, NewSessionError> {
        let selection = self
            .database
            .selection()
            .map_err(|error| NewSessionError::Text(error.to_string()))?;
        match selection {
            native_store::ArchiveSelection::Selected(slot_id) => self.resume_session(&slot_id),
            native_store::ArchiveSelection::Uninitialized => Ok(None),
            native_store::ArchiveSelection::Cleared => Err(NewSessionError::Text(
                "已选启动槽已删除；请明确选择其他日终档或创建新局，不自动回退其他档".into(),
            )),
        }
    }

    /// 装配 actor 与句柄并注册（新局/恢复共用；creator 优先入控制权集，
    /// 数据库既有授权主体并入）。
    fn register_game(
        &self,
        game: ProtocolSession,
        setup: SessionSetup,
        seed: u64,
        slot_id: &str,
        resumed: bool,
        creator: Option<OpaqueSubjectId>,
    ) -> Result<String, NewSessionError> {
        let archive = match native_store::ArchiveWriter::activate(self.database.clone(), slot_id) {
            Ok(archive) => Some(archive),
            Err(error) => {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(NewSessionError::Text(error.to_string()));
            }
        };
        let mut controllers: std::collections::BTreeSet<OpaqueSubjectId> =
            self.database
                .market_controllers("shared-market")
                .map_err(|error| {
                    self.active_count.fetch_sub(1, Ordering::AcqRel);
                    NewSessionError::Text(error.to_string())
                })?
                .into_iter()
                .filter_map(|subject| OpaqueSubjectId::new(subject).ok())
                .collect();
        if let Some(creator) = &creator {
            self.database
                .grant_market_control("shared-market", creator.as_str())
                .map_err(|error| {
                    self.active_count.fetch_sub(1, Ordering::AcqRel);
                    NewSessionError::Text(error.to_string())
                })?;
            controllers.insert(creator.clone());
        }
        let ticks_per_day = setup.ticks_per_day;
        let auction_ticks = setup.auction_ticks;
        let closing_auction_ticks = setup.closing_auction_ticks;
        let session_id = uuid::Uuid::new_v4().to_string();

        // 命令按提交顺序入队；不以固定条数暂停投递，调用方仍等待逐条处理回执。
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, _event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let ingress = Arc::new(RwLock::new((1, game.shared_ingress())));
        let members = Arc::new(RwLock::new(game.trading_subject_accounts()));

        let handles = Arc::new(SessionHandles {
            cmd_tx,
            ingress: Arc::clone(&ingress),
            members: Arc::clone(&members),
            event_tx,
            ticks_per_day,
            auction_ticks,
            closing_auction_ticks,
            session_token: uuid::Uuid::new_v4().to_string(),
            startup_setup: setup,
            startup_seed: seed,
            startup_resumed: resumed,
        });
        self.sessions.insert(session_id.clone(), handles.clone());

        let actor = SessionActor {
            ingress,
            members,
            controllers,
            archive,
            #[cfg(test)]
            injected_step_failure: None,
            pacing: ServerPacing::new(self.base_ms, game.tick()),
            game,
            cmd_rx,
            event_tx: handles.event_tx.clone(),
            session_id: session_id.clone(),
            public_revision: 0,
            timeline_generation: 1,
            pause_preferences: PausePreferences::default(),
            fatal_failure: None,
            deferred_drop_reported: false,
        };
        // P2①：producer 名额必须在 spawn 前同步登记。guard 作为捕获随 future 移动，
        // 未 poll/被取消/panic 的丢弃都会析构捕获并释放名额；正常路径在 run
        // （含 actor 与其 game 的 Drop）完成后释放。修复前登记发生在首次 poll，
        // `new_session` 返回但 future 未 poll 时 actor_producers 仍为 0，drain 可在
        // 空账本上伪成功。普通构建 guard 为空操作。
        let producer = engine::session::verification_actor_guard();
        tokio::spawn(async move {
            let _producer = producer;
            actor.run().await;
        });
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
/// 进程级析构后端退化 → 宿主边界稳定诊断码与 errno。
/// `reported` 已置位后不再重复报告（首个故障可见，不逐 tick 刷屏）。
fn gate_deferred_drop_report(
    reported: &mut bool,
    degradation: Option<engine::session::DeferredDropDegradation>,
) -> Option<(&'static str, Option<i32>)> {
    if *reported {
        return None;
    }
    let degradation = degradation?;
    *reported = true;
    Some(match degradation {
        engine::session::DeferredDropDegradation::SpawnFailed { os_errno } => {
            ("DEFERRED_DROP_SPAWN_FAILED", os_errno)
        }
        engine::session::DeferredDropDegradation::UnexpectedDisconnected => {
            ("DEFERRED_DROP_UNEXPECTED_DISCONNECTED", None)
        }
    })
}

struct SessionActor {
    ingress: Arc<RwLock<(u64, engine::SharedSessionIngress)>>,
    /// B3 远程登录：subject → 玩家账户的成员名册（ingress 写锁内同步维护
    /// ——enqueue 路由与 join/bind 一致）。
    members: Arc<RwLock<std::collections::BTreeMap<OpaqueSubjectId, AccountId>>>,
    /// B3 远程登录：市场控制权主体集（创建者+授权主体；重置/停止/改速等
    /// 控制命令的门禁）。
    controllers: std::collections::BTreeSet<OpaqueSubjectId>,
    /// B3 远程登录：日终归档 writer（None=无归档面——纯内存会话或 writer
    /// 失效后的降级；日终档不再写）。
    archive: Option<native_store::ArchiveWriter>,
    #[cfg(test)]
    injected_step_failure: Option<(usize, engine::session::StepFatal)>,
    pacing: ServerPacing,
    game: ProtocolSession,
    cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    event_tx: broadcast::Sender<EngineUpdate>,
    session_id: String,
    public_revision: u64,
    timeline_generation: u64,
    pause_preferences: PausePreferences,
    fatal_failure: Option<HostFailure>,
    /// 进程级析构后端退化只在本 actor 首次观察时报告（不逐 tick 刷屏）。
    deferred_drop_reported: bool,
}

impl SessionActor {
    fn fatal_rejection(&self) -> Option<SessionError> {
        self.fatal_failure.as_ref().map(|failure| {
            // 1291：宿主致命失败后停机；宿主失败 code/message 在 HostFailure 本体（边界序列化）。
            SessionError::InvalidSave(engine::error::ValidationFault::bare(1291))
        })
    }

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
        info!(session = %self.session_id, interval = ?self.pacing.tick_interval(), "session actor started");

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
                            self.report_deferred_drop_degradation();
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
                _ = interval.tick(), if !self.pacing.is_fastest() && self.pacing.is_running() => {
                    self.tick_and_broadcast().await;
                }
                _ = std::future::ready(()), if self.pacing.is_fastest() && self.pacing.is_running() => {
                    self.run_fastest_batch();
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
    /// 单个不可分割的 pipeline tick 可能耗尽整个 14ms 时间片，因此这里不承诺每轮
    /// 至少完成多个 tick；与 desktop/Worker 一致，强合同是按预算 yield 并响应控制命令。
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
                self.stop_with_failure(HostFailure::step(&error));
                return;
            }
        };
        let revision_before = self.public_revision;
        let started = std::time::Instant::now();
        let mut frames = Vec::new();
        for _ in 0..limit {
            // A request delivered during tick n must be enqueued before tick n+1.
            // Return to the biased command branch at the first tick boundary.
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
                Ok(batch) => Some(ProtocolUpdate::TickBatch(Box::new(batch))),
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
            // B3 远程登录：日终档随广播前持久化（写失败=可恢复失败——上一份
            // 有效档保留，交易提交有效；18725d07 persist_day_end 语义）。
            self.persist_day_end(&update);
            let pause = self.pause_preferences.pauses(&update);
            self.public_revision = self.public_revision.saturating_add(1);
            let date = update.civil_date.clone();
            self.broadcast_at(ProtocolUpdate::CivilUpdate(Box::new(update)), date);
            if pause {
                self.pacing.pause_at_civil_boundary(self.game.tick());
            }
        }
        self.pacing.refresh_metrics(self.game.tick());
        // 成功路径：批级回滚锚点已无用，析构移交池外专用线程——旧账户页的
        // jemalloc free（实测 ~132µs/tick）不再占用 actor 关键路径；析构无任何
        // 可观察副作用，失败原子性/首错序不受影响（错误路径仍走 rollback_cycle）。
        engine::session::defer_destruction(checkpoint);
        self.report_deferred_drop_degradation();
    }

    /// 析构后端退化是性能告警，不是交易失败：不 stop pacing、不关闭 ingress、
    /// 不触碰 HostUpdate/HostFailure schema。只在宿主边界生成可读字符串。
    fn report_deferred_drop_degradation(&mut self) {
        if let Some((code, os_errno)) = gate_deferred_drop_report(
            &mut self.deferred_drop_reported,
            engine::session::deferred_destruction_degradation(),
        ) {
            warn!(
                session = %self.session_id,
                tick = self.game.tick(),
                code,
                os_errno,
                "后台析构后端已退化：析构改为同步释放，交易提交仍有效；请反馈该诊断码与 errno"
            );
        }
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

    /// 日终档持久化（B3 远程登录）：写失败为可恢复失败——交易提交有效、
    /// 上一份有效日终档保留，仅广播 HostFailure::archive 提示人工反馈。
    fn persist_day_end(&mut self, update: &engine::session::protocol::CivilUpdate) {
        let Some(writer) = &self.archive else {
            return;
        };
        let key = engine::session::protocol::SaveCandidateKey {
            seq: update.seq_to,
            settled_date: update.boundary.settled_date,
        };
        let saved = native_store::DayEndCandidate::capture(&self.game, &key)
            .map_err(|error| error.to_string())
            .and_then(|slot| writer.save_day_end(&slot).map_err(|error| error.to_string()));
        if let Err(error) = saved {
            self.stop_with_failure(HostFailure::archive(&error));
        }
    }

    /// 控制权门禁：subject 必须在 controllers 集内。
    fn require_market_control(
        &self,
        subject: &OpaqueSubjectId,
    ) -> Result<(), SendCommandError> {
        if self.controllers.contains(subject) {
            Ok(())
        } else {
            Err(SendCommandError::Rejected(
                "该主体没有当前市场控制能力".into(),
            ))
        }
    }

    /// 成员请求处理（B3 子集——鉴权后的成员/控制面统一入口）。
    fn handle_member_request(
        &mut self,
        subject: OpaqueSubjectId,
        request: MemberRequest,
    ) -> Result<MemberResponse, SendCommandError> {
        let reject =
            |error: engine::session::MembershipError| SendCommandError::Rejected(error.to_string());
        let request_generation = match &request {
            MemberRequest::AdmissionCash { generation, .. }
            | MemberRequest::SetSpeed { generation, .. }
            | MemberRequest::SetRunning { generation, .. }
            | MemberRequest::SetPausePreferences { generation, .. } => Some(*generation),
            _ => None,
        };
        if request_generation.is_some_and(|generation| generation != self.timeline_generation) {
            return Err(SendCommandError::Rejected("STALE_SESSION_GENERATION".into()));
        }
        match request {
            MemberRequest::Reset {
                generation,
                setup,
                seed,
            } => {
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ));
                }
                self.require_market_control(&subject)?;
                if setup.ticks_per_day != self.game.current_setup().ticks_per_day
                    || setup.auction_ticks != self.game.current_setup().auction_ticks
                    || setup.closing_auction_ticks != self.game.current_setup().closing_auction_ticks
                {
                    return Err(SendCommandError::Rejected(
                        "重置市场的交易时钟配置必须与当前共享会话一致".into(),
                    ));
                }
                let mut replacement = ProtocolSession::new(*setup, seed)
                    .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                replacement
                    .bind_market_creator(subject.clone())
                    .map_err(reject)?;
                // 独立复核阻断 1 修复：Reset 是时间线替换（与 Restore 同型）——
                // 必须同步替换 ingress（新代次+新会话的收件入口）与成员名册，
                /// 并 renew 归档 writer 到新时间线（18725d07 replace_timeline 语义；
                // 此前漏换 ingress 使 reset 后新代次下单全部 STALE、旧单打进已
                // 脱离的旧 ingress）。
                let next_generation = self
                    .timeline_generation
                    .checked_add(1)
                    .ok_or(SendCommandError::Rejected("世代计数耗尽".into()))?;
                let replacement_ingress = self
                    .ingress
                    .write()
                    .map_err(|error| {
                        SendCommandError::Rejected(format!(
                            "session ingress lifecycle lock poisoned: {error}"
                        ))
                    })
                    .and_then(|mut ingress| {
                        ingress
                            .1
                            .close()
                            .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                        *ingress = (next_generation, replacement.shared_ingress());
                        let mut members = self.members.write().map_err(|error| {
                            SendCommandError::Rejected(format!("成员名册锁损坏：{error}"))
                        })?;
                        *members = replacement.trading_subject_accounts();
                        Ok(())
                    });
                if let Err(error) = replacement_ingress {
                    return Err(error);
                }
                let renewed_archive = match &self.archive {
                    Some(writer) => writer
                        .renew(None)
                        .map(|next| Some(next))
                        .map_err(|error| SendCommandError::Rejected(error.to_string())),
                    None => Ok(None),
                };
                match renewed_archive {
                    Ok(archive) => self.archive = archive,
                    Err(error) => return Err(error),
                }
                self.game = replacement;
                self.timeline_generation = next_generation;
                self.public_revision = self.public_revision.saturating_add(1);
                self.pacing.reset_after_restore(self.game.tick());
                self.broadcast_timeline_reset();
                Ok(MemberResponse::Snapshot(self.game.snapshot()))
            }
            MemberRequest::Baseline => {
                let account = match self.game.market_membership(&subject) {
                    Ok(member) => Some(member.account_id),
                    Err(engine::session::MembershipError::RejoinConfirmationRequired) => None,
                    Err(error) => return Err(reject(error)),
                };
                self.game.prepare_public_baseline();
                let mut snapshot: PublicBaselineSnapshot = self.game.snapshot().into();
                snapshot.accounts.retain(|id, _| Some(*id) == account);
                Ok(MemberResponse::Baseline(PublicBaseline {
                    timeline_generation: self.timeline_generation,
                    snapshot,
                    civil_date: self.game.civil_date().to_iso(),
                    public_revision: self.public_revision,
                    public_report_ids: self
                        .game
                        .publication_ids()
                        .into_iter()
                        .map(|id| id.value().to_string())
                        .collect(),
                    failure: self.fatal_failure.clone(),
                }))
            }
            MemberRequest::Context => {
                let member = match self.game.market_membership(&subject) {
                    Ok(member) => Some(member.clone()),
                    Err(engine::session::MembershipError::RejoinConfirmationRequired) => None,
                    Err(error) => return Err(reject(error)),
                };
                Ok(MemberResponse::Context(MarketContext {
                    generation: self.timeline_generation.to_string(),
                    needs_rejoin: member.is_none(),
                    member,
                    can_control: self.controllers.contains(&subject),
                    setup: self.game.current_setup().clone(),
                    seed: self.game.initial_seed().to_string(),
                }))
            }
            MemberRequest::Shutdown { generation } => {
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ));
                }
                self.require_market_control(&subject)?;
                self.game
                    .shared_ingress()
                    .close()
                    .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                self.cmd_rx.close();
                Ok(MemberResponse::Control)
            }
            MemberRequest::BindCreator => {
                let member = self
                    .game
                    .bind_market_creator(subject.clone())
                    .map_err(reject)?;
                if let Ok(mut members) = self.members.write() {
                    *members = self.game.trading_subject_accounts();
                }
                self.controllers = std::collections::BTreeSet::from([subject]);
                Ok(MemberResponse::Membership(member))
            }
            MemberRequest::Join {
                generation,
                confirmed_rejoin,
            } => {
                if generation.is_some_and(|generation| generation != self.timeline_generation) {
                    return Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ));
                }
                let member = self
                    .game
                    .join_market(subject.clone(), confirmed_rejoin)
                    .map_err(reject)?;
                if let Ok(mut members) = self.members.write() {
                    members.insert(subject, member.account_id);
                }
                Ok(MemberResponse::Membership(member))
            }
            MemberRequest::AdmissionCash { cash, .. } => {
                self.require_market_control(&subject)?;
                self.game
                    .set_admission_cash(cash)
                    .map(|()| MemberResponse::Control)
                    .map_err(reject)
            }
            MemberRequest::Control => self
                .require_market_control(&subject)
                .map(|()| MemberResponse::Control),
            MemberRequest::SetSpeed { speed, .. } => {
                self.require_market_control(&subject)?;
                if let Some(error) = self.fatal_rejection() {
                    return Err(SendCommandError::Rejected(error.to_string()));
                }
                self.pacing.apply_speed(speed, self.game.tick());
                Ok(MemberResponse::Control)
            }
            MemberRequest::SetRunning { running, .. } => {
                self.require_market_control(&subject)?;
                if let Some(error) = self.fatal_rejection() {
                    return Err(SendCommandError::Rejected(error.to_string()));
                }
                self.pacing.set_running(running, self.game.tick());
                Ok(MemberResponse::Control)
            }
            MemberRequest::SetPausePreferences {
                generation,
                preferences,
            } => {
                self.require_market_control(&subject)?;
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ));
                }
                self.pause_preferences = preferences;
                Ok(MemberResponse::Control)
            }
            MemberRequest::Resolve => {
                self.game
                    .resolve_trading_account(&subject)
                    .map_err(reject)?;
                self.game
                    .market_membership(&subject)
                    .cloned()
                    .map(MemberResponse::Membership)
                    .map_err(reject)
            }
            MemberRequest::Snapshot => {
                let account = self
                    .game
                    .resolve_trading_account(&subject)
                    .map_err(reject)?;
                let mut snapshot = self.game.snapshot();
                // 18725d07 语义：本人快照只含成员自身账户（创建者视他人
                // 账户需各自鉴权——不附带账户 0）。
                snapshot.accounts.retain(|id, _| *id == account);
                Ok(MemberResponse::Snapshot(snapshot))
            }
            MemberRequest::WorkingOrders { generation } => {
                // B3 读面注记：main 的 player_working_orders 是账户 0 的单人
                // 读面（PlayerWorkingOrder 无 owner 字段、多玩家过滤形态尚无）
                // ——远程在途读面待 W3/B2 后随读面多玩家化接线；当前显式
                // 拒绝（不返回他人账户 0 的在途单冒充本人数据）。
                let _ = (generation, &subject);
                Err(SendCommandError::Rejected(
                    "WORKING_ORDERS_REMOTE_PENDING_MULTI_MEMBER_READ_FACE".into(),
                ))
            }
            MemberRequest::History { generation, code } => {
                let account = self
                    .game
                    .resolve_trading_account(&subject)
                    .map_err(reject)?;
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ));
                }
                self.game
                    .query_stock_history(account, &code)
                    .map(|data| MemberResponse::History(self.timeline_generation, data))
                    .map_err(|error| SendCommandError::Rejected(error.to_string()))
            }
        }
    }

    fn rollback_cycle(
        &mut self,
        checkpoint: engine::session::protocol::ProtocolCheckpoint,
        error: SessionError,
    ) {
        self.game.rollback(checkpoint);
        self.stop_with_failure(HostFailure::civil(&error));
    }

    fn stop_with_failure(&mut self, mut failure: HostFailure) {
        // B3：可恢复失败（如日终存档写失败）不关 ingress、不停机——交易
        // 提交有效，恢复推进后自动重试（18725d07 persist_day_end 语义）。
        if !failure.recoverable {
            if let Err(error) = self.game.shared_ingress().close() {
                failure
                    .message
                    .push_str(&format!("；关闭 ingress 失败：{error}"));
            }
        }
        failure.context.tick = Some(self.game.tick());
        failure.context.seq = Some(self.game.seq());
        failure.context.day = Some(self.game.day());
        failure.context.generation = Some(self.timeline_generation.to_string());
        self.pacing.stop_after_failure();
        if failure.recoverable {
            self.broadcast_failure(failure);
            return;
        }
        if self.fatal_failure.is_none() {
            self.broadcast_failure(failure.clone());
            self.fatal_failure = Some(failure);
        }
    }

    #[cfg(feature = "host-parity")]
    fn broadcast_update(&self, protocol: ProtocolUpdate) {
        self.broadcast_at(protocol, self.game.civil_date().to_iso());
    }

    fn broadcast_at(&self, protocol: ProtocolUpdate, civil_date: String) {
        if self.event_tx.receiver_count() == 0
            && matches!(&protocol, ProtocolUpdate::CivilUpdate(_))
        {
            self.game.discard_pending_save_candidates();
        }
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

    fn broadcast_failure(&self, failure: HostFailure) {
        let update = EngineUpdate {
            timeline_generation: self.timeline_generation,
            update: None,
            civil_date: self.game.civil_date().to_iso(),
            public_revision: self.public_revision,
            failure: Some(failure),
        };
        if self.event_tx.send(update).is_err() {
            debug!(session = %self.session_id, "no subscribers for fatal host failure");
        }
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
                let res = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else {
                    self.game.enqueue_player_intent(player_id, intent)
                };
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
            SessionCommand::PlayerWorkingOrders { generation, reply } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(format!(
                        "STALE_SESSION_GENERATION: fault[1290] requested {generation}; current is {}",
                        self.timeline_generation
                    )))
                } else {
                    serde_json::to_value(self.game.player_working_orders())
                        .map(|orders| (self.timeline_generation, orders))
                        .map_err(|error| {
                            SendCommandError::Rejected(format!(
                                "serialize player working orders: {error}"
                            ))
                        })
                };
                let _ = reply.send(result);
            }
            SessionCommand::StockHistory {
                generation,
                code,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(format!(
                        "STALE_SESSION_GENERATION: fault[1290] requested {generation}; current is {}",
                        self.timeline_generation
                    )))
                } else {
                    self.game
                        .query_stock_history(AccountId(0), &code)
                        .map(|data| (self.timeline_generation, data))
                        .map_err(|error| SendCommandError::Rejected(error.to_string()))
                };
                let _ = reply.send(result);
            }
            SessionCommand::InitialAllocation { generation, reply } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(format!(
                        "STALE_SESSION_GENERATION: fault[1290] requested {generation}; current is {}",
                        self.timeline_generation
                    )))
                } else {
                    self.game
                        .initial_allocation()
                        .map(|data| (self.timeline_generation, data))
                        .map_err(|error| SendCommandError::Rejected(error.to_string()))
                };
                let _ = reply.send(result);
            }
            SessionCommand::PublicBaseline { reply } => {
                self.game.prepare_public_baseline();
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
                    failure: self.fatal_failure.clone(),
                });
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
                let result = if generation != self.timeline_generation {
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::new(
                            1290,
                            vec![generation, self.timeline_generation],
                        ),
                    ))
                } else if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else if let Some(key) = candidate.as_ref() {
                    self.game.save_candidate(key)
                } else {
                    self.game.save()
                };
                let _ = reply.send(result);
            }
            SessionCommand::Restore { slot, reply } => match self.fatal_rejection().map_or_else(
                || ProtocolSession::restore(&slot),
                Err,
            ) {
                Ok(restored) => {
                    let next_generation = match self.timeline_generation.checked_add(1) {
                        Some(generation) => generation,
                        None => {
                            // 1292：世代计数耗尽。
                            let _ = reply.send(Err(SessionError::ResourceLimit(
                                engine::error::ValidationFault::bare(1292),
                            )));
                            return;
                        }
                    };
                    let replacement = self
                        .ingress
                        .write()
                        .map_err(|_| {
                            // 1293：ingress 生命周期锁中毒（防御式显式暴露）。
                            SessionError::ResourceLimit(engine::error::ValidationFault::bare(1293))
                        })
                        .and_then(|mut ingress| {
                            ingress.1.close()?;
                            *ingress = (next_generation, restored.shared_ingress());
                            // B3 远程登录+复核建议 5：恢复档是成员名册权威——
                            // 名册更新并入 ingress 写锁段（嵌套序恒 ingress→
                            // members，与 enqueue_for 读锁同序无死锁；此前分
                            // 两段的失败窗会留下部分替换状态）。
                            let mut members = self.members.write().map_err(|_| {
                                // 1293：成员名册锁中毒（与 ingress 同码域）。
                                SessionError::ResourceLimit(
                                    engine::error::ValidationFault::bare(1293),
                                )
                            })?;
                            *members = restored.trading_subject_accounts();
                            Ok(())
                        });
                    if let Err(error) = replacement {
                        let _ = reply.send(Err(error));
                        return;
                    }
                    self.game = restored;
                    self.timeline_generation = next_generation;
                    self.public_revision = self.public_revision.saturating_add(1);
                    self.pacing.reset_after_restore(self.game.tick());
                    self.broadcast_timeline_reset();
                    let _ = reply.send(Ok(self.game.snapshot()));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            #[cfg(feature = "host-parity")]
            SessionCommand::AdvanceCivilDay { generation, reply } => {
                let result = if let Some(failure) = &self.fatal_failure {
                    // 1291：宿主致命失败后停机（同 fatal_rejection）。
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::bare(1291),
                    ))
                } else if generation == self.timeline_generation {
                    self.game.end_civil_day_update()
                } else {
                    // 1290：世代过期；ctx[0]=请求世代，ctx[1]=当前世代。
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::new(
                            1290,
                            vec![generation, self.timeline_generation],
                        ),
                    ))
                };
                if let Ok(report) = &result {
                    // B3 远程登录：日终档随广播前持久化（与 tick 循环同点——
                    // 写失败为可恢复失败，上一份有效档保留）。
                    self.persist_day_end(report);
                    self.public_revision = self.public_revision.saturating_add(1);
                    self.broadcast_update(ProtocolUpdate::CivilUpdate(Box::new(report.clone())));
                    if self.pause_preferences.pauses(report) {
                        self.pacing.pause_at_civil_boundary(self.game.tick());
                    }
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.stop_with_failure(HostFailure::step(fatal));
                }
                let _ = reply.send(result);
            }
            #[cfg(feature = "host-parity")]
            SessionCommand::Step { generation, reply } => {
                let result = if let Some(failure) = &self.fatal_failure {
                    // 1291：宿主致命失败后停机（同 fatal_rejection）。
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::bare(1291),
                    ))
                } else if generation == self.timeline_generation {
                    self.game.civil_day_ready().and_then(|ready| {
                        if ready {
                            // 1294：日结静点未发布前不允许 host-parity 单步
                            //（main 既有缺陷修复：原 `.into()` 到
                            // ValidationFault 编译失败——fault 码化时漏改，
                            // host-parity feature 自此才可编译；码取 129x 段
                            // 下一个空闲位）。
                            return Err(SessionError::InvalidSave(
                                engine::error::ValidationFault::bare(1294),
                            ));
                        }
                        self.game
                            .step_frame()
                            .and_then(|frame| self.game.tick_batch(vec![frame]))
                            .map(|batch| ProtocolUpdate::TickBatch(Box::new(batch)))
                            .map_err(SessionError::from)
                    })
                } else {
                    // 1290：世代过期；ctx[0]=请求世代，ctx[1]=当前世代。
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::new(
                            1290,
                            vec![generation, self.timeline_generation],
                        ),
                    ))
                };
                if let Ok(events) = &result {
                    self.broadcast_update(events.clone());
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.stop_with_failure(HostFailure::step(fatal));
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
                    Ok((
                        self.timeline_generation,
                        self.game.npc_decision_diagnostics(account),
                    ))
                } else {
                    Err(SendCommandError::Rejected(format!(
                        "STALE_SESSION_GENERATION: fault[1290] requested {generation}; current is {}",
                        self.timeline_generation
                    )))
                };
                let _ = reply.send(result);
            }
            SessionCommand::SetSpeed { speed, reply } => {
                let result = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else {
                    self.apply_speed(speed);
                    Ok(())
                };
                let _ = reply.send(result);
            }
            SessionCommand::SetRunning { running, reply } => {
                let result = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else {
                    self.pacing.set_running(running, self.game.tick());
                    Ok(())
                };
                let _ = reply.send(result);
            }
            SessionCommand::SetPausePreferences {
                generation,
                preferences,
                reply,
            } => {
                let result = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else if generation == self.timeline_generation {
                    self.pause_preferences = preferences;
                    Ok(())
                } else {
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::bare(1290),
                    ))
                };
                let _ = reply.send(result);
            }
            SessionCommand::Shutdown { reply } => {
                if let Err(error) = self.game.shared_ingress().close() {
                    self.stop_with_failure(HostFailure::civil(&error));
                }
                let _ = reply.send(());
            }
            SessionCommand::Member {
                subject,
                request,
                reply,
            } => {
                let result = if let Some(error) = self.fatal_rejection() {
                    Err(SendCommandError::Rejected(error.to_string()))
                } else {
                    self.handle_member_request(subject, request)
                };
                let _ = reply.send(result);
            }
            SessionCommand::ArchiveMetadata {
                subject,
                generation,
                slot_id,
                name,
                copy,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected("STALE_SESSION_GENERATION".into()))
                } else {
                    self.require_market_control(&subject).and_then(|()| {
                        let writer = self.archive.as_ref().ok_or_else(|| {
                            SendCommandError::Rejected("Native 存档目标未初始化".into())
                        })?;
                        if copy {
                            writer
                                .copy_slot(&slot_id, &uuid::Uuid::new_v4().to_string(), &name)
                                .map(Some)
                                .map_err(|error| SendCommandError::Rejected(error.to_string()))
                        } else {
                            writer
                                .rename_slot(&slot_id, &name)
                                .map(|()| None)
                                .map_err(|error| SendCommandError::Rejected(error.to_string()))
                        }
                    })
                };
                let _ = reply.send(result);
            }
            SessionCommand::DeleteArchive {
                subject,
                generation,
                slot_id,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected("STALE_SESSION_GENERATION".into()))
                } else {
                    self.require_market_control(&subject).and_then(|()| {
                        let writer = self.archive.as_ref().ok_or_else(|| {
                            SendCommandError::Rejected("Native 存档目标未初始化".into())
                        })?;
                        let next = writer
                            .delete_slot(&slot_id, &uuid::Uuid::new_v4().to_string())
                            .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                        self.archive = Some(next);
                        Ok(())
                    })
                };
                let _ = reply.send(result);
            }
            SessionCommand::SelectArchive {
                subject,
                generation,
                slot_id,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected("STALE_SESSION_GENERATION".into()))
                } else {
                    self.require_market_control(&subject).and_then(|()| {
                        self.archive
                            .as_ref()
                            .ok_or_else(|| {
                                SendCommandError::Rejected("Native 存档目标未初始化".into())
                            })?
                            .select(&slot_id)
                            .map(|()| true)
                            .map_err(|error| SendCommandError::Rejected(error.to_string()))
                    })
                };
                let _ = reply.send(result);
            }
        }
    }

    /// 应用新倍速：重算 tick 周期。`run()` 在处理完 `SetSpeed` 后会重建 tokio interval。
    fn apply_speed(&mut self, speed: f64) {
        let old_interval = self.pacing.tick_interval();
        self.pacing.apply_speed(speed, self.game.tick());
        if self.pacing.is_fastest() {
            debug!(session = %self.session_id, "speed changed to fastest");
        } else {
            let new_interval = self.pacing.tick_interval();
            debug!(session = %self.session_id, ?old_interval, ?new_interval, speed, "speed changed");
        }
    }

    /// 当前 interval 对应的 `Duration`（至少 1ms）。
    fn tick_duration(&self) -> Duration {
        self.pacing.tick_interval()
    }
}
