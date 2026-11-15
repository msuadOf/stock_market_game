//! Release verification example，不进入 Server 或 Browser 产品制品。
#[cfg(all(not(target_family = "wasm"), feature = "verification-harness"))]
use libos::deferred_drop;
use libos::{clock, wait};
use std::hint::black_box;
#[cfg(all(not(target_family = "wasm"), feature = "verification-harness"))]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const MAX_BATCH: u32 = 1 << 20;
const LIMIT: Duration = Duration::from_millis(50);
// 不向保持连接的空队列发布消息：单独测超时，不在性能测试里定义竞态优先级。
const IDLE_LIMIT: Duration = Duration::from_micros(100);
#[cfg(all(not(target_family = "wasm"), feature = "verification-harness"))]
// 队列不与其他生产者共享；另留一个槽给 FIFO 完成 marker。
const DROP_SEGMENT: u32 = (deferred_drop::QUEUE_CAPACITY - 1) as u32;

#[cfg(not(target_family = "wasm"))]
fn direct_clock() -> std::time::Instant {
    std::time::Instant::now()
}

#[cfg(not(target_family = "wasm"))]
fn direct_recv(rx: &Receiver<u32>) -> u32 {
    rx.recv_timeout(LIMIT).expect("直连热态收件必须成功")
}

#[cfg(not(target_family = "wasm"))]
fn direct_deadline(rx: &Receiver<u32>) -> Duration {
    assert_eq!(rx.recv_timeout(IDLE_LIMIT), Err(mpsc::RecvTimeoutError::Timeout));
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    IDLE_LIMIT
}

#[cfg(target_family = "wasm")]
mod wasm {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn engine_clock_now() -> f64;
        fn benchmark_relative_now() -> f64;
    }

    static DIRECT_ORIGIN: AtomicU64 = AtomicU64::new(0);

    // 与 production adapter 同形的直接实现；WASM 没有可用的 std::Instant。
    // 两臂各自锚定 epoch，并观测同一 Worker 的同一 host import。
    pub fn direct_clock() -> u64 {
        let raw = unsafe { engine_clock_now() };
        let raw_bits = raw.to_bits();
        let mut origin = DIRECT_ORIGIN.load(Ordering::Acquire);
        if origin == 0 {
            origin = match DIRECT_ORIGIN.compare_exchange(
                0,
                raw_bits,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => raw_bits,
                Err(actual) => actual,
            };
        }
        let delta_ms = raw - f64::from_bits(origin);
        assert!(delta_ms >= 0.0, "直连时钟倒退");
        let ns = delta_ms * 1_000_000.0;
        assert!(ns < (1_u64 << 63) as f64, "直连时间线纳秒溢出");
        (1_u64 << 63) + ns as u64
    }

    pub type Abort = &'static AtomicBool;

    pub fn host_abort_word() -> Abort {
        let layout = wait::wasm_wait_abort_layout().expect("先初始化唯一等待配置");
        assert_eq!(layout.bytes as usize, std::mem::size_of::<AtomicBool>());
        let address = layout.address as usize;
        assert_ne!(address, 0);
        assert_eq!(address % std::mem::align_of::<AtomicBool>(), 0);
        // 布局 API 公布的是本实例中 static GLOBAL_ABORT 的地址及精确宽度；
        // 此处只做原子读，既不制造另一份 abort，也不改变共享内存。
        unsafe { &*(address as *const AtomicBool) }
    }

    pub fn direct_recv(rx: &Receiver<u32>, abort: Abort) -> u32 {
        // 定义只覆盖已缓冲、无超时、无终止的一次收件。保留热路径两次读时钟和
        // 两次读同一个生产 abort 字；未实现 Empty/Disconnect 路径，不作冷路径对照。
        let started = clock::Instant::now();
        // 保留 elapsed 的取时和不变量检查；不额外 black_box 未使用的 Duration。
        let _elapsed = clock::Instant::now()
            .checked_duration_since(started)
            .expect("WASM wait 观测时钟早于启动时刻");
        assert!(!abort.load(Ordering::SeqCst), "直连热态收件之前已终止");
        let message = rx.try_recv().expect("直连热态队列已预填");
        assert!(!abort.load(Ordering::SeqCst), "直连热态收件之后已终止");
        message
    }

    pub fn direct_deadline(rx: &Receiver<u32>, abort: Abort) -> Duration {
        // 与生产 adapter 使用同一时钟、abort 字、try_recv 与 std sleep；
        // 与统一 deadline 合同一致，到期点重新观察数据/断连，不把入队定时加入热路径。
        // 本例等待预算 100 μs，低于两臂同样的 500 μs 轮询片。
        let anchor = clock::Instant::now();
        let read = || {
            clock::Instant::now()
                .checked_duration_since(anchor)
                .expect("直连等待时钟倒退")
        };
        let started = read();
        let mut previous = started;
        let mut elapsed = || {
            let observed = read();
            assert!(observed >= previous, "直连等待时钟样本倒退");
            previous = observed;
            observed - started
        };
        let budget = IDLE_LIMIT;
        let final_probe = || {
            assert!(!abort.load(Ordering::SeqCst));
            assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty));
            assert!(!abort.load(Ordering::SeqCst));
        };
        loop {
            assert!(!abort.load(Ordering::SeqCst));
            assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty));
            let waited = elapsed();
            if waited >= budget {
                final_probe();
                return waited;
            }
            std::thread::sleep(budget - waited);
            assert!(!abort.load(Ordering::SeqCst));
            let waited = elapsed();
            if waited >= budget {
                final_probe();
                return waited;
            }
        }
    }

    pub fn timer() -> f64 {
        unsafe { benchmark_relative_now() }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn libos_benchmark_init() {
        wait::init_wasm_wait(500, 50_000, 500, 500).expect("唯一 Worker 探针初始化等待");
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn libos_benchmark_probe_version() -> u32 {
        // v2 冷超时探针已对齐最终探测，且不再接受无 owner 的 inline 析构。
        2
    }

    #[cfg(feature = "verification-harness")]
    #[unsafe(no_mangle)]
    pub extern "C" fn libos_benchmark_contracts() {
        clock::verify_duration_contract();
        wait::verify_worker_delivery_contract();
        wait::verify_coordinator_delivery_contract();
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn libos_benchmark_batch(case: u32, facade: u32, operations: u32) -> f64 {
        assert!(matches!(case, 0 | 1 | 3) && facade <= 1 && (1..=MAX_BATCH).contains(&operations));
        match case {
            0 if facade == 0 => measure_clock(operations, direct_clock),
            0 => measure_clock(operations, clock::Instant::now),
            1 if facade == 0 => {
                let abort = host_abort_word();
                measure_queue(operations, |rx| direct_recv(rx, abort))
            }
            1 => measure_queue(operations, facade_recv),
            3 if facade == 0 => {
                let abort = host_abort_word();
                measure_deadline(operations, |rx| direct_deadline(rx, abort))
            }
            3 => measure_deadline(operations, facade_deadline),
            _ => unreachable!("操作码已校验"),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn measure_ns(work: impl FnOnce()) -> f64 {
    let start = std::time::Instant::now();
    work();
    start.elapsed().as_nanos() as f64
}

#[cfg(target_family = "wasm")]
fn measure_ns(work: impl FnOnce()) -> f64 {
    let start = wasm::timer();
    work();
    (wasm::timer() - start) * 1_000_000.0
}

fn measure_clock<T: Ord>(operations: u32, mut read: impl FnMut() -> T) -> f64 {
    let mut previous = read();
    measure_ns(|| {
        for _ in 0..operations {
            let next = black_box(read());
            assert!(next >= previous, "探针观察到时钟倒退");
            previous = next;
        }
        black_box(&previous);
    })
}

fn facade_recv(rx: &Receiver<u32>) -> u32 {
    wait::recv_deadline(rx, LIMIT, false).expect("门面热态收件必须成功")
}

fn facade_deadline(rx: &Receiver<u32>) -> Duration {
    match wait::recv_deadline(rx, IDLE_LIMIT, false) {
        Err(wait::WaitFailure::Deadline { waited }) => waited,
        other => panic!("静置空通道必须返回 Deadline：{other:?}"),
    }
}

fn measure_deadline(operations: u32, mut recv: impl FnMut(&Receiver<u32>) -> Duration) -> f64 {
    let (tx, rx) = mpsc::channel();
    let ns = measure_ns(|| {
        for _ in 0..operations {
            assert!(black_box(recv(&rx)) >= IDLE_LIMIT, "超时诊断早于预算");
        }
    });
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    drop(tx);
    ns
}

#[cfg(not(target_family = "wasm"))]
#[path = "libos_boundary_cost/native_lifecycle.rs"]
mod native_lifecycle;

fn measure_queue(operations: u32, mut recv: impl FnMut(&Receiver<u32>) -> u32) -> f64 {
    let (tx, rx) = mpsc::channel();
    for value in 1..=operations {
        tx.send(black_box(value)).expect("热态队列预填失败");
    }
    let mut actual = 0_u64;
    let elapsed = measure_ns(|| {
        for _ in 0..operations {
            actual += u64::from(black_box(recv(&rx)));
        }
    });
    let expected = u64::from(operations) * (u64::from(operations) + 1) / 2;
    assert_eq!(actual, expected, "两臂必须取得同样的全部消息");
    assert_eq!(
        rx.try_recv(),
        Err(mpsc::TryRecvError::Empty),
        "一臂留下未收的消息"
    );
    elapsed
}

#[cfg(all(not(target_family = "wasm"), feature = "verification-harness"))]
fn measure_queued_drop(operations: u32, facade: bool, completion: bool) -> f64 {
    use deferred_drop::{Job, Submission};
    use std::sync::Arc;

    struct Probe(Arc<AtomicU32>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let count = Arc::new(AtomicU32::new(0));
    let queue = deferred_drop::verification_sender().expect("必须预热真实 Native 后端");
    let mut elapsed = 0.0;
    assert!(
        DROP_SEGMENT < deferred_drop::QUEUE_CAPACITY as u32,
        "单段加完成 marker 不能超过真实队列容量"
    );
    for offset in (0..operations).step_by(DROP_SEGMENT as usize) {
        let len = DROP_SEGMENT.min(operations - offset);
        let probes: Vec<_> = (0..len).map(|_| Probe(Arc::clone(&count))).collect();
        let (tx, rx) = mpsc::sync_channel(1);
        let marker: Job = Box::new(move || tx.send(()).expect("必须交付 FIFO 完成标记"));
        let complete = || {
            queue.try_send(marker).expect("真实队列必须容纳完成标记");
            rx.recv_timeout(Duration::from_secs(2))
                .expect("未收到真实 FIFO 完成标记");
        };
        let submit = || {
            // 分开实例化两个 hot loop，避免把每次选臂开销混进 submit。
            if facade {
                for probe in probes {
                    assert_eq!(
                        deferred_drop::defer(black_box(probe)),
                        Submission::Queued,
                        "facade 本次不是后台排队"
                    );
                }
            } else {
                for probe in probes {
                    let probe = black_box(probe);
                    queue
                        .try_send(Box::new(move || drop(probe)))
                        .expect("直连本次不是后台排队");
                }
            }
        };
        if completion {
            elapsed += measure_ns(|| {
                submit();
                complete();
            });
        } else {
            elapsed += measure_ns(submit);
            complete();
        }
        assert_eq!(
            count.load(Ordering::Relaxed),
            offset + len,
            "FIFO 标记应证明这段所有权已交付且实际析构"
        );
    }
    elapsed
}
#[cfg(not(target_family = "wasm"))]
fn clock_resolution_ns() -> f64 {
    let start = std::time::Instant::now();
    let mut prev = 0;
    let mut min = u128::MAX;
    for _ in 0..100_000 {
        let now = start.elapsed().as_nanos();
        if now > prev {
            min = min.min(now - prev);
        }
        prev = now;
    }
    assert_ne!(min, u128::MAX, "无法观测 Native 计时分辨率");
    min as f64
}

#[cfg(not(target_family = "wasm"))]
fn run_case(
    name: &str,
    mut batch: impl FnMut(bool, u32) -> f64,
    resolution: f64,
    segment: Option<u32>,
) {
    let mut n = if matches!(name, "idle-deadline" | "spawn-join" | "message-handoff" | "shutdown-wake") {
        16
    } else {
        4096
    };
    // 2ms + 200 个计时量化间隔。两臂取同一 batch count；
    // 每侧各自预热并校准，正式样本不参与校准。
    loop {
        let a = batch(false, n);
        let b = batch(true, n);
        let segments = segment.map_or(1, |width| n.div_ceil(width));
        // 分段计时的量化裕量随独立计时窗数量累加。
        if a.min(b) >= 2_000_000.0_f64.max(200.0 * resolution * f64::from(segments)) {
            break;
        }
        assert!(n < MAX_BATCH, "{name} 无法在有界 batch 中超出计时分辨率");
        n *= 2;
    }
    let segments = segment.map_or(1, |width| n.div_ceil(width));
    for pair in 0..31 {
        let (order, direct_ns, facade_ns) = if pair % 2 == 0 {
            ("direct-facade", batch(false, n), batch(true, n))
        } else {
            let facade_ns = batch(true, n);
            ("facade-direct", batch(false, n), facade_ns)
        };
        println!(
            "{{\"case\":\"{name}\",\"order\":\"{order}\",\"operations\":{n},\"segments\":{segments},\"directNs\":{direct_ns},\"facadeNs\":{facade_ns},\"observedResolutionNs\":{resolution}}}"
        );
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    #[cfg(feature = "verification-harness")]
    {
        clock::verify_duration_contract();
        wait::verify_worker_delivery_contract();
        wait::verify_coordinator_delivery_contract();
        let observation = deferred_drop::verify_reclamation().expect("实线 Native 回收验证");
        assert_eq!(
            observation.submissions,
            [deferred_drop::Submission::Queued; 2]
        );
        assert_eq!(observation.completed_on_return, [0, 0]);
        assert_eq!(observation.completed_at_boundary, [1, 1]);
    }
    let resolution = clock_resolution_ns();
    run_case(
        "clock-now",
        |facade, n| {
            // Native 两个表达式连类型都相同：时钟门面是编译期同名 re-export。
            if facade {
                measure_clock(n, clock::Instant::now)
            } else {
                measure_clock(n, direct_clock)
            }
        },
        resolution,
        None,
    );
    run_case(
        "buffered-recv",
        |facade, n| {
            if facade {
                measure_queue(n, facade_recv)
            } else {
                measure_queue(n, direct_recv)
            }
        },
        resolution,
        None,
    );
    run_case(
        "idle-deadline",
        |facade, n| {
            if facade {
                measure_deadline(n, facade_deadline)
            } else {
                measure_deadline(n, direct_deadline)
            }
        },
        resolution,
        None,
    );
    run_case("spawn-join", native_lifecycle::spawn_join, resolution, None);
    run_case("message-handoff", native_lifecycle::message_handoff, resolution, None);
    run_case("shutdown-wake", native_lifecycle::shutdown_wake, resolution, Some(1));
    #[cfg(feature = "verification-harness")]
    {
        for (name, completion) in [
            ("queued-drop-return", false),
            ("queued-drop-complete", true),
        ] {
            run_case(
                name,
                |facade, n| measure_queued_drop(n, facade, completion),
                resolution,
                Some(DROP_SEGMENT),
            );
        }
    }
}

#[cfg(target_family = "wasm")]
fn main() {}
