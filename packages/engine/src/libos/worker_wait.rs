//! 工作者共同的通知等待契约。
use super::mailbox::{receive_interruptible_when, ReceiveError, Receiver};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorkerReceiveError {
    HostAborted,
    Disconnected,
    UnexpectedTimeout { waited: Duration },
    ClockRegressed { previous: Duration, observed: Duration },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorkerTryReceiveError { HostAborted }

/// 取件前后检查故障；发生竞争时保留已出队消息供退出清理。
pub(crate) fn try_receive<T>(
    rx: &Receiver<T>,
    external_fault: impl Fn() -> bool,
) -> Result<Result<T, std::sync::mpsc::TryRecvError>, WorkerTryReceiveError> {
    let failed = || rx.is_aborted() || external_fault();
    if failed() { return Err(WorkerTryReceiveError::HostAborted); }
    let result = rx.try_recv();
    if failed() {
        if let Ok(message) = result { rx.retain_interrupted(message); }
        return Err(WorkerTryReceiveError::HostAborted);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn 未停止时发送端全部断连必须报告断连() {
        let (sender, receiver) = super::super::mailbox::channel::<u32>();
        drop(sender);
        let shutdown = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(super::receive(&receiver, &shutdown, || false), Err(super::WorkerReceiveError::Disconnected));
    }

    #[test]
    fn 断连前已入队消息先交付再报告断连() {
        let (sender, receiver) = super::super::mailbox::channel();
        sender.send(7).unwrap();
        drop(sender);
        let shutdown = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(super::receive(&receiver, &shutdown, || false), Ok(Some(7)));
        assert_eq!(super::receive(&receiver, &shutdown, || false), Err(super::WorkerReceiveError::Disconnected));
    }

    #[test]
    fn 停止与断连同时发生时报告停止() {
        let (sender, receiver) = super::super::mailbox::channel::<u32>();
        drop(sender);
        let shutdown = std::sync::atomic::AtomicBool::new(true);
        assert_eq!(super::receive(&receiver, &shutdown, || false), Ok(None));
    }

    #[test]
    fn 非阻塞取件后发生实例故障时保留消息和顺序() {
        let (sender, receiver) = super::super::mailbox::channel();
        sender.send(7).unwrap();
        sender.send(8).unwrap();
        let checks = std::cell::Cell::new(0);
        let result = super::try_receive(&receiver, || {
            checks.set(checks.get() + 1);
            checks.get() == 2
        });
        assert_eq!(result, Err(super::WorkerTryReceiveError::HostAborted));
        assert_eq!(receiver.try_recv(), Ok(7));
        assert_eq!(receiver.try_recv(), Ok(8));
    }
}

/// 正常空闲没有 worker 自杀期限；永久停止由哨兵通知，宿主故障由故障域广播。
/// 标志本身不会唤醒等待者；调用方须证明每条退出路径都会通知当前通道。
/// 失败后保留槽只延迟载荷释放，调用方仍须在销毁 receiver 前完成清理。
pub(crate) fn receive<T>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    external_fault: impl Fn() -> bool,
) -> Result<Option<T>, WorkerReceiveError> {
    let stopped = || shutdown.load(Ordering::SeqCst);
    let failed = || rx.is_aborted() || external_fault();
    if failed() { return Err(WorkerReceiveError::HostAborted); }
    if stopped() { return Ok(None); }
    let outcome = receive_interruptible_when(rx, Duration::MAX, || failed() || stopped());
    match outcome {
        Ok(message) => {
            let failure = failed();
            let stopping = stopped();
            if failure || stopping {
                rx.retain_interrupted(message);
                if failure { Err(WorkerReceiveError::HostAborted) } else { Ok(None) }
            } else {
                Ok(Some(message))
            }
        }
        Err(ReceiveError::AbortedWithMessage(message)) => {
            rx.retain_interrupted(message);
            if failed() { Err(WorkerReceiveError::HostAborted) } else if stopped() { Ok(None) } else { Err(WorkerReceiveError::HostAborted) }
        }
        Err(ReceiveError::Disconnected) => if failed() { Err(WorkerReceiveError::HostAborted) } else if stopped() { Ok(None) } else { Err(WorkerReceiveError::Disconnected) },
        Err(ReceiveError::Aborted) => if failed() { Err(WorkerReceiveError::HostAborted) } else if stopped() { Ok(None) } else { Err(WorkerReceiveError::HostAborted) },
        Err(ReceiveError::Timeout { waited }) => Err(WorkerReceiveError::UnexpectedTimeout { waited }),
        Err(ReceiveError::ClockRegressed { previous, observed }) => Err(WorkerReceiveError::ClockRegressed { previous, observed }),
    }
}

