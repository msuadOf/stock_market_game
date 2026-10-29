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
//!   划拨 arena；未初始化的 spawn = 显式 Unsupported。当前 wasm 运行时
//!   仍为串行管线（step_runtime_enabled=false），本路径只在真实 substrate
//!   解封后可达；available_parallelism（§1.1 T3）维持显式 Unsupported 至
//!   substrate 解封批（阶段边界登记于 agents/wasm-thread-r4a/）。
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

/// facade arena 既有 slot 上限，供宿主在分配前显式校验容量。
pub const FACADE_SLOT_MAX_CAPACITY: u32 = 1 << 20;

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

    /// 子 trampoline 导出入口（子 Worker bootstrap 调）：返回 0=Ran、
    /// 1=Superseded；协议违约 panic ⇒ trap ⇒ 宿主 D5（§8.2.2）。
    pub fn child_trampoline(slot: u32, gen: u32, worker_id: u32) -> u32 {
        let arena = RUNTIME.get().expect(
            "facade_child_trampoline 在 runtime 未初始化时不可达（spawn 先行 ⇒ init 先行）",
        );
        match trampoline_run(&**arena, &WasmFacadePlatform, slot, gen, worker_id) {
            TrampolineExit::Ran { .. } => 0,
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

        /// 槽协议 spawn（§12.1.2 H1-H5 + §13.1 谓词等待）。当前 wasm 运行
        /// 时仍为串行管线（step_runtime_enabled=false），本路径只在真实
        /// substrate 解封后可达；未初始化/池耗尽/超时均显式 io::Error。
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
    available_parallelism, child_trampoline, facade_layout, init_wasm_facade, Builder, JoinHandle,
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

    /// spawn/join 经门面可用（world.rs T1/T2 生产形态的最小复刻：命名
    /// spawn → JoinHandle::join → 取返回值；spawn 失败面保持 io::Result）。
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn 命名spawn_join经门面往返() {
        let handle = Builder::new()
            .name("facade-probe".to_owned())
            .spawn(|| 41 + 1)
            .expect("native spawn 经门面不应失败");
        assert_eq!(handle.join().expect("join 不应失败"), 42);
    }

    /// available_parallelism 经门面可用（compute.rs T3 形态）。
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn available_parallelism经门面可用() {
        let width = available_parallelism().expect("native 宽度探测不应失败");
        assert!(width.get() >= 1);
    }
}
