//! 验证 Worker 退出值保留类型、身份与接收器所有权。
use super::*;
use crate::libos::worker_exit::Outcome;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

#[test]
fn 组退出值保留身份原错误与未消费消息() {
    let (sender, receiver) = crate::libos::downlink::channel();
    sender.send(GroupIn::Shutdown).unwrap();
    let handle = crate::libos::thread::Builder::new().spawn(move || {
        OwnedWorkerExit::group(2, receiver, |_| Err(RuntimeError::ProtocolInvariant { detail:"组原错误".into() }))
    }).unwrap();
    let exit = handle.join().unwrap();
    let OwnedWorkerExit::Group { who, exit } = exit else { panic!("必须保留组类型") };
    assert_eq!(who, WorkerId::Group(2));
    assert!(matches!(exit.outcome, Outcome::Returned(Err(RuntimeError::ProtocolInvariant { detail })) if detail == "组原错误"));
    assert!(sender.send(GroupIn::Shutdown).is_err());
    let closed = exit.receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
    let remaining = closed.drain(NonZeroUsize::new(2).unwrap());
    assert!(matches!(&remaining.messages[..], [GroupIn::Shutdown]));
}

#[test]
fn 交易部退出值保留身份与未消费消息() {
    let (sender, receiver) = crate::libos::downlink::channel();
    sender.send(DeskMsg::Shutdown).unwrap();
    let handle = crate::libos::thread::Builder::new().spawn(move || {
        OwnedWorkerExit::desk(3, receiver, |_| Ok(()))
    }).unwrap();
    let exit = handle.join().unwrap();
    let OwnedWorkerExit::Desk { who, exit } = exit else { panic!("必须保留交易部类型") };
    assert_eq!(who, WorkerId::Desk(3));
    assert!(matches!(exit.outcome, Outcome::Returned(Ok(()))));
    assert!(sender.send(DeskMsg::Shutdown).is_err());
    let closed = exit.receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
    let remaining = closed.drain(NonZeroUsize::new(2).unwrap());
    assert!(matches!(&remaining.messages[..], [DeskMsg::Shutdown]));
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn typed_组退出后等待已持有许可的发送并保留消息身份与原错误() {
    struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                let _ = release.send(());
            }
        }
    }

    let (sender, receiver) = crate::libos::downlink::channel();
    let send = sender.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let mut release_on_drop = ReleaseOnDrop(Some(resume_tx));
    let sending = crate::libos::thread::Builder::new().spawn(move || {
        send.send_with_test_hook(GroupIn::Shutdown, || {
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
        })
    }).unwrap();
    entered_rx.recv_timeout(Duration::from_millis(500)).unwrap();

    let handle = crate::libos::thread::Builder::new().spawn(move || {
        OwnedWorkerExit::group(7, receiver, |_| Err(RuntimeError::ProtocolInvariant { detail: "组原错误".into() }))
    }).unwrap();
    let OwnedWorkerExit::Group { who, exit } = handle.join().unwrap() else { panic!("必须保留组类型") };
    assert_eq!(who, WorkerId::Group(7));
    assert!(matches!(exit.outcome, Outcome::Returned(Err(RuntimeError::ProtocolInvariant { detail })) if detail == "组原错误"));

    let deadline = crate::clock::Instant::now() + Duration::from_millis(10);
    let before_send_finishes = exit.receiver.close_and_wait(deadline);
    release_on_drop.0.take().unwrap().send(()).unwrap();
    sending.join().unwrap().unwrap();
    assert!(matches!(before_send_finishes, Err(crate::libos::admission::AdmissionWaitError::TimedOut)), "持有 Permit 的发送尚未完成时必须因等待超时而不能判定准入收束");

    let closed = exit.receiver.close_and_wait(crate::clock::Instant::now() + Duration::from_millis(500)).unwrap();
    let remaining = closed.drain(NonZeroUsize::new(2).unwrap());
    assert_eq!(remaining.state, crate::libos::downlink::DrainState::Drained);
    assert!(matches!(&remaining.messages[..], [GroupIn::Shutdown]));
}

#[test]
fn 组_panic_后保留身份原始载荷与未消费消息() {
    let (sender, receiver) = crate::libos::downlink::channel();
    sender.send(GroupIn::Shutdown).unwrap();
    let handle = crate::libos::thread::Builder::new().spawn(move || {
        OwnedWorkerExit::group(4, receiver, |_| std::panic::panic_any(String::from("原始组 panic")))
    }).unwrap();
    let OwnedWorkerExit::Group { who, exit } = handle.join().unwrap() else { panic!("必须保留组类型") };
    assert_eq!(who, WorkerId::Group(4));
    match exit.outcome {
        Outcome::Panicked(payload) => assert_eq!(*payload.downcast::<String>().unwrap(), "原始组 panic"),
        _ => panic!("必须保留原始 panic"),
    }
    assert!(sender.send(GroupIn::Shutdown).is_err());
    let closed = exit.receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
    assert!(matches!(&closed.drain(NonZeroUsize::new(2).unwrap()).messages[..], [GroupIn::Shutdown]));
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 真实组循环通过退出值交还接收器() {
    use crate::runtime::group::{GroupAccount, GroupSetup, GroupState, TickPlan};
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let (sender, receiver) = crate::libos::downlink::channel();
    let (desk_tx, _desk_rx) = std::sync::mpsc::channel();
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    let state = GroupState::new(GroupSetup {
        group: 0, accounts: vec![GroupAccount::new(10_000_000, 1)], chunk: 8, lot: 100,
        config: Arc::new(crate::config::GameConfig::proposed_defaults()), ticks_per_day: 240,
        decision: None, plans: Vec::new(), script: Box::new(|_, _| TickPlan::default()),
    });
    sender.send(GroupIn::Shutdown).unwrap();
    sender.send(GroupIn::EchoTickForTest).unwrap();
    let handle = crate::libos::thread::Builder::new().spawn(move || {
        OwnedWorkerExit::group(0, receiver, |rx| crate::runtime::group::group_main_borrowed(
            state, vec![desk_tx], rx, crate::runtime::worker_context::WorkerContext {
                up_tx, abort: Arc::new(AtomicBool::new(false)), shutdown: Arc::new(AtomicBool::new(false)),
                capture: Arc::new(Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
            },
        ))
    }).unwrap();
    let OwnedWorkerExit::Group { who, exit } = handle.join().unwrap() else { panic!("必须保留组类型") };
    assert_eq!(who, WorkerId::Group(0));
    assert!(matches!(exit.outcome, Outcome::Returned(Ok(()))));
    assert!(matches!(up_rx.try_recv().unwrap(), UpMsg::GroupFinal(_)));
    let closed = exit.receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
    assert!(matches!(&closed.drain(NonZeroUsize::new(2).unwrap()).messages[..], [GroupIn::EchoTickForTest]));
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 真实交易部循环通过退出值交还接收器() {
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let (sender, receiver) = crate::libos::downlink::channel();
    let (group_tx, _group_rx) = std::sync::mpsc::channel();
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    let setup = DeskSetup {
        stock: 0, exchange: StockExchange::Shanghai, initial_price: 1000, limit_bps: 1000,
        max_order_qty_limit: 1_000_000, max_order_qty_market: 150_000, ticks_per_day: 240,
        auction_ticks: 0, closing_auction_ticks: 0, order_ttl_ticks: 10, history_len: 8,
        initial_tick: 0, initial_orders: Vec::new(), initial_recent_prices: Vec::new(),
        initial_daily_bars: Vec::new(), initial_traded_days: 0, initial_market: None,
        initial_public_cutoff: 0, initial_next_serial: 1,
    };
    sender.send(DeskMsg::Shutdown).unwrap();
    sender.send(DeskMsg::EchoCandles).unwrap();
    let handle = crate::libos::thread::Builder::new().spawn(move || {
        OwnedWorkerExit::desk(0, receiver, |rx| crate::runtime::desk::desk_main_borrowed(
            setup, 1, Arc::new(crate::config::GameConfig::proposed_defaults()), rx, vec![group_tx],
            crate::runtime::worker_context::WorkerContext {
                up_tx, abort: Arc::new(AtomicBool::new(false)), shutdown: Arc::new(AtomicBool::new(false)),
                capture: Arc::new(Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
            },
        ))
    }).unwrap();
    let OwnedWorkerExit::Desk { who, exit } = handle.join().unwrap() else { panic!("必须保留交易部类型") };
    assert_eq!(who, WorkerId::Desk(0));
    assert!(matches!(exit.outcome, Outcome::Returned(Ok(()))));
    assert!(matches!(up_rx.try_recv().unwrap(), UpMsg::DeskFinal(_)));
    let closed = exit.receiver.close_and_wait(Instant::now() + Duration::from_millis(500)).unwrap();
    assert!(matches!(&closed.drain(NonZeroUsize::new(2).unwrap()).messages[..], [DeskMsg::EchoCandles]));
}
