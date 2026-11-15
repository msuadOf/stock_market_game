//! World 构造期所有权和永久停止；两端均保留独立的裸 recv 唤醒发送端。
use crate::runtime::protocol::RuntimeError;
use libos::lifecycle::ShutdownWake;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub(super) type ConstructionGuardTransfer<G, D, I, H> = (ShutdownWake<G, D>, Vec<(I, H)>);

pub(super) struct ConstructionGuard<G, D, I: std::fmt::Display, H> {
    shutdown: Arc<AtomicBool>,
    wake: ShutdownWake<G, D>,
    handles: Vec<(I, H)>,
    join: fn(H) -> Result<(), String>,
}

impl<G, D, I: std::fmt::Display, H> ConstructionGuard<G, D, I, H> {
    pub(super) fn new(
        shutdown: Arc<AtomicBool>,
        wake: ShutdownWake<G, D>,
        capacity: usize,
        join: fn(H) -> Result<(), String>,
    ) -> Self {
        Self {
            shutdown,
            wake,
            handles: Vec::with_capacity(capacity),
            join,
        }
    }

    pub(super) fn push(&mut self, who: I, handle: H) {
        self.handles.push((who, handle));
    }

    #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
    pub(super) fn handles_for_verification(&self) -> &[(I, H)] {
        &self.handles
    }

    #[cfg(all(test, not(target_family = "wasm")))]
    pub(super) fn worker_count(&self) -> usize {
        self.handles.len()
    }

    fn join_all(&mut self) -> (usize, Vec<String>) {
        let handles = std::mem::take(&mut self.handles);
        let joined_workers = handles.len();
        let mut errors = Vec::new();
        // 永久停止已通知所有 worker，裸 recv 的唤醒消息也已入队。
        for (who, handle) in handles {
            if let Err(error) = (self.join)(handle) {
                errors.push(format!("worker {who}：{error}"));
            }
        }
        (joined_workers, errors)
    }

    pub(super) fn spawn_failure(
        &mut self,
        primary: String,
        groups: &mut Vec<Sender<G>>,
        desks: &mut Vec<Sender<D>>,
    ) -> RuntimeError {
        self.wake.abort_construction(&self.shutdown, groups, desks);
        let (joined_workers, cleanup_errors) = self.join_all();
        RuntimeError::ConstructionFailure {
            primary,
            joined_workers,
            cleanup_errors,
        }
    }

    pub(super) fn transfer(mut self) -> ConstructionGuardTransfer<G, D, I, H> {
        (
            std::mem::take(&mut self.wake),
            std::mem::take(&mut self.handles),
        )
    }
}

impl<G, D, I: std::fmt::Display, H> Drop for ConstructionGuard<G, D, I, H> {
    fn drop(&mut self) {
        let Some(cleanup) = self
            .wake
            .drop_untransferred(&self.shutdown, !self.handles.is_empty())
        else {
            return;
        };
        let (joined, errors) = self.join_all();
        cleanup.report(joined, &errors);
    }
}

#[cfg(test)]
mod tests;
