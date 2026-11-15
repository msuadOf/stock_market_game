//! 收件机制接口。只有协调者有 deadline，正常 worker 空闲不构成故障。
//! worker 保持 FIFO；非关停断连仍交付缓冲，永久停止优先于消息交付。
//! 标记在入口已置位时不取件；若取件后才观察到标记，已取值销毁、不交付。
//! 永久停止须配套唤醒消息或断开通道，单个原子标记不能唤醒裸 recv。

use std::time::Duration;

/// 热态自旋窗口；关闭时不采用窗口值。
#[derive(Clone, Copy, Debug)]
pub struct SpinConfig {
    pub enabled: bool,
    pub window: Duration,
}

/// 语义分类不暴露等待策略。waited 是诊断值：底座回报的等待时长，或实际观测时长。
#[derive(Debug, PartialEq, Eq)]
pub enum WaitFailure {
    Disconnected,
    Deadline {
        waited: Duration,
    },
    HostAborted,
    ClockRegressed {
        previous: Duration,
        observed: Duration,
    },
    Uninitialized(String),
}

#[cfg(not(target_family = "wasm"))]
mod native;
#[cfg(not(target_family = "wasm"))]
pub use native::{
    recv_deadline, require_wait_initialized, spin_config, spin_us_from_str, worker_recv,
    worker_recv_spin,
};

#[cfg(target_family = "wasm")]
mod wasm;
#[cfg(target_family = "wasm")]
mod wasm_mechanism;
#[cfg(target_family = "wasm")]
pub use wasm::{
    init_wasm_wait, recv_deadline, require_wait_initialized, spin_config, wasm_wait_abort_layout,
    wasm_wait_death_layout, worker_recv, worker_recv_spin, WaitWordLayout,
};
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
pub use wasm::{verify_coordinator_recv, verify_host_aborted, verify_worker_recv, WaitObserver};

/// 同一套收件合同在 Native 单测与真实 WASM Worker 验证制品执行。
#[cfg(any(test, feature = "verification-harness"))]
#[doc(hidden)]
pub fn verify_worker_delivery_contract() {
    use std::sync::{atomic::AtomicBool, mpsc};
    for spin in [
        SpinConfig {
            enabled: false,
            window: Duration::from_micros(500),
        },
        SpinConfig {
            enabled: true,
            window: Duration::from_micros(500),
        },
    ] {
        let (tx, rx) = mpsc::channel();
        let shutdown = AtomicBool::new(false);
        let receive = || worker_recv_spin(&rx, &shutdown, spin);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        assert_eq!(receive(), Ok(Some(1)));
        drop(tx);
        // 正常断连保留 FIFO 缓冲，不等同于永久停止。
        assert_eq!(receive(), Ok(Some(2)));
        assert_eq!(receive(), Ok(None));

        // 不用计时猜测竞争先后：构造取件入口已同时看到关停和缓冲。
        for stop_first in [true, false] {
            let (tx, rx) = mpsc::channel();
            let shutdown = AtomicBool::new(stop_first);
            tx.send(3).unwrap();
            if !stop_first {
                shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            assert_eq!(worker_recv(&rx, &shutdown), Ok(None));
            assert_eq!(worker_recv_spin(&rx, &shutdown, spin), Ok(None));
            assert_eq!(rx.try_recv(), Ok(3), "入口已停止时不消耗缓冲");
        }
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
#[test]
fn worker_delivery_contract() {
    verify_worker_delivery_contract();
    verify_coordinator_delivery_contract();
}

/// 到期与断连均有共同分类；立即收件可以交付已缓冲消息。
#[cfg(any(test, feature = "verification-harness"))]
#[doc(hidden)]
pub fn verify_coordinator_delivery_contract() {
    use std::sync::mpsc;
    for coarse in [false, true] {
        let (tx, rx) = mpsc::channel::<u32>();
        assert!(matches!(
            recv_deadline(&rx, Duration::ZERO, coarse),
            Err(WaitFailure::Deadline { .. })
        ));
        tx.send(5).unwrap();
        drop(tx);
        assert_eq!(recv_deadline(&rx, Duration::ZERO, coarse), Ok(5));
        assert_eq!(
            recv_deadline(&rx, Duration::ZERO, coarse),
            Err(WaitFailure::Disconnected)
        );
    }
}
