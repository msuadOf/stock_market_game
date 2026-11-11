//! 只回收私有 prepared World；不 Drain、不 Abort、不安装或发布 Final。
use super::*;

#[derive(Debug)]
pub(crate) struct StopReport {
    #[cfg_attr(
        not(any(
            all(test, not(target_family = "wasm")),
            all(target_family = "wasm", feature = "verification-harness")
        )),
        expect(
            dead_code,
            reason = "生产错误详情保留 join 数，native test 与 verification harness 逐字段核对"
        )
    )]
    pub joined_workers: usize,
    #[expect(
        dead_code,
        reason = "保留被回收上行消息数，并随 StopReport Debug 错误详情呈现"
    )]
    pub discarded_messages: usize,
}

pub(super) fn panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    crate::libos::thread::panic_payload_detail(payload)
}

/// getter 不得用 UpMsg 的省略 Debug 丢弃 panic 载荷详情。
pub(super) fn read_failure(stage: &str, message: UpMsg) -> RuntimeError {
    match message {
        UpMsg::PendingStartRejected(rejection) => RuntimeError::WorkerFailed {
            who: WorkerId::Group(rejection.group),
            error: Box::new(rejection.error),
        },
        UpMsg::Failed { who, error } => RuntimeError::WorkerFailed {
            who,
            error: Box::new(error),
        },
        UpMsg::TaskDied { who, payload } => RuntimeError::ProtocolInvariant {
            detail: format!(
                "{stage} 收到 worker {who} panic：{}",
                panic_detail(payload.as_ref())
            ),
        },
        other => RuntimeError::ProtocolInvariant {
            detail: format!("{stage} 收到非预期消息：{other:?}"),
        },
    }
}

impl World {
    /// 真正使一个 worker 因失序消息退出；仅用于验证移交尾的错误与全部 join。
    #[cfg(test)]
    pub(crate) fn inject_stop_failure_for_test(&self) -> Result<(), RuntimeError> {
        self.group_txs[0]
            .send(GroupIn::Continue { tick: 0, round: 0 })
            .map_err(|_| RuntimeError::SendFailed {
                detail: "停止失败 fixture 无法发送失序 Continue".to_owned(),
            })?;
        match recv_deadline(&self.up_rx, Phase::TickConverge, &self.deadline)? {
            UpMsg::Failed {
                who: WorkerId::Group(0),
                error: RuntimeError::ProtocolInvariant { detail },
            } if detail.contains("Continue 错序") => Ok(()),
            other => Err(read_failure("停止失败 fixture", other)),
        }
    }

    pub(crate) fn stop_without_drain(mut self) -> Result<StopReport, RuntimeError> {
        self.sever();
        let mut errors = Vec::new();
        let results = self.join_worker_results_with_context(super::ExitReconcileContext {
            phase: super::ExitReconcilePhase::PreparedStop,
            last_committed_tick: Some(self.global.committed_tick),
            active_tick: None,
            round: None,
            candidate_unpublished: false,
        });
        let joined_workers = results.iter().filter(|(_, result)| match result {
            Err(RuntimeError::WorkerJoinPending { .. }) => false,
            Err(RuntimeError::WorkerJoinFailed { child_released, .. }) => *child_released,
            _ => true,
        }).count();
        for (who, result) in results {
            if let Err(error) = result {
                errors.push(format!("worker {who}：{error}"));
            }
        }
        let mut discarded_messages = 0;
        for message in self.up_rx.try_iter() {
            discarded_messages += 1;
            match message {
                UpMsg::PendingStartRejected(rejection) => errors.push(format!(
                    "未交付 pending Start 错误：worker {}：{}",
                    WorkerId::Group(rejection.group),
                    rejection.error
                )),
                UpMsg::Failed { who, error } => {
                    errors.push(format!("未交付 worker {who} 错误：{error}"))
                }
                UpMsg::TaskDied { who, payload } => errors.push(format!(
                    "未交付 worker {who} panic：{}",
                    panic_detail(payload.as_ref())
                )),
                _ => {}
            }
        }
        let report = StopReport {
            joined_workers,
            discarded_messages,
        };
        if errors.is_empty() {
            Ok(report)
        } else {
            Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "prepared World 无 Drain 回收失败：{report:?}；全部错误={errors:?}"
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_read_failure_keeps_original_worker_panic_detail() {
        let error = read_failure(
            "static Full",
            UpMsg::TaskDied {
                who: WorkerId::Group(1),
                payload: Box::new("prepared-worker-original-panic".to_owned()),
            },
        );
        assert!(error.to_string().contains("prepared-worker-original-panic"));
        assert!(error.to_string().contains("static Full"));
    }

    #[cfg(not(target_family = "wasm"))]
    fn private_world() -> World {
        World::spawn(
            crate::runtime::skeleton_tests::basic_spec(
                2,
                2,
                1,
                Box::new(|_| Box::new(|_, _| crate::runtime::group::TickPlan::default())),
            ),
            crate::calendar::TradingCalendar::current_default_calendar().expect("显式Calendar合法"),
        )
        .expect("真实短prepared World构造成功")
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn prepared_stop_joins_real_private_world_without_publishing_final() {
        let world = private_world();
        let publication = world.publication();
        let before = publication.load();
        let report = world.stop_without_drain().expect("私有World回收成功");
        assert_eq!(report.joined_workers, 4);
        assert!(
            Arc::ptr_eq(&before, &publication.load()),
            "回收不得安装Final或发布市场事实"
        );
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn prepared_stop_keeps_real_worker_error_after_joining_all_workers() {
        let world = private_world();
        world.group_txs[0]
            .send(GroupIn::Continue { tick: 0, round: 0 })
            .expect("真实失序输入入队");
        match recv_deadline(&world.up_rx, Phase::TickConverge, &world.deadline)
            .expect("原worker失败握手")
        {
            UpMsg::Failed {
                who: WorkerId::Group(0),
                error: RuntimeError::ProtocolInvariant { detail },
            } if detail.contains("Continue 错序") => {}
            other => panic!("错误握手不是预期原worker失败：{other:?}"),
        }
        let error = world
            .stop_without_drain()
            .expect_err("回收须返回原worker错误");
        let detail = error.to_string();
        assert!(detail.contains("Continue 错序"), "原错误不可丢失：{detail}");
        assert!(
            detail.contains("joined_workers: 4"),
            "报告须证明全部真实handle已join：{detail}"
        );
    }
}
