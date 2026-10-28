use super::*;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, PartialEq)]
enum GroupMessage { Shutdown }
#[derive(Debug, PartialEq)]
enum DeskMessage { Shutdown }

#[test]
fn permanent_stop_wakes_mutually_held_senders() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let (group_tx, group_rx) = channel();
    let (desk_tx, desk_rx) = channel();
    let (ready_tx, ready_rx) = channel();
    let (done_tx, done_rx) = channel();
    let group = std::thread::spawn({
        let retained_desk = desk_tx.clone();
        let ready = ready_tx.clone();
        let done = done_tx.clone();
        let flag = shutdown.clone();
        move || {
            ready.send(()).unwrap();
            assert_eq!(group_rx.recv().unwrap(), GroupMessage::Shutdown);
            assert!(flag.load(Ordering::SeqCst));
            drop(retained_desk);
            done.send(()).unwrap();
        }
    });
    let desk = std::thread::spawn({
        let retained_group = group_tx.clone();
        let flag = shutdown.clone();
        move || {
            ready_tx.send(()).unwrap();
            assert_eq!(desk_rx.recv().unwrap(), DeskMessage::Shutdown);
            assert!(flag.load(Ordering::SeqCst));
            drop(retained_group);
            done_tx.send(()).unwrap();
        }
    });
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    shutdown.store(true, Ordering::SeqCst);
    assert_eq!(done_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    let mut wake = Some(WakeSenders::new(vec![group_tx], vec![desk_tx],
        || GroupMessage::Shutdown, || DeskMessage::Shutdown));
    permanent_stop(&shutdown, &mut wake);
    assert_ne!(done_rx.recv_timeout(Duration::from_millis(100)), Err(RecvTimeoutError::Timeout),
        "永久标记不能唤醒互持Sender的裸recv；必须向全部worker发送Shutdown");
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    group.join().unwrap();
    desk.join().unwrap();
    assert!(wake.is_none());
}

#[test]
fn permanent_stop_takes_wake_once_and_dead_receiver_does_not_short_circuit() {
    let flag = AtomicBool::new(false);
    let (dead_tx, dead_rx) = channel::<GroupMessage>();
    drop(dead_rx);
    let (group_tx, group_rx) = channel();
    let (desk_tx, desk_rx) = channel();
    let mut wake = Some(WakeSenders::new(vec![dead_tx, group_tx], vec![desk_tx],
        || GroupMessage::Shutdown, || DeskMessage::Shutdown));
    permanent_stop(&flag, &mut wake);
    assert!(flag.load(Ordering::SeqCst));
    assert_eq!(group_rx.try_recv(), Ok(GroupMessage::Shutdown));
    assert_eq!(desk_rx.try_recv(), Ok(DeskMessage::Shutdown));
    assert!(wake.is_none());
    permanent_stop(&flag, &mut wake);
    assert_eq!(group_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected));
    assert_eq!(desk_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected));
}

struct Handle {
    thread: std::thread::JoinHandle<Result<(), String>>,
    joins: Arc<AtomicUsize>,
}
type Guard = ConstructionGuard<GroupMessage, DeskMessage, String, Handle>;

fn join(handle: Handle) -> Result<(), String> {
    let result = handle.thread.join();
    handle.joins.fetch_add(1, Ordering::SeqCst);
    match result {
        Ok(result) => result,
        Err(payload) => Err(payload.downcast_ref::<&str>().expect("测试panic为字符串").to_string()),
    }
}

fn fixture(groups: usize, desks: usize) -> (Guard, std::sync::mpsc::Receiver<()>, Arc<AtomicUsize>) {
    let flag = Arc::new(AtomicBool::new(false));
    let joins = Arc::new(AtomicUsize::new(0));
    let (ready_tx, ready_rx) = channel();
    let (done_tx, done_rx) = channel();
    let (group_txs, group_rxs): (Vec<_>, Vec<_>) = (0..groups).map(|_| channel()).unzip();
    let (desk_txs, desk_rxs): (Vec<_>, Vec<_>) = (0..desks).map(|_| channel()).unzip();
    let mut guard = Guard::new(flag.clone(), WakeSenders::new(group_txs.clone(), desk_txs.clone(),
        || GroupMessage::Shutdown, || DeskMessage::Shutdown), groups + desks, join);
    for (index, receiver) in group_rxs.into_iter().enumerate() {
        let retained = desk_txs.clone();
        let ready = ready_tx.clone();
        let done = done_tx.clone();
        let shutdown = flag.clone();
        let handle = std::thread::spawn(move || {
            ready.send(()).unwrap();
            assert_eq!(receiver.recv().unwrap(), GroupMessage::Shutdown);
            assert!(shutdown.load(Ordering::SeqCst));
            drop(retained);
            done.send(()).unwrap();
            Ok(())
        });
        guard.push(format!("group-{index}"), Handle { thread: handle, joins: joins.clone() });
    }
    for (index, receiver) in desk_rxs.into_iter().enumerate() {
        let retained = group_txs.clone();
        let ready = ready_tx.clone();
        let done = done_tx.clone();
        let shutdown = flag.clone();
        let handle = std::thread::spawn(move || {
            ready.send(()).unwrap();
            assert_eq!(receiver.recv().unwrap(), DeskMessage::Shutdown);
            assert!(shutdown.load(Ordering::SeqCst));
            drop(retained);
            done.send(()).unwrap();
            Ok(())
        });
        guard.push(format!("desk-{index}"), Handle { thread: handle, joins: joins.clone() });
    }
    for _ in 0..groups + desks { ready_rx.recv_timeout(Duration::from_secs(1)).unwrap(); }
    (guard, done_rx, joins)
}

fn check_failure(groups: usize, desks: usize, primary: &str) {
    let (mut guard, done, joins) = fixture(groups, desks);
    assert_eq!(guard.spawn_failure(primary.to_owned()), primary);
    assert_eq!(joins.load(Ordering::SeqCst), groups + desks, "普通Err返回前必须完成全部实际join");
    assert!(guard.shutdown.load(Ordering::SeqCst), "构造普通Err必须显式永久停止");
    assert!(guard.wake.is_none());
    assert!(guard.handles.is_empty());
    for _ in 0..groups + desks { done.recv_timeout(Duration::from_secs(1)).unwrap(); }
    assert!(guard.stop_and_join().is_empty());
}

#[test]
fn first_group_spawn_failure_stops_empty_guard() { check_failure(0, 0, "spawn rt-group-0 原错误"); }

#[test]
fn partial_group_spawn_failure_joins_successful_handles() { check_failure(2, 0, "spawn rt-group-2 原错误"); }

#[test]
fn partial_desk_spawn_failure_joins_groups_and_desks() { check_failure(2, 1, "spawn rt-desk-1 原错误"); }

#[test]
fn successful_transfer_keeps_workers_live_until_world_stop() {
    let (guard, done, joins) = fixture(1, 1);
    let flag = guard.shutdown.clone();
    let (mut wake, handles) = guard.transfer();
    assert!(!flag.load(Ordering::SeqCst));
    assert_eq!(joins.load(Ordering::SeqCst), 0);
    assert_eq!(handles.len(), 2);
    assert!(done.try_recv().is_err());
    permanent_stop(&flag, &mut wake);
    done.recv_timeout(Duration::from_secs(1)).unwrap();
    done.recv_timeout(Duration::from_secs(1)).unwrap();
    for (_, handle) in handles { join(handle).unwrap(); }
    assert_eq!(joins.load(Ordering::SeqCst), 2);
    assert!(wake.is_none());
}

#[test]
fn spawn_failure_keeps_primary_and_all_secondary_join_details() {
    let (mut guard, done, joins) = fixture(1, 1);
    guard.push("worker-error".into(), Handle {
        thread: std::thread::spawn(|| Err("原worker错误".into())), joins: joins.clone() });
    guard.push("join-error".into(), Handle {
        thread: std::thread::spawn(|| panic!("原join死亡详情")), joins: joins.clone() });
    guard.push("after-errors".into(), Handle {
        thread: std::thread::spawn(|| Ok(())), joins: joins.clone() });
    let error = guard.spawn_failure("spawn rt-desk-1 主错误".into());
    assert_eq!(joins.load(Ordering::SeqCst), 5, "次生join错误不得短路后续实际join");
    assert!(error.starts_with("spawn rt-desk-1 主错误"));
    assert!(error.contains("worker-error") && error.contains("原worker错误"));
    assert!(error.contains("join-error") && error.contains("原join死亡详情"));
    assert!(guard.handles.is_empty());
    done.recv_timeout(Duration::from_secs(1)).unwrap();
    done.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
fn untransferred_guard_drop_stops_and_joins_workers() {
    let (guard, done, joins) = fixture(1, 1);
    let flag = guard.shutdown.clone();
    drop(guard);
    assert_eq!(joins.load(Ordering::SeqCst), 2, "Drop返回前必须完成全部实际join");
    assert!(flag.load(Ordering::SeqCst));
    done.recv_timeout(Duration::from_secs(1)).unwrap();
    done.recv_timeout(Duration::from_secs(1)).unwrap();
}
