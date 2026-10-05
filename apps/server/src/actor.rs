//! actor-per-session（ADR-0005 §5）：每 session 一个 tokio task 独占 `GameSession`。
//!
//! 设计要点（市场状态独占、契合 engine `Send`）：
//! - `GameSession` 由 actor task 独占；Player 与 NPC 共用每会话 ingress，短生命周期锁不覆盖 step。
//! - Player 在鉴权与 generation 检查后直接登记 ingress；控制命令（快照 / 改速 / restore）走 mpsc。
//! - **更新广播**（`tokio::sync::broadcast`）：actor 每轮把 `Event[]` 与必要的权威运行快照
//!   作为一个原子批次 broadcast；WS 只是传输适配器，与 Worker/Tauri 的应用层语义一致。
//! - **步进节拍**：`interval = base_ms / speed`；`select!` 同时等命令与 interval tick。
//!
//! 远程多人交易依据认证主体的经济成员关系派生本人账户，市场控制授权独立于日终经济档。

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
use native_store::{ArchiveWriter, NativeDatabase};
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
type MarketIngress = (
    u64,
    engine::SharedSessionIngress,
    std::collections::BTreeMap<OpaqueSubjectId, AccountId>,
);

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

impl EngineUpdate {
    pub fn for_account(self, account: AccountId) -> Self {
        self.for_member_account(Some(account))
    }

    pub fn for_member_account(mut self, account: Option<AccountId>) -> Self {
        match self.update.as_mut() {
            Some(ProtocolUpdate::TickBatch(batch)) => {
                for frame in &mut batch.frames {
                    project_viewer_events(&mut frame.events, &mut frame.facts, account);
                }
                if let Some(snapshot) = batch.runtime_snapshot.as_mut() {
                    snapshot.accounts.retain(|id, _| Some(*id) == account);
                }
                if let Some(delta) = batch.runtime_delta.as_mut() {
                    delta.accounts.retain(|id, _| Some(*id) == account);
                    delta
                        .working_orders
                        .upserts
                        .retain(|order| Some(order.owner) == account);
                    delta.working_orders.removed.retain(|order| Some(order.owner) == account);
                }
            }
            Some(ProtocolUpdate::CivilUpdate(update)) => {
                project_viewer_events(&mut update.events, &mut update.facts, account);
                for frame in &mut update.refresh.intraday {
                    project_viewer_events(&mut frame.events, &mut frame.facts, account);
                }
                update.refresh.snapshot.accounts.retain(|id, _| Some(*id) == account);
            }
            None => {}
        }
        self
    }
}

fn project_viewer_events(events: &mut [engine::Event], facts: &mut [engine::session::protocol::EventFact], account: Option<AccountId>) {
    assert_eq!(events.len(), facts.len(), "已提交事件与完整事实必须一一对应");
    for (event, fact) in events.iter_mut().zip(facts) {
        match event {
            engine::Event::Trade { seq, code, price, qty, .. } => {
                *event = engine::Event::PublicTrade { seq: *seq, code: code.clone(), price: *price, qty: *qty };
            }
            engine::Event::IntentRejected { account: owner, seq, .. }
            | engine::Event::SettlementError { account: owner, seq, .. }
            | engine::Event::OrderCanceled { account: owner, seq, .. }
            | engine::Event::OrderAccepted { account: owner, seq, .. } if Some(*owner) != account => {
                *event = engine::Event::PrivateEventOmitted { seq: *seq };
            }
            engine::Event::PublicTrade { .. }
            | engine::Event::PrivateEventOmitted { .. }
            | engine::Event::AuctionTick { .. }
            | engine::Event::AuctionCompleted { .. }
            | engine::Event::PriceTick { .. }
            | engine::Event::DayBoundary { .. }
            | engine::Event::CivilDateAdvanced { .. }
            | engine::Event::CompanyDisclosurePublished { .. }
            | engine::Event::IntentRejected { .. }
            | engine::Event::SettlementError { .. }
            | engine::Event::OrderCanceled { .. }
            | engine::Event::OrderAccepted { .. } => {}
        }
        let local_index = if matches!(event, engine::Event::PrivateEventOmitted { .. }) { event.seq() } else { fact.key.local_event_index() };
        fact.key = engine::session::pipeline::EventStableKey::for_event(event, local_index);
        fact.event = event.clone();
        let canonical = serde_json::to_value(&event).expect("已经校验的投影事件必须可编码为规范JSON值");
        fact.canonical_payload = serde_json::to_string(&canonical).expect("规范JSON值必须可编码为事件事实字符串");
    }
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
    fn archive(error: &str) -> Self {
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
            SessionError::ReportCorrection(_) => Self {
                code: "REPORT_CORRECTION_REJECTED",
                message: error.to_string(),
                r#where: "server.actor.end_civil_day".into(),
                cause: failure_cause(error),
                context: FailureContext::new("endCivilDay"),
                recoverable: true,
                recovery_actions: vec![
                    "查询并取消错误的待处理更正后重试日结",
                    "复制更正操作身份与错误详情反馈",
                ],
            },
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
    pub markets: std::collections::BTreeMap<engine::StockCode, engine::MarketSnap>,
    pub accounts: std::collections::BTreeMap<AccountId, engine::AccountSnap>,
    pub daily_candles: std::collections::BTreeMap<engine::StockCode, Vec<engine::DailyCandle>>,
    pub active_daily_candles: std::collections::BTreeMap<engine::StockCode, engine::DailyCandle>,
}

impl From<Snapshot> for PublicBaselineSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        Self {
            seq: snapshot.seq,
            tick: snapshot.tick,
            day: snapshot.day,
            phase: snapshot.phase,
            markets: snapshot.markets,
            accounts: snapshot.accounts,
            daily_candles: snapshot.daily_candles,
            active_daily_candles: snapshot.active_daily_candles,
        }
    }
}

impl PublicBaseline {
    pub fn for_account(mut self, account: AccountId) -> Self {
        self.snapshot.accounts.retain(|id, _| *id == account);
        self
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
    ArchiveMetadata {
        subject: OpaqueSubjectId,
        generation: u64,
        slot_id: String,
        name: String,
        copy: bool,
        reply: oneshot::Sender<Result<Option<native_store::ArchiveMetadata>, SendCommandError>>,
    },
    DeleteArchive {
        subject: OpaqueSubjectId,
        generation: u64,
        slot_id: String,
        reply: oneshot::Sender<Result<(), SendCommandError>>,
    },
    SelectArchive {
        subject: OpaqueSubjectId,
        generation: u64,
        slot_id: String,
        reply: oneshot::Sender<Result<bool, SendCommandError>>,
    },
    Member {
        subject: OpaqueSubjectId,
        request: MemberRequest,
        reply: oneshot::Sender<Result<MemberResponse, SendCommandError>>,
    },
    /// 可信宿主入队玩家意图；远程请求使用主体感知入口，不接受客户端任意账户身份。
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
        code: StockCode,
        reply:
            oneshot::Sender<Result<(u64, engine::session::HistoricalStockData), SendCommandError>>,
    },
    InitialAllocation {
        generation: u64,
        reply: oneshot::Sender<Result<(u64, engine::session::InitialAllocation), SendCommandError>>,
    },
    PersonalTradeConfirmations {
        generation: u64,
        before_receipt: Option<u64>,
        reply: oneshot::Sender<
            Result<(u64, Vec<engine::session::PersonalTradeConfirmation>), SendCommandError>,
        >,
    },
    CalculateIntradayAverage {
        generation: u64,
        stats: engine::session::DailyTradeStats,
        volume_shares: u64,
        reply: oneshot::Sender<Result<(u64, Option<engine::IntradayAverage>), SendCommandError>>,
    },
    CalculateIntradayAverageCurve {
        generation: u64,
        samples: Vec<engine::IntradayAverageInput>,
        reply:
            oneshot::Sender<Result<(u64, Vec<Option<engine::IntradayAverage>>), SendCommandError>>,
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
        subject: Option<OpaqueSubjectId>,
        generation: Option<u64>,
        archive_slot_id: Option<String>,
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
    PublicReportAvailability {
        query: engine::company::PublicReportAvailabilityQuery,
        reply: oneshot::Sender<Result<engine::company::PublicReportAvailability, SessionError>>,
    },
    SubmitReportCorrection {
        generation: u64,
        request: engine::CompanyReportCorrection,
        reply: oneshot::Sender<Result<(u64, ()), SendCommandError>>,
    },
    CancelReportCorrection {
        generation: u64,
        operation_id: String,
        reply: oneshot::Sender<Result<(u64, ()), SendCommandError>>,
    },
    QueryReportCorrections {
        generation: u64,
        reply: oneshot::Sender<Result<(u64, engine::ReportCorrectionStatus), SendCommandError>>,
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
}

#[derive(Debug)]
pub enum MemberRequest {
    Reset {
        generation: u64,
        setup: Box<SessionSetup>,
        seed: u64,
    },
    Baseline,
    Context,
    Shutdown {
        generation: u64,
    },
    BindCreator,
    Join {
        generation: Option<u64>,
        confirmed_rejoin: bool,
    },
    Resolve,
    Control,
    Snapshot,
    WorkingOrders {
        generation: u64,
    },
    History {
        generation: u64,
        code: StockCode,
    },
    Confirmations {
        generation: u64,
        before_receipt: Option<u64>,
    },
    TradeHistory {
        generation: u64,
        query: engine::session::PersonalTradeHistoryRequest,
    },
    MarketHistory {
        generation: u64,
        query: engine::session::MarketHistoryRequest,
    },
    CurrentMinuteHistory {
        generation: u64,
        query: engine::session::CurrentMinuteHistoryRequest,
    },
    AdmissionCash {
        generation: u64,
        cash: engine::Money,
    },
    SetSpeed {
        generation: u64,
        speed: f64,
    },
    SetRunning {
        generation: u64,
        running: bool,
    },
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
    Confirmations(u64, Vec<engine::session::PersonalTradeConfirmation>),
    TradeHistory(u64, engine::session::PersonalTradeHistoryPage),
    MarketHistory(u64, engine::session::MarketHistoryPage),
    CurrentMinuteHistory(u64, engine::session::CurrentMinuteHistoryResponse),
}

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
        fastest_permits_for_workers, fixed_tick_duration, SpeedMeter, FASTEST_BATCH_BUDGET,
    };
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
    ingress: Arc<RwLock<MarketIngress>>,
    event_tx: broadcast::Sender<EngineUpdate>,
    pub ticks_per_day: u64,
    pub auction_ticks: u64,
    pub closing_auction_ticks: u64,
    pub session_token: String,
    pub startup_setup: SessionSetup,
    pub startup_seed: u64,
    pub startup_resumed: bool,
}

impl SessionHandles {
    pub async fn mutate_archive_metadata(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot_id: String,
        name: String,
        copy: bool,
    ) -> Result<Option<native_store::ArchiveMetadata>, SendCommandError> {
        let (reply, response) = oneshot::channel();
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
        response.await.map_err(|_| SendCommandError::ActorGone)?
    }
    pub async fn delete_archive(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot_id: String,
    ) -> Result<(), SendCommandError> {
        let (reply, response) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::DeleteArchive {
                subject,
                generation,
                slot_id,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        response.await.map_err(|_| SendCommandError::ActorGone)?
    }
    pub async fn select_archive(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot_id: String,
    ) -> Result<bool, SendCommandError> {
        let (reply, response) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SelectArchive {
                subject,
                generation,
                slot_id,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        response.await.map_err(|_| SendCommandError::ActorGone)?
    }
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
    pub async fn public_baseline_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<PublicBaseline, SendCommandError> {
        match self
            .member_request(subject, MemberRequest::Baseline)
            .await?
        {
            MemberResponse::Baseline(baseline) => Ok(baseline),
            _ => Err(SendCommandError::Rejected(
                "本人市场baseline响应类型错误".into(),
            )),
        }
    }

    pub async fn market_context_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<MarketContext, SendCommandError> {
        match self.member_request(subject, MemberRequest::Context).await? {
            MemberResponse::Context(context) => Ok(context),
            _ => Err(SendCommandError::Rejected("市场上下文响应类型错误".into())),
        }
    }

    pub async fn shutdown_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
    ) -> Result<(), SendCommandError> {
        self.member_request(subject, MemberRequest::Shutdown { generation })
            .await
            .map(|_| ())
    }
    async fn member_request(
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

    pub async fn bind_creator(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<MarketMembership, SendCommandError> {
        match self
            .member_request(subject, MemberRequest::BindCreator)
            .await?
        {
            MemberResponse::Membership(member) => Ok(member),
            _ => Err(SendCommandError::Rejected("市场创建者响应类型错误".into())),
        }
    }

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

    pub async fn resolve_member(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<MarketMembership, SendCommandError> {
        match self.member_request(subject, MemberRequest::Resolve).await? {
            MemberResponse::Membership(member) => Ok(member),
            _ => Err(SendCommandError::Rejected("市场成员响应类型错误".into())),
        }
    }

    pub async fn require_control(&self, subject: OpaqueSubjectId) -> Result<(), SendCommandError> {
        match self.member_request(subject, MemberRequest::Control).await? {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("市场控制响应类型错误".into())),
        }
    }

    pub async fn set_admission_cash_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        cash: engine::Money,
    ) -> Result<(), SendCommandError> {
        match self
            .member_request(subject, MemberRequest::AdmissionCash { generation, cash })
            .await?
        {
            MemberResponse::Control => Ok(()),
            _ => Err(SendCommandError::Rejected("入场资金响应类型错误".into())),
        }
    }

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
        self.member_request(subject, MemberRequest::SetSpeed { generation, speed })
            .await
            .map(|_| ())
    }

    pub async fn set_running_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        running: bool,
    ) -> Result<(), SendCommandError> {
        self.member_request(
            subject,
            MemberRequest::SetRunning {
                generation,
                running,
            },
        )
        .await
        .map(|_| ())
    }

    pub async fn set_pause_preferences_for(
        &self,
        subject: OpaqueSubjectId,
        generation: u64,
        preferences: PausePreferences,
    ) -> Result<(), SendCommandError> {
        self.member_request(
            subject,
            MemberRequest::SetPausePreferences {
                generation,
                preferences,
            },
        )
        .await
        .map(|_| ())
    }

    pub async fn restore_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot: SaveSlot,
    ) -> Result<Snapshot, SendCommandError> {
        self.restore_archive_for(generation, subject, slot, None)
            .await
    }

    pub async fn restore_archive_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        slot: SaveSlot,
        archive_slot_id: Option<String>,
    ) -> Result<Snapshot, SendCommandError> {
        self.validate_restore_clock(&slot)?;
        let (reply, receive) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Restore {
                archive_slot_id,
                subject: Some(subject),
                generation: Some(generation),
                slot: Box::new(slot),
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        receive
            .await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn snapshot_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<Snapshot, SendCommandError> {
        match self
            .member_request(subject, MemberRequest::Snapshot)
            .await?
        {
            MemberResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(SendCommandError::Rejected(
                "本人账户快照响应类型错误".into(),
            )),
        }
    }

    pub async fn enqueue_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        intent: Intent,
    ) -> Result<(), SendCommandError> {
        if self.cmd_tx.is_closed() {
            return Err(SendCommandError::ActorGone);
        }
        let ingress = self.ingress.read().map_err(|error| {
            SendCommandError::Rejected(format!("session ingress lifecycle lock poisoned: {error}"))
        })?;
        if generation != ingress.0 {
            return Err(SendCommandError::Rejected(
                "STALE_SESSION_GENERATION".into(),
            ));
        }
        let account = ingress.2.get(&subject).ok_or_else(|| {
            SendCommandError::Rejected(
                engine::session::MembershipError::RejoinConfirmationRequired.to_string(),
            )
        })?;
        ingress
            .1
            .enqueue_player_intent(*account, intent)
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn working_orders_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
    ) -> Result<(u64, serde_json::Value), SendCommandError> {
        match self
            .member_request(subject, MemberRequest::WorkingOrders { generation })
            .await?
        {
            MemberResponse::Orders(generation, orders) => Ok((generation, orders)),
            _ => Err(SendCommandError::Rejected(
                "本人账户委托响应类型错误".into(),
            )),
        }
    }

    pub async fn history_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        code: StockCode,
    ) -> Result<(u64, engine::session::HistoricalStockData), SendCommandError> {
        match self
            .member_request(subject, MemberRequest::History { generation, code })
            .await?
        {
            MemberResponse::History(generation, history) => Ok((generation, history)),
            _ => Err(SendCommandError::Rejected(
                "本人账户历史响应类型错误".into(),
            )),
        }
    }

    pub async fn confirmations_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        before_receipt: Option<u64>,
    ) -> Result<(u64, Vec<engine::session::PersonalTradeConfirmation>), SendCommandError> {
        match self
            .member_request(
                subject,
                MemberRequest::Confirmations {
                    generation,
                    before_receipt,
                },
            )
            .await?
        {
            MemberResponse::Confirmations(generation, confirmations) => {
                Ok((generation, confirmations))
            }
            _ => Err(SendCommandError::Rejected(
                "本人账户成交确认响应类型错误".into(),
            )),
        }
    }

    pub async fn trade_history_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        query: engine::session::PersonalTradeHistoryRequest,
    ) -> Result<(u64, engine::session::PersonalTradeHistoryPage), SendCommandError> {
        match self.member_request(subject, MemberRequest::TradeHistory { generation, query }).await? {
            MemberResponse::TradeHistory(generation, page) => Ok((generation, page)),
            _ => Err(SendCommandError::Rejected("本人日期交割历史响应类型错误".into())),
        }
    }
    pub async fn market_history_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        query: engine::session::MarketHistoryRequest,
    ) -> Result<(u64, engine::session::MarketHistoryPage), SendCommandError> {
        match self.member_request(subject, MemberRequest::MarketHistory { generation, query }).await? {
            MemberResponse::MarketHistory(generation, page) => Ok((generation, page)),
            _ => Err(SendCommandError::Rejected("公开日期量价历史响应类型错误".into())),
        }
    }

    pub async fn current_minute_history_for(&self, generation: u64, subject: OpaqueSubjectId, query: engine::session::CurrentMinuteHistoryRequest) -> Result<(u64, engine::session::CurrentMinuteHistoryResponse), SendCommandError> {
        match self.member_request(subject, MemberRequest::CurrentMinuteHistory { generation, query }).await? {
            MemberResponse::CurrentMinuteHistory(generation, response) => Ok((generation, response)),
            _ => Err(SendCommandError::Rejected("当前分钟历史响应类型错误".into())),
        }
    }

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

    /// 可信本地／测试宿主便捷入口：玩家 `AccountId(0)` 立即登记到共享 ingress。
    ///
    /// 失败两种：actor 已退出（通道关闭）→ `SendCommandError`；engine 拒绝 → `SessionError`。
    /// 两者都显式上抛，不静默。
    pub async fn enqueue(&self, generation: u64, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(generation, AccountId(0), intent).await
    }

    /// 可信宿主按已解析账户入队；生产远程请求经 `enqueue_for` 派生本人账户。
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
                "STALE_SESSION_GENERATION: requested {generation}; current generation is {}",
                ingress.0
            )));
        }
        ingress
            .1
            .enqueue_player_intent(player_id, intent)
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
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
        code: StockCode,
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

    pub async fn personal_trade_confirmations(
        &self,
        generation: u64,
        before_receipt: Option<u64>,
    ) -> Result<(u64, Vec<engine::session::PersonalTradeConfirmation>), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PersonalTradeConfirmations {
                generation,
                before_receipt,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub async fn calculate_intraday_average(
        &self,
        generation: u64,
        stats: engine::session::DailyTradeStats,
        volume_shares: u64,
    ) -> Result<(u64, Option<engine::IntradayAverage>), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CalculateIntradayAverage {
                generation,
                stats,
                volume_shares,
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

    pub async fn calculate_intraday_average_curve(
        &self,
        generation: u64,
        samples: Vec<engine::IntradayAverageInput>,
    ) -> Result<(u64, Vec<Option<engine::IntradayAverage>>), SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CalculateIntradayAverageCurve {
                generation,
                samples,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
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
        self.validate_restore_clock(&slot)?;
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Restore {
                subject: None,
                archive_slot_id: None,
                generation: None,
                slot: Box::new(slot),
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    fn validate_restore_clock(&self, slot: &SaveSlot) -> Result<(), SendCommandError> {
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
        Ok(())
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

    pub async fn public_report_availability(
        &self,
        query: engine::company::PublicReportAvailabilityQuery,
    ) -> Result<engine::company::PublicReportAvailability, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReportAvailability { query, reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn submit_report_correction(
        &self,
        generation: u64,
        request: engine::CompanyReportCorrection,
    ) -> Result<(u64, ()), SendCommandError> {
        let (reply, response) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SubmitReportCorrection {
                generation,
                request,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        response.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub async fn cancel_report_correction(
        &self,
        generation: u64,
        operation_id: String,
    ) -> Result<(u64, ()), SendCommandError> {
        let (reply, response) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CancelReportCorrection {
                generation,
                operation_id,
                reply,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        response.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub async fn query_report_corrections(
        &self,
        generation: u64,
    ) -> Result<(u64, engine::ReportCorrectionStatus), SendCommandError> {
        let (reply, response) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::QueryReportCorrections { generation, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        response.await.map_err(|_| SendCommandError::ActorGone)?
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
}

/// Session 注册表：`DashMap<session_id, Arc<SessionHandles>>`。
///
/// `DashMap` 内部分片锁、读多写少场景高效；actor 状态本身不存于此（actor task own），
/// 这里只存消息端点——无共享可变游戏状态。
#[derive(Clone)]
pub struct SessionManager {
    database: NativeDatabase,
    shared_market_gate: Arc<std::sync::Mutex<()>>,
    sessions: Arc<DashMap<String, Arc<SessionHandles>>>,
    active_count: Arc<AtomicUsize>,
    fastest_budget: Arc<Semaphore>,
    base_ms: u64,
}

impl SessionManager {
    /// 默认基准（`BASE_TICK_MS`）。
    pub fn default_base() -> Self {
        Self::with_base_ms(BASE_TICK_MS)
    }

    /// 测试用：自定义 base_ms（很小的值可加速 step 以便断言事件）。
    pub fn with_base_ms(base_ms: u64) -> Self {
        Self::with_base_ms_and_database(
            base_ms,
            NativeDatabase::open_in_memory().expect("测试 SQLite 初始化失败"),
        )
    }

    pub fn with_database(database: NativeDatabase) -> Self {
        Self::with_base_ms_and_database(BASE_TICK_MS, database)
    }

    fn with_base_ms_and_database(base_ms: u64, database: NativeDatabase) -> Self {
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
            database,
            shared_market_gate: Arc::new(std::sync::Mutex::new(())),
            sessions: Arc::new(DashMap::new()),
            active_count: Arc::new(AtomicUsize::new(0)),
            fastest_budget: Arc::new(Semaphore::new(fastest_permits)),
            base_ms,
        }
    }

    /// 创建新 session：构造 `GameSession` → 建 mpsc+broadcast → spawn actor task → 注册。
    ///
    /// 失败显式返回 `SessionError`（构造非法参数），绝不静默吞（铁律二）。
    pub fn new_session(&self, setup: SessionSetup, seed: u64) -> Result<String, NewSessionError> {
        self.new_session_bound(setup, seed, None)
    }

    pub fn market_ids(&self) -> Vec<String> {
        self.sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn database(&self) -> NativeDatabase {
        self.database.clone()
    }

    pub fn first_session_id(&self) -> Option<String> {
        self.market_ids().into_iter().min()
    }

    pub fn resume_session(&self, slot_id: &str) -> Result<Option<String>, NewSessionError> {
        let _guard = self
            .shared_market_gate
            .lock()
            .map_err(|error| SessionError::InvalidSetup(format!("共享市场创建锁损坏：{error}")))?;
        if self.active_session_count() != 0 {
            return Err(
                SessionError::InvalidSave("只允许在启动且没有内存市场时恢复日终档".into()).into(),
            );
        }
        let slot = self
            .database
            .load(slot_id)
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        let Some(slot) = slot else {
            return Ok(None);
        };
        let game = ProtocolSession::restore(&slot)?;
        self.active_count.fetch_add(1, Ordering::AcqRel);
        self.register_game(game, slot.setup, slot.seed, slot_id, true, None)
            .map(Some)
    }

    pub fn resume_selected_session(&self) -> Result<Option<String>, NewSessionError> {
        match self
            .database
            .selection()
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?
        {
            native_store::ArchiveSelection::Selected(slot_id) => self.resume_session(&slot_id),
            native_store::ArchiveSelection::Uninitialized => Ok(None),
            native_store::ArchiveSelection::Cleared => Err(SessionError::InvalidSave(
                "已选启动槽已删除；请明确选择其他日终档或创建新局，不自动回退其他档".into(),
            )
            .into()),
        }
    }

    pub fn new_shared_session(
        &self,
        setup: SessionSetup,
        seed: u64,
        creator: OpaqueSubjectId,
    ) -> Result<(String, bool), NewSessionError> {
        let _guard = self
            .shared_market_gate
            .lock()
            .map_err(|error| SessionError::InvalidSetup(format!("共享市场创建锁损坏：{error}")))?;
        if let Some(entry) = self.sessions.iter().next() {
            return Ok((entry.key().clone(), false));
        }
        self.new_session_bound(setup, seed, Some(creator))
            .map(|id| (id, true))
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
                return Err(SessionError::InvalidSetup(error.to_string()).into());
            }
        }
        self.register_game(game, setup, seed, "current", false, creator)
    }

    fn register_game(
        &self,
        game: ProtocolSession,
        setup: SessionSetup,
        seed: u64,
        slot_id: &str,
        resumed: bool,
        creator: Option<OpaqueSubjectId>,
    ) -> Result<String, NewSessionError> {
        let archive = match ArchiveWriter::activate(self.database.clone(), slot_id) {
            Ok(archive) => archive,
            Err(error) => {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(SessionError::InvalidSave(error.to_string()).into());
            }
        };
        let controllers = (|| {
            let mut controllers = self
                .database
                .market_controllers("shared-market")?
                .into_iter()
                .map(|subject| {
                    OpaqueSubjectId::new(subject)
                        .map_err(|error| native_store::StoreError::Invalid(error.to_string()))
                })
                .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
            if let Some(creator) = creator {
                self.database
                    .grant_market_control("shared-market", creator.as_str())?;
                controllers.insert(creator);
            }
            Ok::<_, native_store::StoreError>(controllers)
        })();
        let controllers = match controllers {
            Ok(controllers) => controllers,
            Err(error) => {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                return Err(SessionError::InvalidSave(error.to_string()).into());
            }
        };
        let ticks_per_day = setup.ticks_per_day;
        let auction_ticks = setup.auction_ticks;
        let closing_auction_ticks = setup.closing_auction_ticks;
        let session_id = uuid::Uuid::new_v4().to_string();

        // 命令按提交顺序入队；不以固定条数暂停投递，调用方仍等待逐条处理回执。
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, _event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let ingress = Arc::new(RwLock::new((
            1,
            game.shared_ingress(),
            game.trading_subject_accounts(),
        )));

        let handles = Arc::new(SessionHandles {
            cmd_tx,
            ingress: Arc::clone(&ingress),
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
            controllers,
            archive: Some(archive),
            ingress,
            #[cfg(test)]
            injected_step_failure: None,
            pacing: ServerPacing::new(self.base_ms, game.tick()),
            game,
            cmd_rx,
            event_tx: handles.event_tx.clone(),
            session_id: session_id.clone(),
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
    controllers: std::collections::BTreeSet<OpaqueSubjectId>,
    archive: Option<ArchiveWriter>,
    ingress: Arc<RwLock<MarketIngress>>,
    #[cfg(test)]
    injected_step_failure: Option<(usize, engine::session::StepFatal)>,
    pacing: ServerPacing,
    game: ProtocolSession,
    cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    event_tx: broadcast::Sender<EngineUpdate>,
    session_id: String,
    fastest_budget: Arc<Semaphore>,
    public_revision: u64,
    timeline_generation: u64,
    pause_preferences: PausePreferences,
    fatal_failure: Option<HostFailure>,
}

impl SessionActor {
    fn replace_timeline(
        &mut self,
        mut restored: ProtocolSession,
        archive_slot_id: Option<&str>,
    ) -> Result<(), SessionError> {
        let next_generation = self.timeline_generation.checked_add(1).ok_or_else(|| {
            SessionError::ResourceLimit("session generation exhausted; create a new session".into())
        })?;
        let replace_ingress = || -> Result<(), native_store::StoreError> {
            let mut ingress = self.ingress.write().map_err(|error| {
                SessionError::ResourceLimit(format!(
                    "session ingress lifecycle lock poisoned: {error}"
                ))
            })?;
            ingress.1.close()?;
            *ingress = (
                next_generation,
                restored.shared_ingress(),
                restored.trading_subject_accounts(),
            );
            Ok(())
        };
        let archive = match &self.archive {
            Some(writer) => Some(
                writer
                    .renew_after(archive_slot_id, replace_ingress)
                    .map_err(|error| SessionError::InvalidSave(error.to_string()))?,
            ),
            None => {
                replace_ingress().map_err(|error| SessionError::InvalidSave(error.to_string()))?;
                None
            }
        };
        self.game = restored;
        self.archive = archive;
        self.timeline_generation = next_generation;
        self.public_revision = self.public_revision.saturating_add(1);
        self.pacing.reset_after_restore(self.game.tick());
        self.broadcast_timeline_reset();
        Ok(())
    }
    fn require_market_control(&self, subject: &OpaqueSubjectId) -> Result<(), SendCommandError> {
        if self.controllers.contains(subject) {
            Ok(())
        } else {
            Err(SendCommandError::Rejected(
                "该主体没有当前市场控制能力".into(),
            ))
        }
    }
    fn fatal_rejection(&self) -> Option<SessionError> {
        self.fatal_failure.as_ref().map(|failure| {
            SessionError::InvalidSave(format!(
                "session stopped after {}: {}",
                failure.code, failure.message
            ))
        })
    }

    fn step_game(
        game: &mut ProtocolSession,
        #[cfg(test)] injected_step_failure: &mut Option<(usize, engine::session::StepFatal)>,
    ) -> Result<engine::session::protocol::TickFrame, engine::session::StepFatal> {
        #[cfg(test)]
        if let Some((remaining, error)) = injected_step_failure {
            if *remaining == 0 {
                return Err(error.clone());
            }
            *remaining -= 1;
        }
        game.step_frame()
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
                permit = Arc::clone(&self.fastest_budget).acquire_owned(), if self.pacing.is_fastest() && self.pacing.is_running() => {
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
    /// 单个不可分割的 pipeline tick 可能耗尽整个 14ms 时间片，因此这里不承诺每轮
    /// 至少完成多个 tick；与 desktop/Worker 一致，强合同是按预算 yield 并响应控制命令。
    fn run_fastest_batch(&mut self) {
        self.run_protocol_batch(FASTEST_BATCH_MAX_STEPS);
    }

    fn run_protocol_batch(&mut self, limit: usize) {
        if self.fatal_failure.is_some() {
            return;
        }
        let cmd_rx = &self.cmd_rx;
        let pause_preferences = &self.pause_preferences;
        #[cfg(test)]
        let injected_step_failure = &mut self.injected_step_failure;
        let prepared = self.game.with_publication_transaction(|game| {
            let started = std::time::Instant::now();
            let mut frames = Vec::new();
            for _ in 0..limit {
                if limit > 1 && !cmd_rx.is_empty() {
                    break;
                }
                if game.civil_day_ready()? {
                    break;
                }
                let frame = Self::step_game(
                    game,
                    #[cfg(test)]
                    injected_step_failure,
                )?;
                frames.push(frame);
                #[cfg(test)]
                let injecting = injected_step_failure.is_some();
                #[cfg(not(test))]
                let injecting = false;
                if !injecting && started.elapsed() >= FASTEST_BATCH_BUDGET {
                    break;
                }
            }
            let tick_update = if frames.is_empty() {
                None
            } else {
                Some(ProtocolUpdate::TickBatch(Box::new(
                    game.tick_batch(frames)?,
                )))
            };
            let tick_date = game.civil_date().to_iso();
            let civil_updates = Self::prepare_civil_updates(game, pause_preferences)?;
            Ok((tick_update, tick_date, civil_updates))
        });
        let (tick_update, tick_date, civil_updates) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.rollback_cycle(error);
                return;
            }
        };
        if let Some(update) = tick_update {
            self.broadcast_at(update, tick_date);
        }
        for update in civil_updates {
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
    }

    fn prepare_civil_updates(
        game: &mut ProtocolSession,
        pause_preferences: &PausePreferences,
    ) -> Result<Vec<engine::session::protocol::CivilUpdate>, SessionError> {
        let mut updates = Vec::new();
        while game.civil_day_ready()? {
            let update = game.end_civil_day_update()?;
            let pause = pause_preferences.pauses(&update);
            updates.push(update);
            if pause {
                break;
            }
        }
        Ok(updates)
    }

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
            .and_then(|slot| {
                writer
                    .save_day_end(&slot)
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = saved {
            self.stop_with_failure(HostFailure::archive(&error));
        }
    }

    fn rollback_cycle(&mut self, error: SessionError) {
        self.stop_with_failure(HostFailure::civil(&error));
    }

    fn stop_with_failure(&mut self, mut failure: HostFailure) {
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
    fn report_correction_response<T>(
        &mut self,
        generation: u64,
        operation: impl FnOnce(&mut ProtocolSession) -> Result<T, SessionError>,
    ) -> Result<(u64, T), SendCommandError> {
        if generation != self.timeline_generation {
            return Err(SendCommandError::Rejected(format!(
                "STALE_SESSION_GENERATION: requested {generation}; current generation is {}",
                self.timeline_generation
            )));
        }
        if let Some(error) = self.fatal_rejection() {
            return Err(SendCommandError::Rejected(error.to_string()));
        }
        operation(&mut self.game)
            .map(|value| (generation, value))
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

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
            return Err(SendCommandError::Rejected(
                "STALE_SESSION_GENERATION".into(),
            ));
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
                let mut replacement = ProtocolSession::new(*setup, seed)
                    .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                replacement
                    .bind_market_creator(subject.clone())
                    .map_err(reject)?;
                self.replace_timeline(replacement, None)
                    .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
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
                let mut ingress = self.ingress.write().map_err(|error| {
                    SendCommandError::Rejected(format!(
                        "session ingress lifecycle lock poisoned: {error}"
                    ))
                })?;
                let member = self
                    .game
                    .bind_market_creator(subject.clone())
                    .map_err(reject)?;
                ingress.2 = self.game.trading_subject_accounts();
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
                let mut ingress = self.ingress.write().map_err(|error| {
                    SendCommandError::Rejected(format!(
                        "session ingress lifecycle lock poisoned: {error}"
                    ))
                })?;
                let member = self
                    .game
                    .join_market(subject.clone(), confirmed_rejoin)
                    .map_err(reject)?;
                ingress.2.insert(subject, member.account_id);
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
                snapshot.accounts.retain(|id, _| *id == account);
                Ok(MemberResponse::Snapshot(snapshot))
            }
            MemberRequest::CurrentMinuteHistory { generation, query } => {
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)));
                }
                let account = match self.game.market_membership(&subject) {
                    Ok(member) => Some(member.account_id),
                    Err(engine::session::MembershipError::RejoinConfirmationRequired) => None,
                    Err(error) => return Err(reject(error)),
                };
                let response = match account {
                    Some(account) => self.game.query_current_minute_history_for(account, &query),
                    None => self.game.current_minute_history(&query),
                }.map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                Ok(MemberResponse::CurrentMinuteHistory(generation, response))
            }
            MemberRequest::MarketHistory { generation, query } => {
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)));
                }
                let account = match self.game.market_membership(&subject) {
                    Ok(member) => Some(member.account_id),
                    Err(engine::session::MembershipError::RejoinConfirmationRequired) => None,
                    Err(error) => return Err(reject(error)),
                };
                let page = match account {
                    Some(account) => self.game.query_market_history_for(account, &query),
                    None => self.game.market_history_page(&query),
                }.map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                Ok(MemberResponse::MarketHistory(generation, page))
            }
            request => {
                let generation = match &request {
                    MemberRequest::WorkingOrders { generation }
                    | MemberRequest::History { generation, .. }
                    | MemberRequest::Confirmations { generation, .. }
                    | MemberRequest::TradeHistory { generation, .. } => *generation,
                    _ => unreachable!("成员只读交易请求必须携带generation"),
                };
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)));
                }
                let account = self
                    .game
                    .resolve_trading_account(&subject)
                    .map_err(reject)?;
                match request {
                    MemberRequest::WorkingOrders { .. } => {
                        serde_json::to_value(self.game.account_working_orders(account))
                            .map(|orders| MemberResponse::Orders(generation, orders))
                            .map_err(|error| SendCommandError::Rejected(error.to_string()))
                    }
                    MemberRequest::History { code, .. } => self
                        .game
                        .query_stock_history(account, &code)
                        .map(|history| MemberResponse::History(generation, history))
                        .map_err(|error| SendCommandError::Rejected(error.to_string())),
                    MemberRequest::Confirmations { before_receipt, .. } => {
                        Ok(MemberResponse::Confirmations(
                            generation,
                            self.game
                                .personal_trade_confirmations_page(account, before_receipt),
                        ))
                    }
                    MemberRequest::TradeHistory { query, .. } => {
                        self.game.query_personal_trade_history(account, query)
                            .map(|page| MemberResponse::TradeHistory(generation, page))
                            .map_err(|error| SendCommandError::Rejected(error.to_string()))
                    }
                    _ => unreachable!("成员只读交易请求必须已通过generation校验"),
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: SessionCommand) {
        match cmd {
            SessionCommand::ArchiveMetadata {
                subject,
                generation,
                slot_id,
                name,
                copy,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ))
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
                    Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ))
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
                    Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ))
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
            SessionCommand::Member {
                subject,
                request,
                reply,
            } => {
                let result = self.handle_member_request(subject, request);
                let _ = reply.send(result);
            }
            SessionCommand::SubmitReportCorrection {
                generation,
                request,
                reply,
            } => {
                let result = self.report_correction_response(generation, |game| {
                    let epoch = game.report_correction_epoch();
                    game.enqueue_report_correction(&epoch, request)
                });
                let _ = reply.send(result);
            }
            SessionCommand::CancelReportCorrection {
                generation,
                operation_id,
                reply,
            } => {
                let result = self.report_correction_response(generation, |game| {
                    let epoch = game.report_correction_epoch();
                    game.cancel_report_correction(&epoch, &operation_id)
                });
                let _ = reply.send(result);
            }
            SessionCommand::QueryReportCorrections { generation, reply } => {
                let result =
                    self.report_correction_response(generation, |game| game.report_corrections());
                let _ = reply.send(result);
            }
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
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
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
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
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
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
                } else {
                    self.game
                        .initial_allocation()
                        .map(|data| (self.timeline_generation, data))
                        .map_err(|error| SendCommandError::Rejected(error.to_string()))
                };
                let _ = reply.send(result);
            }
            SessionCommand::PersonalTradeConfirmations {
                generation,
                before_receipt,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
                } else {
                    Ok((
                        self.timeline_generation,
                        self.game
                            .personal_trade_confirmations_page(AccountId(0), before_receipt),
                    ))
                };
                let _ = reply.send(result);
            }
            SessionCommand::CalculateIntradayAverage {
                generation,
                stats,
                volume_shares,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
                } else {
                    engine::calculate_intraday_average(Some(&stats), volume_shares)
                        .map(|average| (self.timeline_generation, average))
                        .map_err(|error| SendCommandError::Rejected(error.to_string()))
                };
                let _ = reply.send(result);
            }
            SessionCommand::CalculateIntradayAverageCurve {
                generation,
                samples,
                reply,
            } => {
                let result = if generation != self.timeline_generation {
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
                } else {
                    engine::calculate_intraday_average_curve(&samples)
                        .map(|results| (self.timeline_generation, results))
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
                    Err(SessionError::InvalidSave(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
                } else if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else if let Some(key) = candidate.as_ref() {
                    self.game.save_candidate(key)
                } else {
                    self.game.save()
                };
                let _ = reply.send(result);
            }
            SessionCommand::Restore {
                subject,
                generation,
                archive_slot_id,
                slot,
                reply,
            } => match (if generation
                .is_some_and(|generation| generation != self.timeline_generation)
            {
                Err(SessionError::InvalidSave("STALE_SESSION_GENERATION".into()))
            } else {
                Ok(())
            })
            .and_then(|_| {
                subject
                    .as_ref()
                    .map(|subject| self.require_market_control(subject))
                    .transpose()
                    .map_err(|error| SessionError::InvalidSave(error.to_string()))
            })
            .and_then(|_| {
                self.fatal_rejection()
                    .map_or_else(|| ProtocolSession::restore(&slot), Err)
            }) {
                Ok(restored) => {
                    if let Err(error) = self.replace_timeline(restored, archive_slot_id.as_deref())
                    {
                        let _ = reply.send(Err(error));
                        return;
                    }
                    let mut snapshot = self.game.snapshot();
                    if let Some(subject) = &subject {
                        let account = match self.game.market_membership(subject) {
                            Ok(member) => Some(member.account_id),
                            Err(engine::session::MembershipError::RejoinConfirmationRequired) => {
                                None
                            }
                            Err(error) => {
                                let _ =
                                    reply.send(Err(SessionError::InvalidSave(error.to_string())));
                                return;
                            }
                        };
                        snapshot.accounts.retain(|id, _| Some(*id) == account);
                    }
                    let _ = reply.send(Ok(snapshot));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            #[cfg(feature = "host-parity")]
            SessionCommand::AdvanceCivilDay { generation, reply } => {
                let result = if let Some(failure) = &self.fatal_failure {
                    Err(SessionError::InvalidSave(format!(
                        "session stopped after {}: {}",
                        failure.code, failure.message
                    )))
                } else if generation == self.timeline_generation {
                    self.game.end_civil_day_update()
                } else {
                    Err(SessionError::InvalidSave(format!(
                        "stale session generation {generation}; current generation is {}",
                        self.timeline_generation
                    )))
                };
                if let Ok(report) = &result {
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
                    Err(SessionError::InvalidSave(format!(
                        "session stopped after {}: {}",
                        failure.code, failure.message
                    )))
                } else if generation == self.timeline_generation {
                    self.game.civil_day_ready().and_then(|ready| {
                        if ready {
                            return Err(SessionError::InvalidSave(
                                "civil day barrier must be published before stepping".into(),
                            ));
                        }
                        self.game
                            .step_frame()
                            .and_then(|frame| self.game.tick_batch(vec![frame]))
                            .map(|batch| ProtocolUpdate::TickBatch(Box::new(batch)))
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
                    self.stop_with_failure(HostFailure::step(fatal));
                }
                let _ = reply.send(result);
            }
            SessionCommand::PublicReportPage { query, reply } => {
                let _ = reply.send(self.game.query_public_reports(&query));
            }
            SessionCommand::PublicReportAvailability { query, reply } => {
                let _ = reply.send(self.game.query_public_report_availability(&query));
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
                    Err(SendCommandError::Rejected(format!("STALE_SESSION_GENERATION: requested {generation}; current generation is {}", self.timeline_generation)))
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
                        "stale timeline generation".into(),
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
