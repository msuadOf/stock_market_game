//! 发布永久停止状态并唤醒所有空闲工作线程。
use crate::libos::wake::WorkerWake;
use std::sync::atomic::{AtomicBool, Ordering};

/// 协议要求处理的消息须由调用方先完成发送及必要确认，再关闭准入并唤醒。
pub(super) fn wake_all(
    shutdown: &AtomicBool,
    groups: &[impl WorkerWake],
    desks: &[impl WorkerWake],
) {
    shutdown.store(true, Ordering::SeqCst);
    for sender in groups {
        sender.wake_for_stop();
    }
    for sender in desks {
        sender.wake_for_stop();
    }
}

#[cfg(test)]
mod tests {
    use super::wake_all;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn 先发布停止状态再唤醒所有成对下行且不发送业务消息() {
        let (group_sender, group_receiver) = crate::libos::downlink::channel::<u8>();
        let (desk_sender, desk_receiver) = crate::libos::downlink::channel::<u8>();
        let shutdown = AtomicBool::new(false);
        wake_all(&shutdown, &[group_sender.clone()], &[desk_sender.clone()]);

        assert!(shutdown.load(Ordering::SeqCst));
        assert!(matches!(group_receiver.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Empty)));
        assert!(matches!(desk_receiver.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Empty)));
        assert_eq!(group_sender.send(1).unwrap_err().0, 1);
        assert_eq!(desk_sender.send(2).unwrap_err().0, 2);
    }

    #[test]
    fn 先发送_shutdown_协议载荷后停止通知只唤醒不插入第二条消息() {
        let (sender, receiver) = crate::libos::downlink::channel::<u8>();
        sender.send(7).unwrap();
        let shutdown = AtomicBool::new(false);
        wake_all(&shutdown, &[sender.clone()], &[] as &[crate::libos::downlink::Sender<u8>]);

        assert_eq!(receiver.try_receive().unwrap(), Ok(7));
        assert!(matches!(receiver.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn std_旧通道仅作为测试替身参与通知接口() {
        let (sender, receiver) = std::sync::mpsc::channel::<u8>();
        let shutdown = AtomicBool::new(false);
        wake_all(&shutdown, &[sender], &[] as &[std::sync::mpsc::Sender<u8>]);
        assert!(shutdown.load(Ordering::SeqCst));
        assert!(matches!(receiver.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
    }
}
