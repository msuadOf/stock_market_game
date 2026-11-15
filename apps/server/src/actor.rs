//! 每会话只有一个市场权威；推进、批次与日界由共享 runner 决定。
//! Player 与 NPC 共享 ingress，控制指令走邮箱；Native 等待由 LibOS 提供。
//! 对外兼容层只包装已验证的协议批次及带会话元数据的控制响应。

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use dashmap::DashMap;
#[cfg(feature = "host-parity")]
use engine::session::protocol::CivilUpdate;
use engine::session::protocol::{
    EngineUpdate as ProtocolUpdate, PausePreferences, PlayerWorkingOrder, ProtocolSession,
};
use engine::session::publication::{self, Observer, Producer, Push};
#[cfg(test)]
use engine::session::runner::fixed_tick_duration;
#[cfg(test)]
use engine::session::runner::MAX_BATCH_BUDGET;
use engine::session::runner::{
    apply_authority_failure, apply_cycle_pacing, run_cycle, validate_speed, CycleError, DrivePort,
    Pacing as ServerPacing, Permit, WakePlan,
};
pub use engine::session::runner::{
    RequestedSpeed, SpeedMetrics, BASE_TICK_MS, MAX_SPEED_MULTIPLIER,
};
use engine::session::{HostRuntimeTopology, MarketMembership, OpaqueSubjectId};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup, Snapshot};
use libos::native_async::{self, WaitPlan, Wake};
use libos::native_async::{broadcast, mpsc, oneshot};
use serde::Serialize;
use tracing::{debug, info, warn};

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

/// 观察通道只可提取属于授权主体的快照和增量；原始 Arc 不暴露给发送端。
pub struct ScopedEngineUpdate {
    update: Arc<EngineUpdate>,
    account: Option<AccountId>,
}

impl ScopedEngineUpdate {
    pub fn into_projected(
        self,
        initial_orders: &mut Option<Vec<PlayerWorkingOrder>>,
    ) -> EngineUpdate {
        let mut update = Arc::unwrap_or_clone(self.update);
        if let Some(protocol) = &mut update.update {
            publication::scope_observer_update(protocol, self.account);
            publication::seed_observer_orders(protocol, initial_orders);
        }
        update
    }
}

/// 快照和新通道在同一权威循环轮内捕获，通道的初始发布游标为 0。
pub struct ObservationAttachment {
    pub baseline: PublicBaseline,
    pub observer: Observer<ScopedEngineUpdate>,
    pub initial_orders: Option<Vec<PlayerWorkingOrder>>,
}

impl std::fmt::Debug for ObservationAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservationAttachment")
            .field("generation", &self.baseline.timeline_generation)
            .field("seq", &self.baseline.snapshot.seq)
            .finish_non_exhaustive()
    }
}

struct ScopedObserver {
    account: Option<AccountId>,
    producer: Producer<ScopedEngineUpdate>,
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
    pub fault: Option<engine::ValidationFault>,
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
            SessionError::ReportCorrectionDayEnd(source) => failure_description(source),
            SessionError::CivilInformation(_) => (
                "CIVIL_INFORMATION_FAILED",
                "自然日信息事务失败（内部详情已脱敏）",
            ),
            SessionError::Closing(_) => (
                "SESSION_CLOSING_FAILED",
                "自然日日终封账失败（私有详情已脱敏）",
            ),
            SessionError::ReportCorrectionRestore(_) => (
                "SESSION_STATE_INVALID",
                "更正完成事实与真实会计和公开版本不一致（原始详情已脱敏）",
            ),
            SessionError::InvalidSave(fault)
                if fault.code == engine::error::SAVE_TARGET_DAY_NOT_READY =>
            {
                (
                    "SAVE_TARGET_DAY_NOT_READY",
                    "目标自然日尚未完成日终，当前日存档不可用（原始详情已脱敏）",
                )
            }
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
    if let Some(error) = error.downcast_ref::<engine::session::ReportCorrectionDayEndError>() {
        return if error.classification() == engine::company::CompanyErrorClass::SystemState {
            (
                "REPORT_CORRECTION_FAILED",
                "报表更正日终准备失败（内部详情已脱敏）",
            )
        } else {
            (
                "REPORT_CORRECTION_REJECTED",
                "报表更正未满足提交条件（内部详情已脱敏）",
            )
        };
    }
    if error.is::<engine::session::CivilInformationError>() {
        return (
            "CIVIL_INFORMATION_FAILED",
            "自然日信息事务失败（内部详情已脱敏）",
        );
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
    if error.is::<engine::session::ReportCorrectionRestoreError>() {
        return (
            "REPORT_CORRECTION_RESTORE_FAILED",
            "报表更正存档关联校验失败（原始详情已脱敏）",
        );
    }
    ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
}

fn save_error_message(error: SessionError) -> String {
    if matches!(&error, SessionError::InvalidSave(fault) if fault.code == engine::error::SAVE_TARGET_DAY_NOT_READY)
    {
        let (code, message) = failure_description(&error);
        format!("{code}: {message}")
    } else {
        error.to_string()
    }
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
            fault: None,
            tick: None,
            seq: None,
            day: None,
            generation: None,
        }
    }
}

impl HostFailure {
    pub(crate) fn catalog(
        error: &engine::session::CompanyCatalogViewError,
        operation: &'static str,
        location: &'static str,
    ) -> Self {
        let mut context = FailureContext::new(operation);
        context.fault = Some(engine::session::company_catalog_fault(error));
        Self {
            code: "CATALOG_BASELINE_FAILED",
            message: "公开公司身份目录构造失败".to_owned(),
            r#where: location.to_owned(),
            cause: None,
            context,
            recoverable: false,
            recovery_actions: vec![
                "停止当前提交并重新同步权威会话；仍失败则重开有效日终档或新局",
                "复制错误详情反馈",
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
            SessionError::Step(fatal) => Self::step(fatal),
            other => Self {
                code: "CIVIL_DAY_SETTLEMENT_FAILED",
                message: failure_description(other).1.to_owned(),
                r#where: "server.actor.civil_day".into(),
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

    fn correction_prepare_rejected(error: &SessionError) -> Self {
        let mut failure = Self::civil(error);
        failure.code = "REPORT_CORRECTION_PREPARE_REJECTED";
        failure.r#where = "server.actor.prepare_civil_updates".into();
        failure.recoverable = true;
        failure.recovery_actions = vec![
            "取消或修正待处理更正后恢复推进并重试日终",
            "复制脱敏错误详情反馈",
        ];
        failure
    }

    pub(crate) fn is_correction_prepare_rejected(&self, generation: u64) -> bool {
        self.code == "REPORT_CORRECTION_PREPARE_REJECTED"
            && self.r#where == "server.actor.prepare_civil_updates"
            && self.recoverable
            && self.context.operation == "endCivilDay"
            && self.context.fault.is_none()
            && self.context.tick.is_some()
            && self.context.seq.is_some()
            && self.context.day.is_some()
            && self.context.generation.as_deref() == Some(generation.to_string().as_str())
            && self
                .cause
                .as_ref()
                .is_some_and(|cause| cause.code == "REPORT_CORRECTION_REJECTED")
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
    pub stock_catalog: Vec<engine::session::StockIdentityEntry>,
    pub company_catalog: Vec<engine::session::CompanyCatalogEntry>,
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
    pub mirror_origin: engine::RuntimeMirrorOrigin,
    pub markets: std::collections::BTreeMap<engine::StockId, engine::MarketSnap>,
    pub accounts: std::collections::BTreeMap<AccountId, engine::AccountSnap>,
    pub daily_candles: std::collections::BTreeMap<engine::StockId, Vec<engine::DailyCandle>>,
    pub active_daily_candles: std::collections::BTreeMap<engine::StockId, engine::DailyCandle>,
}

impl From<Snapshot> for PublicBaselineSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        Self::scoped(snapshot, Some(AccountId(0)))
    }
}

impl PublicBaselineSnapshot {
    fn scoped(mut snapshot: Snapshot, account: Option<AccountId>) -> Self {
        publication::scope_observer_snapshot(&mut snapshot, account);
        Self {
            seq: snapshot.seq,
            tick: snapshot.tick,
            day: snapshot.day,
            phase: snapshot.phase,
            mirror_origin: snapshot.mirror_origin,
            markets: snapshot.markets,
            accounts: snapshot.accounts,
            daily_candles: snapshot.daily_candles,
            active_daily_candles: snapshot.active_daily_candles,
        }
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
        reply: oneshot::Sender<Result<PublicBaseline, engine::session::CompanyCatalogViewError>>,
    },
    /// 原子建立只读观察流，不改变任一活跃观察者的增量基线。
    ObserveFor {
        subject: OpaqueSubjectId,
        reply: oneshot::Sender<Result<ObservationAttachment, PublicBaselineError>>,
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
        reply: oneshot::Sender<Result<Snapshot, RestoreSessionError>>,
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
    /// 日终 I/O 完成不代表此代 writer 仍拥有会话。
    ArchiveWriteCompleted {
        key: CheckpointKey,
        result: Result<(), String>,
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
pub enum HostQuery {
    MarketHistory(engine::session::MarketHistoryRequest),
    CurrentMinuteHistory(engine::session::CurrentMinuteHistoryRequest),
    PersonalTradeHistory(engine::session::PersonalTradeHistoryRequest),
    PersonalTradeConfirmations(ConfirmationQuery),
    IntradayAverage(engine::IntradayAverageInput),
    IntradayAverageCurve(Vec<engine::IntradayAverageInput>),
}

/// 共享市场成员请求；本人公司查询由当前成员关系导出账户。
#[derive(Debug)]
pub enum MemberRequest {
    HostQuery {
        generation: u64,
        query: HostQuery,
    },
    SubscribeRightsOffering {
        generation: u64,
        event_id: String,
        requested_shares: u64,
    },
    ProposePlayerCompanyAction {
        generation: u64,
        proposal: engine::session::PlayerCompanyProposal,
    },
    EnqueueReportCorrection {
        generation: u64,
        request: engine::session::CompanyReportCorrection,
    },
    CancelReportCorrection {
        generation: u64,
        operation_id: String,
    },
    CompanyRead {
        generation: u64,
        query: engine::session::protocol::CompanyReadQuery,
    },
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
    Shutdown {
        generation: u64,
    },
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
    WorkingOrders {
        generation: u64,
    },
    /// 本人股票历史。
    History {
        generation: u64,
        code: engine::StockId,
    },
    /// 调整入场资金（控制权）。
    AdmissionCash {
        generation: u64,
        cash: engine::Money,
    },
    /// 改速（控制权 Subject 化——取代裸 SetSpeed 的远程形态）。
    SetSpeed {
        generation: u64,
        speed: f64,
    },
    /// 启停（控制权 Subject 化）。
    SetRunning {
        generation: u64,
        running: bool,
    },
    /// 暂停偏好（控制权 Subject 化）。
    SetPausePreferences {
        generation: u64,
        preferences: PausePreferences,
    },
}

#[derive(Debug)]
pub enum MemberResponse {
    HostQuery(u64, serde_json::Value),
    CompanyWriteFatal(HostFailure),
    RightsSubscription(
        u64,
        Result<
            engine::session::RightsSubscriptionReceipt,
            engine::session::protocol::CompanyWriteError,
        >,
    ),
    CompanyProposal(
        u64,
        Result<
            engine::session::PlayerCompanyCommandReceipt,
            engine::session::protocol::CompanyWriteError,
        >,
    ),
    ReportCorrection(
        u64,
        Result<
            engine::session::protocol::ReportCorrectionAcknowledgement,
            engine::session::protocol::CompanyWriteError,
        >,
    ),
    CompanyRead(
        u64,
        AccountId,
        Result<
            engine::session::protocol::CompanyReadResult,
            engine::session::protocol::CompanyReadError,
        >,
    ),
    Baseline(Result<PublicBaseline, engine::session::CompanyCatalogViewError>),
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
#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod session_fixture;

use engine::session::checkpoint::{
    CapturedCheckpoint, CheckpointKey, CheckpointOrder, ReceiptDisposition,
};
use native_store::archive_lane::ArchiveLane;

#[cfg(test)]
mod fatal_tests;

#[cfg(test)]
mod correction_recovery_tests;

#[cfg(test)]
mod interval_tests {
    use super::{fixed_tick_duration, gate_deferred_drop_report, MAX_BATCH_BUDGET};
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
        assert_eq!(MAX_BATCH_BUDGET, Duration::from_millis(14));
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
    pub async fn company_read_query_for(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        query: engine::session::protocol::CompanyReadQuery,
    ) -> Result<
        (
            u64,
            AccountId,
            Result<
                engine::session::protocol::CompanyReadResult,
                engine::session::protocol::CompanyReadError,
            >,
        ),
        SendCommandError,
    > {
        match self
            .member_request(subject, MemberRequest::CompanyRead { generation, query })
            .await?
        {
            MemberResponse::CompanyRead(generation, owner, result) => {
                Ok((generation, owner, result))
            }
            _ => Err(SendCommandError::Rejected(
                "公司只读查询响应类型错误".into(),
            )),
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
            SendCommandError::Rejected("当前市场没有该主体的成员关系，请确认重新加入".into())
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
    ) -> Result<PublicBaseline, PublicBaselineError> {
        match self
            .member_request(subject, MemberRequest::Baseline)
            .await?
        {
            MemberResponse::Baseline(baseline) => baseline.map_err(PublicBaselineError::Catalog),
            _ => Err(PublicBaselineError::Command(SendCommandError::Rejected(
                "本人基线响应类型错误".into(),
            ))),
        }
    }

    /// 产品 WS 使用主体绑定的新观察通道，避免基线和订阅之间出现空窗。
    pub async fn observe_for(
        &self,
        subject: OpaqueSubjectId,
    ) -> Result<ObservationAttachment, PublicBaselineError> {
        let (reply, receive) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ObserveFor { subject, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        receive.await.map_err(|_| SendCommandError::ActorGone)?
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
    pub async fn require_control(&self, subject: OpaqueSubjectId) -> Result<(), SendCommandError> {
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
            .member_request(subject, MemberRequest::AdmissionCash { generation, cash })
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
        validate_speed(BASE_TICK_MS, speed, native_async::now())
            .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
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
                MemberRequest::SetRunning {
                    generation,
                    running,
                },
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
        match self
            .member_request(subject, MemberRequest::Snapshot)
            .await?
        {
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

    pub async fn host_query(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        query: HostQuery,
    ) -> Result<(u64, serde_json::Value), SendCommandError> {
        match self
            .member_request(subject, MemberRequest::HostQuery { generation, query })
            .await?
        {
            MemberResponse::HostQuery(generation, value) => Ok((generation, value)),
            _ => Err(SendCommandError::Rejected("查询响应类型错误".into())),
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

    pub async fn public_baseline(&self) -> Result<PublicBaseline, PublicBaselineError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicBaseline { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(PublicBaselineError::Catalog)
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
            .map_err(|error| SendCommandError::Rejected(save_error_message(error)))
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
        validate_speed(BASE_TICK_MS, speed, native_async::now())
            .map_err(|_| SendCommandError::InvalidSpeed(speed))?;
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
pub enum PublicBaselineError {
    #[error(transparent)]
    Command(#[from] SendCommandError),
    #[error("公开公司身份目录构造失败")]
    Catalog(#[from] engine::session::CompanyCatalogViewError),
}

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

/// 读档既要校验引擎候选，也要独占 Native 日终写入世代；两类失败均保留原因。
#[derive(Debug, thiserror::Error)]
pub enum RestoreSessionError {
    #[error(transparent)]
    Engine(#[from] SessionError),
    #[error("恢复已取消，未能接管日终存档写入：{0}")]
    Archive(#[source] native_store::StoreError),
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
    runtime_topology: HostRuntimeTopology,
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

    /// 专用对照进程选择双组；生产 HTTP 请求不能改变执行拓扑。
    #[cfg(feature = "host-parity")]
    pub fn with_two_group_parity_topology(mut self) -> Self {
        self.runtime_topology = HostRuntimeTopology::TwoGroupParity;
        self
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
            native_store::NativeDatabase::open_in_memory().expect("测试 SQLite 初始化失败"),
        )
    }

    fn with_base_ms_and_database(base_ms: u64, database: native_store::NativeDatabase) -> Self {
        Self {
            database,
            shared_market_gate: Arc::new(std::sync::Mutex::new(())),
            sessions: Arc::new(DashMap::new()),
            active_count: Arc::new(AtomicUsize::new(0)),
            base_ms,
            runtime_topology: HostRuntimeTopology::Standard,
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
        let mut game =
            match ProtocolSession::new_with_topology(setup.clone(), seed, self.runtime_topology) {
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
        let game = ProtocolSession::restore_with_topology(&slot, self.runtime_topology)?;
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
        let mut controllers: std::collections::BTreeSet<OpaqueSubjectId> = self
            .database
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
        let completion = cmd_tx.downgrade();
        let archive_lane = ArchiveLane::spawn(move |key, result| {
            if let Err(error) = &result {
                tracing::warn!(generation = key.generation, seq = key.seq, %error, "异步日终归档失败");
            }
            if let Some(sender) = completion.upgrade() {
                let _ = sender.send(SessionCommand::ArchiveWriteCompleted { key, result });
            }
        });
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
            archive_lane: Some(archive_lane),
            archive_epoch: 1,
            archive_order: CheckpointOrder::new(1, 1),
            #[cfg(test)]
            injected_step_failure: None,
            pacing: ServerPacing::new(self.base_ms, game.tick(), native_async::now()),
            game,
            runtime_topology: self.runtime_topology,
            cmd_rx,
            event_tx: handles.event_tx.clone(),
            observers: Vec::new(),
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
        native_async::spawn(async move {
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

#[cfg(test)]
#[derive(Default)]
struct StepFailureCapture {
    saved: Option<serde_json::Value>,
    origin: Option<engine::RuntimeMirrorOrigin>,
    business: Option<engine::session::StateHash>,
    frames: Vec<engine::session::protocol::TickFrame>,
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
    archive_lane: Option<ArchiveLane>,
    archive_epoch: u64,
    archive_order: CheckpointOrder,
    #[cfg(test)]
    injected_step_failure: Option<(
        usize,
        engine::session::StepFatal,
        Arc<std::sync::Mutex<StepFailureCapture>>,
    )>,
    pacing: ServerPacing,
    game: ProtocolSession,
    runtime_topology: HostRuntimeTopology,
    cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    event_tx: broadcast::Sender<EngineUpdate>,
    observers: Vec<ScopedObserver>,
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
        self.game
            .require_healthy()
            .err()
            .map(SessionError::Step)
            .or_else(|| {
                // 1291 仅表示健康 engine 的 Host fatal；engine poison 必须保留原首错。
                self.fatal_failure
                    .as_ref()
                    .map(|_| SessionError::InvalidSave(engine::error::ValidationFault::bare(1291)))
            })
    }

    fn step_game(
        &mut self,
    ) -> Result<engine::session::protocol::TickFrame, engine::session::StepFatal> {
        #[cfg(test)]
        let capture = if let Some((remaining, error, capture)) = &mut self.injected_step_failure {
            if *remaining == 0 {
                let mut observed = capture.lock().unwrap();
                observed.saved =
                    Some(serde_json::to_value(self.game.game().save().unwrap()).unwrap());
                observed.origin = Some(self.game.runtime_mirror_origin());
                observed.business = Some(self.game.business_state_hash().unwrap());
                return Err(error.clone());
            }
            *remaining -= 1;
            Some(Arc::clone(capture))
        } else {
            None
        };
        let result = self.game.step_frame();
        #[cfg(test)]
        if let (Some(capture), Ok(frame)) = (capture, &result) {
            capture.lock().unwrap().frames.push(frame.clone());
        }
        result
    }
    /// LibOS 只负责唤醒命令或到期；可推进的时间与批大小来自同一 runner。
    async fn run(mut self) {
        info!(session = %self.session_id, "session actor started");
        loop {
            let wait_plan = match self.pacing.cadence.wake_plan() {
                WakePlan::Idle => WaitPlan::Idle,
                WakePlan::At(deadline) => WaitPlan::At(deadline),
                WakePlan::Immediate => WaitPlan::Yield,
            };
            match native_async::wait(&mut self.cmd_rx, wait_plan).await {
                Some(Wake::Command(command)) => {
                    let shutting_down = matches!(command, SessionCommand::Shutdown { .. });
                    self.handle_command(command).await;
                    self.report_deferred_drop_degradation();
                    if shutting_down {
                        break;
                    }
                }
                Some(Wake::Deadline) => {
                    if let Some(permit) = self.pacing.cadence.begin_turn(native_async::now()) {
                        self.drive_cycle(permit);
                    }
                }
                None => break,
            }
        }
        if let Some(lane) = self.archive_lane.take() {
            if let Err(error) = lane.finish().await {
                tracing::error!(%error, "归档任务退出失败");
            }
        }
    }

    fn drive_cycle(&mut self, permit: Permit) {
        if self.fatal_failure.is_some() {
            return;
        }
        let preferences = self.pause_preferences;
        let result = run_cycle(self, permit, preferences);
        match apply_cycle_pacing(
            &mut self.pacing,
            self.game.tick(),
            native_async::now(),
            result,
        ) {
            Ok(()) => self.report_deferred_drop_degradation(),
            Err(CycleError::Advance {
                source,
                unpublished,
            }) => {
                let mut failure = HostFailure::civil(&source);
                if let Some(range) = unpublished {
                    range.describe(&mut failure.message);
                }
                self.stop_with_failure(failure);
            }
            Err(CycleError::Batch {
                source,
                unpublished,
            }) => {
                let mut failure = HostFailure::step(&source);
                unpublished.describe(&mut failure.message);
                self.stop_with_failure(failure);
            }
            Err(CycleError::Civil { source, retryable }) => {
                let error = source.into_error();
                self.stop_with_failure(if retryable {
                    HostFailure::correction_prepare_rejected(&error)
                } else {
                    HostFailure::civil(&error)
                });
            }
            Err(CycleError::Stage(failure)) => self.stop_with_failure(failure),
        }
    }

    #[cfg(test)]
    fn run_protocol_batch(&mut self, limit: usize) {
        self.drive_cycle(Permit::for_explicit_limit(limit));
    }

    #[cfg(test)]
    fn run_fastest_batch(&mut self) {
        self.run_protocol_batch(engine::session::runner::MAX_BATCH_TICKS);
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

    fn publish_civil_update(
        &mut self,
        update: engine::session::protocol::CivilUpdate,
    ) -> Result<(), HostFailure> {
        let archive = self.stage_day_end(&update);
        self.public_revision = self.public_revision.saturating_add(1);
        let date = update.civil_date.clone();
        self.broadcast_at(ProtocolUpdate::CivilUpdate(Box::new(update)), date);
        // 日界已提交，不能因档案队列满而撤销；首个交接错误中断本轮后续日界。
        archive
    }

    fn finish_archive_write(&mut self, key: CheckpointKey, result: Result<(), String>) {
        if self.archive_order.accept_receipt(key) != ReceiptDisposition::Accepted {
            return;
        }
        let Err(error) = result else { return };
        let mut failure = HostFailure::archive(&error);
        failure.context.tick = Some(key.tick);
        failure.context.seq = Some(key.seq);
        failure.context.day = Some(key.day);
        failure.context.generation = Some(key.generation.to_string());
        if self.fatal_failure.is_none() {
            self.pacing.stop_after_failure(native_async::now());
        }
        self.broadcast_failure(failure);
    }

    fn stage_day_end(
        &mut self,
        update: &engine::session::protocol::CivilUpdate,
    ) -> Result<(), HostFailure> {
        let Some(writer) = self.archive.clone() else {
            self.game.discard_pending_save_candidates();
            return Ok(());
        };
        self.archive_order
            .install(self.timeline_generation, self.archive_epoch);
        let storage_error =
            |error: native_store::StoreError| HostFailure::archive(&error.to_string());
        let captured = CapturedCheckpoint::take(
            &self.game,
            update,
            self.timeline_generation,
            self.archive_epoch,
        )
        .map_err(native_store::StoreError::from)
        .map_err(storage_error)?;
        let key = captured.key;
        self.archive_order
            .can_stage(key)
            .map_err(|error| HostFailure::archive(&error.to_string()))?;
        match &self.archive_lane {
            Some(lane) => {
                lane.stage(writer, captured).map_err(storage_error)?;
                self.archive_order
                    .staged(key)
                    .map_err(|error| HostFailure::archive(&error.to_string()))?;
            }
            // 只为直接调用 actor 的夹具保留同步存储；真实会话始终有独立 lane。
            #[cfg(test)]
            None => {
                let candidate = native_store::DayEndCandidate::from_captured(captured.slot)
                    .map_err(storage_error)?;
                writer.save_day_end(&candidate).map_err(storage_error)?;
                self.archive_order
                    .staged(key)
                    .map_err(|error| HostFailure::archive(&error.to_string()))?;
                self.finish_archive_write(key, Ok(()));
            }
            #[cfg(not(test))]
            None => return Err(HostFailure::archive("会话没有初始化存档任务")),
        }
        Ok(())
    }

    async fn archive_io<T>(
        &mut self,
        action: impl FnOnce() -> Result<T, native_store::StoreError> + Send + 'static,
    ) -> Result<T, native_store::StoreError>
    where
        T: Send + 'static,
    {
        match &self.archive_lane {
            Some(lane) => lane.execute(action).await,
            #[cfg(test)]
            None => action(),
            #[cfg(not(test))]
            None => Err(native_store::StoreError::Invalid(
                "会话没有初始化存档任务".into(),
            )),
        }
    }

    fn install_archive_writer(&mut self, writer: Option<native_store::ArchiveWriter>) {
        let same_lease = match (&self.archive, &writer) {
            (Some(before), Some(after)) => before.same_lease(after),
            (None, None) => true,
            _ => false,
        };
        if !same_lease {
            self.archive_epoch = self
                .archive_epoch
                .checked_add(1)
                .expect("会话 writer epoch 已耗尽");
            self.archive_order
                .install(self.timeline_generation, self.archive_epoch);
        }
        self.archive = writer;
    }

    async fn activate_replacement_timeline(
        &mut self,
        generation: u64,
        replacement_ingress: engine::SharedSessionIngress,
        replacement_members: std::collections::BTreeMap<OpaqueSubjectId, AccountId>,
    ) -> Result<(), native_store::StoreError> {
        let ingress = Arc::clone(&self.ingress);
        let members = Arc::clone(&self.members);
        let install = move || -> Result<(), native_store::StoreError> {
            // 锁序与 enqueue_for 一致。全部锁取得且旧入口关闭后才提交两份替换。
            let poisoned =
                || SessionError::ResourceLimit(engine::error::ValidationFault::bare(1293));
            let mut ingress = ingress.write().map_err(|_| poisoned())?;
            let mut members = members.write().map_err(|_| poisoned())?;
            ingress.1.close()?;
            *ingress = (generation, replacement_ingress);
            *members = replacement_members;
            Ok(())
        };
        let renewed = match self.archive.clone() {
            Some(writer) => Some(
                self.archive_io(move || writer.renew_after(None, install))
                    .await?,
            ),
            None => {
                install()?;
                None
            }
        };
        self.install_archive_writer(renewed);
        Ok(())
    }

    fn authorize_archive_command(
        &self,
        generation: u64,
        subject: &OpaqueSubjectId,
    ) -> Result<native_store::ArchiveWriter, SendCommandError> {
        if generation != self.timeline_generation {
            return Err(SendCommandError::Rejected(
                "STALE_SESSION_GENERATION".into(),
            ));
        }
        self.require_market_control(subject)?;
        self.archive
            .as_ref()
            .cloned()
            .ok_or_else(|| SendCommandError::Rejected("Native 存档目标未初始化".into()))
    }

    /// 控制权门禁：subject 必须在 controllers 集内。
    fn require_market_control(&self, subject: &OpaqueSubjectId) -> Result<(), SendCommandError> {
        if self.controllers.contains(subject) {
            Ok(())
        } else {
            Err(SendCommandError::Rejected(
                "该主体没有当前市场控制能力".into(),
            ))
        }
    }

    fn company_write_fatal(
        &mut self,
        error: Option<&engine::session::protocol::CompanyWriteError>,
    ) -> Option<MemberResponse> {
        let fatal = self
            .game
            .require_healthy()
            .err()
            .or_else(|| error.and_then(|error| error.step_fatal().cloned()))?;
        // 公开 fatal 只含脱敏投影；原 typed cause 在转换前进入可信宿主日志。
        if let Some(error) = error {
            warn!(session = %self.session_id, error = ?error, "公司写命令触发引擎致命错误");
        }
        let failure = HostFailure::step(&fatal);
        self.stop_with_failure(failure.clone());
        Some(MemberResponse::CompanyWriteFatal(failure))
    }

    /// 成员请求处理（B3 子集——鉴权后的成员/控制面统一入口）。
    async fn handle_member_request(
        &mut self,
        subject: OpaqueSubjectId,
        request: MemberRequest,
    ) -> Result<MemberResponse, SendCommandError> {
        let reject =
            |error: engine::session::MembershipError| SendCommandError::Rejected(error.to_string());
        let request_generation = match &request {
            MemberRequest::SubscribeRightsOffering { generation, .. }
            | MemberRequest::ProposePlayerCompanyAction { generation, .. }
            | MemberRequest::EnqueueReportCorrection { generation, .. }
            | MemberRequest::CancelReportCorrection { generation, .. }
            | MemberRequest::AdmissionCash { generation, .. }
            | MemberRequest::CompanyRead { generation, .. }
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
            MemberRequest::SubscribeRightsOffering {
                event_id,
                requested_shares,
                ..
            } => {
                let owner = self
                    .game
                    .market_membership(&subject)
                    .map_err(reject)?
                    .account_id;
                if let Some(failure) = &self.fatal_failure {
                    return Ok(MemberResponse::CompanyWriteFatal(failure.clone()));
                }
                if let Some(fatal) = self.company_write_fatal(None) {
                    return Ok(fatal);
                }
                let result =
                    self.game
                        .subscribe_rights_offering(owner, &event_id, requested_shares);
                if let Some(fatal) = self.company_write_fatal(result.as_ref().err()) {
                    return Ok(fatal);
                }
                Ok(MemberResponse::RightsSubscription(
                    self.timeline_generation,
                    result,
                ))
            }
            MemberRequest::ProposePlayerCompanyAction { proposal, .. } => {
                let owner = self
                    .game
                    .market_membership(&subject)
                    .map_err(reject)?
                    .account_id;
                if let Some(failure) = &self.fatal_failure {
                    return Ok(MemberResponse::CompanyWriteFatal(failure.clone()));
                }
                let result = self
                    .game
                    .propose_player_company_action(owner, proposal)
                    .map_err(Into::into);
                if let Some(fatal) = self.company_write_fatal(result.as_ref().err()) {
                    return Ok(fatal);
                }
                Ok(MemberResponse::CompanyProposal(
                    self.timeline_generation,
                    result,
                ))
            }
            MemberRequest::EnqueueReportCorrection { request, .. } => {
                self.require_market_control(&subject)?;
                if let Some(failure) = &self.fatal_failure {
                    return Ok(MemberResponse::CompanyWriteFatal(failure.clone()));
                }
                let epoch = self.game.game().report_correction_epoch();
                let operation_id = request.operation_id.clone();
                let result = self
                    .game
                    .enqueue_report_correction(&epoch, request)
                    .map(
                        |_| engine::session::protocol::ReportCorrectionAcknowledgement {
                            operation_id,
                        },
                    )
                    .map_err(Into::into);
                if let Some(fatal) = self.company_write_fatal(result.as_ref().err()) {
                    return Ok(fatal);
                }
                Ok(MemberResponse::ReportCorrection(
                    self.timeline_generation,
                    result,
                ))
            }
            MemberRequest::CancelReportCorrection { operation_id, .. } => {
                self.require_market_control(&subject)?;
                if let Some(failure) = &self.fatal_failure {
                    return Ok(MemberResponse::CompanyWriteFatal(failure.clone()));
                }
                let epoch = self.game.game().report_correction_epoch();
                let result = self
                    .game
                    .cancel_report_correction(&epoch, &operation_id)
                    .map(
                        |_| engine::session::protocol::ReportCorrectionAcknowledgement {
                            operation_id,
                        },
                    )
                    .map_err(Into::into);
                if let Some(fatal) = self.company_write_fatal(result.as_ref().err()) {
                    return Ok(fatal);
                }
                Ok(MemberResponse::ReportCorrection(
                    self.timeline_generation,
                    result,
                ))
            }
            MemberRequest::CompanyRead { query, .. } => {
                if let Some(error) = self.fatal_rejection() {
                    return Err(SendCommandError::Rejected(error.to_string()));
                }
                let owner = self
                    .game
                    .market_membership(&subject)
                    .map_err(reject)?
                    .account_id;
                Ok(MemberResponse::CompanyRead(
                    self.timeline_generation,
                    owner,
                    self.game.company_read_query(owner, query),
                ))
            }

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
                    || setup.closing_auction_ticks
                        != self.game.current_setup().closing_auction_ticks
                {
                    return Err(SendCommandError::Rejected(
                        "重置市场的交易时钟配置必须与当前共享会话一致".into(),
                    ));
                }
                let mut replacement =
                    ProtocolSession::new_with_topology(*setup, seed, self.runtime_topology)
                        .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                replacement
                    .bind_market_creator(subject.clone())
                    .map_err(reject)?;
                // 独立复核阻断 1 修复：Reset 是时间线替换（与 Restore 同型）——
                // 必须同步替换 ingress（新代次+新会话的收件入口）与成员名册，
                // 并 renew 归档 writer 到新时间线（18725d07 replace_timeline 语义；
                // 此前漏换 ingress 使 reset 后新代次下单全部 STALE、旧单打进已
                // 脱离的旧 ingress）。
                let next_generation = self
                    .timeline_generation
                    .checked_add(1)
                    .ok_or(SendCommandError::Rejected("世代计数耗尽".into()))?;
                self.activate_replacement_timeline(
                    next_generation,
                    replacement.shared_ingress(),
                    replacement.trading_subject_accounts(),
                )
                .await
                .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                self.game = replacement;
                self.timeline_generation = next_generation;
                self.public_revision = self.public_revision.saturating_add(1);
                self.pacing
                    .reset_after_restore(self.game.tick(), native_async::now());
                self.broadcast_timeline_reset();
                Ok(MemberResponse::Snapshot(self.game.snapshot()))
            }
            MemberRequest::Baseline => {
                let account = self.observer_account(&subject)?;
                let catalog = self.game.company_catalog_view();
                let baseline = self
                    .finish_observer_baseline(catalog, account)
                    .map(|(baseline, _)| baseline);
                Ok(MemberResponse::Baseline(baseline))
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
                self.apply_speed(speed)
                    .map_err(|error| SendCommandError::Rejected(error.to_string()))?;
                Ok(MemberResponse::Control)
            }
            MemberRequest::SetRunning { running, .. } => {
                self.require_market_control(&subject)?;
                if let Some(error) = self.fatal_rejection() {
                    return Err(SendCommandError::Rejected(error.to_string()));
                }
                self.pacing
                    .set_running(running, self.game.tick(), native_async::now());
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
            MemberRequest::HostQuery { generation, query } => {
                if generation != self.timeline_generation {
                    return Err(SendCommandError::Rejected(
                        "STALE_SESSION_GENERATION".into(),
                    ));
                }
                let value = match query {
                    HostQuery::MarketHistory(query) => {
                        let account = match self.game.market_membership(&subject) {
                            Ok(member) => Some(member.account_id),
                            Err(engine::session::MembershipError::RejoinConfirmationRequired) => {
                                None
                            }
                            Err(error) => return Err(reject(error)),
                        };
                        let page = match account {
                            Some(account) => self.game.query_market_history_for(account, &query),
                            None => self.game.market_history_page(&query),
                        }
                        .map_err(|e| SendCommandError::Rejected(e.to_string()))?;
                        serde_json::to_value(page)
                    }
                    HostQuery::CurrentMinuteHistory(query) => {
                        let account = match self.game.market_membership(&subject) {
                            Ok(member) => Some(member.account_id),
                            Err(engine::session::MembershipError::RejoinConfirmationRequired) => {
                                None
                            }
                            Err(error) => return Err(reject(error)),
                        };
                        let response = match account {
                            Some(account) => {
                                self.game.query_current_minute_history_for(account, &query)
                            }
                            None => self.game.current_minute_history(&query),
                        }
                        .map_err(|e| SendCommandError::Rejected(e.to_string()))?;
                        serde_json::to_value(response)
                    }
                    HostQuery::PersonalTradeHistory(query) => {
                        let account = self
                            .game
                            .resolve_trading_account(&subject)
                            .map_err(reject)?;
                        serde_json::to_value(
                            self.game
                                .query_personal_trade_history(account, query)
                                .map_err(|e| SendCommandError::Rejected(e.to_string()))?,
                        )
                    }
                    HostQuery::PersonalTradeConfirmations(query) => {
                        let cursor = query.before_receipt;
                        let cursor = cursor
                            .map(|s| {
                                if s.is_empty()
                                    || (s.len() > 1 && s.starts_with('0'))
                                    || !s.bytes().all(|b| b.is_ascii_digit())
                                {
                                    return Err(SendCommandError::Rejected(
                                        "交割单游标无效".into(),
                                    ));
                                }
                                s.parse::<u64>()
                                    .map_err(|e| SendCommandError::Rejected(e.to_string()))
                            })
                            .transpose()?;
                        serde_json::to_value(
                            self.game.personal_trade_confirmations_page(
                                self.game
                                    .resolve_trading_account(&subject)
                                    .map_err(reject)?,
                                cursor,
                            ),
                        )
                    }
                    HostQuery::IntradayAverage(input) => serde_json::to_value(
                        engine::calculate_intraday_average_from_input(&input)
                            .map_err(|e| SendCommandError::Rejected(e.to_string()))?,
                    ),
                    HostQuery::IntradayAverageCurve(samples) => {
                        if samples.len() > 600 {
                            return Err(SendCommandError::Rejected(
                                "均价曲线样本数超过 600".into(),
                            ));
                        }
                        serde_json::to_value(
                            engine::calculate_intraday_average_curve(&samples)
                                .map_err(|e| SendCommandError::Rejected(e.to_string()))?,
                        )
                    }
                }
                .map_err(|e| SendCommandError::Rejected(e.to_string()))?;
                Ok(MemberResponse::HostQuery(generation, value))
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

    fn public_baseline_candidate(
        &mut self,
    ) -> Result<PublicBaseline, engine::session::CompanyCatalogViewError> {
        let catalog = self.game.company_catalog_view();
        self.finish_public_baseline(catalog)
    }

    fn observer_account(
        &self,
        subject: &OpaqueSubjectId,
    ) -> Result<Option<AccountId>, SendCommandError> {
        match self.game.market_membership(subject) {
            Ok(member) => Ok(Some(member.account_id)),
            Err(engine::session::MembershipError::RejoinConfirmationRequired) => Ok(None),
            Err(error) => Err(SendCommandError::Rejected(error.to_string())),
        }
    }

    fn attach_observer(
        &mut self,
        subject: OpaqueSubjectId,
    ) -> Result<ObservationAttachment, PublicBaselineError> {
        let account = self.observer_account(&subject)?;
        let catalog = self.game.company_catalog_view();
        let (baseline, initial_orders) = self.finish_observer_baseline(catalog, account)?;
        self.observers
            .retain(|entry| !entry.producer.is_observer_closed());
        let (producer, observer) = publication::channel(publication::OBSERVER_PACKET_LIMIT);
        if baseline.failure.is_none() {
            self.observers.push(ScopedObserver { account, producer });
        }
        Ok(ObservationAttachment {
            baseline,
            observer,
            initial_orders: Some(initial_orders),
        })
    }

    fn finish_public_baseline(
        &mut self,
        catalog: Result<
            Vec<engine::session::CompanyCatalogEntry>,
            engine::session::CompanyCatalogViewError,
        >,
    ) -> Result<PublicBaseline, engine::session::CompanyCatalogViewError> {
        self.finish_observer_baseline(catalog, Some(AccountId(0)))
            .map(|(baseline, _)| baseline)
    }

    fn finish_observer_baseline(
        &mut self,
        catalog: Result<
            Vec<engine::session::CompanyCatalogEntry>,
            engine::session::CompanyCatalogViewError,
        >,
        account: Option<AccountId>,
    ) -> Result<(PublicBaseline, Vec<PlayerWorkingOrder>), engine::session::CompanyCatalogViewError>
    {
        let company_catalog = match catalog {
            Ok(catalog) => catalog,
            Err(error) => {
                self.stop_with_failure(HostFailure::catalog(
                    &error,
                    "public_baseline",
                    "server.actor.public_baseline",
                ));
                return Err(error);
            }
        };
        if let Err(fatal) = self.game.require_healthy() {
            self.stop_with_failure(HostFailure::step(&fatal));
        }
        // 旧委托投影只覆盖 0 号账户；其余成员不读取或借用该账户的订单。
        let (snapshot, initial_orders) =
            if account == Some(AccountId(0)) && self.fatal_failure.is_none() {
                self.game.observer_baseline()
            } else {
                (self.game.snapshot(), Vec::new())
            };
        Ok((
            PublicBaseline {
                timeline_generation: self.timeline_generation,
                snapshot: PublicBaselineSnapshot::scoped(snapshot, account),
                stock_catalog: self.game.stock_catalog_view(),
                company_catalog,
                civil_date: self.game.civil_date().to_iso(),
                public_revision: self.public_revision,
                public_report_ids: self
                    .game
                    .publication_ids()
                    .into_iter()
                    .map(|id| id.value().to_string())
                    .collect(),
                failure: self.fatal_failure.clone(),
            },
            initial_orders,
        ))
    }

    fn stop_with_failure(&mut self, mut failure: HostFailure) {
        apply_authority_failure(&self.game, failure.recoverable, &mut failure.message);
        failure.context.tick = Some(self.game.tick());
        failure.context.seq = Some(self.game.seq());
        failure.context.day = Some(self.game.day());
        failure.context.generation = Some(self.timeline_generation.to_string());
        self.pacing.stop_after_failure(native_async::now());
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
    fn broadcast_update(&mut self, protocol: ProtocolUpdate) {
        self.broadcast_at(protocol, self.game.civil_date().to_iso());
    }

    fn broadcast_at(&mut self, protocol: ProtocolUpdate, civil_date: String) {
        self.publish_observation(
            EngineUpdate {
                timeline_generation: self.timeline_generation,
                update: Some(protocol),
                civil_date,
                public_revision: self.public_revision,
                failure: None,
            },
            false,
        );
    }

    fn broadcast_failure(&mut self, failure: HostFailure) {
        self.publish_observation(
            EngineUpdate {
                timeline_generation: self.timeline_generation,
                update: None,
                civil_date: self.game.civil_date().to_iso(),
                public_revision: self.public_revision,
                failure: Some(failure),
            },
            true,
        );
    }

    fn broadcast_timeline_reset(&mut self) {
        self.publish_observation(
            EngineUpdate {
                timeline_generation: self.timeline_generation,
                update: None,
                civil_date: self.game.civil_date().to_iso(),
                public_revision: self.public_revision,
                failure: None,
            },
            true,
        );
        // 旧租约收到有序重置通知或显式缺口，绝不转到新一代私有账户。
        self.observers.clear();
    }

    fn publish_observation(&mut self, update: EngineUpdate, urgent: bool) {
        if self.observers.is_empty() {
            // 旧入口仅作为诊断/兼容订阅；产品传输无需这个 broadcast。
            if self.event_tx.receiver_count() != 0 {
                let _ = self.event_tx.send(update);
            }
            return;
        }
        let weight = update
            .update
            .as_ref()
            .map_or(NonZeroUsize::MIN, publication::protocol_weight);
        let update = Arc::new(update);
        self.observers.retain_mut(|entry| {
            let scoped = ScopedEngineUpdate {
                update: Arc::clone(&update),
                account: entry.account,
            };
            let result = if urgent {
                entry.producer.publish_weighted_urgent(scoped, weight)
            } else {
                entry.producer.publish_weighted(scoped, weight)
            };
            !matches!(result, Push::ObserverClosed { .. })
        });
        if self.event_tx.receiver_count() != 0 {
            let _ = self.event_tx.send(Arc::unwrap_or_clone(update));
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
                } else if let Some(error) = self.fatal_rejection() {
                    Err(SendCommandError::Rejected(error.to_string()))
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
                } else if let Some(error) = self.fatal_rejection() {
                    Err(SendCommandError::Rejected(error.to_string()))
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
                } else if let Some(error) = self.fatal_rejection() {
                    Err(SendCommandError::Rejected(error.to_string()))
                } else {
                    self.game
                        .initial_allocation()
                        .map(|data| (self.timeline_generation, data))
                        .map_err(|error| SendCommandError::Rejected(error.to_string()))
                };
                let _ = reply.send(result);
            }
            SessionCommand::PublicBaseline { reply } => {
                let baseline = self.public_baseline_candidate();
                let _ = reply.send(baseline);
            }
            SessionCommand::ObserveFor { subject, reply } => {
                let attachment = self.attach_observer(subject);
                if reply.send(attachment).is_err() {
                    self.observers
                        .retain(|entry| !entry.producer.is_observer_closed());
                }
            }
            SessionCommand::SpeedMetrics { reply } => {
                let metrics = self
                    .pacing
                    .refresh_metrics(self.game.tick(), native_async::now());
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
                || ProtocolSession::restore_with_topology(&slot, self.runtime_topology),
                Err,
            ) {
                Ok(restored) => {
                    let next_generation = match self.timeline_generation.checked_add(1) {
                        Some(generation) => generation,
                        None => {
                            // 1292：世代计数耗尽。
                            let _ = reply.send(Err(SessionError::ResourceLimit(
                                engine::error::ValidationFault::bare(1292),
                            )
                            .into()));
                            return;
                        }
                    };
                    let activation = self
                        .activate_replacement_timeline(
                            next_generation,
                            restored.shared_ingress(),
                            restored.trading_subject_accounts(),
                        )
                        .await;
                    if let Err(error) = activation {
                        let error = match error {
                            native_store::StoreError::Session(error) => error.into(),
                            error => RestoreSessionError::Archive(error),
                        };
                        let _ = reply.send(Err(error));
                        return;
                    }
                    self.game = restored;
                    self.timeline_generation = next_generation;
                    self.public_revision = self.public_revision.saturating_add(1);
                    self.pacing
                        .reset_after_restore(self.game.tick(), native_async::now());
                    self.broadcast_timeline_reset();
                    let _ = reply.send(Ok(self.game.snapshot()));
                }
                Err(error) => {
                    let _ = reply.send(Err(error.into()));
                }
            },
            #[cfg(feature = "host-parity")]
            SessionCommand::AdvanceCivilDay { generation, reply } => {
                use engine::session::runner::manual::{manual_civil, ManualCivilError};

                let result = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else if generation != self.timeline_generation {
                    // 1290：世代过期；ctx[0]=请求世代，ctx[1]=当前世代。
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::new(
                            1290,
                            vec![generation, self.timeline_generation],
                        ),
                    ))
                } else {
                    match manual_civil(&mut self.game, self.pause_preferences) {
                        Ok((update, paused)) => {
                            // 日界已提交；归档及发包使用与自动推进相同的非阻塞交接。
                            if paused {
                                self.pacing
                                    .pause_at_civil_boundary(self.game.tick(), native_async::now());
                            }
                            if let Err(failure) = self.publish_civil_update(update.clone()) {
                                self.stop_with_failure(failure);
                            }
                            Ok(update)
                        }
                        Err(ManualCivilError::Rejected(error)) => Err(error),
                        Err(ManualCivilError::Retryable(error)) => {
                            self.stop_with_failure(HostFailure::correction_prepare_rejected(
                                &error,
                            ));
                            Err(error)
                        }
                        Err(ManualCivilError::Fatal(error)) => {
                            self.stop_with_failure(HostFailure::civil(&error));
                            Err(error)
                        }
                    }
                };
                let _ = reply.send(result);
            }
            #[cfg(feature = "host-parity")]
            SessionCommand::Step { generation, reply } => {
                use engine::session::runner::manual::{manual_tick, ManualStepError};

                let result = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else if generation != self.timeline_generation {
                    // 1290：世代过期；ctx[0]=请求世代，ctx[1]=当前世代。
                    Err(SessionError::InvalidSave(
                        engine::error::ValidationFault::new(
                            1290,
                            vec![generation, self.timeline_generation],
                        ),
                    ))
                } else {
                    manual_tick(&mut self.game, self.pacing.is_running())
                        .map(|batch| ProtocolUpdate::TickBatch(Box::new(batch)))
                        .map_err(ManualStepError::into_session_error)
                };
                if let Ok(update) = &result {
                    self.broadcast_update(update.clone());
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
                    self.apply_speed(speed)
                };
                let _ = reply.send(result);
            }
            SessionCommand::SetRunning { running, reply } => {
                let result = if let Some(error) = self.fatal_rejection() {
                    Err(error)
                } else {
                    self.pacing
                        .set_running(running, self.game.tick(), native_async::now());
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
                if let Some(lane) = self.archive_lane.take() {
                    if let Err(error) = lane.finish().await {
                        tracing::error!(%error, "归档任务退出失败");
                    }
                }
                let _ = reply.send(());
            }
            SessionCommand::Member {
                subject,
                request,
                reply,
            } => {
                let fatal_probe = matches!(
                    &request,
                    MemberRequest::Control
                        | MemberRequest::Resolve
                        | MemberRequest::SubscribeRightsOffering { .. }
                        | MemberRequest::ProposePlayerCompanyAction { .. }
                        | MemberRequest::EnqueueReportCorrection { .. }
                        | MemberRequest::CancelReportCorrection { .. }
                );
                let result = match self.fatal_rejection() {
                    Some(error) if !fatal_probe => {
                        Err(SendCommandError::Rejected(error.to_string()))
                    }
                    _ => self.handle_member_request(subject, request).await,
                };
                let _ = reply.send(result);
            }
            SessionCommand::ArchiveWriteCompleted { key, result } => {
                self.finish_archive_write(key, result);
            }
            SessionCommand::ArchiveMetadata {
                subject,
                generation,
                slot_id,
                name,
                copy,
                reply,
            } => {
                let result = match self.authorize_archive_command(generation, &subject) {
                    Ok(writer) => self
                        .archive_io(move || {
                            if copy {
                                writer
                                    .copy_slot(&slot_id, &uuid::Uuid::new_v4().to_string(), &name)
                                    .map(Some)
                            } else {
                                writer.rename_slot(&slot_id, &name).map(|()| None)
                            }
                        })
                        .await
                        .map_err(|error| SendCommandError::Rejected(error.to_string())),
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            SessionCommand::DeleteArchive {
                subject,
                generation,
                slot_id,
                reply,
            } => {
                let result = match self.authorize_archive_command(generation, &subject) {
                    Ok(writer) => self
                        .archive_io(move || {
                            writer.delete_slot(&slot_id, &uuid::Uuid::new_v4().to_string())
                        })
                        .await
                        .map(|next| self.install_archive_writer(Some(next)))
                        .map_err(|error| SendCommandError::Rejected(error.to_string())),
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            SessionCommand::SelectArchive {
                subject,
                generation,
                slot_id,
                reply,
            } => {
                let result = match self.authorize_archive_command(generation, &subject) {
                    Ok(writer) => self
                        .archive_io(move || writer.select(&slot_id))
                        .await
                        .map(|()| true)
                        .map_err(|error| SendCommandError::Rejected(error.to_string())),
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
        }
    }

    /// 将既有对外倍速值应用到共享 Cadence，再报告调速。
    fn apply_speed(&mut self, speed: f64) -> Result<(), SessionError> {
        let old_interval = self.pacing.tick_interval();
        self.pacing
            .apply_speed(speed, self.game.tick(), native_async::now())
            .map_err(|_| {
                SessionError::InvalidSave(engine::error::ValidationFault::new(
                    1311,
                    vec![speed.to_bits()],
                ))
            })?;
        if self.pacing.is_fastest() {
            debug!(session = %self.session_id, "speed changed to fastest");
        } else {
            let new_interval = self.pacing.tick_interval();
            debug!(session = %self.session_id, ?old_interval, ?new_interval, speed, "speed changed");
        }
        Ok(())
    }
}

impl DrivePort for SessionActor {
    type Error = HostFailure;

    fn session(&self) -> &ProtocolSession {
        &self.game
    }

    fn session_mut(&mut self) -> &mut ProtocolSession {
        &mut self.game
    }

    fn command_pending(&self) -> bool {
        !self.cmd_rx.is_empty()
    }

    fn step_frame(
        &mut self,
    ) -> Result<engine::session::protocol::TickFrame, engine::session::StepFatal> {
        self.step_game()
    }

    fn stage_tick_batch(
        &mut self,
        batch: engine::session::protocol::TickBatch,
    ) -> Result<(), Self::Error> {
        self.broadcast_at(
            ProtocolUpdate::TickBatch(Box::new(batch)),
            self.game.civil_date().to_iso(),
        );
        Ok(())
    }

    fn stage_civil(
        &mut self,
        update: engine::session::protocol::CivilUpdate,
        _barrier_paused: bool,
    ) -> Result<(), Self::Error> {
        self.publish_civil_update(update)
    }

    #[cfg(test)]
    fn enforce_budget(&self) -> bool {
        self.injected_step_failure.is_none()
    }
}

#[cfg(test)]
mod catalog_boundary_tests {
    use super::*;
    use std::error::Error;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn observation_attaches_at_frozen_baseline_with_nonzero_member_scope() {
        let manager = SessionManager::with_base_ms(10);
        let setup =
            session_fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
        let id = manager.new_session(setup, 7).unwrap();
        let handles = manager.lookup(&id).unwrap();
        let subject = OpaqueSubjectId::new("observation-member".into()).unwrap();
        let member = handles.join_member(subject.clone(), true).await.unwrap();
        assert_ne!(member.account_id, AccountId(0));
        let mut attached = handles.observe_for(subject).await.unwrap();
        assert_eq!(
            attached
                .baseline
                .snapshot
                .accounts
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![member.account_id]
        );
        assert_eq!(attached.initial_orders, Some(Vec::new()));
        assert!(matches!(attached.observer.poll(), publication::Read::Empty));

        handles.set_running(true).await.unwrap();
        let wire = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let signal = Arc::clone(attached.observer.signal());
                let before = signal.load();
                match attached.observer.poll() {
                    publication::Read::Item { ordinal: 1, value } => {
                        return value.into_projected(&mut attached.initial_orders);
                    }
                    publication::Read::Empty => {
                        signal.wait_for_change(before).await;
                    }
                    _ => panic!("首个观察批必须紧接原子建立的基线"),
                }
            }
        })
        .await
        .unwrap();
        let Some(ProtocolUpdate::TickBatch(batch)) = wire.update else {
            panic!("新观察者应收到首个有效 tick 批")
        };
        assert_eq!(batch.frames[0].seq_from, attached.baseline.snapshot.seq);
        let delta = batch.runtime_delta.as_ref().unwrap();
        assert!(delta.accounts.keys().all(|id| *id == member.account_id));
        assert!(delta.working_orders.reset);
        assert!(delta.working_orders.removed.is_empty());
        assert!(delta.working_orders.upserts.is_empty());
        handles.shutdown().await.unwrap();
    }

    #[test]
    fn catalog_error_keeps_leaf_and_numeric_fault() {
        let error = PublicBaselineError::Catalog(
            engine::session::CompanyCatalogViewError::UnknownListedStock {
                company: engine::company::CompanyId(7),
            },
        );
        let source = error
            .source()
            .unwrap()
            .downcast_ref::<engine::session::CompanyCatalogViewError>()
            .unwrap();
        let failure =
            HostFailure::catalog(source, "public_baseline", "server.actor.public_baseline");
        assert_eq!(
            failure.context.fault,
            Some(engine::ValidationFault::new(3019, vec![7]))
        );
        assert!(!failure.recoverable);
    }
}
