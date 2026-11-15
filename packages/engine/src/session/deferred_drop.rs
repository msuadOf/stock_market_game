//! tick 提交的旧状态析构移交；平台队列、线程与同步释放由 LibOS 实现。
//! 字段分类、验收计数和受控 backend 仅留在 engine，生产不读取这些诊断作决策。
//! 单次 commit_from 移交多个旧字段。队列限制的是等待 Job 数，不是保留字节；
//! 旧权威字段和共享页的实际保留量须结合 pending、RSS 与 drain 观测。

#[cfg(not(test))]
use libos::deferred_drop::defer as dispatch_value;
pub use libos::deferred_drop::DeferredDropDegradation;
#[cfg(any(feature = "verification-harness", test))]
use libos::deferred_drop::Job;
use libos::deferred_drop::Submission;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(feature = "verification-harness", test))]
use std::sync::mpsc;
#[cfg(feature = "verification-harness")]
use std::sync::mpsc::SyncSender;

static DEFERRED_COUNT: AtomicU64 = AtomicU64::new(0);

/// 仅用于测试的实例级 backend：观察真实移交值，并把真实 Job 留在受控队列。
/// 不新增生产开关，不读取全进程计数，也不替换提交或旧状态的 Drop。
#[cfg(test)]
pub(super) mod controlled {
    use super::{DeferredDropDegradation, Job, Submission};
    use libos::deferred_drop::TestBackend;
    use std::any::Any;
    use std::cell::RefCell;
    use std::sync::mpsc::SyncSender;

    type AnyObserver = Box<dyn FnMut(&dyn Any)>;

    struct Installed {
        backend: TestBackend,
        observe: Option<AnyObserver>,
    }

    thread_local! {
        static BACKEND: RefCell<Option<Installed>> = const { RefCell::new(None) };
    }

    pub(super) fn observe<T: 'static>(value: &T) {
        BACKEND.with(|backend| {
            if let Some(installed) = backend.borrow_mut().as_mut() {
                if let Some(observe) = installed.observe.as_mut() {
                    observe(value);
                }
            }
        });
    }

    /// 未安装受控 backend 时返还原 Job，调用方继续走生产后端；不丢所有权。
    pub(super) fn dispatch(job: Job) -> Result<Submission, Job> {
        BACKEND.with(|slot| match slot.borrow().as_ref() {
            None => Err(job),
            Some(installed) => Ok(installed.backend.dispatch(job)),
        })
    }

    struct Reset;

    impl Drop for Reset {
        fn drop(&mut self) {
            BACKEND.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn install(backend: TestBackend, observe: Option<AnyObserver>) -> Reset {
        BACKEND.with(|slot| {
            assert!(slot.borrow().is_none(), "controlled backend 不支持嵌套");
            *slot.borrow_mut() = Some(Installed { backend, observe });
        });
        Reset
    }

    pub(in crate::session) fn with_backend<R>(
        sender: SyncSender<Job>,
        observe: impl FnMut(&dyn Any) + 'static,
        run: impl FnOnce() -> R,
    ) -> R {
        with_running_backend(sender, observe, run).0
    }

    /// 安装在途受控 backend，返回 (结果, 退出时的退化诊断)。
    pub(in crate::session) fn with_running_backend<R>(
        sender: SyncSender<Job>,
        observe: impl FnMut(&dyn Any) + 'static,
        run: impl FnOnce() -> R,
    ) -> (R, Option<DeferredDropDegradation>) {
        let _reset = install(TestBackend::running(sender), Some(Box::new(observe)));
        let output = run();
        let degradation = BACKEND.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|installed| installed.backend.degradation())
        });
        (output, degradation.flatten())
    }

    /// 安装"线程创建失败"的持久退化 backend（errno 来自真实 OS 失败），
    /// 返回 (结果, 退出时的退化诊断)。
    pub(in crate::session) fn with_failed_backend<R>(
        os_errno: Option<i32>,
        run: impl FnOnce() -> R,
    ) -> (R, Option<DeferredDropDegradation>) {
        let _reset = install(TestBackend::failed(os_errno), None);
        let output = run();
        let degradation = BACKEND.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|installed| installed.backend.degradation())
        });
        (output, degradation.flatten())
    }
}

/// 把值的析构移交给专用线程。
///
/// 两端使用同一 FIFO：满时就地释放；退化分类与队列满区分，计数只反映已接纳任务。
pub fn defer<T: Send + 'static>(value: T) {
    #[cfg(feature = "verification-harness")]
    {
        use crate::session::verify_metrics::JobKind;
        let kind = if std::any::TypeId::of::<T>()
            == std::any::TypeId::of::<super::CommittableSessionState>()
        {
            JobKind::CommitState
        } else if std::any::TypeId::of::<T>()
            == std::any::TypeId::of::<super::protocol::ProtocolCheckpoint>()
        {
            JobKind::BatchCheckpoint
        } else {
            JobKind::Other
        };
        defer_tagged(value, kind);
    }
    #[cfg(not(feature = "verification-harness"))]
    defer_tagged(value);
}

/// 字段交接 + 旧值移交析构：`self.field` 换成新值，旧值去专用线程。
/// commit_from 逐字段路径唯一入口（裁决 (d)）。修复1（Luna 到位性）：启用
/// verification-harness 时显式计量为 JobKind::CommitField——经通用 defer 的
/// TypeId 判类只会得到 Other，CommitField 生产路径原本不可达；同时按
/// Astra 修复2升级登记字段身份（FieldId），供字段集合级完整性绊线断言。
/// 析构语义零变化（与 defer 同一 defer_value 出口），纯插桩分类。
pub(super) fn defer_swap<T: Send + 'static>(
    slot: &mut T,
    value: T,
    field: crate::session::FieldId,
) {
    let old = std::mem::replace(slot, value);
    #[cfg(feature = "verification-harness")]
    {
        defer_tagged(old, crate::session::verify_metrics::JobKind::CommitField);
        crate::session::verify_metrics::METRICS.record_commit_field(field);
    }
    #[cfg(not(feature = "verification-harness"))]
    {
        let _ = field;
        defer(old);
    }
}

/// Kind 随本次移交值进入 closure，不用线程本位的隐式标签。
#[cfg(feature = "verification-harness")]
pub(in crate::session) fn defer_tagged<T: Send + 'static>(
    value: T,
    kind: crate::session::verify_metrics::JobKind,
) {
    defer_value(value, kind);
}
#[cfg(not(feature = "verification-harness"))]
fn defer_tagged<T: Send + 'static>(value: T) {
    defer_value(value);
}

fn defer_value<T: Send + 'static>(
    value: T,
    #[cfg(feature = "verification-harness")] kind: crate::session::verify_metrics::JobKind,
) {
    #[cfg(test)]
    controlled::observe(&value);
    #[cfg(feature = "verification-harness")]
    let (value, ticket) = crate::session::verify_metrics::Tracked::new(
        value,
        &crate::session::verify_metrics::METRICS,
        kind,
    );
    let outcome = dispatch_value(value);
    if outcome == Submission::Queued {
        DEFERRED_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(feature = "verification-harness")]
    ticket.outcome(match outcome {
        Submission::Queued => crate::session::verify_metrics::Outcome::Queued,
        Submission::Inline | Submission::FullInline => {
            crate::session::verify_metrics::Outcome::FullInline
        }
        Submission::DisconnectedInline => {
            crate::session::verify_metrics::Outcome::DisconnectedInline
        }
        Submission::SpawnFailedInline => crate::session::verify_metrics::Outcome::SpawnFailedInline,
    });
}

#[cfg(test)]
fn dispatch_value<T: Send + 'static>(value: T) -> Submission {
    match controlled::dispatch(Box::new(move || drop(value))) {
        Ok(outcome) => outcome,
        Err(job) => libos::deferred_drop::defer_job(job),
    }
}

/// 测试观测：累计成功移交到专用线程的析构任务数。
#[cfg(test)]
pub(super) fn deferred_count() -> u64 {
    DEFERRED_COUNT.load(Ordering::Relaxed)
}

/// 宿主侧（server/desktop/wasm 测试）观测：累计移交的析构任务数。
/// 仅用于生产形态断言（证明真实批路径确有移交），生产代码不得依赖。
///
/// 两端只计成功入队（退化路径不增）。
#[doc(hidden)]
pub fn host_deferred_count() -> u64 {
    DEFERRED_COUNT.load(Ordering::Relaxed)
}

/// 宿主侧（server/desktop）观测：后台析构后端的进程级退化诊断。
/// 只读且不初始化后端；`None` = 正常（含尚未初始化）。字符串只在宿主边界
/// 生成；首个故障通过状态持久可见，宿主应只报告一次，不逐 tick 刷屏。
/// 注意：Desktop 经 step_frame→共享 engine commit_from 间接使用此后端，
/// 但未接入退化诊断读取与告警，本轮未覆盖该宿主告警。
#[doc(hidden)]
pub fn host_deferred_degradation() -> Option<DeferredDropDegradation> {
    libos::deferred_drop::degradation()
}

/// 仅诊断用的尾部 drain 失败，不影响任何 tick 提交。
#[cfg(feature = "verification-harness")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum DrainFailure {
    Timeout,
    WorkerDisconnected,
    ProducerStillActive,
    LedgerInvalid(u8),
    InvalidAcknowledgement,
}

/// Producer 已经退出后，由同一 FIFO 队列发 marker 并等待执行回执。
/// 在 marker 前后登记数变化或仍有未完成 id，都拒绝称为完整 drain。
#[cfg(feature = "verification-harness")]
fn drain_running(
    sender: &SyncSender<Job>,
    metrics: &super::verify_metrics::VerifyMetrics,
    timeout: std::time::Duration,
) -> Result<super::verify_metrics::VerifySnapshot, DrainFailure> {
    let deadline = crate::clock::Instant::now() + timeout;
    let before = metrics
        .snapshot()
        .map_err(|e| DrainFailure::LedgerInvalid(e.code))?;
    if before.actor_producers != 0 {
        return Err(DrainFailure::ProducerStillActive);
    }
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    let mut marker: Job = Box::new(move || {
        // 请求方 deadline 已结束时接收端会关闭；这里只是验证回执被取消，不是交易失败。
        let _ = ack_tx.send(());
    });
    loop {
        if crate::clock::Instant::now() >= deadline {
            return Err(DrainFailure::Timeout);
        }
        match sender.try_send(marker) {
            Ok(()) => break,
            Err(mpsc::TrySendError::Full(returned)) => {
                marker = returned;
                std::hint::spin_loop();
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(DrainFailure::WorkerDisconnected)
            }
        }
    }
    let remaining = deadline.saturating_duration_since(crate::clock::Instant::now());
    match libos::wait::recv_deadline(&ack_rx, remaining, false) {
        Ok(()) => {}
        Err(libos::wait::WaitFailure::Deadline { .. }) => return Err(DrainFailure::Timeout),
        Err(libos::wait::WaitFailure::Disconnected | libos::wait::WaitFailure::HostAborted) => {
            return Err(DrainFailure::WorkerDisconnected);
        }
        Err(_) => return Err(DrainFailure::InvalidAcknowledgement),
    }
    let after = metrics
        .snapshot()
        .map_err(|e| DrainFailure::LedgerInvalid(e.code))?;
    if before.registered != after.registered || after.actor_producers != 0 || !after.all_ids_paired
    {
        return Err(DrainFailure::ProducerStillActive);
    }
    Ok(after)
}

#[cfg(feature = "verification-harness")]
#[doc(hidden)]
pub fn drain(
    timeout: std::time::Duration,
) -> Result<super::verify_metrics::VerifySnapshot, DrainFailure> {
    libos::deferred_drop::with_verification_sender(|sender| {
        if let Some(sender) = sender {
            return drain_running(sender, &super::verify_metrics::METRICS, timeout);
        }
        let snap = super::verify_metrics::METRICS
            .snapshot()
            .map_err(|e| DrainFailure::LedgerInvalid(e.code))?;
        if snap.actor_producers == 0 && snap.all_ids_paired {
            Ok(snap)
        } else {
            Err(DrainFailure::ProducerStillActive)
        }
    })
}

// D5 wasm 方向 A：deferred_drop 测试面依赖专用线程 backend（wasm 宿主
// 无 OS 线程）——wasm 下不编译。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use libos::deferred_drop::{TestBackend, QUEUE_CAPACITY};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    #[test]
    #[cfg(feature = "verification-harness")]
    fn fifo_barrier_acknowledges_an_actual_earlier_job() {
        let metrics = super::super::verify_metrics::VerifyMetrics::new();
        let (sender, receiver) = mpsc::sync_channel::<Job>(2);
        let dropped = Arc::new(AtomicUsize::new(0));
        let probe = CountingProbe(dropped.clone());
        sender.try_send(Box::new(move || drop(probe))).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                for _ in 0..2 {
                    receiver.recv().unwrap()();
                }
            });
            let snap = drain_running(&sender, &metrics, std::time::Duration::from_secs(1)).unwrap();
            assert!(snap.all_ids_paired);
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    #[cfg(feature = "verification-harness")]
    fn fifo_barrier_rejects_a_late_registration_even_when_all_ids_completed() {
        use super::super::verify_metrics::{JobKind, Outcome, Tracked, VerifyMetrics};
        let metrics = Arc::new(VerifyMetrics::new());
        let worker_metrics = metrics.clone();
        let (sender, receiver) = mpsc::sync_channel::<Job>(2);
        sender
            .try_send(Box::new(move || {
                let (value, ticket) = Tracked::new((), worker_metrics.as_ref(), JobKind::Other);
                drop(value);
                ticket.outcome(Outcome::Queued);
            }))
            .unwrap();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                // 等待 marker 入队才执行前一个 Job，确保注册发生在 before-snapshot 后。
                let earlier = receiver.recv().unwrap();
                let marker = receiver.recv().unwrap();
                earlier();
                marker();
            });
            assert!(matches!(
                drain_running(&sender, metrics.as_ref(), std::time::Duration::from_secs(1)),
                Err(DrainFailure::ProducerStillActive)
            ));
        });
        assert!(
            metrics.snapshot().unwrap().all_ids_paired,
            "仅看到计数相等会误验收，marker必须同时拒绝迟到注册"
        );
    }

    #[test]
    #[cfg(feature = "verification-harness")]
    fn fifo_barrier_rejects_disconnected_worker_and_live_producer() {
        let metrics = super::super::verify_metrics::VerifyMetrics::new();
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        drop(receiver);
        assert!(matches!(
            drain_running(&sender, &metrics, std::time::Duration::from_millis(100)),
            Err(DrainFailure::WorkerDisconnected)
        ));
        metrics.actor_producers.store(1, Ordering::Release);
        assert!(matches!(
            drain_running(&sender, &metrics, std::time::Duration::from_millis(100)),
            Err(DrainFailure::ProducerStillActive)
        ));
    }

    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn defer_runs_on_dedicated_thread_and_counts() {
        struct Probe {
            flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                self.flag.store(true, Ordering::SeqCst);
            }
        }
        let before = deferred_count();
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        defer(Probe { flag: flag.clone() });
        // 专用线程尽快执行；轮询等待而非 sleep。
        let mut spins = 0;
        while !flag.load(Ordering::SeqCst) && spins < 10_000_000 {
            spins += 1;
            std::hint::spin_loop();
        }
        assert!(flag.load(Ordering::SeqCst), "析构任务未在专用线程执行");
        assert!(deferred_count() > before);
    }

    struct CountingProbe(Arc<AtomicUsize>);
    impl Drop for CountingProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn full_queue_distinguishes_attempts_from_queued_and_inline_release() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let backend = TestBackend::running(sender);
        let mut queued = 0;
        let mut synchronous = 0;
        for _ in 0..QUEUE_CAPACITY + 1 {
            let probe = CountingProbe(dropped.clone());
            match backend.dispatch(Box::new(move || drop(probe))) {
                Submission::Queued => queued += 1,
                Submission::FullInline => synchronous += 1,
                Submission::Inline
                | Submission::DisconnectedInline
                | Submission::SpawnFailedInline => {
                    panic!("受控 receiver 不应退出或启动失败")
                }
            }
        }
        assert_eq!(queued, QUEUE_CAPACITY);
        assert_eq!(synchronous, 1);
        // 队列满是正常背压：不置位退化诊断。
        assert_eq!(backend.degradation(), None);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let jobs: Vec<_> = receiver.try_iter().collect();
        assert_eq!(jobs.len(), QUEUE_CAPACITY);
        for job in jobs {
            job();
        }
        assert_eq!(dropped.load(Ordering::SeqCst), QUEUE_CAPACITY + 1);
    }

    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn exited_worker_releases_the_value_synchronously_once() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        std::thread::spawn(move || drop(receiver)).join().unwrap();
        let backend = TestBackend::running(sender);
        let probe = CountingProbe(dropped.clone());
        assert_eq!(
            backend.dispatch(Box::new(move || drop(probe))),
            Submission::DisconnectedInline
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        // 首次断连置位 UnexpectedDisconnected；幂等重复置位不改变诊断语义。
        assert_eq!(
            backend.degradation(),
            Some(DeferredDropDegradation::UnexpectedDisconnected)
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    /// 有界队列满时 defer 不阻塞、不 panic——退化路径由 try_send 的 Err
    /// 语义结构性保证（闭包随 Err 一起 drop，值就地析构）。此测试验证
    /// 大量并发 defer 调用不会死锁或溢出。
    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn many_rapid_defers_do_not_block_or_panic() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let dropped = Arc::new(AtomicUsize::new(0));
        let total = QUEUE_CAPACITY * 4;
        for _ in 0..total {
            defer(CountingProbe(dropped.clone()));
        }
        let mut spins = 0;
        while dropped.load(Ordering::SeqCst) < total && spins < 10_000_000 {
            spins += 1;
            std::hint::spin_loop();
        }
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            total,
            "defer 不应阻塞或丢值；已析构 {}/{}",
            dropped.load(Ordering::SeqCst),
            total
        );
    }

    /// usize::MAX 栈申请会被 OS 真实拒绝（非模拟错误）：负控的 errno 来自真实失败。
    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn impossible_stack_request_is_a_real_os_spawn_failure() {
        let error = std::thread::Builder::new()
            .name("probe-impossible-stack".to_owned())
            .stack_size(usize::MAX)
            .spawn(|| ())
            .expect_err("usize::MAX 栈申请必须被 OS 拒绝");
        assert!(
            error.raw_os_error().is_some(),
            "真实 OS 失败必须携带 errno：{error:?}"
        );
    }

    /// 真实 spawn 失败（生产同款 backend_from_spawn）→ 持久退化：
    /// 值同步释放恰一次、不计入队、诊断带真实 errno、后续 defer 不重试。
    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn spawn_failed_backend_releases_once_and_reports_errno_without_retry() {
        let real = std::thread::Builder::new()
            .name("probe-impossible-stack".to_owned())
            .stack_size(usize::MAX)
            .spawn(|| ())
            .expect_err("usize::MAX 栈申请必须被 OS 拒绝");
        let os_errno = real.raw_os_error();
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let built = TestBackend::from_spawn_failure(real, sender);
        assert_eq!(
            built.degradation(),
            Some(DeferredDropDegradation::SpawnFailed { os_errno })
        );
        drop(receiver);
        let dropped = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let probe = CountingProbe(dropped.clone());
            assert_eq!(
                built.dispatch(Box::new(move || drop(probe))),
                Submission::SpawnFailedInline,
                "创建失败的同一 backend 不能成功入队或自动重试"
            );
        }
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            2,
            "两个真实派发值都已同步释放"
        );
        assert_eq!(
            built.degradation(),
            Some(DeferredDropDegradation::SpawnFailed { os_errno })
        );
    }

    /// 在途线程退出（receiver 已 drop）→ 断连退化：恰一次释放 + 诊断可见；
    /// 后续同 backend 的 defer 仍是同步释放。
    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn disconnected_backend_releases_once_and_reports_unexpected_disconnected() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        std::thread::spawn(move || drop(receiver)).join().unwrap();
        let before = deferred_count();
        let ((), degradation) = controlled::with_running_backend(
            sender,
            |_| {},
            || {
                defer(CountingProbe(dropped.clone()));
            },
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(deferred_count(), before);
        assert_eq!(
            degradation,
            Some(DeferredDropDegradation::UnexpectedDisconnected)
        );
    }

    /// 队列满（受控容量 1）→ 正常背压：不产生任何退化诊断。
    #[test]
    #[cfg(all(test, not(target_family = "wasm")))]
    fn full_queue_backpressure_is_not_a_degradation() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(1);
        let ((), degradation) = controlled::with_running_backend(
            sender,
            |_| {},
            || {
                defer(CountingProbe(dropped.clone()));
                defer(CountingProbe(dropped.clone()));
            },
        );
        drop(receiver);
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
        assert_eq!(degradation, None, "FullInline 是正常背压，不算退化");
    }
}
