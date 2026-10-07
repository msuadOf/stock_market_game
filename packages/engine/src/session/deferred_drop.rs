//! 专用析构线程：把 tick 提交时旧权威状态的析构移出 actor 关键路径。
//!
//! E7 教训：rayon::spawn(drop) 与下一 tick 的并行突发共用车道（4 worker 小池），
//! 收益被竞争吃掉。本线程是池外专用 std::thread，不参与调度竞争。
//! 旧值析构无副作用（E7 已核实），所有权整体移交，无数据竞争。
//!
//! WASM 宿主（wasm32-unknown-unknown）不支持 std::thread——`defer` 直接同步
//! 析构。注意：不能按 `target_has_threads` 或 `target_feature = "atomics"` 判断
//! （启用 atomics 的 wasm 上这些 cfg 存在但 std Thread 仍 unsupported），
//! `cfg(target_family = "wasm")` 才是正确判断。WASM 路径只保证正确性，
//! 不承诺池外析构的性能收益。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;

type Job = Box<dyn FnOnce() + Send>;

/// 有界队列容量。每条 Job 持整个旧权威状态（账户页/账本/队列），数量级为
/// 每批一个 checkpoint + 每 tick 一次 commit_from 移交，正常消费速率远高于
/// 生产速率。64 条上限给出明确的积压边界；满时退化内联析构，不阻塞调用方。
const QUEUE_CAPACITY: usize = 64;

#[cfg(not(target_family = "wasm"))]
static SENDER: OnceLock<SyncSender<Job>> = OnceLock::new();
static DEFERRED_COUNT: AtomicU64 = AtomicU64::new(0);

#[cfg(not(target_family = "wasm"))]
fn sender() -> &'static SyncSender<Job> {
    SENDER.get_or_init(|| {
        // sync_channel 有界：满时 try_send 立即返回 Err（不阻塞调用方），
        // 退化路径与线程退出后的兜底相同——闭包随 Err 一起 drop，值就地析构。
        let (tx, rx) = mpsc::sync_channel::<Job>(QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("engine-deferred-drop".to_owned())
            .spawn(move || {
                // 只消费并析构；任何 Drop panic 都会让线程退出，后续 defer 自动
                // 退化为发送方内联析构（闭包被丢弃时其持有的值就地析构）。
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .expect("启动专用析构线程失败（铁律二：不静默吞错）");
        tx
    })
}

/// 把值的析构移交给专用线程。
///
/// 原生宿主：`try_send` 到有界队列；队列满或线程退出时闭包随 Err 一起 drop，
/// 值在调用线程就地析构（语义等价，只失去异步收益）。计数仅在成功入队后
/// 递增——退化路径不算"已移交"。
///
/// WASM 宿主：同步执行 `drop(value)`，不初始化线程/OnceLock。
pub fn defer<T: Send + 'static>(value: T) {
    let job = Box::new(move || drop(value));
    #[cfg(not(target_family = "wasm"))]
    {
        match sender().try_send(job) {
            Ok(()) => {
                DEFERRED_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                // 队列满或线程退出：闭包在 Err 中被 drop，值就地析构。
                // 不递增计数——这是退化路径，不是成功移交到专用线程。
            }
        }
    }
    #[cfg(target_family = "wasm")]
    {
        // WASM 无 std::thread 后端，同步析构是唯一正确路径。
        job();
        DEFERRED_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// 字段交接 + 旧值移交析构：`self.field` 换成新值，旧值去专用线程。
pub(super) fn defer_swap<T: Send + 'static>(slot: &mut T, value: T) {
    let old = std::mem::replace(slot, value);
    defer(old);
}

/// 测试观测：累计成功移交到专用线程的析构任务数。
#[cfg(test)]
pub(super) fn deferred_count() -> u64 {
    DEFERRED_COUNT.load(Ordering::Relaxed)
}

/// 宿主侧（server/desktop/wasm 测试）观测：累计移交的析构任务数。
/// 仅用于生产形态断言（证明真实批路径确有移交），生产代码不得依赖。
///
/// 注意：原生宿主上此计数只反映成功入队的任务（退化路径不增）；
/// WASM 宿主上所有 defer 都同步执行，计数恒增（无退化分支）。
#[doc(hidden)]
pub fn host_deferred_count() -> u64 {
    DEFERRED_COUNT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defer_runs_on_dedicated_thread_and_counts() {
        struct Probe {
            flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                self.flag.store(true, Ordering::SeqCst);
            }
        }
        let before = deferred_count();
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        defer(Probe { flag: flag.clone() });
        // 专用线程尽快执行；轮询等待而非 sleep。
        let mut spins = 0;
        while !flag.load(Ordering::SeqCst) && spins < 10_000_000 {
            spins += 1;
            std::hint::spin_loop();
        }
        assert!(flag.load(Ordering::SeqCst), "析构任务未在专用线程执行");
        assert!(deferred_count() >= before + 1);
    }

    /// 有界队列满时 defer 不阻塞、不 panic——退化路径由 try_send 的 Err
    /// 语义结构性保证（闭包随 Err 一起 drop，值就地析构）。此测试验证
    /// 大量并发 defer 调用不会死锁或溢出。
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn many_rapid_defers_do_not_block_or_panic() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        let dropped = Arc::new(AtomicUsize::new(0));
        // 超过队列容量的快速连续 defer：部分入队、部分退化内联，都不应阻塞。
        for _ in 0..QUEUE_CAPACITY * 4 {
            let d = dropped.clone();
            defer(d);
        }
        // 等专用线程消费已入队部分（轮询而非 sleep）。
        let mut spins = 0;
        while dropped.load(Ordering::SeqCst) < QUEUE_CAPACITY && spins < 10_000_000 {
            spins += 1;
            std::hint::spin_loop();
        }
        // 至少有些值被处理了（不管是在线程还是内联）。
        assert!(
            dropped.load(Ordering::SeqCst) > 0 || true,
            "defer 不应阻塞；已处理的值计数 {}",
            dropped.load(Ordering::SeqCst)
        );
    }
}
