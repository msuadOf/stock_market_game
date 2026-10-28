//! wasm 三收件入口的实际装配；显式 settings 与同 memory 的单向宿主 abort 分离于 tick Abort。
use super::{Phase, SpinConfig, WatchdogConfig};
use crate::runtime::protocol::{RuntimeError, WorkerId};
use crate::runtime::wasm_wait::{self, WaitConfig, WaitFailure, WaitRuntime};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::OnceLock;
use std::time::Duration;

struct Settings {
    spin: Duration,
    quiet: Duration,
    fine: Duration,
    coarse: Duration,
}

impl Settings {
    fn config(&self, spin: Duration) -> WaitConfig {
        WaitConfig::new(spin, self.quiet, self.fine, self.coarse)
            .expect("wasm wait settings 已在唯一初始化时校验，fine/coarse 不得为零")
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();
// AtomicBool 只允许宿主使用同 memory 的 Uint8Array 原子写入 1；绝不以 Int32Array 跨字覆盖。
static GLOBAL_ABORT: AtomicBool = AtomicBool::new(false);

/// 所有时长单位为微秒；首次 World spawn/收件前显式调用，同实例不允许重复或重置。
/// 引擎不选生产政策值，调用者须明确给出 spin/quiet/fine/coarse。
pub fn init_wasm_wait(
    spin_us: u32,
    quiet_us: u32,
    fine_us: u32,
    coarse_us: u32,
) -> Result<(), String> {
    if SETTINGS.get().is_some() {
        return Err("wasm wait settings 已初始化，同一 memory 禁止重复初始化或重置".to_owned());
    }
    let settings = Settings {
        spin: Duration::from_micros(u64::from(spin_us)),
        quiet: Duration::from_micros(u64::from(quiet_us)),
        fine: Duration::from_micros(u64::from(fine_us)),
        coarse: Duration::from_micros(u64::from(coarse_us)),
    };
    WaitConfig::new(
        settings.spin,
        settings.quiet,
        settings.fine,
        settings.coarse,
    )
    .map_err(|error| format!("wasm wait settings 非法（单位微秒）：{error:?}"))?;
    SETTINGS
        .set(settings)
        .map_err(|_| "wasm wait settings 并发重复初始化被拒绝".to_owned())
}

fn settings() -> Result<&'static Settings, String> {
    SETTINGS.get().ok_or_else(|| {
        "wasm wait settings 尚未初始化；须在首次 World spawn/收件前调用 init_wasm_wait".to_owned()
    })
}

pub(crate) fn require_wasm_wait_initialized() -> Result<(), String> {
    settings().map(|_| ())
}

/// 宿主只能对 address 的 bytes=1 字节原子写入 1；不提供复位接口，不能唤醒裸 mpsc recv。
#[derive(serde::Serialize)]
pub struct WasmWaitAbortLayout {
    pub address: u32,
    pub bytes: u32,
}

pub fn wasm_wait_abort_layout() -> Result<WasmWaitAbortLayout, String> {
    require_wasm_wait_initialized()?;
    assert_eq!(
        std::mem::size_of::<AtomicBool>(),
        1,
        "宿主 abort 布局必须精确为 1 字节"
    );
    let address = u32::try_from((&GLOBAL_ABORT as *const AtomicBool) as usize)
        .map_err(|_| "wasm wait abort 地址超出 wasm32 范围".to_owned())?;
    Ok(WasmWaitAbortLayout { address, bytes: 1 })
}

struct Runtime {
    started: crate::clock::Instant,
    #[cfg(feature = "verification-harness")]
    probe: Option<std::sync::Arc<crate::runtime::watchdog_wait_verification::WaitProbe>>,
}
impl Runtime {
    fn new() -> Self {
        Self {
            started: crate::clock::Instant::now(),
            #[cfg(feature = "verification-harness")]
            probe: None,
        }
    }
}
impl WaitRuntime for Runtime {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
    fn sleep(&self, duration: Duration) {
        #[cfg(feature = "verification-harness")]
        if let Some(probe) = &self.probe {
            probe.note_sleep();
        }
        std::thread::sleep(duration);
    }
    fn spin(&self) {
        #[cfg(feature = "verification-harness")]
        if let Some(probe) = &self.probe {
            probe.note_spin();
        }
        std::hint::spin_loop();
    }
    fn host_aborted(&self) -> bool {
        GLOBAL_ABORT.load(Ordering::SeqCst)
    }
    #[cfg(feature = "verification-harness")]
    fn before_blocking_recv(&self) {
        if let Some(probe) = &self.probe {
            probe.note_raw_recv_selected();
        }
    }
}

fn setup_error(detail: String) -> RuntimeError {
    RuntimeError::ProtocolInvariant { detail }
}
fn failure(error: WaitFailure, phase: Phase, who: Option<WorkerId>) -> RuntimeError {
    match error {
        WaitFailure::Disconnected => RuntimeError::Disconnected { who, stage: format!("wasm 相位 {phase:?} 收件断连") },
        WaitFailure::Deadline { waited } => RuntimeError::Watchdog {
            phase, waited, detail: format!("wasm 相位实际等待超时：who={who:?}，waited={waited:?}"),
        },
        WaitFailure::HostAborted => RuntimeError::HostAborted { phase, who },
        WaitFailure::ClockRegressed { previous, observed } => RuntimeError::ProtocolInvariant {
            detail: format!("wasm 等待时钟倒退：phase={phase:?} who={who:?} previous={previous:?} observed={observed:?}"),
        },
    }
}

fn coordinator_recv<T>(
    rx: &Receiver<T>,
    phase: Phase,
    cfg: &WatchdogConfig,
    runtime: &Runtime,
) -> Result<T, RuntimeError> {
    let settings = settings().map_err(setup_error)?;
    wasm_wait::recv_coordinator(
        rx,
        cfg.limit_for(phase),
        &settings.config(settings.spin),
        matches!(phase, Phase::Drain | Phase::FinalCollection),
        runtime,
    )
    .map_err(|error| failure(error, phase, None))
}

fn configured_worker_recv<T>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    who: WorkerId,
    spin: Duration,
    runtime: &Runtime,
) -> Result<Option<T>, RuntimeError> {
    let settings = settings().map_err(setup_error)?;
    wasm_wait::recv_worker(rx, shutdown, &settings.config(spin), runtime)
        .map_err(|error| failure(error, Phase::WorkerIdle, Some(who)))
}

/// 协调者仅有相位限：细片用于 tick/seal，粗片用于 drain/final；绝不落裸 recv。
pub fn recv_deadline<T>(
    rx: &Receiver<T>,
    phase: Phase,
    cfg: &WatchdogConfig,
) -> Result<T, RuntimeError> {
    coordinator_recv(rx, phase, cfg, &Runtime::new())
}

/// worker 无相位自杀限：共享 settings 的 Hot→quiet→裸 recv；永久 Shutdown 哨兵负责唤醒。
pub fn worker_recv<T>(
    rx: &Receiver<T>,
    _cfg: &WatchdogConfig,
    shutdown: &AtomicBool,
    who: WorkerId,
) -> Result<Option<T>, RuntimeError> {
    let spin = settings().map_err(setup_error)?.spin;
    configured_worker_recv(rx, shutdown, who, spin, &Runtime::new())
}

/// 与真实 group 的收件配置同源；未初始化是显式失败，不从 native env 或默认值取策略。
pub fn spin_config() -> SpinConfig {
    let settings = settings().expect("wasm spin_config：World spawn 前必须显式 init_wasm_wait");
    SpinConfig {
        enabled: !settings.spin.is_zero(),
        window: settings.spin,
    }
}

/// 保持传入 SpinConfig 的承诺：disabled 精确关闭 Hot，enabled 使用调用者明确的 window。
pub fn worker_recv_spin<T>(
    rx: &Receiver<T>,
    _cfg: &WatchdogConfig,
    shutdown: &AtomicBool,
    who: WorkerId,
    spin: SpinConfig,
) -> Result<Option<T>, RuntimeError> {
    let budget = if spin.enabled {
        spin.window
    } else {
        Duration::ZERO
    };
    configured_worker_recv(rx, shutdown, who, budget, &Runtime::new())
}

/// 每 call 显式 probe 只观测真实 adapter；不替换 clock/channel/sleep 或关停算法。
#[cfg(feature = "verification-harness")]
pub(crate) fn verify_worker_recv<T>(
    rx: &Receiver<T>,
    _cfg: &WatchdogConfig,
    shutdown: &AtomicBool,
    who: WorkerId,
    spin: SpinConfig,
    probe: std::sync::Arc<crate::runtime::watchdog_wait_verification::WaitProbe>,
) -> Result<Option<T>, RuntimeError> {
    let runtime = Runtime {
        started: crate::clock::Instant::now(),
        probe: Some(probe),
    };
    let budget = if spin.enabled {
        spin.window
    } else {
        Duration::ZERO
    };
    configured_worker_recv(rx, shutdown, who, budget, &runtime)
}

#[cfg(feature = "verification-harness")]
pub(crate) fn verify_coordinator_recv<T>(
    rx: &Receiver<T>,
    phase: Phase,
    cfg: &WatchdogConfig,
    probe: std::sync::Arc<crate::runtime::watchdog_wait_verification::WaitProbe>,
) -> Result<T, RuntimeError> {
    let runtime = Runtime {
        started: crate::clock::Instant::now(),
        probe: Some(probe),
    };
    coordinator_recv(rx, phase, cfg, &runtime)
}

#[cfg(feature = "verification-harness")]
pub(crate) fn verify_host_aborted() -> bool {
    GLOBAL_ABORT.load(Ordering::SeqCst)
}
