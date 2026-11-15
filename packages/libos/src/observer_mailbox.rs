//! 有界观察邮箱，只转移值；不等待消费者读取或确认，不决定满载后的处理策略。

use std::num::NonZeroUsize;
use std::sync::mpsc;

pub use mpsc::{TryRecvError, TrySendError};

/// 生产端只提供非阻塞尝试，不公开标准库的阻塞发送方法。
pub struct Sender<T>(mpsc::SyncSender<T>);

/// 消费端不向生产端发送确认；取值或返回空队列/断连。
pub struct Receiver<T>(mpsc::Receiver<T>);

/// 创建独立的有界邮箱。容量按值计，不按字节计。
pub fn channel<T>(capacity: NonZeroUsize) -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::sync_channel(capacity.get());
    (Sender(tx), Receiver(rx))
}

impl<T> Sender<T> {
    /// 满载或断连时返回原值，调用方决定如何保留、释放或通知上层。
    #[inline]
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(value)
    }
}

impl<T> Receiver<T> {
    /// 按 FIFO 读取一个值。正常断连不丢弃此前已入队的值。
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one() -> NonZeroUsize {
        NonZeroUsize::MIN
    }

    #[test]
    fn mailbox_is_fifo_and_full_returns_ownership_immediately() {
        let (tx, rx) = channel(NonZeroUsize::new(2).unwrap());
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(tx.try_send(1), Ok(()));
        assert_eq!(tx.try_send(2), Ok(()));
        assert_eq!(tx.try_send(3), Err(TrySendError::Full(3)));
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(tx.try_send(3), Ok(()));
        assert_eq!(rx.try_recv(), Ok(2));
        assert_eq!(rx.try_recv(), Ok(3));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn undrained_mailbox_does_not_block_independent_mailbox() {
        let (slow_tx, slow_rx) = channel(one());
        let (fast_tx, fast_rx) = channel(one());
        assert_eq!(slow_tx.try_send(1), Ok(()));
        assert_eq!(slow_tx.try_send(2), Err(TrySendError::Full(2)));
        assert_eq!(fast_tx.try_send(2), Ok(()));
        assert_eq!(fast_rx.try_recv(), Ok(2));
        assert_eq!(slow_rx.try_recv(), Ok(1));
    }

    #[test]
    fn producer_disconnect_drains_queued_messages_before_closed() {
        let (tx, rx) = channel(one());
        tx.try_send(1).unwrap();
        drop(tx);
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn consumer_disconnect_is_not_reported_as_full() {
        let (tx, rx) = channel(one());
        tx.try_send(1).unwrap();
        drop(rx);
        assert_eq!(tx.try_send(2), Err(TrySendError::Disconnected(2)));
    }

    #[test]
    fn running_consumer_does_not_need_to_send_ack() {
        let (tx, rx) = channel(one());
        tx.try_send(String::from("owned")).unwrap();
        let received = std::thread::spawn(move || rx.try_recv()).join().unwrap();
        assert_eq!(received, Ok(String::from("owned")));
    }
}
