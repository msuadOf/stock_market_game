//! 跨平台执行能力的共同入口。
pub(crate) mod transport;
pub(crate) mod thread;
pub(crate) mod fault_domain;
pub(crate) mod mailbox;
pub(crate) mod worker_wait;
pub(crate) mod admission;
pub(crate) mod worker_exit;
pub(crate) mod downlink;
pub(crate) mod wake;
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
pub mod verification;

#[cfg(test)]
mod paired_downlink_tests {
    #[test]
    fn 清理期限到期后不能宣称排空且保留载荷供重试() {
        let (sender, receiver) = super::downlink::channel();
        sender.send(42).unwrap();
        let closed = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(10)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
        let batch = closed.drain(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(batch.state, super::downlink::DrainState::TimedOut);
        assert!(batch.messages.is_empty());
        let retry = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        assert_eq!(retry.drain(std::num::NonZeroUsize::new(2).unwrap()).messages, [42]);
    }
    #[test]
    fn 线程失败后关闭成对通道并原样保留消息() {
        let (sender, receiver) = super::downlink::channel();
        sender.send(String::from("资金和股份载荷")).unwrap();
        let clone = sender.clone();
        let worker = std::thread::spawn(move || receiver.run(|_| -> Result<(), ()> {
            std::panic::panic_any(String::from("worker 原始错误"));
        }));
        let exit = worker.join().unwrap();
        assert!(matches!(exit.outcome, super::worker_exit::Outcome::Panicked(_)));
        assert_eq!(clone.send(String::from("拒绝的载荷")).unwrap_err().0, "拒绝的载荷");
        let closed = exit.receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        let batch = closed.drain(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(batch.messages, ["资金和股份载荷"]);
        assert_eq!(batch.state, super::downlink::DrainState::Drained);
    }
}

#[cfg(test)]
mod worker_exit_tests {
    #[test]
    fn worker_panic_后保留接收器和原始_panic_载荷() {
        let (tx, rx) = super::downlink::channel();
        tx.send(42).unwrap();
        let exit = rx.run(|_| -> Result<(), ()> {
            std::panic::panic_any(String::from("原始 worker 故障"));
        });
        assert!(tx.send(43).is_err());
        match exit.outcome {
            super::worker_exit::Outcome::Panicked(payload) => {
                assert_eq!(*payload.downcast::<String>().unwrap(), "原始 worker 故障");
            }
            _ => panic!("必须保留 panic 结果"),
        }
        let closed = exit.receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        assert_eq!(closed.drain(std::num::NonZeroUsize::new(2).unwrap()).messages, [42]);
    }
}

#[cfg(test)]
mod common_transport_tests {
    #[test]
    fn native_上下行都通过统一入口保持消息与断连语义() {
        let (tx, rx) = super::transport::upstream_channel::<u32>(|| false).unwrap();
        let (down_tx, down_rx) = super::transport::downstream_channel::<u32>(|| false).unwrap();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        down_tx.send(3).unwrap();
        drop(tx);
        drop(down_tx);
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(rx.try_recv(), Ok(2));
        assert!(matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
        assert_eq!(down_rx.try_receive().unwrap(), Ok(3));
        assert!(matches!(down_rx.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn native_统一非阻塞收件保持原有快速路径() {
        let (tx, rx) = super::transport::upstream_channel::<u32>(|| false).unwrap();
        tx.send(7).unwrap();
        let fault_checks = std::cell::Cell::new(0);
        let received = super::transport::try_receive_up(&rx, || {
            fault_checks.set(fault_checks.get() + 1);
            true
        }).unwrap();
        assert_eq!(received, Ok(7));
        assert_eq!(fault_checks.get(), 0);
        drop(tx);
        assert!(matches!(super::transport::try_receive_up(&rx, || false).unwrap(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn native_统一阻塞收件使用直接就绪路径() {
        let (tx, rx) = super::transport::upstream_channel::<u32>(|| false).unwrap();
        tx.send(7).unwrap();
        let result = super::transport::receive_up(&rx, std::time::Duration::from_secs(1), || panic!("Native 无宿主故障"));
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn 统一阻塞接收直接提供超时断连和就绪结果() {
        use super::transport::{receive_up, ReceiveFailure};
        let (tx, rx) = super::transport::upstream_channel::<u32>(|| false).unwrap();
        assert!(matches!(receive_up(&rx, std::time::Duration::ZERO, || false), Err(ReceiveFailure::Timeout { .. })));
        tx.send(17).unwrap();
        assert_eq!(receive_up(&rx, std::time::Duration::from_secs(1), || false).unwrap(), 17);
        drop(tx);
        assert!(matches!(receive_up(&rx, std::time::Duration::from_secs(1), || false), Err(ReceiveFailure::Disconnected)));
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn native_线程入口直接复用std且兼容现有门面() {
        let builder: super::thread::Builder = crate::thread_facade::Builder::new();
        let handle: std::thread::JoinHandle<u32> = builder.spawn(|| 42).unwrap();
        assert_eq!(handle.join().unwrap(), 42);
    }

    #[test]
    fn 通用工作者等待提供消息与永久停止语义() {
        let (tx, rx) = super::mailbox::channel();
        let stop = std::sync::atomic::AtomicBool::new(false);
        tx.send(7).unwrap();
        assert_eq!(super::worker_wait::receive(&rx, &stop, || false), Ok(Some(7)));
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(super::worker_wait::receive(&rx, &stop, || false), Ok(None));
    }
}
