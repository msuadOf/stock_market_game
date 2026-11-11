//! 协调者通过 LibOS 上行通道接收消息与故障。
use super::protocol::RuntimeError;
use super::wait_contract::{DeadlineConfig, Phase};

pub(super) use crate::libos::transport::{UpReceiver, UpSender};

pub(super) fn channel<T>() -> Result<(UpSender<T>, UpReceiver<T>), RuntimeError> {
    crate::libos::transport::upstream_channel(external_fault).map_err(|error| channel_error(error, "上行"))
}

/// 同一 World 的上行与下行共享实例故障域。
pub(super) fn channel_in<T>(domain: &std::sync::Arc<crate::libos::fault_domain::FaultDomain>) -> Result<(UpSender<T>, UpReceiver<T>), RuntimeError> {
    crate::libos::transport::upstream_channel_in(domain, external_fault).map_err(|error| channel_error(error, "上行"))
}

pub(super) fn channel_error(error: crate::libos::transport::ChannelError, direction: &str) -> RuntimeError {
    use crate::libos::transport::ChannelError;
    match error {
        ChannelError::HostAborted => RuntimeError::HostAborted { phase: Phase::TickConverge, who: None },
        ChannelError::InvalidCapacity => RuntimeError::ProtocolInvariant { detail: format!("LibOS {direction}故障域容量必须大于零") },
        ChannelError::HostNotReady => RuntimeError::ProtocolInvariant { detail: format!("LibOS 宿主故障通知尚未确认，拒绝启动{direction}通道") },
        ChannelError::DomainNotReady => RuntimeError::ProtocolInvariant { detail: format!("LibOS {direction}故障域未就绪") },
        ChannelError::SlotsFull => RuntimeError::ProtocolInvariant { detail: format!("LibOS {direction}故障槽耗尽") },
    }
}

pub(super) fn recv_up_deadline<T>(
    rx: &UpReceiver<T>,
    phase: Phase,
    cfg: &DeadlineConfig,
) -> Result<T, RuntimeError> {
    crate::libos::transport::receive_up(rx, cfg.limit_for(phase), external_fault).map_err(|error| {
        use crate::libos::transport::ReceiveFailure;
        match error {
            ReceiveFailure::Timeout { waited } => RuntimeError::DeadlineExceeded { phase, waited, detail: format!("LibOS 上行相位 {phase:?} 等待超时") },
            ReceiveFailure::Disconnected => RuntimeError::Disconnected { who: None, stage: format!("LibOS 上行相位 {phase:?} 通道断连") },
            ReceiveFailure::ClockRegressed { previous, observed } => RuntimeError::ProtocolInvariant { detail: format!("LibOS 上行等待时钟倒退：phase={phase:?} previous={previous:?} observed={observed:?}") },
            ReceiveFailure::HostAborted => RuntimeError::HostAborted { phase, who: None },
        }
    })
}

pub(super) fn try_recv_up<T>(
    rx: &UpReceiver<T>,
    phase: Phase,
) -> Result<Result<T, std::sync::mpsc::TryRecvError>, RuntimeError> {
    let _ = phase;
    crate::libos::transport::try_receive_up(rx, external_fault).map_err(|error| match error {
        crate::libos::transport::TryReceiveError::HostAborted => RuntimeError::HostAborted { phase, who: None },
    })
}

pub(super) fn external_fault() -> bool {
    crate::libos::fault_domain::instance_failed()
}

pub(super) fn send_task_died(
    sender: &UpSender<super::protocol::UpMsg>,
    who: super::protocol::WorkerId,
    payload: Box<dyn std::any::Any + Send>,
) -> Result<(), RuntimeError> {
    match sender.send(super::protocol::UpMsg::TaskDied { who, payload }) {
        Ok(()) => Ok(()),
        Err(std::sync::mpsc::SendError(super::protocol::UpMsg::TaskDied { who, payload })) => {
            Err(RuntimeError::TaskDiedUndelivered { who, payload: super::protocol::RetainedPanicPayload::new(payload) })
        }
        Err(_) => unreachable!("发送错误必须交还原始 TaskDied"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_died_发送失败时保留原始_panic_载荷和_worker_身份() {
        let (tx, rx) = channel::<super::super::protocol::UpMsg>().unwrap();
        drop(rx);
        let error = send_task_died(&tx, super::super::protocol::WorkerId::Group(3), Box::new(String::from("原始 panic"))).unwrap_err();
        let retained = error.retained_panic_payload().expect("必须可从结构化错误取得 panic");
        assert_eq!(*retained.take().unwrap().downcast::<String>().unwrap(), "原始 panic");
        match error {
            RuntimeError::TaskDiedUndelivered { who, payload } => {
                assert_eq!(who, super::super::protocol::WorkerId::Group(3));
                assert!(payload.take().is_none());
            }
            other => panic!("必须保留原始 panic，实际：{other:?}"),
        }
    }

    #[test]
    fn task_died_发送成功时原始载荷由接收方取得() {
        let (tx, rx) = channel::<super::super::protocol::UpMsg>().unwrap();
        let original = std::sync::Arc::new(String::from("原始 panic"));
        send_task_died(&tx, super::super::protocol::WorkerId::Desk(2), Box::new(original.clone())).unwrap();
        match rx.try_recv().unwrap() {
            super::super::protocol::UpMsg::TaskDied { who, payload } => {
                assert_eq!(who, super::super::protocol::WorkerId::Desk(2));
                assert!(std::sync::Arc::ptr_eq(&original, &payload.downcast::<std::sync::Arc<String>>().unwrap()));
            }
            other => panic!("必须收到 TaskDied，实际：{other:?}"),
        }
    }
    #[test]
    fn 通道初始化错误标明上下行() {
        use crate::libos::transport::ChannelError;
        for (direction, expected) in [("上行", "上行"), ("下行", "下行")] {
            for error in [ChannelError::HostNotReady, ChannelError::DomainNotReady, ChannelError::SlotsFull] {
                assert!(channel_error(error, direction).to_string().contains(expected));
            }
        }
    }
    #[test]
    fn 上行已入队消息直接可用并保留顺序() {
        let (tx, rx) = channel().unwrap();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        let cfg = DeadlineConfig::default();
        assert_eq!(recv_up_deadline(&rx, Phase::TickConverge, &cfg).unwrap(), 1);
        assert_eq!(recv_up_deadline(&rx, Phase::TickConverge, &cfg).unwrap(), 2);
    }
    #[test]
    fn 上行超时与断连保留相位和错误类别() {
        let (tx, rx) = channel::<u8>().unwrap();
        let cfg = DeadlineConfig { normal: std::time::Duration::ZERO, long: std::time::Duration::from_secs(1) };
        assert!(matches!(recv_up_deadline(&rx, Phase::TickConverge, &cfg), Err(RuntimeError::DeadlineExceeded { phase:Phase::TickConverge, waited, .. }) if waited.is_zero()));
        drop(tx);
        assert!(matches!(recv_up_deadline(&rx, Phase::TickConverge, &cfg), Err(RuntimeError::Disconnected { who:None, .. })));
    }
}
