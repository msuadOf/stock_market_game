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
//!   runtime override；engine 内部未设置 override 时仍走串行回退。
//!   available_parallelism（§1.1 T3）维持显式 Unsupported。
//!
//! **为什么 mpsc/Mutex/Condvar 不进门面**（§0 推论 + §1.3 结论）：std 同步
//! 原语在「+atomics + build-std + 共享线性内存」产物里由 std 原生提供
//! （futex 同一代码路径，wasm 与 native 同源），包一层只会在 native 引入
//! 运行期间接——直接违反 §0。门面只收编 wasm 真正缺的三个符号。
//!
//! 上游来源边界（`vendor/web-thread/upstream/src/thread/`，commit
//! `ce376d95dbdd9e7b59ac7de9c6f14090076f7865`）：`builder.rs` 的
//! `Builder::{new,name,spawn}` 及 `spawn.rs` 的 `JoinHandle::{join,
//! is_finished}` 是 API 对照；本地 WASM `Builder::spawn` 以 `SpawnJob` 擦除
//! 闭包，经 `SlotArena` 的 REQ/ANSWERING/OK 握手由宿主创建子 Worker，
//! `join_typed` 从配对 join 槽单次取回结果。它不是上游 `atomics::spawn`
//! 的直接移植：上游使用主线程 `Command` 队列、`web_sys::Worker`、共享
//! `WebAssembly::Module/Memory` 和 `wait_async` 回收，本仓库的 Worker 创建
//! 与释放由产品宿主监督。上游 Parker 的 token 机制在 `libos::mailbox::Signal`
//! 中另行适配；本模块不以 Parker 代称完整线程层接入。上游 `Thread`/`ThreadId`
//! 未映射为伪造的本地线程身份，`available_parallelism` 也因缺少已接通的
//! `Navigator.hardwareConcurrency` 宿主接口而明确 Unsupported。WASM panic
//! 的 `DEAD`/tombstone 早期通知仍由 `wasm_tombstone` 与 trampoline 实现；
//! hard trap 不可由 Rust join 恢复为 panic payload。

// ---------------------------------------------------------------------------
// native：编译期符号别名（§0——零运行期间接；asm 对照留证于 agents 台账）。
// ---------------------------------------------------------------------------
#[cfg(not(target_family = "wasm"))]
pub use std::thread::{available_parallelism, Builder, JoinHandle};

/// 线程 join 失败的描述；原始 payload 的所有权仍由调用方持有。
#[cfg(not(target_family = "wasm"))]
pub(crate) fn join_error_detail(error: &Box<dyn std::any::Any + Send>) -> String {
    panic_payload_detail(error.as_ref())
}

#[cfg(target_family = "wasm")]
pub(crate) fn join_error_detail(error: &std::io::Error) -> String {
    error.to_string()
}

pub(crate) fn panic_payload_detail(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() { return message.clone(); }
    if let Some(message) = payload.downcast_ref::<&str>() { return (*message).to_owned(); }
    format!("非字符串 panic payload，type_id={:?}", payload.type_id())
}

/// 绝对 deadline join 结果；超时分支携带尚未消费的原句柄，可继续重试。
///
/// `Panicked` 保留 Native 原始 panic payload；WASM hard trap 以 `Failed` 明确
/// 返回。join 结果不表示宿主已释放 child 或业务资源已对账。
pub enum JoinUntilError<H> {
    /// 截止时仍未观察到 join 结果；原句柄与等待状态保留，可用新 deadline 重试。
    TimedOut(H),
    /// Native collector 未能启动；原始 worker handle 仍留在槽中，可重试。
    CollectorStartFailed(H, std::io::Error),
    /// Native collector 已启动但结果通道异常；不能证明原结果或 child 状态，不能重试。
    CollectorFailed(std::io::Error),
    /// Native worker panic；保留原始 panic payload。
    Panicked(Box<dyn std::any::Any + Send>),
    /// WASM Worker 进入 panic/hard trap 终态；错误显式返回且 join slot 已消费。
    Failed(std::io::Error),
}

#[cfg(not(target_family = "wasm"))]
mod tracked_native {
    use std::io;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::{JoinUntilError, JoinHandle};

    enum CollectorResult<T> {
        Joined(thread::Result<T>),
        Failed(io::Error),
    }

    pub struct TrackedJoinHandle<T> {
        worker: Option<JoinHandle<T>>,
        // collector 尚未启动时，此槽仍拥有 raw handle；超时后 Arc 维持可重试状态。
        worker_slot: Option<Arc<Mutex<Option<JoinHandle<T>>>>>,
        // 只保留 collector 以明确其生命周期；绝不在 deadline 调用方 join collector。
        _collector: Option<JoinHandle<()>>,
        result: Option<Receiver<CollectorResult<T>>>,
    }

    pub fn spawn<F, T>(name: Option<String>, f: F) -> io::Result<TrackedJoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let mut builder = thread::Builder::new();
        if let Some(name) = name {
            builder = builder.name(name);
        }
        let handle = builder.spawn(f)?;
        Ok(TrackedJoinHandle {
            worker: Some(handle),
            worker_slot: None,
            _collector: None,
            result: None,
        })
    }

    impl<T: Send + 'static> TrackedJoinHandle<T> {
        /// 等待到绝对 deadline；超时返回原句柄，原生 join 仅由 collector 执行。
        pub fn join_until(
            mut self,
            deadline: crate::clock::Instant,
        ) -> Result<T, JoinUntilError<Self>> {
            if self.result.is_none() {
                if self.worker_slot.is_none() {
                    let Some(worker) = self.worker.take() else {
                        return Err(JoinUntilError::CollectorFailed(
                            io::Error::other("Native tracked join 缺少 worker 所有权"),
                        ));
                    };
                    self.worker_slot = Some(Arc::new(Mutex::new(Some(worker))));
                }
                let worker_slot = self.worker_slot.as_ref().map(Arc::clone);
                let Some(worker_slot) = worker_slot else {
                    return Err(JoinUntilError::CollectorFailed(
                        io::Error::other("Native tracked join 创建 collector 时缺少 worker slot"),
                    ));
                };
                let (result_tx, result_rx) = mpsc::channel();
                let collector = thread::Builder::new()
                    .name("libos-join-collector".to_owned())
                    .spawn(move || {
                        // OS thread 的 TLS destructor 在这里的真实 join 中收束；调用方
                        // 只等 result_rx，因此 destructor 不能把调用方 deadline 变无界。
                        let worker = match worker_slot.lock() {
                            Ok(mut slot) => slot.take(),
                            Err(poisoned) => poisoned.into_inner().take(),
                        };
                        let result = match worker {
                            Some(worker) => CollectorResult::Joined(worker.join()),
                            None => CollectorResult::Failed(io::Error::other(
                                "Native join collector 未取得原始 JoinHandle",
                            )),
                        };
                        let _ = result_tx.send(result);
                    });
                match collector {
                    Ok(collector) => {
                        self._collector = Some(collector);
                        self.result = Some(result_rx);
                    }
                    Err(error) => {
                        // spawn 失败时 closure 未启动；worker_slot 仍拥有原始 handle，
                        // 保留在 TrackedJoinHandle 中供下一次调用重试。
                        drop(result_rx);
                        return Err(JoinUntilError::CollectorStartFailed(self, error));
                    }
                }
            }

            let outcome = {
                    let remaining = deadline.saturating_duration_since(crate::clock::Instant::now());
                    let received = self
                        .result
                        .as_ref()
                        .map(|receiver| receiver.recv_timeout(remaining));
                    let Some(received) = received else {
                        return Err(JoinUntilError::CollectorFailed(
                            io::Error::other("Native collector 已启动但结果接收端缺失"),
                        ));
                    };
                    match received {
                        Ok(outcome) => outcome,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            return Err(JoinUntilError::TimedOut(self));
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return Err(JoinUntilError::CollectorFailed(
                                io::Error::other(
                                    "Native join collector 已断开；无法确认原始结果是否仍可取得",
                                ),
                            ));
                        }
                    }
            };
            match outcome {
                CollectorResult::Joined(Ok(value)) => Ok(value),
                CollectorResult::Joined(Err(payload)) => Err(JoinUntilError::Panicked(payload)),
                CollectorResult::Failed(error) => {
                    Err(JoinUntilError::CollectorFailed(error))
                }
            }
        }
    }
}

#[cfg(target_family = "wasm")]
mod tracked_wasm {
    use std::io;

    use super::{wasm_impl, JoinUntilError};

    pub struct TrackedJoinHandle<T> {
        handle: wasm_impl::JoinHandle<T>,
    }

    pub fn spawn<F, T>(name: Option<String>, f: F) -> io::Result<TrackedJoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let mut builder = wasm_impl::Builder::new();
        if let Some(name) = name {
            builder = builder.name(name);
        }
        Ok(TrackedJoinHandle {
            handle: builder.spawn(f)?,
        })
    }

    impl<T> TrackedJoinHandle<T>
    where
        T: Send + 'static,
    {
        /// join 槽索引，仅用于 WASM 宿主验证与诊断。
        pub fn join_index(&self) -> u32 {
            self.handle.join_index()
        }

        /// 等待到绝对 deadline；超时保留原句柄与 generation，终态后才消费 join 槽。
        pub fn join_until(
            self,
            deadline: crate::clock::Instant,
        ) -> Result<T, JoinUntilError<Self>> {
            let ready = self.handle.wait_until(deadline);
            if !ready {
                return Err(JoinUntilError::TimedOut(self));
            }
            self.handle.join().map_err(JoinUntilError::Failed)
        }
    }
}

#[cfg(not(target_family = "wasm"))]
pub use tracked_native::TrackedJoinHandle;
#[cfg(target_family = "wasm")]
pub use tracked_wasm::TrackedJoinHandle;

/// 通过 LibOS spawn 并取得支持绝对 deadline、可超时重试的 join 句柄。
///
/// `name=None` 使用平台默认线程名；调用方应向所有 Worker 传同一 `deadline`，
/// 以共享一个收束等待上限。deadline 限定阻塞等待；若调用时已能观察到终态，
/// 即使当前时间已过 deadline 仍可立即收取结果，因此 API 不证明 Worker 在截止时刻前完成。
/// 此句柄只报告 join 结果，不代表宿主 child 释放或 receiver/资产对账。
pub fn spawn_tracked<F, T>(name: Option<String>, f: F) -> std::io::Result<TrackedJoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    #[cfg(not(target_family = "wasm"))]
    {
        tracked_native::spawn(name, f)
    }
    #[cfg(target_family = "wasm")]
    {
        tracked_wasm::spawn(name, f)
    }
}

/// facade arena 既有 slot 上限，供宿主在分配前显式校验容量。
pub const FACADE_SLOT_MAX_CAPACITY: u32 = 1 << 20;

#[cfg(any(test, target_family = "wasm"))]
fn validate_fault_capacity(capacity: u32) -> Result<(), &'static str> {
    if capacity == 0 || capacity > FACADE_SLOT_MAX_CAPACITY {
        return Err("故障域容量必须在 1..=2^20");
    }
    Ok(())
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
        join_typed, request_spawn, trampoline_run, wait_join_until, SpawnFailure, SpawnJob,
        TrampolineExit,
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
        {
            super::validate_fault_capacity(capacity)?;
            crate::libos::fault_domain::GLOBAL_FAULT.initialize(capacity as usize)?;
        }
        RUNTIME
            .set(Arc::new(SlotArena::alloc(capacity, GEN_BITS_DEFAULT)))
            .map_err(|_| {
                "facade runtime 已初始化（重复 init 拒绝——换实例走整实例废弃，非重初始化）"
            })
    }

    /// 返回宿主登记故障广播所需的共享内存布局。
    pub fn libos_host_fault_layout() -> Result<crate::libos::fault_domain::HostFaultLayout, &'static str> {
        crate::libos::fault_domain::GLOBAL_FAULT.host_layout()
    }

    fn runtime() -> io::Result<Arc<SlotArena>> {
        RUNTIME.get().cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "wasm32 facade runtime 未初始化：宿主 bootstrap 须先调 facade_slot_init（容量 2×(G+N+2)，§12.1.6）",
            )
        })
    }

    /// 初始化 engine/ingress 记录，engine TLS 在返回前完成绑定。
    pub fn init_tombstone_primary() -> Result<(u32, u32, u32), &'static str> {
        crate::wasm_tombstone::init_primary()
    }

    /// 在 ingress 首次业务调用前验证并绑定预登记记录。
    pub fn bind_tombstone_ingress(address: u32, worker_id: u32) -> Result<(), &'static str> {
        crate::wasm_tombstone::bind_ingress(address, worker_id)
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

    #[cfg(feature = "verification-harness")]
    static VERIFICATION_FAULT_GATE: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(0);
    #[cfg(feature = "verification-harness")]
    thread_local! {
        static VERIFICATION_FAULT_CHANNEL: std::cell::RefCell<Option<(
            crate::libos::mailbox::Sender<()>,
            crate::libos::mailbox::Receiver<()>,
            crate::libos::mailbox::AbortHandle,
        )>> = const { std::cell::RefCell::new(None) };
    }

    /// 验证专用：供宿主在共享 memory 中发布故障并唤醒等待字。
    #[cfg(feature = "verification-harness")]
    pub fn verification_host_fault_layout(
    ) -> Result<crate::libos::fault_domain::HostFaultLayout, &'static str> {
        crate::libos::fault_domain::GLOBAL_FAULT.host_layout()
    }

    /// 验证专用：准备登记通道并返回等待字地址；正式构建不含此接口。
    #[cfg(feature = "verification-harness")]
    pub fn verification_fault_prepare() -> Result<u32, &'static str> {
        let domain = crate::libos::fault_domain::GLOBAL_FAULT
            .domain()
            .ok_or("故障域未就绪")?;
        let (sender, receiver) = domain.channel::<()>().map_err(|_| "故障域登记失败")?;
        let handle = receiver.abort_handle();
        let address =
            u32::try_from(handle.wait_word_address()).map_err(|_| "等待字地址超出 wasm32")?;
        VERIFICATION_FAULT_CHANNEL.with(|slot| {
            let mut current = slot.borrow_mut();
            if current.is_some() {
                return Err("验证通道已存在");
            }
            *current = Some((sender, receiver, handle));
            Ok(address)
        })
    }

    /// 验证专用：阻塞至故障域广播，超时和意外消息均显式失败。
    #[cfg(feature = "verification-harness")]
    pub fn verification_fault_wait() -> Result<(), &'static str> {
        VERIFICATION_FAULT_CHANNEL.with(|slot| {
            let current = slot.borrow();
            let (_, receiver, _) = current.as_ref().ok_or("验证通道未准备")?;
            match crate::libos::mailbox::receive_interruptible(
                receiver,
                std::time::Duration::from_secs(3),
            ) {
                Err(crate::libos::mailbox::ReceiveError::Aborted) => Ok(()),
                _ => Err("验证通道未由故障唤醒"),
            }
        })
    }

    #[cfg(feature = "verification-harness")]
    pub fn verification_fault_gate_address() -> u32 {
        (&VERIFICATION_FAULT_GATE as *const std::sync::atomic::AtomicU32) as u32
    }

    /// 测试线程由宿主释放 gate，保证接收方先登记等待。
    #[cfg(feature = "verification-harness")]
    pub fn verification_fault_gate_wait() -> bool {
        use std::sync::atomic::Ordering;
        let started = crate::clock::Instant::now();
        let timeout = std::time::Duration::from_secs(3);
        while VERIFICATION_FAULT_GATE.load(Ordering::SeqCst) == 0 {
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return false;
            }
            let remaining = (timeout - elapsed).as_nanos() as i64;
            unsafe {
                std::arch::wasm32::memory_atomic_wait32(
                    (&VERIFICATION_FAULT_GATE as *const std::sync::atomic::AtomicU32)
                        .cast_mut()
                        .cast::<i32>(),
                    0,
                    remaining,
                );
            }
        }
        true
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

        /// 槽协议 spawn（§12.1.2 H1-H5 + §13.1 谓词等待）。Web 产品宿主在
        /// runtime override 启用后经 facade 调用本路径；未初始化/池耗尽/超时
        /// 均显式 io::Error。
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
        /// 等待到绝对 deadline，但不消费 join 槽或句柄。
        pub(super) fn wait_until(&self, deadline: crate::clock::Instant) -> bool {
            wait_join_until(
                &self.inner.arena,
                &WasmFacadePlatform,
                self.inner.join_index,
                self.inner.gen,
                deadline,
            )
        }

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
    available_parallelism, bind_tombstone_ingress, child_trampoline, facade_layout,
    init_tombstone_primary, init_wasm_facade, libos_host_fault_layout, Builder, JoinHandle,
};

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
pub use wasm_impl::{
    verification_fault_gate_address, verification_fault_gate_wait, verification_fault_prepare,
    verification_fault_wait, verification_host_fault_layout,
};

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn 线程_join_错误经_libos_保留原始内容() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("原始线程故障"));
        assert_eq!(super::join_error_detail(&payload), "原始线程故障");
        let literal: Box<dyn std::any::Any + Send> = Box::new("静态线程故障");
        assert_eq!(super::join_error_detail(&literal), "静态线程故障");
        let other: Box<dyn std::any::Any + Send> = Box::new(42_u32);
        assert!(super::join_error_detail(&other).contains("非字符串 panic payload"));
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn 真实线程_join_错误描述后仍保留原始_payload() {
        let error = super::Builder::new().spawn(|| std::panic::panic_any(String::from("真实线程故障")))
            .unwrap().join().unwrap_err();
        assert_eq!(super::join_error_detail(&error), "真实线程故障");
        assert_eq!(*error.downcast::<String>().unwrap(), "真实线程故障");
    }
    use super::*;

    #[test]
    fn 故障域容量必须先验证() {
        assert!(validate_fault_capacity(0).is_err());
        assert!(validate_fault_capacity(FACADE_SLOT_MAX_CAPACITY + 1).is_err());
        assert!(validate_fault_capacity(1).is_ok());
        assert!(validate_fault_capacity(FACADE_SLOT_MAX_CAPACITY).is_ok());
    }

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
