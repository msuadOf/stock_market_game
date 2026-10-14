//! 按需记录单个已成功提交权威 tick 的 wall-clock 证据。
//!
//! recorder 是临时 thread-local 状态，不属于 GameSession，不参与存档或 hash。
//! 只有 CommitTick 成功后才构造公共证据。runnable thread 字段记录 Rayon registry
//! 配置的 worker 数量，不能当作实际 runnable/active worker、操作系统负载或 CPU 利用率。
//! DecisionAndCoordinatorWork 包含决策、计划反馈，以及并发 plan-root 生产与
//! AccountValidation/StockProcessing coordinator 初始化共享的 wall time。
//! AccountValidation/StockProcessing 的独占计时包含输入准备与后续请求工作，
//! 但不重复包含并发 coordinator 初始化；AccountValidation 还计入消费 validator 与移交输出。
//! 无请求时，validator 收尾是唯一 AccountValidation span，并不表示已校验订单。
//! 重叠分支时长不得加到这些独占阶段记录上。

use crate::{session::pipeline::TickPhase, Event, GameSession};
use serde::{Serialize, Serializer};
use std::{cell::RefCell, fmt::Display, time::Instant};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseTimingPhase {
    ExpiryShadow,
    SealAllocationSnapshot,
    DecisionAndCoordinatorWork,
    AccountValidation,
    StockProcessing,
    ReceiptAggregation,
    SettlementShadow,
    DerivationAudit,
    PreCommitValidation,
    CommitTick,
}

impl PhaseTimingPhase {
    pub const ALL: [Self; 10] = [
        Self::ExpiryShadow,
        Self::SealAllocationSnapshot,
        Self::DecisionAndCoordinatorWork,
        Self::AccountValidation,
        Self::StockProcessing,
        Self::ReceiptAggregation,
        Self::SettlementShadow,
        Self::DerivationAudit,
        Self::PreCommitValidation,
        Self::CommitTick,
    ];

    const fn metadata(self) -> (u8, &'static str) {
        match self {
            Self::ExpiryShadow => (0, "expiry_shadow"),
            Self::SealAllocationSnapshot => (1, "seal_allocation_snapshot"),
            Self::DecisionAndCoordinatorWork => (2, "decision_and_coordinator_work"),
            Self::AccountValidation => (3, "account_validation"),
            Self::StockProcessing => (4, "stock_processing"),
            Self::ReceiptAggregation => (5, "receipt_aggregation"),
            Self::SettlementShadow => (6, "settlement_shadow"),
            Self::DerivationAudit => (7, "derivation_audit"),
            Self::PreCommitValidation => (8, "pre_commit_validation"),
            Self::CommitTick => (9, "commit_tick"),
        }
    }

    pub const fn rank(self) -> u8 {
        self.metadata().0
    }

    pub const fn name(self) -> &'static str {
        self.metadata().1
    }

    const fn index(self) -> usize {
        self.rank() as usize
    }
}

impl From<TickPhase> for PhaseTimingPhase {
    fn from(phase: TickPhase) -> Self {
        match phase {
            TickPhase::ExpiryShadow => Self::ExpiryShadow,
            TickPhase::SealAllocationSnapshot => Self::SealAllocationSnapshot,
            TickPhase::DecisionAndContinuation => Self::DecisionAndCoordinatorWork,
            TickPhase::AccountValidation => Self::AccountValidation,
            TickPhase::StockProcessing => Self::StockProcessing,
            TickPhase::ReceiptAggregation => Self::ReceiptAggregation,
            TickPhase::SettlementShadow => Self::SettlementShadow,
            TickPhase::DerivationAudit => Self::DerivationAudit,
            TickPhase::PreCommitValidation => Self::PreCommitValidation,
            TickPhase::CommitTick => Self::CommitTick,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunnableThreadSample {
    #[serde(serialize_with = "serialize_decimal")]
    sample_count: u64,
    #[serde(serialize_with = "serialize_decimal")]
    minimum: usize,
    #[serde(serialize_with = "serialize_decimal")]
    maximum: usize,
}

impl RunnableThreadSample {
    pub const fn sample_count(&self) -> u64 {
        self.sample_count
    }

    pub const fn minimum(&self) -> usize {
        self.minimum
    }

    pub const fn maximum(&self) -> usize {
        self.maximum
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PhaseTimingRecord {
    phase: PhaseTimingPhase,
    #[serde(serialize_with = "serialize_decimal")]
    wall_time_ns: u128,
    #[serde(serialize_with = "serialize_decimal")]
    span_count: u64,
    runnable_threads: RunnableThreadSample,
}

impl PhaseTimingRecord {
    pub const fn phase(&self) -> PhaseTimingPhase {
        self.phase
    }

    pub const fn wall_time_ns(&self) -> u128 {
        self.wall_time_ns
    }

    pub const fn span_count(&self) -> u64 {
        self.span_count
    }

    pub const fn runnable_thread_sample(&self) -> &RunnableThreadSample {
        &self.runnable_threads
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommittedPhaseTiming {
    schema: &'static str,
    schema_version: u32,
    #[serde(serialize_with = "serialize_decimal")]
    tick_before: u64,
    #[serde(serialize_with = "serialize_decimal")]
    tick_after: u64,
    records: Vec<PhaseTimingRecord>,
}

impl CommittedPhaseTiming {
    pub const SCHEMA: &'static str = "escrow-committed-phase-timing";

    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    pub const fn tick_before(&self) -> u64 {
        self.tick_before
    }

    pub const fn tick_after(&self) -> u64 {
        self.tick_after
    }

    pub fn records(&self) -> &[PhaseTimingRecord] {
        &self.records
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimedStep {
    events: Vec<Event>,
    timing: CommittedPhaseTiming,
}

impl TimedStep {
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub const fn timing(&self) -> &CommittedPhaseTiming {
        &self.timing
    }

    pub fn into_parts(self) -> (Vec<Event>, CommittedPhaseTiming) {
        (self.events, self.timing)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PhaseTimingCaptureError {
    #[error("authoritative step failed before committed phase timing existed: {0}")]
    Step(#[source] crate::session::StepFatal),
    #[error("a phase-timing capture is already active on this thread")]
    CaptureAlreadyActive,
    #[error("the operation completed without one committed authoritative tick")]
    NoCommittedTick,
    #[error("phase timing 观察绑定仍被在途计算任务持有；完成信号必须在绑定释放后发出")]
    CaptureStillBound,
    #[error("phase-timing capture is incomplete at {phase}")]
    IncompletePhase { phase: &'static str },
    #[error("committed tick advanced from {before} to {after}, expected exactly one tick")]
    UnexpectedTickAdvance { before: u64, after: u64 },
    #[error("phase-timing counter overflow at {field}")]
    CounterOverflow { field: &'static str },
}

impl GameSession {
    /// 在普通公共 GameSession::step 路径上启用临时计时。
    ///
    /// step 失败时只返回 typed fatal，丢弃部分采样。
    /// 成功证据仅在 CommitTick 已将 candidate 安装到会话后返回。
    pub fn step_with_phase_timing(&mut self) -> Result<TimedStep, PhaseTimingCaptureError> {
        let (events, timing) = capture_one_committed_tick(|| self.step())?;
        Ok(TimedStep { events, timing })
    }
}

#[derive(Clone, Copy, Debug)]
struct Accumulator {
    wall_time_ns: u128,
    span_count: u64,
    sample_count: u64,
    runnable_minimum: usize,
    runnable_maximum: usize,
}

impl Accumulator {
    const EMPTY: Self = Self {
        wall_time_ns: 0,
        span_count: 0,
        sample_count: 0,
        runnable_minimum: usize::MAX,
        runnable_maximum: 0,
    };

    fn sample(&mut self, runnable: usize) -> Result<(), &'static str> {
        self.sample_count = self
            .sample_count
            .checked_add(1)
            .ok_or("runnable_thread_sample_count")?;
        self.runnable_minimum = self.runnable_minimum.min(runnable);
        self.runnable_maximum = self.runnable_maximum.max(runnable);
        Ok(())
    }
}

struct PhaseTimingLedger {
    accumulators: [Accumulator; PhaseTimingPhase::ALL.len()],
}

impl PhaseTimingLedger {
    fn new() -> Self {
        Self {
            accumulators: [Accumulator::EMPTY; PhaseTimingPhase::ALL.len()],
        }
    }

    fn accumulator(&self, phase: PhaseTimingPhase) -> &Accumulator {
        &self.accumulators[phase.index()]
    }

    fn accumulator_mut(&mut self, phase: PhaseTimingPhase) -> &mut Accumulator {
        &mut self.accumulators[phase.index()]
    }

    fn record_span(
        &mut self,
        phase: PhaseTimingPhase,
        elapsed: u128,
        workers: usize,
    ) -> Option<&'static str> {
        let mut overflow = None;
        let accumulator = self.accumulator_mut(phase);
        accumulator.wall_time_ns = match accumulator.wall_time_ns.checked_add(elapsed) {
            Some(total) => total,
            None => {
                overflow = Some("wall_time_ns");
                accumulator.wall_time_ns
            }
        };
        accumulator.span_count = match accumulator.span_count.checked_add(1) {
            Some(total) => total,
            None => {
                overflow = Some("span_count");
                accumulator.span_count
            }
        };
        if let Err(field) = accumulator.sample(workers) {
            overflow = Some(field);
        }
        overflow
    }

    fn sample(&mut self, phase: PhaseTimingPhase, runnable: usize) -> Result<(), &'static str> {
        self.accumulator_mut(phase).sample(runnable)
    }

    fn missing_precommit_phase(&self) -> Option<PhaseTimingPhase> {
        PhaseTimingPhase::ALL
            .into_iter()
            .take(9)
            .find(|phase| self.accumulator(*phase).span_count == 0)
    }

    fn records(&self) -> Result<Vec<PhaseTimingRecord>, PhaseTimingCaptureError> {
        let mut records = Vec::with_capacity(PhaseTimingPhase::ALL.len());
        for phase in PhaseTimingPhase::ALL {
            let accumulator = *self.accumulator(phase);
            if accumulator.span_count == 0 || accumulator.sample_count == 0 {
                return Err(PhaseTimingCaptureError::IncompletePhase {
                    phase: phase.name(),
                });
            }
            records.push(PhaseTimingRecord {
                phase,
                wall_time_ns: accumulator.wall_time_ns,
                span_count: accumulator.span_count,
                runnable_threads: RunnableThreadSample {
                    sample_count: accumulator.sample_count,
                    minimum: accumulator.runnable_minimum,
                    maximum: accumulator.runnable_maximum,
                },
            });
        }
        Ok(records)
    }
}

struct ActiveSpan {
    phase: PhaseTimingPhase,
    started: Instant,
}

struct Collector {
    context_workers: Option<usize>,
    tick_before: Option<u64>,
    tick_after: Option<u64>,
    current: Option<ActiveSpan>,
    ledger: PhaseTimingLedger,
    overflow: Option<&'static str>,
}

impl Collector {
    fn new() -> Self {
        Self {
            context_workers: None,
            tick_before: None,
            tick_after: None,
            current: None,
            ledger: PhaseTimingLedger::new(),
            overflow: None,
        }
    }

    fn close_current(&mut self) {
        let Some(span) = self.current.take() else {
            return;
        };
        let elapsed = span.started.elapsed().as_nanos();
        if let Some(field) = self.ledger.record_span(
            span.phase,
            elapsed,
            self.context_workers
                .expect("计时开始前必须绑定真实会话上下文"),
        ) {
            self.overflow = Some(field);
        }
    }

    fn enter(&mut self, phase: PhaseTimingPhase) {
        self.close_current();
        if let Err(field) = self.ledger.sample(
            phase,
            self.context_workers
                .expect("计时开始前必须绑定真实会话上下文"),
        ) {
            self.overflow = Some(field);
        }
        self.current = Some(ActiveSpan {
            phase,
            started: Instant::now(),
        });
    }

    fn finish(mut self) -> Result<CommittedPhaseTiming, PhaseTimingCaptureError> {
        self.close_current();
        if let Some(field) = self.overflow {
            return Err(PhaseTimingCaptureError::CounterOverflow { field });
        }
        let tick_before = self
            .tick_before
            .ok_or(PhaseTimingCaptureError::NoCommittedTick)?;
        let tick_after = self
            .tick_after
            .ok_or(PhaseTimingCaptureError::NoCommittedTick)?;
        if tick_before.checked_add(1) != Some(tick_after) {
            return Err(PhaseTimingCaptureError::UnexpectedTickAdvance {
                before: tick_before,
                after: tick_after,
            });
        }
        let records = self.ledger.records()?;
        Ok(CommittedPhaseTiming {
            schema: CommittedPhaseTiming::SCHEMA,
            schema_version: 2,
            tick_before,
            tick_after,
            records,
        })
    }
}

thread_local! {
    static COLLECTOR: RefCell<Option<std::sync::Arc<std::sync::Mutex<Collector>>>> = const { RefCell::new(None) };
}

/// 显式 compute burst 继承同一次捕获；只共享临时观察账本，不共享业务权威状态。
#[derive(Clone)]
pub(crate) struct PhaseTimingBinding(Option<std::sync::Arc<std::sync::Mutex<Collector>>>);

pub(crate) fn current_phase_timing_binding() -> PhaseTimingBinding {
    PhaseTimingBinding(COLLECTOR.with_borrow(Clone::clone))
}

pub(crate) fn with_phase_timing_binding<R>(
    binding: PhaseTimingBinding,
    work: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<std::sync::Arc<std::sync::Mutex<Collector>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COLLECTOR.with_borrow_mut(|slot| *slot = self.0.take());
        }
    }
    let previous = COLLECTOR.with_borrow_mut(|slot| std::mem::replace(slot, binding.0));
    let _restore = Restore(previous);
    work()
}

fn with_collector<R>(work: impl FnOnce(Option<&mut Collector>) -> R) -> R {
    COLLECTOR.with_borrow(|slot| {
        let mut collector = slot
            .as_ref()
            .map(|value| value.lock().expect("phase timing 观察账本锁被 poison"));
        work(collector.as_deref_mut())
    })
}

/// 本线程 TLS 是否仍安装着相位计时绑定（发布点直证辅助：发布现场必须已
/// 卸下=实际绑定已释放，非计数推断）。
#[cfg(any(test, feature = "verification-harness"))]
pub(crate) fn phase_timing_binding_installed_on_this_thread() -> bool {
    COLLECTOR.with_borrow(|slot| slot.is_some())
}

#[cfg(test)]
impl PhaseTimingBinding {
    /// 跨线程传播测试用：以全新观察账本构造可识别绑定（与生产捕获同构）。
    pub(crate) fn test_collector() -> Self {
        Self(Some(std::sync::Arc::new(std::sync::Mutex::new(
            Collector::new(),
        ))))
    }

    /// 跨线程传播测试用：空绑定（等价于从未进入捕获线程的 TLS）。
    pub(crate) fn empty() -> Self {
        Self(None)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// 两个绑定是否指向同一观察账本（跨线程身份比较）。
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(left), Some(right)) => std::sync::Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }

    /// 当前绑定持有的观察账本强引用数（空绑定为 0）。
    /// 用于断言完成通知严格晚于最后一个绑定克隆的释放。
    pub(crate) fn strong_count(&self) -> usize {
        self.0
            .as_ref()
            .map(|collector| std::sync::Arc::strong_count(collector))
            .unwrap_or(0)
    }
}

fn capture_one_committed_tick<T>(
    operation: impl FnOnce() -> Result<T, crate::session::StepFatal>,
) -> Result<(T, CommittedPhaseTiming), PhaseTimingCaptureError> {
    start_capture()?;
    let result = operation();
    match result {
        Ok(value) => {
            let collector = take_capture()?;
            collector.finish().map(|timing| (value, timing))
        }
        // 错误路径：原 StepFatal 必须原样上抛；在途任务仍持有的绑定随其自然结束释放，
        // 观察账本就地丢弃，绝不 panic 遮蔽原始错误。
        Err(fatal) => {
            discard_capture();
            Err(PhaseTimingCaptureError::Step(fatal))
        }
    }
}

fn start_capture() -> Result<(), PhaseTimingCaptureError> {
    COLLECTOR.with_borrow_mut(|slot| {
        if slot.is_some() {
            Err(PhaseTimingCaptureError::CaptureAlreadyActive)
        } else {
            *slot = Some(std::sync::Arc::new(std::sync::Mutex::new(Collector::new())));
            Ok(())
        }
    })
}

fn take_capture() -> Result<Collector, PhaseTimingCaptureError> {
    let collector = COLLECTOR
        .with_borrow_mut(|slot| slot.take().ok_or(PhaseTimingCaptureError::NoCommittedTick))?;
    // 协议保证：协调者可见的完成信号都在绑定释放之后发出（spawn 两段式），
    // 因此成功路径此处必然独占；若仍共享，说明有任务绕过协议，用类型化错误显式上抛。
    let collector = std::sync::Arc::try_unwrap(collector)
        .map_err(|_| PhaseTimingCaptureError::CaptureStillBound)?;
    Ok(collector
        .into_inner()
        .expect("phase timing 观察账本锁被 poison"))
}

/// 丢弃未完成的观察账本：错误路径不发布计时，也不因在途任务仍持有绑定而 panic。
fn discard_capture() {
    COLLECTOR.with_borrow_mut(|slot| *slot = None);
}

pub(crate) fn begin_authoritative_tick(
    tick_before: u64,
    compute: &crate::ComputeContext,
) -> Result<(), crate::session::StepFatal> {
    with_collector(|collector| {
        let Some(collector) = collector else {
            return Ok(());
        };
        // 读取实际指定 pool 的 registry 容量，不采样 caller 的另一个 pool。
        collector.context_workers = Some(compute.workers());
        if collector.tick_before.replace(tick_before).is_some() {
            return Err(invariant(
                "phase-timing capture observed more than one tick",
            ));
        }
        Ok(())
    })
}

pub(crate) fn enter_phase(phase: TickPhase) {
    with_collector(|collector| {
        if let Some(collector) = collector {
            collector.enter(phase.into());
        }
    });
}

pub(crate) fn validate_precommit() -> Result<(), crate::session::StepFatal> {
    with_collector(|collector| {
        let Some(collector) = collector else {
            return Ok(());
        };
        collector.close_current();
        if let Some(phase) = collector.ledger.missing_precommit_phase() {
            return Err(invariant(&format!(
                "phase timing 到达 CommitTick 时缺少 {}",
                phase.name()
            )));
        }
        if let Some(field) = collector.overflow {
            return Err(invariant(&format!(
                "phase-timing capture overflowed {field}"
            )));
        }
        Ok(())
    })
}

pub(crate) fn mark_committed(tick_after: u64) {
    with_collector(|collector| {
        if let Some(collector) = collector {
            collector.close_current();
            collector.tick_after = Some(tick_after);
        }
    });
}

fn invariant(description: &str) -> crate::session::StepFatal {
    crate::session::StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "verification_evidence::phase_timing".to_owned(),
    }
}

fn serialize_decimal<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Display,
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

#[cfg(test)]
pub(super) fn with_phase_timing_capture_for_test<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, PhaseTimingCaptureError> {
    start_capture()?;
    let result = operation();
    match result {
        Ok(value) => {
            let collector = take_capture()?;
            collector.finish().map(|_| value)
        }
        Err(_) => {
            discard_capture();
            Err(PhaseTimingCaptureError::NoCommittedTick)
        }
    }
}

#[cfg(test)]
#[path = "phase_timing_tests.rs"]
mod tests;
