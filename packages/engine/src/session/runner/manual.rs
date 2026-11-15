//! 手工操作共享提交、日界顺序与暂停决策；宿主只负责把已冻结结果交接给发布任务。

use super::{
    CivilDayEndFailure, CivilUpdate, PausePreferences, ProtocolSession, SessionError, StepFatal,
    TickBatch,
};
use crate::error::ValidationFault;
use crate::session::protocol::EngineUpdate;

#[derive(Debug, thiserror::Error)]
pub enum ManualStepError {
    #[error(transparent)]
    Readiness(SessionError),
    #[error("运行中不能执行受控单步；请先暂停")]
    MustBePaused,
    #[error("请先提交当前自然日的日界，再执行单步")]
    BarrierPending,
    #[error(transparent)]
    Step(StepFatal),
}

impl ManualStepError {
    pub fn into_session_error(self) -> SessionError {
        match self {
            Self::Readiness(error) => error,
            // 1295：受控单步仅在暂停时受理。
            Self::MustBePaused => SessionError::InvalidSave(ValidationFault::bare(1295)),
            Self::BarrierPending => SessionError::InvalidSave(ValidationFault::bare(1294)),
            Self::Step(error) => error.into(),
        }
    }
}

#[derive(Debug)]
pub enum ManualNextError {
    Step(ManualStepError),
    Civil(CivilDayEndFailure),
}

/// 单步只提交一拍；日界由独立的命令按序提交，不能由单步隐式跨越。
pub fn manual_tick(
    session: &mut ProtocolSession,
    running: bool,
) -> Result<TickBatch, ManualStepError> {
    session
        .game()
        .require_healthy()
        .map_err(ManualStepError::Step)?;
    if running {
        return Err(ManualStepError::MustBePaused);
    }
    if session
        .civil_day_ready()
        .map_err(ManualStepError::Readiness)?
    {
        return Err(ManualStepError::BarrierPending);
    }
    freeze_one_tick(session)
}

#[derive(Debug)]
pub enum ManualCivilError {
    Rejected(SessionError),
    Retryable(SessionError),
    Fatal(SessionError),
}

impl ManualCivilError {
    pub fn into_session_error(self) -> SessionError {
        match self {
            Self::Rejected(error) | Self::Retryable(error) | Self::Fatal(error) => error,
        }
    }
}

/// 显式命令只结算当前已结束的自然日；未到边界属于请求拒绝，不是权威故障。
pub fn manual_civil(
    session: &mut ProtocolSession,
    pause: PausePreferences,
) -> Result<(CivilUpdate, bool), ManualCivilError> {
    session
        .game()
        .require_healthy()
        .map_err(|error| ManualCivilError::Fatal(error.into()))?;
    if !session.civil_day_ready().map_err(ManualCivilError::Fatal)? {
        // 3009：当前自然日尚未覆盖完整的收盘会话。
        return Err(ManualCivilError::Rejected(SessionError::InvalidSave(
            ValidationFault::bare(3009),
        )));
    }
    commit_civil(session, pause).map_err(|source| {
        let retryable = super::civil_is_retryable(session, &source);
        let error = source.into_error();
        if retryable {
            ManualCivilError::Retryable(error)
        } else {
            ManualCivilError::Fatal(error)
        }
    })
}

/// 自动与手工由同一函数提交日界并决定是否暂停。
pub(super) fn commit_civil(
    session: &mut ProtocolSession,
    pause: PausePreferences,
) -> Result<(CivilUpdate, bool), CivilDayEndFailure> {
    let update = session.end_civil_day_update_classified()?;
    let paused = pause.pauses(&update);
    Ok((update, paused))
}

/// 兼容旧逐条提取 API：只交付一个事实单元，待提交日界优先于下一拍。
pub fn manual_next(session: &mut ProtocolSession) -> Result<EngineUpdate, ManualNextError> {
    if session
        .civil_day_ready()
        .map_err(|error| ManualNextError::Step(ManualStepError::Readiness(error)))?
    {
        let (update, _) =
            commit_civil(session, PausePreferences::default()).map_err(ManualNextError::Civil)?;
        return Ok(EngineUpdate::CivilUpdate(Box::new(update)));
    }
    freeze_one_tick(session)
        .map(|batch| EngineUpdate::TickBatch(Box::new(batch)))
        .map_err(ManualNextError::Step)
}

fn freeze_one_tick(session: &mut ProtocolSession) -> Result<TickBatch, ManualStepError> {
    let frame = session.step_frame().map_err(ManualStepError::Step)?;
    session
        .tick_batch(vec![frame])
        .map_err(ManualStepError::Step)
}
