//! 实验性发送准入：用于验证接收器退出与在途发送的收束边界。

use super::worker_inbox::WorkerSender;
use std::sync::{atomic::Ordering, mpsc, Arc};

use crate::libos::admission::{Admission, AdmissionWaitError};

pub(super) struct GatedSender<S> { inner: S, gate: Arc<Admission> }
impl<S> GatedSender<S> {
    fn new(inner: S) -> (Self, Arc<Admission>) {
        let gate = Arc::new(Admission::new());
        (Self { inner, gate: gate.clone() }, gate)
    }
}
impl<S: Clone> Clone for GatedSender<S> {
    fn clone(&self) -> Self { Self { inner: self.inner.clone(), gate: self.gate.clone() } }
}
impl<T, S: WorkerSender<T>> WorkerSender<T> for GatedSender<S> {
    fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        let Some(_permit) = self.gate.acquire() else { return Err(mpsc::SendError(value)); };
        self.inner.send(value)
    }
    #[cfg(test)]
    fn disconnected_for_test() -> Self { Self::new(S::disconnected_for_test()).0 }
}

/// 仅验证轻量关闭检查；不跟踪在途发送，不能用于判定排空或安全回收。
struct StopCheckedSender<S> { inner: S, closed: Arc<std::sync::atomic::AtomicBool> }
impl<S> StopCheckedSender<S> {
    fn new(inner: S) -> (Self, Arc<std::sync::atomic::AtomicBool>) {
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        (Self { inner, closed: closed.clone() }, closed)
    }
}
impl<T, S: WorkerSender<T>> WorkerSender<T> for StopCheckedSender<S> {
    fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        if self.closed.load(Ordering::Acquire) { return Err(mpsc::SendError(value)); }
        self.inner.send(value)
    }
    #[cfg(test)]
    fn disconnected_for_test() -> Self { Self::new(S::disconnected_for_test()).0 }
}

#[cfg(feature = "verification-harness")]
pub(super) fn benchmark_send_path() {
    fn run<S: WorkerSender<u64> + Send + 'static>(sender: S, receiver: crate::libos::mailbox::Receiver<u64>, count: u64) -> u128 {
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let ready = barrier.clone();
        let child = std::thread::spawn(move || {
            ready.wait();
            for value in 0..count { WorkerSender::send(&sender, value).unwrap(); }
        });
        let start = std::time::Instant::now();
        barrier.wait();
        for expected in 0..count {
            loop {
                match receiver.try_recv() {
                    Ok(value) => { assert_eq!(value, expected); break; }
                    Err(mpsc::TryRecvError::Empty) => std::hint::spin_loop(),
                    Err(mpsc::TryRecvError::Disconnected) => panic!("消息发送提前断连"),
                }
            }
        }
        child.join().unwrap();
        start.elapsed().as_nanos()
    }
    fn plain(count: u64) -> u128 {
        let (sender, receiver) = crate::libos::mailbox::channel();
        run(sender, receiver, count)
    }
    fn gated(count: u64) -> u128 {
        let (sender, receiver) = crate::libos::mailbox::channel();
        run(GatedSender::new(sender).0, receiver, count)
    }
    fn checked(count: u64) -> u128 {
        let (sender, receiver) = crate::libos::mailbox::channel();
        run(StopCheckedSender::new(sender).0, receiver, count)
    }
    let count = 1_000_000;
    plain(20_000);
    gated(20_000);
    checked(20_000);
    for round in 0..6 {
        let (plain_ns, checked_ns, gated_ns) = match round % 3 {
            0 => (plain(count), checked(count), gated(count)),
            1 => { let checked_ns = checked(count); let gated_ns = gated(count); (plain(count), checked_ns, gated_ns) },
            _ => { let gated_ns = gated(count); let plain_ns = plain(count); (plain_ns, checked(count), gated_ns) },
        };
        println!("round={round} count={count} plain_ns={plain_ns} checked_ns={checked_ns} gated_ns={gated_ns}");
    }
}

/// 仅从同一构造器取得 sender，避免复制原始发送端绕过关闭。
pub(super) fn paired_registered_channel<T>(domain: &Arc<crate::libos::fault_domain::FaultDomain>) -> Result<(GatedSender<crate::libos::mailbox::Sender<T>>, PairedReceiver<T>), crate::libos::fault_domain::RegisterError> {
    let (inner, receiver) = domain.channel()?;
    let (sender, gate) = GatedSender::new(inner);
    Ok((sender, PairedReceiver { receiver, gate }))
}

pub(super) struct PairedReceiver<T> {
    receiver: crate::libos::mailbox::Receiver<T>,
    gate: Arc<Admission>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ClosedDrainState {
    /// 准入未关闭或仍有发送许可；本批不移除消息。
    NotQuiescent,
    /// 闭合准入域已收束且当前队列为空；不表示资产已对账。
    Drained,
    /// 达到批次预算，需要再次调用确认队列是否为空。
    BudgetExhausted,
}

pub(super) struct ClosedDrain<T> {
    pub(super) messages: Vec<T>,
    pub(super) state: ClosedDrainState,
}

impl<T> PairedReceiver<T> {
    pub(super) fn run_catching<E>(self, worker: impl FnOnce(&crate::libos::mailbox::Receiver<T>) -> Result<(), E>) -> crate::libos::worker_exit::Exit<Self, E> {
        let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker(&self.receiver))) {
            Ok(result) => crate::libos::worker_exit::Outcome::Returned(result),
            Err(payload) => crate::libos::worker_exit::Outcome::Panicked(payload),
        };
        self.gate.close();
        crate::libos::worker_exit::Exit { outcome, receiver: self }
    }
    pub(super) fn run<E>(self, worker: impl FnOnce(&crate::libos::mailbox::Receiver<T>) -> Result<(), E>) -> (Result<(), E>, Self) {
        let result = worker(&self.receiver);
        self.gate.close();
        (result, self)
    }
    pub(super) fn close(&self) { self.gate.close(); }
    /// 关闭准入后在共同 deadline 内等待已获许可的发送完成。
    pub(super) fn close_and_wait(&self, deadline: crate::clock::Instant) -> Result<(), AdmissionWaitError> {
        self.gate.close();
        self.gate.wait_closed(deadline)
    }
    pub(super) fn is_closed_and_quiescent(&self) -> bool { self.gate.is_closed_and_quiescent() }
    pub(super) fn try_recv(&self) -> Result<T, mpsc::TryRecvError> { self.receiver.try_recv() }
    /// 仅在关闭且无在途许可后分批移交消息，不执行消息或对账。
    pub(super) fn drain_closed(&self, budget: std::num::NonZeroUsize) -> ClosedDrain<T> {
        if !self.gate.is_closed_and_quiescent() {
            return ClosedDrain { messages: Vec::new(), state: ClosedDrainState::NotQuiescent };
        }
        let mut messages = Vec::new();
        while messages.len() < budget.get() {
            match self.receiver.try_recv() {
                Ok(message) => messages.push(message),
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {
                    return ClosedDrain { messages, state: ClosedDrainState::Drained };
                }
            }
        }
        ClosedDrain { messages, state: ClosedDrainState::BudgetExhausted }
    }
}
impl<T> Drop for PairedReceiver<T> {
    // 这里只关闭准入并释放队列，不能代替等待在途发送、排空和资源对账。
    fn drop(&mut self) { self.gate.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::worker_inbox::WorkerSender;
    use std::sync::{mpsc, Arc};

    #[test]
    fn worker_unwind_后关闭准入并保留资金股份消息() {
        use crate::runtime::{DeskBatch, Entity};
        use crate::runtime::protocol::GroupIn;
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        WorkerSender::send(&sender, GroupIn::Batch(DeskBatch {
            stock: 0, tick: 1, round: 0,
            entities: vec![Entity::CashRefund { local: 0, amount: 500 }, Entity::RefundedShares { local: 1, qty: 100 }],
            ..DeskBatch::default()
        })).unwrap();
        let exit = receiver.run_catching(|_| -> Result<(), ()> { std::panic::panic_any(String::from("原始 worker 错误")) });
        match exit.outcome {
            crate::libos::worker_exit::Outcome::Panicked(payload) => assert_eq!(*payload.downcast::<String>().unwrap(), "原始 worker 错误"),
            _ => panic!("必须保留原始 panic"),
        }
        assert!(WorkerSender::send(&sender, GroupIn::Shutdown).is_err());
        let drained = exit.receiver.drain_closed(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(drained.state, ClosedDrainState::Drained);
        assert!(matches!(&drained.messages[..], [GroupIn::Batch(batch)] if batch.entities == [Entity::CashRefund {local:0, amount:500}, Entity::RefundedShares {local:1, qty:100}]));
    }

    #[test]
    fn 线程_panic_后等待已获许可的发送完成再移交载荷() {
        use std::time::{Duration, Instant};
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        let held = sender.gate.acquire().unwrap();
        let worker = std::thread::spawn(move || receiver.run_catching(|_| -> Result<(), ()> {
            std::panic::panic_any(String::from("线程原始错误"));
        }));
        let exit = worker.join().unwrap();
        assert!(matches!(exit.outcome, crate::libos::worker_exit::Outcome::Panicked(_)));
        assert_eq!(WorkerSender::send(&sender, 8).unwrap_err().0, 8);
        assert_eq!(exit.receiver.close_and_wait(Instant::now()), Err(AdmissionWaitError::TimedOut));
        assert_eq!(exit.receiver.drain_closed(std::num::NonZeroUsize::new(2).unwrap()).state, ClosedDrainState::NotQuiescent);
        WorkerSender::send(&sender.inner, 7).unwrap();
        drop(held);
        exit.receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
        let drained = exit.receiver.drain_closed(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(drained.state, ClosedDrainState::Drained);
        assert_eq!(drained.messages, [7]);
        assert!(exit.receiver.drain_closed(std::num::NonZeroUsize::new(2).unwrap()).messages.is_empty());
    }

    #[test]
    fn 关闭后等待已准入发送释放并可按共同期限超时() {
        use std::time::{Duration, Instant};
        let gate = Arc::new(Admission::new());
        let held = gate.acquire().unwrap();
        gate.close();
        assert!(matches!(gate.wait_closed(Instant::now()), Err(super::AdmissionWaitError::TimedOut)));
        assert!(!gate.is_closed_and_quiescent());
        let wait_gate = gate.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let child = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            wait_gate.wait_closed(Instant::now() + Duration::from_millis(500))
        });
        started_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        drop(held);
        assert_eq!(child.join().unwrap(), Ok(()));
        assert!(gate.is_closed_and_quiescent());
    }

    #[test]
    fn 未关闭的准入不能被误报为收束() {
        assert!(matches!(Admission::new().wait_closed(std::time::Instant::now()), Err(super::AdmissionWaitError::NotClosed)));
    }

    #[test]
    fn 接收器关闭后使用同一期限等待发送并保留资金与股份载荷() {
        use crate::runtime::{DeskBatch, Entity};
        use crate::runtime::protocol::GroupIn;
        use std::time::{Duration, Instant};
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        let permit = sender.gate.acquire().unwrap();
        let payload = GroupIn::Batch(DeskBatch { stock:0, tick:1, round:0,
            entities:vec![Entity::CashRefund {local:0, amount:500}, Entity::RefundedShares {local:1, qty:100}], ..DeskBatch::default() });
        WorkerSender::send(&sender.inner, payload).unwrap();
        assert_eq!(receiver.close_and_wait(Instant::now()), Err(AdmissionWaitError::TimedOut));
        assert!(sender.gate.acquire().is_none());
        drop(permit);
        receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
        let drained = receiver.drain_closed(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(drained.state, ClosedDrainState::Drained);
        assert!(matches!(&drained.messages[..], [GroupIn::Batch(batch)] if batch.entities == [Entity::CashRefund {local:0, amount:500}, Entity::RefundedShares {local:1, qty:100}]));
        let again = receiver.drain_closed(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(again.state, ClosedDrainState::Drained);
        assert!(again.messages.is_empty());
    }

    #[test]
    fn 关闭后拒绝新载荷并保留已入队消息() {
        let (inner, receiver) = crate::libos::mailbox::channel();
        let (sender, gate) = GatedSender::new(inner);
        let clone = sender.clone();
        WorkerSender::send(&sender, String::from("先前载荷")).unwrap();
        assert!(!gate.is_closed_and_quiescent());
        gate.close();
        assert!(gate.is_closed_and_quiescent());
        let error = WorkerSender::send(&clone, String::from("被拒载荷")).unwrap_err();
        assert_eq!(error.0, "被拒载荷");
        assert_eq!(receiver.try_recv().unwrap(), "先前载荷");
        assert!(matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn 只读关闭标记的候选拒绝后续载荷() {
        let (inner, receiver) = crate::libos::mailbox::channel();
        let (sender, closed) = StopCheckedSender::new(inner);
        WorkerSender::send(&sender, 1).unwrap();
        closed.store(true, Ordering::Release);
        assert_eq!(WorkerSender::send(&sender, 2).unwrap_err().0, 2);
        assert_eq!(receiver.try_recv(), Ok(1));
        assert!(matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn 只读关闭标记不能证明已有发送已收束() {
        struct Held { entered: mpsc::Sender<()>, resume: Arc<std::sync::Barrier>, inner: mpsc::Sender<u32> }
        impl WorkerSender<u32> for Held {
            fn send(&self, value: u32) -> Result<(), mpsc::SendError<u32>> {
                self.entered.send(()).unwrap();
                self.resume.wait();
                self.inner.send(value)
            }
            fn disconnected_for_test() -> Self { panic!("测试不使用断连构造") }
        }
        let (inner, receiver) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let resume = Arc::new(std::sync::Barrier::new(2));
        let (sender, closed) = StopCheckedSender::new(Held { entered:entered_tx, resume:resume.clone(), inner });
        let child = std::thread::spawn(move || WorkerSender::send(&sender, 42));
        entered_rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap();
        closed.store(true, Ordering::Release);
        assert!(matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)));
        resume.wait();
        child.join().unwrap().unwrap();
        assert_eq!(receiver.try_recv(), Ok(42));
    }

    #[test]
    fn 成对通道正常返回或接收器销毁都关闭所有克隆() {
        let domain = crate::libos::fault_domain::FaultDomain::new(2);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        let clone = sender.clone();
        WorkerSender::send(&sender, 1).unwrap();
        let (result, receiver) = receiver.run(|_| Ok::<(), &str>(()));
        assert_eq!(result, Ok(()));
        assert!(receiver.is_closed_and_quiescent());
        assert_eq!(WorkerSender::send(&clone, 2).unwrap_err().0, 2);
        assert_eq!(receiver.try_recv(), Ok(1));
        drop(receiver);
        assert_eq!(WorkerSender::send(&sender, 3).unwrap_err().0, 3);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        drop(receiver);
        assert_eq!(WorkerSender::send(&sender, 4).unwrap_err().0, 4);
    }

    #[test]
    fn 已关闭且无在途发送时分批移交保留槽和队列() {
        use crate::runtime::{DeskBatch, Entity};
        use crate::runtime::protocol::GroupIn;
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        for amount in [500, 700] {
            WorkerSender::send(&sender, GroupIn::Batch(DeskBatch { stock:0, tick:0, round:0, entities:vec![Entity::CashRefund {local:0, amount}], ..DeskBatch::default() })).unwrap();
        }
        receiver.receiver.retain_interrupted(receiver.receiver.try_recv().unwrap());
        let budget = std::num::NonZeroUsize::new(1).unwrap();
        let pending = receiver.drain_closed(budget);
        assert_eq!(pending.state, ClosedDrainState::NotQuiescent);
        assert!(pending.messages.is_empty());
        receiver.close();
        let first = receiver.drain_closed(budget);
        assert_eq!(first.state, ClosedDrainState::BudgetExhausted);
        assert!(matches!(&first.messages[..], [GroupIn::Batch(batch)] if batch.entities == [Entity::CashRefund {local:0, amount:500}]));
        let second = receiver.drain_closed(budget);
        assert_eq!(second.state, ClosedDrainState::BudgetExhausted);
        assert!(matches!(&second.messages[..], [GroupIn::Batch(batch)] if batch.entities == [Entity::CashRefund {local:0, amount:700}]));
        let complete = receiver.drain_closed(budget);
        assert_eq!(complete.state, ClosedDrainState::Drained);
        assert!(complete.messages.is_empty());
        assert!(WorkerSender::send(&sender, GroupIn::EchoTickForTest).is_err());
    }

    #[test]
    fn 仍有发送许可时不排空也不宣称完成() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        WorkerSender::send(&sender, 12).unwrap();
        let permit = sender.gate.acquire().unwrap();
        receiver.close();
        let budget = std::num::NonZeroUsize::new(2).unwrap();
        let pending = receiver.drain_closed(budget);
        assert_eq!(pending.state, ClosedDrainState::NotQuiescent);
        assert!(pending.messages.is_empty());
        drop(permit);
        let complete = receiver.drain_closed(budget);
        assert_eq!(complete.state, ClosedDrainState::Drained);
        assert_eq!(complete.messages, [12]);
    }

    #[test]
    fn 成对通道使用故障域登记并在所有引用释放前保留槽() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel::<u32>(&domain).unwrap();
        assert!(matches!(domain.register(), Err(crate::libos::fault_domain::RegisterError::Full)));
        assert!(matches!(paired_registered_channel::<u32>(&domain), Err(crate::libos::fault_domain::RegisterError::Full)));
        drop(receiver);
        assert!(matches!(domain.register(), Err(crate::libos::fault_domain::RegisterError::Full)));
        drop(sender);
        assert!(domain.register().is_ok());
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel::<u32>(&domain).unwrap();
        domain.abort();
        assert!(matches!(paired_registered_channel::<u32>(&domain), Err(crate::libos::fault_domain::RegisterError::Aborted)));
        // 故障广播不会直接关闭准入；worker 返回之前仍可能接收发送。
        WorkerSender::send(&sender, 7).unwrap();
        let (result, receiver) = receiver.run(|rx| crate::libos::mailbox::receive_interruptible(rx, std::time::Duration::from_millis(300)).map(|_| ()));
        assert_eq!(result, Err(crate::libos::mailbox::ReceiveError::Aborted));
        assert!(receiver.is_closed_and_quiescent());
        assert_eq!(WorkerSender::send(&sender, 8).unwrap_err().0, 8);
        assert_eq!(receiver.try_recv(), Ok(7));
    }

    #[test]
    fn 成对通道在真实等待中由故障域唤醒并关闭准入() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel::<u32>(&domain).unwrap();
        let wait = receiver.receiver.abort_handle();
        let child = std::thread::spawn(move || receiver.run(|rx| crate::libos::mailbox::receive_interruptible(rx, std::time::Duration::from_secs(1)).map(|_| ())));
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while !wait.waiting_registered() {
            assert!(std::time::Instant::now() < deadline, "接收者未登记等待");
            std::thread::yield_now();
        }
        domain.abort();
        let (result, receiver) = child.join().unwrap();
        assert_eq!(result, Err(crate::libos::mailbox::ReceiveError::Aborted));
        assert!(receiver.is_closed_and_quiescent());
        assert_eq!(WorkerSender::send(&sender, 9).unwrap_err().0, 9);
    }

    #[test]
    fn 成对通道关闭后等待既有许可并取得其入队载荷() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (sender, receiver) = paired_registered_channel(&domain).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let resume = barrier.clone();
        let child = std::thread::spawn(move || {
            // 在真实许可和底层 send 之间设置测试屏障，固定关闭竞态。
            let permit = sender.gate.acquire().unwrap();
            entered_tx.send(()).unwrap();
            resume.wait();
            let result = WorkerSender::send(&sender.inner, 42);
            drop(permit);
            result
        });
        entered_rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap();
        receiver.close();
        assert!(!receiver.is_closed_and_quiescent());
        let budget = std::num::NonZeroUsize::new(2).unwrap();
        let pending = receiver.drain_closed(budget);
        assert_eq!(pending.state, ClosedDrainState::NotQuiescent);
        assert!(pending.messages.is_empty());
        barrier.wait();
        child.join().unwrap().unwrap();
        assert!(receiver.is_closed_and_quiescent());
        let drained = receiver.drain_closed(budget);
        assert_eq!(drained.state, ClosedDrainState::Drained);
        assert_eq!(drained.messages, [42]);
    }

    #[test]
    fn 关闭与已准入发送竞争时不提前宣称收束() {
        #[derive(Clone)]
        struct HeldSender { entered: mpsc::Sender<()>, resume: Arc<std::sync::Barrier>, inner: mpsc::Sender<u32> }
        impl WorkerSender<u32> for HeldSender {
            fn send(&self, value: u32) -> Result<(), mpsc::SendError<u32>> {
                self.entered.send(()).unwrap();
                self.resume.wait();
                self.inner.send(value)
            }
            fn disconnected_for_test() -> Self { panic!("本测试不使用断连构造") }
        }
        let (inner, receiver) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let resume = Arc::new(std::sync::Barrier::new(3));
        let (sender, gate) = GatedSender::new(HeldSender { entered:entered_tx, resume:resume.clone(), inner });
        let other = sender.clone();
        let child = std::thread::spawn(move || WorkerSender::send(&sender, 42));
        let other_child = std::thread::spawn(move || WorkerSender::send(&other, 43));
        entered_rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap();
        entered_rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap();
        gate.close();
        assert!(!gate.is_closed_and_quiescent());
        resume.wait();
        child.join().unwrap().unwrap();
        other_child.join().unwrap().unwrap();
        assert!(gate.is_closed_and_quiescent());
        let mut messages: Vec<_> = receiver.try_iter().collect();
        messages.sort_unstable();
        assert_eq!(messages, [42, 43]);
    }

    #[test]
    fn 底层断连保留载荷且释放准入计数() {
        let (inner, receiver) = crate::libos::mailbox::channel();
        drop(receiver);
        let (sender, gate) = GatedSender::new(inner);
        assert_eq!(WorkerSender::send(&sender, 7).unwrap_err().0, 7);
        gate.close();
        assert!(gate.is_closed_and_quiescent());
    }

    #[test]
    fn native_unwind释放许可但不伪造发送结果() {
        struct PanickingSender;
        impl WorkerSender<u32> for PanickingSender {
            fn send(&self, _value: u32) -> Result<(), mpsc::SendError<u32>> { panic!("发送故障") }
            fn disconnected_for_test() -> Self { Self }
        }
        let (sender, gate) = GatedSender::new(PanickingSender);
        assert!(std::panic::catch_unwind(|| WorkerSender::send(&sender, 7)).is_err());
        gate.close();
        assert!(gate.is_closed_and_quiescent());
    }

    #[test]
    fn 多个克隆同时竞争关闭时成功消息恰好进入队列() {
        for _ in 0..32 {
            let (inner, receiver) = crate::libos::mailbox::channel();
            let (sender, gate) = GatedSender::new(inner);
            let barrier = Arc::new(std::sync::Barrier::new(3));
            let children: Vec<_> = (0..2).map(|value| {
                let sender = sender.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || { barrier.wait(); WorkerSender::send(&sender, value).map_err(|error| error.0) })
            }).collect();
            barrier.wait();
            gate.close();
            let results: Vec<_> = children.into_iter().map(|child| child.join().unwrap()).collect();
            assert!(gate.is_closed_and_quiescent());
            let mut accepted = results.iter().enumerate().filter_map(|(value, result)| result.is_ok().then_some(value as i32)).collect::<Vec<_>>();
            let mut queued = receiver.try_iter().collect::<Vec<_>>();
            accepted.sort_unstable();
            queued.sort_unstable();
            assert_eq!(queued, accepted);
            for (value, result) in results.into_iter().enumerate() {
                if let Err(returned) = result { assert_eq!(returned, value as i32); }
            }
        }
    }

}
