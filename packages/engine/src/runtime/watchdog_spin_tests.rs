//! B-1' 分级自旋合同（协调者批 2026-10-19）：组线程等待热态自旋窗
//! （`worker_recv_spin`——seal-spread-wakeup-design.md）。判别面核心：
//! 自旋耗尽后**立即**查 shutdown 返回（~T_spin），现行睡眠路径每步
//! 50ms 才查——返回耗时把两条路径分开。语义等价面：消息序/断连/
//! 关停/watchdog 判定全部保持（自旋段计入 waited）。

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::watchdog::{worker_recv_spin, SpinConfig, WatchdogConfig};
use super::protocol::WorkerId;

fn who() -> WorkerId {
    WorkerId::Group(0)
}

/// 判别面（红先行锚）：活 sender（不 send、不断连），shutdown 置位后
/// 调用——自旋实现 ~500μs 返回 Ok(None)；睡眠实现 ≥50ms。
#[test]
fn spin_exhausted_shutdown_returns_fast() {
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

/// 禁用旋钮（ENGINE_SPIN_DISABLE 语义）：行为与现行 worker_recv 完全
/// 一致——shutdown 观察仍经 50ms 步进（≥50ms 返回 Ok(None)）。
#[test]
fn spin_disabled_preserves_sleep_path() {
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
        elapsed >= Duration::from_millis(50),
        "禁用时须保持现行 50ms 步进关停观察（耗时 {elapsed:?}）"
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

/// watchdog 判定保持：自旋段计入空闲等待——注入小 limit（300μs）时
/// 500μs 自旋窗耗尽 limit 即 Watchdog Err（不进睡眠放大）。
#[test]
fn spin_respects_watchdog_limit() {
    let (tx, rx) = mpsc::channel::<u32>();
    let shutdown = AtomicBool::new(false);
    let cfg = WatchdogConfig {
        normal: Duration::from_micros(300),
        long: Duration::from_secs(60),
    };
    let spin = SpinConfig {
        enabled: true,
        window: Duration::from_micros(500),
    };
    let out = worker_recv_spin(&rx, &cfg, &shutdown, who(), spin);
    assert!(
        matches!(out, Err(super::protocol::RuntimeError::Watchdog { .. })),
        "自旋段耗尽 limit 必须 Watchdog（got: {out:?}）"
    );
    drop(tx);
}
