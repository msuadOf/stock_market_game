//! W2 wasm 收件纯机制合同；独立运行不代表 engine 或浏览器整链通过。

#[path = "../src/runtime/wasm_wait.rs"]
mod wasm_wait;

use std::cell::{Cell, RefCell};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::time::Duration;
use wasm_wait::{WaitConfig, WaitFailure, WaitRuntime, recv_coordinator, recv_worker};

fn us(value: u64) -> Duration {
    Duration::from_micros(value)
}
fn config() -> WaitConfig {
    WaitConfig::new(us(2), us(4), us(1), us(3)).unwrap()
}

struct FakeRuntime<'a> {
    now: Cell<Duration>,
    sleeps: RefCell<Vec<Duration>>,
    spins: Cell<usize>,
    abort: Cell<bool>,
    after_sleep: RefCell<Option<Box<dyn FnMut() + 'a>>>,
    overshoot: Duration,
    abort_after_sleep: bool,
    rewind_after_sleep: Option<Duration>,
}

impl Default for FakeRuntime<'_> {
    fn default() -> Self {
        Self {
            now: Cell::new(Duration::ZERO),
            sleeps: RefCell::new(Vec::new()),
            spins: Cell::new(0),
            abort: Cell::new(false),
            after_sleep: RefCell::new(None),
            overshoot: Duration::ZERO,
            abort_after_sleep: false,
            rewind_after_sleep: None,
        }
    }
}
impl WaitRuntime for FakeRuntime<'_> {
    fn now(&self) -> Duration {
        self.now.get()
    }
    fn sleep(&self, duration: Duration) {
        self.sleeps.borrow_mut().push(duration);
        self.now.set(self.now.get() + duration + self.overshoot);
        if self.abort_after_sleep {
            self.abort.set(true);
        }
        if let Some(now) = self.rewind_after_sleep {
            self.now.set(now);
        }
        if let Some(action) = self.after_sleep.borrow_mut().as_mut() {
            action();
        }
    }
    fn spin(&self) {
        self.spins.set(self.spins.get() + 1);
        self.now.set(self.now.get() + us(1));
    }
    fn host_aborted(&self) -> bool {
        self.abort.get()
    }
}

#[test]
fn coordinator_preserves_fifo_before_disconnection() {
    let (tx, rx) = channel();
    tx.send(11).unwrap();
    tx.send(22).unwrap();
    drop(tx);
    let runtime = FakeRuntime::default();
    assert_eq!(
        recv_coordinator(&rx, us(5), &config(), false, &runtime),
        Ok(11)
    );
    assert_eq!(
        recv_coordinator(&rx, us(5), &config(), false, &runtime),
        Ok(22)
    );
    assert_eq!(
        recv_coordinator(&rx, us(5), &config(), false, &runtime),
        Err(WaitFailure::Disconnected)
    );
    assert!(runtime.sleeps.borrow().is_empty());
}

#[test]
fn coordinator_uses_coarse_slice_and_remaining_deadline() {
    let (_tx, rx) = channel::<()>();
    let runtime = FakeRuntime::default();
    assert_eq!(
        recv_coordinator(&rx, us(8), &config(), true, &runtime),
        Err(WaitFailure::Deadline { waited: us(8) })
    );
    assert_eq!(*runtime.sleeps.borrow(), [us(3), us(3), us(2)]);
}

#[test]
fn oversleep_reports_actual_wait_without_consuming_late_message() {
    let (tx, rx) = channel();
    let runtime = FakeRuntime {
        overshoot: us(4),
        after_sleep: RefCell::new(Some(Box::new(move || tx.send(44).unwrap()))),
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_coordinator(&rx, us(3), &config(), false, &runtime),
        Err(WaitFailure::Deadline { waited: us(5) })
    );
    assert_eq!(rx.try_recv().unwrap(), 44);
}

#[test]
fn worker_permanent_shutdown_does_not_process_buffered_message() {
    let (tx, rx) = channel();
    tx.send(9).unwrap();
    assert_eq!(
        recv_worker(
            &rx,
            &AtomicBool::new(true),
            &config(),
            &FakeRuntime::default()
        ),
        Ok(None)
    );
    assert_eq!(rx.try_recv().unwrap(), 9);
}

#[test]
fn worker_hot_spin_and_quiet_sleep_use_separate_budgets() {
    let (tx, rx) = channel();
    let runtime = FakeRuntime::default();
    *runtime.after_sleep.borrow_mut() = Some(Box::new(move || tx.send(6).unwrap()));
    assert_eq!(
        recv_worker(&rx, &AtomicBool::new(false), &config(), &runtime),
        Ok(Some(6))
    );
    assert_eq!(runtime.spins.get(), 2);
    assert_eq!(*runtime.sleeps.borrow(), [us(1)]);
}

#[test]
fn zero_poll_slices_are_rejected_explicitly() {
    assert!(WaitConfig::new(us(0), us(0), us(0), us(1)).is_err());
    assert!(WaitConfig::new(us(0), us(0), us(1), us(0)).is_err());
    assert!(WaitConfig::new(us(0), us(0), us(1), us(1)).is_ok());
}

#[test]
fn host_abort_has_priority_over_simultaneous_permanent_shutdown() {
    let (tx, rx) = channel();
    tx.send(9).unwrap();
    let runtime = FakeRuntime::default();
    runtime.abort.set(true);
    assert_eq!(
        recv_worker(&rx, &AtomicBool::new(true), &config(), &runtime),
        Err(WaitFailure::HostAborted)
    );
    assert_eq!(rx.try_recv().unwrap(), 9);
}

#[test]
fn coordinator_observes_abort_during_slice() {
    let (_tx, rx) = channel::<()>();
    let runtime = FakeRuntime {
        abort_after_sleep: true,
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_coordinator(&rx, us(9), &config(), false, &runtime),
        Err(WaitFailure::HostAborted)
    );
    assert_eq!(*runtime.sleeps.borrow(), [us(1)]);
}

#[test]
fn clock_regression_is_explicit() {
    let (_tx, rx) = channel::<()>();
    let runtime = FakeRuntime {
        rewind_after_sleep: Some(us(7)),
        ..FakeRuntime::default()
    };
    runtime.now.set(us(8));
    assert_eq!(
        recv_coordinator(&rx, us(9), &config(), false, &runtime),
        Err(WaitFailure::ClockRegressed {
            previous: us(8),
            observed: us(7)
        })
    );
}
