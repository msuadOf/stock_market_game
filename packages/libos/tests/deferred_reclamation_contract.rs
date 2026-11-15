//! Native 真实有界队列的合同；不改全进程后端，不伪造析构完成。
#![cfg(all(feature = "verification-harness", not(target_family = "wasm")))]

use libos::deferred_drop::{DeferredDropDegradation, Job, Submission, TestBackend, QUEUE_CAPACITY};
use std::sync::{
    mpsc::{channel, sync_channel},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

const BUDGET: Duration = Duration::from_secs(1);

type Drops = Arc<Mutex<Vec<(usize, thread::ThreadId)>>>;

struct Probe {
    id: usize,
    drops: Drops,
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.drops
            .lock()
            .unwrap()
            .push((self.id, thread::current().id()));
    }
}

fn job(id: usize, drops: &Drops) -> Job {
    let probe = Probe {
        id,
        drops: Arc::clone(drops),
    };
    Box::new(move || drop(probe))
}

#[test]
fn 满队列与随后断开在同一后端上必须是两种结果() {
    let (tx, rx) = sync_channel(QUEUE_CAPACITY);
    let backend = TestBackend::running(tx);
    let (ready_tx, ready_rx) = channel();
    let (resume_tx, resume_rx) = channel();
    let (drained_tx, drained_rx) = channel();
    let worker = thread::spawn(move || {
        ready_tx.send(thread::current().id()).unwrap();
        // 明确冻结消费者；不靠 sleep 或执行速度判断队列是否填满。
        resume_rx.recv_timeout(BUDGET).expect("等待测试释放栏杆");
        for _ in 0..QUEUE_CAPACITY {
            rx.recv_timeout(BUDGET).expect("已接纳的工作不能丢失")();
        }
        drop(rx);
        drained_tx.send(()).unwrap();
    });

    let producer_id = thread::current().id();
    let consumer_id = ready_rx.recv_timeout(BUDGET).unwrap();
    assert_ne!(producer_id, consumer_id);
    let drops = Arc::new(Mutex::new(Vec::new()));
    for id in 0..QUEUE_CAPACITY {
        assert_eq!(backend.dispatch(job(id, &drops)), Submission::Queued);
    }
    assert!(
        drops.lock().unwrap().is_empty(),
        "消费者被冻结时不能释放排队值"
    );

    let overflow = QUEUE_CAPACITY;
    assert_eq!(
        backend.dispatch(job(overflow, &drops)),
        Submission::FullInline
    );
    assert_eq!(*drops.lock().unwrap(), [(overflow, producer_id)]);
    assert_eq!(backend.degradation(), None, "仅饱和不能污染后端健康状态");

    resume_tx.send(()).unwrap();
    drained_rx
        .recv_timeout(BUDGET)
        .expect("确认队列排空且 Receiver 真实断开");
    worker.join().expect("消费者正常退出");

    let after_loss = overflow + 1;
    assert_eq!(
        backend.dispatch(job(after_loss, &drops)),
        Submission::DisconnectedInline,
    );
    assert_eq!(
        backend.degradation(),
        Some(DeferredDropDegradation::UnexpectedDisconnected)
    );
    assert_eq!(
        backend.dispatch(job(after_loss + 1, &drops)),
        Submission::DisconnectedInline,
        "再次提交必须保留真实断开诊断，不得冒充普通饱和或自动重启",
    );
    assert_eq!(
        backend.degradation(),
        Some(DeferredDropDegradation::UnexpectedDisconnected)
    );
    let expected: Vec<_> = std::iter::once((overflow, producer_id))
        .chain((0..QUEUE_CAPACITY).map(|id| (id, consumer_id)))
        .chain([(after_loss, producer_id), (after_loss + 1, producer_id)])
        .collect();
    assert_eq!(
        *drops.lock().unwrap(),
        expected,
        "已接纳工作 FIFO；所有值恰好释放一次"
    );
}

#[test]
fn 启动失败与运行时断开不得共用诊断或重新提交值() {
    let (tx, rx) = sync_channel(QUEUE_CAPACITY);
    let errno = 11;
    let backend = TestBackend::from_spawn_failure(std::io::Error::from_raw_os_error(errno), tx);
    assert!(matches!(
        rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    let drops = Arc::new(Mutex::new(Vec::new()));
    assert_eq!(
        backend.degradation(),
        Some(DeferredDropDegradation::SpawnFailed {
            os_errno: Some(errno)
        })
    );
    for id in 0..2 {
        assert_eq!(
            backend.dispatch(job(id, &drops)),
            Submission::SpawnFailedInline
        );
        assert_eq!(
            backend.degradation(),
            Some(DeferredDropDegradation::SpawnFailed {
                os_errno: Some(errno)
            })
        );
    }
    let caller = thread::current().id();
    assert_eq!(*drops.lock().unwrap(), [(0, caller), (1, caller)]);
}

#[test]
fn 真实消费者panic不能把连接断开冒充正常完成() {
    use std::sync::atomic::{AtomicBool, Ordering};

    const SENTINEL: &str = "LIBOS_NATIVE_RECLAIMER_PANIC_PROBE";
    let (tx, rx) = sync_channel::<Job>(QUEUE_CAPACITY);
    let backend = TestBackend::running(tx);
    let (armed_tx, armed_rx) = channel();
    let (release_tx, release_rx) = channel();
    let worker = thread::spawn(move || {
        let first = rx.recv_timeout(BUDGET).expect("必须读取已排队的故障工作");
        first();
        panic!("故障工作没有触发预期 panic");
    });
    let panic_job = Box::new(move || {
        armed_tx.send(thread::current().id()).unwrap();
        release_rx.recv_timeout(BUDGET).expect("等待测试允许故障");
        std::panic::panic_any(SENTINEL);
    });
    assert_eq!(backend.dispatch(panic_job), Submission::Queued);
    let consumer = armed_rx.recv_timeout(BUDGET).unwrap();

    let drops = Arc::new(Mutex::new(Vec::new()));
    let queued_body_ran = Arc::new(AtomicBool::new(false));
    let ran = Arc::clone(&queued_body_ran);
    let release_queued_value = job(0, &drops);
    assert_eq!(
        backend.dispatch(Box::new(move || {
            ran.store(true, Ordering::SeqCst);
            release_queued_value();
        })),
        Submission::Queued
    );
    assert!(drops.lock().unwrap().is_empty());
    release_tx.send(()).unwrap();

    let failure = worker.join().expect_err("消费者 panic 不能报告成功 join");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&SENTINEL));
    assert!(
        !queued_body_ran.load(Ordering::SeqCst),
        "队列清理不是继续执行任务"
    );
    assert_eq!(
        *drops.lock().unwrap(),
        [(0, consumer)],
        "关闭接收端必须释放仍排队的值"
    );

    let caller = thread::current().id();
    assert_eq!(
        backend.dispatch(job(1, &drops)),
        Submission::DisconnectedInline
    );
    assert_eq!(
        backend.degradation(),
        Some(DeferredDropDegradation::UnexpectedDisconnected)
    );
    assert_eq!(*drops.lock().unwrap(), [(0, consumer), (1, caller)]);
}
