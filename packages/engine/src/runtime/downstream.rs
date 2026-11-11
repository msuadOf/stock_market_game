//! World 下行通道的 LibOS 静态类型及构造入口。

pub(super) use crate::libos::transport::{DownReceiver as Receiver, DownSender as Sender};

pub(super) fn channel<T>() -> Result<(Sender<T>, Receiver<T>), super::protocol::RuntimeError> {
    crate::libos::transport::downstream_channel(super::upstream::external_fault)
        .map_err(|error| super::upstream::channel_error(error, "下行"))
}

/// 为同一 World 故障域登记成对下行；调用者负责接入退出与资源清理协议。
pub(super) fn paired_channel_in<T>(domain: &std::sync::Arc<crate::libos::fault_domain::FaultDomain>) -> Result<(Sender<T>, Receiver<T>), super::protocol::RuntimeError> {
    crate::libos::transport::paired_downstream_channel(domain, super::upstream::external_fault)
        .map_err(|error| super::upstream::channel_error(error, "下行"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn 默认下行保持消息顺序与断连() {
        let (sender, receiver) = super::channel::<u32>().unwrap();
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        drop(sender);
        assert_eq!(receiver.try_receive().unwrap(), Ok(1));
        assert_eq!(receiver.try_receive().unwrap(), Ok(2));
        assert!(matches!(receiver.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
    }
}
