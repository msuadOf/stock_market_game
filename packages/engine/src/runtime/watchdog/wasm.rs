//! 仅 verification-harness 的 Browser 实测探针，用实际 LibOS 等待途径。

pub(crate) use libos::wait::verify_host_aborted;

mod verification {
    use super::super::{coarse_wait, failure, Phase, SpinConfig, WatchdogConfig};
    use crate::runtime::protocol::{RuntimeError, WorkerId};
    use crate::runtime::watchdog_wait_verification::WaitProbe;
    use libos::wait;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;

    impl wait::WaitObserver for WaitProbe {
        fn note_spin(&self) {
            WaitProbe::note_spin(self);
        }

        fn note_sleep(&self) {
            WaitProbe::note_sleep(self);
        }

        fn note_raw_recv_selected(&self) {
            WaitProbe::note_raw_recv_selected(self);
        }
    }

    pub(crate) fn verify_worker_recv<T>(
        rx: &Receiver<T>,
        cfg: &WatchdogConfig,
        shutdown: &AtomicBool,
        who: WorkerId,
        spin: SpinConfig,
        probe: Arc<WaitProbe>,
    ) -> Result<Option<T>, RuntimeError> {
        wait::verify_worker_recv(rx, shutdown, spin, probe.as_ref())
            .map_err(|error| failure(error, Phase::WorkerIdle, cfg, Some(who)))
    }

    pub(crate) fn verify_coordinator_recv<T>(
        rx: &Receiver<T>,
        phase: Phase,
        cfg: &WatchdogConfig,
        probe: Arc<WaitProbe>,
    ) -> Result<T, RuntimeError> {
        wait::verify_coordinator_recv(rx, cfg.limit_for(phase), coarse_wait(phase), probe.as_ref())
            .map_err(|error| failure(error, phase, cfg, None))
    }
}

pub(crate) use verification::{verify_coordinator_recv, verify_worker_recv};
