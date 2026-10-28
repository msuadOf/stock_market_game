//! 已提交 TickArc 的事实边界；pending 与同 tick 重发布均不能冒充新 commit。
use super::*;
use crate::runtime::group::{PlaceSpec, TickPlan};
use crate::runtime::skeleton_tests::basic_spec;

fn world() -> World {
    let mut spec = basic_spec(
        1,
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
