//! wasm 收件纯机制：协调者相位限与 worker 安静预算分置。
//!
//! 仅供 wasm adapter 使用；native 生产保持原 std 收件路径。裸 recv 只能由消息、
//! 断连或永久关停哨兵唤醒，宿主 abort 字不唤醒 mpsc 私有 futex。
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError},
};
use std::time::Duration;

pub struct WaitConfig {
    spin: Duration,
    quiet: Duration,
    fine: Duration,
    coarse: Duration,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    ZeroFine,
    ZeroCoarse,
}
impl WaitConfig {
    pub fn new(
        spin: Duration,
        quiet: Duration,
        fine: Duration,
        coarse: Duration,
    ) -> Result<Self, ConfigError> {
        if fine.is_zero() {
            return Err(ConfigError::ZeroFine);
        }
        if coarse.is_zero() {
            return Err(ConfigError::ZeroCoarse);
        }
        Ok(Self {
            spin,
            quiet,
            fine,
            coarse,
        })
    }
    pub fn fine(&self) -> Duration {
        self.fine
    }
    pub fn coarse(&self) -> Duration {
        self.coarse
    }
}
pub trait WaitRuntime {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
    fn spin(&self);
    fn host_aborted(&self) -> bool;
    /// 仅验证 feature 观测已选择裸 recv 分支；不能据此宣称已在私有 futex park。
    #[cfg(feature = "verification-harness")]
    fn before_blocking_recv(&self) {}
}
#[derive(Debug, PartialEq, Eq)]
pub enum WaitFailure {
    Disconnected,
    HostAborted,
    Deadline {
        waited: Duration,
    },
    ClockRegressed {
        previous: Duration,
        observed: Duration,
    },
}
pub fn recv_coordinator<T, R: WaitRuntime>(
    rx: &Receiver<T>,
    limit: Duration,
    config: &WaitConfig,
    coarse: bool,
    runtime: &R,
) -> Result<T, WaitFailure> {
    let mut clock = Samples::new(runtime);
    let started = clock.previous;
    let slice = if coarse {
        config.coarse()
    } else {
        config.fine()
    };
    loop {
        check_host(runtime)?;
        match rx.try_recv() {
            Ok(message) => {
                check_host(runtime)?;
                return Ok(message);
            }
            Err(TryRecvError::Disconnected) => return Err(WaitFailure::Disconnected),
            Err(TryRecvError::Empty) => {}
        }
        let waited = clock.sample(runtime)? - started;
        if waited >= limit {
            return Err(WaitFailure::Deadline { waited });
        }
        runtime.sleep(slice.min(limit - waited));
        check_host(runtime)?;
        // 首次轮询可取已缓冲消息；一旦已等待，先检查截止再消费边界消息。
        let waited = clock.sample(runtime)? - started;
        if waited >= limit {
            return Err(WaitFailure::Deadline { waited });
        }
    }
}
pub fn recv_worker<T, R: WaitRuntime>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    config: &WaitConfig,
    runtime: &R,
) -> Result<Option<T>, WaitFailure> {
    let mut clock = Samples::new(runtime);
    let started = clock.previous;
    // spin 和 quiet 各有自己的预算；这里没有 worker 自杀相位限。
    loop {
        if stopped(shutdown, runtime)? {
            return Ok(None);
        }
        match rx.try_recv() {
            Ok(message) => return worker_message(message, shutdown, runtime),
            Err(TryRecvError::Disconnected) => return Ok(None),
            Err(TryRecvError::Empty) => {}
        }
        if clock.sample(runtime)? - started >= config.spin {
            break;
        }
        runtime.spin();
    }
    let quiet_started = clock.previous;
    loop {
        if stopped(shutdown, runtime)? {
            return Ok(None);
        }
        let elapsed = clock.sample(runtime)? - quiet_started;
        if elapsed >= config.quiet {
            break;
        }
        runtime.sleep(config.fine.min(config.quiet - elapsed));
        if stopped(shutdown, runtime)? {
            return Ok(None);
        }
        clock.sample(runtime)?;
        match rx.try_recv() {
            Ok(message) => return worker_message(message, shutdown, runtime),
            Err(TryRecvError::Disconnected) => return Ok(None),
            Err(TryRecvError::Empty) => {}
        }
    }
    if stopped(shutdown, runtime)? {
        return Ok(None);
    }
    #[cfg(feature = "verification-harness")]
    runtime.before_blocking_recv();
    match rx.recv() {
        Ok(message) => worker_message(message, shutdown, runtime),
        Err(_) => {
            check_host(runtime)?;
            Ok(None)
        }
    }
}

fn check_host<R: WaitRuntime>(runtime: &R) -> Result<(), WaitFailure> {
    if runtime.host_aborted() {
        Err(WaitFailure::HostAborted)
    } else {
        Ok(())
    }
}
fn stopped<R: WaitRuntime>(shutdown: &AtomicBool, runtime: &R) -> Result<bool, WaitFailure> {
    check_host(runtime)?;
    Ok(shutdown.load(Ordering::SeqCst))
}
fn worker_message<T, R: WaitRuntime>(
    message: T,
    shutdown: &AtomicBool,
    runtime: &R,
) -> Result<Option<T>, WaitFailure> {
    if stopped(shutdown, runtime)? {
        Ok(None)
    } else {
        Ok(Some(message))
    }
}

struct Samples {
    previous: Duration,
}
impl Samples {
    fn new<R: WaitRuntime>(runtime: &R) -> Self {
        Self {
            previous: runtime.now(),
        }
    }
    fn sample<R: WaitRuntime>(&mut self, runtime: &R) -> Result<Duration, WaitFailure> {
        let observed = runtime.now();
        if observed < self.previous {
            return Err(WaitFailure::ClockRegressed {
                previous: self.previous,
                observed,
            });
        }
        self.previous = observed;
        Ok(observed)
    }
}
