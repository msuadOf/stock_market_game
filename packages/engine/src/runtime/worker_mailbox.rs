//! LibOS worker 收件验证：消息、永久停止与宿主故障使用同一可唤醒通道。

use crate::libos::mailbox::Receiver;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DrainState {
    /// 当前为空，但未来仍可能有发送者提交消息。
    SendersAlive,
    /// 所有 sender 已释放，队列中也没有剩余消息。
    Disconnected,
    /// 本次预算耗尽，调用方必须继续检查，不能当成清理完成。
    BudgetExhausted,
}

pub(super) struct Drained<T> {
    pub(super) messages: Vec<T>,
    pub(super) state: DrainState,
}

/// 普通返回和显式 Err 均把接收器的所有权带回 join 方；panic/trap 不保证执行。
pub(super) struct WorkerHandoff<R, E> {
    result: Result<(), E>,
    receiver: R,
}

impl<R, E> WorkerHandoff<R, E> {
    pub(super) fn into_parts(self) -> (Result<(), E>, R) {
        (self.result, self.receiver)
    }
}

pub(super) fn handoff_after_worker<R, E>(
    receiver: R,
    worker: impl FnOnce(&R) -> Result<(), E>,
) -> WorkerHandoff<R, E> {
    let result = worker(&receiver);
    WorkerHandoff { result, receiver }
}

/// 清理仅依赖非阻塞取件；std 和 LibOS 接收器使用相同静态分发协议。
pub(super) trait ShutdownInbox<T> {
    fn try_take(&self) -> Result<T, std::sync::mpsc::TryRecvError>;
}
impl<T> ShutdownInbox<T> for Receiver<T> {
    fn try_take(&self) -> Result<T, std::sync::mpsc::TryRecvError> { self.try_recv() }
}
impl<T> ShutdownInbox<T> for crate::libos::downlink::Receiver<T> {
    fn try_take(&self) -> Result<T, std::sync::mpsc::TryRecvError> {
        self.try_receive()
            .expect("World 下行 drain 时不能丢弃宿主故障")
    }
}
impl<T> ShutdownInbox<T> for std::sync::mpsc::Receiver<T> {
    fn try_take(&self) -> Result<T, std::sync::mpsc::TryRecvError> { self.try_recv() }
}

/// 转移当前消息所有权，不执行、不解释载荷。分批防止活跃发送者使调用无限循环。
pub(super) fn drain_for_shutdown<T>(rx: &impl ShutdownInbox<T>, budget: std::num::NonZeroUsize) -> Drained<T> {
    use std::sync::mpsc::TryRecvError;
    let mut messages = Vec::new();
    while messages.len() < budget.get() {
        match rx.try_take() {
            Ok(message) => messages.push(message),
            Err(TryRecvError::Empty) => return Drained { messages, state: DrainState::SendersAlive },
            Err(TryRecvError::Disconnected) => return Drained { messages, state: DrainState::Disconnected },
        }
    }
    Drained { messages, state: DrainState::BudgetExhausted }
}

#[cfg(test)]
use crate::libos::worker_wait::{receive as recv_worker, WorkerReceiveError};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn 通知等待的工作者互持发送端时永久停止仍可全部唤醒() {
        let (group_tx, group_rx) = crate::libos::mailbox::channel::<u8>();
        let (desk_tx, desk_rx) = crate::libos::mailbox::channel::<u8>();
        let group_wait = group_rx.abort_handle();
        let desk_wait = desk_rx.abort_handle();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::channel();
        let group = std::thread::spawn({
            let shutdown = shutdown.clone(); let done = done_tx.clone(); let held = desk_tx.clone();
            move || { let result = recv_worker(&group_rx, &shutdown, || false); drop(held); done.send(result).unwrap(); }
        });
        let desk = std::thread::spawn({
            let shutdown = shutdown.clone(); let held = group_tx.clone();
            move || { let result = recv_worker(&desk_rx, &shutdown, || false); drop(held); done_tx.send(result).unwrap(); }
        });
        等待已登记(&group_wait);
        等待已登记(&desk_wait);
        crate::runtime::worker_stop::wake_all(&shutdown, &[group_tx], &[desk_tx]);
        assert_eq!(done_rx.recv_timeout(Duration::from_millis(500)).unwrap(), Ok(None));
        assert_eq!(done_rx.recv_timeout(Duration::from_millis(500)).unwrap(), Ok(None));
        group.join().unwrap();
        desk.join().unwrap();
    }

    fn 等待已登记(handle: &crate::libos::mailbox::AbortHandle) {
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while !handle.waiting_registered() {
            assert!(std::time::Instant::now() < deadline, "worker 未进入等待");
            std::thread::yield_now();
        }
    }

    #[test]
    fn 消息直接唤醒并保持同一发送者顺序() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (tx, rx) = domain.channel().unwrap();
        let handle = rx.abort_handle();
        let stop = Arc::new(AtomicBool::new(false));
        let child_stop = stop.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let child = std::thread::spawn(move || {
            let first = recv_worker(&rx, &child_stop, || false).unwrap();
            let second = recv_worker(&rx, &child_stop, || false).unwrap();
            result_tx.send((first, second)).unwrap();
        });
        等待已登记(&handle);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        assert_eq!(result_rx.recv_timeout(Duration::from_millis(500)).unwrap(), (Some(1), Some(2)));
        child.join().unwrap();
    }

    #[test]
    fn 永久停止哨兵唤醒空闲工作者() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (tx, rx) = domain.channel().unwrap();
        let handle = rx.abort_handle();
        let stop = Arc::new(AtomicBool::new(false));
        let child_stop = stop.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let child = std::thread::spawn(move || result_tx.send(recv_worker(&rx, &child_stop, || false)).unwrap());
        等待已登记(&handle);
        stop.store(true, Ordering::SeqCst);
        tx.send(0).unwrap();
        assert_eq!(result_rx.recv_timeout(Duration::from_millis(500)).unwrap(), Ok(None));
        child.join().unwrap();
    }

    #[test]
    fn 故障广播唤醒空闲工作者且不伪装成正常停止() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (_tx, rx) = domain.channel::<u8>().unwrap();
        let handle = rx.abort_handle();
        let (result_tx, result_rx) = mpsc::channel();
        let child = std::thread::spawn(move || result_tx.send(recv_worker(&rx, &AtomicBool::new(false), || false)).unwrap());
        等待已登记(&handle);
        domain.abort();
        assert_eq!(result_rx.recv_timeout(Duration::from_millis(500)).unwrap(), Err(WorkerReceiveError::HostAborted));
        child.join().unwrap();
    }

    #[test]
    fn 出队后发生故障保留原始消息供清理() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (tx, rx) = domain.channel().unwrap();
        tx.send(String::from("实体载荷")).unwrap();
        let checks = std::cell::Cell::new(0);
        let result = recv_worker(&rx, &AtomicBool::new(false), || {
            checks.set(checks.get() + 1);
            checks.get() >= 3
        });
        assert_eq!(result, Err(WorkerReceiveError::HostAborted));
        assert_eq!(rx.try_recv().unwrap(), "实体载荷");
    }

    #[test]
    fn 出队后永久停止保留消息且断连干净退出() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (tx, rx) = domain.channel().unwrap();
        tx.send(7).unwrap();
        let stop = AtomicBool::new(false);
        let checks = std::cell::Cell::new(0);
        let result = recv_worker(&rx, &stop, || {
            checks.set(checks.get() + 1);
            if checks.get() == 3 { stop.store(true, Ordering::SeqCst); }
            false
        });
        assert_eq!(result, Ok(None));
        assert_eq!(rx.try_recv().unwrap(), 7);
        drop(tx);
        assert_eq!(recv_worker(&rx, &AtomicBool::new(false), || false), Ok(None));
    }

    #[test]
    fn 清理移交保留消息和后续队列并明确区分活跃发送者() {
        let domain = crate::libos::fault_domain::FaultDomain::new(1);
        let (tx, rx) = domain.channel().unwrap();
        tx.send(String::from("已出队载荷")).unwrap();
        tx.send(String::from("排队载荷")).unwrap();
        rx.retain_interrupted(rx.try_recv().unwrap());
        let first = drain_for_shutdown(&rx, std::num::NonZeroUsize::new(1).unwrap());
        assert_eq!(first.messages, ["已出队载荷"]);
        assert_eq!(first.state, DrainState::BudgetExhausted);
        let second = drain_for_shutdown(&rx, std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(second.messages, ["排队载荷"]);
        assert_eq!(second.state, DrainState::SendersAlive);
        tx.send(String::from("稍后送达的载荷")).unwrap();
        drop(tx);
        let last = drain_for_shutdown(&rx, std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(last.messages, ["稍后送达的载荷"]);
        assert_eq!(last.state, DrainState::Disconnected);
    }

    #[test]
    fn 真实交易部批次与下单资源载荷由清理方取得() {
        use crate::runtime::{DeskBatch, DeskIn, Entity};
        use crate::runtime::protocol::{DeskMsg, GroupIn, RoutedPrice};
        let domain = crate::libos::fault_domain::FaultDomain::new(2);
        let (group_tx, group_rx) = domain.channel().unwrap();
        let (desk_tx, desk_rx) = domain.channel().unwrap();
        let batch = DeskBatch { stock: 2, tick: 4, round: 1, entities: vec![Entity::CashRefund { local: 3, amount: 1234 }], ..DeskBatch::default() };
        group_tx.send(GroupIn::Batch(batch)).unwrap();
        desk_tx.send(DeskMsg::Ops { tick:4, round:1, group:0, final_flush:true, ops:vec![DeskIn::Place { owner:(0, 3), side:crate::Side::Buy, price:RoutedPrice::Fixed(100), qty:100, purse:10005, #[cfg(feature = "simulation-diagnostics")] source:crate::runtime::diagnostics::OperationSource::PlayerExternal }] }).unwrap();
        drop(group_tx);
        drop(desk_tx);
        let mut groups = drain_for_shutdown(&group_rx, std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(groups.state, DrainState::Disconnected);
        match groups.messages.pop().unwrap() {
            GroupIn::Batch(batch) => {
                assert_eq!((batch.stock, batch.tick, batch.round), (2, 4, 1));
                assert_eq!(batch.entities, [Entity::CashRefund { local:3, amount:1234 }]);
            }
            _ => panic!("清理方未取得原始实体批次"),
        }
        let mut desks = drain_for_shutdown(&desk_rx, std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(desks.state, DrainState::Disconnected);
        match desks.messages.pop().unwrap() {
            DeskMsg::Ops { tick, round, group, ops, final_flush } => {
                assert_eq!((tick, round, group, final_flush), (4, 1, 0, true));
                assert!(matches!(&ops[..], [DeskIn::Place {owner:(0, 3), side:crate::Side::Buy, price:RoutedPrice::Fixed(100), qty:100, purse:10005, ..}]));
            }
            _ => panic!("清理方未取得原始下单载荷"),
        }
    }

    #[test]
    fn 工作线程交还接收器后可先结束互持发送端再转移载荷() {
        let domain = crate::libos::fault_domain::FaultDomain::new(2);
        let (group_tx, group_rx) = domain.channel::<u32>().unwrap();
        let (desk_tx, desk_rx) = domain.channel::<u32>().unwrap();
        group_tx.send(9).unwrap();
        group_tx.send(10).unwrap();
        let group_to_desk = desk_tx.clone();
        let desk_to_group = group_tx.clone();
        let group = std::thread::spawn(move || handoff_after_worker(group_rx, |rx| {
            let interrupted = rx.try_recv().unwrap();
            assert_eq!(interrupted, 9);
            rx.retain_interrupted(interrupted);
            group_to_desk.send(21).unwrap();
            Err::<(), _>("组故障")
        }));
        let desk = std::thread::spawn(move || handoff_after_worker(desk_rx, |_rx| {
            desk_to_group.send(11).unwrap();
            Ok::<_, &str>(())
        }));
        drop(group_tx);
        drop(desk_tx);
        let (group_result, group_rx) = group.join().unwrap().into_parts();
        let (desk_result, desk_rx) = desk.join().unwrap().into_parts();
        assert_eq!(group_result, Err("组故障"));
        assert_eq!(desk_result, Ok(()));
        let group_left = drain_for_shutdown(&group_rx, std::num::NonZeroUsize::new(4).unwrap());
        let desk_left = drain_for_shutdown(&desk_rx, std::num::NonZeroUsize::new(4).unwrap());
        assert_eq!((group_left.state, group_left.messages), (DrainState::Disconnected, vec![9, 10, 11]));
        assert_eq!((desk_left.state, desk_left.messages), (DrainState::Disconnected, vec![21]));
    }
}
