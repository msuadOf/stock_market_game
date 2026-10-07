//! 专用析构线程：把 tick 提交时旧权威状态的析构移出 actor 关键路径。
//!
//! E7 教训：rayon::spawn(drop) 与下一 tick 的并行突发共用车道（4 worker 小池），
//! 收益被竞争吃掉。本线程是池外专用 std::thread，独占一个核，不参与调度竞争。
//! 旧值析构无副作用（E7 已核实），所有权整体移交，无数据竞争。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

type Job = Box<dyn FnOnce() + Send>;

static SENDER: OnceLock<Sender<Job>> = OnceLock::new();
static DEFERRED_COUNT: AtomicU64 = AtomicU64::new(0);

fn sender() -> &'static Sender<Job> {
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
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

/// 把值的析构移交给专用线程。发送失败（析构线程已退出）时闭包就地丢弃，
/// 值在调用线程析构——语义等价，只是失去异步收益。
pub fn defer<T: Send + 'static>(value: T) {
    DEFERRED_COUNT.fetch_add(1, Ordering::Relaxed);
    let _ = sender().send(Box::new(move || drop(value)));
}

/// 字段交接 + 旧值移交析构：`self.field` 换成新值，旧值去专用线程。
pub(super) fn defer_swap<T: Send + 'static>(slot: &mut T, value: T) {
    let old = std::mem::replace(slot, value);
    defer(old);
}

/// 测试观测：累计移交的析构任务数。
#[cfg(test)]
pub(super) fn deferred_count() -> u64 {
    DEFERRED_COUNT.load(Ordering::Relaxed)
}

/// 宿主侧（server/desktop/wasm 测试）观测：累计移交的析构任务数。
/// 仅用于生产形态断言（证明真实批路径确有移交），生产代码不得依赖。
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
}
