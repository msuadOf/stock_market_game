//! Native 真实线程对照。两个测量臂的消息、原子顺序、拓扑和完成点相同。

use super::{black_box, measure_ns, mpsc, Duration, Receiver};
use libos::lifecycle::ShutdownWake;
use libos::thread_facade::WorkerHandle;
use libos::wait;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;

const GUARD: Duration = Duration::from_secs(2);
const STOP: u32 = 0;

fn raw_recv(rx: &Receiver<u32>, stop: &AtomicBool) -> Option<u32> {
    if stop.load(Ordering::SeqCst) {
        return None;
    }
    rx.recv().ok().filter(|_| !stop.load(Ordering::SeqCst))
}

fn libos_recv(rx: &Receiver<u32>, stop: &AtomicBool) -> Option<u32> {
    wait::worker_recv(rx, stop).expect("正常 Native worker 不应进入失败路径")
}

pub fn spawn_join(facade: bool, operations: u32) -> f64 {
    // 两臂均由 fresh OS thread 计算并完成 join；不把结果就绪当作线程退出。
    let direct = |n| {
        let name = "libos-bench-join".to_owned();
        thread::Builder::new()
            .name(name)
            .spawn(move || black_box(n))
            .expect("直连 spawn 未成功")
            .join()
            .expect("直连 join 未成功")
    };
    let wrapped = |n| {
        WorkerHandle::spawn("libos-bench-join".to_owned(), move || black_box(n))
            .expect("LibOS spawn 未成功")
            .join()
            .expect("LibOS join 未成功")
    };
    fn batch(n: u32, mut operation: impl FnMut(u32) -> u32) -> f64 {
        measure_ns(|| {
            for i in 0..n {
                assert_eq!(operation(black_box(i)), i);
            }
        })
    }
    if facade {
        batch(operations, wrapped)
    } else {
        batch(operations, direct)
    }
}

fn handoff_batch(
    operations: u32,
    recv: impl Fn(&Receiver<u32>, &AtomicBool) -> Option<u32> + Send + 'static,
) -> f64 {
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (out_tx, out_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let stop = AtomicBool::new(false);
        ready_tx.send(()).expect("收件 worker 未就绪");
        for _ in 0..operations {
            let item = recv(&rx, &stop).expect("无停止或断连");
            out_tx.send(item).expect("回执端仍存活");
        }
        assert_eq!(recv(&rx, &stop), None, "普通断连后应结束收件");
    });
    ready_rx.recv_timeout(GUARD).expect("收件 worker 启动超时");
    // 单个在途消息：测量双向交接的均摊时间，不冒充 futex 确认已 park 或单向唤醒。
    let elapsed = measure_ns(|| {
        for number in 1..=operations {
            tx.send(black_box(number)).expect("收件端仍存活");
            assert_eq!(out_rx.recv_timeout(GUARD).expect("未回收消息"), number);
        }
    });
    drop(tx);
    worker.join().expect("收件 worker 未完成");
    elapsed
}

pub fn message_handoff(facade: bool, operations: u32) -> f64 {
    if facade {
        handoff_batch(operations, libos_recv)
    } else {
        handoff_batch(operations, raw_recv)
    }
}

enum StopMechanism {
    Facade(ShutdownWake<u32, u32>),
    Direct(Vec<mpsc::Sender<u32>>, Vec<mpsc::Sender<u32>>),
}

pub fn shutdown_wake(facade: bool, operations: u32) -> f64 {
    let mut sum = 0.0;
    for _ in 0..operations {
        let (tx_a, rx_a) = mpsc::channel();
        let (tx_b, rx_b) = mpsc::channel();
        // 让两个通道都保持连接，单纯 drop 协调者发送端不能唤醒。
        let keepalive = (tx_a.clone(), tx_b.clone());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (mut a, mut b) = (vec![tx_a], vec![tx_b]);
        let mut mechanism = if facade {
            StopMechanism::Facade(ShutdownWake::for_workers(&a, &b, || STOP, || STOP))
        } else {
            StopMechanism::Direct(a.clone(), b.clone())
        };
        let workers = [rx_a, rx_b].map(|rx| {
            let (stop, ready, done) = (stop.clone(), ready_tx.clone(), done_tx.clone());
            thread::spawn(move || {
                assert!(!stop.load(Ordering::SeqCst));
                ready.send(()).expect("报告 worker 就绪");
                // 无论调度先进入 recv 或先看到 stop，都从同一生产 API 验证不可交付。
                let result = if facade {
                    libos_recv(&rx, &stop)
                } else {
                    raw_recv(&rx, &stop)
                };
                assert_eq!(result, None, "硬停止不得向业务交付唤醒值");
                // 保持接收端到所有者释放 keepalive，提高等价性而不假装测得已 park。
                done.send(rx).expect("所有者必须收到停止回执");
            })
        });
        for _ in 0..2 {
            ready_rx.recv_timeout(GUARD).expect("停止前 worker 未就绪");
        }
        // 注册与 spawn 在计时外；只测停止门面 + 实际双回执，join 另测。
        let mut acknowledged = Vec::with_capacity(2);
        sum += measure_ns(|| {
            match &mut mechanism {
                StopMechanism::Facade(wake) => wake.sever(&stop, &mut a, &mut b),
                StopMechanism::Direct(primary, secondary) => {
                    stop.store(true, Ordering::SeqCst);
                    for sender in std::mem::take(primary)
                        .into_iter()
                        .chain(std::mem::take(secondary))
                    {
                        sender.send(STOP).expect("保留收件端必须收到哨兵");
                    }
                    a.clear();
                    b.clear();
                }
            }
            for _ in 0..2 {
                acknowledged.push(
                    done_rx
                        .recv_timeout(GUARD)
                        .expect("未收到双 worker 停止回执"),
                );
            }
        });
        assert!(stop.load(Ordering::SeqCst) && a.is_empty() && b.is_empty());
        for rx in &acknowledged {
            // 若入口已观察 stop，哨兵还在；若先进入 recv，哨兵被消费但不交给业务。
            assert!(matches!(
                rx.try_recv(),
                Ok(STOP) | Err(mpsc::TryRecvError::Empty)
            ));
            assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty), "重复哨兵");
        }
        drop(keepalive);
        for worker in workers {
            worker.join().expect("停止 worker 未退出");
        }
    }
    sum
}
