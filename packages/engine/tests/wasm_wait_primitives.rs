//! W2 wasm 收件纯机制合同；独立运行不代表 engine 或浏览器整链通过。

#[path = "../../libos/src/wait/wasm_mechanism.rs"]
mod wasm_wait;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use wasm_wait::{recv_coordinator, WaitConfig, WaitFailure, WaitRuntime};

fn recv_worker<T, R: WaitRuntime>(
    rx: &Receiver<T>,
    shutdown: &AtomicBool,
    config: &WaitConfig,
    runtime: &R,
) -> Result<Option<T>, WaitFailure> {
    wasm_wait::recv_worker(rx, shutdown, config, config.spin(), runtime)
}

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
    during_sleep: RefCell<Option<(Duration, Box<dyn FnMut(Duration) + 'a>)>>,
    clock_samples: RefCell<Option<VecDeque<Duration>>>,
    host_checks: Cell<usize>,
    on_host_check: RefCell<Option<Box<dyn FnMut(usize) -> bool + 'a>>>,
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
            during_sleep: RefCell::new(None),
            clock_samples: RefCell::new(None),
            host_checks: Cell::new(0),
            on_host_check: RefCell::new(None),
            overshoot: Duration::ZERO,
            abort_after_sleep: false,
            rewind_after_sleep: None,
        }
    }
}
impl WaitRuntime for FakeRuntime<'_> {
    fn now(&self) -> Duration {
        match self.clock_samples.borrow_mut().as_mut() {
            Some(samples) => samples.pop_front().expect("clock fixture 的样本不足"),
            None => self.now.get(),
        }
    }
    fn sleep(&self, duration: Duration) {
        self.sleeps.borrow_mut().push(duration);
        let started = self.now.get();
        if let Some((offset, mut action)) = self.during_sleep.borrow_mut().take() {
            assert!(offset < duration, "测试事件应明确定在唤醒前");
            self.now.set(started + offset);
            action(self.now.get());
        }
        self.now.set(started + duration + self.overshoot);
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
        let checks = self.host_checks.get() + 1;
        self.host_checks.set(checks);
        let injected = self
            .on_host_check
            .borrow_mut()
            .as_mut()
            .is_some_and(|action| action(checks));
        self.abort.get() || injected
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
fn oversleep_empty_channel_reports_actual_wait() {
    let (_tx, rx) = channel::<()>();
    let runtime = FakeRuntime {
        overshoot: us(4),
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_coordinator(&rx, us(3), &config(), false, &runtime),
        Err(WaitFailure::Deadline { waited: us(5) })
    );
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

#[test]
fn zero_deadline_polls_buffered_empty_and_disconnected_queue_without_sleep() {
    let (tx, rx) = channel();
    tx.send(7).unwrap();
    let runtime = FakeRuntime::default();
    assert_eq!(
        recv_coordinator(&rx, us(0), &config(), false, &runtime),
        Ok(7)
    );
    assert_eq!(
        recv_coordinator(&rx, us(0), &config(), false, &runtime),
        Err(WaitFailure::Deadline { waited: us(0) })
    );
    drop(tx);
    assert_eq!(
        recv_coordinator(&rx, us(0), &config(), false, &runtime),
        Err(WaitFailure::Disconnected)
    );
    assert!(runtime.sleeps.borrow().is_empty());
}

// 超时限制空队列的等待，不把超时判定点已可见的数据/断连抹成超时。
#[test]
fn coordinator_delivers_message_observable_at_deadline_decision() {
    for overshoot in [us(0), us(4)] {
        let (tx, rx) = channel();
        let runtime = FakeRuntime {
            overshoot,
            after_sleep: RefCell::new(Some(Box::new(move || tx.send(45).unwrap()))),
            ..FakeRuntime::default()
        };
        assert_eq!(recv_coordinator(&rx, us(1), &config(), false, &runtime), Ok(45));
        assert!(rx.try_recv().is_err(), "同一个值只能交付一次");
    }
}

#[test]
fn coordinator_delivers_when_receiver_resumes_after_predeadline_publication() {
    let (tx, rx) = channel();
    let runtime = FakeRuntime {
        during_sleep: RefCell::new(Some((us(3), Box::new(move |at| {
            assert_eq!(at, us(3));
            tx.send(48).unwrap();
        })))),
        overshoot: us(2),
        ..FakeRuntime::default()
    };
    let cfg = WaitConfig::new(us(0), us(0), us(5), us(5)).unwrap();
    assert_eq!(recv_coordinator(&rx, us(5), &cfg, false, &runtime), Ok(48));
    assert_eq!(runtime.now.get(), us(7));
    assert_eq!(*runtime.sleeps.borrow(), [us(5)]);
}

#[test]
fn coordinator_reports_disconnect_observable_at_deadline_decision() {
    let (tx, rx) = channel::<()>();
    let mut sender = Some(tx);
    let runtime = FakeRuntime {
        after_sleep: RefCell::new(Some(Box::new(move || drop(sender.take())))),
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_coordinator(&rx, us(1), &config(), false, &runtime),
        Err(WaitFailure::Disconnected)
    );
}

#[test]
fn coordinator_timeout_does_not_consume_a_message_published_after_decision() {
    let (tx, rx) = channel();
    let runtime = FakeRuntime::default();
    assert_eq!(
        recv_coordinator(&rx, us(1), &config(), false, &runtime),
        Err(WaitFailure::Deadline { waited: us(1) })
    );
    tx.send(46).unwrap();
    assert_eq!(recv_coordinator(&rx, us(0), &config(), false, &runtime), Ok(46));
}

#[test]
fn coordinator_host_abort_still_wins_when_message_is_observable_at_deadline() {
    let (tx, rx) = channel();
    let runtime = FakeRuntime {
        abort_after_sleep: true,
        after_sleep: RefCell::new(Some(Box::new(move || tx.send(47).unwrap()))),
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_coordinator(&rx, us(1), &config(), false, &runtime),
        Err(WaitFailure::HostAborted)
    );
    assert_eq!(rx.try_recv(), Ok(47));
}

fn regressing_runtime(samples: &[u64]) -> FakeRuntime<'static> {
    FakeRuntime {
        clock_samples: RefCell::new(Some(samples.iter().copied().map(us).collect())),
        ..FakeRuntime::default()
    }
}

fn regression_above_start() -> WaitFailure {
    WaitFailure::ClockRegressed {
        previous: us(12),
        observed: us(10),
    }
}

#[test]
fn coordinator_rejects_clock_regression_above_start() {
    let (_tx, rx) = channel::<()>();
    let runtime = regressing_runtime(&[8, 12, 10]);
    assert_eq!(
        recv_coordinator(&rx, us(20), &config(), false, &runtime),
        Err(regression_above_start())
    );
}

#[test]
fn worker_rejects_clock_regression_during_spin() {
    let (_tx, rx) = channel::<()>();
    let runtime = regressing_runtime(&[8, 12, 10]);
    let cfg = WaitConfig::new(us(20), us(4), us(1), us(3)).unwrap();
    assert_eq!(
        recv_worker(&rx, &AtomicBool::new(false), &cfg, &runtime),
        Err(regression_above_start())
    );
    assert_eq!(runtime.spins.get(), 1);
    assert!(runtime.sleeps.borrow().is_empty());
}

#[test]
fn worker_rejects_clock_regression_during_quiet_sleep() {
    let (_tx, rx) = channel::<()>();
    let runtime = regressing_runtime(&[8, 8, 12, 10]);
    let cfg = WaitConfig::new(us(0), us(20), us(1), us(3)).unwrap();
    assert_eq!(
        recv_worker(&rx, &AtomicBool::new(false), &cfg, &runtime),
        Err(regression_above_start())
    );
    assert_eq!(runtime.spins.get(), 0);
    assert_eq!(*runtime.sleeps.borrow(), [us(1)]);
}

#[test]
fn coordinator_rechecks_host_abort_before_returning_buffered_message() {
    let (tx, rx) = channel();
    tx.send(9).unwrap();
    let runtime = FakeRuntime {
        on_host_check: RefCell::new(Some(Box::new(|checks| checks == 2))),
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_coordinator(&rx, us(5), &config(), false, &runtime),
        Err(WaitFailure::HostAborted)
    );
}

#[test]
fn worker_rechecks_host_abort_before_returning_buffered_message() {
    let (tx, rx) = channel();
    tx.send(9).unwrap();
    let runtime = FakeRuntime {
        on_host_check: RefCell::new(Some(Box::new(|checks| checks == 2))),
        ..FakeRuntime::default()
    };
    assert_eq!(
        recv_worker(&rx, &AtomicBool::new(false), &config(), &runtime),
        Err(WaitFailure::HostAborted)
    );
}

#[test]
fn worker_rechecks_shutdown_before_returning_buffered_message() {
    let (tx, rx) = channel();
    tx.send(9).unwrap();
    let shutdown = AtomicBool::new(false);
    let runtime = FakeRuntime {
        on_host_check: RefCell::new(Some(Box::new(|checks| {
            if checks == 2 {
                shutdown.store(true, Ordering::SeqCst);
            }
            false
        }))),
        ..FakeRuntime::default()
    };
    assert_eq!(recv_worker(&rx, &shutdown, &config(), &runtime), Ok(None));
    assert!(shutdown.load(Ordering::SeqCst));
}
