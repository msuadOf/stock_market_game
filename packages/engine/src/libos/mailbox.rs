//! LibOS 单接收者消息通道：发送和断连均通知等待者。
//! Parker 的 token 状态机改编自 vendor/web-thread/upstream/src/thread/atomics/parker.rs，
//! 上游采用 MIT OR Apache-2.0；原始版权和许可证保留在 vendor/web-thread/upstream。
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, TryRecvError},
    Arc,
};
use std::time::Duration;

const EMPTY: u32 = 0;
const NOTIFIED: u32 = 1;
const PARKED: u32 = u32::MAX;

pub(super) struct Signal {
    pub(super) aborted: AtomicBool,
    state: AtomicU32,
    #[cfg(not(target_family = "wasm"))]
    lock: std::sync::Mutex<()>,
    #[cfg(not(target_family = "wasm"))]
    wake: std::sync::Condvar,
}
impl Signal {
    #[cfg(test)]
    pub(super) fn is_parked(&self) -> bool { self.state.load(Ordering::Acquire) == PARKED }
    pub(super) fn new() -> Self {
        Self {
            aborted: AtomicBool::new(false),
            state: AtomicU32::new(EMPTY),
            #[cfg(not(target_family = "wasm"))]
            lock: std::sync::Mutex::new(()),
            #[cfg(not(target_family = "wasm"))]
            wake: std::sync::Condvar::new(),
        }
    }
    #[cfg(target_family = "wasm")]
    pub(super) fn host_addresses(&self) -> Result<(u32, u32), &'static str> {
        if std::mem::size_of::<AtomicBool>() != 1 || std::mem::size_of::<AtomicU32>() != 4 {
            return Err("故障槽原子布局不兼容宿主");
        }
        let abort = u32::try_from((&self.aborted as *const AtomicBool) as usize)
            .map_err(|_| "故障标志地址超出 wasm32")?;
        let wait = u32::try_from((&self.state as *const AtomicU32) as usize)
            .map_err(|_| "等待字地址超出 wasm32")?;
        Ok((abort, wait))
    }
    pub(super) fn notify(&self) {
        #[cfg(not(target_family = "wasm"))]
        let _guard = self.lock.lock().expect("LibOS 通知锁已中毒");
        if self.state.swap(NOTIFIED, Ordering::Release) == PARKED {
            #[cfg(not(target_family = "wasm"))]
            self.wake.notify_one();
            #[cfg(target_family = "wasm")]
            unsafe {
                std::arch::wasm32::memory_atomic_notify(
                    &self.state as *const AtomicU32 as *mut i32,
                    1,
                );
            }
        }
    }
    pub(super) fn wait(&self, timeout: Duration) {
        #[cfg(not(target_family = "wasm"))]
        let guard = self.lock.lock().expect("LibOS 等待锁已中毒");
        if self.state.fetch_sub(1, Ordering::Acquire) == NOTIFIED {
            return;
        }
        #[cfg(not(target_family = "wasm"))]
        let _guard = self
            .wake
            .wait_timeout_while(guard, timeout, |_| {
                self.state.load(Ordering::Acquire) == PARKED
            })
            .expect("LibOS 等待锁已中毒");
        #[cfg(target_family = "wasm")]
        unsafe {
            // 溢出时使用最大有限值，不能转换成表示无限等待的 -1。
            let nanos = i64::try_from(timeout.as_nanos()).unwrap_or(i64::MAX);
            std::arch::wasm32::memory_atomic_wait32(
                &self.state as *const AtomicU32 as *mut i32,
                PARKED as i32,
                nanos,
            );
        }
        self.state.swap(EMPTY, Ordering::Acquire);
    }
}

pub struct Sender<T> {
    inner: Option<mpsc::Sender<T>>,
    signal: Arc<Signal>,
    registration: Option<Arc<super::fault_domain::Registration>>,
}
pub struct Receiver<T> {
    inner: mpsc::Receiver<T>,
    interrupted: std::cell::RefCell<Option<T>>,
    signal: Arc<Signal>,
    registration: Option<Arc<super::fault_domain::Registration>>,
}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Some(self.inner.as_ref().expect("发送者已释放").clone()),
            signal: self.signal.clone(),
            registration: self.registration.clone(),
        }
    }
}
impl<T> Sender<T> {
    pub(crate) fn wake(&self) { self.signal.notify(); }
    pub(crate) fn is_aborted(&self) -> bool {
        self.registration.as_ref().is_some_and(|registration| registration.domain_aborted())
            || self.signal.aborted.load(Ordering::SeqCst)
    }
    pub fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        // 故障前已通过检查的发送仍可能并发完成；清理方必须等待发送者退出。
        if self.is_aborted() || super::fault_domain::instance_failed() {
            return Err(mpsc::SendError(value));
        }
        self.send_admitted(value)
    }
    pub(super) fn send_admitted(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        let result = self.inner.as_ref().expect("发送者已释放").send(value);
        self.signal.notify();
        result
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        drop(self.inner.take());
        self.signal.notify();
    }
}
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    make_channel(Arc::new(Signal::new()), None)
}

#[cfg(test)]
mod interrupted_message_tests {
    use super::*;

    #[test]
    fn 保留已出队消息并先于后续消息交给清理路径() {
        let (tx, rx) = channel();
        tx.send(String::from("原消息")).unwrap();
        tx.send(String::from("后续消息")).unwrap();
        let first = rx.try_recv().unwrap();
        rx.retain_interrupted(first);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec!["原消息", "后续消息"]);
    }
}
pub(super) fn channel_with_registration<T>(
    signal: Arc<Signal>,
    registration: Arc<super::fault_domain::Registration>,
) -> (Sender<T>, Receiver<T>) {
    make_channel(signal, Some(registration))
}
fn make_channel<T>(
    signal: Arc<Signal>,
    registration: Option<Arc<super::fault_domain::Registration>>,
) -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    (
        Sender {
            inner: Some(tx),
            signal: signal.clone(),
            registration: registration.clone(),
        },
        Receiver {
            inner: rx,
            interrupted: std::cell::RefCell::new(None),
            signal,
            registration,
        },
    )
}
#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveError<T> {
    Timeout {
        waited: Duration,
    },
    Disconnected,
    Aborted,
    AbortedWithMessage(T),
    ClockRegressed {
        previous: Duration,
        observed: Duration,
    },
}

pub struct AbortHandle {
    signal: Arc<Signal>,
    _registration: Option<Arc<super::fault_domain::Registration>>,
}
impl AbortHandle {
    #[cfg(feature = "verification-harness")]
    pub fn wait_word_address(&self) -> usize {
        (&self.signal.state as *const AtomicU32) as usize
    }
    #[cfg(any(test, feature = "verification-harness"))]
    pub fn waiting_registered(&self) -> bool {
        self.signal.state.load(Ordering::Acquire) == PARKED
    }
    pub fn abort(&self) {
        self.signal.aborted.store(true, Ordering::Release);
        self.signal.notify();
    }
}
impl<T> Receiver<T> {
    pub(crate) fn is_aborted(&self) -> bool {
        self.registration.as_ref().is_some_and(|registration| registration.domain_aborted())
            || self.signal.aborted.load(Ordering::Acquire)
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.interrupted.borrow_mut().take() {
            Some(message) => Ok(message),
            None => self.inner.try_recv(),
        }
    }
    pub fn try_iter(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(move || self.try_recv().ok())
    }
    pub(crate) fn retain_interrupted(&self, message: T) {
        let mut interrupted = self.interrupted.borrow_mut();
        assert!(interrupted.is_none(), "LibOS 不可覆盖未处理的在途消息");
        *interrupted = Some(message);
    }
    pub fn abort_handle(&self) -> AbortHandle {
        AbortHandle {
            signal: self.signal.clone(),
            _registration: self.registration.clone(),
        }
    }
}
fn aborted_after_poll<T>(result: Result<T, TryRecvError>) -> ReceiveError<T> {
    match result {
        Ok(value) => ReceiveError::AbortedWithMessage(value),
        Err(_) => ReceiveError::Aborted,
    }
}
pub fn receive_interruptible<T>(rx: &Receiver<T>, timeout: Duration) -> Result<T, ReceiveError<T>> {
    receive_when_inner(rx, timeout, || {}, || false)
}

/// 平台故障标志参与每次出队前后的判定；通知仍由所属故障域送达。
pub fn receive_interruptible_when<T>(
    rx: &Receiver<T>,
    timeout: Duration,
    external_fault: impl Fn() -> bool,
) -> Result<T, ReceiveError<T>> {
    receive_when_inner(rx, timeout, || {}, external_fault)
}

#[cfg(test)]
fn receive_interruptible_inner<T>(
    rx: &Receiver<T>,
    timeout: Duration,
    after_poll: impl Fn(),
) -> Result<T, ReceiveError<T>> {
    receive_when_inner(rx, timeout, after_poll, || false)
}

fn receive_when_inner<T>(
    rx: &Receiver<T>,
    timeout: Duration,
    after_poll: impl Fn(),
    external_fault: impl Fn() -> bool,
) -> Result<T, ReceiveError<T>> {
    #[cfg(target_family = "wasm")]
    use crate::clock::Instant;
    #[cfg(not(target_family = "wasm"))]
    use std::time::Instant;
    receive_using_external(rx, timeout, after_poll, external_fault, || {
        let started = Instant::now();
        move || started.elapsed()
    })
}

fn poll_message<T>(
    rx: &Receiver<T>,
    after_poll: &impl Fn(),
    external_fault: &impl Fn() -> bool,
) -> Result<Option<T>, ReceiveError<T>> {
    if rx.is_aborted() || external_fault() {
        return Err(ReceiveError::Aborted);
    }
    let result = rx.try_recv();
    after_poll();
    if rx.is_aborted() || external_fault() {
        return Err(aborted_after_poll(result));
    }
    match result {
        Ok(value) => Ok(Some(value)),
        Err(TryRecvError::Disconnected) => Err(ReceiveError::Disconnected),
        Err(TryRecvError::Empty) => Ok(None),
    }
}

#[cfg(test)]
fn receive_using<T, F: Fn() -> Duration>(
    rx: &Receiver<T>,
    timeout: Duration,
    after_poll: impl Fn(),
    make_clock: impl FnOnce() -> F,
) -> Result<T, ReceiveError<T>> {
    receive_using_external(rx, timeout, after_poll, || false, make_clock)
}

fn receive_using_external<T, F: Fn() -> Duration>(
    rx: &Receiver<T>,
    timeout: Duration,
    after_poll: impl Fn(),
    external_fault: impl Fn() -> bool,
    make_clock: impl FnOnce() -> F,
) -> Result<T, ReceiveError<T>> {
    if let Some(value) = poll_message(rx, &after_poll, &external_fault)? {
        return Ok(value);
    }
    let elapsed = make_clock();
    receive_with_external(
        rx,
        timeout,
        elapsed,
        |remaining| rx.signal.wait(remaining),
        after_poll,
        external_fault,
    )
}

#[cfg(test)]
fn receive_with<T>(
    rx: &Receiver<T>,
    timeout: Duration,
    elapsed: impl Fn() -> Duration,
    wait: impl Fn(Duration),
    after_poll: impl Fn(),
) -> Result<T, ReceiveError<T>> {
    receive_with_external(rx, timeout, elapsed, wait, after_poll, || false)
}

fn receive_with_external<T>(
    rx: &Receiver<T>,
    timeout: Duration,
    elapsed: impl Fn() -> Duration,
    wait: impl Fn(Duration),
    after_poll: impl Fn(),
    external_fault: impl Fn() -> bool,
) -> Result<T, ReceiveError<T>> {
    let mut previous = Duration::ZERO;
    let mut sample = || {
        let observed = elapsed();
        if observed < previous {
            return Err(ReceiveError::ClockRegressed { previous, observed });
        }
        previous = observed;
        Ok(observed)
    };
    loop {
        if let Some(value) = poll_message(rx, &after_poll, &external_fault)? {
            return Ok(value);
        }
        let spent = sample()?;
        if spent >= timeout {
            return Err(ReceiveError::Timeout { waited: spent });
        }
        wait(timeout - spent);
        if rx.is_aborted() || external_fault() {
            return Err(ReceiveError::Aborted);
        }
        let waited = sample()?;
        if waited >= timeout {
            return Err(ReceiveError::Timeout { waited });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn 已入队消息不等待() {
        let (tx, rx) = channel();
        tx.send(42).unwrap();
        let start = Instant::now();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(300)),
            Ok(42)
        );
        assert!(start.elapsed() < Duration::from_millis(150));
    }

    #[test]
    fn 消息到达唤醒等待者() {
        let (tx, rx) = channel();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            tx.send(17).unwrap();
        });
        let start = Instant::now();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(500)),
            Ok(17)
        );
        assert!(start.elapsed() < Duration::from_millis(250));
        sender.join().unwrap();
    }

    #[test]
    fn 最后发送者断连唤醒等待者() {
        let (tx, rx) = channel::<u8>();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(tx);
        });
        let start = Instant::now();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(500)),
            Err(ReceiveError::Disconnected)
        );
        assert!(start.elapsed() < Duration::from_millis(250));
        sender.join().unwrap();
    }
    #[test]
    fn 检查后发送仍保留通知() {
        let (tx, rx) = channel();
        assert_eq!(rx.inner.try_recv(), Err(TryRecvError::Empty));
        tx.send(23).unwrap();
        let start = Instant::now();
        rx.signal.wait(Duration::from_millis(300));
        assert!(start.elapsed() < Duration::from_millis(150));
        assert_eq!(rx.inner.try_recv(), Ok(23));
    }

    #[test]
    fn 多个发送者保持各自顺序且最后断连() {
        let (tx, rx) = channel();
        let tx2 = tx.clone();
        let worker = std::thread::spawn(move || {
            for n in 0..100 {
                tx2.send((1, n)).unwrap();
            }
        });
        for n in 0..100 {
            tx.send((0, n)).unwrap();
        }
        drop(tx);
        worker.join().unwrap();
        let mut next = [0, 0];
        for _ in 0..200 {
            let (producer, n) = rx.inner.try_recv().unwrap();
            assert_eq!(n, next[producer]);
            next[producer] += 1;
        }
        assert_eq!(rx.inner.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn 重复通知合并且不丢消息() {
        let (tx, rx) = channel();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(300)),
            Ok(1)
        );
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(300)),
            Ok(2)
        );
    }
    #[test]
    fn 旧通知不能提前结束超时等待() {
        let (tx, rx) = channel();
        tx.send(1).unwrap();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(100)),
            Ok(1)
        );
        let start = Instant::now();
        assert!(matches!(
            receive_interruptible(&rx, Duration::from_millis(80)),
            Err(ReceiveError::Timeout { .. })
        ));
        assert!(start.elapsed() >= Duration::from_millis(60));
    }

    #[test]
    fn 非最后发送者释放不能提前结束等待() {
        let (tx, rx) = channel::<u8>();
        let other = tx.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(other);
        });
        let start = Instant::now();
        assert!(matches!(
            receive_interruptible(&rx, Duration::from_millis(100)),
            Err(ReceiveError::Timeout { .. })
        ));
        assert!(start.elapsed() >= Duration::from_millis(75));
        sender.join().unwrap();
        drop(tx);
    }
    #[test]
    fn 等待到期后不消费边界消息() {
        use std::cell::Cell;
        let (tx, rx) = channel();
        let elapsed = Cell::new(Duration::ZERO);
        let result = receive_with(
            &rx,
            Duration::from_millis(10),
            || elapsed.get(),
            |_| {
                tx.send(99).unwrap();
                elapsed.set(Duration::from_millis(10));
            },
            || {},
        );
        assert_eq!(
            result,
            Err(ReceiveError::Timeout {
                waited: Duration::from_millis(10)
            })
        );
        assert_eq!(rx.inner.try_recv(), Ok(99));
    }
    #[test]
    fn 故障先于等待时立即返回且不消费消息() {
        let (tx, rx) = channel();
        tx.send(7).unwrap();
        rx.abort_handle().abort();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(300)),
            Err(ReceiveError::Aborted)
        );
        assert_eq!(rx.inner.try_recv(), Ok(7));
    }
    #[test]
    fn 故障唤醒阻塞接收者() {
        let (_tx, rx) = channel::<u8>();
        let abort = rx.abort_handle();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            abort.abort();
        });
        let start = Instant::now();
        assert_eq!(
            receive_interruptible(&rx, Duration::from_millis(400)),
            Err(ReceiveError::Aborted)
        );
        assert!(start.elapsed() < Duration::from_millis(200));
        worker.join().unwrap();
    }
    #[test]
    fn 伪唤醒后继续等待消息() {
        use std::cell::Cell;
        let (tx, rx) = channel();
        let elapsed = Cell::new(Duration::ZERO);
        let wakes = Cell::new(0);
        let result = receive_with(
            &rx,
            Duration::from_millis(10),
            || elapsed.get(),
            |_| {
                wakes.set(wakes.get() + 1);
                elapsed.set(Duration::from_millis(wakes.get()));
                if wakes.get() == 2 {
                    tx.send(31).unwrap();
                }
            },
            || {},
        );
        assert_eq!(result, Ok(31));
        assert_eq!(wakes.get(), 2);
    }
    #[test]
    fn 故障与已取出消息竞争时保留所有权() {
        let (tx, rx) = channel();
        tx.send(String::from("回执")).unwrap();
        let abort = rx.abort_handle();
        let result = receive_interruptible_inner(&rx, Duration::from_secs(1), || abort.abort());
        assert_eq!(
            result,
            Err(ReceiveError::AbortedWithMessage(String::from("回执")))
        );
    }
    #[test]
    fn 已进入等待时故障确实唤醒() {
        let (_tx, rx) = channel::<u8>();
        let abort = rx.abort_handle();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            done_tx
                .send(receive_interruptible(&rx, Duration::from_secs(2)))
                .unwrap();
        });
        let limit = Instant::now() + Duration::from_millis(500);
        while abort.signal.state.load(Ordering::Acquire) != PARKED {
            assert!(Instant::now() < limit, "等待者未进入 PARKED");
            std::thread::yield_now();
        }
        abort.abort();
        assert_eq!(
            done_rx.recv_timeout(Duration::from_millis(500)).unwrap(),
            Err(ReceiveError::Aborted)
        );
        worker.join().unwrap();
    }
    #[test]
    fn 检查完成后进入等待前发生故障不会漏唤醒() {
        use std::cell::Cell;
        let (_tx, rx) = channel::<u8>();
        let abort = rx.abort_handle();
        let waited = Cell::new(false);
        let result = receive_with(
            &rx,
            Duration::from_secs(1),
            || Duration::ZERO,
            |remaining| {
                abort.abort();
                rx.signal.wait(remaining);
                waited.set(true);
            },
            || {},
        );
        assert!(waited.get());
        assert_eq!(result, Err(ReceiveError::Aborted));
    }
    #[test]
    fn 非阻塞收件保留消息顺序与断连状态() {
        let (tx, rx) = channel();
        tx.send(11).unwrap();
        tx.send(12).unwrap();
        drop(tx);
        assert_eq!(rx.try_recv(), Ok(11));
        assert_eq!(rx.try_recv(), Ok(12));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }
    #[test]
    fn 非阻塞迭代仅取现有消息并保持顺序() {
        let (tx, rx) = channel();
        tx.send(3).unwrap();
        tx.send(4).unwrap();
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![3, 4]);
        assert!(rx.try_iter().next().is_none());
        tx.send(5).unwrap();
        drop(tx);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![5]);
    }
    #[test]
    fn 等待中单调时钟相邻回退显式失败() {
        use std::cell::Cell;
        let (_tx, rx) = channel::<u8>();
        let step = Cell::new(0);
        let elapsed = || {
            let n = step.get();
            step.set(n + 1);
            match n {
                0 => Duration::from_millis(5),
                1 => Duration::from_millis(3),
                _ => Duration::from_secs(1),
            }
        };
        let result = receive_with(&rx, Duration::from_secs(1), elapsed, |_| {}, || {});
        assert_eq!(
            result,
            Err(ReceiveError::ClockRegressed {
                previous: Duration::from_millis(5),
                observed: Duration::from_millis(3)
            })
        );
    }
    #[test]
    fn 已入队消息无需启动平台时钟() {
        use std::cell::Cell;
        let (tx, rx) = channel();
        tx.send(41).unwrap();
        let starts = Cell::new(0);
        let result = receive_using(
            &rx,
            Duration::from_secs(1),
            || {},
            || {
                starts.set(starts.get() + 1);
                || Duration::ZERO
            },
        );
        assert_eq!(result, Ok(41));
        assert_eq!(starts.get(), 0);
    }
    #[test]
    fn 全局故障先于槽通知时不得消费消息() {
        use std::cell::Cell;
        let (tx, rx) = channel();
        tx.send(42).unwrap();
        let failed = Cell::new(true);
        let result = receive_when_inner(&rx, Duration::from_secs(1), || {}, || failed.get());
        assert_eq!(result, Err(ReceiveError::Aborted));
        assert_eq!(rx.try_recv(), Ok(42));
    }
    #[test]
    fn 出队与返回之间的全局故障保留消息所有权() {
        use std::cell::Cell;
        let (tx, rx) = channel();
        tx.send(42).unwrap();
        let failed = Cell::new(false);
        let result = receive_when_inner(
            &rx,
            Duration::from_secs(1),
            || failed.set(true),
            || failed.get(),
        );
        assert_eq!(result, Err(ReceiveError::AbortedWithMessage(42)));
    }
}
