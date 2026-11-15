//! 独立唤醒字。它不是数据通道，不携带业务顺序、确认或丢失含义。

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// 单个独立等待者的合并通知。生产者先发布外部状态，再调用 `notify`；
/// 等待者先捕获 `load` 快照，再复查外部状态，最后按快照进入等待。
/// 状态谓词始终归调用方；值按 i32 循环，只用于比较相等，不用于计算消息数量。
pub struct WakeEpoch {
    epoch: AtomicI32,
    closed: AtomicBool,
    #[cfg(all(not(target_family = "wasm"), feature = "native-async"))]
    waiter: tokio::sync::Notify,
}

impl WakeEpoch {
    pub fn new() -> Self {
        Self {
            epoch: AtomicI32::new(0),
            closed: AtomicBool::new(false),
            #[cfg(all(not(target_family = "wasm"), feature = "native-async"))]
            waiter: tokio::sync::Notify::new(),
        }
    }

    /// 读取与此前发布配对的唤醒快照。
    #[inline]
    pub fn load(&self) -> i32 {
        self.epoch.load(Ordering::Acquire)
    }

    /// 不等待消费者。等待者必须重新检查外部状态，通知可合并。
    pub fn notify(&self) {
        self.epoch.fetch_add(1, Ordering::Release);
        #[cfg(all(not(target_family = "wasm"), feature = "native-async"))]
        self.waiter.notify_one();
        #[cfg(target_arch = "wasm32")]
        // AtomicI32 对齐；调用期间对象仍由 &self 保持存活。
        unsafe {
            core::arch::wasm32::memory_atomic_notify(self.epoch.as_ptr(), 1);
        }
    }

    /// 发布永久关闭并唤醒。调用方负责使数据生产先停止，接收端仍可排空邮箱。
    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.notify();
        }
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Native 异步等待，不占用执行器线程。仅允许一个等待者；
    /// 调用方不得在 await 期间持有外部状态锁。
    #[cfg(all(not(target_family = "wasm"), feature = "native-async"))]
    pub async fn wait_for_change(&self, observed: i32) -> i32 {
        loop {
            let current = self.load();
            if observed != current || self.is_closed() {
                return current;
            }
            // 单等待者时 notify_one 在 await 登记前仍保留一个 permit。
            self.waiter.notified().await;
        }
    }

    /// 共享 Wasm 线性内存中 4 字节对齐的通知字节地址。
    /// 外部等待必须使用同一 Memory，且在对象移动或释放前全部结束。
    /// 不通过返回地址转移所有权；建议用 Arc 保存对象，在停止后先 join 消费者。
    pub fn shared_memory_offset(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        {
            Some(self.epoch.as_ptr() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }
}

impl Default for WakeEpoch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_advance_the_epoch_without_a_waiter() {
        let wake = WakeEpoch::new();
        let initial = wake.load();
        wake.notify();
        wake.notify();
        assert_eq!(wake.load(), initial.wrapping_add(2));
        assert!(!wake.is_closed());
    }

    #[test]
    fn close_publishes_a_single_terminal_notification() {
        let wake = WakeEpoch::new();
        let initial = wake.load();
        wake.close();
        assert!(wake.is_closed());
        assert_ne!(wake.load(), initial);
        let closed = wake.load();
        wake.close();
        assert_eq!(wake.load(), closed);
    }

    #[test]
    fn epoch_wrap_preserves_change_detection() {
        let wake = WakeEpoch::new();
        wake.epoch.store(i32::MAX, Ordering::Relaxed);
        wake.notify();
        assert_eq!(wake.load(), i32::MIN);
    }

    #[cfg(all(not(target_family = "wasm"), feature = "native-async"))]
    #[tokio::test(flavor = "current_thread")]
    async fn notify_between_snapshot_and_wait_is_not_lost() {
        let wake = WakeEpoch::new();
        let observed = wake.load();
        wake.notify();
        assert_eq!(
            wake.wait_for_change(observed).await,
            observed.wrapping_add(1)
        );
    }

    #[cfg(all(not(target_family = "wasm"), feature = "native-async"))]
    #[tokio::test(flavor = "current_thread")]
    async fn actual_waiter_is_woken_by_close() {
        use std::future::{poll_fn, Future};
        use std::task::Poll;

        let wake = WakeEpoch::new();
        let observed = wake.load();
        let waiting = wake.wait_for_change(observed);
        tokio::pin!(waiting);
        poll_fn(|context| {
            assert!(waiting.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        wake.close();
        assert_eq!(waiting.await, observed.wrapping_add(1));
        assert!(wake.is_closed());
    }
}
