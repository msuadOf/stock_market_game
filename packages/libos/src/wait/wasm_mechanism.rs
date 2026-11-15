//! wasm 收件纯机制：协调者相位限与 worker 安静预算分置。
//!
//! 仅供 wasm adapter 使用；两个后端的裸 recv 均只能由消息、
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
    pub fn spin(&self) -> Duration {
        self.spin
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
    /// 真实 adapter 可在第一次相位样本之前已取过起点；必须保留两者的回退诊断。
    fn first_sample(&self) -> Result<Duration, WaitFailure> {
        Ok(self.now())
    }
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
    let mut clock = Samples::new(runtime)?;
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
            Err(TryRecvError::Disconnected) => {
                check_host(runtime)?;
                return Err(WaitFailure::Disconnected);
            }
            Err(TryRecvError::Empty) => {}
        }
        let waited = clock.sample(runtime)? - started;
        if waited >= limit {
            return ready_at_deadline(rx, runtime, waited);
        }
        runtime.sleep(slice.min(limit - waited));
        // 醒点先验证时钟；回退不能被同时发布的消息掩盖。
        check_host(runtime)?;
        let woke = clock.sample(runtime)? - started;
        if woke >= limit {
            return ready_at_deadline(rx, runtime, woke);
        }
    }
}

#[cold]
fn ready_at_deadline<T, R: WaitRuntime>(
    rx: &Receiver<T>,
    runtime: &R,
    waited: Duration,
) -> Result<T, WaitFailure> {
    // 时限限制空队列等待，不为发送事件定时。到判定点后再观察一次数据和断连；
    // 判定后的发送保留给下一次接收。宿主错误不可报告为正常超时。
    check_host(runtime)?;
    let ready = rx.try_recv();
    check_host(runtime)?;
    match ready {
        Ok(message) => Ok(message),
        Err(TryRecvError::Disconnected) => Err(WaitFailure::Disconnected),
        Err(TryRecvError::Empty) => Err(WaitFailure::Deadline { waited }),
    }
}
pub fn recv_worker<T, R: WaitRuntime>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    config: &WaitConfig,
    spin: Duration,
    runtime: &R,
) -> Result<Option<T>, WaitFailure> {
    let mut clock = Samples::new(runtime)?;
    let started = clock.previous;
    // spin 和 quiet 各有自己的预算；这里没有 worker 自杀相位限。
    loop {
        if stopped(shutdown, runtime)? {
            return Ok(None);
        }
        match rx.try_recv() {
            Ok(message) => return worker_message(message, shutdown, runtime),
            Err(TryRecvError::Disconnected) => {
                check_host(runtime)?;
                return Ok(None);
            }
            Err(TryRecvError::Empty) => {}
        }
        if clock.sample(runtime)? - started >= spin {
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
            Err(TryRecvError::Disconnected) => {
                check_host(runtime)?;
                return Ok(None);
            }
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
    fn new<R: WaitRuntime>(runtime: &R) -> Result<Self, WaitFailure> {
        Ok(Self {
            previous: runtime.first_sample()?,
        })
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
