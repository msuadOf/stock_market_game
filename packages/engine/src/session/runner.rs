//! 市场会话推进的共同规则。宿主只搬运命令、等候唤醒，并把已冻结结果交给独立的发布任务。
use super::{
    protocol::{
        CivilDayEndFailure, CivilRecovery, CivilUpdate, PausePreferences, ProtocolSession,
        TickBatch, TickFrame,
    },
    SessionError, StepFatal,
};
use libos::clock::Instant;
use std::time::Duration;
pub mod manual;
mod pacing;
pub use pacing::{
    validate_speed, Pacing, RequestedSpeed, SpeedError, SpeedMetrics, BASE_TICK_MS,
    MAX_SPEED_MULTIPLIER,
};

pub const MAX_BATCH_BUDGET: Duration = Duration::from_millis(14);
pub const MAX_BATCH_TICKS: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Fixed(Duration),
    Fastest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakePlan {
    Idle,
    At(Instant),
    Immediate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidPeriod;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permit {
    limit: usize,
}

impl Permit {
    pub const fn single_step() -> Self {
        Self { limit: 1 }
    }

    /// 用于宿主注入测试或明确受控的兼容入口；0 只推进待处理的日界。
    #[doc(hidden)]
    pub const fn for_explicit_limit(limit: usize) -> Self {
        assert!(limit <= MAX_BATCH_TICKS, "验证许可不得超过生产批次上限");
        Self { limit }
    }

    pub const fn limit(self) -> usize {
        self.limit
    }
}

pub struct Cadence {
    running: bool,
    speed: Speed,
    fixed_period: Duration,
    deadline: Option<Instant>,
}

impl Cadence {
    pub fn new(period: Duration) -> Self {
        assert!(!period.is_zero(), "市场调度的固定周期必须为正");
        Self {
            running: false,
            speed: Speed::Fixed(period),
            fixed_period: period,
            deadline: None,
        }
    }

    pub fn set_speed(&mut self, speed: Speed, now: Instant) -> Result<(), InvalidPeriod> {
        if let Speed::Fixed(period) = speed {
            if period.is_zero() {
                return Err(InvalidPeriod);
            }
            self.fixed_period = period;
        }
        self.speed = speed;
        self.reanchor(now);
        Ok(())
    }

    pub fn set_running(&mut self, running: bool, now: Instant) {
        if self.running != running {
            self.running = running;
            self.reanchor(now);
        }
    }

    pub const fn is_running(&self) -> bool {
        self.running
    }

    pub const fn is_fastest(&self) -> bool {
        matches!(self.speed, Speed::Fastest)
    }

    pub const fn period(&self) -> Duration {
        self.fixed_period
    }

    pub fn reanchor(&mut self, now: Instant) {
        self.deadline = match (self.running, self.speed) {
            (true, Speed::Fixed(period)) => Some(add_period(now, period)),
            _ => None,
        };
    }

    pub fn wake_plan(&self) -> WakePlan {
        match (self.running, self.speed) {
            (false, _) => WakePlan::Idle,
            (true, Speed::Fastest) => WakePlan::Immediate,
            (true, Speed::Fixed(_)) => {
                WakePlan::At(self.deadline.expect("运行中的固定节拍缺少 deadline"))
            }
        }
    }

    pub fn begin_turn(&mut self, now: Instant) -> Option<Permit> {
        match self.wake_plan() {
            WakePlan::Idle => None,
            WakePlan::Immediate => Some(Permit {
                limit: MAX_BATCH_TICKS,
            }),
            WakePlan::At(due) => {
                let late = now.checked_duration_since(due)?;
                let period = self.fixed_period;
                // 跳过已错过的时隙，不追补；保留原绝对节拍避免累积漂移。
                let remainder = late.as_nanos() % period.as_nanos();
                let elapsed_slot = Duration::new(
                    (remainder / 1_000_000_000) as u64,
                    (remainder % 1_000_000_000) as u32,
                );
                self.deadline = Some(add_period(now, period - elapsed_slot));
                Some(Permit::single_step())
            }
        }
    }
}

fn add_period(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).expect("市场调度 deadline 溢出")
}

/// 仅供既有宿主断言测试的便携周期；生产命令经有错误返回的 validate_speed。
#[doc(hidden)]
pub fn fixed_tick_duration(base_ms: u64, multiplier: f64) -> Duration {
    match validate_speed(base_ms, multiplier, Instant::now()) {
        Ok(Speed::Fixed(period)) => period,
        _ => panic!("测试必须提供有效的固定周期"),
    }
}

/// 已经提交但没有交接的成功前缀。适配器只能报告这个范围，不能重放交易。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommittedRange {
    pub tick_from: u64,
    pub tick_to: u64,
    pub seq_from: u64,
    pub seq_to: u64,
}

impl CommittedRange {
    fn from_frames(frames: &[TickFrame]) -> Option<Self> {
        frames.first().zip(frames.last()).map(|(first, last)| Self {
            tick_from: first.tick,
            tick_to: last.tick,
            seq_from: first.seq_from,
            seq_to: last.seq_to,
        })
    }

    pub fn describe(self, message: &mut String) {
        use std::fmt::Write;
        write!(
            message,
            "；本批已提交但尚未交付的成功前缀：tick_from={}, tick_to={}, seq_from={}, seq_to={}",
            self.tick_from, self.tick_to, self.seq_from, self.seq_to
        )
        .expect("写 String 不会失败");
    }
}

#[derive(Debug)]
pub enum CycleError<E> {
    Advance {
        source: SessionError,
        unpublished: Option<CommittedRange>,
    },
    Batch {
        source: StepFatal,
        unpublished: CommittedRange,
    },
    Civil {
        source: CivilDayEndFailure,
        retryable: bool,
    },
    Stage(E),
}

/// 恢复资格归权威会话所有，不因某个观察者或控制连接断开而改变。
pub fn civil_is_retryable(session: &ProtocolSession, failure: &CivilDayEndFailure) -> bool {
    failure.recovery() == CivilRecovery::CorrectionPrepareRejected
        && session.game().require_healthy().is_ok()
        && session.civil_day_ready().is_ok_and(|ready| ready)
}

/// 首错为不可恢复时，所有宿主停止继续受理；观察和传输故障不得调用此入口。
pub fn apply_authority_failure(session: &ProtocolSession, retryable: bool, message: &mut String) {
    if !retryable {
        if let Err(error) = session.shared_ingress().close() {
            use std::fmt::Write;
            write!(message, "；关闭 ingress 失败：{error}").expect("写 String 不会失败");
        }
    }
}

/// 自动推进、日界暂停与错误停止均遵守同一采样合同。
pub fn apply_cycle_pacing<E>(
    pacing: &mut Pacing,
    tick: u64,
    now: Instant,
    result: Result<CycleOutcome, CycleError<E>>,
) -> Result<(), CycleError<E>> {
    match result {
        Ok(outcome) => {
            if outcome.paused {
                pacing.pause_at_civil_boundary(tick, now);
            }
            pacing.refresh_metrics(tick, now);
            Ok(())
        }
        Err(failure) => {
            pacing.stop_after_failure(now);
            Err(failure)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CycleOutcome {
    pub paused: bool,
}

/// 只提供局部操作与无阻塞交接；这里的交接不能同步写盘、编码、等待浏览器或网络。
pub trait DrivePort {
    type Error;

    fn session(&self) -> &ProtocolSession;
    fn session_mut(&mut self) -> &mut ProtocolSession;
    fn command_pending(&self) -> bool;
    fn stage_tick_batch(&mut self, batch: TickBatch) -> Result<(), Self::Error>;
    fn stage_civil(&mut self, update: CivilUpdate, barrier_paused: bool)
        -> Result<(), Self::Error>;

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        self.session_mut().step_frame()
    }

    /// 用于已有宿主故障注入，生产实现不得覆盖。
    fn enforce_budget(&self) -> bool {
        true
    }
}

/// 在权威会话上冻结完整批次后交接，而不是由异步消费者重读或修改会话。
pub fn run_cycle<P: DrivePort>(
    port: &mut P,
    permit: Permit,
    pause: PausePreferences,
) -> Result<CycleOutcome, CycleError<P::Error>> {
    port.session()
        .game()
        .require_healthy()
        .map_err(|error| CycleError::Advance {
            source: error.into(),
            unpublished: None,
        })?;
    let started = port.now();
    let mut frames = Vec::new();
    for _ in 0..permit.limit {
        if port.command_pending() {
            break;
        }
        match port.session().civil_day_ready() {
            Ok(true) => break,
            Ok(false) => (),
            Err(source) => return Err(fault_with_prefix(port, source, frames)),
        }
        match port.step_frame() {
            Ok(frame) => frames.push(frame),
            Err(error) => return Err(fault_with_prefix(port, error.into(), frames)),
        }
        // 最少提交一个不可分割的 tick，然后才比较时间预算。
        if port.enforce_budget()
            && port.now().saturating_duration_since(started) >= MAX_BATCH_BUDGET
        {
            break;
        }
    }
    if !frames.is_empty() {
        let unpublished = CommittedRange::from_frames(&frames).expect("批次至少包含一个 tick");
        let batch = port
            .session()
            .tick_batch(frames)
            .map_err(|source| CycleError::Batch {
                source,
                unpublished,
            })?;
        port.stage_tick_batch(batch).map_err(CycleError::Stage)?;
    }

    while !port.command_pending() {
        match port.session().civil_day_ready() {
            Ok(false) => break,
            Ok(true) => (),
            Err(error) => {
                return Err(CycleError::Civil {
                    source: error.into(),
                    retryable: false,
                })
            }
        }
        let (update, paused) = match manual::commit_civil(port.session_mut(), pause) {
            Ok(outcome) => outcome,
            Err(source) => {
                let retryable = civil_is_retryable(port.session(), &source);
                return Err(CycleError::Civil { source, retryable });
            }
        };
        // 每个静点的稳定候选必须在继续下一自然日之前由 port 捕获并入队。
        port.stage_civil(update, paused)
            .map_err(CycleError::Stage)?;
        if paused {
            return Ok(CycleOutcome { paused: true });
        }
    }
    Ok(CycleOutcome::default())
}

fn fault_with_prefix<P: DrivePort>(
    port: &mut P,
    source: SessionError,
    frames: Vec<TickFrame>,
) -> CycleError<P::Error> {
    let mut unpublished = CommittedRange::from_frames(&frames);
    // 封装/暂存失败仍保留原首错，适配器不会用已失效的会话重试封装。
    if unpublished.is_some() {
        if let Ok(batch) = port.session().freeze_committed_frames(frames) {
            if port.stage_tick_batch(batch).is_ok() {
                unpublished = None;
            }
        }
    }
    CycleError::Advance {
        source,
        unpublished,
    }
}

#[cfg(test)]
mod tests;
