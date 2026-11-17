//! 会话权威的共同状态转移、故障策略与公开错误合同；不识别部署目标。
use super::runner::{self, CycleError, DrivePort, Pacing, Permit};
use super::{protocol::ProtocolSession, SessionError, StepFatal};
use libos::clock::Instant;
use serde::Serialize;

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
    pub fault: Option<crate::ValidationFault>,
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
    pub code: &'static str,
    pub message: String,
    pub cause: Option<Box<FailureCause>>,
}

pub fn failure_description(
    error: &(dyn std::error::Error + 'static),
) -> (&'static str, &'static str) {
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
                if fault.code == crate::error::SAVE_TARGET_DAY_NOT_READY =>
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
    if let Some(error) = error.downcast_ref::<crate::session::ReportCorrectionDayEndError>() {
        return if error.classification() == crate::company::CompanyErrorClass::SystemState {
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
    if error.is::<crate::session::CivilInformationError>() {
        return (
            "CIVIL_INFORMATION_FAILED",
            "自然日信息事务失败（内部详情已脱敏）",
        );
    }
    if let Some(error) = error.downcast_ref::<crate::accounting::closing::ClosingError>() {
        return match error {
            crate::accounting::closing::ClosingError::Accounting(_) => (
                "CLOSING_ACCOUNTING_FAILED",
                "日终封账的会计处理失败（私有详情已脱敏）",
            ),
            crate::accounting::closing::ClosingError::Report(_) => (
                "CLOSING_REPORT_FAILED",
                "日终封账的报表处理失败（私有详情已脱敏）",
            ),
            _ => (
                "CLOSING_VALIDATION_FAILED",
                "日终封账校验失败（私有详情已脱敏）",
            ),
        };
    }
    if let Some(error) = error.downcast_ref::<crate::accounting::AccountingError>() {
        return match error {
            crate::accounting::AccountingError::AmountOverflow { .. } => (
                "ACCOUNTING_AMOUNT_OVERFLOW",
                "公司会计金额运算溢出（操作数已脱敏）",
            ),
            _ => (
                "ACCOUNTING_VALIDATION_FAILED",
                "公司会计校验失败（私有详情已脱敏）",
            ),
        };
    }
    if error.is::<crate::session::StepFatal>() {
        return (
            "INVARIANT_VIOLATION",
            "引擎不变量校验失败（原始详情已脱敏）",
        );
    }
    if error.is::<crate::session::ReportCorrectionRestoreError>() {
        return (
            "REPORT_CORRECTION_RESTORE_FAILED",
            "报表更正存档关联校验失败（原始详情已脱敏）",
        );
    }
    ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
}

pub fn save_error_message(error: SessionError) -> String {
    if matches!(&error, SessionError::InvalidSave(fault) if fault.code == crate::error::SAVE_TARGET_DAY_NOT_READY)
    {
        let (code, message) = failure_description(&error);
        format!("{code}: {message}")
    } else {
        error.to_string()
    }
}

pub fn failure_cause(error: &(dyn std::error::Error + 'static)) -> Option<Box<FailureCause>> {
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
    pub fn new(operation: &'static str) -> Self {
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
    pub fn catalog(
        error: &crate::session::CompanyCatalogViewError,
        operation: &'static str,
        location: &'static str,
    ) -> Self {
        let mut context = FailureContext::new(operation);
        context.fault = Some(crate::session::company_catalog_fault(error));
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

    pub fn step(error: impl std::borrow::Borrow<StepFatal>) -> Self {
        let error = error.borrow();
        let crate::session::StepFatal::InvariantViolation { location, .. } = error;
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

    pub fn civil(error: impl std::borrow::Borrow<SessionError>) -> Self {
        let error = error.borrow();
        match error {
            SessionError::Step(fatal) => Self::step(fatal),
            other => Self {
                code: "CIVIL_DAY_SETTLEMENT_FAILED",
                message: failure_description(other).1.to_owned(),
                r#where: "session.authority.civil_day".into(),
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

    pub fn correction_prepare_rejected(error: &SessionError) -> Self {
        let mut failure = Self::civil(error);
        failure.code = "REPORT_CORRECTION_PREPARE_REJECTED";
        failure.r#where = "session.authority.prepare_civil_updates".into();
        failure.recoverable = true;
        failure.recovery_actions = vec![
            "取消或修正待处理更正后恢复推进并重试日终",
            "复制脱敏错误详情反馈",
        ];
        failure
    }

    pub fn is_correction_prepare_rejected(&self, generation: u64) -> bool {
        self.code == "REPORT_CORRECTION_PREPARE_REJECTED"
            && self.r#where == "session.authority.prepare_civil_updates"
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

    /// 归档失败不撤销已提交日界，也不允许遗漏候选继续跨日。
    pub fn archive(error: &str) -> Self {
        Self {
            code: "DAY_END_ARCHIVE_FAILED",
            message: format!("日终已提交，但当前日终尚未确认耐久；市场保持归档阻塞，上一份有效日终档仍保留：{error}"),
            r#where: "session.authority.save_day_end".into(),
            cause: None,
            context: FailureContext::new("saveDayEnd"),
            recoverable: true,
            recovery_actions: vec![
                "检查存储权限和空间；当前日终保持阻塞，明确恢复有效日终档或新局",
                "复制存档错误详情反馈",
            ],
        }
    }
}

impl From<StepFatal> for HostFailure {
    fn from(error: StepFatal) -> Self {
        Self::step(error)
    }
}
impl From<String> for HostFailure {
    fn from(error: String) -> Self {
        Self::archive(&error)
    }
}
impl From<Box<HostFailure>> for HostFailure {
    fn from(error: Box<HostFailure>) -> Self {
        *error
    }
}
impl HostFailure {
    pub fn civil_ref(error: &SessionError) -> Self {
        Self::civil(error)
    }
    pub fn at_session(mut self, session: &ProtocolSession) -> Self {
        self.context.tick.get_or_insert(session.tick());
        self.context.seq.get_or_insert(session.seq());
        self.context.day.get_or_insert(session.day());
        self
    }
    pub fn with_archive_key(&mut self, key: super::checkpoint::CheckpointKey) {
        self.context.tick = Some(key.tick);
        self.context.seq = Some(key.seq);
        self.context.day = Some(key.day);
        self.context.generation = Some(key.generation.to_string());
    }
}

/// 运行控制、故障锁存与节拍属于同一个权威，传输任务仅消费其输出。
pub const SHUTDOWN_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Authority {
    pacing: Pacing,
    fatal: Option<HostFailure>,
    recovery: Option<HostFailure>,
}
impl std::ops::Deref for Authority {
    type Target = Pacing;
    fn deref(&self) -> &Pacing {
        &self.pacing
    }
}
impl std::ops::DerefMut for Authority {
    fn deref_mut(&mut self) -> &mut Pacing {
        &mut self.pacing
    }
}
impl Authority {
    pub fn new(base_ms: u64, tick: u64, now: Instant) -> Self {
        Self {
            pacing: Pacing::new(base_ms, tick, now),
            fatal: None,
            recovery: None,
        }
    }
    pub fn fatal_failure(&self) -> Option<&HostFailure> {
        self.fatal.as_ref()
    }
    pub fn latest_failure(&self) -> Option<&HostFailure> {
        self.fatal.as_ref().or(self.recovery.as_ref())
    }
    fn civil_committed(&mut self) {
        if self
            .recovery
            .as_ref()
            .is_some_and(|failure| failure.code == "REPORT_CORRECTION_PREPARE_REJECTED")
        {
            self.recovery = None;
        }
    }
    pub fn require_control(&self, blocked: Option<&str>) -> Result<(), SessionError> {
        if self.fatal.is_some() || blocked.is_some() {
            return Err(SessionError::InvalidSave(
                crate::error::ValidationFault::bare(1291),
            ));
        }
        Ok(())
    }
    pub fn reset_after_restore(&mut self, tick: u64, now: Instant) {
        self.fatal = None;
        self.recovery = None;
        self.pacing.reset_after_restore(tick, now);
    }
    fn latch(&mut self, failure: HostFailure, now: Instant) -> Option<HostFailure> {
        self.pacing.stop_after_failure(now);
        if !failure.recoverable {
            if self.fatal.is_some() {
                return None;
            }
            self.fatal = Some(failure.clone());
        } else {
            self.recovery = Some(failure.clone());
        }
        Some(failure)
    }
    /// 可恢复次错仍显式交付，首个 terminal 永不被替换。
    pub fn fail(
        &mut self,
        session: &ProtocolSession,
        generation: u64,
        failure: HostFailure,
        now: Instant,
    ) -> Option<HostFailure> {
        let mut failure = failure.at_session(session);
        failure
            .context
            .generation
            .get_or_insert_with(|| generation.to_string());
        if failure.code == "DAY_END_ARCHIVE_FAILED" {
            if let Err(error) = session.shared_ingress().suspend() {
                use std::fmt::Write;
                write!(failure.message, "；暂停共享 ingress 失败：{error}")
                    .expect("写 String 不会失败");
            }
        }
        runner::apply_authority_failure(session, failure.recoverable, &mut failure.message);
        self.latch(failure, now)
    }
}

pub fn classify_cycle<E: Into<HostFailure>>(failure: CycleError<E>) -> HostFailure {
    let (mut failure, range) = match failure {
        CycleError::Advance {
            source,
            unpublished,
        } => (HostFailure::civil(source), unpublished),
        CycleError::Batch {
            source,
            unpublished,
        } => (HostFailure::step(source), Some(unpublished)),
        CycleError::Civil { source, retryable } => {
            let error = source.into_error();
            (
                if retryable {
                    HostFailure::correction_prepare_rejected(&error)
                } else {
                    HostFailure::civil(error)
                },
                None,
            )
        }
        CycleError::Stage(failure) => (failure.into(), None),
    };
    if let Some(range) = range {
        range.describe(&mut failure.message);
    }
    failure
}

pub trait AuthorityPort: DrivePort {
    fn authority(&mut self) -> &mut Authority;
    fn archive_blocked(&self) -> bool;
}

/// 完整自动 turn：宿主不再匹配 CycleError 或自行选择暂停/恢复策略。
pub fn drive<P: AuthorityPort>(
    port: &mut P,
    permit: Permit,
    pause: super::protocol::PausePreferences,
) -> Option<HostFailure>
where
    P::Error: Into<HostFailure>,
{
    if port.archive_blocked() || port.authority().fatal_failure().is_some() {
        let now = port.now();
        port.authority().stop_after_failure(now);
        return None;
    }
    let previous_date = port.session().civil_date();
    let result = runner::run_cycle(port, permit, pause);
    if port.session().civil_date() != previous_date {
        port.authority().civil_committed();
    }
    let tick = port.session().tick();
    let now = port.now();
    runner::apply_cycle_pacing(port.authority(), tick, now, result)
        .err()
        .map(classify_cycle)
}

#[cfg(test)]
mod tests;

pub struct ManualOutcome<T> {
    pub result: Result<T, SessionError>,
    pub failure: Option<HostFailure>,
}
fn reject_manual<T>(error: SessionError, failure: Option<HostFailure>) -> ManualOutcome<T> {
    ManualOutcome {
        result: Err(error),
        failure,
    }
}
fn require_port<P: AuthorityPort>(port: &mut P) -> Result<(), SessionError> {
    port.session()
        .game()
        .require_healthy()
        .map_err(SessionError::from)?;
    let blocked = port.archive_blocked();
    port.authority()
        .require_control(blocked.then_some("日终归档阻塞"))
}

/// 手动 tick 的拒绝、冻结、交接与错误分类均由共享权威负责。
pub fn manual_tick<P: AuthorityPort>(port: &mut P) -> ManualOutcome<super::protocol::EngineUpdate>
where
    P::Error: Into<HostFailure>,
{
    if let Err(error) = require_port(port) {
        let failure = match &error {
            SessionError::Step(fatal) => Some(HostFailure::step(fatal)),
            _ => None,
        };
        return reject_manual(error, failure);
    }
    let running = port.authority().is_running();
    match runner::manual::manual_tick(port.session_mut(), running) {
        Ok(batch) => {
            let update = super::protocol::EngineUpdate::TickBatch(Box::new(batch.clone()));
            let failure = port.stage_tick_batch(batch).err().map(Into::into);
            ManualOutcome {
                result: Ok(update),
                failure,
            }
        }
        Err(source) => {
            let error = source.into_session_error();
            let failure = match &error {
                SessionError::Step(fatal) => Some(HostFailure::step(fatal)),
                _ => None,
            };
            reject_manual(error, failure)
        }
    }
}

/// 手工日结保留已提交响应；归档交接失败另以有序权威失败交付，不重做日结。
pub fn manual_civil<P: AuthorityPort>(
    port: &mut P,
    pause: super::protocol::PausePreferences,
) -> ManualOutcome<super::protocol::CivilUpdate>
where
    P::Error: Into<HostFailure>,
{
    use runner::manual::ManualCivilError;
    if let Err(error) = require_port(port) {
        let failure = match &error {
            SessionError::Step(fatal) => Some(HostFailure::step(fatal)),
            _ => None,
        };
        return reject_manual(error, failure);
    }
    match runner::manual::manual_civil(port.session_mut(), pause) {
        Ok((update, paused)) => {
            port.authority().civil_committed();
            if paused {
                let tick = port.session().tick();
                let now = port.now();
                port.authority().pause_at_civil_boundary(tick, now);
            }
            let failure = port
                .stage_civil(update.clone(), paused)
                .err()
                .map(Into::into);
            ManualOutcome {
                result: Ok(update),
                failure,
            }
        }
        Err(ManualCivilError::Rejected(error)) => reject_manual(error, None),
        Err(ManualCivilError::Retryable(error)) => {
            let failure = HostFailure::correction_prepare_rejected(&error);
            reject_manual(error, Some(failure))
        }
        Err(ManualCivilError::Fatal(error)) => {
            let failure = HostFailure::civil(&error);
            reject_manual(error, Some(failure))
        }
    }
}

impl Authority {
    pub fn control_running(
        &mut self,
        running: bool,
        tick: u64,
        now: Instant,
        blocked: Option<&str>,
    ) -> Result<(), SessionError> {
        self.require_control(if running { blocked } else { None })?;
        self.pacing.set_running(running, tick, now);
        Ok(())
    }
    pub fn control_speed(
        &mut self,
        speed: f64,
        tick: u64,
        now: Instant,
        blocked: Option<&str>,
    ) -> Result<(), SessionError> {
        self.require_control(blocked)?;
        self.pacing.apply_speed(speed, tick, now).map_err(|_| {
            SessionError::InvalidSave(crate::error::ValidationFault::new(
                1311,
                vec![speed.to_bits()],
            ))
        })
    }
    pub fn control_pause(
        &self,
        preferences: super::protocol::PausePreferences,
    ) -> Result<super::protocol::PausePreferences, SessionError> {
        self.require_control(None)?;
        Ok(preferences)
    }
}

/// 关闭只有入口、I/O 与耐久收据全部完成才成功；保留全部次错。
pub fn finish_shutdown(
    ingress: Result<(), String>,
    transport: Result<(), String>,
    receipts: Result<(), String>,
) -> Result<(), String> {
    let errors: Vec<String> = [ingress, transport, receipts]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

impl Authority {
    pub fn begin_shutdown(
        &mut self,
        session: &ProtocolSession,
        now: Instant,
    ) -> Result<(), String> {
        self.pacing.stop_after_failure(now);
        session
            .shared_ingress()
            .close()
            .map_err(|error| error.to_string())
    }
}
