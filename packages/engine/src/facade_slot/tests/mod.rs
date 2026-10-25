//! facade_slot 测试集（R4a 阶段 2）。
//!
//! 按构件分组：codec（W 编解码）→ arena（分配器/退休/四账）→ lifecycle
//! （角色操作正测）→ interleave（N1/N1-b/N2/N3/N3-b 确定性交错负测）→
//! exhaustion（T-耗-1~4 四账恒等式）。

mod arena_tests;
mod codec_tests;
mod exhaustion_tests;
mod interleave_tests;
mod lifecycle_tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::facade_slot::arena::SlotArena;
use crate::facade_slot::platform::{FacadePlatform, FutexWait, NullPlatform};

/// H1 持有下执行（分配器锁纪律：H2/H3-H4/push 均须持锁；单线程静点无竞争，
/// NullPlatform 不等待）。
pub(crate) fn with_h1<R>(arena: &SlotArena, f: impl FnOnce(&SlotArena) -> R) -> R {
    let _guard = H1Guard::new(arena, &NullPlatform);
    f(arena)
}

/// 测试缝闸（§12.1.5：命中即 post PAUSED 并 wait 冻结字——harness 定序）。
/// 未 arm 时命中为 no-op（生产等价面：默认不触发）。
#[derive(Default)]
pub(crate) struct SeamGate {
    armed: AtomicBool,
    hits: AtomicU32,
    release: AtomicU32,
}

impl SeamGate {
    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
    /// 角色侧：命中缝（armed 时冻结至 harness 放行）。
    pub fn hit_and_wait(&self) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        self.hits.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.release.load(Ordering::SeqCst) == 0 {
            if Instant::now() > deadline {
                panic!("测试缝放行超时：harness 未 release（测试脚本死锁）");
            }
            thread::yield_now();
        }
        self.release.fetch_sub(1, Ordering::SeqCst);
    }
    /// harness 侧：等 PAUSED。
    pub fn wait_hit(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.hits.load(Ordering::SeqCst) == 0 {
            if Instant::now() > deadline {
                panic!("等待测试缝命中超时：角色未到达冻结点（5s）");
            }
            thread::yield_now();
        }
    }
    /// harness 侧：放行一次并撤防（首命中冻结语义——设计的 SEAM 只钉住
    /// 目标交错方；后续命中者（如 N1 的 g+1 新子）直行，不逐次累计令牌）。
    /// 时序前提：harness 已 wait_hit 确认目标方命中后才调用。
    pub fn release_once(&self) {
        self.release.fetch_add(1, Ordering::SeqCst);
        self.armed.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub(crate) struct SeamSet {
    /// SEAM-A：trampoline wait 醒后、决策读前。
    pub a: SeamGate,
    /// SEAM-A2：trampoline 读 W=(g,OK) 后、载荷读前。
    pub a2: SeamGate,
    /// SEAM-B：engine 醒见 FAIL 后、错误码载荷读前。
    pub b: SeamGate,
    /// SEAM-C0：supervisor 消息处理入口、A1 前。
    pub c0: SeamGate,
    /// SEAM-C1：supervisor A1 成功后、A2 首次载荷 store 前。
    pub c1: SeamGate,
    pub d: SeamGate,
    pub recovery: SeamGate,
}

/// native 测试平台：wait 为轮询（值变化即返）；TimedOut **仅**由 harness
/// 注入的虚拟截止触发（T-缩 的确定性等价）——生产 wasm 平台为真 futex 超时。
pub(crate) struct TestPlatform {
    pub seams: SeamSet,
    deadlines: Mutex<HashMap<usize, Arc<AtomicBool>>>,
    pub notify_spawn_log: Mutex<Vec<(u32, u32, String)>>,
    pub worker_exit_log: Mutex<Vec<(u32, u32, u32)>>,
    pub terminate_log: Mutex<Vec<u32>>,
    pub wake_log: Mutex<Vec<usize>>,
    pub wait_log: Mutex<Vec<u32>>,
    spin_cap: Duration,
}

impl Default for TestPlatform {
    fn default() -> Self {
        Self {
            seams: SeamSet::default(),
            deadlines: Mutex::new(HashMap::new()),
            notify_spawn_log: Mutex::new(Vec::new()),
            worker_exit_log: Mutex::new(Vec::new()),
            terminate_log: Mutex::new(Vec::new()),
            wake_log: Mutex::new(Vec::new()),
            wait_log: Mutex::new(Vec::new()),
            spin_cap: Duration::from_secs(8),
        }
    }
}

impl TestPlatform {
    /// harness 注入虚拟 ACK 截止（等价 T-缩 到点）。
    pub fn fire_deadline(&self, addr: *const AtomicU32) {
        let mut map = self.deadlines.lock().expect("deadlines 锁中毒");
        map.entry(addr as usize)
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .store(true, Ordering::SeqCst);
    }
    /// harness 撤销虚拟截止（粘性标志——多等待者共享一字时不可自动消费，
    /// 由 harness 在选定时机显式撤销；见 n1）。
    pub fn clear_deadline(&self, addr: *const AtomicU32) {
        let mut map = self.deadlines.lock().expect("deadlines 锁中毒");
        if let Some(flag) = map.get_mut(&(addr as usize)) {
            flag.store(false, Ordering::SeqCst);
        }
    }
    /// harness 等 H5 消息（第 n 条，0 基）。
    pub fn wait_notify_spawn(&self, n: usize) -> (u32, u32, String) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            {
                let log = self.notify_spawn_log.lock().expect("notify 日志锁中毒");
                if log.len() > n {
                    return log[n].clone();
                }
            }
            if Instant::now() > deadline {
                panic!("等待 notify_spawn 第 {n} 条超时（5s）");
            }
            thread::yield_now();
        }
    }
    /// harness 等 workerExit 消息（W3）出现。
    pub fn wait_worker_exit(&self) -> (u32, u32, u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            {
                let log = self.worker_exit_log.lock().expect("exit 日志锁中毒");
                if let Some(entry) = log.first() {
                    return *entry;
                }
            }
            if Instant::now() > deadline {
                panic!("等待 workerExit 超时（5s）");
            }
            thread::yield_now();
        }
    }
}

/// harness 观测/驱动面（**非**平台契约：`FacadePlatform` 是生产平台能力面，
/// 平台实现方不必也不该提供这些。写进 trait 会强制 wasm 生产平台与
/// `NullPlatform` 实现无意义的测试桩；故留在 inherent impl，测试侧直接调用）。
impl TestPlatform {
    /// harness 观测：某地址上的 wake 次数（I6 断言面）。
    pub fn wake_count(&self, addr: *const AtomicU32) -> usize {
        self.wake_log
            .lock()
            .expect("wake 日志锁中毒")
            .iter()
            .filter(|&&a| a == addr as usize)
            .count()
    }
}

impl FacadePlatform for TestPlatform {
    fn wait_u32(&self, addr: *const AtomicU32, expected: u32, _timeout_ms: u64) -> FutexWait {
        self.wait_log.lock().unwrap().push(expected);
        let start = Instant::now();
        loop {
            let cur = unsafe { (*addr).load(Ordering::SeqCst) };
            if cur != expected {
                return FutexWait::ValueChanged;
            }
            // 每轮重读注册表：虚拟截止可在等待者入场后注入。
            let deadline_fired = {
                let map = self.deadlines.lock().expect("deadlines 锁中毒");
                map.get(&(addr as usize))
                    .is_some_and(|flag| flag.load(Ordering::SeqCst))
            };
            if deadline_fired {
                return FutexWait::TimedOut;
            }
            if start.elapsed() > self.spin_cap {
                panic!("TestPlatform wait 超安全上限（8s）：疑似协议死锁或 harness 未注入事件");
            }
            thread::yield_now();
        }
    }
    fn wake_all_u32(&self, addr: *const AtomicU32) {
        // 轮询型 wait：唤醒由等待者自旋重读吸收（I6 不丢唤醒由构造保证）。
        // **wake 计数（复核补记）**：此前 wake 是纯 no-op ⇒ "一切 W 转移后
        // wake_all"这条纪律在 native 测试面完全不可观测。计数入账后 I6 的
        // 可断言部分（转移次数对应的 wake 次数）可被测试钉住。
        self.wake_log
            .lock()
            .expect("wake 日志锁中毒")
            .push(addr as usize);
    }
    fn notify_spawn(&self, slot: u32, gen: u32, name: &str) {
        self.notify_spawn_log
            .lock()
            .expect("notify 日志锁中毒")
            .push((slot, gen, name.to_owned()));
    }
    fn worker_exit(&self, worker_id: u32, slot: u32, gen: u32) {
        self.worker_exit_log
            .lock()
            .expect("exit 日志锁中毒")
            .push((worker_id, slot, gen));
    }
    fn terminate_child(&self, worker_id: u32) {
        self.terminate_log
            .lock()
            .expect("terminate 日志锁中毒")
            .push(worker_id);
    }
    fn seam_recovery_before_lock(&self) { self.seams.recovery.hit_and_wait(); }
    fn seam_dead_after_acquire(&self, _join: u32) {
        self.seams.d.hit_and_wait();
    }
    fn seam_trampoline_wake(&self, slot: u32) {
        let _ = slot;
        self.seams.a.hit_and_wait();
    }
    fn seam_trampoline_ok_read(&self, slot: u32) {
        let _ = slot;
        self.seams.a2.hit_and_wait();
    }
    fn seam_engine_fail_seen(&self, slot: u32) {
        let _ = slot;
        self.seams.b.hit_and_wait();
    }
    fn seam_supervisor_msg_entry(&self, slot: u32) {
        let _ = slot;
        self.seams.c0.hit_and_wait();
    }
    fn seam_supervisor_after_acquire(&self, slot: u32) {
        let _ = slot;
        self.seams.c1.hit_and_wait();
    }
}

/// 局部测试锁守卫：panic 展开时也归还 H1。
pub(crate) struct H1Guard<'a, P: FacadePlatform> {
    arena: &'a SlotArena,
    platform: &'a P,
}
impl<'a, P: FacadePlatform> H1Guard<'a, P> {
    pub fn new(arena: &'a SlotArena, platform: &'a P) -> Self {
        arena.h1_lock_blocking(platform);
        Self { arena, platform }
    }
}
impl<P: FacadePlatform> Drop for H1Guard<'_, P> {
    fn drop(&mut self) {
        self.arena.h1_unlock(self.platform);
    }
}
