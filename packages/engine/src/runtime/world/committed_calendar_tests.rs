//! 已提交 TickArc 的事实边界；pending 与同 tick 重发布均不能冒充新 commit。
use super::*;
use crate::runtime::group::{PlaceSpec, TickPlan};
use crate::runtime::skeleton_tests::basic_spec;

fn world() -> World {
    world_with_groups(1)
}

fn world_with_groups(n_groups: usize) -> World {
    let mut spec = basic_spec(
        n_groups,
        1,
        2,
        Box::new(|_| {
            Box::new(|tick, round| {
                let mut plan = TickPlan::default();
                if tick == 1 && round == 0 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: crate::Side::Buy,
                        price: 900,
                        qty: 100,
                    });
                    plan.continuation = true;
                }
                plan
            })
        }),
    );
    spec.ticks_per_day = 2;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .unwrap()
}

#[test]
fn 真实_world_正常关停后回收全部_worker_接收器() {
    let captured = native_handoff_capture::capture_next_world();
    let mut world = world();
    world.run_ticks(1).unwrap();
    let report = world.shutdown().unwrap();
    assert_eq!((report.group_finals.len(), report.desk_finals.len()), (1, 1));
    let recovered = captured.lock().unwrap();
    assert_eq!(recovered.len(), 2);
    let mut groups = 0;
    let mut desks = 0;
    for item in recovered.iter() {
        match item {
            native_handoff_capture::Recovered::Group(who, rx) => {
                assert_eq!(*who, 0);
                assert!(matches!(rx.try_receive().unwrap(), Ok(GroupIn::Shutdown)), "World 析构仍会发送尾随 Shutdown");
                assert!(matches!(rx.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
                groups += 1;
            }
            native_handoff_capture::Recovered::Desk(who, rx) => {
                assert_eq!(*who, 0);
                assert!(matches!(rx.try_receive().unwrap(), Ok(DeskMsg::Shutdown)), "World 析构仍会发送尾随 Shutdown");
                assert!(matches!(rx.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
                desks += 1;
            }
        }
    }
    assert_eq!((groups, desks), (1, 1));
}

#[test]
#[cfg(feature = "verification-harness")]
fn 真实_world_画像覆盖收束密封和安装() {
    let mut world = world();
    crate::runtime::step_profile::enable_on_this_thread();
    world.run_ticks(1).unwrap();
    let ledger = crate::runtime::step_profile::take_ledger().unwrap();
    for (segment, minimum) in [
        (crate::runtime::step_profile::StepSegment::WorldConverge, 1),
        (crate::runtime::step_profile::StepSegment::WorldSeal, 1),
        (crate::runtime::step_profile::StepSegment::WorldInstall, 1),
        (crate::runtime::step_profile::StepSegment::ConvergeReceive, 2),
    ] {
        let (_, ns, count) = ledger.iter().find(|(name, _, _)| *name == segment.name()).unwrap();
        assert!(*ns > 0 && *count >= minimum, "真实 World 画像缺少 {}", segment.name());
    }
    world.shutdown().unwrap();
}

#[test]
fn 构造部分失败后已启动线程的接收器全部断连() {
    for (started, worker_error) in [(0, false), (1, false), (2, false), (3, false), (3, true), (4, false)] {
        let captured = native_handoff_capture::capture_next_world();
        if worker_error {
            native_construction::fail_next_spawn_with_cleanup_error_for_test(started);
        } else {
            native_construction::fail_next_spawn_after_for_test(started);
        }
        let spec = basic_spec(2, 2, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
        let error = match World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()) {
            Ok(_) => panic!("构造注入必须失败"),
            Err(error) => error,
        };
        match error {
            RuntimeError::ConstructionFailure { primary, joined_workers, cleanup_errors } => {
                assert!(primary.contains("注入 spawn"));
                assert_eq!(joined_workers, started);
                if worker_error {
                    assert_eq!(cleanup_errors.len(), 1, "次生错误数量不符：{cleanup_errors:?}");
                    assert!(cleanup_errors[0].contains("group-0") && cleanup_errors[0].contains("Continue 错序"));
                } else {
                    assert!(cleanup_errors.is_empty(), "清理发生非预期次生错误：{cleanup_errors:?}");
                }
            }
            other => panic!("必须保留构造失败及 join 结果：{other:?}"),
        }
        let recovered = captured.lock().unwrap();
        assert_eq!(recovered.len(), started);
        let mut groups = Vec::new();
        let mut desks = Vec::new();
        for item in recovered.iter() {
            match item {
                native_handoff_capture::Recovered::Group(who, rx) => {
                    groups.push(*who);
                    let drained = crate::runtime::worker_mailbox::drain_for_shutdown(rx, std::num::NonZeroUsize::new(4).unwrap());
                    assert!(drained.messages.is_empty());
                    assert_eq!(drained.state, crate::runtime::worker_mailbox::DrainState::Disconnected);
                }
                native_handoff_capture::Recovered::Desk(who, rx) => {
                    desks.push(*who);
                    let drained = crate::runtime::worker_mailbox::drain_for_shutdown(rx, std::num::NonZeroUsize::new(4).unwrap());
                    assert!(drained.messages.is_empty());
                    assert_eq!(drained.state, crate::runtime::worker_mailbox::DrainState::Disconnected);
                }
            }
        }
        groups.sort_unstable();
        desks.sort_unstable();
        assert_eq!(groups, match started { 0 => vec![], 1 => vec![0], _ => vec![0, 1] });
        assert_eq!(desks, match started { 3 => vec![0], 4 => vec![0, 1], _ => vec![] });
    }
}

#[test]
fn 观察请求未启动_world时释放后不得污染下一次() {
    drop(native_handoff_capture::capture_next_world());
    drop(native_handoff_capture::capture_next_world());
}

#[test]
fn 重复观察注册被拒绝且不替换原注册() {
    let first = native_handoff_capture::capture_next_world();
    assert!(std::panic::catch_unwind(native_handoff_capture::capture_next_world).is_err());
    let world = world();
    world.shutdown().unwrap();
    assert_eq!(first.lock().unwrap().len(), 2, "真实 World 必须仍向首次注册交还两个 receiver");
}

#[test]
fn 真实_world_组失败后可观察未消费的后续资源批次() {
    let captured = native_handoff_capture::capture_next_world();
    let mut world = world();
    world.group_txs[0].send(GroupIn::Continue { tick:0, round:0 }).unwrap();
    match recv_deadline(&world.up_rx, Phase::TickConverge, &world.deadline).unwrap() {
        UpMsg::Failed { who:WorkerId::Group(0), error:RuntimeError::ProtocolInvariant { detail } } => assert!(detail.contains("Continue 错序")),
        other => panic!("必须先收到真实组错误：{other:?}"),
    }
    world.group_txs[0].send(GroupIn::Batch(super::super::DeskBatch {
        stock:0, tick:0, round:0,
        entities:vec![super::super::Entity::CashRefund { local:0, amount:500 }],
        ..super::super::DeskBatch::default()
    })).unwrap();
    world.sever();
    let results = world.join_worker_results();
    assert!(results.iter().any(|(who, result)| *who == WorkerId::Group(0) && result.is_err()));
    assert_eq!(results.len(), 2);
    drop(world);
    let recovered = captured.lock().unwrap();
    assert_eq!(recovered.len(), 2);
    let group = recovered.iter().find_map(|item| match item {
        native_handoff_capture::Recovered::Group(0, rx) => Some(rx), _ => None,
    }).expect("未交还组接收器");
    match group.try_receive().unwrap().unwrap() {
        GroupIn::Batch(batch) => assert_eq!(batch.entities, [super::super::Entity::CashRefund { local:0, amount:500 }]),
        _ => panic!("未消费的退款批次未保留"),
    }
    assert!(matches!(group.try_receive().unwrap(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
}

#[test]
fn 真实_world_全部线程退出后仍须等待外部发送端释放() {
    let captured = native_handoff_capture::capture_next_world();
    let mut world = world();
    let outstanding = world.group_txs[0].clone();
    world.group_txs[0].send(GroupIn::Continue { tick:0, round:0 }).unwrap();
    assert!(matches!(recv_deadline(&world.up_rx, Phase::TickConverge, &world.deadline).unwrap(), UpMsg::Failed { who:WorkerId::Group(0), .. }));
    world.sever();
    let results = world.join_worker_results();
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|(who, result)| *who == WorkerId::Group(0) && result.is_err()));
    drop(world);
    let recovered = captured.lock().unwrap();
    let group = recovered.iter().find_map(|item| match item {
        native_handoff_capture::Recovered::Group(0, rx) => Some(rx), _ => None,
    }).expect("未交还组接收器");
    let budget = std::num::NonZeroUsize::new(4).unwrap();
    let pending = crate::runtime::worker_mailbox::drain_for_shutdown(group, budget);
    assert!(pending.messages.is_empty());
    assert_eq!(pending.state, crate::runtime::worker_mailbox::DrainState::SendersAlive);
    outstanding.send(GroupIn::Batch(super::super::DeskBatch {
        stock:0, tick:0, round:0,
        entities:vec![super::super::Entity::CashRefund { local:0, amount:500 }],
        ..super::super::DeskBatch::default()
    })).unwrap();
    drop(outstanding);
    let drained = crate::runtime::worker_mailbox::drain_for_shutdown(group, budget);
    assert_eq!(drained.state, crate::runtime::worker_mailbox::DrainState::Disconnected);
    assert!(matches!(&drained.messages[..], [GroupIn::Batch(batch)] if batch.entities == [super::super::Entity::CashRefund { local:0, amount:500 }]));
}

#[test]
fn committed_calendar_only_records_successful_seal_and_keeps_historical_source() {
    let mut world = world();
    assert!(world.committed_calendar_tick().unwrap().is_none());
    let first = world.prepare_raw_calendar_tick().unwrap();
    world.stage_calendar_tick(Arc::clone(&first)).unwrap();
    assert!(world.committed_calendar_tick().unwrap().is_none());
    world.run_ticks(0).unwrap();
    assert!(world.committed_calendar_tick().unwrap().is_none());
    world.run_ticks(1).unwrap();
    assert!(
        world.report_log[0].rounds > 0,
        "真实 Continue 必须汇入同一成功 tick"
    );
    let committed = world.committed_calendar_tick().unwrap().unwrap();
    assert!(Arc::ptr_eq(&committed.calendar, &first));
    assert_eq!(committed.source, world.mirror_source());
    world.run_ticks(1).unwrap();
    let second = world.committed_calendar_tick().unwrap().unwrap();
    assert!(!Arc::ptr_eq(&second.calendar, &first));
    assert_eq!(second.calendar.after_input().tick, 2);
    assert_eq!(
        second.source.generation,
        world.publication.load().generation
    );
    world.run_ticks(0).unwrap();
    assert_eq!(
        world.committed_calendar_tick().unwrap().unwrap().source,
        second.source
    );
    world.freeze_retained_day(Arc::new(super::super::publication::RetainedDaySnapshot {
        date: world.civil_date,
        securities: Default::default(),
    }));
    let retained = world.committed_calendar_tick().unwrap().unwrap();
    assert!(Arc::ptr_eq(&retained.calendar, &second.calendar));
    assert_eq!(
        retained.source, second.source,
        "不得用新 generation 重新盖旧事实戳"
    );
    assert_ne!(retained.source.generation, world.mirror_source().generation);
    world.shutdown().unwrap();
}

#[test]
fn validator_rejection_does_not_replace_committed_calendar() {
    let mut world = world();
    world.run_ticks(1).unwrap();
    let before = world.committed_calendar_tick().unwrap().unwrap();
    world.set_validator(Box::new(|_| Err("synthetic-test-validator-reject".into())));
    world.run_ticks(1).expect_err("真实 validator 拒绝");
    assert!(Arc::ptr_eq(
        &world.last_committed_calendar.as_ref().unwrap().calendar,
        &before.calendar
    ));
    assert_eq!(
        world.last_committed_calendar.as_ref().unwrap().source,
        before.source
    );
    // 健康门不因仍有历史事实而放宽。
    if world.dead_reason.is_some() {
        assert!(world.committed_calendar_tick().is_err());
    }
    let _ = world.shutdown();
}

#[test]
fn information_old_context_is_rejected_after_new_tick_and_retained_keeps_original_identity() {
    let mut world = world();
    world.run_ticks(1).unwrap();
    let record = world.committed_calendar_tick().unwrap().unwrap();
    let clock = crate::runtime::InformationClockContext::after_committed(record.clone());
    let anchors = clock.anchors().unwrap();
    world
        .validate_information_clock(&clock, clock.cutoff(), &anchors)
        .unwrap();
    world.freeze_retained_day(Arc::new(super::super::publication::RetainedDaySnapshot {
        date: world.civil_date,
        securities: Default::default(),
    }));
    world
        .validate_information_clock(&clock, clock.cutoff(), &anchors)
        .expect("retained 同 tick 的旧事实戳仍是本拍原事实");
    assert_eq!(clock.committed_record().unwrap().source, record.source);
    world.run_ticks(1).unwrap();
    let error = world
        .validate_information_clock(&clock, clock.cutoff(), &anchors)
        .expect_err("新 tick 后不能复用旧 INFO clock");
    assert!(
        matches!(error, RuntimeError::ProtocolInvariant { detail } if detail.contains("历史身份或当前 tick"))
    );
    world.shutdown().unwrap();
}

#[test]
fn seal_failure_and_prepared_stop_never_promote_pending_calendar() {
    let mut world = world();
    world.run_ticks(1).unwrap();
    let before = world.committed_calendar_tick().unwrap().unwrap();
    let pending = world.prepare_raw_calendar_tick().unwrap();
    world.stage_calendar_tick(Arc::clone(&pending)).unwrap();
    assert!(Arc::ptr_eq(
        &world.committed_calendar_tick().unwrap().unwrap().calendar,
        &before.calendar
    ));
    world.set_seal_ack_failure_once_for_test(0);
    world.run_ticks(1).expect_err("真实 Seal 广播后收集失败");
    assert_eq!(world.global.committed_tick, 1);
    assert!(Arc::ptr_eq(
        &world.last_committed_calendar.as_ref().unwrap().calendar,
        &before.calendar
    ));
    assert_eq!(
        world.last_committed_calendar.as_ref().unwrap().source,
        before.source
    );
    world.run_ticks(1).expect("相同市场拍可重试，成功才换 Arc");
    let second = world.committed_calendar_tick().unwrap().unwrap();
    assert!(!Arc::ptr_eq(&second.calendar, &before.calendar));
    world.shutdown().unwrap();
}

#[test]
fn seal_宿主永久故障不得发布半拍或允许重试() {
    for ack_count in [0, 1] {
        let mut world = world_with_groups(2);
        world.run_ticks(1).unwrap();
        let before = world.committed_calendar_tick().unwrap().unwrap();
        let before_publication = world.publication.load();
        let before_reports = world.report_log.len();
        world.seal_host_abort_for_test = Some(ack_count);
        let error = world.run_ticks(1).expect_err("Seal 收集期间的宿主故障必须显式失败");
        assert!(matches!(error, RuntimeError::HostAborted { phase: Phase::Seal, .. }));
        assert_eq!(world.global.committed_tick, 1);
        assert!(Arc::ptr_eq(&world.last_committed_calendar.as_ref().unwrap().calendar, &before.calendar));
        assert!(Arc::ptr_eq(&world.publication.load(), &before_publication), "部分 SealAck 不得发布新视图");
        assert_eq!(world.report_log.len(), before_reports, "失败 tick 不得发布报告");
        assert!(world.is_terminal(), "宿主永久故障必须使 World 进入终态");
        assert!(world.run_ticks(1).is_err(), "终态不得重试或提交新一拍");
    }
}

#[test]
fn drain_宿主永久故障不得返回投影或允许重试() {
    let mut world = world();
    world.run_ticks(1).unwrap();
    let before_publication = world.publication.load();
    let before_tick = world.global.committed_tick;
    world.drain_host_abort_for_test = true;
    let error = world.take_candle_projection().expect_err("投影收集中的宿主故障必须显式失败");
    assert!(matches!(error, RuntimeError::HostAborted { phase: Phase::Drain, .. }));
    assert!(world.is_terminal(), "宿主永久故障必须使 World 进入终态");
    assert_eq!(world.global.committed_tick, before_tick);
    assert!(Arc::ptr_eq(&world.publication.load(), &before_publication));
    assert!(world.run_ticks(1).is_err(), "终态不得重试或发布新一拍");
}

#[test]
fn 生产计划簿读取期间宿主故障必须终止_world() {
    let mut world = world_with_groups(2);
    world.run_ticks(1).unwrap();
    let before_publication = world.publication.load();
    let before_tick = world.global.committed_tick;
    world.drain_host_abort_for_test = true;
    let error = world.collect_plans().expect_err("计划簿读取中的宿主故障必须显式失败");
    assert!(matches!(error, RuntimeError::HostAborted { phase: Phase::Drain, .. }));
    assert!(world.is_terminal(), "宿主永久故障必须使 World 进入终态");
    assert_eq!(world.global.committed_tick, before_tick);
    assert!(Arc::ptr_eq(&world.publication.load(), &before_publication));
    assert!(world.run_ticks(1).is_err(), "终态不得重试或发布新一拍");
}

#[test]
fn 各类上行读面遇宿主故障时不得返回局部结果或继续推进() {
    let reads: [(&str, fn(&mut World) -> Result<(), RuntimeError>); 9] = [
        ("开仓派生", |world| world.collect_open_derivations().map(|_| ())),
        ("观察基", |world| world.collect_group_observations().map(|_| ())),
        ("零售轨迹", |world| world.collect_retail_traces().map(|_| ())),
        ("信念", |world| world.collect_group_belief().map(|_| ())),
        ("稀疏注意力", |world| world.collect_group_attention_sparse().map(|_| ())),
        ("注意力", |world| world.collect_group_attention().map(|_| ())),
        ("挂单", |world| world.collect_desk_resting().map(|_| ())),
        ("账户", |world| world.snapshot_accounts().map(|_| ())),
        ("稀疏账户", |world| world.snapshot_accounts_sparse(&vec![Vec::new(); world.n_groups]).map(|_| ())),
    ];
    for (name, read) in reads {
        let mut world = world_with_groups(2);
        world.run_ticks(1).unwrap();
        let before = world.publication.load();
        let tick = world.global.committed_tick;
        world.drain_host_abort_for_test = true;
        let result = read(&mut world);
        assert!(matches!(result, Err(RuntimeError::HostAborted { phase: Phase::Drain, .. })), "{name} 必须保留宿主故障：{result:?}");
        assert!(world.is_terminal(), "{name} 的宿主故障必须判死");
        assert_eq!(world.global.committed_tick, tick);
        assert!(Arc::ptr_eq(&world.publication.load(), &before));
        assert!(read(&mut world).is_err(), "{name} 不得在终态返回成功的空结果");
        assert!(world.run_ticks(1).is_err());
    }
}

#[test]
fn 日结_drain_宿主故障必须保留原错误并终止_world() {
    let mut world = world_with_groups(2);
    world.run_ticks(1).unwrap();
    let before_tick = world.global.committed_tick;
    let before_publication = world.publication.load();
    world.drain_host_abort_for_test = true;
    let error = world.run_day_end_public().expect_err("日结 Drain 宿主故障必须失败");
    assert!(matches!(error, RuntimeError::HostAborted { phase: Phase::Drain, who: None }));
    assert!(world.is_terminal(), "宿主永久故障不能成为可重试的日结拒绝");
    assert_eq!(world.global.committed_tick, before_tick);
    assert!(Arc::ptr_eq(&world.publication.load(), &before_publication));
    assert!(world.last_day_end_candidate.is_none(), "故障不得发布日结候选");
    assert!(world.run_day_end_public().is_err(), "终态不得重试日结");
}

#[test]
fn 日结组回执阶段宿主故障不能发布候选或重试() {
    for received in [0, 1] {
        let mut world = world_with_groups(2);
        world.run_ticks(1).unwrap();
        let before_tick = world.global.committed_tick;
        let before_publication = world.publication.load();
        world.day_end_group_host_abort_after = Some(received);
        let error = world.run_day_end_public().expect_err("组日结回执期间宿主故障必须失败");
        assert!(matches!(error, RuntimeError::HostAborted { phase: Phase::Drain, who: None }));
        assert!(world.is_terminal());
        assert_eq!(world.global.committed_tick, before_tick);
        assert!(Arc::ptr_eq(&world.publication.load(), &before_publication));
        assert!(world.last_day_end_candidate.is_none());
        assert!(world.run_day_end_public().is_err());
    }
}

#[test]
fn 日结撤销时宿主故障保留原失败并终止_world() {
    use std::error::Error;
    let mut world = world_with_groups(2);
    world.run_ticks(1).unwrap();
    let before_publication = world.publication.load();
    world.set_day_end_validator(Box::new(|_| Err("验证用原始日结拒绝".to_owned())));
    world.day_end_rollback_host_abort_for_test = true;
    let error = world.run_day_end_public().expect_err("日结撤销期间宿主故障必须失败");
    let source = error.source()
        .and_then(|source| source.downcast_ref::<Box<RuntimeError>>())
        .map(Box::as_ref);
    assert!(matches!(source, Some(RuntimeError::DayEndValidation { detail, .. }) if detail.contains("验证用原始日结拒绝")));
    match error {
        RuntimeError::DayEndRollbackHostAborted { original, host_abort } => {
            assert!(matches!(*host_abort, RuntimeError::HostAborted { phase: Phase::Drain, who: None }));
            assert!(matches!(*original, RuntimeError::DayEndValidation { detail, .. } if detail.contains("验证用原始日结拒绝")));
        }
        other => panic!("必须同时保留原日结失败与宿主故障：{other:?}"),
    }
    assert!(world.is_terminal());
    assert!(Arc::ptr_eq(&world.publication.load(), &before_publication));
    assert!(world.last_day_end_candidate.is_none());
    assert!(world.run_day_end_public().is_err());
}

#[test]
fn start_send_failure_keeps_prior_fact_and_closes_healthy_read_gate() {
    let mut world = world();
    world.run_ticks(1).unwrap();
    let before = world.committed_calendar_tick().unwrap().unwrap();
    let (disconnected, rx) = channel();
    drop(rx);
    world.group_txs[0] = disconnected;
    world
        .run_ticks(1)
        .expect_err("真实断连 Start Sender 必须失败");
    assert_eq!(world.global.committed_tick, 1);
    assert!(Arc::ptr_eq(
        &world.last_committed_calendar.as_ref().unwrap().calendar,
        &before.calendar
    ));
    assert_eq!(
        world.last_committed_calendar.as_ref().unwrap().source,
        before.source
    );
    assert!(
        world.committed_calendar_tick().is_err(),
        "死亡 World 不得凭历史 Arc 绕健康门"
    );
    let _ = world.shutdown();
}

#[test]
fn closed_civil_frontier_rejects_prior_date_context_even_when_tick_is_unchanged() {
    for old_market_snapshot in [false, true] {
        let date = crate::CivilDate::from_iso("2030-01-05").unwrap();
        let start = if old_market_snapshot {
            date.prev().unwrap()
        } else {
            date
        };
        let mut spec = basic_spec(1, 1, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
        spec.calendar_start_date = start;
        spec.civil_start_date = start;
        spec.ticks_per_day = 2;
        spec.auction_ticks = 0;
        spec.closing_auction_ticks = 0;
        let mut world = World::spawn(
            spec,
            crate::TradingCalendar::current_default_calendar().unwrap(),
        )
        .unwrap();
        if old_market_snapshot {
            world.run_ticks(2).unwrap();
            world.set_civil_date(date);
        }
        let raw = |world: &World| {
            let day = world.calendar_source.day(world.civil_date).unwrap();
            let input = crate::session::calendar_clock_input(
                &day,
                world.tick_layout,
                world.global.committed_tick,
                world.global.committed_tick / world.ticks_per_day,
            )
            .unwrap();
            crate::runtime::InformationClockContext::fixture_static(
                day,
                world.tick_layout,
                input,
                crate::CivilInstant::from_hms(world.civil_date, 18, 0, 0).unwrap(),
            )
        };
        let saturday = raw(&world);
        let anchors = saturday.anchors().unwrap();
        world
            .validate_information_clock(&saturday, saturday.cutoff(), &anchors)
            .unwrap();
        let tick = world.global.committed_tick;
        world.set_civil_date(date.next().unwrap());
        assert_eq!(world.global.committed_tick, tick);
        assert!(world
            .validate_information_clock(&saturday, saturday.cutoff(), &anchors)
            .is_err());
        let sunday = raw(&world);
        world
            .validate_information_clock(&sunday, sunday.cutoff(), &sunday.anchors().unwrap())
            .unwrap();
        world.shutdown().unwrap();
    }
}
