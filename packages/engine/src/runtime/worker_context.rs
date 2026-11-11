//! worker 独占的协调通道、停止信号与诊断绑定；不持有业务权威状态。

use super::protocol::UpMsg;
use super::upstream::UpSender;
use crate::verification_evidence::PhaseTimingBinding;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub(crate) struct WorkerContext {
    pub(crate) up_tx: UpSender<UpMsg>,
    pub(crate) abort: Arc<AtomicBool>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) capture: Arc<Mutex<PhaseTimingBinding>>,
}
