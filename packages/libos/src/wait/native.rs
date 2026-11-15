//! Native 使用 std 通道：协调者限时等待；空闲 worker 裸 recv，由消息或断连唤醒。

use super::{SpinConfig, WaitFailure};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::OnceLock;
use std::time::Duration;

pub fn require_wait_initialized() -> Result<(), String> {
    Ok(())
}

pub fn recv_deadline<T>(
    rx: &Receiver<T>,
    limit: Duration,
    _coarse: bool,
) -> Result<T, WaitFailure> {
    match rx.recv_timeout(limit) {
        Ok(message) => Ok(message),
        // 超时判定点与 Browser 一致：有数据或已断连时不返回空队列超时。
        // 成功热路径仍只有一次 std 限时等待。
        Err(RecvTimeoutError::Timeout) => match rx.try_recv() {
            Ok(message) => Ok(message),
            Err(TryRecvError::Disconnected) => Err(WaitFailure::Disconnected),
            Err(TryRecvError::Empty) => Err(WaitFailure::Deadline { waited: limit }),
        },
        Err(RecvTimeoutError::Disconnected) => Err(WaitFailure::Disconnected),
    }
}

/// 永久停止可在进入口或收件提交点生效；停机发起方必须同时唤醒。
pub fn worker_recv<T>(rx: &Receiver<T>, shutdown: &AtomicBool) -> Result<Option<T>, WaitFailure> {
    if is_stopped(shutdown) {
        return Ok(None);
    }
    Ok(rx.recv().ok().filter(|_| !is_stopped(shutdown)))
}

#[inline]
fn is_stopped(shutdown: &AtomicBool) -> bool {
    shutdown.load(Ordering::SeqCst)
}

/// native 进程级配置只读一次；engine 单测显式关闭自旋。
pub fn spin_config(disable_for_test: bool) -> SpinConfig {
    if disable_for_test {
        return SpinConfig {
            enabled: false,
            window: Duration::ZERO,
        };
    }
    static SPIN: OnceLock<SpinConfig> = OnceLock::new();
    *SPIN.get_or_init(|| {
        let disabled = std::env::var("ENGINE_SPIN_DISABLE").ok().as_deref() == Some("1");
        // 默认 500μs：覆盖静置窗实测 spread 主体（124-185μs）且不过度空转。
        let window_us = match std::env::var("ENGINE_SPIN_US") {
            Err(_) => 500,
            Ok(raw) => spin_us_from_str(&raw),
        };
        SpinConfig {
            enabled: !disabled,
            window: Duration::from_micros(window_us),
        }
    })
}

pub fn spin_us_from_str(raw: &str) -> u64 {
    raw.parse::<u64>().unwrap_or_else(|e| {
        panic!("ENGINE_SPIN_US 非法值 {raw:?}：{e}——显式部署意图不静默回落默认 500μs（铁律二）")
    })
}

/// 同一接收队列保持 FIFO；自旋只影响调度，不决定空闲 worker 的寿命。
pub fn worker_recv_spin<T>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    spin: SpinConfig,
) -> Result<Option<T>, WaitFailure> {
    if !spin.enabled || spin.window.is_zero() || is_stopped(shutdown) {
        return worker_recv(rx, shutdown);
    }
    let started = crate::clock::Instant::now();
    loop {
        match rx.try_recv() {
            Ok(msg) => return Ok((!is_stopped(shutdown)).then_some(msg)),
            Err(TryRecvError::Disconnected) => return Ok(None),
            Err(TryRecvError::Empty) => {}
        }
        if is_stopped(shutdown) {
            return Ok(None);
        }
        if started.elapsed() >= spin.window {
            return worker_recv(rx, shutdown);
        }
        std::hint::spin_loop();
    }
}
