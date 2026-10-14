//! 相位感知看门狗（Sol 终审修订项）。
//!
//! 固定单值超时（原型 5s）在长相位（日结公司域事务、持久化窗口）会误报——worker
//! 长时空闲等待是协议设计的一部分。处置：相位分档放宽阈值，超时上报必须携带相位
//! 上下文（不静默放大全局阈值）。正常相位保持紧阈值以尽快暴露挂死。

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use super::protocol::{RuntimeError, WorkerId};

/// 等待相位：决定看门狗阈值档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// worker 收件循环空闲等待（正常档）。
    WorkerIdle,
    /// 协调者 tick 收束（等待 G GroupReport + N DeskReport，正常档）。
    TickConverge,
    /// 日终 Drain→DeskDrained 收集（长相位档：清簿回流可重）。
    Drain,
    /// 关停 Final 收集（长相位档：期末上交可重）。
    FinalCollection,
}

/// 看门狗阈值配置。正常相位与长相位分档；测试注入小值。
#[derive(Clone, Copy, Debug)]
pub struct WatchdogConfig {
    /// 正常相位上限（tick 收束/worker 空闲）。
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
            Phase::WorkerIdle | Phase::TickConverge => self.normal,
        }
    }
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// 带相位的收件：超时→携带相位上下文的显式 `RuntimeError::Watchdog`；
/// 断连→显式 `RuntimeError::Disconnected`。绝不静默返回 None。
pub fn recv_deadline<T>(
    rx: &Receiver<T>,
    phase: Phase,
    cfg: &WatchdogConfig,
) -> Result<T, RuntimeError> {
    match rx.recv_timeout(cfg.limit_for(phase)) {
        Ok(msg) => Ok(msg),
        Err(RecvTimeoutError::Timeout) => Err(RuntimeError::Watchdog {
            phase,
            waited: cfg.limit_for(phase),
            detail: format!(
                "相位 {phase:?} 等待超时（正常档={}s，长相位档={}s）",
                cfg.normal.as_secs(),
                cfg.long.as_secs()
            ),
        }),
        Err(RecvTimeoutError::Disconnected) => Err(RuntimeError::Disconnected {
            who: None,
            stage: format!("相位 {phase:?} 上行通道断连"),
        }),
    }
}

/// worker 侧收件变体：超时但全局 abort 标记已置位 ⇒ 干净退出（对端失败引发的
/// 全局撤退，不是本 worker 的挂死）；超时且无标记 ⇒ 显式失败上报。
/// 断连 ⇒ 干净退出（协调者已释放通道，关停协议的一部分）。
pub fn worker_recv<T>(
    rx: &Receiver<T>,
    cfg: &WatchdogConfig,
    abort: &std::sync::atomic::AtomicBool,
    who: WorkerId,
) -> Result<Option<T>, RuntimeError> {
    use std::sync::atomic::Ordering;
    match rx.recv_timeout(cfg.limit_for(Phase::WorkerIdle)) {
        Ok(msg) => Ok(Some(msg)),
        Err(RecvTimeoutError::Timeout) => {
            if abort.load(Ordering::SeqCst) {
                Ok(None)
            } else {
                Err(RuntimeError::Watchdog {
                    phase: Phase::WorkerIdle,
                    waited: cfg.limit_for(Phase::WorkerIdle),
                    detail: format!("worker {who} 空闲超时且无全局中止标记"),
                })
            }
        }
        Err(RecvTimeoutError::Disconnected) => Ok(None),
    }
}
