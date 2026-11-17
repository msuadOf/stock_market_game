//! 观察者身份、切面安装和两类确认的共同状态机；传输只负责搬运有身份的请求。
use super::TerminalReadiness;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserverLease {
    pub generation: u64,
    pub lease: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeliveryEvidence {
    pub sent: u64,
    pub flushed: u64,
    pub accepted: u64,
}

#[derive(Default)]
pub struct ObserverLifecycle {
    counter: u64,
    current: Option<ObserverLease>,
    held: bool,
    fenced: bool,
    fenced_ordinal: u64,
    evidence: DeliveryEvidence,
}

impl ObserverLifecycle {
    /// 同一 generation 的重新挂接也必须更换 lease。
    pub fn hold(&mut self, generation: u64) -> ObserverLease {
        self.counter = self.counter.checked_add(1).expect("观察者 lease 已耗尽");
        let lease = ObserverLease {
            generation,
            lease: self.counter,
        };
        self.current = Some(lease);
        self.held = true;
        self.fenced = false;
        self.evidence = DeliveryEvidence::default();
        lease
    }
    pub fn is_current(&self, lease: ObserverLease) -> bool {
        self.current == Some(lease)
    }
    pub fn current(&self) -> Option<ObserverLease> {
        self.current
    }
    pub fn invalidate(&mut self) {
        self.current = None;
        self.held = false;
        self.fenced = false;
    }
    pub fn is_held(&self) -> bool {
        self.held
    }
    pub fn fence(&mut self, lease: ObserverLease, ordinal: u64) -> bool {
        if !self.is_current(lease) || !self.held || ordinal < self.evidence.accepted {
            return false;
        }
        self.fenced = true;
        self.fenced_ordinal = ordinal;
        true
    }
    pub fn release(&mut self, lease: ObserverLease) -> bool {
        if !self.is_current(lease) || !self.held || !self.fenced {
            return false;
        }
        self.held = false;
        self.evidence = DeliveryEvidence {
            sent: self.fenced_ordinal,
            flushed: self.fenced_ordinal,
            accepted: self.fenced_ordinal,
        };
        true
    }
    pub fn sent(&mut self, lease: ObserverLease, ordinal: u64) -> bool {
        if !self.is_current(lease) || self.held || ordinal < self.evidence.sent {
            return false;
        }
        self.evidence.sent = ordinal;
        true
    }
    pub fn flushed(&mut self, lease: ObserverLease, ordinal: u64) -> bool {
        if !self.is_current(lease)
            || ordinal < self.evidence.flushed
            || ordinal > self.evidence.sent
        {
            return false;
        }
        self.evidence.flushed = ordinal;
        true
    }
    pub fn accepted(&mut self, lease: ObserverLease, ordinal: u64) -> bool {
        if !self.is_current(lease)
            || ordinal < self.evidence.accepted
            || ordinal > self.evidence.sent
        {
            return false;
        }
        self.evidence.accepted = ordinal;
        true
    }
    pub fn evidence(&self) -> DeliveryEvidence {
        self.evidence
    }
    pub fn snapshot(&self) -> ObserverEvidence {
        ObserverEvidence {
            lease: self.current,
            held: self.held,
            delivery: self.evidence,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserverEvidence {
    pub lease: Option<ObserverLease>,
    pub held: bool,
    pub delivery: DeliveryEvidence,
}

/// terminal 不消耗观察窗口；成功前缀先送达，无法交付则显式说明缺口。
#[derive(Default)]
pub struct TerminalDelivery {
    delivered: bool,
}
impl TerminalDelivery {
    pub fn take(
        &mut self,
        readiness: TerminalReadiness,
        cannot_drain: bool,
        sent: u64,
        through: u64,
    ) -> Option<Option<(u64, u64)>> {
        if self.delivered {
            return None;
        }
        let gap = match readiness {
            TerminalReadiness::Ready => None,
            TerminalReadiness::Missing { from, through } => Some((from, through)),
            TerminalReadiness::Pending if cannot_drain => {
                (sent < through).then(|| (sent + 1, through))
            }
            TerminalReadiness::Pending => return None,
        };
        self.delivered = true;
        Some(gap)
    }
}
