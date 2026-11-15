//! 测试专用：分别观察提交返回和可确定等待的析构完成边界。
use super::{defer, Submission};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

#[derive(Debug, PartialEq, Eq)]
pub struct ReclamationObservation {
    pub submissions: [Submission; 2],
    pub completed_on_return: [u32; 2],
    pub completed_at_boundary: [u32; 2],
}

struct Probe(Arc<AtomicU32>);

impl Drop for Probe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// 两端调用相同的生产 `defer`；Native 用真实队列栏杆固定取样顺序，
/// WASM 直接以调用返回为完成边界。两次计数不代表队列行为相同。
pub fn verify_reclamation() -> Result<ReclamationObservation, &'static str> {
    #[cfg(not(target_family = "wasm"))]
    let gate = native_boundary::Gate::install()?;

    let first = Arc::new(AtomicU32::new(0));
    let second = Arc::new(AtomicU32::new(0));
    let snapshot = || [first.load(Ordering::SeqCst), second.load(Ordering::SeqCst)];
    let submissions = [defer(Probe(first.clone())), defer(Probe(second.clone()))];
    let completed_on_return = snapshot();

    #[cfg(not(target_family = "wasm"))]
    gate.release_and_await_completion()?;

    Ok(ReclamationObservation {
        submissions,
        completed_on_return,
        completed_at_boundary: snapshot(),
    })
}

#[cfg(not(target_family = "wasm"))]
mod native_boundary {
    use super::Submission;
    use crate::deferred_drop::backend::native::defer_job;
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    use std::time::Duration;

    const BUDGET: Duration = Duration::from_secs(2);

    /// 夹具失败时丢弃 Sender 即撤销，实线队列不会被测试永久占住。
    pub(super) struct Gate {
        release: SyncSender<()>,
        released: Receiver<bool>,
    }

    fn signal<T>(tx: &SyncSender<T>, value: T) {
        match tx.try_send(value) {
            Ok(()) => {}
            // 验证端已按期限退出，消费者不能为陈旧回执阻塞。
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {}
            // 容量是 1，每个回执只发一次。
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                unreachable!("析构验证回执重复")
            }
        }
    }

    impl Gate {
        pub(super) fn install() -> Result<Self, &'static str> {
            let (entered_tx, entered) = sync_channel(1);
            let (release, release_rx) = sync_channel(1);
            let (released_tx, released) = sync_channel(1);
            let outcome = defer_job(Box::new(move || {
                signal(&entered_tx, ());
                signal(&released_tx, release_rx.recv_timeout(BUDGET).is_ok());
            }));
            if outcome != Submission::Queued {
                return Err("栏杆未进入真实 Native 析构队列");
            }
            entered
                .recv_timeout(BUDGET)
                .map_err(|_| "真实 Native 析构消费者未抵达栏杆")?;
            Ok(Self { release, released })
        }

        pub(super) fn release_and_await_completion(self) -> Result<(), &'static str> {
            let (done_tx, done) = sync_channel(1);
            let outcome = defer_job(Box::new(move || signal(&done_tx, ())));
            if outcome != Submission::Queued {
                return Err("完成 marker 未进入真实 Native 析构队列");
            }
            self.release
                .try_send(())
                .map_err(|_| "真实 Native 析构栏杆已撤销或超时")?;
            if self.released.recv_timeout(BUDGET) != Ok(true) {
                return Err("真实 Native 析构栏杆未正常释放");
            }
            done.recv_timeout(BUDGET)
                .map_err(|_| "真实 Native 析构未到达 FIFO 完成 marker")
        }
    }
}
