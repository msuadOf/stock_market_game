use super::{delivery::Packet, SessionActor};
use engine::{session::StepFatal, SessionError};
use serde::Serialize;
use tauri::Runtime;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct HostFailure {
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
pub(crate) struct FailureContext {
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
pub(crate) struct FailureCause {
    code: &'static str,
    message: String,
    cause: Option<Box<FailureCause>>,
}

pub(super) fn failure_description(
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

pub(super) fn save_error_message(error: SessionError) -> String {
    if matches!(&error, SessionError::InvalidSave(fault) if fault.code == engine::error::SAVE_TARGET_DAY_NOT_READY)
    {
        let (code, message) = failure_description(&error);
        format!("{code}: {message}")
    } else {
        error.to_string()
    }
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
            fault: None,
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
        Self::civil_ref(&error)
    }

    pub(super) fn civil_ref(error: &SessionError) -> Self {
        match error {
            SessionError::Step(fatal) => Self::step(fatal.clone()),
            other => Self {
                code: "CIVIL_DAY_SETTLEMENT_FAILED",
                message: failure_description(other).1.to_owned(),
                r#where: "desktop.actor.civil_day".into(),
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

    pub(super) fn archive(error: &str) -> Self {
        Self {
            code: "DAY_END_ARCHIVE_FAILED",
            message: format!("日终已成功完成，但 SQLite 保存失败，上一份有效日终档仍保留：{error}"),
            r#where: "desktop.actor.save_day_end".into(),
            cause: None,
            context: FailureContext::new("saveDayEnd"),
            recoverable: true,
            recovery_actions: vec![
                "检查 SQLite 文件权限与磁盘空间后恢复推进，在下一日终重试保存",
                "复制存档错误详情反馈",
            ],
        }
    }

    pub(super) fn with_archive_key(&mut self, key: engine::session::checkpoint::CheckpointKey) {
        self.context.tick = Some(key.tick);
        self.context.seq = Some(key.seq);
        self.context.day = Some(key.day);
        self.context.generation = Some(key.generation.to_string());
    }

    pub(super) fn correction_prepare_rejected(error: &SessionError) -> Self {
        let mut failure = Self::civil_ref(error);
        failure.code = "REPORT_CORRECTION_PREPARE_REJECTED";
        failure.r#where = "desktop.actor.prepare_civil_updates".into();
        failure.recoverable = true;
        failure.recovery_actions = vec![
            "取消或修正待处理更正后恢复推进并重试日终",
            "复制脱敏错误详情反馈",
        ];
        failure
    }
}

#[derive(Clone, Serialize)]
pub(super) struct EngineFailurePayload {
    session_id: String,
    timeline_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) publication_ordinal: Option<String>,
    #[serde(flatten)]
    failure: HostFailure,
    events: [engine::Event; 0],
}

impl EngineFailurePayload {
    pub(super) fn note_missing_publications(&mut self, from: u64, through: u64) {
        use std::fmt::Write;
        write!(
            self.failure.message,
            "；观察者未收到已提交的发布序号 {from}–{through}"
        )
        .expect("写 String 不会失败");
    }

    #[cfg(test)]
    pub(super) fn terminal_fixture() -> Self {
        let mut failure = HostFailure::archive("terminal fixture");
        failure.code = "TEST_TERMINAL";
        failure.recoverable = false;
        Self {
            session_id: "test".into(),
            timeline_id: "test".into(),
            publication_ordinal: None,
            failure,
            events: [],
        }
    }
}

impl<R: Runtime> SessionActor<R> {
    pub(super) fn stop_after_step_failure(&mut self, error: StepFatal) {
        self.stop_after_host_failure(HostFailure::step(error));
    }

    pub(super) fn stop_after_host_failure(&mut self, mut failure: HostFailure) {
        engine::session::runner::apply_authority_failure(
            &self.game,
            failure.recoverable,
            &mut failure.message,
        );
        failure.context.tick.get_or_insert_with(|| self.game.tick());
        failure.context.seq.get_or_insert_with(|| self.game.seq());
        failure.context.day.get_or_insert_with(|| self.game.day());
        failure
            .context
            .generation
            .get_or_insert_with(|| self.generation.to_string());
        self.pacing.stop_after_failure(libos::native_async::now());
        let recoverable = failure.recoverable;
        let payload = EngineFailurePayload {
            session_id: self.session_id.clone(),
            timeline_id: self.timeline_id.clone(),
            publication_ordinal: None,
            failure,
            events: [],
        };
        if recoverable {
            self.delivery.stage(Packet::Failure(payload));
        } else {
            self.delivery.stage_terminal(payload);
            self.cmd_rx.close();
        }
    }
}
