//! 两端同源的真实 LibOS API 合同；不读私有等待状态。
use libos::{
    clock::Instant,
    wait::{self, SpinConfig, WaitFailure},
};
use std::sync::{atomic::AtomicBool, mpsc};
use std::time::Duration;

pub fn clock_arithmetic() {
    let a = Instant::now();
    let gap = Duration::from_millis(17);
    let b = a.checked_add(gap).expect("正常时钟范围可加");
    let before_a = a.checked_sub(gap).expect("可表示原点前的正常相对时刻");
    assert_eq!(a.checked_add(Duration::ZERO), Some(a));
    assert!(before_a < a && a < b);
    assert_eq!(b.checked_duration_since(a), Some(gap));
    assert_eq!(b - a, gap);
    assert_eq!(b.checked_sub(gap), Some(a));
    assert_eq!(a.checked_duration_since(b), None);
    assert_eq!(a.duration_since(b), Duration::ZERO);
    assert_eq!(a.saturating_duration_since(b), Duration::ZERO);
    assert_eq!(a - before_a, gap);
    // 极端表示范围由底座决定，不假定 Native/WASM 的最大 Instant 完全相同。
    if let Some(far) = a.checked_add(Duration::MAX) {
        assert_eq!(far.checked_duration_since(a), Some(Duration::MAX));
    }
}

pub fn coordinator_queue() {
    for coarse in [false, true] {
        let (tx, rx) = mpsc::channel();
        for value in [21, 34] {
            tx.send(value).unwrap();
        }
        drop(tx);
        assert_eq!(wait::recv_deadline(&rx, Duration::ZERO, coarse), Ok(21));
        assert_eq!(wait::recv_deadline(&rx, Duration::ZERO, coarse), Ok(34));
        assert_eq!(
            wait::recv_deadline(&rx, Duration::ZERO, coarse),
            Err(WaitFailure::Disconnected)
        );

        let (tx, rx) = mpsc::channel();
        assert!(matches!(
            wait::recv_deadline(&rx, Duration::ZERO, coarse),
            Err(WaitFailure::Deadline { .. })
        ));
        tx.send(55).unwrap(); // 超时已经返回，后续消息不得被已结束调用消费。
        assert_eq!(wait::recv_deadline(&rx, Duration::ZERO, coarse), Ok(55));
    }
}

pub fn worker_queue() {
    for enabled in [false, true] {
        let spin = SpinConfig {
            enabled,
            window: Duration::from_micros(500),
        };
        let stop = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        for value in [3, 5, 8] {
            tx.send(value).unwrap();
        }
        drop(tx);
        assert_eq!(wait::worker_recv(&rx, &stop), Ok(Some(3)));
        assert_eq!(wait::worker_recv_spin(&rx, &stop, spin), Ok(Some(5)));
        assert_eq!(wait::worker_recv_spin(&rx, &stop, spin), Ok(Some(8)));
        assert_eq!(wait::worker_recv_spin(&rx, &stop, spin), Ok(None));
        let (tx, rx) = mpsc::channel();
        tx.send(13).unwrap();
        assert_eq!(
            wait::worker_recv_spin(&rx, &AtomicBool::new(true), spin),
            Ok(None)
        );
        assert_eq!(rx.try_recv(), Ok(13), "硬停在入口已知时不能吞掉业务消息");
    }
}

pub fn shutdown_wake() {
    use libos::lifecycle::ShutdownWake;
    use std::sync::{atomic::Ordering, mpsc::TryRecvError};

    let (already_dead, drop_rx) = mpsc::channel::<u32>();
    drop(drop_rx);
    let (tx1, rx1) = mpsc::channel::<u32>();
    let (tx2, rx2) = mpsc::channel::<&str>();
    // 保留外部 Sender 验证重复调用不会发第二个哨兵，也不把外部连接假装断开。
    let retained1 = tx1.clone();
    let retained2 = tx2.clone();
    let mut first = vec![already_dead, tx1];
    let mut second = vec![tx2];
    let mut wake = ShutdownWake::for_workers(&first, &second, || 99, || "stop");
    let stop = AtomicBool::new(false);
    retained1.send(12).unwrap();
    wake.sever(&stop, &mut first, &mut second);
    assert!(stop.load(Ordering::SeqCst));
    assert!(
        first.is_empty() && second.is_empty(),
        "协调者不能保留被结束的 Sender"
    );
    assert_eq!(rx1.try_recv(), Ok(12), "哨兵不重排已入队内容");
    assert_eq!(rx1.try_recv(), Ok(99), "失效同组接收端不能阻塞其余哨兵");
    assert_eq!(rx2.try_recv(), Ok("stop"), "异类型的另一组也必须收到唤醒");
    wake.sever(&stop, &mut first, &mut second);
    assert_eq!(rx1.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(rx2.try_recv(), Err(TryRecvError::Empty));
    drop((retained1, retained2));
    assert_eq!(rx1.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(rx2.try_recv(), Err(TryRecvError::Disconnected));
}
