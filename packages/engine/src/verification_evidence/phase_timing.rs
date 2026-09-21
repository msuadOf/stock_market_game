//! Opt-in wall-clock evidence for one successfully committed authoritative tick.
//!
//! This recorder is intentionally thread-local and ephemeral. It is not a `GameSession` field,
//! never participates in persistence or hashing, and constructs public evidence only after P9 has
//! committed. The runnable-thread value is the number of Rayon workers in the current registry
//! that are eligible to execute work at the sample point; it is not an operating-system load or
//! CPU-utilization estimate.

use crate::{session::pipeline::TickPhase, Event, GameSession};
use serde::{Serialize, Serializer};
use std::{cell::RefCell, fmt::Display, time::Instant};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseTimingPhase {
    ExpiryShadow,
    SealAllocationSnapshot,
    DecisionShadow,
    AccountValidation,
    StockProcessing,
    ReceiptAggregation,
    SettlementShadow,
    DerivationAudit,
    DualHashCheck,
    CommitTick,
}

impl PhaseTimingPhase {
    pub const ALL: [Self; 10] = [
        Self::ExpiryShadow,
        Self::SealAllocationSnapshot,
        Self::DecisionShadow,
        Self::AccountValidation,
        Self::StockProcessing,
        Self::ReceiptAggregation,
        Self::SettlementShadow,
        Self::DerivationAudit,
        Self::DualHashCheck,
        Self::CommitTick,
    ];

    pub const fn rank(self) -> u8 {
        match self {
            Self::ExpiryShadow => 0,
            Self::SealAllocationSnapshot => 1,
            Self::DecisionShadow => 2,
            Self::AccountValidation => 3,
            Self::StockProcessing => 4,
            Self::ReceiptAggregation => 5,
            Self::SettlementShadow => 6,
            Self::DerivationAudit => 7,
            Self::DualHashCheck => 8,
            Self::CommitTick => 9,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::ExpiryShadow => "expiry_shadow",
            Self::SealAllocationSnapshot => "seal_allocation_snapshot",
            Self::DecisionShadow => "decision_shadow",
            Self::AccountValidation => "account_validation",
            Self::StockProcessing => "stock_processing",
            Self::ReceiptAggregation => "receipt_aggregation",
            Self::SettlementShadow => "settlement_shadow",
            Self::DerivationAudit => "derivation_audit",
            Self::DualHashCheck => "dual_hash_check",
            Self::CommitTick => "commit_tick",
        }
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
            TickPhase::DecisionShadow => Self::DecisionShadow,
            TickPhase::AccountValidation => Self::AccountValidation,
            TickPhase::StockProcessing => Self::StockProcessing,
            TickPhase::ReceiptAggregation => Self::ReceiptAggregation,
            TickPhase::SettlementShadow => Self::SettlementShadow,
            TickPhase::DerivationAudit => Self::DerivationAudit,
            TickPhase::DualHashCheck => Self::DualHashCheck,
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
    #[serde(serialize_with = "serialize_decimal")]
    tick_before: u64,
    #[serde(serialize_with = "serialize_decimal")]
    tick_after: u64,
    records: Vec<PhaseTimingRecord>,
}

impl CommittedPhaseTiming {
    pub const SCHEMA: &'static str = "escrow-committed-phase-timing-v1";

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
    #[error("phase-timing capture is incomplete at {phase}")]
    IncompletePhase { phase: &'static str },
    #[error("committed tick advanced from {before} to {after}, expected exactly one tick")]
    UnexpectedTickAdvance { before: u64, after: u64 },
    #[error("phase-timing counter overflow at {field}")]
    CounterOverflow { field: &'static str },
}

impl GameSession {
    /// Executes the ordinary public [`GameSession::step`] path with ephemeral timing enabled.
    ///
    /// A failed step returns only its typed fatal and discards all partial samples. Successful
    /// evidence is returned only after P9 has installed the tick candidate into this session.
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

struct ActiveSpan {
    phase: PhaseTimingPhase,
    started: Instant,
}

struct Collector {
    tick_before: Option<u64>,
    tick_after: Option<u64>,
    current: Option<ActiveSpan>,
    accumulators: [Accumulator; 10],
    overflow: Option<&'static str>,
}

impl Collector {
    fn new() -> Self {
        Self {
            tick_before: None,
            tick_after: None,
            current: None,
            accumulators: [Accumulator::EMPTY; 10],
            overflow: None,
        }
    }

    fn close_current(&mut self) {
        let Some(span) = self.current.take() else {
            return;
        };
        let elapsed = span.started.elapsed().as_nanos();
        let accumulator = &mut self.accumulators[span.phase.index()];
        accumulator.wall_time_ns = match accumulator.wall_time_ns.checked_add(elapsed) {
            Some(total) => total,
            None => {
                self.overflow = Some("wall_time_ns");
                accumulator.wall_time_ns
            }
        };
        accumulator.span_count = match accumulator.span_count.checked_add(1) {
            Some(total) => total,
            None => {
                self.overflow = Some("span_count");
                accumulator.span_count
            }
        };
        if let Err(field) = accumulator.sample(current_runnable_threads()) {
            self.overflow = Some(field);
        }
    }

    fn enter(&mut self, phase: PhaseTimingPhase) {
        self.close_current();
        if let Err(field) = self.accumulators[phase.index()].sample(current_runnable_threads()) {
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
        let mut records = Vec::with_capacity(PhaseTimingPhase::ALL.len());
        for phase in PhaseTimingPhase::ALL {
            let accumulator = self.accumulators[phase.index()];
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
        Ok(CommittedPhaseTiming {
            schema: CommittedPhaseTiming::SCHEMA,
            tick_before,
            tick_after,
            records,
        })
    }
}

thread_local! {
    static COLLECTOR: RefCell<Option<Collector>> = const { RefCell::new(None) };
}

fn current_runnable_threads() -> usize {
    rayon::current_num_threads()
}

fn capture_one_committed_tick<T>(
    operation: impl FnOnce() -> Result<T, crate::session::StepFatal>,
) -> Result<(T, CommittedPhaseTiming), PhaseTimingCaptureError> {
    start_capture()?;
    let result = operation();
    let collector = take_capture()?;
    match result {
        Ok(value) => collector.finish().map(|timing| (value, timing)),
        Err(fatal) => Err(PhaseTimingCaptureError::Step(fatal)),
    }
}

fn start_capture() -> Result<(), PhaseTimingCaptureError> {
    COLLECTOR.with_borrow_mut(|slot| {
        if slot.is_some() {
            Err(PhaseTimingCaptureError::CaptureAlreadyActive)
        } else {
            *slot = Some(Collector::new());
            Ok(())
        }
    })
}

fn take_capture() -> Result<Collector, PhaseTimingCaptureError> {
    COLLECTOR.with_borrow_mut(|slot| slot.take().ok_or(PhaseTimingCaptureError::NoCommittedTick))
}

pub(crate) fn begin_authoritative_tick(tick_before: u64) -> Result<(), crate::session::StepFatal> {
    COLLECTOR.with_borrow_mut(|slot| {
        let Some(collector) = slot.as_mut() else {
            return Ok(());
        };
        if collector.tick_before.replace(tick_before).is_some() {
            return Err(invariant(
                "phase-timing capture observed more than one tick",
            ));
        }
        Ok(())
    })
}

pub(crate) fn enter_phase(phase: TickPhase) {
    COLLECTOR.with_borrow_mut(|slot| {
        if let Some(collector) = slot.as_mut() {
            collector.enter(phase.into());
        }
    });
}

pub(crate) fn validate_precommit() -> Result<(), crate::session::StepFatal> {
    COLLECTOR.with_borrow_mut(|slot| {
        let Some(collector) = slot.as_mut() else {
            return Ok(());
        };
        collector.close_current();
        for phase in PhaseTimingPhase::ALL.into_iter().take(9) {
            if collector.accumulators[phase.index()].span_count == 0 {
                return Err(invariant(&format!(
                    "phase-timing capture reached P9 without {}",
                    phase.name()
                )));
            }
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
    COLLECTOR.with_borrow_mut(|slot| {
        if let Some(collector) = slot.as_mut() {
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
    let collector = take_capture()?;
    match result {
        Ok(value) => collector.finish().map(|_| value),
        Err(_) => Err(PhaseTimingCaptureError::NoCommittedTick),
    }
}

#[cfg(test)]
#[path = "phase_timing_tests.rs"]
mod tests;
