//! 永久停止通知；唤醒只改变等待状态，不发送业务载荷。

pub(crate) trait WorkerWake {
    fn wake_for_stop(&self);
}

impl<T> WorkerWake for super::mailbox::Sender<T> {
    fn wake_for_stop(&self) {
        self.wake();
    }
}

impl<T> WorkerWake for super::downlink::Sender<T> {
    fn wake_for_stop(&self) {
        self.stop();
    }
}

/// 仅用于覆盖遗留通道适配测试；正式运行只能通过 LibOS 通道唤醒。
#[cfg(test)]
impl<T> WorkerWake for std::sync::mpsc::Sender<T> {
    fn wake_for_stop(&self) {}
}

#[cfg(test)]
mod tests {
    use super::WorkerWake;

    #[test]
    fn 停止唤醒不会向_mailbox_插入控制消息() {
        let (sender, receiver) = super::super::mailbox::channel();
        sender.send(7).unwrap();
        sender.wake_for_stop();
        assert_eq!(receiver.try_recv(), Ok(7));
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn 成对下行停止唤醒不插入业务消息并拒绝后续发送() {
        let (sender, receiver) = super::super::downlink::channel();
        sender.send(7).unwrap();
        sender.wake_for_stop();
        assert_eq!(receiver.try_receive().unwrap(), Ok(7));
        assert!(matches!(
            receiver.try_receive().unwrap(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert_eq!(sender.send(9).unwrap_err().0, 9);
    }

    #[test]
    fn 遗留_std_测试替身不会构造或发送业务消息() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.wake_for_stop();
        assert!(matches!(receiver.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
    }
}
