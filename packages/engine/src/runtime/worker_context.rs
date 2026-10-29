//! worker 独占的协调通道、停止信号与诊断绑定；不持有业务权威状态。

use super::protocol::UpMsg;
use super::watchdog::WatchdogConfig;
use crate::verification_evidence::PhaseTimingBinding;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub(crate) struct WorkerContext {
    pub(crate) up_tx: Sender<UpMsg>,
    pub(crate) abort: Arc<AtomicBool>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) watchdog: WatchdogConfig,
    pub(crate) capture: Arc<Mutex<PhaseTimingBinding>>,
}
