//! typed DayEnd：实际 caller 拒绝 foreign/recreated TickArc 并保留手动日终合同。
//! 同值 context 不能替代已提交原件；Manual 不伪造 Completed。

use super::calendar_cancel_all_tests::mixed_spec;
use super::group::TickPlan;
use super::protocol::{DayEndCalendar, RuntimeError};
use super::world::World;
use std::sync::Arc;

fn assert_same_clock_input(
    actual: crate::session::exchange_calendar_model::SharedClockInput,
    expected: crate::session::exchange_calendar_model::SharedClockInput,
) {
    use crate::session::exchange_calendar_model::SharedClockInput;
    let SharedClockInput {
        tick,
        completed_shared_sessions,
        progress,
    } = actual;
    let SharedClockInput {
        tick: expected_tick,
        completed_shared_sessions: expected_sessions,
        progress: expected_progress,
    } = expected;
    assert_eq!(
        (tick, completed_shared_sessions, progress),
        (expected_tick, expected_sessions, expected_progress)
    );
}

fn world(ticks: u64) -> World {
    let (spec, calendar) = mixed_spec(Box::new(|_| Box::new(|_, _| TickPlan::default())));
    let mut world = World::spawn(spec, calendar).unwrap();
    world.run_ticks(ticks).unwrap();
    world
}

fn reject_without_drain(world: &mut World, input: DayEndCalendar, expected: &str) {
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    let plans = world.collect_plans().unwrap();
    let orders = world.collect_desk_resting().unwrap();
    let candles = world.take_candle_projection().unwrap();
    let public_views = world.public_views().to_vec();
    let error = world
        .run_day_end_with_calendar(input)
        .expect_err("无权context必须在Drain之前拒绝");
    assert_eq!(
        error,
        RuntimeError::ProtocolInvariant {
            detail: expected.to_owned()
        }
    );
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    assert_eq!(world.collect_plans().unwrap(), plans);
    assert_eq!(world.collect_desk_resting().unwrap(), orders);
    assert_eq!(world.take_candle_projection().unwrap(), candles);
    assert_eq!(world.public_views(), public_views.as_slice());
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert!(world.take_pending_day_bars().iter().all(Option::is_none));
    assert!(world.take_pending_day_candles().iter().all(Option::is_none));
    assert_eq!(world.live_workers_for_test(), 4, "拒绝不应杀死真实worker");
}

#[test]
fn market_day_end_rejects_same_content_committed_tick_from_foreign_world_before_drain() {
    let mut target = world(12);
    let foreign = world(12);
    let own = target.committed_calendar_tick().unwrap().unwrap().calendar;
    let other = foreign.committed_calendar_tick().unwrap().unwrap().calendar;
    assert_same_clock_input(own.before_input(), other.before_input());
    assert_same_clock_input(own.after_input(), other.after_input());
    assert!(!Arc::ptr_eq(&own, &other));
    reject_without_drain(
        &mut target,
        DayEndCalendar::MarketTick(other),
        "Calendar context 不属于接收方持有的 source owner",
    );
    target
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(own))
        .unwrap();
    let finished = target.shutdown().unwrap();
    assert_eq!(finished.worker_results.len(), 4);
    assert!(finished
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    assert!(foreign
        .shutdown()
        .unwrap()
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn market_day_end_rejects_recreated_same_owner_same_value_tick_then_accepts_committed_arc() {
    let mut target = world(12);
    let own = target.committed_calendar_tick().unwrap().unwrap().calendar;
    let recreated = own.day().tick(own.layout(), own.before_input()).unwrap();
    assert!(Arc::ptr_eq(own.day(), recreated.day()));
    assert_same_clock_input(own.after_input(), recreated.after_input());
    assert!(!Arc::ptr_eq(&own, &recreated));
    reject_without_drain(
        &mut target,
        DayEndCalendar::MarketTick(recreated),
        "市场 DayEnd 原始已提交 Calendar 身份或末拍进度不一致",
    );
    target
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(own))
        .unwrap();
    let report = target.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn manual_day_end_accepts_same_owner_day_without_completed_market_tick() {
    let mut target = world(4);
    let committed = target.committed_calendar_tick().unwrap().unwrap();
    assert_eq!(committed.source.tick, 4);
    assert!(matches!(
        committed.calendar.after_input().progress,
        crate::session::exchange_calendar_model::SharedDayProgress::Trading { completed_ticks: 4 }
    ));
    let day = Arc::clone(committed.calendar.day());
    let closed_book = target.collect_plans().unwrap()[0].1.clone();
    target
        .run_day_end_with_calendar(DayEndCalendar::ManualDayEnd(day))
        .expect("显式手动日终按同owner纯投影验证，不伪造Completed或MarketTick");
    assert_eq!(target.global.committed_tick, 4);
    assert_eq!(target.report_log.len(), 4);
    assert_eq!(target.collect_plans().unwrap()[0].1, closed_book);
    let final_candles = target.take_pending_day_candles();
    assert!(
        final_candles.iter().all(Option::is_none),
        "midday手动Drain不roll或伪造终K"
    );
    let after = target.committed_calendar_tick().unwrap().unwrap();
    assert!(Arc::ptr_eq(&after.calendar, &committed.calendar));
    assert!(matches!(
        after.calendar.after_input().progress,
        crate::session::exchange_calendar_model::SharedDayProgress::Trading { completed_ticks: 4 }
    ));
    let report = target.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    assert!(report.desk_finals[0].market.candles.is_empty());
    assert_eq!(
        report.desk_finals[0].market.candle.unwrap().volume,
        0,
        "open活动K保持日内形态"
    );
    assert!(report.desk_finals[1].market.candles.is_empty());
    assert!(
        report.desk_finals[1].market.candle.is_none(),
        "closed没有活动或已完成K占位"
    );
}
