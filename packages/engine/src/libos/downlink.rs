//! Worker 成对下行通道；发送准入与 receiver 同时创建。
use super::admission::{Admission, AdmissionWaitError, ClockSampler};
use super::fault_domain::{FaultDomain, RegisterError};
use super::mailbox;
use std::num::NonZeroUsize;
use std::cell::RefCell;
use std::sync::{mpsc, Arc};

pub(crate) struct Sender<T> {
    inner: mailbox::Sender<T>,
    admission: Arc<Admission>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone(), admission: self.admission.clone() }
    }
}

impl<T> Sender<T> {
    pub(crate) fn stop(&self) {
        self.admission.close();
        self.inner.wake();
    }
    pub(crate) fn send(&self, message: T) -> Result<(), mpsc::SendError<T>> {
        self.send_with_hook(message, || {})
    }

    #[cfg(test)]
    pub(crate) fn send_with_test_hook(&self, message: T, after_permit: impl FnOnce()) -> Result<(), mpsc::SendError<T>> {
        self.send_with_fault(message, || false, after_permit)
    }

    #[inline]
    fn send_with_hook(&self, message: T, after_check: impl FnOnce()) -> Result<(), mpsc::SendError<T>> {
        self.send_with_fault(message, super::fault_domain::instance_failed, after_check)
    }

    #[inline]
    fn send_with_fault(&self, message: T, instance_failed: impl FnOnce() -> bool, after_check: impl FnOnce()) -> Result<(), mpsc::SendError<T>> {
        let Some(_permit) = self.admission.acquire() else { return Err(mpsc::SendError(message)); };
        // 故障前已检查并准入的发送仍可能完成，清理方必须等待许可收束。
        if instance_failed() || self.inner.is_aborted() { return Err(mpsc::SendError(message)); }
        after_check();
        self.inner.send_admitted(message)
    }
}

pub(crate) struct Receiver<T> {
    inner: mailbox::Receiver<T>,
    admission: Arc<Admission>,
}

fn pair<T>((sender, receiver): (mailbox::Sender<T>, mailbox::Receiver<T>)) -> (Sender<T>, Receiver<T>) {
    let admission = Arc::new(Admission::new());
    (Sender { inner: sender, admission: admission.clone() }, Receiver { inner: receiver, admission })
}

pub(crate) fn channel<T>() -> (Sender<T>, Receiver<T>) { pair(mailbox::channel()) }

pub(crate) fn registered_channel<T>(domain: &Arc<FaultDomain>) -> Result<(Sender<T>, Receiver<T>), RegisterError> {
    domain.channel().map(pair)
}

impl<T> Receiver<T> {
    pub(super) fn close_admission(&self) { self.admission.close(); }
    pub(crate) fn run<E>(self, worker: impl FnOnce(&Self) -> Result<(), E>) -> super::worker_exit::Exit<Self, E> {
        super::worker_exit::run(self, worker)
    }

    pub(crate) fn receive(&self, shutdown: &std::sync::atomic::AtomicBool) -> Result<Option<T>, super::worker_wait::WorkerReceiveError> {
        super::worker_wait::receive(&self.inner, shutdown, super::fault_domain::instance_failed)
    }

    pub(crate) fn try_receive(&self) -> Result<Result<T, mpsc::TryRecvError>, super::worker_wait::WorkerTryReceiveError> {
        super::worker_wait::try_receive(&self.inner, super::fault_domain::instance_failed)
    }

    /// 仅表示发送准入已收束，不表示消息已处理或资产已对账。
    pub(crate) fn close_and_wait(&self, deadline: crate::clock::Instant) -> Result<Closed<'_, T>, AdmissionWaitError> {
        self.close_and_wait_with_clock(deadline, crate::clock::Instant::now)
    }

    fn close_and_wait_with_clock(&self, deadline: crate::clock::Instant, now: impl Fn() -> crate::clock::Instant) -> Result<Closed<'_, T>, AdmissionWaitError> {
        self.admission.close();
        let sampler = self.admission.wait_closed_with_sampler(deadline, now)?;
        Ok(Closed { receiver: self, deadline, sampler: RefCell::new(sampler) })
    }
}

impl<T> Drop for Receiver<T> {
    // Drop 不等待或回收队列；调用方必须先完成显式清理。
    fn drop(&mut self) { self.admission.close(); }
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
impl<T> Receiver<T> {
    /// 验证专用只读观测：返回内部 mailbox 的真实 wait word 地址。
    /// 宿主只能读取 PARKED 状态，停止及故障仍须经 LibOS typed API 发布。
    pub(crate) fn verification_wait_word_address(&self) -> usize {
        self.inner.abort_handle().wait_word_address()
    }
}

pub(crate) struct Closed<'a, T> {
    receiver: &'a Receiver<T>,
    deadline: crate::clock::Instant,
    // sampler 与首次清理采样同属一个执行域；ClockSampler 不可 Send，避免
    // 取得 Closed 后再移交到其他 Worker 并将两域读数误判为真实回退。
    sampler: RefCell<ClockSampler>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DrainState {
    Drained,
    BudgetExhausted,
    TimedOut,
    ClockRegressed { previous: crate::clock::Instant, observed: crate::clock::Instant },
    /// 同一 `Closed` 正在执行另一轮 drain；本次未取消息，借用释放后可重试。
    SamplerBusy,
}

pub(crate) struct Drain<T> {
    pub(crate) messages: Vec<T>,
    pub(crate) state: DrainState,
}

impl<T> Closed<'_, T> {
    /// 移交队列中的消息，不执行消息或推断资产归属。
    pub(crate) fn drain(&self, budget: NonZeroUsize) -> Drain<T> {
        self.drain_with_clock(budget, crate::clock::Instant::now)
    }

    fn drain_with_clock(&self, budget: NonZeroUsize, now: impl Fn() -> crate::clock::Instant) -> Drain<T> {
        self.drain_with_hook(budget, now, || {})
    }

    fn drain_with_hook(&self, budget: NonZeroUsize, now: impl Fn() -> crate::clock::Instant, after_message: impl Fn()) -> Drain<T> {
        let mut messages = Vec::new();
        let Ok(mut sampler) = self.sampler.try_borrow_mut() else {
            return Drain { messages, state: DrainState::SamplerBusy };
        };
        let mut check_clock = || {
            let observed = match self.receiver.admission.sample_clock(&mut *sampler, &now) {
                Ok(observed) => observed,
                Err(AdmissionWaitError::ClockRegressed { previous, observed }) => return Some(DrainState::ClockRegressed { previous, observed }),
                Err(_) => unreachable!("采样时钟不会报告准入状态或超时"),
            };
            (observed >= self.deadline).then_some(DrainState::TimedOut)
        };
        while messages.len() < budget.get() {
            if let Some(state) = check_clock() {
                return Drain { messages, state };
            }
            match self.receiver.inner.try_recv() {
                Ok(message) => { messages.push(message); after_message(); },
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {
                    let state = check_clock().unwrap_or(DrainState::Drained);
                    return Drain { messages, state };
                }
            }
        }
        Drain { messages, state: DrainState::BudgetExhausted }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn 两个已准入发送均完成前不得报告收束() {
        use std::time::Duration;
        use crate::clock::Instant;
        let (sender, receiver) = super::channel();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (first_tx, first_rx) = std::sync::mpsc::channel();
        let (second_tx, second_rx) = std::sync::mpsc::channel();
        let first_sender = sender.clone();
        let first_entered = entered_tx.clone();
        let first = std::thread::spawn(move || first_sender.send_with_fault(1, || false, || {
            first_entered.send(()).unwrap();
            first_rx.recv().unwrap();
        }));
        let second = std::thread::spawn(move || sender.send_with_fault(2, || false, || {
            entered_tx.send(()).unwrap();
            second_rx.recv().unwrap();
        }));
        entered_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        entered_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        assert!(matches!(receiver.close_and_wait(Instant::now() + Duration::from_millis(10)), Err(super::AdmissionWaitError::TimedOut)));
        first_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        assert!(matches!(receiver.close_and_wait(Instant::now() + Duration::from_millis(10)), Err(super::AdmissionWaitError::TimedOut)));
        second_tx.send(()).unwrap();
        second.join().unwrap().unwrap();
        let closed = receiver.close_and_wait(Instant::now() + Duration::from_secs(1)).unwrap();
        let result = closed.drain(std::num::NonZeroUsize::new(3).unwrap());
        assert_eq!(result.state, super::DrainState::Drained);
        assert_eq!(result.messages, [1, 2]);
    }
    #[test]
    fn 排空重入时报告忙碌且不重复移交消息() {
        use std::cell::RefCell;
        use std::time::Duration;
        use crate::clock::Instant;
        let (sender, receiver) = super::channel();
        sender.send(7).unwrap();
        let closed = receiver.close_and_wait(Instant::now() + Duration::from_secs(1)).unwrap();
        let nested = RefCell::new(None);
        let outer = closed.drain_with_hook(std::num::NonZeroUsize::new(2).unwrap(), Instant::now, || {
            *nested.borrow_mut() = Some(closed.drain(std::num::NonZeroUsize::new(2).unwrap()));
        });
        assert_eq!(outer.state, super::DrainState::Drained);
        assert_eq!(outer.messages, [7]);
        let nested = nested.into_inner().unwrap();
        assert_eq!(nested.state, super::DrainState::SamplerBusy);
        assert!(nested.messages.is_empty());
        assert_eq!(closed.drain(std::num::NonZeroUsize::new(2).unwrap()).state, super::DrainState::Drained);
    }
    #[test]
    fn 准入收束与首次排空之间时钟回退保留消息() {
        let (sender, receiver) = super::channel();
        sender.send(1).unwrap();
        let start = crate::clock::Instant::now() + std::time::Duration::from_millis(2);
        let closed = receiver.close_and_wait_with_clock(start + std::time::Duration::from_secs(1), || start).unwrap();
        let earlier = start - std::time::Duration::from_millis(1);
        let result = closed.drain_with_clock(std::num::NonZeroUsize::new(2).unwrap(), || earlier);
        assert_eq!(result.state, super::DrainState::ClockRegressed { previous: start, observed: earlier });
        assert!(result.messages.is_empty());
        assert_eq!(closed.drain_with_clock(std::num::NonZeroUsize::new(2).unwrap(), || start).messages, [1]);
    }
    #[test]
    fn 不同排空批次之间时钟回退不能报告完成() {
        let (sender, receiver) = super::channel();
        sender.send(1).unwrap();
        let start = crate::clock::Instant::now() + std::time::Duration::from_millis(2);
        let closed = receiver.close_and_wait_with_clock(start + std::time::Duration::from_secs(1), || start).unwrap();
        let first = closed.drain_with_clock(std::num::NonZeroUsize::new(1).unwrap(), || start);
        assert_eq!(first.messages, [1]);
        let earlier = start - std::time::Duration::from_millis(1);
        let retry = receiver.close_and_wait_with_clock(start + std::time::Duration::from_secs(1), || start).unwrap();
        let second = retry.drain_with_clock(std::num::NonZeroUsize::new(1).unwrap(), || earlier);
        assert_eq!(second.state, super::DrainState::ClockRegressed { previous: start, observed: earlier });
    }
    #[test]
    fn 排空期间时钟回退必须保留已取消息及队尾() {
        let (sender, receiver) = super::channel();
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        let start = crate::clock::Instant::now() + std::time::Duration::from_millis(2);
        let closed = receiver.close_and_wait_with_clock(start + std::time::Duration::from_secs(1), || start).unwrap();
        let calls = std::cell::Cell::new(0);
        let batch = closed.drain_with_clock(std::num::NonZeroUsize::new(4).unwrap(), || {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 { start } else { start - std::time::Duration::from_millis(1) }
        });
        assert_eq!(batch.state, super::DrainState::ClockRegressed {
            previous: start,
            observed: start - std::time::Duration::from_millis(1),
        });
        assert_eq!(batch.messages, [1]);
        assert_eq!(closed.drain_with_clock(std::num::NonZeroUsize::new(4).unwrap(), || start).messages, [2]);
    }
    #[test]
    fn 永久停止后立即拒绝新消息并保留已发送消息() {
        use crate::libos::wake::WorkerWake;
        let (sender, receiver) = super::channel();
        sender.send(7).unwrap();
        sender.wake_for_stop();
        assert_eq!(sender.send(8).unwrap_err().0, 8);
        let closed = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        let drained = closed.drain(std::num::NonZeroUsize::new(3).unwrap());
        assert_eq!(drained.state, super::DrainState::Drained);
        assert_eq!(drained.messages, [7]);
    }

    #[test]
    fn 永久停止等待已准入发送完成后才能排空() {
        use crate::libos::wake::WorkerWake;
        use std::time::Duration;
        use crate::clock::Instant;
        let (sender, receiver) = super::channel();
        let worker_sender = sender.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || worker_sender.send_with_fault(7, || false, || {
            entered_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        }));
        entered_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        sender.wake_for_stop();
        assert_eq!(sender.send(8).unwrap_err().0, 8);
        assert!(matches!(receiver.close_and_wait(Instant::now() + Duration::from_millis(10)), Err(super::AdmissionWaitError::TimedOut)));
        resume_tx.send(()).unwrap();
        worker.join().unwrap().unwrap();
        let closed = receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
        let drained = closed.drain(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(drained.state, super::DrainState::Drained);
        assert_eq!(drained.messages, [7]);
    }

    #[test]
    fn 永久停止唤醒不向资源队列插入控制消息() {
        use crate::libos::wake::WorkerWake;
        let (sender, receiver) = super::channel();
        sender.send(7).unwrap();
        sender.wake_for_stop();
        let closed = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        let batch = closed.drain(std::num::NonZeroUsize::new(3).unwrap());
        assert_eq!(batch.state, super::DrainState::Drained);
        assert_eq!(batch.messages, [7]);
    }
    #[test]
    fn 实例故障已发布但故障域尚未标记时拒绝发送() {
        let domain = super::FaultDomain::new(1);
        let (sender, receiver) = super::registered_channel(&domain).unwrap();
        let failed = std::sync::atomic::AtomicBool::new(true);
        let error = sender.send_with_fault(42, || failed.load(std::sync::atomic::Ordering::SeqCst), || {}).unwrap_err();
        assert_eq!(error.0, 42);
        let closed = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        assert!(closed.drain(std::num::NonZeroUsize::new(1).unwrap()).messages.is_empty());
    }
    #[test]
    fn 故障前已检查的发送允许完成但清理不能提前排空() {
        use std::time::Duration;
        use crate::clock::Instant;
        let domain = super::FaultDomain::new(1);
        let (sender, receiver) = super::registered_channel(&domain).unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || sender.send_with_hook(7, || {
            entered_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        }));
        entered_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        domain.abort();
        assert!(matches!(receiver.close_and_wait(Instant::now() + Duration::from_millis(10)), Err(super::AdmissionWaitError::TimedOut)));
        resume_tx.send(()).unwrap();
        worker.join().unwrap().unwrap();
        let closed = receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
        let batch = closed.drain(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(batch.state, super::DrainState::Drained);
        assert_eq!(batch.messages, [7]);
    }
    #[test]
    fn 故障广播后等待_worker_返回期间拒绝新载荷() {
        let domain = super::FaultDomain::new(1);
        let (sender, receiver) = super::registered_channel(&domain).unwrap();
        sender.send(String::from("故障前载荷")).unwrap();
        domain.abort();
        let error = sender.send(String::from("故障后载荷")).unwrap_err();
        assert_eq!(error.0, "故障后载荷");
        let closed = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        let batch = closed.drain(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(batch.state, super::DrainState::Drained);
        assert_eq!(batch.messages, ["故障前载荷"]);
    }
    #[test]
    fn 排空中途超时返回已取消息并保留队尾() {
        let (sender, receiver) = super::channel();
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        let deadline = crate::clock::Instant::now() + std::time::Duration::from_secs(1);
        let closed = receiver.close_and_wait_with_clock(deadline, || deadline - std::time::Duration::from_millis(1)).unwrap();
        let calls = std::cell::Cell::new(0);
        let batch = closed.drain_with_clock(std::num::NonZeroUsize::new(4).unwrap(), || {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 { deadline - std::time::Duration::from_millis(1) } else { deadline }
        });
        assert_eq!(batch.state, super::DrainState::TimedOut);
        assert_eq!(batch.messages, [1]);
        let retry = receiver.close_and_wait_with_clock(deadline + std::time::Duration::from_secs(1), || deadline).unwrap();
        assert_eq!(retry.drain_with_clock(std::num::NonZeroUsize::new(4).unwrap(), || deadline).messages, [2]);
    }
}
