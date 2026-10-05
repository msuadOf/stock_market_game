use super::SessionActor;
use engine::{session::StepFatal, SessionError};
use serde::Serialize;
use tauri::{Emitter, Runtime};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct HostFailure {
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
pub(super) struct FailureContext {
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
pub(super) struct FailureCause {
    code: &'static str,
    message: String,
    cause: Option<Box<FailureCause>>,
}

pub(super) fn failure_description(
    error: &(dyn std::error::Error + 'static),
) -> (&'static str, &'static str) {
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

pub(super) fn failure_cause(
    error: &(dyn std::error::Error + 'static),
) -> Option<Box<FailureCause>> {
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

impl From<StepFatal> for HostFailure {
    fn from(error: StepFatal) -> Self {
        Self::step(error)
    }
}

impl HostFailure {
    pub(super) fn archive(error: &str) -> Self {
        Self {
            code: "DAY_END_ARCHIVE_FAILED",
            message: format!("日终已成功完成，但 SQLite 保存失败，上一份有效日终档仍保留：{error}"),
            r#where: "desktop.actor.save_day_end".into(),
            cause: None,
            context: FailureContext::new("saveDayEnd"),
            recoverable: true,
            recovery_actions: vec!["检查 SQLite 文件权限与磁盘空间后恢复推进，在下一日终重试保存", "复制存档错误详情反馈"],
        }
    }
    pub(super) fn step(error: StepFatal) -> Self {
        let StepFatal::InvariantViolation { ref location, .. } = error;
        Self {
            code: "STEP_FATAL",
            message: failure_description(&error).1.to_owned(),
            r#where: location.clone(),
            cause: failure_cause(&error),
            context: FailureContext::new("step"),
            recoverable: false,
            recovery_actions: vec![
                "停止当前会话；重新打开上一份有效日终存档或新局",
                "复制脱敏错误详情反馈",
            ],
        }
    }

    pub(super) fn civil(error: SessionError) -> Self {
        match error {
            SessionError::ReportCorrection(_) => Self {
                code: "REPORT_CORRECTION_REJECTED",
                message: error.to_string(),
                r#where: "desktop.actor.end_civil_day".into(),
                cause: failure_cause(&error),
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
                message: failure_description(&other).1.to_owned(),
                r#where: "desktop.actor.rollback_cycle".into(),
                cause: failure_cause(&other),
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

#[derive(Clone, Serialize)]
struct EngineFailurePayload<'a> {
    session_id: &'a str,
    timeline_id: &'a str,
    #[serde(flatten)]
    failure: HostFailure,
    events: [engine::Event; 0],
}

impl<R: Runtime> SessionActor<R> {
    pub(super) fn persist_day_end(&mut self, update: &engine::session::protocol::CivilUpdate) {
        let Some(writer) = &self.archive else { return; };
        let key = engine::session::protocol::SaveCandidateKey { seq: update.seq_to, settled_date: update.boundary.settled_date };
        let saved = native_store::DayEndCandidate::capture(&self.game, &key)
            .map_err(|error| error.to_string())
            .and_then(|slot| writer.save_day_end(&slot).map_err(|error| error.to_string()));
        if let Err(error) = saved {
            self.stop_after_host_failure(HostFailure::archive(&error));
        }
    }
    pub(super) fn stop_after_step_failure(&mut self, error: StepFatal) {
        self.stop_after_host_failure(HostFailure::step(error));
    }

    pub(super) fn stop_after_host_failure(&mut self, mut failure: HostFailure) {
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
        failure.context.generation = Some(self.generation.to_string());
        self.pacing.stop_after_failure();
        let payload = EngineFailurePayload {
            session_id: &self.session_id,
            timeline_id: &self.timeline_id,
            failure: failure.clone(),
            events: [],
        };
        if let Err(emit_error) = self.app.emit("engine-failure", payload) {
            eprintln!(
                "[session {}] {} notification failed: {emit_error}; {}",
                self.session_id, failure.code, failure.message
            );
        }
        if !failure.recoverable {
            self.cmd_rx.close();
        }
    }
}
