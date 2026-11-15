//! Native 自旋启停只改变等待策略，不改变 FIFO、断连和永久停止合同。

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::protocol::WorkerId;
use super::watchdog::{worker_recv_spin, SpinConfig, WatchdogConfig};

fn who() -> WorkerId {
    WorkerId::Group(0)
}

/// 活 sender 保持开放时，入口已关停也不进入阻塞。
#[test]
fn spin_enabled_with_stop_set_returns_fast() {
    let (tx, rx) = mpsc::channel::<u32>();
    let shutdown = AtomicBool::new(true);
    let cfg = WatchdogConfig::default_config();
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(500),
    };
    let started = Instant::now();
    let out = worker_recv_spin(&rx, &cfg, &shutdown, who(), spin);
    let elapsed = started.elapsed();
    assert!(
        matches!(out, Ok(None)),
        "自旋耗尽+shutdown 必须 Ok(None)（got: {out:?}）"
    );
    assert!(
        elapsed < Duration::from_millis(30),
        "自旋路径耗尽后须立即观察 shutdown（自旋窗 500μs——耗时 {elapsed:?} ≥30ms 说明走了睡眠步进）"
    );
    drop(tx);
}

/// 禁用自旋同样在入口观察永久停止，不需要周期唤醒。
#[test]
fn spin_disabled_observes_stop_at_entry() {
    let (tx, rx) = mpsc::channel::<u32>();
    let shutdown = AtomicBool::new(true);
    let cfg = WatchdogConfig::default_config();
    let spin = SpinConfig {
        enabled: false,
        window: Duration::from_micros(500),
    };
    let started = Instant::now();
    let out = worker_recv_spin(&rx, &cfg, &shutdown, who(), spin);
    let elapsed = started.elapsed();
    assert!(matches!(out, Ok(None)));
    assert!(
        elapsed < Duration::from_millis(30),
        "禁用自旋也不等待旧的 50ms 轮询片（耗时 {elapsed:?}）"
    );
    drop(tx);
}

/// 自旋窗内消息到达：正常返回消息（等价面——消息序不变）。
#[test]
fn spin_window_message_arrival_returns_message() {
    let (tx, rx) = mpsc::channel::<u32>();
    tx.send(7).expect("预发消息");
    let shutdown = AtomicBool::new(false);
    let cfg = WatchdogConfig::default_config();
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(500),
    };
    let out = worker_recv_spin(&rx, &cfg, &shutdown, who(), spin);
    assert_eq!(out, Ok(Some(7)));
}

/// 永久停止在入口可见时，保留缓冲但不交付。
#[test]
fn native_stop_precedes_buffered_messages() {
    let (tx, rx) = mpsc::channel::<u32>();
    tx.send(7).expect("队首入队");
    tx.send(8).expect("下一条入队");
    let shutdown = AtomicBool::new(true);
    let cfg = WatchdogConfig::default_config();
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(500),
    };
    assert_eq!(
        super::watchdog::worker_recv(&rx, &cfg, &shutdown, who()),
        Ok(None)
    );
    assert_eq!(
        worker_recv_spin(&rx, &cfg, &shutdown, who(), spin),
        Ok(None)
    );
    assert_eq!(rx.try_recv(), Ok(7));
    assert_eq!(rx.try_recv(), Ok(8));
}

/// 自旋期断连：Ok(None)（干净退出——现行语义保持）。
#[test]
fn spin_disconnect_returns_none() {
    let (tx, rx) = mpsc::channel::<u32>();
    drop(tx);
    let shutdown = AtomicBool::new(false);
    let cfg = WatchdogConfig::default_config();
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(500),
    };
    let out = worker_recv_spin(&rx, &cfg, &shutdown, who(), spin);
    assert!(matches!(out, Ok(None)), "断连必须干净退出（got: {out:?}）");
}

/// 自旋耗尽回落睡眠路径：窗后到达的消息仍被接收（等价面——回落不
/// 丢消息）。窗 50μs，消息 80ms 后到达——总耗时 ≥50ms 证明真回落。
#[test]
fn spin_exhausted_falls_back_to_sleep_and_receives() {
    let (tx, rx) = mpsc::channel::<u32>();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        tx.send(42).expect("发送");
    });
    let shutdown = AtomicBool::new(false);
    let cfg = WatchdogConfig::default_config();
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(50),
    };
    let started = Instant::now();
    let out = worker_recv_spin(&rx, &cfg, &shutdown, who(), spin);
    let elapsed = started.elapsed();
    assert_eq!(out, Ok(Some(42)));
    assert!(
        elapsed >= Duration::from_millis(50),
        "窗后消息须经睡眠路径（耗时 {elapsed:?} <50ms 说明窗未真耗尽）"
    );
    sender.join().expect("发送线程");
}

/// 业务 watchdog 即使设置为零，也不结束正常 worker 的空闲等待。
#[test]
fn coordinator_deadline_does_not_expire_idle_worker() {
    let (tx, rx) = mpsc::channel::<u32>();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        tx.send(31).expect("空闲 worker 仍可接收新消息");
    });
    let shutdown = AtomicBool::new(false);
    let cfg = WatchdogConfig {
        normal: Duration::ZERO,
        long: Duration::from_secs(1),
    };
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(500),
    };
    assert_eq!(
        worker_recv_spin(&rx, &cfg, &shutdown, who(), spin),
        Ok(Some(31))
    );
    sender.join().expect("发送线程");
}

/// 复核建议②收口：ENGINE_SPIN_US 显式设置但非法（非数字/溢出）必须
/// 显式 panic（含值与原因）——静默回落默认 500μs 会掩盖配置错误
///（铁律二"不用默认值掩盖异常"）。合法值透传。
#[test]
#[should_panic(expected = "ENGINE_SPIN_US 非法值")]
fn spin_us_invalid_value_panics_loud() {
    super::watchdog::spin_us_from_str("abc");
}

/// 合法值透传+空串/超 u64 拒绝（panic 面）。
#[test]
fn spin_us_valid_value_passes_through() {
    assert_eq!(super::watchdog::spin_us_from_str("250"), 250);
    assert_eq!(super::watchdog::spin_us_from_str("1000"), 1000);
}
