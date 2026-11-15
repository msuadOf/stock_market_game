//! 两端共用的有界后台析构队列；只有宿主 owner 的生命周期不同。
//! 任务只移交所有权，不执行重试，不参与调用方的提交或诊断政策。
//!
//! **等待点账（设计 §11.1）**：共用的 `sync_channel(64)` 是 mpmc array
//! flavor，提交面只用 `try_send`。等待点为 (o) 在途槽发布、(p) 消费线程
//! 空队列 `recv()`、(q) 末 Receiver Drop 等在途 sender 结束。队列满
//! 由调用线程释放值，不等待入队；阻塞 send 不属于生产提交面。
//! Backoff 无超时，生产错误处理不凭空释放已入队任务；验证端的
//! FIFO marker 与 deadline 等待不是 WASM 生产等待点。

#[cfg(any(test, feature = "verification-harness"))]
mod verification;
#[cfg(any(test, feature = "verification-harness"))]
#[doc(hidden)]
pub use verification::{verify_reclamation, ReclamationObservation};

/// 后台析构后端的退化诊断（Copy、数值化；字符串只允许在宿主边界生成）。
///
/// 只描述进程级后端状态，不含任何调用方信息；`FullInline`（队列满背压）
/// 是正常行为，不出现在此诊断中。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferredDropDegradation {
    /// 专用线程创建失败。`os_errno` 为 OS 错误码（如 ENOMEM/EAGAIN）；
    /// 之后所有 `defer` 都同步释放，不再重试创建线程（无自动重试）。
    SpawnFailed { os_errno: Option<i32> },
    /// 专用线程曾在途但已退出（receiver 断开）；之后的 `defer` 转为同步释放。
    UnexpectedDisconnected,
}

/// 本次提交结束时，析构的所有权去向。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Submission {
    Queued,
    /// 旧 WASM 证据的分类；当前两端后台路径均不产生该值。
    Inline,
    /// 两端队列满，正常背压，不置位退化诊断。
    FullInline,
    DisconnectedInline,
    SpawnFailedInline,
}

#[cfg(not(target_family = "wasm"))]
pub use backend::native::{defer, degradation};

#[cfg(target_family = "wasm")]
mod wasm_owner;
#[cfg(target_family = "wasm")]
pub use wasm_owner::{close_wasm_reclaimer, defer, degradation, start_wasm_reclaimer,
    CloseError, RejectReason, StartError, try_defer};

#[cfg(feature = "verification-harness")]
#[doc(hidden)]
pub use backend::{Job, TestBackend, QUEUE_CAPACITY};
#[cfg(all(feature = "verification-harness", not(target_family = "wasm")))]
#[doc(hidden)]
pub use backend::native::{defer_job, verification_sender};
#[cfg(all(feature = "verification-harness", target_family = "wasm"))]
#[doc(hidden)]
pub use wasm_owner::defer_job;

#[cfg(all(feature = "verification-harness", not(target_family = "wasm")))]
pub fn with_verification_sender<R>(observe: impl FnOnce(Option<&std::sync::mpsc::SyncSender<Job>>) -> R) -> R {
    observe(verification_sender())
}

#[cfg(all(feature = "verification-harness", target_family = "wasm"))]
pub use wasm_owner::{with_verification_sender, verify_close_wasm_reclaimer};

mod backend {
    use super::{DeferredDropDegradation, Submission};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, SyncSender};

    pub type Job = Box<dyn FnOnce() + Send>;
    // 只限制等待 Job 数，不限制保留字节。
    pub const QUEUE_CAPACITY: usize = 64;

    pub(super) struct RunningBackend {
        pub(super) sender: SyncSender<Job>,
        /// 首次 `DisconnectedInline` 后置位（幂等；此后在途线程已不可用）。
        disconnected: AtomicBool,
    }

    impl RunningBackend {
        pub(super) fn new(sender: SyncSender<Job>) -> Self {
            Self {
                sender,
                disconnected: AtomicBool::new(false),
            }
        }
    }

    /// 进程级后端状态：在途线程，或"创建失败"的持久退化（不重试）。
    #[cfg(any(not(target_family = "wasm"), feature = "verification-harness", test))]
    enum Backend {
        Running(RunningBackend),
        SpawnFailed { os_errno: Option<i32> },
    }

    pub(super) fn spawn_consumer() -> std::io::Result<(RunningBackend, crate::thread_facade::WorkerHandle<()>)> {
        let (tx, rx) = mpsc::sync_channel::<Job>(QUEUE_CAPACITY);
        let handle = crate::thread_facade::WorkerHandle::spawn("engine-deferred-drop".to_owned(), move || {
            while let Ok(job) = rx.recv() {
                job();
            }
        })?;
        Ok((RunningBackend::new(tx), handle))
    }

    #[cfg(not(target_family = "wasm"))]
    pub(super) mod native {
        use super::*;
        use std::sync::OnceLock;

        static BACKEND: OnceLock<Backend> = OnceLock::new();

        fn backend() -> &'static Backend {
            BACKEND.get_or_init(|| match spawn_consumer() {
                Ok((running, _handle)) => Backend::Running(running),
                Err(error) => Backend::SpawnFailed { os_errno: error.raw_os_error() },
            })
        }

        pub fn defer<T: Send + 'static>(value: T) -> Submission {
            defer_job(Box::new(move || drop(value)))
        }

        #[doc(hidden)]
        pub fn defer_job(job: Job) -> Submission {
            dispatch_on(backend(), job)
        }

        pub fn degradation() -> Option<DeferredDropDegradation> {
            BACKEND.get().and_then(backend_degradation)
        }

        #[cfg(feature = "verification-harness")]
        #[doc(hidden)]
        pub fn verification_sender() -> Option<&'static SyncSender<Job>> {
            match BACKEND.get() {
                Some(Backend::Running(running)) => Some(&running.sender),
                _ => None,
            }
        }
    }

    /// 由真实 spawn 结果构造后端状态（生产与负控共用；失败不 panic、不重试）。
    #[cfg(feature = "verification-harness")]
    fn backend_from_spawn(
        result: std::io::Result<crate::thread_facade::WorkerHandle<()>>,
        sender: SyncSender<Job>,
    ) -> Backend {
        match result {
            Ok(_handle) => Backend::Running(RunningBackend::new(sender)),
            Err(error) => Backend::SpawnFailed {
                os_errno: error.raw_os_error(),
            },
        }
    }

    /// 未入队的值在返回前同步释放；保留真实结果分类，不把内联完成算作 queued。
    #[cfg(any(not(target_family = "wasm"), feature = "verification-harness"))]
    fn dispatch_on(backend: &Backend, job: Job) -> Submission {
        match backend {
            Backend::Running(running) => submit(running, job),
            Backend::SpawnFailed { .. } => {
                drop(job);
                Submission::SpawnFailedInline
            }
        }
    }

    #[cfg(any(not(target_family = "wasm"), feature = "verification-harness", test))]
    fn backend_degradation(backend: &Backend) -> Option<DeferredDropDegradation> {
        match backend {
            Backend::SpawnFailed { os_errno } => Some(DeferredDropDegradation::SpawnFailed {
                os_errno: *os_errno,
            }),
            Backend::Running(running) => {
                running_degradation(running)
            }
        }
    }

    pub(super) fn running_degradation(running: &RunningBackend) -> Option<DeferredDropDegradation> {
        running.disconnected.load(Ordering::Relaxed)
            .then_some(DeferredDropDegradation::UnexpectedDisconnected)
    }

    pub(super) fn submit(running: &RunningBackend, job: Job) -> Submission {
        match running.sender.try_send(job) {
            Ok(()) => Submission::Queued,
            Err(mpsc::TrySendError::Full(job)) => {
                drop(job);
                Submission::FullInline
            }
            Err(mpsc::TrySendError::Disconnected(job)) => {
                running.disconnected.store(true, Ordering::Relaxed);
                drop(job);
                Submission::DisconnectedInline
            }
        }
    }

    /// 受控负例与真实台账验证复用同一派发逻辑，不创建生产开关。
    #[cfg(feature = "verification-harness")]
    #[doc(hidden)]
    pub struct TestBackend(Backend);

    #[cfg(feature = "verification-harness")]
    impl TestBackend {
        pub fn running(sender: SyncSender<Job>) -> Self {
            Self(Backend::Running(RunningBackend::new(sender)))
        }

        pub fn failed(os_errno: Option<i32>) -> Self {
            Self(Backend::SpawnFailed { os_errno })
        }

        pub fn from_spawn_failure(error: std::io::Error, sender: SyncSender<Job>) -> Self {
            Self(backend_from_spawn(Err(error), sender))
        }

        pub fn dispatch(&self, job: Job) -> Submission {
            dispatch_on(&self.0, job)
        }

        pub fn degradation(&self) -> Option<DeferredDropDegradation> {
            backend_degradation(&self.0)
        }
    }

    #[cfg(test)]
    mod contract_tests {
        use super::{backend_degradation, submit, Backend, DeferredDropDegradation, Job};
        use super::{RunningBackend, Submission, QUEUE_CAPACITY};
        use std::sync::{
            atomic::{AtomicU32, Ordering},
            mpsc, Arc,
        };

        struct DropCount(Arc<AtomicU32>);

        impl Drop for DropCount {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        fn counted_job(count: &Arc<AtomicU32>) -> Job {
            let value = DropCount(Arc::clone(count));
            Box::new(move || drop(value))
        }

        #[test]
        fn 有界发送保留排队_满时在返回前内联且不报告退化() {
            let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
            let running = RunningBackend::new(tx);
            let count = Arc::new(AtomicU32::new(0));
            let fifo = Arc::new(AtomicU32::new(0));
            for offset in 0..QUEUE_CAPACITY {
                let fifo = Arc::clone(&fifo);
                let drop_once = counted_job(&count);
                let job = Box::new(move || {
                    assert_eq!(fifo.fetch_add(1, Ordering::SeqCst), offset as u32);
                    drop_once();
                });
                assert_eq!(submit(&running, job), Submission::Queued);
            }
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert_eq!(
                submit(&running, counted_job(&count)),
                Submission::FullInline
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert_eq!(backend_degradation(&Backend::Running(running)), None);
            // 唯一 Sender 已被消费，真实接收端必须排空至断连且无额外任务。
            for (index, job) in rx.into_iter().enumerate() {
                assert!(index < QUEUE_CAPACITY);
                job();
            }
            assert_eq!(fifo.load(Ordering::SeqCst), QUEUE_CAPACITY as u32);
            assert_eq!(count.load(Ordering::SeqCst), QUEUE_CAPACITY as u32 + 1);
        }

        #[test]
        fn 消费者已断开时内联一次并保留真实退化信号() {
            let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
            let running = RunningBackend::new(tx);
            drop(rx);
            let count = Arc::new(AtomicU32::new(0));
            assert_eq!(
                submit(&running, counted_job(&count)),
                Submission::DisconnectedInline,
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert_eq!(
                backend_degradation(&Backend::Running(running)),
                Some(DeferredDropDegradation::UnexpectedDisconnected),
            );
        }
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
#[test]
fn 真实队列在提交返回后恰好完成一次每项析构() {
    let observed = verify_reclamation().expect("真实 Native 析构队列夹具");
    assert_eq!(observed.submissions, [Submission::Queued; 2]);
    assert_eq!(observed.completed_on_return, [0, 0]);
    assert_eq!(observed.completed_at_boundary, [1, 1]);
}
