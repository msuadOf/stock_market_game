//! 本次 civil 操作的恢复资格随结果 move；原始领域错误本身不携带可重放资格。
use crate::session::{SessionError, StepFatal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CivilRecovery {
    None,
    CorrectionPrepareRejected,
}

/// 只由 engine 构造本次失败的来源；不能 Clone、反序列化或经 callback 带回旧资格。
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct CivilDayEndFailure {
    #[source]
    error: SessionError,
    recovery: CivilRecovery,
}

impl CivilDayEndFailure {
    /// 仅真实 prepare 的局部来源且当前 authority 健康时调用；callback 只能返回 SessionError。
    pub(in crate::session) fn correction_prepare_rejected(error: SessionError) -> Self {
        Self {
            error,
            recovery: CivilRecovery::CorrectionPrepareRejected,
        }
    }

    pub fn error(&self) -> &SessionError {
        &self.error
    }
    pub fn into_error(self) -> SessionError {
        self.error
    }
    pub const fn recovery(&self) -> CivilRecovery {
        self.recovery
    }
}

impl From<SessionError> for CivilDayEndFailure {
    fn from(error: SessionError) -> Self {
        Self {
            error,
            recovery: CivilRecovery::None,
        }
    }
}

impl From<StepFatal> for CivilDayEndFailure {
    fn from(error: StepFatal) -> Self {
        SessionError::Step(error).into()
    }
}
