//! 构造与异常清理合同。Native 单测和真实 Browser 验证入口复用。
use libos::lifecycle::ShutdownWake;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender, TryRecvError},
};

pub fn abort_without_registration() {
    let shutdown = AtomicBool::new(false);
    let (group, group_rx) = mpsc::channel::<u32>();
    let (desk, desk_rx) = mpsc::channel::<&str>();
    group.send(13).unwrap();
    let mut group = vec![group];
    let mut desk = vec![desk];
    let mut guard = ShutdownWake::default();

    guard.abort_construction(&shutdown, &mut group, &mut desk);
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(group.is_empty() && desk.is_empty(), "无唤醒登记时必须断开构造者发送端");
    assert_eq!(group_rx.try_recv(), Ok(13), "断连不重排已有消息");
    assert_eq!(group_rx.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(desk_rx.try_recv(), Err(TryRecvError::Disconnected));
}

pub fn abort_after_partial_registration() {
    let shutdown = AtomicBool::new(false);
    let (started, started_rx) = mpsc::channel::<u32>();
    let (not_started, not_started_rx) = mpsc::channel::<&str>();
    drop(not_started_rx);
    started.send(2).unwrap();
    let mut groups = vec![started];
    let mut desks = vec![not_started];
    let mut guard = ShutdownWake::for_workers(&groups, &desks, || 17, || "stop");

    guard.abort_construction(&shutdown, &mut groups, &mut desks);
    assert!(shutdown.load(Ordering::SeqCst));
    // 登记的哨兵已交付；已持有的原发送端仍由构造者在后续显式接管。
    assert_eq!((groups.len(), desks.len()), (1, 1));
    assert_eq!(started_rx.try_recv(), Ok(2));
    assert_eq!(started_rx.try_recv(), Ok(17));
    assert_eq!(started_rx.try_recv(), Err(TryRecvError::Empty));

    guard.abort_construction(&shutdown, &mut groups, &mut desks);
    assert!(groups.is_empty() && desks.is_empty());
    assert_eq!(started_rx.try_recv(), Err(TryRecvError::Disconnected), "第二次清理不得发重复哨兵");
}

pub fn untransferred_ownership() {
    let shutdown = AtomicBool::new(false);
    let mut empty = ShutdownWake::<u32, &str>::default();
    assert!(empty.drop_untransferred(&shutdown, false).is_none());
    assert!(!shutdown.load(Ordering::SeqCst), "既未登记也未启动不产生清理凭证");

    assert!(empty.drop_untransferred(&shutdown, true).is_some(), "存在已启动句柄时必须留下清理记录");
    assert!(shutdown.load(Ordering::SeqCst));

    let shutdown = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel::<u32>();
    let mut registered = ShutdownWake::for_workers(
        &[tx.clone()],
        &[] as &[Sender<&str>],
        || 23,
        || "stop",
    );
    let report = registered.drop_untransferred(&shutdown, false)
        .expect("即使首个 spawn 失败，已有登记也必须取走唤醒所有权");
    report.report(0, &[]);
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(registered.drop_untransferred(&shutdown, false).is_none());
    assert_eq!(rx.try_recv(), Ok(23));
    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    drop(tx);
    assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
}
