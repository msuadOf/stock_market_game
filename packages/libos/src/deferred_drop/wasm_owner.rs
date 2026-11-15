//! 共享 Memory 的 engine Worker 独占析构队列 owner；同 Memory 换代不得销毁它。

use super::{DeferredDropDegradation, Submission, backend};
use crate::thread_facade::{JoinFailure, WorkerHandle};
use std::{cell::RefCell, fmt, io};

enum State {
    Missing,
    Running(backend::RunningBackend, WorkerHandle<()>),
    Failed(Option<i32>),
    Closed,
}

thread_local! {
    static OWNER: RefCell<State> = const { RefCell::new(State::Missing) };
}

#[derive(Debug)]
pub enum StartError {
    AlreadyStarted,
    AlreadyClosed,
    SpawnFailed(io::Error),
    PriorSpawnFailure,
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyStarted => f.write_str("析构 Worker 已启动"),
            Self::AlreadyClosed => f.write_str("析构 Worker 已关闭"),
            Self::SpawnFailed(error) => write!(f, "析构 Worker 启动失败：{error}"),
            Self::PriorSpawnFailure => f.write_str("已有析构 Worker 启动失败，不重试"),
        }
    }
}

#[derive(Debug)]
pub enum CloseError {
    AlreadyClosed,
    Join(JoinFailure),
}

impl fmt::Display for CloseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyClosed => f.write_str("析构 Worker 关闭已消费"),
            Self::Join(error) => write!(f, "析构 Worker join 失败：{error}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectReason {
    NotStarted,
    StartFailed,
    Closed,
}

/// 只应由配置完成且能获得真实 Worker 启动回执的 engine owner 调用。
pub fn start_wasm_reclaimer() -> Result<(), StartError> {
    OWNER.with(|owner| {
        let mut state = owner.borrow_mut();
        match &*state {
            State::Running(..) => return Err(StartError::AlreadyStarted),
            State::Failed(_) => return Err(StartError::PriorSpawnFailure),
            State::Closed => return Err(StartError::AlreadyClosed),
            State::Missing => {}
        }
        match backend::spawn_consumer() {
            Ok((running, handle)) => {
                *state = State::Running(running, handle);
                Ok(())
            }
            Err(error) => {
                *state = State::Failed(error.raw_os_error());
                Err(StartError::SpawnFailed(error))
            }
        }
    })
}

/// 先禁止提交、断开唯一 Sender，再由真实消费者 FIFO 排空。true 只证明 Rust
/// 已 drain/join；宿主必须独立观察实际 childExit 和槽回收后才能发布退休成功。
pub fn close_wasm_reclaimer() -> Result<bool, CloseError> {
    close_with_probe(|| {})
}

fn close_with_probe(after_disabling_submit: impl FnOnce()) -> Result<bool, CloseError> {
    let running = OWNER.with(|owner| {
        let mut state = owner.borrow_mut();
        match &*state {
            State::Missing | State::Failed(_) => Ok(None),
            State::Closed => Err(CloseError::AlreadyClosed),
            State::Running(..) => match std::mem::replace(&mut *state, State::Closed) {
                State::Running(running, handle) => Ok(Some((running, handle))),
                _ => unreachable!("owner 已在同一 TLS 借用中确认 Running"),
            },
        }
    })?;
    match running {
        Some((running, handle)) => {
            drop(running);
            after_disabling_submit();
            handle.join().map_err(CloseError::Join)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(feature = "verification-harness")]
pub fn verify_close_wasm_reclaimer(after_disabling_submit: impl FnOnce()) -> Result<bool, CloseError> {
    close_with_probe(after_disabling_submit)
}

#[inline]
fn with_running<T, J>(
    value: T,
    into_job: J,
) -> Result<Submission, (RejectReason, T)>
where
    J: FnOnce(T) -> backend::Job,
{
    OWNER.with(|owner| match &*owner.borrow() {
        State::Running(running, _) => Ok(backend::submit(running, into_job(value))),
        State::Missing => Err((RejectReason::NotStarted, value)),
        State::Failed(_) => Err((RejectReason::StartFailed, value)),
        State::Closed => Err((RejectReason::Closed, value)),
    })
}

#[inline]
pub fn try_defer<T: Send + 'static>(value: T) -> Result<Submission, (RejectReason, T)> {
    with_running(value, |value| Box::new(move || drop(value)))
}

#[inline]
pub fn defer<T: Send + 'static>(value: T) -> Submission {
    match try_defer(value) {
        Ok(outcome) => outcome,
        Err((reason, _value)) => panic!("析构 owner 未处于提交阶段：{reason:?}"),
    }
}

#[cfg(feature = "verification-harness")]
pub fn defer_job(job: backend::Job) -> Submission {
    match with_running(job, |job| job) {
        Ok(outcome) => outcome,
        Err((reason, _job)) => panic!("析构验证：owner 未处于提交阶段：{reason:?}"),
    }
}

pub fn degradation() -> Option<DeferredDropDegradation> {
    OWNER.with(|owner| match &*owner.borrow() {
        State::Running(running, _) => backend::running_degradation(running),
        State::Failed(os_errno) => Some(DeferredDropDegradation::SpawnFailed { os_errno: *os_errno }),
        State::Missing | State::Closed => None,
    })
}


#[cfg(feature = "verification-harness")]
pub fn with_verification_sender<R>(
    observe: impl FnOnce(Option<&std::sync::mpsc::SyncSender<backend::Job>>) -> R,
) -> R {
    OWNER.with(|owner| match &*owner.borrow() {
        State::Running(running, _) => observe(Some(&running.sender)),
        _ => observe(None),
    })
}
