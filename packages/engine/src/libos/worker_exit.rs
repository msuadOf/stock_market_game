//! Worker 返回及 Rust unwind 时移交接收器；宿主 trap 不保证执行此流程。

pub(crate) enum Outcome<E> {
    Returned(Result<(), E>),
    Panicked(Box<dyn std::any::Any + Send>),
}

pub(crate) struct Exit<R, E> {
    pub(crate) outcome: Outcome<E>,
    pub(crate) receiver: R,
}

/// 在移交前关闭发送准入；调用方仍须等待收束并核对载荷。
pub(super) fn run<T, E>(receiver: super::downlink::Receiver<T>, worker: impl FnOnce(&super::downlink::Receiver<T>) -> Result<(), E>) -> Exit<super::downlink::Receiver<T>, E> {
    let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker(&receiver))) {
        Ok(result) => Outcome::Returned(result),
        Err(payload) => Outcome::Panicked(payload),
    };
    receiver.close_admission();
    Exit { outcome, receiver }
}
