//! World 尚未形成时持有已启动 worker，并在普通失败时显式回收。
use super::*;
use crate::libos::downlink::Sender;

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
    up: &super::super::upstream::UpReceiver<UpMsg>,
    deadline: &super::wait_contract::DeadlineConfig,
) {
    groups[0]
        .send(GroupIn::Continue { tick: 0, round: 0 })
        .expect("部分构造中的真实 worker 注入失序 Continue");
    match recv_deadline(up, super::wait_contract::Phase::TickConverge, deadline).expect("部分构造 worker 原错误握手")
    {
        UpMsg::Failed {
            who: WorkerId::Group(0),
            error: RuntimeError::ProtocolInvariant { detail },
        } if detail.contains("Continue 错序") => {}
        other => panic!("部分构造错误握手不符：{other:?}"),
    }
}

pub(super) struct ConstructionGuard {
    shutdown: Arc<AtomicBool>,
    groups: Vec<Sender<GroupIn>>,
    desks: Vec<Sender<DeskMsg>>,
    handles: Vec<(WorkerId, super::WorldWorkerHandle)>,
    deadline: super::wait_contract::DeadlineConfig,
    #[cfg(all(test, not(target_family = "wasm")))]
    capture: Option<super::native_handoff_capture::Capture>,
}

impl ConstructionGuard {
    pub(super) fn new(shutdown: Arc<AtomicBool>, groups: Vec<Sender<GroupIn>>, desks: Vec<Sender<DeskMsg>>, capacity: usize, deadline: super::wait_contract::DeadlineConfig, #[cfg(all(test, not(target_family = "wasm")))] capture: Option<super::native_handoff_capture::Capture>) -> Self {
        Self {
            shutdown,
            groups,
            desks,
            handles: Vec::with_capacity(capacity),
            deadline,
            #[cfg(all(test, not(target_family = "wasm")))]
            capture,
        }
    }

    pub(super) fn push(
        &mut self,
        who: WorkerId,
        handle: super::WorldWorkerHandle,
    ) {
        self.handles.push((who, handle));
    }

    #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
    pub(super) fn handles_for_verification(&self) -> &[(WorkerId, super::WorldWorkerHandle)] {
        &self.handles
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
        self.wake_workers();
        groups.clear();
        desks.clear();
        let (joined, errors, retained_payloads) = self.join_all();
        if retained_payloads.is_empty() {
            RuntimeError::ConstructionFailure { primary, joined_workers: joined, cleanup_errors: errors }
        } else {
            RuntimeError::ConstructionCleanupFailure { primary, joined_workers: joined, cleanup_errors: errors, retained_payloads }
        }
    }

    fn wake_workers(&mut self) {
        super::super::worker_stop::wake_all(&self.shutdown, &self.groups, &self.desks);
        self.groups.clear();
        self.desks.clear();
    }

    fn join_all(&mut self) -> (usize, Vec<String>, Vec<super::protocol::RetainedWorkerPayload>) {
        let handles = std::mem::take(&mut self.handles);
        let mut joined = 0;
        let mut errors = Vec::new();
        let mut retained_payloads = Vec::new();
        let settle_deadline = crate::clock::Instant::now()
            + self.deadline.limit_for(super::wait_contract::Phase::Drain);
        let context = super::ExitReconcileContext {
            phase: super::ExitReconcilePhase::Construction,
            last_committed_tick: None,
            active_tick: None,
            round: None,
            candidate_unpublished: true,
        };
        for (who, handle) in handles {
            match super::join_world_worker(who, handle, settle_deadline, context.clone()) {
                Ok((_actual, exit)) => {
                    joined += 1;
                    #[cfg(all(test, not(target_family = "wasm")))]
                    let result = if let Some(capture) = &self.capture {
                        super::native_handoff_capture::capture_exit(capture, exit).1
                    } else {
                        super::settle_worker_exit(exit, settle_deadline, context.clone()).1
                    };
                    #[cfg(not(all(test, not(target_family = "wasm"))))]
                    let result = super::settle_worker_exit(exit, settle_deadline, context.clone()).1;
                    if let Err(error) = result {
                        errors.push(format!("worker {who}：{error}"));
                        if let Some(payload) = error.take_worker_payload() { retained_payloads.push(payload); }
                    }
                }
                Err(error) => {
                    if matches!(
                        &error,
                        RuntimeError::WorkerJoinFailed { child_released: true, .. }
                    ) {
                        joined += 1;
                    }
                    errors.push(format!("worker {who}：{error}"));
                    if let Some(payload) = error.take_worker_payload() {
                        retained_payloads.push(payload);
                    }
                }
            }
        }
        (joined, errors, retained_payloads)
    }

    pub(super) fn transfer(mut self) -> super::wasm_shutdown::ConstructionGuardTransfer<GroupIn, DeskMsg, WorkerId, super::WorldWorkerHandle> {
        let wake = super::wasm_shutdown::WakeSenders::new(
            std::mem::take(&mut self.groups), std::mem::take(&mut self.desks),
        );
        (Some(wake), std::mem::take(&mut self.handles))
    }
}

impl Drop for ConstructionGuard {
    fn drop(&mut self) {
        if self.handles.is_empty() {
            return;
        }
        self.wake_workers();
        let (joined, errors, retained_payloads) = self.join_all();
        for payload in retained_payloads { super::retain_unreported_payload(payload); }
        if !errors.is_empty() {
            if std::thread::panicking() {
                eprintln!("World construction 清理失败，已消费 {joined} 个 worker 句柄；错误={errors:?}");
            } else {
                panic!("World construction 遗漏显式清理，已消费 {joined} 个 worker 句柄；错误={errors:?}");
            }
        }
    }
}

#[cfg(test)]
mod unified_guard_tests {
    use super::*;

    #[test]
    fn 统一构造_guard_可移交唤醒端及句柄() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let (group, _group_rx) = crate::libos::downlink::channel::<GroupIn>();
        let (desk, _desk_rx) = crate::libos::downlink::channel::<DeskMsg>();
        let guard = ConstructionGuard::new(
            shutdown,
            vec![group],
            vec![desk],
            0,
            super::wait_contract::DeadlineConfig::default_config(),
            #[cfg(all(test, not(target_family = "wasm")))]
            None,
        );
        let (wake, handles) = guard.transfer();
        assert!(wake.is_some());
        assert!(handles.is_empty());
    }

    #[test]
    fn 构造_guard_遗漏显式清理时不得吞掉_worker_错误() {
        let mut guard = ConstructionGuard::new(
            Arc::new(AtomicBool::new(false)),
            Vec::new(),
            Vec::new(),
            1,
            super::wait_contract::DeadlineConfig::default_config(),
            #[cfg(all(test, not(target_family = "wasm")))]
            None,
        );
        let (_, receiver) = crate::libos::downlink::channel::<GroupIn>();
        let handle = crate::libos::thread::spawn_tracked(None, move || {
            super::super::OwnedWorkerExit::group(0, receiver, |_| Err(RuntimeError::ProtocolInvariant { detail: "原始 worker 故障".to_owned() }))
        }).unwrap();
        guard.push(WorkerId::Group(0), handle);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(guard)))
            .expect_err("遗漏显式清理且 worker 失败时必须显式报错");
        assert!(super::super::prepared_stop::panic_detail(panic.as_ref()).contains("原始 worker 故障"));
    }
}
