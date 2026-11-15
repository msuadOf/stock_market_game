//! 永久停止的执行层协议。调用方决定何时永久停止并提供唤醒消息；
//! LibOS 不了解任何业务阶段或可重试 Abort。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

struct WakeSenders<A, B> {
    first: Vec<Sender<A>>,
    second: Vec<Sender<B>>,
    wake_first: fn() -> A,
    wake_second: fn() -> B,
}

/// 裸 recv 在两端均需独立唤醒发送端，构造 guard 也可在 unwind 中通知 worker。
pub struct ShutdownWake<A, B> {
    retained: Option<WakeSenders<A, B>>,
}

impl<A, B> Default for ShutdownWake<A, B> {
    fn default() -> Self {
        Self { retained: None }
    }
}

impl<A, B> ShutdownWake<A, B> {
    pub fn for_workers(
        first: &[Sender<A>],
        second: &[Sender<B>],
        wake_first: fn() -> A,
        wake_second: fn() -> B,
    ) -> Self {
        Self {
            retained: Some(WakeSenders {
                first: first.to_vec(),
                second: second.to_vec(),
                wake_first,
                wake_second,
            }),
        }
    }

    /// 在 Native 测试中按裸 recv 所需协议检验真正的通道和唤醒。
    #[doc(hidden)]
    #[cfg(feature = "verification-harness")]
    pub fn with_explicit_wake_for_verification(
        first: Vec<Sender<A>>,
        second: Vec<Sender<B>>,
        wake_first: fn() -> A,
        wake_second: fn() -> B,
    ) -> Self {
        Self {
            retained: Some(WakeSenders {
                first,
                second,
                wake_first,
                wake_second,
            }),
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "verification-harness")]
    pub fn is_drained_for_verification(&self) -> bool {
        self.retained.is_none()
    }

    fn signal(&mut self, shutdown: &AtomicBool) {
        shutdown.store(true, Ordering::SeqCst);
        if let Some(wake) = self.retained.take() {
            // 每个接收者独立尝试；一个已经退出不阻止其余 worker 被唤醒。
            for sender in wake.first {
                let _ = sender.send((wake.wake_first)());
            }
            for sender in wake.second {
                let _ = sender.send((wake.wake_second)());
            }
        }
    }

    /// 永久断开：先通知，必要时唤醒，再释放协调者发送端。
    pub fn sever(
        &mut self,
        shutdown: &AtomicBool,
        first: &mut Vec<Sender<A>>,
        second: &mut Vec<Sender<B>>,
    ) {
        self.signal(shutdown);
        first.clear();
        second.clear();
    }

    /// 灾难收尾：唤醒已登记 worker，并释放协调者的发送端。
    pub fn start_fatal_stop(
        &mut self,
        shutdown: &AtomicBool,
        first: &mut Vec<Sender<A>>,
        second: &mut Vec<Sender<B>>,
    ) {
        if self.retained.is_some() {
            self.sever(shutdown, first, second);
        } else {
            self.signal(shutdown);
        }
    }

    /// 构造失败：无独立唤醒端时在 join 前释放原发送端。
    pub fn abort_construction(
        &mut self,
        shutdown: &AtomicBool,
        first: &mut Vec<Sender<A>>,
        second: &mut Vec<Sender<B>>,
    ) {
        if self.retained.is_some() {
            self.signal(shutdown);
        } else {
            self.sever(shutdown, first, second);
        }
    }

    /// 析构前准备唤醒。未登记 guard 时仍尝试已知发送端。
    pub fn drop_wake(
        &mut self,
        shutdown: &AtomicBool,
        first: &mut Vec<Sender<A>>,
        second: &mut Vec<Sender<B>>,
        wake_first: fn() -> A,
        wake_second: fn() -> B,
    ) {
        if self.retained.is_some() {
            self.sever(shutdown, first, second);
        } else {
            self.signal(shutdown);
            for sender in first.iter() {
                let _ = sender.send(wake_first());
            }
            for sender in second.iter() {
                let _ = sender.send(wake_second());
            }
        }
    }

    pub fn drop_untransferred(
        &mut self,
        shutdown: &AtomicBool,
        has_handles: bool,
    ) -> Option<UntransferredCleanup> {
        let explicit_wake = self.retained.is_some();
        if !explicit_wake && !has_handles {
            return None;
        }
        self.signal(shutdown);
        Some(UntransferredCleanup { explicit_wake })
    }
}

pub struct UntransferredCleanup {
    explicit_wake: bool,
}

impl UntransferredCleanup {
    pub fn report(self, joined: usize, errors: &[String]) {
        if cfg!(target_family = "wasm") && self.explicit_wake {
            // 普通 Err 已显式报告；遗漏出口不得假设浏览器可读取 stderr。
            if !errors.is_empty() {
                panic!("构造guard遗漏显式清理，Drop附加错误={errors:?}");
            }
        } else {
            use std::io::Write;
            // Native 已在 unwind 时，清理诊断不得覆盖其原始 panic。
            let _ = writeln!(
                std::io::stderr(),
                "native construction 未显式移交，已 join {joined} 个 worker；错误={errors:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, TryRecvError};

    #[test]
    fn 未登记发送端的灾难收尾暂不插入消息() {
        let shutdown = AtomicBool::new(false);
        let (tx, rx) = channel();
        let mut first = vec![tx];
        let mut second = Vec::<Sender<()>>::new();
        let mut wake = ShutdownWake::default();
        first[0].send(1).unwrap();

        wake.start_fatal_stop(&shutdown, &mut first, &mut second);
        assert!(shutdown.load(Ordering::SeqCst));
        assert_eq!(first.len(), 1);
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

        wake.drop_wake(&shutdown, &mut first, &mut second, || 2, || ());
        assert_eq!(first.len(), 1);
        assert_eq!(rx.try_recv(), Ok(2));
    }

    #[test]
    fn 生产构造登记两组裸接收唤醒端() {
        let shutdown = AtomicBool::new(false);
        let (group, group_rx) = channel();
        let (desk, desk_rx) = channel();
        // 模拟各方交叉持有。仅清空协调者发送端不能令裸接收断连。
        let mut groups = vec![group.clone()];
        let mut desks = vec![desk.clone()];
        let mut wake = ShutdownWake::for_workers(&groups, &desks, || 11, || 22);
        wake.sever(&shutdown, &mut groups, &mut desks);
        assert!(shutdown.load(Ordering::SeqCst));
        assert!(groups.is_empty() && desks.is_empty());
        assert_eq!(group_rx.try_recv(), Ok(11));
        assert_eq!(desk_rx.try_recv(), Ok(22));
        assert_eq!(group_rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(desk_rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn 裸等待的灾难收尾只追加一次唤醒并断开原发送端() {
        let shutdown = AtomicBool::new(false);
        let (tx, rx) = channel();
        let mut first = vec![tx];
        let mut second = Vec::<Sender<()>>::new();
        let mut wake = ShutdownWake {
            retained: Some(WakeSenders {
                first: first.clone(),
                second: Vec::new(),
                wake_first: || 2,
                wake_second: || (),
            }),
        };
        first[0].send(1).unwrap();

        wake.start_fatal_stop(&shutdown, &mut first, &mut second);
        wake.drop_wake(&shutdown, &mut first, &mut second, || 3, || ());
        assert!(shutdown.load(Ordering::SeqCst));
        assert!(first.is_empty());
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(rx.try_recv(), Ok(2));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }
}
