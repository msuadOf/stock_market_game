//! 线程门面（wasm 线程抽象层 R4a 阶段 1）。
//!
//! 设计锚：`.tmp/wasm-thread-abstraction-design-input.md` §0（窄面硬约束）/
//! §1.1（std::thread 生产使用面 = spawn/join/available_parallelism 三项）/
//! §1.3（其余 std 原语 wasm 原生可用——不得包一层）。
//!
//! - **native**：`pub use std::thread::{Builder, JoinHandle,
//!   available_parallelism}`——编译期符号别名，零运行期间接；native 逐指令
//!   等价由 asm 对照留证（agents/wasm-thread-r4a/）。
//! - **wasm32**：阶段 2 槽协议接线（设计 §12-14）——spawn/join 经
//!   `crate::facade_slot` 协议核心 + `WasmFacadePlatform`（env import 面）。
//!   运行时前置：宿主 bootstrap 先调 `facade_slot_init`（容量 2×(G+N+2)）
//!   划拨 arena；未初始化的 spawn = 显式 Unsupported。Web 产品宿主缺省启用
//!   产品 World 必须先完成宿主初始化，不支持在门面内回退到串行管线。
//!   available_parallelism（§1.1 T3）维持显式 Unsupported。
//!
//! **为什么 mpsc/Mutex/Condvar 不进门面**（§0 推论 + §1.3 结论）：std 同步
//! 原语在「+atomics + build-std + 共享线性内存」产物里由 std 原生提供
//! （futex 同一代码路径，wasm 与 native 同源），包一层只会在 native 引入
//! 运行期间接——直接违反 §0。门面只收编 wasm 真正缺的三个符号。

// ---------------------------------------------------------------------------
// native：编译期符号别名（§0——零运行期间接；asm 对照留证于 agents 台账）。
// ---------------------------------------------------------------------------
#[cfg(not(target_family = "wasm"))]
pub use std::thread::{available_parallelism, Builder, JoinHandle};

use std::{any::Any, fmt, io};

/// facade arena 既有 slot 上限，供宿主在分配前显式校验容量。
pub const FACADE_SLOT_MAX_CAPACITY: u32 = 1 << 20;

/// Shared slot 中实际观察到的终态，不等同于可重抛的 Rust panic payload。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerDeathKind {
    Panic,
    HardTrap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerDeath {
    pub kind: WorkerDeathKind,
    pub code: u32,
}

impl WorkerDeath {
    pub const fn new(kind: WorkerDeathKind, code: u32) -> Self {
        Self { kind, code }
    }
}

impl fmt::Display for WorkerDeath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            WorkerDeathKind::Panic => "PANIC",
            WorkerDeathKind::HardTrap => "HARDTRAP",
        };
        write!(
            f,
            "FACADE_DEAD_{kind}:0x{:X}（worker 死亡信号，§8.2.2）",
            self.code
        )
    }
}

impl std::error::Error for WorkerDeath {}

/// 已返回的业务结果仍是 T。只有 NativePanic 携带能原样重抛的 payload。
#[derive(Debug)]
pub enum JoinFailure {
    NativePanic(Box<dyn Any + Send>),
    WorkerDied(WorkerDeath),
    Transport(io::Error),
}

/// 共同控制流需要的失败分类，不把 panic 载荷是否可重抛绑定到宿主名。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinFailureClass {
    PanicPayload,
    WorkerDied(WorkerDeath),
    Transport,
}

impl JoinFailure {
    pub fn class(&self) -> JoinFailureClass {
        match self {
            Self::NativePanic(_) => JoinFailureClass::PanicPayload,
            Self::WorkerDied(death) => JoinFailureClass::WorkerDied(*death),
            Self::Transport(_) => JoinFailureClass::Transport,
        }
    }

    /// 只有可重抛的任务 panic 才移交实际 payload；其余错误原样退还调用方。
    pub fn into_panic_payload(self) -> Result<Box<dyn Any + Send>, Self> {
        match self {
            Self::NativePanic(payload) => Ok(payload),
            other => Err(other),
        }
    }
}

impl fmt::Display for JoinFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NativePanic(payload) => {
                if let Some(message) = payload.downcast_ref::<String>() {
                    f.write_str(message)
                } else if let Some(message) = payload.downcast_ref::<&str>() {
                    f.write_str(message)
                } else {
                    write!(
                        f,
                        "非字符串 panic payload，type_id={:?}",
                        payload.as_ref().type_id()
                    )
                }
            }
            Self::WorkerDied(death) => fmt::Display::fmt(death, f),
            Self::Transport(error) => write!(f, "kind={:?}；{error}", error.kind()),
        }
    }
}

impl std::error::Error for JoinFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NativePanic(_) => None,
            Self::WorkerDied(death) => Some(death),
            Self::Transport(error) => Some(error),
        }
    }
}

impl From<io::Error> for JoinFailure {
    fn from(error: io::Error) -> Self {
        match error
            .get_ref()
            .and_then(|source| source.downcast_ref::<WorkerDeath>())
        {
            Some(death) => Self::WorkerDied(*death),
            None => Self::Transport(error),
        }
    }
}

/// 消费线程句柄。Native 保留 std 句柄，不为归一 join 增加调度或堆分配。
#[inline]
pub fn join_worker<T: Send + 'static>(handle: JoinHandle<T>) -> Result<T, JoinFailure> {
    #[cfg(not(target_family = "wasm"))]
    return handle.join().map_err(JoinFailure::NativePanic);
    #[cfg(target_family = "wasm")]
    return handle.join().map_err(JoinFailure::from);
}

/// 跨宿主的 worker 所有权句柄。底层句柄不暴露给 runtime，join 只可消费一次。
#[repr(transparent)]
pub struct WorkerHandle<T>(JoinHandle<T>);

impl<T: Send + 'static> WorkerHandle<T> {
    /// 两端分别交给 OS 线程和 Worker slot；本层不分配或另加调度。
    #[inline]
    pub fn spawn(name: String, work: impl FnOnce() -> T + Send + 'static) -> io::Result<Self> {
        Builder::new().name(name).spawn(work).map(Self)
    }

    #[inline]
    pub fn join(self) -> Result<T, JoinFailure> {
        join_worker(self.0)
    }

    /// Native 验证通过 std 观察线程是否退出；生产 runtime 无需轮询句柄。
    #[cfg(all(not(target_family = "wasm"), feature = "verification-harness"))]
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    /// 真实 Worker 故障验证读取实际槽号；产品 runtime 不看 slot。
    #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
    pub fn join_index(&self) -> u32 {
        self.0.join_index()
    }
}

/// 内联计算兼容层的逻辑分块宽度，不是实际线程数，也不用于规划 World。
/// Native 保留按 OS 资源提示分块的历史行为；Browser 内联执行，不报告虚构的硬件宽度。
pub fn default_inline_chunk_width() -> io::Result<std::num::NonZeroUsize> {
    #[cfg(not(target_family = "wasm"))]
    return available_parallelism();
    #[cfg(target_family = "wasm")]
    return Ok(std::num::NonZeroUsize::MIN);
}

/// 当前 Worker 的阻塞睡眠；Browser 须由支持原子等待的 Worker 调用。
pub use std::thread::sleep as sleep_current;
/// 请求底座调度器让位；只是调度提示，不保证其他任务被执行，不用于协议正确性。
pub use std::thread::yield_now as yield_hint;

/// 借用任务的内联执行器。两端均在调用线程立即完成，不创建池、不分配或隐式派发。
/// 需要并发进度的业务应使用独立的持久 `WorkerHandle`，不能假定这里另有消费线程。
pub struct InlineScope<'scope>(std::marker::PhantomData<&'scope ()>);

impl<'scope> InlineScope<'scope> {
    #[inline]
    pub fn spawn<F>(&self, work: F)
    where
        F: FnOnce(&Self) + Send + 'scope,
    {
        work(self);
    }

    /// 短借用在调用返回前结束，可直接写入调用线程持有的 FIFO。
    #[inline]
    pub fn run<R>(&self, work: impl FnOnce() -> R) -> R {
        work()
    }
}

#[inline]
pub fn inline_scope<'scope, R>(work: impl FnOnce(&InlineScope<'scope>) -> R) -> R {
    work(&InlineScope(std::marker::PhantomData))
}

// ---------------------------------------------------------------------------
// wasm32：槽协议接线（R4a 阶段 2，设计 §12-14）。native 侧零影响——本段
// 仅 wasm 编译；协议核心见 crate::facade_slot。
// ---------------------------------------------------------------------------
#[cfg(target_family = "wasm")]
mod wasm_impl {
    use std::io;
    use std::num::NonZeroUsize;
    use std::sync::{Arc, OnceLock};

    use crate::facade_slot::arena::SlotArena;
    use crate::facade_slot::codec::GEN_BITS_DEFAULT;
    use crate::facade_slot::platform::WasmFacadePlatform;
    use crate::facade_slot::roles::{
        join_typed, request_spawn, trampoline_run, SpawnFailure, SpawnJob, TrampolineExit,
    };

    /// ACK 超时（§8.2.1-3：10s，对齐 wasm-worker init timeout 先例）。
    pub const SPAWN_ACK_TIMEOUT_MS: u64 = 10_000;

    /// facade 运行时（arena 单例；容量由宿主按 2×(G+N+2) 传入——并行参数
    /// 不写死，无默认容量隐式初始化）。init 须在单线程期（facade init，
    /// §12.1.6：arena 预划拨）完成；未初始化的 spawn = 显式 Unsupported。
    static RUNTIME: OnceLock<Arc<SlotArena>> = OnceLock::new();

    /// facade init：arena 单线程期划拨（宿主 bootstrap 在 substrate 就绪后
    /// 调 `facade_slot_init`；容量 = 2×(G+N+2)）。
    pub fn init_wasm_facade(capacity: u32) -> Result<(), &'static str> {
        extern "C" {
            fn wasm_tombstone_std_marker() -> u32;
        }
        if unsafe { wasm_tombstone_std_marker() } != 0x57325431 {
            return Err("WASM std tombstone 版本标记不匹配");
        }
        RUNTIME
            .set(Arc::new(SlotArena::alloc(capacity, GEN_BITS_DEFAULT)))
            .map_err(|_| {
                "facade runtime 已初始化（重复 init 拒绝——换实例走整实例废弃，非重初始化）"
            })
    }

    fn runtime() -> io::Result<Arc<SlotArena>> {
        RUNTIME.get().cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "wasm32 facade runtime 未初始化：宿主 bootstrap 须先调 facade_slot_init（容量 2×(G+N+2)，§12.1.6）",
            )
        })
    }

    /// 初始化 engine/ingress/publisher 记录，engine TLS 在返回前完成绑定。
    pub fn init_tombstone_primary() -> Result<(u32, u32, u32, u32), &'static str> {
        crate::wasm_tombstone::init_primary()
    }

    /// 在 ingress 首次业务调用前验证并绑定预登记记录。
    pub fn bind_tombstone_ingress(address: u32, worker_id: u32) -> Result<(), &'static str> {
        crate::wasm_tombstone::bind_ingress(address, worker_id)
    }

    /// 在 publisher 首次发送前验证并绑定预登记记录及保留身份。
    pub fn bind_tombstone_publisher(address: u32, worker_id: u32) -> Result<(), &'static str> {
        crate::wasm_tombstone::bind_publisher(address, worker_id)
    }

    pub fn auxiliary_tombstone() -> Result<u32, &'static str> {
        crate::wasm_tombstone::auxiliary_address()
    }

    pub fn bind_auxiliary_tombstone(address: u32) -> Result<(), &'static str> {
        crate::wasm_tombstone::bind_auxiliary(address)
    }

    /// 子 trampoline 导出入口（子 Worker bootstrap 调）：返回 0=Ran、
    /// 1=Superseded；协议违约 panic ⇒ trap ⇒ 宿主 D5（§8.2.2）。
    pub fn child_trampoline(slot: u32, gen: u32, worker_id: u32) -> u32 {
        let arena = RUNTIME.get().expect(
            "facade_child_trampoline 在 runtime 未初始化时不可达（spawn 先行 ⇒ init 先行）",
        );
        match trampoline_run(&**arena, &WasmFacadePlatform, slot, gen, worker_id) {
            TrampolineExit::Ran => 0,
            TrampolineExit::Superseded => 1,
        }
    }

    pub struct Builder {
        name: Option<String>,
    }

    impl Builder {
        pub fn new() -> Builder {
            Builder { name: None }
        }

        pub fn name(mut self, name: String) -> Builder {
            self.name = Some(name);
            self
        }

        /// 槽协议 spawn（§12.1.2 H1-H5 + §13.1 谓词等待）。未初始化、
        /// 池耗尽或超时均返回显式 io::Error。
        pub fn spawn<F, T>(self, f: F) -> io::Result<JoinHandle<T>>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            let arena = runtime()?;
            let named = self.name.unwrap_or_else(|| "<unnamed>".to_owned());
            let job = SpawnJob::from_closure(f, named);
            match request_spawn::<T, _>(&arena, &WasmFacadePlatform, job, SPAWN_ACK_TIMEOUT_MS) {
                Ok(handle) => Ok(JoinHandle { inner: handle }),
                Err(failure) => Err(map_failure(failure)),
            }
        }
    }

    impl Default for Builder {
        fn default() -> Self {
            Builder::new()
        }
    }

    /// spawn 失败面 → io::Error 映射（结构化消息携带 RA/RB 计数，§14.1）。
    fn map_failure(failure: SpawnFailure) -> io::Error {
        match failure {
            SpawnFailure::SlotPoolExhausted {
                retired_fail_unacked,
                retired_gen_exhausted,
            } => io::Error::new(
                // ErrorKind::ResourceExhausted 在锁定工具链仍 unstable
                // （io_error_more 部分）——kind 取 Other，结构化计数入消息。
                io::ErrorKind::Other,
                format!(
                    "SLOT_POOL_EXHAUSTED{{retired_fail_unacked={retired_fail_unacked},retired_gen_exhausted={retired_gen_exhausted}}}（RA=垂死累积→查 R2-3；RB=健康耗尽→换实例，§13.3）"
                ),
            ),
            SpawnFailure::AckTimeout => io::Error::new(
                io::ErrorKind::TimedOut,
                "SPAWN_ACK_TIMEOUT（10s 应答超时，取消成立自回收，§8.2.1-3）",
            ),
            SpawnFailure::WorkerFailed { errcode } => io::Error::other(format!(
                "SPAWN_WORKER_FAILED{{errcode={errcode}}}（构造失败错误码已随 FAIL 载荷移交）"
            )),
        }
    }

    pub struct JoinHandle<T> {
        inner: crate::facade_slot::roles::FacadeJoinHandle<T>,
    }

    impl<T> JoinHandle<T>
    where
        T: Send + 'static,
    {
        /// 配对 join 槽索引（验证/诊断面：宿主据此在共享内存中定位 join 字）。
        pub fn join_index(&self) -> u32 {
            self.inner.join_index
        }
        pub fn join(self) -> io::Result<T> {
            join_typed::<T, _>(
                &self.inner.arena,
                &WasmFacadePlatform,
                self.inner.join_index,
                self.inner.gen,
            )
        }
    }

    /// 同阶段 1：显式 Unsupported（navigator.hardwareConcurrency 映射 =
    /// §1.1 T3，非本批范围；真实 substrate 解封后接线）。
    pub fn available_parallelism() -> io::Result<NonZeroUsize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "wasm32 available_parallelism 未实施（§1.1 T3：navigator.hardwareConcurrency 门面，substrate 解封批接线）",
        ))
    }

    /// 布局信息（宿主监督者建 Int32Array 视图用；wasm32 地址 u32）。
    pub struct FacadeLayoutInfo {
        pub base_addr: u32,
        pub header_words: u32,
        pub slot_stride: u32,
        pub join_stride: u32,
        pub capacity: u32,
        pub gen_bits: u32,
    }

    pub fn facade_layout() -> io::Result<FacadeLayoutInfo> {
        let arena = runtime()?;
        Ok(FacadeLayoutInfo {
            base_addr: arena.shared_words_addr() as u32,
            header_words: crate::facade_slot::arena::HEADER_WORDS as u32,
            slot_stride: crate::facade_slot::arena::SLOT_STRIDE as u32,
            join_stride: crate::facade_slot::arena::JOIN_STRIDE as u32,
            capacity: arena.capacity(),
            gen_bits: arena.gen_bits(),
        })
    }
}

#[cfg(target_family = "wasm")]
pub use wasm_impl::{
    auxiliary_tombstone, available_parallelism, bind_auxiliary_tombstone, bind_tombstone_ingress,
    bind_tombstone_publisher, child_trampoline, facade_layout, init_tombstone_primary,
    init_wasm_facade, Builder, JoinHandle,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// §0 合同：native 门面的三个符号就是 std::thread 本体（re-export，
    /// 非包装类型/包装函数）。
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn native_门面即std_thread本体() {
        assert_eq!(
            std::any::type_name::<Builder>(),
            std::any::type_name::<std::thread::Builder>()
        );
        assert_eq!(
            std::any::type_name::<JoinHandle<u8>>(),
            std::any::type_name::<std::thread::JoinHandle<u8>>()
        );
        // 函数指针同一：available_parallelism 是同一个静态符号。
        let facade_fn: fn() -> std::io::Result<std::num::NonZeroUsize> = available_parallelism;
        let std_fn: fn() -> std::io::Result<std::num::NonZeroUsize> =
            std::thread::available_parallelism;
        assert_eq!(facade_fn as usize, std_fn as usize);
    }

    /// spawn/join 经统一的 WorkerHandle，spawn 失败面保持 io::Result。
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn 命名spawn_join经门面往返() {
        let handle = WorkerHandle::spawn("facade-probe".to_owned(), || 41 + 1)
            .expect("native spawn 经门面不应失败");
        assert_eq!(handle.join().expect("join 不应失败"), 42);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn 原生join保留真实panic载荷() {
        let handle: WorkerHandle<()> =
            WorkerHandle::spawn("facade-panic".to_owned(), || std::panic::panic_any(42_u32))
                .expect("native spawn 不应失败");
        let Err(JoinFailure::NativePanic(payload)) = handle.join() else {
            panic!("只有真实 Native panic 应携带原载荷");
        };
        assert_eq!(payload.downcast_ref::<u32>(), Some(&42));
    }

    /// available_parallelism 经门面可用（compute.rs T3 形态）。
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn available_parallelism经门面可用() {
        let width = available_parallelism().expect("native 宽度探测不应失败");
        assert!(width.get() >= 1);
    }
}
