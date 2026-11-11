//! Worker 消息通道的平台选择；上层不依赖平台具体类型。

pub(crate) type UpSender<T> = super::mailbox::Sender<T>;
pub(crate) type UpReceiver<T> = super::mailbox::Receiver<T>;

pub(crate) type DownSender<T> = super::downlink::Sender<T>;
pub(crate) type DownReceiver<T> = super::downlink::Receiver<T>;

/// 通道初始化失败；业务层负责关联相位与可见错误。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ChannelError {
    HostAborted,
    InvalidCapacity,
    HostNotReady,
    DomainNotReady,
    SlotsFull,
}

/// 创建实例级故障域；WASM 的域由宿主预先登记，Native 为每个实例独立创建。
pub(crate) fn runtime_fault_domain(capacity: usize, fault: impl Fn() -> bool) -> Result<std::sync::Arc<super::fault_domain::FaultDomain>, ChannelError> {
    if capacity == 0 { return Err(ChannelError::InvalidCapacity); }
    if fault() { return Err(ChannelError::HostAborted); }
    #[cfg(not(target_family = "wasm"))]
    let domain = super::fault_domain::FaultDomain::new(capacity);
    #[cfg(target_family = "wasm")]
    let domain = {
        use super::fault_domain::GLOBAL_FAULT;
        if !GLOBAL_FAULT.host_is_ready() { return Err(ChannelError::HostNotReady); }
        GLOBAL_FAULT.domain().cloned().ok_or_else(|| if fault() { ChannelError::HostAborted } else { ChannelError::DomainNotReady })?
    };
    if fault() || domain.is_aborted() { return Err(ChannelError::HostAborted); }
    if domain.capacity() < capacity { return Err(ChannelError::SlotsFull); }
    Ok(domain)
}

fn register_error(error: super::fault_domain::RegisterError) -> ChannelError {
    match error {
        super::fault_domain::RegisterError::Aborted => ChannelError::HostAborted,
        super::fault_domain::RegisterError::Full => ChannelError::SlotsFull,
    }
}

/// 向实例故障域登记上行通道；故障广播可唤醒等待者。
pub(crate) fn upstream_channel_in<T>(domain: &std::sync::Arc<super::fault_domain::FaultDomain>, fault: impl Fn() -> bool) -> Result<(UpSender<T>, UpReceiver<T>), ChannelError> {
    if fault() || domain.is_aborted() { return Err(ChannelError::HostAborted); }
    let pair = domain.channel().map_err(register_error)?;
    if fault() || domain.is_aborted() { return Err(ChannelError::HostAborted); }
    Ok(pair)
}

/// 向同一实例故障域登记成对下行；调用者须同时完成退出和清理协议。
pub(crate) fn paired_downstream_channel<T>(domain: &std::sync::Arc<super::fault_domain::FaultDomain>, fault: impl Fn() -> bool) -> Result<(super::downlink::Sender<T>, super::downlink::Receiver<T>), ChannelError> {
    if fault() || domain.is_aborted() { return Err(ChannelError::HostAborted); }
    let pair = super::downlink::registered_channel(domain).map_err(register_error)?;
    if fault() || domain.is_aborted() { return Err(ChannelError::HostAborted); }
    Ok(pair)
}

pub(crate) fn upstream_channel<T>(fault: impl Fn() -> bool) -> Result<(UpSender<T>, UpReceiver<T>), ChannelError> {
    if fault() { return Err(ChannelError::HostAborted); }
    #[cfg(not(target_family = "wasm"))]
    let pair = super::mailbox::channel();
    #[cfg(target_family = "wasm")]
    let pair = {
        use super::fault_domain::{RegisterError, GLOBAL_FAULT};
        if !GLOBAL_FAULT.host_is_ready() { return Err(ChannelError::HostNotReady); }
        let domain = GLOBAL_FAULT.domain().ok_or_else(|| if fault() { ChannelError::HostAborted } else { ChannelError::DomainNotReady })?;
        domain.channel().map_err(|error| match error {
            RegisterError::Aborted => ChannelError::HostAborted,
            RegisterError::Full => ChannelError::SlotsFull,
        })?
    };
    if fault() { return Err(ChannelError::HostAborted); }
    Ok(pair)
}

pub(crate) fn downstream_channel<T>(fault: impl Fn() -> bool) -> Result<(DownSender<T>, DownReceiver<T>), ChannelError> {
    if fault() { return Err(ChannelError::HostAborted); }
    let pair = super::downlink::channel();
    if fault() { return Err(ChannelError::HostAborted); }
    Ok(pair)
}

/// 非阻塞收件的宿主终态；已出队消息由通知通道保留供清理。
#[derive(Debug)]
pub(crate) enum TryReceiveError {
    HostAborted,
}

fn checked_try_receive<T>(fault: impl Fn() -> bool, receive: impl FnOnce() -> T) -> Result<T, Option<T>> {
    if fault() { return Err(None); }
    let result = receive();
    if fault() { Err(Some(result)) } else { Ok(result) }
}

fn try_receive_mailbox<T>(
    rx: &super::mailbox::Receiver<T>,
    fault: impl Fn() -> bool,
) -> Result<Result<T, std::sync::mpsc::TryRecvError>, TryReceiveError> {
    checked_try_receive(|| rx.is_aborted() || fault(), || rx.try_recv()).map_err(|message| {
        if let Some(Ok(message)) = message { rx.retain_interrupted(message); }
        TryReceiveError::HostAborted
    })
}

pub(crate) fn try_receive_up<T>(
    rx: &UpReceiver<T>,
    fault: impl Fn() -> bool,
) -> Result<Result<T, std::sync::mpsc::TryRecvError>, TryReceiveError> {
    try_receive_mailbox(rx, fault)
}

/// 通知通道等待的底层结果；相位和业务错误由调用方提供。
#[derive(Debug)]
pub(crate) enum ReceiveFailure {
    Timeout { waited: std::time::Duration },
    ClockRegressed { previous: std::time::Duration, observed: std::time::Duration },
    Disconnected,
    HostAborted,
}

fn receive_mailbox<T>(
    rx: &super::mailbox::Receiver<T>,
    timeout: std::time::Duration,
    fault: impl Fn() -> bool,
) -> Result<T, ReceiveFailure> {
    use super::mailbox::ReceiveError;
    super::mailbox::receive_interruptible_when(rx, timeout, fault).map_err(|error| match error {
        ReceiveError::Timeout { waited } => ReceiveFailure::Timeout { waited },
        ReceiveError::ClockRegressed { previous, observed } => ReceiveFailure::ClockRegressed { previous, observed },
        ReceiveError::Disconnected => ReceiveFailure::Disconnected,
        ReceiveError::Aborted => ReceiveFailure::HostAborted,
        ReceiveError::AbortedWithMessage(message) => {
            rx.retain_interrupted(message);
            ReceiveFailure::HostAborted
        }
    })
}

/// 通过 LibOS 通知通道接收，故障和已取出消息的竞态由共同实现处理。
pub(crate) fn receive_up<T>(
    rx: &UpReceiver<T>,
    timeout: std::time::Duration,
    fault: impl Fn() -> bool,
) -> Result<T, ReceiveFailure> {
    receive_mailbox(rx, timeout, fault)
}

#[cfg(test)]
mod tests {
    use super::checked_try_receive;
    use std::cell::Cell;

    #[test]
    fn 非阻塞检查在取消息前后都拒绝宿主故障() {
        let reads = Cell::new(0);
        let before = checked_try_receive(|| true, || { reads.set(reads.get() + 1); 7 });
        assert_eq!(before, Err(None));
        assert_eq!(reads.get(), 0);
        let checks = Cell::new(0);
        let after = checked_try_receive(|| { checks.set(checks.get() + 1); checks.get() == 2 }, || 7);
        assert_eq!(after, Err(Some(7)));
        assert_eq!(checked_try_receive(|| false, || 7), Ok(7));
    }

    #[test]
    fn 故障发生在出队后时保留原消息和后续顺序() {
        let (tx, rx) = super::super::mailbox::channel();
        tx.send(7).unwrap();
        tx.send(8).unwrap();
        let checks = Cell::new(0);
        let result = super::try_receive_mailbox(&rx, || {
            checks.set(checks.get() + 1);
            checks.get() == 2
        });
        assert!(matches!(result, Err(super::TryReceiveError::HostAborted)));
        assert_eq!(rx.try_recv(), Ok(7));
        assert_eq!(rx.try_recv(), Ok(8));
    }

    #[test]
    fn 阻塞接收出队后故障仍按原序保留消息() {
        let (tx, rx) = super::super::mailbox::channel();
        tx.send(7).unwrap();
        tx.send(8).unwrap();
        let checks = Cell::new(0);
        let result = super::receive_mailbox(&rx, std::time::Duration::from_secs(1), || {
            checks.set(checks.get() + 1);
            checks.get() == 2
        });
        assert!(matches!(result, Err(super::ReceiveFailure::HostAborted)));
        assert_eq!(rx.try_recv(), Ok(7));
        assert_eq!(rx.try_recv(), Ok(8));
    }
}
