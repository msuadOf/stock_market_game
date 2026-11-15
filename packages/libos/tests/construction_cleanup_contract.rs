//! 构造异常的公开 API 合同；在真实线程中证明哨兵能到达，且 Native 保留原 panic。
#![cfg(not(target_family = "wasm"))]
#[path = "support/construction_cleanup.rs"]
mod shared;

#[test]
fn 构造尚无登记时断开两组发送端() {
    shared::abort_without_registration();
}

#[test]
fn 部分构造先唤醒后断开且不重复发哨兵() {
    shared::abort_after_partial_registration();
}

#[test]
fn 未移交所有权时区分未启动和仍有清理义务() {
    shared::untransferred_ownership();
}

#[test]
fn 部分启动的真线程观察先发布停止再送哨兵() {
    use libos::{lifecycle::ShutdownWake, thread_facade::WorkerHandle};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};
    use std::time::Duration;

    let budget = Duration::from_secs(1);
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&shutdown);
    let (start, started) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let (unused, unused_rx) = mpsc::channel::<&str>();
    drop(unused_rx);
    let worker = WorkerHandle::spawn("libos-cleanup-fixture".into(), move || {
        start.send(()).unwrap();
        rx.recv_timeout(budget).map(|value| (value, worker_flag.load(Ordering::SeqCst)))
    }).unwrap();
    started.recv_timeout(budget).unwrap();
    let mut group = vec![tx];
    let mut desk = vec![unused];
    let mut wake = ShutdownWake::for_workers(&group, &desk, || 31, || "stop");
    wake.abort_construction(&shutdown, &mut group, &mut desk);
    assert_eq!(worker.join().unwrap().unwrap(), (31, true));
    wake.sever(&shutdown, &mut group, &mut desk);
    assert!(group.is_empty() && desk.is_empty());
}

#[cfg(panic = "unwind")]
#[test]
fn native清理诊断不覆盖原panic对象() {
    use libos::lifecycle::ShutdownWake;
    use std::sync::{atomic::AtomicBool, mpsc};

    #[derive(Debug, PartialEq)]
    struct Primary(u32);

    struct Owner {
        shutdown: AtomicBool,
        wake: ShutdownWake<u32, &'static str>,
        observed: std::sync::Arc<AtomicBool>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.wake.drop_untransferred(&self.shutdown, true)
                .expect("存在构造回收义务")
                .report(0, &["secondary-join-error".to_owned()]);
            self.observed.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let (tx, rx) = mpsc::channel();
    let observed = std::sync::Arc::new(AtomicBool::new(false));
    let proof = observed.clone();
    let error = std::panic::catch_unwind(move || {
        let _owner = Owner {
            shutdown: AtomicBool::new(false),
            wake: ShutdownWake::for_workers(&[tx], &[], || 37, || "stop"),
            observed: proof,
        };
        std::panic::panic_any(Primary(71));
    }).expect_err("测试必须传播最初的 panic");
    assert_eq!(error.downcast_ref::<Primary>(), Some(&Primary(71)));
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(rx.try_recv(), Ok(37));
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}
