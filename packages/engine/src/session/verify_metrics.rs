//! 仅 verification-harness 使用的后台析构账本；不改普通构建的提交或释放路径。
//!
//! 每个值发送前登记 numeric id，实际 Drop 后按同 id 完成。结果可晚于完成到达。
//! Mutex/BTreeMap 的额外诊断开销会扰动调度，纳秒数据不能冒充普通 binary 收益；
//! 精确计数与 id 配对用于验证机制。任何账本异常都使诊断失效，不改变交易提交。

use super::{FieldId, FIELD_ID_COUNT};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU64, AtomicU8, Ordering},
    Mutex, MutexGuard,
};
use std::time::Instant;

/// 固定 Job 领域分类，不是 worker 数或调度参数。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[repr(u8)]
pub enum JobKind {
    Other = 0,
    CommitField = 1,
    CommitState = 2,
    BatchCheckpoint = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum Outcome {
    Queued,
    FullInline,
    DisconnectedInline,
    SpawnFailedInline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct TraceInvalid {
    pub code: u8,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct JobRecord {
    pub id: u64,
    pub kind: JobKind,
    pub outcome: Option<Outcome>,
    pub completed: bool,
    pub drop_ns: u64,
    pub drop_panicked: bool,
}

struct Ledger {
    next: u64,
    pending: u64,
    peak: u64,
    jobs: BTreeMap<u64, JobRecord>,
}

impl Ledger {
    const fn new() -> Self {
        Self {
            next: 1,
            pending: 0,
            peak: 0,
            jobs: BTreeMap::new(),
        }
    }
}

pub(in crate::session) struct VerifyMetrics {
    pub commit_from_calls: AtomicU64,
    pub commit_from_ns: AtomicU64,
    pub civil_checkpoint_calls: AtomicU64,
    pub civil_checkpoint_ns: AtomicU64,
    pub civil_checkpoint_drop_calls: AtomicU64,
    pub civil_checkpoint_drop_ns: AtomicU64,
    pub actor_producers: AtomicU64,
    /// 修复1/修复2升级（Luna+Astra）：commit_from 逐字段移交的 per-FieldId 计数，
    /// 供字段集合级完整性绊线（观测字段集 == COMMIT_FIELD_EXPECTED 全集）。
    pub commit_fields: [AtomicU64; FIELD_ID_COUNT],
    ledger: Mutex<Ledger>,
    invalid: AtomicU8,
}

impl VerifyMetrics {
    pub(in crate::session) const fn new() -> Self {
        Self {
            commit_from_calls: AtomicU64::new(0),
            commit_from_ns: AtomicU64::new(0),
            civil_checkpoint_calls: AtomicU64::new(0),
            civil_checkpoint_ns: AtomicU64::new(0),
            civil_checkpoint_drop_calls: AtomicU64::new(0),
            civil_checkpoint_drop_ns: AtomicU64::new(0),
            actor_producers: AtomicU64::new(0),
            commit_fields: [const { AtomicU64::new(0) }; FIELD_ID_COUNT],
            ledger: Mutex::new(Ledger::new()),
            invalid: AtomicU8::new(0),
        }
    }

    fn fault(&self, code: u8) {
        let _ = self
            .invalid
            .compare_exchange(0, code, Ordering::Relaxed, Ordering::Relaxed);
    }

    /// P2②：producer 退出的 checked 递减。计数为 0 时显式 fault（code=5），
    /// 替代旧 `fetch_sub` 的静默 u64 回绕；fault 使诊断账本失效而不是伪装成功。
    pub(in crate::session) fn producer_exit(&self) {
        match self
            .actor_producers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_sub(1)
            }) {
            Ok(_) => {}
            Err(_) => self.fault(5),
        }
    }

    /// 修复1/修复2升级：登记一次 commit 字段移交（与 JobKind::CommitField 计数互补，
    /// 按 FieldId 细分）。纯计数诊断，不进 ledger、不影响 any 提交/释放路径。
    pub(in crate::session) fn record_commit_field(&self, field: FieldId) {
        self.commit_fields[field as usize].fetch_add(1, Ordering::Relaxed);
    }

    fn lock(&self) -> Option<MutexGuard<'_, Ledger>> {
        match self.ledger.lock() {
            Ok(ledger) => Some(ledger),
            Err(_) => {
                self.fault(1);
                None
            }
        }
    }

    pub(in crate::session) fn register(&self, kind: JobKind) -> Ticket<'_> {
        let mut id = 0;
        if let Some(mut ledger) = self.lock() {
            id = ledger.next;
            match ledger.next.checked_add(1) {
                Some(next) => ledger.next = next,
                None => self.fault(2),
            }
            ledger.pending += 1;
            ledger.peak = ledger.peak.max(ledger.pending);
            ledger.jobs.insert(
                id,
                JobRecord {
                    id,
                    kind,
                    outcome: None,
                    completed: false,
                    drop_ns: 0,
                    drop_panicked: false,
                },
            );
        }
        Ticket { metrics: self, id }
    }

    fn outcome(&self, id: u64, outcome: Outcome) {
        if let Some(mut ledger) = self.lock() {
            match ledger.jobs.get_mut(&id) {
                Some(record) if record.outcome.is_none() => record.outcome = Some(outcome),
                _ => self.fault(3),
            }
        }
    }

    fn complete(&self, id: u64, drop_ns: u64, panicked: bool) {
        if let Some(mut ledger) = self.lock() {
            match ledger.jobs.get_mut(&id) {
                Some(record) if !record.completed => {
                    record.completed = true;
                    record.drop_ns = drop_ns;
                    record.drop_panicked = panicked;
                    ledger.pending -= 1;
                }
                _ => self.fault(4),
            }
        }
    }

    pub(in crate::session) fn records(&self) -> Result<Vec<JobRecord>, TraceInvalid> {
        let records = match self.lock() {
            Some(ledger) => ledger.jobs.values().copied().collect(),
            None => Vec::new(),
        };
        let code = self.invalid.load(Ordering::Relaxed);
        if code == 0 {
            Ok(records)
        } else {
            Err(TraceInvalid { code })
        }
    }

    pub(in crate::session) fn snapshot(&self) -> Result<VerifySnapshot, TraceInvalid> {
        let mut snap = VerifySnapshot {
            commit_from_calls: self.commit_from_calls.load(Ordering::Relaxed),
            commit_from_ns: self.commit_from_ns.load(Ordering::Relaxed),
            civil_checkpoint_calls: self.civil_checkpoint_calls.load(Ordering::Relaxed),
            civil_checkpoint_ns: self.civil_checkpoint_ns.load(Ordering::Relaxed),
            civil_checkpoint_drop_calls: self.civil_checkpoint_drop_calls.load(Ordering::Relaxed),
            civil_checkpoint_drop_ns: self.civil_checkpoint_drop_ns.load(Ordering::Relaxed),
            actor_producers: self.actor_producers.load(Ordering::Acquire),
            registered: 0,
            completed: 0,
            pending: 0,
            pending_peak: 0,
            unclassified: 0,
            all_ids_paired: false,
            by_kind: [KindSnapshot::new(); 4],
            by_field: [0; FIELD_ID_COUNT],
        };
        for (slot, counter) in snap
            .by_field
            .iter_mut()
            .zip(self.commit_fields.iter())
        {
            *slot = counter.load(Ordering::Relaxed);
        }
        if let Some(ledger) = self.lock() {
            snap.pending = ledger.pending;
            snap.pending_peak = ledger.peak;
            for record in ledger.jobs.values() {
                snap.registered += 1;
                snap.completed += u64::from(record.completed);
                snap.unclassified += u64::from(record.outcome.is_none());
                let row = &mut snap.by_kind[record.kind as usize];
                row.registered += 1;
                row.completed += u64::from(record.completed);
                row.drop_ns += record.drop_ns;
                row.drop_panicked += u64::from(record.drop_panicked);
                match record.outcome {
                    Some(Outcome::Queued) => row.queued += 1,
                    Some(Outcome::FullInline) => row.full_inline += 1,
                    Some(Outcome::DisconnectedInline) => row.disconnected_inline += 1,
                    Some(Outcome::SpawnFailedInline) => row.spawn_failed_inline += 1,
                    None => {}
                }
            }
            snap.all_ids_paired = snap.pending == 0
                && snap.unclassified == 0
                && snap.registered == snap.completed
                && ledger
                    .jobs
                    .values()
                    .all(|r| r.completed && r.outcome.is_some() && !r.drop_panicked);
        }
        let code = self.invalid.load(Ordering::Relaxed);
        if code == 0 {
            Ok(snap)
        } else {
            Err(TraceInvalid { code })
        }
    }
}

pub(in crate::session) static METRICS: VerifyMetrics = VerifyMetrics::new();

/// Producer 的结果报告 token；id 对应账本的一行，不复制任何交易状态。
#[derive(Clone, Copy)]
pub(in crate::session) struct Ticket<'a> {
    metrics: &'a VerifyMetrics,
    id: u64,
}
impl Ticket<'_> {
    pub(in crate::session) fn outcome(self, outcome: Outcome) {
        self.metrics.outcome(self.id, outcome);
    }
}

struct Completion<'a> {
    ticket: Ticket<'a>,
    started: Instant,
    inherited_unwind: bool,
}
impl Drop for Completion<'_> {
    fn drop(&mut self) {
        // P2③：只有进入本 payload Drop 之后新出现的 panic 才记 drop_panicked。
        // `thread::panicking()` 只能证明线程在 unwind；外层 panic 连带析构
        /// （如 receiver 析构队列剩余 Job）时健康 payload 不得被误标。
        let panicked = !self.inherited_unwind && std::thread::panicking();
        self.ticket.metrics.complete(
            self.ticket.id,
            self.started.elapsed().as_nanos() as u64,
            panicked,
        );
    }
}

/// Closure 被执行或在 Full/Disconnected 路径被丢弃时，同一值都由此 Drop 恰一次。
/// Completion 是字段析构 guard，即使 payload Drop panic 也会配对该 id 并标明 panic。
pub(in crate::session) struct Tracked<'a, T> {
    value: Option<T>,
    completion: Completion<'a>,
}
impl<'a, T> Tracked<'a, T> {
    pub(in crate::session) fn new(
        value: T,
        metrics: &'a VerifyMetrics,
        kind: JobKind,
    ) -> (Self, Ticket<'a>) {
        let ticket = metrics.register(kind);
        (
            Self {
                value: Some(value),
                completion: Completion {
                    ticket,
                    started: Instant::now(),
                    inherited_unwind: false,
                },
            },
            ticket,
        )
    }
}
impl<T> Drop for Tracked<'_, T> {
    fn drop(&mut self) {
        // P2③：先记录进入时是否已处于 unwind，再析构 payload；Drop 内嵌套 panic
        // 不会递归重标（Completion 只记录一次，before/after 语义保持）。
        self.completion.inherited_unwind = std::thread::panicking();
        self.completion.started = Instant::now();
        drop(self.value.take());
    }
}

/// 原有调用点的区间计时；不包含 checkpoint 创建失败或 rollback 的假成功。
pub(in crate::session) struct Scope<'a> {
    started: Instant,
    nanos: &'a AtomicU64,
}
impl<'a> Scope<'a> {
    pub(in crate::session) fn new(calls: &AtomicU64, nanos: &'a AtomicU64) -> Self {
        calls.fetch_add(1, Ordering::Relaxed);
        Self {
            started: Instant::now(),
            nanos,
        }
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.nanos
            .fetch_add(self.started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}
pub(in crate::session) fn add_span(calls: &AtomicU64, nanos: &AtomicU64, started: Instant) {
    calls.fetch_add(1, Ordering::Relaxed);
    nanos.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct KindSnapshot {
    pub registered: u64,
    pub completed: u64,
    pub queued: u64,
    pub full_inline: u64,
    pub disconnected_inline: u64,
    pub spawn_failed_inline: u64,
    pub drop_ns: u64,
    pub drop_panicked: u64,
}
impl KindSnapshot {
    const fn new() -> Self {
        Self {
            registered: 0,
            completed: 0,
            queued: 0,
            full_inline: 0,
            disconnected_inline: 0,
            spawn_failed_inline: 0,
            drop_ns: 0,
            drop_panicked: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct VerifySnapshot {
    pub commit_from_calls: u64,
    pub commit_from_ns: u64,
    pub civil_checkpoint_calls: u64,
    pub civil_checkpoint_ns: u64,
    pub civil_checkpoint_drop_calls: u64,
    pub civil_checkpoint_drop_ns: u64,
    pub actor_producers: u64,
    pub registered: u64,
    pub completed: u64,
    pub pending: u64,
    pub pending_peak: u64,
    pub unclassified: u64,
    pub all_ids_paired: bool,
    pub by_kind: [KindSnapshot; 4],
    /// per-FieldId 的 commit 字段移交累计计数（修复2升级：集合级完整性证据）。
    /// serde 对 >32 元素数组无内置 Serialize 实现，诊断输出跳过该字段（进程内直读）。
    #[serde(skip)]
    pub by_field: [u64; FIELD_ID_COUNT],
}

#[doc(hidden)]
pub fn snapshot() -> Result<VerifySnapshot, TraceInvalid> {
    METRICS.snapshot()
}
#[doc(hidden)]
pub fn records() -> Result<Vec<JobRecord>, TraceInvalid> {
    METRICS.records()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Arc};
    struct Probe(Arc<AtomicUsize>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn registration_records_pending_before_a_consumer_can_finish() {
        let metrics = VerifyMetrics::new();
        let dropped = Arc::new(AtomicUsize::new(0));
        let (value, ticket) = Tracked::new(Probe(dropped.clone()), &metrics, JobKind::CommitState);
        let registered = metrics.snapshot().unwrap();
        assert_eq!(
            (
                registered.registered,
                registered.pending,
                registered.pending_peak
            ),
            (1, 1, 1)
        );
        // 消费者先完成，再返回 producer 的 queued 结果，仍按同一 id 合并。
        drop(value);
        assert!(!metrics.snapshot().unwrap().all_ids_paired);
        ticket.outcome(Outcome::Queued);
        let done = metrics.snapshot().unwrap();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(
            (done.completed, done.pending, done.by_kind[2].queued),
            (1, 0, 1)
        );
        assert!(done.all_ids_paired);
        let record = metrics.records().unwrap();
        assert_eq!(record[0].id, 1);
        assert!(record[0].completed);
    }

    #[test]
    fn thirty_four_fields_and_other_job_cannot_share_a_sticky_kind() {
        let metrics = VerifyMetrics::new();
        for _ in 0..34 {
            let (value, ticket) = Tracked::new((), &metrics, JobKind::CommitField);
            drop(value);
            ticket.outcome(Outcome::Queued);
        }
        let (value, ticket) = Tracked::new((), &metrics, JobKind::Other);
        ticket.outcome(Outcome::FullInline);
        drop(value);
        let snap = metrics.snapshot().unwrap();
        assert_eq!(snap.by_kind[1].queued, 34);
        assert_eq!(snap.by_kind[0].full_inline, 1);
        assert_eq!(snap.by_kind[2].registered, 0);
        assert_eq!(snap.by_kind[3].registered, 0);
        assert!(snap.all_ids_paired);
    }

    #[test]
    fn instances_keep_ids_and_outcomes_isolated() {
        let first = VerifyMetrics::new();
        let second = VerifyMetrics::new();
        let (v1, t1) = Tracked::new((), &first, JobKind::CommitState);
        let (v2, t2) = Tracked::new((), &second, JobKind::BatchCheckpoint);
        t1.outcome(Outcome::Queued);
        t2.outcome(Outcome::DisconnectedInline);
        drop(v2);
        assert!(!first.snapshot().unwrap().all_ids_paired);
        assert!(second.snapshot().unwrap().all_ids_paired);
        assert_eq!(first.records().unwrap()[0].id, 1);
        assert_eq!(second.records().unwrap()[0].id, 1);
        drop(v1);
        assert!(first.snapshot().unwrap().all_ids_paired);
    }

    #[test]
    fn duplicate_completion_is_explicitly_invalid_not_a_drained_success() {
        let metrics = VerifyMetrics::new();
        let (value, ticket) = Tracked::new((), &metrics, JobKind::Other);
        ticket.outcome(Outcome::Queued);
        drop(value);
        metrics.complete(ticket.id, 0, false);
        assert_eq!(metrics.snapshot().unwrap_err(), TraceInvalid { code: 4 });
    }

    #[test]
    fn full_disconnected_and_spawn_failed_are_completed_but_not_queued() {
        for outcome in [
            Outcome::FullInline,
            Outcome::DisconnectedInline,
            Outcome::SpawnFailedInline,
        ] {
            let metrics = VerifyMetrics::new();
            let dropped = Arc::new(AtomicUsize::new(0));
            let (value, ticket) = Tracked::new(Probe(dropped.clone()), &metrics, JobKind::Other);
            drop(value);
            ticket.outcome(outcome);
            let snap = metrics.snapshot().unwrap();
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
            assert_eq!(snap.by_kind[0].queued, 0);
            assert_eq!(snap.completed, 1);
            assert!(snap.all_ids_paired);
        }
    }

    #[test]
    fn a_late_registration_invalidates_a_previous_empty_snapshot() {
        let metrics = VerifyMetrics::new();
        assert!(metrics.snapshot().unwrap().all_ids_paired);
        let (value, ticket) = Tracked::new((), &metrics, JobKind::CommitState);
        assert!(!metrics.snapshot().unwrap().all_ids_paired);
        drop(value);
        ticket.outcome(Outcome::Queued);
        assert!(metrics.snapshot().unwrap().all_ids_paired);
    }

    #[test]
    fn timings_use_instance_specific_counters() {
        let metrics = VerifyMetrics::new();
        {
            let _scope = Scope::new(&metrics.commit_from_calls, &metrics.commit_from_ns);
        }
        add_span(
            &metrics.civil_checkpoint_calls,
            &metrics.civil_checkpoint_ns,
            Instant::now(),
        );
        let snap = metrics.snapshot().unwrap();
        assert_eq!(snap.commit_from_calls, 1);
        assert_eq!(snap.civil_checkpoint_calls, 1);
        assert!(snap.commit_from_ns > 0);
    }

    /// payload Drop panic 时不二次 panic abort：unwind 中 Completion 仍按同一 id
    /// 配对并标记 drop_panicked；健康排空判据显式拒绝该记录。
    #[test]
    fn payload_drop_panic_pairs_the_id_without_abort() {
        struct PanickyDrop;
        impl Drop for PanickyDrop {
            fn drop(&mut self) {
                panic!("payload drop panic（负控）");
            }
        }
        let metrics = VerifyMetrics::new();
        let (tracked, ticket) = Tracked::new(PanickyDrop, &metrics, JobKind::Other);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            drop(tracked);
        }));
        assert!(caught.is_err(), "payload panic 应被捕获而不是 abort");
        ticket.outcome(Outcome::Queued);
        let records = metrics.records().expect("账本在 unwind 后必须仍可读");
        assert_eq!(records.len(), 1);
        assert!(records[0].completed, "unwind 中 Completion 必须完成配对");
        assert!(records[0].drop_panicked, "必须记录 panic 事实");
        assert_eq!(records[0].outcome, Some(Outcome::Queued));
        let snap = metrics.snapshot().unwrap();
        assert_eq!(snap.pending, 0);
        assert!(
            !snap.all_ids_paired,
            "含 panic 的排空不得宣称健康配对（drain 显式失败）"
        );
    }

    /// 双 producer 并发注册/完成/outcome 乱序：id 精确不重不漏，账本一致。
    #[test]
    fn concurrent_producers_get_disjoint_paired_ids() {
        let metrics = VerifyMetrics::new();
        const PER: u64 = 200;
        std::thread::scope(|scope| {
            for worker in 0..2u64 {
                let metrics = &metrics;
                scope.spawn(move || {
                    for i in 0..PER {
                        let (value, ticket) = Tracked::new((), metrics, JobKind::CommitField);
                        if (worker + i) % 2 == 0 {
                            // outcome 先于完成到达（worker 抢先消费场景）
                            ticket.outcome(Outcome::Queued);
                            drop(value);
                        } else {
                            // 完成先于 outcome 返回
                            drop(value);
                            ticket.outcome(Outcome::Queued);
                        }
                    }
                });
            }
        });
        let snap = metrics.snapshot().unwrap();
        assert_eq!(snap.registered, 2 * PER);
        assert_eq!(snap.completed, 2 * PER);
        assert_eq!(snap.pending, 0);
        assert_eq!(snap.by_kind[JobKind::CommitField as usize].queued, 2 * PER);
        assert!(snap.all_ids_paired);
        let ids: Vec<u64> = metrics.records().unwrap().iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            (1..=2 * PER).collect::<Vec<_>>(),
            "id 必须连续且不重不漏"
        );
    }

    /// P2③负控A：健康 payload 在"外来的" unwind 中被析构时不得误记 drop_panicked。
    ///
    /// 修复前 Completion::drop 只看 `thread::panicking()`：本 Job 自身 Drop 健康，
    /// 但同线程先发生的外层 panic 使 unwind 路过本 Completion，健康 Job 被误标，
    /// 进而污染 all_ids_paired 与 drop_panicked 统计。
    #[test]
    fn healthy_payload_dropped_during_foreign_unwind_is_not_marked_panicked() {
        let metrics = VerifyMetrics::new();
        let ticket_storage = std::cell::RefCell::new(None);
        {
            let metrics_ref = &metrics;
            let storage = &ticket_storage;
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let (tracked, ticket) = Tracked::new((), metrics_ref, JobKind::Other);
                *storage.borrow_mut() = Some(ticket);
                // 保持 tracked 活到 panic 触发的 unwind：unwind 析构局部 _keep，
                // 模拟"外层 panic 连带析构队列中健康 Job"。
                let _keep = tracked;
                panic!("外层 panic（负控）：本 tracked 自身 Drop 健康");
            }));
            assert!(caught.is_err(), "外层 panic 应被捕获");
        }
        let ticket = ticket_storage.borrow_mut().take().unwrap();
        ticket.outcome(Outcome::Queued);
        let records = metrics.records().expect("unwind 后账本必须可读");
        assert_eq!(records.len(), 1);
        assert!(records[0].completed, "unwind 中 Completion 仍完成配对");
        assert!(
            !records[0].drop_panicked,
            "健康 payload 在外来 unwind 中析构不得误记 drop_panicked（修复前为 true）"
        );
        assert!(metrics.snapshot().unwrap().all_ids_paired);
    }

    /// P2③负控B（reviewer 场景单元化）：同一次 unwind 中，先 panic 的 Job 与
    /// 随后被连带析构的健康 Job 必须分别标 true/false，不互相污染。
    struct PanickyDrop;
    impl Drop for PanickyDrop {
        fn drop(&mut self) {
            panic!("payload drop panic（负控B）");
        }
    }

    #[test]
    fn panicking_job_does_not_taint_the_next_unwound_healthy_job() {
        use std::cell::RefCell;
        // 声明序 first 在前：unwind 按声明序析构，first 的 Drop 触发 panic 后
        // second 在同一 unwind 中被析构（等价于 receiver 析构连带队列剩余 Job）。
        struct DuringUnwind<'a> {
            first: Tracked<'a, PanickyDrop>,
            second: Tracked<'a, ()>,
        }
        let metrics = VerifyMetrics::new();
        let tickets = RefCell::new(Vec::new());
        {
            let metrics_ref = &metrics;
            let tickets_ref = &tickets;
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let (first, t1) = Tracked::new(PanickyDrop, metrics_ref, JobKind::Other);
                let (second, t2) = Tracked::new((), metrics_ref, JobKind::CommitField);
                *tickets_ref.borrow_mut() = vec![t1, t2];
                // 整体析构：first 的 value Drop panic 开始 unwind，second 连带析构。
                drop(DuringUnwind { first, second });
            }));
            assert!(caught.is_err(), "first 的 payload panic 应被捕获");
        }
        let tickets = tickets.borrow();
        tickets[0].outcome(Outcome::Queued);
        tickets[1].outcome(Outcome::Queued);
        let records = metrics.records().expect("unwind 后账本必须可读");
        assert!(
            records.iter().all(|r| r.completed),
            "两个 id 都必须在 unwind 中完成配对"
        );
        let first = records.iter().find(|r| r.kind == JobKind::Other).unwrap();
        let second = records
            .iter()
            .find(|r| r.kind == JobKind::CommitField)
            .unwrap();
        assert!(first.drop_panicked, "真正 panic 的 Job 必须仍被标记");
        assert!(
            !second.drop_panicked,
            "同一 unwind 中连带析构的健康 Job 不得被误标（修复前为 true）"
        );
        assert!(
            !metrics.snapshot().unwrap().all_ids_paired,
            "含真实 panic 的排空仍不得宣称健康配对"
        );
    }

    /// P2②负控（红测迁移版）：修复前绕过工厂构造 guard 再 Drop 会把计数静默回绕
    /// 成 u64::MAX 且账本仍 Ok（红测证据见增量 .tmp/red-engine.log）。私有字段修复
    /// 后外部构造不再可能，此处迁移为工厂路径断言 + checked 递减的 underflow fault。
    #[test]
    fn producer_registration_pairs_and_underflow_exit_faults_explicitly() {
        use super::super::verification_actor_guard;
        use super::METRICS;
        METRICS.actor_producers.store(0, Ordering::Release);
        {
            let _guard = verification_actor_guard();
            assert_eq!(
                METRICS.actor_producers.load(Ordering::Acquire),
                1,
                "工厂构造必须同步登记"
            );
        }
        assert_eq!(
            METRICS.actor_producers.load(Ordering::Acquire),
            0,
            "guard Drop 必须归位计数"
        );
        // 无对应登记的退出必须显式 fault（code=5）而不是回绕。
        METRICS.producer_exit();
        assert_eq!(
            METRICS.snapshot().unwrap_err().code,
            5,
            "underflow 退出必须显式 fault"
        );
        // 恢复计数；invalid 是 sticky 的，此断言后本测试不再依赖 Ok 快照。
        METRICS.actor_producers.store(0, Ordering::Release);
    }
}
