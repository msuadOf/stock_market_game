//! V5：mixed 市场保留 #120 昨收基准、真实清算→归档→roll 与 Abort。
//! 深市并列价沿既有简化；不将其当作完整交易所规则。

use super::calendar_cancel_all_tests::mixed_spec;
use super::group::{PlaceSpec, TickPlan};
use super::protocol::{DayEndCalendar, RuntimeError};
use super::world::World;
use std::sync::Arc;

fn mixed_closing_tie_world() -> World {
    let (mut spec, calendar) = mixed_spec(Box::new(|group| Box::new(move |tick, round| {
        let mut plan = TickPlan::default();
        if round == 0 && (tick == 9 || tick == 11) {
            plan.places.push(PlaceSpec {
                local: 0, stock: 0,
                side: if group == 0 { crate::Side::Buy } else { crate::Side::Sell },
                price: if tick == 9 || group == 0 { 1100 } else { 900 },
                qty: 100,
            });
        }
        plan.continuation = tick == 12 && round == 0 && group == 0;
        plan
    })));
    // 与 #120 原并列候选 fixture 一致，隔离已登记的收盘价选择/roll 顺序。
    Arc::make_mut(&mut spec.config).price_cage_enabled = false;
    World::spawn(spec, calendar).unwrap()
}

fn original_market_tick(world: &World) -> DayEndCalendar {
    let last = world.committed_calendar_tick().unwrap().unwrap();
    let again = world.committed_calendar_tick().unwrap().unwrap();
    assert!(Arc::ptr_eq(&last.calendar, &again.calendar));
    DayEndCalendar::MarketTick(last.calendar)
}

#[test]
fn mixed_closing_tie_finalizes_old_close_once_and_never_rolls_closed_stock() {
    let mut world = mixed_closing_tie_world();
    world.run_ticks(12).unwrap();
    assert_eq!(world.report_log.last().unwrap().rounds, 1);
    let before = world.publication().load();
    assert_eq!(before.desks[0].view.last_price, 1100);
    assert_eq!(before.desks[0].view.daily_upper_limit, 1100, "末拍仍按昨收1000而非最新1100");
    assert_eq!(before.desks[1].view.last_price, 1000);
    let closed_serial = world.collect_desk_resting().unwrap().1[1];
    let calendar = original_market_tick(&world);
    world.run_day_end_with_calendar(calendar.clone()).unwrap();
    assert_eq!(world.drained_day_trades.len(), 1);
    assert_eq!((world.drained_day_trades[0].stock, world.drained_day_trades[0].price, world.drained_day_trades[0].qty),
        (0, 900, 100), "深市既有统一昨收并列取低价简化，不能改按最新1100选价");
    let finalized = world.take_pending_day_candles();
    let candle = finalized[0].unwrap();
    assert_eq!((candle.open, candle.high, candle.low, candle.close, candle.volume, candle.turnover_cents, candle.trade_count),
        (1100, 1100, 900, 900, 200, 200_000, 2));
    assert!(finalized[1].is_none(), "closed无清算或终K占位");
    let after = world.public_views();
    assert_eq!((after[0].daily_upper_limit, after[0].daily_lower_limit), (990, 810));
    assert_eq!(after[1].daily_upper_limit, 1100);
    assert_eq!(after[1].last_price, 1000);
    assert_eq!(world.collect_desk_resting().unwrap().1[1], closed_serial);
    assert!(matches!(world.run_day_end_with_calendar(calendar), Err(RuntimeError::ProtocolInvariant { detail })
        if detail == "交易日已日结（收尾 tick=12）——不得重复日结"));
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report.worker_results.iter().all(|(_, result)| result.is_ok()));
    assert_eq!(report.desk_finals[0].market.candles.len(), 1);
    assert_eq!(report.desk_finals[0].market.last_close, 900);
    assert!(report.desk_finals[1].market.candles.is_empty());
    assert_eq!(report.desk_finals[1].market.last_close, 1000);
}

#[test]
fn mixed_closing_tie_failed_day_end_restores_before_roll_then_retries_once() {
    let mut world = mixed_closing_tie_world();
    world.run_ticks(12).unwrap();
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    let plans = world.collect_plans().unwrap();
    let orders = world.collect_desk_resting().unwrap();
    let candles = world.take_candle_projection().unwrap();
    let calendar = original_market_tick(&world);
    world.set_day_end_validator(Box::new(|_| Err("V5 mixed真实清算后拒绝日结".into())));
    let error = world.run_day_end_with_calendar(calendar.clone()).expect_err("清算后校验失败必须显式返回");
    assert!(matches!(error, RuntimeError::DayEndValidation { ref detail, .. } if detail == "V5 mixed真实清算后拒绝日结"));
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    assert_eq!(world.collect_plans().unwrap(), plans);
    assert_eq!(world.collect_desk_resting().unwrap(), orders);
    assert_eq!(world.take_candle_projection().unwrap(), candles);
    assert_eq!(world.public_views()[0].daily_upper_limit, 1100);
    assert!(world.drained_day_trades.is_empty());
    assert!(world.take_pending_day_bars().iter().all(Option::is_none));
    assert!(world.take_pending_day_candles().iter().all(Option::is_none));
    world.set_day_end_validator(Box::new(|_| Ok(())));
    world.run_day_end_with_calendar(calendar).unwrap();
    assert_eq!(world.drained_day_trades.len(), 1, "Abort后重试不能重复收盘成交");
    assert_eq!(world.drained_day_trades[0].price, 900);
    assert_eq!(world.public_views()[0].daily_upper_limit, 990);
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report.worker_results.iter().all(|(_, result)| result.is_ok()));
    assert_eq!(report.desk_finals[0].market.candles.len(), 1);
    assert_eq!(report.desk_finals[0].market.candles[0].volume, 200);
    assert!(report.desk_finals[1].market.candles.is_empty());
}
