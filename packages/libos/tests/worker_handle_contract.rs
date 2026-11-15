//! 以 crate 使用者身份验证 thread facade；不借助内部类型或错误文本选择恢复行为。

#![cfg(not(target_family = "wasm"))]

use libos::thread_facade::{
    JoinFailure, JoinFailureClass, WorkerDeath, WorkerDeathKind, WorkerHandle,
};
use std::cell::RefCell;
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq)]
struct JobRejected(u32);

#[test]
fn 任务错误仍属于成功取得的业务结果() {
    let handle = WorkerHandle::spawn("contract-job-result".into(), || {
        Err::<(), _>(JobRejected(91))
    })
    .expect("应成功建线程");

    assert_eq!(
        handle.join().expect("业务 Err 不是 join 失败"),
        Err(JobRejected(91))
    );
}

#[test]
fn join移交原所有权且原生线程清理已经发生() {
    struct OnThreadExit(Arc<Mutex<Vec<&'static str>>>);
    impl Drop for OnThreadExit {
        fn drop(&mut self) {
            self.0.lock().expect("测试日志未中毒").push("tls-drop");
        }
    }
    thread_local! {
        static OWNED_BY_THREAD: RefCell<Option<OnThreadExit>> = const { RefCell::new(None) };
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let identity = Arc::new(42_u32);
    let retained = Arc::clone(&identity);
    let worker_events = Arc::clone(&events);
    let handle = WorkerHandle::spawn("contract-job-ownership".into(), move || {
        OWNED_BY_THREAD.with(|slot| {
            assert!(slot
                .borrow_mut()
                .replace(OnThreadExit(Arc::clone(&worker_events)))
                .is_none());
        });
        worker_events.lock().expect("测试日志未中毒").push("body");
        identity
    })
    .expect("应成功建线程");

    let actual = handle.join().expect("应取得返回值");
    assert!(
        Arc::ptr_eq(&actual, &retained),
        "join 不应重建任务返回的对象"
    );
    assert_eq!(
        *events.lock().expect("测试日志未中毒"),
        ["body", "tls-drop"]
    );
}

#[test]
fn 原生panic保留非字符串对象且执行一次线程清理() {
    struct OnUnwind(Arc<Mutex<Vec<&'static str>>>);
    impl Drop for OnUnwind {
        fn drop(&mut self) {
            self.0.lock().expect("测试日志未中毒").push("unwind");
        }
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let worker_events = Arc::clone(&events);
    let payload = Arc::new(JobRejected(127));
    let retained = Arc::clone(&payload);
    let handle = WorkerHandle::<()>::spawn("contract-panic-payload".into(), move || {
        let _cleanup = OnUnwind(worker_events);
        std::panic::panic_any(payload);
    })
    .expect("应成功建线程");

    let failure = handle
        .join()
        .expect_err("任务 panic 时必须无法交付业务结果");
    assert_eq!(failure.class(), JoinFailureClass::PanicPayload);
    let payload = failure
        .into_panic_payload()
        .expect("真正的原 panic 必须可提取载荷");
    let actual = payload
        .downcast::<Arc<JobRejected>>()
        .expect("原始 panic 类型必须保留");
    assert!(Arc::ptr_eq(actual.as_ref(), &retained));
    assert_eq!(*events.lock().expect("测试日志未中毒"), ["unwind"]);
}

#[test]
fn join死亡分类依据结构化错误而非相似诊断文本() {
    // 这里验证跨宿主公共错误转换，不把 Native 构造的样本充当真实 Browser 死亡证据。
    for kind in [WorkerDeathKind::Panic, WorkerDeathKind::HardTrap] {
        let death = WorkerDeath::new(kind, 0xA17);
        let typed = JoinFailure::from(io::Error::other(death));
        assert_eq!(typed.class(), JoinFailureClass::WorkerDied(death));
        assert!(matches!(&typed, JoinFailure::WorkerDied(actual) if *actual == death));
        assert_eq!(
            typed
                .source()
                .and_then(|value| value.downcast_ref::<WorkerDeath>()),
            Some(&death)
        );
        let recovered = typed
            .into_panic_payload()
            .expect_err("宿主死亡不提供原 panic 对象");
        assert_eq!(recovered.class(), JoinFailureClass::WorkerDied(death));

        let same_text = JoinFailure::from(io::Error::other(death.to_string()));
        assert!(matches!(same_text, JoinFailure::Transport(cause)
            if cause.kind() == io::ErrorKind::Other && cause.to_string() == death.to_string()));
    }
    let timed_out = JoinFailure::from(io::Error::new(io::ErrorKind::TimedOut, "transport timeout"));
    assert_eq!(timed_out.class(), JoinFailureClass::Transport);
    let timed_out = timed_out
        .into_panic_payload()
        .expect_err("传输失败不应构造 panic payload");
    assert!(matches!(timed_out, JoinFailure::Transport(cause)
        if cause.kind() == io::ErrorKind::TimedOut));
}
