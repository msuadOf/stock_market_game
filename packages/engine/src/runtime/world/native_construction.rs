//! Native 构造失败的测试注入；生产构造和回收由统一 worker 生命周期管理。
use super::*;

thread_local! { static FAIL_AFTER: std::cell::Cell<Option<(usize, bool)>> = const { std::cell::Cell::new(None) }; }

pub(crate) fn fail_next_spawn_after_for_test(count: usize) {
    FAIL_AFTER.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some((count, false)));
    });
}

pub(crate) fn fail_next_spawn_with_cleanup_error_for_test(count: usize) {
    FAIL_AFTER.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some((count, true)));
    });
}

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

pub(super) fn inject_failure(spawned: usize) -> Option<bool> {
    FAIL_AFTER.with(|slot| {
        if let Some((count, with_worker_error)) = slot.get() {
            if count == spawned {
                slot.set(None);
                return Some(with_worker_error);
            }
        }
        None
    })
}
