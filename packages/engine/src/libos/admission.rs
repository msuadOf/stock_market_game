//! 发送准入与已关闭通道的有界收束。
use super::mailbox::Signal;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{marker::PhantomData, rc::Rc};

const CLOSED: usize = 1usize << (usize::BITS - 1);

/// 关闭前取得的许可可以完成；关闭后不得取得新许可。
pub(crate) struct Admission {
    state: AtomicUsize,
    waiter: AtomicBool,
    settled: Signal,
}

/// 只记录同一清理执行域内的相邻时钟样本。
///
/// WASM 的 `performance.now()` 跨 Worker 共享 time origin，但其精度可被降低，
/// 不同执行域的读数不能用于证明真实时钟回退。`Rc` 标记使 sampler 及其后续
/// `Closed` 不能在首次清理采样后再移交到另一个线程；receiver 本身仍可先移交给
/// 实际清理者，再创建 sampler。
pub(super) struct ClockSampler {
    last: Option<crate::clock::Instant>,
    _owner: PhantomData<Rc<()>>,
}

impl ClockSampler {
    fn new() -> Self { Self { last: None, _owner: PhantomData } }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AdmissionWaitError {
    NotClosed,
    AlreadyWaiting,
    TimedOut,
    ClockRegressed { previous: crate::clock::Instant, observed: crate::clock::Instant },
}

impl Admission {
    pub(crate) fn new() -> Self {
        Self { state: AtomicUsize::new(0), waiter: AtomicBool::new(false), settled: Signal::new() }
    }
    pub(crate) fn acquire(&self) -> Option<Permit<'_>> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & CLOSED != 0 { return None; }
            assert!(state < CLOSED - 1, "发送准入计数溢出");
            match self.state.compare_exchange_weak(state, state + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(Permit(self)),
                Err(current) => state = current,
            }
        }
    }
    pub(crate) fn close(&self) { self.state.fetch_or(CLOSED, Ordering::AcqRel); }
    pub(crate) fn is_closed_and_quiescent(&self) -> bool { self.state.load(Ordering::Acquire) == CLOSED }

    /// 使用调用方共享的绝对 deadline；同一时刻仅允许一个清理者调用。
    pub(crate) fn wait_closed(&self, deadline: crate::clock::Instant) -> Result<(), AdmissionWaitError> {
        self.wait_closed_with_clock(deadline, crate::clock::Instant::now)
    }

    pub(super) fn wait_closed_with_clock(&self, deadline: crate::clock::Instant, now: impl Fn() -> crate::clock::Instant) -> Result<(), AdmissionWaitError> {
        self.wait_closed_with_sampler(deadline, now).map(|_| ())
    }

    pub(super) fn wait_closed_with_sampler(&self, deadline: crate::clock::Instant, now: impl Fn() -> crate::clock::Instant) -> Result<ClockSampler, AdmissionWaitError> {
        if self.state.load(Ordering::Acquire) & CLOSED == 0 { return Err(AdmissionWaitError::NotClosed); }
        if self.waiter.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(AdmissionWaitError::AlreadyWaiting);
        }
        struct Waiter<'a>(&'a AtomicBool);
        impl Drop for Waiter<'_> { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }
        let _waiter = Waiter(&self.waiter);
        let mut sampler = ClockSampler::new();
        // deadline 仍按共享 timeOrigin 数值逐次精确比较；不增加精度容差，
        // 未到期才继续等待，到期或无法证明同域单调时均不能报告收束。
        let remaining = |sampler: &mut ClockSampler| {
            let observed = self.sample_clock(sampler, &now)?;
            let Some(duration) = deadline.checked_duration_since(observed) else { return Err(AdmissionWaitError::TimedOut); };
            if duration.is_zero() { return Err(AdmissionWaitError::TimedOut); }
            Ok(duration)
        };
        remaining(&mut sampler)?;
        if self.is_closed_and_quiescent() {
            remaining(&mut sampler)?;
            return Ok(sampler);
        }
        loop {
            let remaining = remaining(&mut sampler)?;
            if self.is_closed_and_quiescent() {
                remaining(&mut sampler)?;
                return Ok(sampler);
            }
            self.settled.wait(remaining);
        }
    }

    pub(super) fn sample_clock(&self, sampler: &mut ClockSampler, now: impl FnOnce() -> crate::clock::Instant) -> Result<crate::clock::Instant, AdmissionWaitError> {
        let observed = now();
        if let Some(previous) = sampler.last {
            if observed < previous { return Err(AdmissionWaitError::ClockRegressed { previous, observed }); }
        }
        sampler.last = Some(observed);
        Ok(observed)
    }
}

pub(crate) struct Permit<'a>(&'a Admission);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if self.0.state.fetch_sub(1, Ordering::Release) == (CLOSED | 1) {
            self.0.settled.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    use crate::clock::Instant;

    #[test]
    fn 准入等待时钟回退不能伪装成超时() {
        let gate = Admission::new();
        let permit = gate.acquire().unwrap();
        gate.close();
        let start = Instant::now() + Duration::from_millis(2);
        let deadline = start + Duration::from_secs(1);
        let calls = std::cell::Cell::new(0);
        let result = gate.wait_closed_with_clock(deadline, || {
            let n = calls.get();
            calls.set(n + 1);
            match n { 0 => start, 1 => start - Duration::from_millis(1), _ => deadline }
        });
        assert_eq!(result, Err(AdmissionWaitError::ClockRegressed {
            previous: start,
            observed: start - Duration::from_millis(1),
        }));
        drop(permit);
        assert_eq!(gate.wait_closed_with_clock(start + Duration::from_millis(500), || start), Ok(()));
    }

    #[test]
    fn 等待唤醒后时钟回退释放等待槽并允许重试() {
        let gate = Admission::new();
        let permit = gate.acquire().unwrap();
        gate.close();
        gate.settled.notify();
        let start = Instant::now() + Duration::from_millis(2);
        let calls = std::cell::Cell::new(0);
        let result = gate.wait_closed_with_clock(start + Duration::from_secs(1), || {
            let n = calls.get();
            calls.set(n + 1);
            if n < 2 { start } else { start - Duration::from_millis(1) }
        });
        assert!(matches!(result, Err(AdmissionWaitError::ClockRegressed { .. })));
        let checks = std::cell::Cell::new(0);
        assert_eq!(gate.wait_closed_with_clock(start + Duration::from_millis(10), || {
            let n = checks.get(); checks.set(n + 1);
            if n == 0 { start } else { start + Duration::from_millis(10) }
        }), Err(AdmissionWaitError::TimedOut));
        drop(permit);
        assert_eq!(gate.wait_closed_with_clock(start + Duration::from_millis(500), || start + Duration::from_millis(10)), Ok(()));
    }

    #[test]
    fn 新清理尝试从自身时钟样本建立基线() {
        let gate = Admission::new();
        gate.close();
        let start = Instant::now() + Duration::from_millis(2);
        assert_eq!(gate.wait_closed_with_clock(start, || start), Err(AdmissionWaitError::TimedOut));
        let earlier = start - Duration::from_millis(1);
        assert_eq!(gate.wait_closed_with_clock(start + Duration::from_secs(1), || earlier),
            Ok(()));
    }

    #[test]
    fn 实际进入等待后唤醒时钟回退可重试() {
        let gate = Arc::new(Admission::new());
        let permit = gate.acquire().unwrap();
        gate.close();
        let start = Instant::now() + Duration::from_millis(2);
        let worker_gate = gate.clone();
        let child = std::thread::spawn(move || {
            let calls = std::cell::Cell::new(0);
            worker_gate.wait_closed_with_clock(start + Duration::from_secs(1), || {
                let n = calls.get();
                calls.set(n + 1);
                if n < 2 { start } else { start - Duration::from_millis(1) }
            })
        });
        let deadline = Instant::now() + Duration::from_millis(500);
        while !gate.settled.is_parked() {
            assert!(Instant::now() < deadline, "清理者未进入等待");
            std::thread::yield_now();
        }
        gate.settled.notify();
        assert!(matches!(child.join().unwrap(), Err(AdmissionWaitError::ClockRegressed { .. })));
        let checks = std::cell::Cell::new(0);
        assert_eq!(gate.wait_closed_with_clock(start + Duration::from_millis(10), || {
            let n = checks.get(); checks.set(n + 1);
            if n == 0 { start } else { start + Duration::from_millis(10) }
        }), Err(AdmissionWaitError::TimedOut));
        drop(permit);
        assert_eq!(gate.wait_closed_with_clock(start + Duration::from_millis(500), || start + Duration::from_millis(10)), Ok(()));
    }


    #[test]
    fn 并发清理者不能共享单等待槽() {
        let gate = Arc::new(Admission::new());
        let permit = gate.acquire().unwrap();
        gate.close();
        let other = gate.clone();
        let child = std::thread::spawn(move || other.wait_closed(Instant::now() + Duration::from_millis(500)));
        let deadline = Instant::now() + Duration::from_millis(400);
        while !gate.waiter.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "清理者未登记等待");
            std::thread::yield_now();
        }
        assert_eq!(gate.wait_closed(deadline), Err(AdmissionWaitError::AlreadyWaiting));
        drop(permit);
        assert_eq!(child.join().unwrap(), Ok(()));
    }

    #[test]
    fn 第二清理者不能在首个采样期间干扰它() {
        let gate = Arc::new(Admission::new());
        gate.close();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let worker_gate = gate.clone();
        let child = std::thread::spawn(move || {
            worker_gate.wait_closed_with_clock(Instant::now() + Duration::from_secs(1), || {
                entered_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
                Instant::now()
            })
        });
        entered_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        let second = gate.wait_closed(Instant::now() + Duration::from_millis(500));
        resume_tx.send(()).unwrap();
        assert_eq!(child.join().unwrap(), Ok(()));
        assert_eq!(second, Err(AdmissionWaitError::AlreadyWaiting));
    }

    #[test]
    fn 遗失发送许可时只报告超时不伪造收束() {
        let gate = Admission::new();
        let permit = gate.acquire().unwrap();
        std::mem::forget(permit);
        gate.close();
        assert_eq!(gate.wait_closed(Instant::now() + Duration::from_millis(10)), Err(AdmissionWaitError::TimedOut));
        assert!(!gate.is_closed_and_quiescent());
    }

    #[test]
    fn 已收束但共同期限已过也必须报告超时() {
        let gate = Admission::new();
        gate.close();
        let observed = Instant::now() + Duration::from_millis(2);
        let expired = observed - Duration::from_millis(1);
        assert_eq!(gate.wait_closed_with_clock(expired, || observed), Err(AdmissionWaitError::TimedOut));
        assert!(gate.is_closed_and_quiescent());
    }

    #[test]
    fn 关闭与最后许可释放的竞争不会丢失通知() {
        for _ in 0..32 {
            let gate = Arc::new(Admission::new());
            let child_gate = gate.clone();
            let (started_tx, started_rx) = mpsc::channel();
            let (resume_tx, resume_rx) = mpsc::channel();
            let child = std::thread::spawn(move || {
                let permit = child_gate.acquire().unwrap();
                started_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_millis(500)).unwrap();
                drop(permit);
            });
            started_rx.recv_timeout(Duration::from_millis(500)).unwrap();
            gate.close();
            resume_tx.send(()).unwrap();
            assert_eq!(gate.wait_closed(Instant::now() + Duration::from_millis(500)), Ok(()));
            child.join().unwrap();
        }
    }
}
