//! 平台层：futex 等待/唤醒 + H5/W3 消息面 + 测试缝。
//!
//! - **native（测试）**：wait = 轮询（值变化即返、TimedOut 仅由 harness
//!   虚拟注入）；wake = no-op（等待者自旋重读吸收，I6 不丢唤醒由构造保证）。
//!   native **生产路径不经此模块**（§0：native 门面 = std re-export）。
//! - **wasm32**：等待/唤醒/消息面全部走 env import（宿主 bootstrap =
//!   vite alias 'env' → `apps/web/src/host/facade-env.ts`，与阶段 1
//!   `engine_clock_now` 同一注入面）：
//!   - futex 等待 = 宿主 `Atomics.wait(memoryView, wordIndex, expected, timeout)`
//!     ——与 `memory.atomic.wait32` 同一原语、同一按地址等待者队列（core 的
//!     `memory_atomic_wait32` intrinsic 在 1.96.1 仍 unstable（issue 77839），
//!     crate 级 feature 属性会污染 native stable 面，故经宿主等价实现）；
//!     期望值 = 完整 W 字（I1/I2 不变；唤醒经任意线程 Atomics.notify 达）；
//!   - 主线程监督者永不 wait（WebAssembly/threads#106）——只 CAS+notify。
//!
//! 缝（seam_*）：确定性交错负测（N1-N3）的定序钩子；生产实现为 no-op
//! （spawn 为 ms 级一次性路径，非热路径）。

/// futex 等待结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutexWait {
    /// 值已 ≠ 期望（醒后由调用者重读决策）。
    ValueChanged,
    /// 截止到点（wasm=真超时；native 测试=虚拟注入）。
    TimedOut,
}

/// 平台能力面。所有角色操作（请求者/trampoline）以泛型静态分派注入。
pub trait FacadePlatform: Sync {
    /// 在 addr 上等待至值 ≠ expected 或超时（标准 futex 循环的一步；
    /// 虚假唤醒由调用者谓词循环吸收，I6）。
    fn wait_u32(
        &self,
        addr: *const std::sync::atomic::AtomicU32,
        expected: u32,
        timeout_ms: u64,
    ) -> FutexWait;
    /// 对 addr futex_wake_all（一切 W 转移后必呼，I6；换代转移同样唤醒）。
    fn wake_all_u32(&self, addr: *const std::sync::atomic::AtomicU32);
    /// H5：REQ 已发布后向监督者发 facadeSpawn 消息（§13.2 时序：H4 之后、
    /// supervisor 一切槽操作之前——REQ 未发布而消息先到会使 A1 假性失败）。
    fn notify_spawn(&self, slot: u32, gen: u32, name: &str);
    /// W3：trampoline 出口向监督者报 workerExit（W4 rust_done/W5 self.close
    /// 由宿主 bootstrap 在本回调内承接，§10.2.2 关闭全序）。
    fn worker_exit(&self, worker_id: u32, slot: u32, gen: u32);
    /// 迟到应答/取消后的 terminate 卫生义务（幂等；§12.1.3 表）。
    fn terminate_child(&self, worker_id: u32);

    // —— 测试缝（生产 no-op）——
    /// SEAM-A：trampoline wait 醒后、决策读前（§12.1.5）。
    fn seam_trampoline_wake(&self, _slot: u32) {}
    /// SEAM-A2：trampoline 读 W=(g,OK) 后、载荷读前。
    fn seam_trampoline_ok_read(&self, _slot: u32) {}
    /// SEAM-B：engine 醒见 (g,FAIL) 后、错误码载荷读前。
    fn seam_engine_fail_seen(&self, _slot: u32) {}
    /// SEAM-C0：supervisor 消息处理入口、A1 前。
    fn seam_supervisor_msg_entry(&self, _slot: u32) {}
    /// SEAM-C1：supervisor A1 成功后、A2 首次载荷 store 前（§13.1）。
    fn seam_supervisor_after_acquire(&self, _slot: u32) {}
    /// D5 已领 join 载荷写权、尚未写 code（仅确定性交错测试）。
    fn seam_dead_after_acquire(&self, _join: u32) {}
    /// 回收门链初检完成，H1取锁之前。
    fn seam_recovery_before_lock(&self) {}
}

/// 无操作平台（arena 级测试用：不等待、不发消息、缝恒 no-op）。
pub struct NullPlatform;

impl FacadePlatform for NullPlatform {
    fn wait_u32(
        &self,
        _addr: *const std::sync::atomic::AtomicU32,
        _expected: u32,
        _timeout_ms: u64,
    ) -> FutexWait {
        panic!("NullPlatform 不承载等待（等待路径须用真实平台）");
    }
    fn wake_all_u32(&self, _addr: *const std::sync::atomic::AtomicU32) {}
    fn notify_spawn(&self, _slot: u32, _gen: u32, _name: &str) {}
    fn worker_exit(&self, _worker_id: u32, _slot: u32, _gen: u32) {}
    fn terminate_child(&self, _worker_id: u32) {}
}

// ---------------------------------------------------------------------------
// wasm32 生产平台（env import 面；native 侧零引用）
// ---------------------------------------------------------------------------
#[cfg(target_family = "wasm")]
mod wasm_impl {
    use super::{FacadePlatform, FutexWait};
    use std::sync::atomic::AtomicU32;

    // 宿主 bootstrap（vite alias 'env' → facade-env.ts）提供。命名稳定；
    // 均不含时间类符号（产物门时间 import 白名单不受影响）。
    #[link(wasm_import_module = "env")]
    extern "C" {
        /// futex 等待：期望值 = 完整 W 字（Atomics.wait 同原语）。
        /// 返回 0=醒（值变化或虚假唤醒）、1=期望不符立即返回、2=超时。
        fn facade_futex_wait(word_addr: u32, expected: u32, timeout_ms: i64) -> i32;
        /// futex_wake_all（Atomics.notify 全体）。
        fn facade_futex_notify(word_addr: u32);
        /// H5：facadeSpawn 消息（name 指针+len 指入共享线性内存）。
        fn facade_spawn_notify(slot_index: u32, gen: u32, name_ptr: u32, name_len: u32);
        /// W3：workerExit（宿主承接 W4 rust_done + W5 self.close）。
        fn facade_worker_exit_notify(worker_id: u32, slot_index: u32, gen: u32);
        /// 迟到构造子 terminate（幂等卫生）。
        fn facade_terminate_child(worker_id: u32);
    }

    /// wasm 生产平台。
    pub struct WasmFacadePlatform;

    impl FacadePlatform for WasmFacadePlatform {
        fn wait_u32(&self, addr: *const AtomicU32, expected: u32, timeout_ms: u64) -> FutexWait {
            // timeout：u64::MAX 视为无限等（宿主转 Atomics.wait 无超时形）。
            let timeout = if timeout_ms == u64::MAX {
                -1i64
            } else {
                timeout_ms as i64
            };
            let r = unsafe { facade_futex_wait(addr as u32, expected, timeout) };
            match r {
                0 | 1 => FutexWait::ValueChanged,
                2 => FutexWait::TimedOut,
                // 宿主违约（返回值域外）= 绑定层 bug ⇒ 显式 panic（铁律二）。
                other => panic!("facade_futex_wait 非法返回值 {other}（宿主 bootstrap 违约）"),
            }
        }
        fn wake_all_u32(&self, addr: *const AtomicU32) {
            unsafe { facade_futex_notify(addr as u32) }
        }
        fn notify_spawn(&self, slot: u32, gen: u32, name: &str) {
            unsafe { facade_spawn_notify(slot, gen, name.as_ptr() as u32, name.len() as u32) }
        }
        fn worker_exit(&self, worker_id: u32, slot: u32, gen: u32) {
            unsafe { facade_worker_exit_notify(worker_id, slot, gen) }
        }
        fn terminate_child(&self, worker_id: u32) {
            unsafe { facade_terminate_child(worker_id) }
        }
    }
}

#[cfg(target_family = "wasm")]
pub use wasm_impl::WasmFacadePlatform;

/// `Arc<P>` 平台穿透（测试/门面持 Arc 共享平台对象；生产 wasm 为零包装
/// 静态分派单态化——Arc 仅为别名层，无运行期间接差异：所有方法转发）。
impl<P: FacadePlatform + Send> FacadePlatform for std::sync::Arc<P> {
    fn wait_u32(
        &self,
        addr: *const std::sync::atomic::AtomicU32,
        expected: u32,
        timeout_ms: u64,
    ) -> FutexWait {
        (**self).wait_u32(addr, expected, timeout_ms)
    }
    fn wake_all_u32(&self, addr: *const std::sync::atomic::AtomicU32) {
        (**self).wake_all_u32(addr)
    }
    fn notify_spawn(&self, slot: u32, gen: u32, name: &str) {
        (**self).notify_spawn(slot, gen, name)
    }
    fn worker_exit(&self, worker_id: u32, slot: u32, gen: u32) {
        (**self).worker_exit(worker_id, slot, gen)
    }
    fn terminate_child(&self, worker_id: u32) {
        (**self).terminate_child(worker_id)
    }
    fn seam_recovery_before_lock(&self) {
        (**self).seam_recovery_before_lock()
    }
    fn seam_dead_after_acquire(&self, join: u32) {
        (**self).seam_dead_after_acquire(join)
    }
    fn seam_trampoline_wake(&self, slot: u32) {
        (**self).seam_trampoline_wake(slot)
    }
    fn seam_trampoline_ok_read(&self, slot: u32) {
        (**self).seam_trampoline_ok_read(slot)
    }
    fn seam_engine_fail_seen(&self, slot: u32) {
        (**self).seam_engine_fail_seen(slot)
    }
    fn seam_supervisor_msg_entry(&self, slot: u32) {
        (**self).seam_supervisor_msg_entry(slot)
    }
    fn seam_supervisor_after_acquire(&self, slot: u32) {
        (**self).seam_supervisor_after_acquire(slot)
    }
}
