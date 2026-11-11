//! Group 与 Desk 共用的 LibOS 收件协议。
use super::protocol::{RuntimeError, WorkerId};
use std::sync::{atomic::AtomicBool, mpsc::TryRecvError};

/// 数据面发送维持底层队列的所有权转移与 FIFO，不改变消息含义。
pub(super) trait WorkerSender<T> {
    fn send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>>;
    #[cfg(test)]
    fn disconnected_for_test() -> Self;
}

impl<T> WorkerSender<T> for crate::libos::downlink::Sender<T> {
    fn send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>> { self.send(value) }
    #[cfg(test)]
    fn disconnected_for_test() -> Self {
        let (sender, receiver) = crate::libos::downlink::channel();
        drop(receiver);
        sender
    }
}

#[cfg(test)]
impl<T> WorkerSender<T> for std::sync::mpsc::Sender<T> {
    fn send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>> { self.send(value) }
    fn disconnected_for_test() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        sender
    }
}

#[cfg(test)]
impl<T> WorkerSender<T> for crate::libos::mailbox::Sender<T> {
    fn send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>> { self.send(value) }
    fn disconnected_for_test() -> Self {
        let (sender, receiver) = crate::libos::mailbox::channel();
        drop(receiver);
        sender
    }
}

pub(super) trait WorkerInbox<T> {
    fn receive(&self, shutdown: &AtomicBool, who: WorkerId) -> Result<Option<T>, RuntimeError>;
    fn try_receive(&self, who: WorkerId) -> Result<Result<T, TryRecvError>, RuntimeError>;
}

fn map_worker_receive_error(who: WorkerId, error: crate::libos::worker_wait::WorkerReceiveError) -> RuntimeError {
    use crate::libos::worker_wait::WorkerReceiveError;
    match error {
        WorkerReceiveError::HostAborted => RuntimeError::HostAborted { phase: super::wait_contract::Phase::WorkerIdle, who: Some(who) },
        WorkerReceiveError::Disconnected => RuntimeError::Disconnected { who: Some(who), stage: "Worker 收件通道断连".to_owned() },
        WorkerReceiveError::UnexpectedTimeout { waited } => RuntimeError::ProtocolInvariant { detail: format!("worker {who} 无期限等待异常超时：{waited:?}") },
        WorkerReceiveError::ClockRegressed { previous, observed } => RuntimeError::ProtocolInvariant { detail: format!("worker {who} 等待时钟倒退：{previous:?} -> {observed:?}") },
    }
}

impl<T> WorkerInbox<T> for crate::libos::downlink::Receiver<T> {
    fn receive(&self, shutdown: &AtomicBool, who: WorkerId) -> Result<Option<T>, RuntimeError> {
        self.receive(shutdown).map_err(|error| map_worker_receive_error(who, error))
    }
    fn try_receive(&self, who: WorkerId) -> Result<Result<T, TryRecvError>, RuntimeError> {
        self.try_receive().map_err(|_| RuntimeError::HostAborted { phase: super::wait_contract::Phase::Seal, who: Some(who) })
    }
}

/// 原生通道仅供现有单元测试替身使用；正式 Group/Desk 只接受 LibOS receiver。
#[cfg(test)]
impl<T> WorkerInbox<T> for std::sync::mpsc::Receiver<T> {
    fn receive(&self, _shutdown: &AtomicBool, who: WorkerId) -> Result<Option<T>, RuntimeError> {
        // 原生 mpsc 仅作单元测试替身；测试以 Shutdown 消息退出，不模拟停止唤醒或旧等待策略。
        self.recv().map(Some).map_err(|_| RuntimeError::Disconnected {
            who: Some(who),
            stage: "Worker 收件通道断连".to_owned(),
        })
    }
    fn try_receive(&self, _who: WorkerId) -> Result<Result<T, TryRecvError>, RuntimeError> { Ok(self.try_recv()) }
}

#[cfg(test)]
pub(super) fn checked_try_receive<T>(rx: &crate::libos::mailbox::Receiver<T>, who: WorkerId, external_fault: impl Fn() -> bool) -> Result<Result<T, TryRecvError>, RuntimeError> {
    crate::libos::worker_wait::try_receive(rx, external_fault).map_err(|_| RuntimeError::HostAborted {
        phase: super::wait_contract::Phase::Seal, who: Some(who),
    })
}

/// mailbox 直连适配仅用于为收件协议提供确定性单元测试替身。
#[cfg(test)]
impl<T> WorkerInbox<T> for crate::libos::mailbox::Receiver<T> {
    fn receive(&self, shutdown: &AtomicBool, who: WorkerId) -> Result<Option<T>, RuntimeError> {
        let external_fault = super::upstream::external_fault;
        crate::libos::worker_wait::receive(self, shutdown, external_fault).map_err(|error| map_worker_receive_error(who, error))
    }
    fn try_receive(&self, who: WorkerId) -> Result<Result<T, TryRecvError>, RuntimeError> {
        checked_try_receive(self, who, super::upstream::external_fault)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 断连错误保留_worker_身份() {
        let error = map_worker_receive_error(WorkerId::Group(3), crate::libos::worker_wait::WorkerReceiveError::Disconnected);
        assert!(matches!(error, RuntimeError::Disconnected { who: Some(WorkerId::Group(3)), .. }));
    }
    #[test]
    fn 成对下行接收器可直接用于共同_worker_收件接口() {
        fn verify<T: WorkerInbox<u32>>(_: &T) {}
        let (sender, receiver) = crate::libos::downlink::channel::<u32>();
        verify(&receiver);
        sender.send(42).unwrap();
        let shutdown = AtomicBool::new(false);
        assert_eq!(WorkerInbox::receive(&receiver, &shutdown, WorkerId::Group(0)).unwrap(), Some(42));
        assert!(matches!(WorkerInbox::try_receive(&receiver, WorkerId::Group(0)).unwrap(), Err(TryRecvError::Empty)));
    }
    #[test]
    fn seal_非阻塞取件遇故障时保留已出队消息() {
        let (tx, rx) = crate::libos::mailbox::channel();
        tx.send(7).unwrap();
        let checks = std::cell::Cell::new(0);
        let error = checked_try_receive(&rx, WorkerId::Group(0), || { checks.set(checks.get() + 1); checks.get() >= 2 }).unwrap_err();
        assert!(matches!(error, RuntimeError::HostAborted { who:Some(WorkerId::Group(0)), .. }));
        assert_eq!(rx.try_recv().unwrap(), 7);
    }

    #[test]
    fn 发送端断连时原始载荷由调用方取得() {
        let (tx, rx) = crate::libos::mailbox::channel();
        drop(rx);
        let error = WorkerSender::send(&tx, String::from("未发送载荷")).unwrap_err();
        assert_eq!(error.0, "未发送载荷");
    }
}
