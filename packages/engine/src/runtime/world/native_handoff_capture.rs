//! 仅 Native 测试：观察真实 World worker 返回时的 receiver 所有权。
use super::*;
use std::sync::Mutex;

pub(super) enum Recovered {
    Group(GroupIdx, Receiver<GroupIn>),
    Desk(StockIdx, Receiver<DeskMsg>),
}

pub(super) type Capture = Arc<Mutex<Vec<Recovered>>>;

pub(super) fn capture_exit(capture: &Capture, exit: super::OwnedWorkerExit) -> (WorkerId, Result<(), RuntimeError>) {
    match exit {
        super::OwnedWorkerExit::Group { who, exit } => {
            capture.lock().expect("组接收器观察槽锁损坏").push(Recovered::Group(match who { WorkerId::Group(group) => group, _ => unreachable!("组exit身份") }, exit.receiver));
            (who, super::exit_outcome(who, exit.outcome))
        }
        super::OwnedWorkerExit::Desk { who, exit } => {
            capture.lock().expect("交易部接收器观察槽锁损坏").push(Recovered::Desk(match who { WorkerId::Desk(stock) => stock, _ => unreachable!("交易部exit身份") }, exit.receiver));
            (who, super::exit_outcome(who, exit.outcome))
        }
    }
}
thread_local! { static NEXT: std::cell::RefCell<Option<Capture>> = const { std::cell::RefCell::new(None) }; }

pub(super) struct CaptureGuard {
    capture: Capture,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl std::ops::Deref for CaptureGuard {
    type Target = Mutex<Vec<Recovered>>;
    fn deref(&self) -> &Self::Target { &self.capture }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        NEXT.with(|slot| {
            let mut pending = slot.borrow_mut();
            if pending.as_ref().is_some_and(|capture| Arc::ptr_eq(capture, &self.capture)) {
                *pending = None;
            }
        });
    }
}

pub(super) fn capture_next_world() -> CaptureGuard {
    let capture = Arc::new(Mutex::new(Vec::new()));
    NEXT.with(|slot| {
        let mut pending = slot.borrow_mut();
        assert!(pending.is_none(), "上一个 World 接收器观察尚未消费");
        *pending = Some(capture.clone());
    });
    CaptureGuard { capture, _thread_bound: std::marker::PhantomData }
}

pub(super) fn take_next_world() -> Option<Capture> {
    NEXT.with(|slot| slot.borrow_mut().take())
}
