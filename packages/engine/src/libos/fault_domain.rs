//! 实例级故障广播：登记槽在 domain 生命周期内保持地址稳定。
use super::mailbox::Signal;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

#[derive(Debug, PartialEq, Eq)]
pub enum RegisterError {
    Aborted,
    Full,
}
struct Slot {
    leased: AtomicBool,
    signal: Arc<Signal>,
}
pub struct FaultDomain {
    aborted: AtomicBool,
    slots: Vec<Slot>,
}
pub struct Registration {
    domain: Arc<FaultDomain>,
    index: usize,
}
impl FaultDomain {
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            aborted: AtomicBool::new(false),
            slots: (0..capacity)
                .map(|_| Slot {
                    leased: AtomicBool::new(false),
                    signal: Arc::new(Signal::new()),
                })
                .collect(),
        })
    }
    pub(crate) fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }
    pub(crate) fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn register(self: &Arc<Self>) -> Result<Registration, RegisterError> {
        self.register_with(|| {})
    }
    fn register_with(
        self: &Arc<Self>,
        after_check: impl Fn(),
    ) -> Result<Registration, RegisterError> {
        if self.aborted.load(Ordering::SeqCst) {
            return Err(RegisterError::Aborted);
        }
        after_check();
        for (index, slot) in self.slots.iter().enumerate() {
            if slot
                .leased
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                slot.signal.aborted.store(false, Ordering::SeqCst);
                if self.aborted.load(Ordering::SeqCst) {
                    slot.leased.store(false, Ordering::Release);
                    return Err(RegisterError::Aborted);
                }
                return Ok(Registration {
                    domain: self.clone(),
                    index,
                });
            }
        }
        if self.aborted.load(Ordering::SeqCst) {
            Err(RegisterError::Aborted)
        } else {
            Err(RegisterError::Full)
        }
    }
    pub fn channel<T>(
        self: &Arc<Self>,
    ) -> Result<(super::mailbox::Sender<T>, super::mailbox::Receiver<T>), RegisterError> {
        let lease = Arc::new(self.register()?);
        let signal = self.slots[lease.index].signal.clone();
        Ok(super::mailbox::channel_with_registration(signal, lease))
    }
    pub fn abort(&self) {
        self.abort_with(|| {});
    }
    fn abort_with(&self, after_mark: impl FnOnce()) {
        self.aborted.store(true, Ordering::SeqCst);
        after_mark();
        for slot in &self.slots {
            slot.signal.aborted.store(true, Ordering::SeqCst);
            slot.signal.notify();
        }
    }
}
impl Registration {
    pub(super) fn domain_aborted(&self) -> bool {
        self.domain.aborted.load(Ordering::SeqCst)
    }
    pub fn notified(&self) -> bool {
        self.domain.slots[self.index]
            .signal
            .aborted
            .load(Ordering::Acquire)
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.domain.slots[self.index]
            .leased
            .store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn 故障域已标记但槽尚未通知时下行拒绝新载荷() {
        let domain = FaultDomain::new(1);
        let (sender, receiver) = super::super::downlink::registered_channel(&domain).unwrap();
        domain.abort_with(|| {
            assert_eq!(sender.send(42).unwrap_err().0, 42);
        });
        let closed = receiver.close_and_wait(crate::clock::Instant::now() + std::time::Duration::from_millis(500)).unwrap();
        assert!(closed.drain(std::num::NonZeroUsize::new(1).unwrap()).messages.is_empty());
    }
    use super::*;
    #[test]
    fn 故障广播所有活跃登记并拒绝后续登记() {
        let domain = FaultDomain::new(2);
        let first = domain.register().unwrap();
        let second = domain.register().unwrap();
        domain.abort();
        assert!(first.notified());
        assert!(second.notified());
        drop(first);
        assert!(matches!(domain.register(), Err(RegisterError::Aborted)));
    }
    #[test]
    fn 正常释放后槽可以重用() {
        let domain = FaultDomain::new(1);
        let first = domain.register().unwrap();
        assert!(matches!(domain.register(), Err(RegisterError::Full)));
        drop(first);
        assert!(domain.register().is_ok());
    }
    #[test]
    fn 故障能唤醒登记的等待字() {
        use std::{sync::mpsc, time::Duration};
        let domain = FaultDomain::new(1);
        let registration = domain.register().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            registration.domain.slots[registration.index]
                .signal
                .wait(Duration::from_secs(1));
            done_tx.send(registration.notified()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        domain.abort();
        assert_eq!(done_rx.recv_timeout(Duration::from_millis(300)), Ok(true));
        worker.join().unwrap();
    }
    #[test]
    fn 登记与故障并发时不能遗漏故障() {
        use std::sync::Barrier;
        for _ in 0..128 {
            let domain = FaultDomain::new(1);
            let gate = Arc::new(Barrier::new(2));
            let child_domain = domain.clone();
            let child_gate = gate.clone();
            let worker = std::thread::spawn(move || {
                child_gate.wait();
                child_domain.register()
            });
            gate.wait();
            domain.abort();
            match worker.join().unwrap() {
                Ok(registration) => assert!(registration.notified()),
                Err(RegisterError::Aborted) => (),
                Err(RegisterError::Full) => panic!("空槽不能报告 Full"),
            }
        }
    }
    #[test]
    fn 故障域唤醒登记通道且存活引用阻止槽重用() {
        use std::{sync::mpsc, time::Duration};
        let domain = FaultDomain::new(1);
        let (sender, receiver) = domain.channel::<u32>().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            done_tx
                .send(super::super::mailbox::receive_interruptible(
                    &receiver,
                    Duration::from_secs(1),
                ))
                .unwrap();
        });
        assert!(matches!(domain.register(), Err(RegisterError::Full)));
        domain.abort();
        let outcome = done_rx.recv_timeout(Duration::from_millis(300)).unwrap();
        assert_eq!(outcome, Err(super::super::mailbox::ReceiveError::Aborted));
        drop(sender);
        worker.join().unwrap();
    }
    #[test]
    fn 槽在最后一个故障句柄释放后才重用() {
        let domain = FaultDomain::new(1);
        let (sender, receiver) = domain.channel::<u32>().unwrap();
        let handle = receiver.abort_handle();
        drop(sender);
        drop(receiver);
        assert!(matches!(domain.register(), Err(RegisterError::Full)));
        drop(handle);
        assert!(domain.register().is_ok());
    }
    #[test]
    fn 单通道故障后释放槽的新通道不继承故障() {
        use super::super::mailbox::receive_interruptible;
        use std::time::Duration;
        let domain = FaultDomain::new(1);
        let (sender, receiver) = domain.channel::<u32>().unwrap();
        let handle = receiver.abort_handle();
        handle.abort();
        drop(handle);
        drop(sender);
        drop(receiver);
        let (next_sender, next_receiver) = domain.channel::<u32>().unwrap();
        next_sender.send(42).unwrap();
        assert_eq!(
            receive_interruptible(&next_receiver, Duration::from_millis(100)),
            Ok(42)
        );
    }
    #[test]
    fn 登记满槽时发生故障应返回故障() {
        let domain = FaultDomain::new(1);
        let _occupied = domain.register().unwrap();
        assert!(matches!(
            domain.register_with(|| domain.abort()),
            Err(RegisterError::Aborted)
        ));
    }
}

#[cfg(target_family = "wasm")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostFaultSlot {
    abort_address: u32,
    wait_address: u32,
}
#[cfg(target_family = "wasm")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostFaultLayout {
    global_state_address: u32,
    domain_abort_address: u32,
    host_ready_address: u32,
    slots: Vec<HostFaultSlot>,
}

/// 实例生命周期内持有故障域，保证早期故障路径读取的地址不悬空。
pub struct GlobalFault {
    state: AtomicU32,
    host_ready: AtomicU32,
    domain: std::sync::OnceLock<Arc<FaultDomain>>,
}

/// 检查实例级故障；平台细节留在 LibOS 内。
pub(crate) fn instance_failed() -> bool {
    #[cfg(target_family = "wasm")]
    {
        GLOBAL_FAULT.is_failed()
            || unsafe { (*crate::wasm_tombstone::death_word()).load(Ordering::SeqCst) != 0 }
    }
    #[cfg(not(target_family = "wasm"))]
    { false }
}
const GLOBAL_READY: u32 = 1;
const GLOBAL_FAILED: u32 = 2;
impl GlobalFault {
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
            host_ready: AtomicU32::new(0),
            domain: std::sync::OnceLock::new(),
        }
    }
    pub fn initialize(&self, capacity: usize) -> Result<(), &'static str> {
        self.initialize_with(capacity, || {})
    }
    fn initialize_with(&self, capacity: usize, after_set: impl Fn()) -> Result<(), &'static str> {
        if self.state.load(Ordering::SeqCst) & GLOBAL_FAILED != 0 {
            return Err("实例已故障，不能初始化故障域");
        }
        self.domain
            .set(FaultDomain::new(capacity))
            .map_err(|_| "故障域已初始化")?;
        after_set();
        let previous = self.state.fetch_or(GLOBAL_READY, Ordering::SeqCst);
        if previous & GLOBAL_FAILED != 0 {
            self.domain.get().expect("已发布的故障域缺失").abort();
            return Err("实例初始化故障域时已故障");
        }
        Ok(())
    }
    pub fn abort(&self) {
        let previous = self.state.fetch_or(GLOBAL_FAILED, Ordering::SeqCst);
        if previous & GLOBAL_READY != 0 {
            match self.domain.get() {
                Some(domain) => domain.abort(),
                None => std::process::abort(),
            }
        }
    }
    #[cfg(target_family = "wasm")]
    pub fn host_layout(&self) -> Result<HostFaultLayout, &'static str> {
        if std::mem::size_of::<AtomicBool>() != 1 || std::mem::size_of::<AtomicU32>() != 4 {
            return Err("故障域原子布局不兼容宿主");
        }
        let domain = self.domain().ok_or("故障域未就绪或已故障")?;
        let global_state_address = u32::try_from((&self.state as *const AtomicU32) as usize)
            .map_err(|_| "全局状态地址超出 wasm32")?;
        let domain_abort_address = u32::try_from((&domain.aborted as *const AtomicBool) as usize)
            .map_err(|_| "故障域标志地址超出 wasm32")?;
        let host_ready_address = u32::try_from((&self.host_ready as *const AtomicU32) as usize)
            .map_err(|_| "宿主准备标志地址超出 wasm32")?;
        let slots = domain
            .slots
            .iter()
            .map(|slot| {
                let (abort_address, wait_address) = slot.signal.host_addresses()?;
                Ok(HostFaultSlot {
                    abort_address,
                    wait_address,
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        Ok(HostFaultLayout {
            global_state_address,
            domain_abort_address,
            host_ready_address,
            slots,
        })
    }
    pub fn host_is_ready(&self) -> bool {
        self.host_ready.load(Ordering::SeqCst) == 1
    }
    pub fn is_failed(&self) -> bool {
        self.state.load(Ordering::SeqCst) & GLOBAL_FAILED != 0
    }
    pub fn domain(&self) -> Option<&Arc<FaultDomain>> {
        let state = self.state.load(Ordering::SeqCst);
        if state & GLOBAL_READY == 0 || state & GLOBAL_FAILED != 0 {
            return None;
        }
        self.domain.get()
    }
}

#[cfg(test)]
mod global_tests {
    use super::*;
    #[test]
    fn 已初始化域的早期故障广播至登记槽() {
        let global = GlobalFault::new();
        global.initialize(1).unwrap();
        let registration = global.domain().unwrap().register().unwrap();
        global.abort();
        assert!(registration.notified());
    }
    #[test]
    fn 故障早于初始化时不得发布健康域() {
        let global = GlobalFault::new();
        global.abort();
        assert!(global.initialize(1).is_err());
        assert!(global.domain().is_none());
        if let Some(domain) = global.domain.get() {
            assert!(matches!(domain.register(), Err(RegisterError::Aborted)));
        }
    }
    #[test]
    fn 初始化与早期故障竞争后域不可健康接收登记() {
        use std::sync::Barrier;
        for _ in 0..128 {
            let global = Arc::new(GlobalFault::new());
            let gate = Arc::new(Barrier::new(2));
            let child = global.clone();
            let child_gate = gate.clone();
            let worker = std::thread::spawn(move || {
                child_gate.wait();
                child.initialize(1)
            });
            gate.wait();
            global.abort();
            let _ = worker.join().unwrap();
            assert!(global.domain().is_none());
            if let Some(domain) = global.domain.get() {
                assert!(matches!(domain.register(), Err(RegisterError::Aborted)));
            }
        }
    }
    #[test]
    fn 域存入与发布之间故障仍传播() {
        let global = GlobalFault::new();
        assert!(global.initialize_with(1, || global.abort()).is_err());
        assert!(global.domain().is_none());
        assert!(matches!(
            global.domain.get().unwrap().register(),
            Err(RegisterError::Aborted)
        ));
    }
    #[test]
    fn 故障域未发布前不可取用() {
        let global = GlobalFault::new();
        global
            .initialize_with(1, || assert!(global.domain().is_none()))
            .unwrap();
        assert!(global.domain().is_some());
    }
    #[test]
    fn 宿主确认前不得视为可用() {
        let global = GlobalFault::new();
        global.initialize(1).unwrap();
        assert!(!global.host_is_ready());
        global.host_ready.store(1, Ordering::SeqCst);
        assert!(global.host_is_ready());
    }
}

#[cfg(target_family = "wasm")]
pub(crate) static GLOBAL_FAULT: GlobalFault = GlobalFault::new();
