//! 真实短 World 的生产停止行为候选；不把 Drop 或 stub Err 当作 join 证明。
use super::*;
use crate::runtime::protocol::GroupIn;
use std::sync::Arc;

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
            &world.watchdog,
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
