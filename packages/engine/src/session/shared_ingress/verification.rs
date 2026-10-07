use super::{ReceiptBearingIntent, SessionError};
use crate::AccountId;
use std::sync::{Condvar, Mutex, MutexGuard};

#[derive(Clone, Debug, serde::Serialize)]
pub struct VerificationPoint {
    pub phase: String,
    pub tick: u64,
    pub account: Option<AccountId>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NpcReceiptObservation {
    pub observed_tick: u64,
    pub receipt: ReceiptBearingIntent,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CutoffObservation {
    pub observed_tick: u64,
    pub cutoff: u64,
    pub receipts: Vec<ReceiptBearingIntent>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct IngressVerificationSnapshot {
    #[serde(skip)]
    epoch: u64,
    pub point: Option<VerificationPoint>,
    pub entered: bool,
    pub released: bool,
    pub players: Vec<ReceiptBearingIntent>,
    pub npc_receipts: Vec<NpcReceiptObservation>,
    pub cutoffs: Vec<CutoffObservation>,
}

#[derive(Default)]
pub(super) struct IngressVerification {
    state: Mutex<IngressVerificationSnapshot>,
    released: Condvar,
}

impl IngressVerification {
    fn lock(&self) -> Result<MutexGuard<'_, IngressVerificationSnapshot>, SessionError> {
        self.state.lock().map_err(|error| {
            SessionError::ResourceLimit(format!("verification ingress 控制锁损坏：{error}"))
        })
    }

    pub(super) fn arm(&self, point: VerificationPoint) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.point.is_some() && !state.released {
            return Err(SessionError::ResourceLimit(
                "verification ingress 上一个 gate 尚未释放".to_owned(),
            ));
        }
        state.epoch = state.epoch.checked_add(1).ok_or_else(|| {
            SessionError::ResourceLimit("verification ingress gate epoch 溢出".to_owned())
        })?;
        state.point = Some(point);
        state.entered = false;
        state.released = false;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> Result<IngressVerificationSnapshot, SessionError> {
        Ok(self.lock()?.clone())
    }

    pub(super) fn release(&self) -> Result<(), SessionError> {
        self.lock()?.released = true;
        self.released.notify_all();
        Ok(())
    }

    pub(super) fn wait(
        &self,
        phase: &str,
        tick: u64,
        account: Option<AccountId>,
    ) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if !state.point.as_ref().is_some_and(|point| {
            point.phase == phase && point.tick == tick && point.account == account
        }) {
            return Ok(());
        }
        state.entered = true;
        let epoch = state.epoch;
        while state.epoch == epoch && !state.released {
            state = self.released.wait(state).map_err(|error| {
                SessionError::ResourceLimit(format!(
                    "verification ingress gate 等待锁损坏：{error}"
                ))
            })?;
        }
        Ok(())
    }

    pub(super) fn player(&self, receipt: &ReceiptBearingIntent) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.point.is_some() {
            state.players.push(receipt.clone());
        }
        Ok(())
    }

    pub(super) fn npc(
        &self,
        observed_tick: u64,
        receipt: &ReceiptBearingIntent,
    ) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.point.is_some() {
            state.npc_receipts.push(NpcReceiptObservation {
                observed_tick,
                receipt: receipt.clone(),
            });
        }
        Ok(())
    }

    pub(super) fn cutoff(
        &self,
        observed_tick: u64,
        cutoff: u64,
        receipts: &[ReceiptBearingIntent],
    ) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.point.is_some() {
            state.cutoffs.push(CutoffObservation {
                observed_tick,
                cutoff,
                receipts: receipts.to_vec(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_gate_waiter_does_not_wait_on_the_immediately_rearmed_epoch() {
        let control = std::sync::Arc::new(IngressVerification::default());
        control
            .arm(VerificationPoint {
                phase: "cutoff".to_owned(),
                tick: 1,
                account: None,
            })
            .unwrap();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || {
            worker_control.wait("cutoff", 1, None).unwrap();
            completed_tx.send(()).unwrap();
        });
        for _ in 0..1000 {
            if control.snapshot().unwrap().entered {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(control.snapshot().unwrap().entered);
        {
            let mut state = control.lock().unwrap();
            state.released = true;
            control.released.notify_all();
            state.epoch += 1;
            state.point = Some(VerificationPoint {
                phase: "cutoff".to_owned(),
                tick: 2,
                account: None,
            });
            state.entered = false;
            state.released = false;
        }
        let result = completed_rx.recv_timeout(std::time::Duration::from_secs(1));
        control.release().unwrap();
        worker.join().unwrap();
        result.expect("旧 waiter 必须在新 gate 仍未释放时退出");
    }
}
