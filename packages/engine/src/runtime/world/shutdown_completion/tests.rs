//! 真实短 World 的生产停止行为候选；不把 Drop 或 stub Err 当作 join 证明。
use super::*;
use crate::runtime::protocol::GroupIn;
use crate::runtime::wait_contract::Phase;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn 宿主故障聚合错误保留来源_worker_与清理诊断() {
    let who = WorkerId::Group(1);
    let issue = runtime_issue("行政收尾", RuntimeError::HostAbortedCleanup {
        primary: Box::new(RuntimeError::HostAborted { phase: Phase::Drain, who: Some(who) }),
        detail: "真实 Final 缺失".to_owned(),
    });
    assert_eq!(issue.worker, Some(who.to_string()));
    assert!(issue.detail.contains("真实 Final 缺失"));
    assert!(issue.detail.contains("Drain"));
}

#[test]
fn 日结撤销聚合错误保留宿主故障的_worker_身份() {
    let who = WorkerId::Desk(1);
    let issue = runtime_issue("日结撤销", RuntimeError::DayEndRollbackHostAborted {
        original: Box::new(RuntimeError::DayEndValidation { day: 0, detail: "日结校验拒绝".to_owned() }),
        host_abort: Box::new(RuntimeError::HostAborted { phase: Phase::Drain, who: Some(who) }),
    });
    assert_eq!(issue.worker, Some(who.to_string()));
    assert!(issue.detail.contains("日结校验拒绝"));
    assert!(issue.detail.contains("Drain"));
}

#[test]
fn shutdown_failure_stays_within_the_error_layout_budget() {
    assert!(
        std::mem::size_of::<ShutdownFailure>() <= 128,
        "ShutdownFailure embeds its potentially large completion: size={}",
        std::mem::size_of::<ShutdownFailure>()
    );
}

fn world() -> World {
    let spec = crate::runtime::skeleton_tests::basic_spec(
        2,
        2,
        1,
        Box::new(|_| Box::new(|_, _| crate::runtime::group::TickPlan::default())),
    );
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar().expect("显式 Calendar 合法"),
    )
    .expect("真实 2 组 2 部短 World 构造成功")
}

#[test]
fn 真实_world_正常关停必须逐个证明接收器移交和消息收束() {
    use crate::libos::downlink::DrainState;
    use crate::libos::worker_exit::Outcome;
    use std::num::NonZeroUsize;

    let mut world = world();
    let mut stage = "真实关停";
    let report = world.shutdown_message_protocol(&mut stage).expect("正常消息协议应完成");
    assert_eq!((report.group_finals.len(), report.desk_finals.len()), (2, 2));
    world.stop_permanently();
    let mut recovered = Vec::new();
    let mut missing = Vec::new();
    let mut join_failures = Vec::new();
    for (who, handle) in std::mem::take(&mut world.handles) {
        let joined = match super::super::join_world_worker(
            who,
            handle,
            crate::clock::Instant::now() + Duration::from_secs(5),
            super::super::ExitReconcileContext {
                phase: super::super::ExitReconcilePhase::Shutdown,
                last_committed_tick: Some(world.global.committed_tick),
                active_tick: None,
                round: None,
                candidate_unpublished: true,
            },
        ) {
            Ok((_actual, joined)) => joined,
            Err(error) => {
                join_failures.push((who, error.to_string()));
                continue;
            }
        };
        let returned: Box<dyn std::any::Any> = Box::new(joined);
        match returned.downcast::<super::super::OwnedWorkerExit>() {
            Ok(exit) => recovered.push((who, *exit)),
            Err(_) => missing.push(who),
        }
    }
    assert!(join_failures.is_empty(), "真实 worker join 失败：{join_failures:?}");
    assert!(missing.is_empty(), "真实 join 必须移交有类型的 receiver：{missing:?}");
    assert_eq!(recovered.len(), 4);
    for (who, exit) in recovered {
        let deadline = crate::clock::Instant::now() + std::time::Duration::from_secs(1);
        match exit {
            super::super::OwnedWorkerExit::Group { who: actual, exit } => {
                assert_eq!(actual, who);
                assert!(matches!(exit.outcome, Outcome::Returned(Ok(()))));
                let closed = exit.receiver.close_and_wait(deadline).expect("组发送准入必须收束");
                let drain = closed.drain(NonZeroUsize::new(8).unwrap());
                assert_eq!(drain.state, DrainState::Drained);
                assert!(drain.messages.is_empty(), "正常关停后组不得遗留消息");
            }
            super::super::OwnedWorkerExit::Desk { who: actual, exit } => {
                assert_eq!(actual, who);
                assert!(matches!(exit.outcome, Outcome::Returned(Ok(()))));
                let closed = exit.receiver.close_and_wait(deadline).expect("交易部发送准入必须收束");
                let drain = closed.drain(NonZeroUsize::new(8).unwrap());
                assert_eq!(drain.state, DrainState::Drained);
                assert!(drain.messages.is_empty(), "正常关停后交易部不得遗留消息");
            }
        }
    }
}

#[test]
fn 未送达组_final_转停止诊断保留_worker_与首错() {
    let mut report = world().shutdown().unwrap();
    let final_state = report.group_finals.remove(0);
    let who = WorkerId::Group(final_state.group);
    let error = RuntimeError::GroupFinalUndelivered {
        who,
        primary: Some(Box::new(RuntimeError::ProtocolInvariant { detail: "原始组故障".to_owned() })),
        final_state: crate::runtime::protocol::RetainedGroupFinal::new(Box::new(final_state)),
    };
    let issue = runtime_issue("join", error);
    assert_eq!(issue.worker, Some(who.to_string()));
    assert!(issue.detail.contains("原始组故障"));
}

#[test]
fn 未送达交易部_final_转停止诊断保留_worker_与首错() {
    let mut report = world().shutdown().unwrap();
    let final_state = report.desk_finals.remove(0);
    let who = WorkerId::Desk(final_state.stock);
    let error = RuntimeError::DeskFinalUndelivered {
        who,
        primary: Some(Box::new(RuntimeError::ProtocolInvariant { detail: "原始交易部故障".to_owned() })),
        final_state: crate::runtime::protocol::RetainedDeskFinal::new(Box::new(final_state)),
    };
    let issue = runtime_issue("join", error);
    assert_eq!(issue.worker, Some(who.to_string()));
    assert!(issue.detail.contains("原始交易部故障"));
}

#[test]
fn 真实_world_上行断连后_join_携带每个组的原始_final() {
    let mut world = world();
    let (_replacement_tx, replacement_rx) = std::sync::mpsc::channel();
    drop(std::mem::replace(&mut world.up_rx, replacement_rx));
    world.sever();
    let results = world.join_worker_results();
    assert_eq!(results.len(), 4);
    let mut groups = Vec::new();
    let mut desks = Vec::new();
    for (who, result) in results {
        if let WorkerId::Group(group) = who {
            match result {
                Err(RuntimeError::GroupFinalUndelivered { who: actual, final_state, .. }) => {
                    assert_eq!(actual, who);
                    assert_eq!(final_state.as_ref().group, group);
                    assert!(!final_state.as_ref().accounts.is_empty(), "必须保留实际组账户快照");
                    groups.push(group);
                }
                other => panic!("上行断连必须随 join 交还组 Final：{other:?}"),
            }
        } else if let WorkerId::Desk(stock) = who {
            match result {
                Err(RuntimeError::DeskFinalUndelivered { who: actual, final_state, .. }) => {
                    assert_eq!(actual, who);
                    assert_eq!(final_state.as_ref().stock, stock);
                    desks.push(stock);
                }
                other => panic!("上行断连必须随 join 交还交易部 Final：{other:?}"),
            }
        }
    }
    groups.sort_unstable();
    assert_eq!(groups, [0, 1]);
    desks.sort_unstable();
    assert_eq!(desks, [0, 1]);
}

#[test]
fn 真实_world_上行断连后停止报告保留结构化组_final() {
    let mut world = world();
    let (replacement_tx, replacement_rx) = std::sync::mpsc::channel();
    drop(replacement_tx);
    drop(std::mem::replace(&mut world.up_rx, replacement_rx));
    let failure = world.shutdown_with_completion().expect_err("上行断连不得报告成功");
    let completion = failure.completion.expect("必须保留停止结果");
    assert_eq!((completion.owned_workers, completion.joined_workers), (4, 4));
    let mut groups = Vec::new();
    for worker in &completion.worker_results {
        if let Some(RuntimeError::GroupFinalUndelivered { who, final_state, .. }) = &worker.structured_error {
            assert_eq!(worker.worker, who.to_string());
            assert!(!final_state.as_ref().accounts.is_empty());
            assert!(std::ptr::eq(worker.retained_group_final().unwrap(), final_state.as_ref()));
            groups.push(final_state.as_ref().group);
        }
    }
    groups.sort_unstable();
    assert_eq!(groups, [0, 1], "两个组的 Final 必须随停止报告保留");
    let mut desks: Vec<_> = completion.worker_results.iter().filter_map(|worker| worker.retained_desk_final().map(|final_state| final_state.stock)).collect();
    desks.sort_unstable();
    assert_eq!(desks, [0, 1], "两个交易部的 Final 必须随停止报告保留");
    let wire = serde_json::to_value(&completion).unwrap();
    assert!(wire["worker_results"].as_array().unwrap().iter().all(|worker| worker.get("structured_error").is_none()));
    assert!(wire.to_string().contains("文本不含载荷"));
}

#[test]
fn active_shutdown_returns_all_real_joins_and_verified_finals_without_publication() {
    let mut world = world();
    // 沿用既有正常 Shutdown 前停顿 seam，超过 native worker 50ms 检查片。
    world.shutdown_stall_for_test = Some(std::time::Duration::from_millis(80));
    let publication = world.publication();
    let before = publication.load();
    let completion = world
        .shutdown_with_completion()
        .expect("活动 World 应完成正常 Drain/Final/join");
    assert_eq!(completion.owned_workers, 4);
    assert_eq!(completion.joined_workers, 4);
    assert_eq!(completion.worker_results.len(), 4);
    assert!(completion
        .worker_results
        .iter()
        .all(|worker| worker.result.is_ok()));
    assert_eq!(
        completion.group_finals, 2,
        "正常停止不能预置永久标记跳过 Final"
    );
    assert_eq!(completion.desk_finals, 2);
    let mut observed = completion.observed_group_shutdowns.clone();
    observed.sort_unstable();
    assert_eq!(
        observed,
        ["group-0", "group-1"],
        "两组 Final 必须见证实际处理 Shutdown 消息"
    );
    let mut identities: Vec<_> = completion
        .worker_results
        .iter()
        .map(|worker| worker.worker.as_str())
        .collect();
    identities.sort_unstable();
    assert_eq!(identities, ["desk-0", "desk-1", "group-0", "group-1"]);
    assert!(
        Arc::ptr_eq(&before, &publication.load()),
        "关闭不能发布新市场拍"
    );
}

#[test]
fn completed_normal_report_with_worker_errors_cannot_be_converted_to_success() {
    let mut report = world()
        .shutdown()
        .expect("先取得真实正常 Final 和实际 join 报告");
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    // 这里只验证报告转换边界，注入的 Err 不冒充真实 worker 执行失败。
    for (index, (_, result)) in report.worker_results.iter_mut().take(2).enumerate() {
        *result = Err(crate::runtime::protocol::RuntimeError::ProtocolInvariant {
            detail: format!("shutdown-report-worker-error-{index}"),
        });
    }
    let failure =
        complete_shutdown_report(4, report).expect_err("Ok(report) 内含 worker Err 必须判失败");
    let issues: Vec<_> = std::iter::once(&failure.primary)
        .chain(&failure.secondary)
        .collect();
    for (index, identity) in ["group-0", "group-1"].iter().enumerate() {
        assert!(
            issues
                .iter()
                .any(|issue| issue.worker.as_deref() == Some(*identity)
                    && issue
                        .detail
                        .contains(&format!("shutdown-report-worker-error-{index}"))),
            "转换不得短路遗漏第二个 worker Err"
        );
    }
    let completion = failure
        .completion
        .expect("已取得的真实 join 和 Final 事实不能丢失");
    assert_eq!(completion.owned_workers, 4);
    assert_eq!(completion.joined_workers, 4);
    assert_eq!((completion.group_finals, completion.desk_finals), (2, 2));
    assert_eq!(completion.worker_results.len(), 4);
    assert_eq!(
        completion
            .worker_results
            .iter()
            .filter(|worker| worker.result.is_err())
            .count(),
        2
    );
}

#[test]
fn active_shutdown_rejects_worker_errors_and_keeps_every_real_join_failure() {
    let mut world = world();
    for sender in &world.group_txs {
        sender
            .send(GroupIn::Continue { tick: 0, round: 0 })
            .expect("两组真实失序输入分别入队");
    }
    let mut failed = std::collections::BTreeSet::new();
    while failed.len() < 2 {
        match super::super::recv_deadline(
            &world.up_rx,
            super::super::Phase::TickConverge,
            &world.deadline,
        )
        .expect("真实 worker 失败握手必须先于停止入口")
        {
            super::super::UpMsg::Failed {
                who: super::super::WorkerId::Group(group),
                error,
            } => {
                assert!(error.to_string().contains("Continue 错序"));
                assert!(failed.insert(group), "同一 worker 不能冒作两项失败");
            }
            super::super::UpMsg::GroupFinal(_) => {}
            other => panic!("原 worker 失败握手收到非预期消息：{other:?}"),
        }
    }
    let (disconnected, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let _original_desk_sender = std::mem::replace(&mut world.desk_txs[0], disconnected);
    let failure = world
        .shutdown_with_completion()
        .expect_err("worker Err 不能装成成功 ack");
    assert_eq!(failure.primary.stage, "Drain");
    assert!(failure.primary.detail.contains("Drain 广播失败"));
    let completion = failure
        .completion
        .as_ref()
        .expect("正常流程失败仍需全员真实唤醒和 join 后给出事实");
    assert_eq!(completion.owned_workers, 4);
    assert_eq!(completion.joined_workers, 4);
    assert_eq!(completion.worker_results.len(), 4);
    for identity in ["group-0", "group-1"] {
        let worker = completion
            .worker_results
            .iter()
            .find(|worker| worker.worker == identity)
            .expect("不能短路遗漏第二个 worker");
        assert!(worker
            .result
            .as_ref()
            .unwrap_err()
            .contains("Continue 错序"));
        assert!(
            std::iter::once(&failure.primary)
                .chain(&failure.secondary)
                .any(|issue| issue.worker.as_deref() == Some(identity)
                    && issue.detail.contains("Continue 错序")),
            "主错和全部次错必须保留 worker 身份与原错误"
        );
    }

    assert_eq!((completion.group_finals, completion.desk_finals), (0, 0));
    let expected_issues = std::iter::once(&failure.primary)
        .chain(&failure.secondary)
        .map(|issue| {
            serde_json::json!({
                "stage": issue.stage,
                "worker": issue.worker,
                "detail": issue.detail,
            })
        })
        .collect::<Vec<_>>();
    let expected_workers = completion
        .worker_results
        .iter()
        .map(|worker| {
            let result = match &worker.result {
                Ok(()) => serde_json::json!({ "Ok": null }),
                Err(detail) => serde_json::json!({ "Err": detail }),
            };
            serde_json::json!({ "worker": worker.worker, "result": result })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(&failure).expect("ShutdownFailure 应可完整序列化"),
        serde_json::json!({
            "primary": expected_issues[0],
            "secondary": &expected_issues[1..],
            "completion": {
                "owned_workers": 4,
                "joined_workers": 4,
                "worker_results": expected_workers,
                "group_finals": 0,
                "desk_finals": 0,
            },
        }),
        "SHUTDOWN_FAILURE_JSON_TRUNCATED: primary、secondary 与真实 partial completion 必须完整保留"
    );
}

fn wait_group0_exit(world: &World) {
    let handle = world
        .handles
        .iter()
        .find(|(worker, _)| *worker == crate::runtime::protocol::WorkerId::Group(0))
        .map(|(_, handle)| handle)
        .expect("真实 group-0 native handle 必须仍由 World 持有");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while !handle.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "真实 group-0 worker 未在 1 秒内退出"
        );
        std::thread::yield_now();
    }
}

#[test]
fn active_shutdown_keeps_original_task_died_payload_and_worker_identity() {
    let script_for = |group: crate::runtime::GroupIdx| -> crate::runtime::group::GroupScript {
        Box::new(move |tick, _round| {
            let mut plan = crate::runtime::group::TickPlan::default();
            if group == 0 && tick == 1 {
                plan.panic_payload = Some("shutdown-original-task-died-payload".to_owned());
            }
            plan
        })
    };
    let spec = crate::runtime::skeleton_tests::basic_spec(2, 2, 1, Box::new(script_for));
    let world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar().expect("显式 Calendar 合法"),
    )
    .expect("真实 2 组 2 部短 World 构造成功");
    let calendar = world
        .prepare_raw_calendar_tick()
        .expect("首 tick Calendar 合法");
    crate::runtime::coordinator::broadcast_start(
        &world.group_txs[..1],
        1,
        calendar,
        &world.public_views,
        Default::default(),
    )
    .expect("只向真实 group-0 广播 Start");
    wait_group0_exit(&world);

    let failure = world
        .shutdown_with_completion()
        .expect_err("TaskDied worker 必须使活动 Shutdown 显式失败");
    assert_eq!(failure.primary.stage, "Drain");
    let issues = std::iter::once(&failure.primary)
        .chain(&failure.secondary)
        .collect::<Vec<_>>();
    assert!(
        issues.iter().any(|issue| {
            issue.worker.as_deref() == Some("group-0")
                && issue.detail.contains("shutdown-original-task-died-payload")
        }),
        "SHUTDOWN_TASK_DIED_DETAIL_LOST: 必须保留原 TaskDied payload 与 group-0 身份"
    );
    let completion = failure.completion.expect("失败清理仍须保留真实 join 事实");
    assert_eq!(completion.owned_workers, 4);
    assert_eq!(completion.joined_workers, 4);
    assert_eq!(completion.worker_results.len(), 4);
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 上行断连后真实组_panic_和_final_均保留在停止结果() {
    let script_for = |group: crate::runtime::GroupIdx| -> crate::runtime::group::GroupScript {
        Box::new(move |tick, _| {
            let mut plan = crate::runtime::group::TickPlan::default();
            if group == 0 && tick == 1 { plan.panic_payload = Some("原始组 panic 载荷".to_owned()); }
            plan
        })
    };
    let spec = crate::runtime::skeleton_tests::basic_spec(2, 2, 1, Box::new(script_for));
    let mut world = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    let calendar = world.prepare_raw_calendar_tick().unwrap();
    let (replacement_tx, replacement_rx) = std::sync::mpsc::channel();
    drop(std::mem::replace(&mut world.up_rx, replacement_rx));
    crate::runtime::coordinator::broadcast_start(&world.group_txs[..1], 1, calendar, &world.public_views, Default::default()).unwrap();
    wait_group0_exit(&world);
    drop(replacement_tx);
    let failure = world.shutdown_with_completion().expect_err("上行断连必须失败");
    let completion = failure.completion.expect("必须保留 join 结果");
    let group = completion.worker_results.iter().find(|result| result.worker == "group-0").unwrap();
    assert!(group.retained_group_final().is_some());
    assert_eq!(*group.take_undelivered_panic().expect("必须保留原始 panic").downcast::<String>().unwrap(), "原始组 panic 载荷");
    assert!(group.take_undelivered_panic().is_none());
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 失败清理恢复未送达的真实组_panic() {
    let script_for = |group: crate::runtime::GroupIdx| -> crate::runtime::group::GroupScript {
        Box::new(move |tick, _| {
            let mut plan = crate::runtime::group::TickPlan::default();
            if group == 0 && tick == 1 { plan.panic_payload = Some("失败清理原始 panic".to_owned()); }
            plan
        })
    };
    let spec = crate::runtime::skeleton_tests::basic_spec(2, 2, 1, Box::new(script_for));
    let mut world = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    let calendar = world.prepare_raw_calendar_tick().unwrap();
    let (replacement_tx, replacement_rx) = std::sync::mpsc::channel();
    drop(std::mem::replace(&mut world.up_rx, replacement_rx));
    crate::runtime::coordinator::broadcast_start(&world.group_txs[..1], 1, calendar, &world.public_views, Default::default()).unwrap();
    wait_group0_exit(&world);
    drop(replacement_tx);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        world.stop_after_fatal_failure(1, 0, "Start", RuntimeError::SendFailed { detail: "上行已断连".to_owned() }, super::super::FatalCleanupMode::StopImmediately, None)
    })).expect_err("必须恢复真实组原始 panic");
    assert_eq!(*panic.downcast::<String>().unwrap(), "失败清理原始 panic");
    let failure = world.shutdown_with_completion().expect_err("World 必须保持终态");
    let completion = failure.completion.unwrap();
    let group = completion.worker_results.iter().find(|result| result.worker == "group-0").unwrap();
    assert!(group.retained_group_final().is_some());
    assert!(group.take_undelivered_panic().is_none(), "panic 已移交，不能重复取出");
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 已知首个_panic_优先并保留另一组未送达载荷() {
    let script_for = |group: crate::runtime::GroupIdx| -> crate::runtime::group::GroupScript {
        Box::new(move |tick, _| {
            let mut plan = crate::runtime::group::TickPlan::default();
            if group == 0 && tick == 1 { plan.panic_payload = Some("另一组 panic".to_owned()); }
            plan
        })
    };
    let spec = crate::runtime::skeleton_tests::basic_spec(2, 2, 1, Box::new(script_for));
    let mut world = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    let calendar = world.prepare_raw_calendar_tick().unwrap();
    let (replacement_tx, replacement_rx) = std::sync::mpsc::channel();
    drop(std::mem::replace(&mut world.up_rx, replacement_rx));
    crate::runtime::coordinator::broadcast_start(&world.group_txs[..1], 1, calendar, &world.public_views, Default::default()).unwrap();
    wait_group0_exit(&world);
    drop(replacement_tx);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        world.stop_after_fatal_failure(1, 0, "Start", RuntimeError::SendFailed { detail: "上行已断连".to_owned() }, super::super::FatalCleanupMode::StopImmediately,
            Some((WorkerId::Group(1), Box::new(String::from("已知首个 panic")))))
    })).expect_err("必须优先恢复已知首个 panic");
    assert_eq!(*panic.downcast::<String>().unwrap(), "已知首个 panic");
    let failure = world.shutdown_with_completion().expect_err("World 必须保持终态");
    let completion = failure.completion.unwrap();
    let group = completion.worker_results.iter().find(|result| result.worker == "group-0").unwrap();
    assert!(group.retained_group_final().is_some());
    assert_eq!(*group.take_undelivered_panic().unwrap().downcast::<String>().unwrap(), "另一组 panic");
}

#[test]
fn shutdown_keeps_real_join_results_after_missing_final_cleanup() {
    let mut world = world();
    world.group_txs[0]
        .send(GroupIn::EchoCandles)
        .expect("路由错误消息应真实进入 group-0");
    wait_group0_exit(&world);

    world
        .run_ticks(1)
        .expect_err("真实 group-0 退出后 Start 必须触发失败清场");
    assert!(
        world.handles.is_empty(),
        "失败清场应已真实 join 全部 worker"
    );
    assert!(world.dead_reason.is_some(), "失败清场应记录终止原因");
    assert!(
        world.dead_finals.is_none(),
        "该真实失败路径未取得 Final 报告"
    );

    let failure = world
        .shutdown_with_completion()
        .expect_err("已终止 World 的停止必须显式返回失败事实");
    let completion = failure.completion.expect("已 join 事实必须进入 completion");
    assert_eq!(completion.owned_workers, 4);
    assert_eq!(
        completion.joined_workers, 4,
        "SHUTDOWN_PRIOR_JOIN_FACTS_LOST: 已完成 join 的数量不能被清空"
    );
    assert_eq!(completion.worker_results.len(), 4);
    let group0 = completion
        .worker_results
        .iter()
        .find(|worker| worker.worker == "group-0")
        .expect("必须保留 group-0 的真实 join 结果");
    assert!(
        group0.result.as_ref().unwrap_err().contains("EchoCandles"),
        "group-0 的真实 worker 错误应保留 EchoCandles 详情"
    );
}
