//! wasm 三收件入口的实际装配；显式 settings 与同 memory 的单向宿主 abort 分离于 tick Abort。
use super::wasm_mechanism::{self, WaitConfig, WaitFailure as PollFailure, WaitRuntime};
use super::{SpinConfig, WaitFailure};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::OnceLock;
use std::time::Duration;

// 配置只在初始化时校验；热收件直接借用已验证的静态配置。
static SETTINGS: OnceLock<WaitConfig> = OnceLock::new();
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
    let settings = WaitConfig::new(
        Duration::from_micros(u64::from(spin_us)),
        Duration::from_micros(u64::from(quiet_us)),
        Duration::from_micros(u64::from(fine_us)),
        Duration::from_micros(u64::from(coarse_us)),
    )
    .map_err(|error| format!("wasm wait settings 非法（单位微秒）：{error:?}"))?;
    SETTINGS
        .set(settings)
        .map_err(|_| "wasm wait settings 并发重复初始化被拒绝".to_owned())
}

fn settings() -> Result<&'static WaitConfig, String> {
    SETTINGS.get().ok_or_else(|| {
        "wasm wait settings 尚未初始化；须在首次 World spawn/收件前调用 init_wasm_wait".to_owned()
    })
}

pub fn require_wait_initialized() -> Result<(), String> {
    settings().map(|_| ())
}

/// 宿主只能对 address 的 bytes=1 字节原子写入 1；不提供复位接口，不能唤醒裸 mpsc recv。
pub struct WaitWordLayout {
    pub address: u32,
    pub bytes: u32,
}

pub fn wasm_wait_abort_layout() -> Result<WaitWordLayout, String> {
    require_wait_initialized()?;
    assert_eq!(
        std::mem::size_of::<AtomicBool>(),
        1,
        "宿主 abort 布局必须精确为 1 字节"
    );
    let address = u32::try_from((&GLOBAL_ABORT as *const AtomicBool) as usize)
        .map_err(|_| "wasm wait abort 地址超出 wasm32 范围".to_owned())?;
    Ok(WaitWordLayout { address, bytes: 1 })
}

/// 全局 DEAD 在 panic 初期置位并 futex notify；记录可能尚未发布。
pub fn wasm_wait_death_layout() -> Result<WaitWordLayout, String> {
    require_wait_initialized()?;
    let address = u32::try_from(crate::wasm_tombstone::death_word() as usize)
        .map_err(|_| "wasm DEAD 地址超出 wasm32 范围".to_owned())?;
    Ok(WaitWordLayout { address, bytes: 4 })
}

#[cfg(feature = "verification-harness")]
pub trait WaitObserver {
    fn note_sleep(&self);
    fn note_spin(&self);
    fn note_raw_recv_selected(&self);
}

struct Runtime<'a> {
    started: crate::clock::Instant,
    #[cfg(feature = "verification-harness")]
    probe: Option<&'a dyn WaitObserver>,
    #[cfg(not(feature = "verification-harness"))]
    lifetime: std::marker::PhantomData<&'a ()>,
}
impl Runtime<'_> {
    fn new() -> Self {
        Self {
            started: crate::clock::Instant::now(),
            #[cfg(feature = "verification-harness")]
            probe: None,
            #[cfg(not(feature = "verification-harness"))]
            lifetime: std::marker::PhantomData,
        }
    }
}
impl WaitRuntime for Runtime<'_> {
    #[inline]
    fn now(&self) -> Duration {
        crate::clock::Instant::now().wait_sample()
    }
    #[inline]
    fn first_sample(&self) -> Result<Duration, PollFailure> {
        let previous = self.started.wait_sample();
        let observed = self.now();
        if observed < previous {
            return Err(PollFailure::ClockRegressed { previous, observed });
        }
        Ok(observed)
    }
    fn sleep(&self, duration: Duration) {
        #[cfg(feature = "verification-harness")]
        if let Some(probe) = self.probe {
            probe.note_sleep();
        }
        std::thread::sleep(duration);
    }
    fn spin(&self) {
        #[cfg(feature = "verification-harness")]
        if let Some(probe) = self.probe {
            probe.note_spin();
        }
        std::hint::spin_loop();
    }
    fn host_aborted(&self) -> bool {
        GLOBAL_ABORT.load(Ordering::SeqCst)
    }
    #[cfg(feature = "verification-harness")]
    fn before_blocking_recv(&self) {
        if let Some(probe) = self.probe {
            probe.note_raw_recv_selected();
        }
    }
}

fn failure(error: PollFailure) -> WaitFailure {
    match error {
        PollFailure::Disconnected => WaitFailure::Disconnected,
        PollFailure::Deadline { waited } => WaitFailure::Deadline { waited },
        PollFailure::HostAborted => WaitFailure::HostAborted,
        PollFailure::ClockRegressed { previous, observed } => {
            WaitFailure::ClockRegressed { previous, observed }
        }
    }
}

fn coordinator_recv<T>(
    rx: &Receiver<T>,
    limit: Duration,
    coarse: bool,
    runtime: &Runtime<'_>,
) -> Result<T, WaitFailure> {
    let settings = settings().map_err(WaitFailure::Uninitialized)?;
    wasm_mechanism::recv_coordinator(rx, limit, settings, coarse, runtime).map_err(failure)
}

fn configured_worker_recv<T>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    spin: Duration,
    runtime: &Runtime<'_>,
) -> Result<Option<T>, WaitFailure> {
    let settings = settings().map_err(WaitFailure::Uninitialized)?;
    wasm_mechanism::recv_worker(rx, shutdown, settings, spin, runtime).map_err(failure)
}

/// 协调者使用细片或粗片轮询，绝不落入裸 recv。
pub fn recv_deadline<T>(rx: &Receiver<T>, limit: Duration, coarse: bool) -> Result<T, WaitFailure> {
    coordinator_recv(rx, limit, coarse, &Runtime::new())
}

/// worker 没有相位自杀限；永久关停哨兵负责唤醒裸 recv。
pub fn worker_recv<T>(rx: &Receiver<T>, shutdown: &AtomicBool) -> Result<Option<T>, WaitFailure> {
    let settings = settings().map_err(WaitFailure::Uninitialized)?;
    wasm_mechanism::recv_worker(rx, shutdown, settings, settings.spin(), &Runtime::new())
        .map_err(failure)
}

/// 与真实 group 的收件配置同源；不读取 native 环境变量或采用默认值。
pub fn spin_config(_disable_native_for_test: bool) -> SpinConfig {
    let settings = settings().expect("wasm spin_config：World spawn 前必须显式 init_wasm_wait");
    SpinConfig {
        enabled: !settings.spin().is_zero(),
        window: settings.spin(),
    }
}

/// disabled 精确关闭热态自旋；启用时使用调用者的窗口。
pub fn worker_recv_spin<T>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    spin: SpinConfig,
) -> Result<Option<T>, WaitFailure> {
    let budget = if spin.enabled {
        spin.window
    } else {
        Duration::ZERO
    };
    configured_worker_recv(rx, shutdown, budget, &Runtime::new())
}

/// 探针只观测真实等待，不替换时钟、消息通道或关停算法。
#[cfg(feature = "verification-harness")]
pub fn verify_worker_recv<T>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    spin: SpinConfig,
    probe: &dyn WaitObserver,
) -> Result<Option<T>, WaitFailure> {
    let runtime = Runtime {
        started: crate::clock::Instant::now(),
        probe: Some(probe),
    };
    let budget = if spin.enabled {
        spin.window
    } else {
        Duration::ZERO
    };
    configured_worker_recv(rx, shutdown, budget, &runtime)
}

#[cfg(feature = "verification-harness")]
pub fn verify_coordinator_recv<T>(
    rx: &Receiver<T>,
    limit: Duration,
    coarse: bool,
    probe: &dyn WaitObserver,
) -> Result<T, WaitFailure> {
    let runtime = Runtime {
        started: crate::clock::Instant::now(),
        probe: Some(probe),
    };
    coordinator_recv(rx, limit, coarse, &runtime)
}

#[cfg(feature = "verification-harness")]
pub fn verify_host_aborted() -> bool {
    GLOBAL_ABORT.load(Ordering::SeqCst)
}
