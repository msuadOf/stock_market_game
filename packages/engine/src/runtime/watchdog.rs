//! 相位感知看门狗（Sol 终审修订项）。
//!
//! **等待点账（wasm 线程抽象层 R4a 阶段 1，设计 §10.1 两层等待模型）**：
//! 本文件三函数（recv_deadline / worker_recv / worker_recv_spin）是
//! **业务层等待点**：协调者由消息/相位限/断连/宿主 abort 退出；两端 worker
//! 均无空闲时限，裸 recv 由消息/断连/永久 Shutdown 哨兵唤醒。业务层之下的
//! **原语层等待点**（std mpmc 内部：(l) 全局分配锁自旋、(m) SyncWaker
//! 内部 Mutex、(n) unbounded 链表发布自旋；bounded array flavor 另有
//! (o)(p)(q)——见 session/deferred_drop.rs 账）不由本文件改写，其失败
//! 逃逸统一外包宿主（D3/D5→R2-3，§10.1.2 设计立场：不加超时包装层）。
//! wasm 不使用 recv_timeout：协调者按细/粗片轮询直到相位限；worker 仅有
//! Hot/quiet 预算，耗尽后裸 recv，由实际消息、断连或永久 Shutdown 哨兵唤醒。
//! 宿主 GLOBAL_ABORT 不唤醒 mpsc 私有 futex；永久关停仍须全员补发哨兵。
//! 队列等待由 LibOS 实现；本层只选择相位档并翻译错误。
//!
//! 固定单值超时（原型 5s）在长相位（日结公司域事务、持久化窗口）会误报——worker
//! 长时空闲等待是协议设计的一部分。处置：相位分档放宽阈值，超时上报必须携带相位
//! 上下文（不静默放大全局阈值）。正常相位保持紧阈值以尽快暴露挂死。

use super::protocol::{RuntimeError, WorkerId};
use libos::wait::{self, WaitFailure};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub use wait::SpinConfig;

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
mod wasm;
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
pub(crate) use wasm::{verify_coordinator_recv, verify_host_aborted, verify_worker_recv};

#[cfg(all(test, not(target_family = "wasm")))]
pub(crate) use wait::spin_us_from_str;

pub(crate) fn require_wait_initialized() -> Result<(), String> {
    wait::require_wait_initialized()
}

/// 等待相位：决定看门狗阈值档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// worker 空闲收件诊断标识；此相位不启动 watchdog。
    WorkerIdle,
    /// 协调者 tick 收束（等待 G GroupReport + N DeskReport，正常档）。
    TickConverge,
    /// Q2 Seal 收尾确认收集（等待 G SealAck，正常档——一组一轮小消息）。
    Seal,
    /// 日终 Drain→DeskDrained 收集（长相位档：清簿回流可重）。
    Drain,
    /// 关停 Final 收集（长相位档：期末上交可重）。
    FinalCollection,
}

/// 看门狗阈值配置。正常相位与长相位分档；测试注入小值。
#[derive(Clone, Copy, Debug)]
pub struct WatchdogConfig {
    /// 协调者正常相位上限（tick 收束、Seal），不限制 worker 空闲。
    pub normal: Duration,
    /// 长相位上限（Drain/FinalCollection：日结事务与持久化窗口）。
    pub long: Duration,
}

impl WatchdogConfig {
    /// 缺省：正常 5s（对齐原型 RECV_TIMEOUT），长相位 60s。
    pub const fn default_config() -> Self {
        WatchdogConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(60),
        }
    }

    /// 相位对应阈值：Drain/FinalCollection 走长档，其余走正常档。
    pub fn limit_for(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Drain | Phase::FinalCollection => self.long,
            Phase::WorkerIdle | Phase::TickConverge | Phase::Seal => self.normal,
        }
    }
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

fn coarse_wait(phase: Phase) -> bool {
    matches!(phase, Phase::Drain | Phase::FinalCollection)
}

fn failure(
    error: WaitFailure,
    phase: Phase,
    cfg: &WatchdogConfig,
    who: Option<WorkerId>,
) -> RuntimeError {
    match error {
        WaitFailure::Disconnected => RuntimeError::Disconnected {
            who,
            stage: format!("相位 {phase:?} 上行通道断连"),
        },
        WaitFailure::Deadline { waited } => RuntimeError::Watchdog {
            phase,
            waited,
            detail: format!(
                "相位 {phase:?} 等待超时（正常档={}s，长相位档={}s）",
                cfg.normal.as_secs(),
                cfg.long.as_secs(),
            ),
        },
        WaitFailure::HostAborted => RuntimeError::HostAborted { phase, who },
        WaitFailure::ClockRegressed { previous, observed } => RuntimeError::ProtocolInvariant {
            detail: format!("wasm 等待时钟倒退：phase={phase:?} who={who:?} previous={previous:?} observed={observed:?}"),
        },
        WaitFailure::Uninitialized(detail) => RuntimeError::ProtocolInvariant { detail },
    }
}

/// 协调者相位限与断连都显式上报。
pub fn recv_deadline<T>(
    rx: &Receiver<T>,
    phase: Phase,
    cfg: &WatchdogConfig,
) -> Result<T, RuntimeError> {
    wait::recv_deadline(rx, cfg.limit_for(phase), coarse_wait(phase))
        .map_err(|error| failure(error, phase, cfg, None))
}

/// worker 只接收永久关停标记；可重试 tick 的 Abort 不用于结束等待循环。
pub fn worker_recv<T>(
    rx: &Receiver<T>,
    cfg: &WatchdogConfig,
    shutdown: &AtomicBool,
    who: WorkerId,
) -> Result<Option<T>, RuntimeError> {
    let phase = Phase::WorkerIdle;
    wait::worker_recv(rx, shutdown).map_err(|error| failure(error, phase, cfg, Some(who)))
}

/// 单测在 Native 禁用热态自旋；WASM 始终使用宿主显式配置。
pub fn spin_config() -> SpinConfig {
    wait::spin_config(cfg!(test))
}

/// 保持同一队列先后顺序；热态自旋耗尽后回落到对应底座的等待。
pub fn worker_recv_spin<T>(
    rx: &Receiver<T>,
    cfg: &WatchdogConfig,
    shutdown: &AtomicBool,
    who: WorkerId,
    spin: SpinConfig,
) -> Result<Option<T>, RuntimeError> {
    let phase = Phase::WorkerIdle;
    wait::worker_recv_spin(rx, shutdown, spin)
        .map_err(|error| failure(error, phase, cfg, Some(who)))
}
