//! native World 尚未形成时也拥有全部已启动 worker，并在普通失败时显式回收。
use super::*;

#[cfg(test)]
thread_local! { static FAIL_AFTER: std::cell::Cell<Option<(usize, bool)>> = const { std::cell::Cell::new(None) }; }

#[cfg(test)]
pub(crate) fn fail_next_spawn_after_for_test(count: usize) {
    FAIL_AFTER.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some((count, false)));
    });
}

#[cfg(test)]
pub(crate) fn fail_next_spawn_with_cleanup_error_for_test(count: usize) {
    FAIL_AFTER.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some((count, true)));
    });
}

#[cfg(test)]
pub(super) fn fail_worker_for_test(
    groups: &[Sender<GroupIn>],
    up: &Receiver<UpMsg>,
    watchdog: &WatchdogConfig,
) {
    groups[0]
        .send(GroupIn::Continue { tick: 0, round: 0 })
        .expect("部分构造中的真实 worker 注入失序 Continue");
    match recv_deadline(up, Phase::TickConverge, watchdog).expect("部分构造 worker 原错误握手")
    {
        UpMsg::Failed {
            who: WorkerId::Group(0),
            error: RuntimeError::ProtocolInvariant { detail },
        } if detail.contains("Continue 错序") => {}
        other => panic!("部分构造错误握手不符：{other:?}"),
    }
}

pub(super) struct NativeConstructionGuard {
    shutdown: Arc<AtomicBool>,
    handles: Vec<(
        WorkerId,
        crate::thread_facade::JoinHandle<Result<(), RuntimeError>>,
    )>,
}

impl NativeConstructionGuard {
    pub(super) fn new(shutdown: Arc<AtomicBool>, capacity: usize) -> Self {
        Self {
            shutdown,
            handles: Vec::with_capacity(capacity),
        }
    }

    pub(super) fn push(
        &mut self,
        who: WorkerId,
        handle: crate::thread_facade::JoinHandle<Result<(), RuntimeError>>,
    ) {
        self.handles.push((who, handle));
    }

    #[cfg(test)]
    pub(super) fn inject_failure(&self) -> Option<bool> {
        FAIL_AFTER.with(|slot| {
            if let Some((count, with_worker_error)) = slot.get() {
                if count != self.handles.len() {
                    return None;
                }
                slot.set(None);
                Some(with_worker_error)
            } else {
                None
            }
        })
    }

    pub(super) fn spawn_failure(
        &mut self,
        primary: String,
        groups: &mut Vec<Sender<GroupIn>>,
        desks: &mut Vec<Sender<DeskMsg>>,
    ) -> RuntimeError {
        self.shutdown.store(true, Ordering::SeqCst);
        groups.clear();
        desks.clear();
        let (joined, errors) = self.join_all();
        RuntimeError::ConstructionFailure {
            primary,
            joined_workers: joined,
            cleanup_errors: errors,
        }
    }

    fn join_all(&mut self) -> (usize, Vec<String>) {
        let handles = std::mem::take(&mut self.handles);
        let joined = handles.len();
        let mut errors = Vec::new();
        for (who, handle) in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => errors.push(format!("worker {who}：{error}")),
                Err(payload) => errors.push(format!(
                    "join {who} panic：{}",
                    prepared_stop::panic_detail(payload.as_ref())
                )),
            }
        }
        (joined, errors)
    }

    pub(super) fn transfer(
        mut self,
    ) -> Vec<(
        WorkerId,
        crate::thread_facade::JoinHandle<Result<(), RuntimeError>>,
    )> {
        std::mem::take(&mut self.handles)
    }
}

impl Drop for NativeConstructionGuard {
    fn drop(&mut self) {
        if self.handles.is_empty() {
            return;
        }
        self.shutdown.store(true, Ordering::SeqCst);
        let (joined, errors) = self.join_all();
        // 普通 Err 已走显式清理；这里只报告未来遗漏出口或 unwind 的完整回收事实。
        eprintln!("native construction 未显式移交，已 join {joined} 个 worker；错误={errors:?}");
    }
}
