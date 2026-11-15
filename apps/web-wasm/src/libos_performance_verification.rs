//! 仅验证制品：两臂运行于同一真实 WASM Worker 底座，产品不能访问探针。
use libos::clock::Instant;
use libos::deferred_drop::{self, Submission};
use libos::lifecycle::ShutdownWake;
use libos::thread_facade::{Builder, WorkerHandle};
use libos::wait;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

const STOP: u32 = 0;
const JOIN_NAME: &str = "libos-perf-join";
const HANDOFF_NAME: &str = "libos-perf-handoff";
const STOP_NAME: &str = "libos-perf-stop";
const DROP_NAME: &str = "engine-deferred-drop";

fn clock_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).expect("测量窗超出纳秒范围")
}

#[wasm_bindgen]
pub fn verification_libos_boundary_timer_resolution() -> f64 {
    let mut previous = Instant::now();
    let mut minimum = u64::MAX;
    for _ in 0..100_000 {
        let cursor = Instant::now();
        let delta = cursor.checked_duration_since(previous).expect("实测 Worker 时钟倒退");
        if !delta.is_zero() {
            minimum = minimum.min(u64::try_from(delta.as_nanos()).expect("定时器一次跳变溢出"));
        }
        previous = cursor;
    }
    assert_ne!(minimum, u64::MAX, "缺少实测 Worker 定时器分辨率");
    minimum as f64
}

fn spawn_join(wrapped: bool) -> u64 {
    let task = || black_box(0x594_u32);
    // Builder + raw JoinHandle 与 WorkerHandle 共用真实 slot、ACK 与 join 实现。
    let start = Instant::now();
    let value = if wrapped {
        WorkerHandle::spawn(JOIN_NAME.to_owned(), task)
            .expect("门面未启动真实 worker")
            .join()
            .expect("门面未取得 worker 结果")
    } else {
        Builder::new().name(JOIN_NAME.to_owned())
            .spawn(task).expect("直连未启动真实 worker")
            .join().expect("直连未取得 worker 结果")
    };
    let elapsed = clock_ns(start);
    assert_eq!(value, 0x594);
    elapsed
}

fn host_stop_word() -> &'static AtomicBool {
    let layout = wait::wasm_wait_abort_layout().expect("同一宿主 abort 字必须就绪");
    assert_eq!(layout.bytes, 1);
    // 布局公布真实共享 static 的 AtomicBool 地址；只读取，不写入或复位。
    unsafe { &*(layout.address as *const AtomicBool) }
}

fn direct_recv(rx: &Receiver<u32>, stop: &AtomicBool, abort: &AtomicBool) -> Option<u32> {
    // 直连与门面均要求宿主正常、同一永久标志、FIFO 和断连。
    // baseline 只覆盖 spin=quiet=0 的正常宿主；它仍先 try_recv 一次，再进入裸 recv。
    let start = Instant::now();
    let observed = Instant::now();
    assert!(observed.checked_duration_since(start).is_some());
    assert!(!abort.load(Ordering::SeqCst));
    if stop.load(Ordering::SeqCst) { return None; }
    let message = match rx.try_recv() {
        Ok(item) => Some(item),
        Err(mpsc::TryRecvError::Empty) => {
            let spin_end = Instant::now();
            assert!(spin_end.checked_duration_since(observed).is_some());
            assert!(!abort.load(Ordering::SeqCst));
            if stop.load(Ordering::SeqCst) { return None; }
            let quiet_end = Instant::now();
            assert!(quiet_end.checked_duration_since(spin_end).is_some());
            assert!(!abort.load(Ordering::SeqCst));
            if stop.load(Ordering::SeqCst) { return None; }
            rx.recv().ok()
        }
        Err(mpsc::TryRecvError::Disconnected) => return None,
    };
    assert!(!abort.load(Ordering::SeqCst));
    message.filter(|_| !stop.load(Ordering::SeqCst))
}

fn facade_recv(rx: &Receiver<u32>, stop: &AtomicBool) -> Option<u32> {
    wait::worker_recv(rx, stop).expect("正常宿主不应进入异常失败路径")
}

fn handoff(wrapped: bool, operations: u32) -> u64 {
    let abort = host_stop_word();
    let receive = move |rx: &Receiver<u32>, stop: &AtomicBool| {
        if wrapped { facade_recv(rx, stop) } else { direct_recv(rx, stop, abort) }
    };
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (out_tx, out_rx) = mpsc::channel();
    let handle = WorkerHandle::spawn(HANDOFF_NAME.to_owned(), move || {
        let stop = AtomicBool::new(false);
        ready_tx.send(()).expect("报告接收线程就绪");
        for _ in 0..operations {
            out_tx.send(receive(&rx, &stop).expect("消息必须完整交付")).expect("交付回执");
        }
        assert_eq!(receive(&rx, &stop), None, "普通断连后必须退出");
    }).expect("必须启动真实 Worker");
    ready_rx.recv().expect("接收线程未就绪");
    let start = Instant::now();
    for item in 1..=operations {
        tx.send(black_box(item)).expect("真实 Worker 仍存活");
        assert_eq!(out_rx.recv().expect("缺少真实 Worker 回执"), item);
    }
    let elapsed = clock_ns(start);
    drop(tx);
    handle.join().expect("必须消费结果");
    elapsed
}

enum StopDelivery {
    Facade(ShutdownWake<u32, u32>),
    Direct(Vec<mpsc::Sender<u32>>, Vec<mpsc::Sender<u32>>),
}

fn shutdown_wake(wrapped: bool) -> u64 {
    let abort = host_stop_word();
    let receive = move |rx: &Receiver<u32>, stop: &AtomicBool| {
        if wrapped { facade_recv(rx, stop) } else { direct_recv(rx, stop, abort) }
    };
    let (tx_a, rx_a) = mpsc::channel();
    let (tx_b, rx_b) = mpsc::channel();
    let keepalive = (tx_a.clone(), tx_b.clone());
    let (mut a, mut b) = (vec![tx_a], vec![tx_b]);
    let mut delivery = if wrapped {
        StopDelivery::Facade(ShutdownWake::for_workers(&a, &b, || STOP, || STOP))
    } else {
        StopDelivery::Direct(a.clone(), b.clone())
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let handles = [rx_a, rx_b].map(|rx| {
        let (ready, done, stop) = (ready_tx.clone(), done_tx.clone(), stop.clone());
        WorkerHandle::spawn(STOP_NAME.to_owned(), move || {
            ready.send(()).expect("报告 worker 就绪");
            assert_eq!(receive(&rx, &stop), None, "哨兵不能作为业务值交付");
            done.send(rx).expect("报告硬停止完成");
        }).expect("必须启动真实 Worker")
    });
    for _ in 0..2 { ready_rx.recv().expect("未取得双 worker 就绪"); }
    let start = Instant::now();
    match &mut delivery {
        StopDelivery::Facade(wake) => wake.sever(&stop, &mut a, &mut b),
        StopDelivery::Direct(first, second) => {
            stop.store(true, Ordering::SeqCst);
            for tx in std::mem::take(first).into_iter().chain(std::mem::take(second)) {
                tx.send(STOP).expect("哨兵必须发送给保持连接的通道");
            }
            a.clear();
            b.clear();
        }
    }
    let stopped = [0, 1].map(|_| done_rx.recv().expect("缺少停止回执"));
    let elapsed = clock_ns(start);
    assert!(stop.load(Ordering::SeqCst) && a.is_empty() && b.is_empty());
    for rx in stopped {
        assert!(matches!(rx.try_recv(), Ok(STOP) | Err(mpsc::TryRecvError::Empty)));
        assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty), "重复哨兵");
    }
    drop(keepalive);
    for handle in handles { handle.join().expect("必须消费停止结果"); }
    elapsed
}

struct Probe(Arc<AtomicU32>, u32);
impl Drop for Probe {
    fn drop(&mut self) {
        let expected = self.1;
        assert_eq!(self.0.fetch_add(1, Ordering::SeqCst), expected, "析构必须保持 FIFO 且恰好一次");
    }
}

fn deferred(wrapped: bool, operations: u32) -> Result<(u64, u64), JsValue> {
    type Job = Box<dyn FnOnce() + Send>;
    // 只比较同样的 Box / 有界 try_send / 独立 Worker 与正常排队路径，不测满队列分支。
    let direct = if wrapped { None } else {
        let (tx, rx) = mpsc::sync_channel::<Job>(64);
        let handle = Builder::new().name(DROP_NAME.to_owned()).spawn(move || {
            while let Ok(job) = rx.recv() { job(); }
        }).expect("直连必须创建真实后台 Worker");
        Some((tx, handle))
    };
    let count = Arc::new(AtomicU32::new(0));
    let (ack_tx, ack_rx) = mpsc::channel();
    let probes: Vec<_> = (0..operations).map(|i| Probe(Arc::clone(&count), i)).collect();
    let start = Instant::now();
    for probe in probes {
        let probe = black_box(probe);
        if let Some((tx, _handle)) = &direct {
            tx.try_send(Box::new(move || drop(probe)))
                .unwrap_or_else(|_| panic!("直连后端必须排队"));
        } else if deferred_drop::defer(probe) != Submission::Queued {
            return Err(JsValue::from_str("未提供同语义 WASM 后台排队；禁止同步析构对照"));
        }
    }
    let submitted = clock_ns(start);
    // 独立标记排在动作后：不能用最后一个被测 Drop 内发出的回执证明同一 Drop 已返回。
    struct Completion(mpsc::Sender<()>);
    impl Drop for Completion {
        fn drop(&mut self) { self.0.send(()).expect("报告独立 FIFO 完成标记"); }
    }
    let marker = Completion(ack_tx);
    if let Some((tx, _handle)) = &direct {
        tx.try_send(Box::new(move || drop(marker)))
            .unwrap_or_else(|_| panic!("为真实 FIFO 完成标记保留一槽"));
    } else if deferred_drop::defer(marker) != Submission::Queued {
        return Err(JsValue::from_str("独立 FIFO 完成标记也必须排队"));
    }
    ack_rx.recv().expect("缺少真实 FIFO 完成标记");
    let completed = clock_ns(start);
    assert_eq!(count.load(Ordering::SeqCst), operations);
    if let Some((tx, handle)) = direct {
        drop(tx);
        handle.join().expect("必须消费直连后台 Worker 结果");
    }
    Ok((submitted, completed))
}

#[wasm_bindgen]
pub fn verification_libos_boundary_cost(case_name: &str, wrapped: bool, operations: u32) -> Result<String, JsValue> {
    let (elapsed_ns, completed_ns) = match case_name {
        "spawn-join" if operations == 1 => (spawn_join(wrapped), None),
        "message-handoff" if (1..=2048).contains(&operations) => (handoff(wrapped, operations), None),
        "shutdown-wake" if operations == 1 => (shutdown_wake(wrapped), None),
        "deferred" if (1..=63).contains(&operations) => {
            let (submitted, completed) = deferred(wrapped, operations)?;
            (submitted, Some(completed))
        }
        _ => return Err(JsValue::from_str("未知或非法的性能夹具操作")),
    };
    Ok(serde_json::json!({ "elapsedNs": elapsed_ns, "completedNs": completed_ns }).to_string())
}
