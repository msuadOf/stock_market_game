//! V8/V9：真实成交、末拍 Continue 与 mixed/reopen 的实际 Drain 生命周期。
//! 区分可重试的 Drain 校验失败与 terminal 的真实通道断连。

use super::calendar_cancel_all_tests::mixed_spec;
use super::group::{PlaceSpec, TickPlan};
use super::protocol::{DayEndCalendar, MinuteBarFact, RuntimeError};
use super::world::{ShutdownReport, World};
use crate::calendar::CivilDate;
use crate::{Side, StockId};
use std::sync::Arc;

fn market_world(reopen: bool) -> World {
    let base = if reopen { 12 } else { 0 };
    let stock = if reopen { 1 } else { 0 };
    let (mut spec, calendar) = mixed_spec(Box::new(move |group| {
        Box::new(move |tick, round| {
            let mut plan = TickPlan::default();
            if round == 0 && (tick == base + 4 || tick == base + 11) {
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock,
                    side: if group == 0 { Side::Buy } else { Side::Sell },
                    price: 1000,
                    qty: 100,
                });
            }
            plan.continuation = group == 0 && tick == base + 12 && round == 0;
            plan
        })
    }));
    if reopen {
        spec.civil_start_date = CivilDate::from_iso("2030-01-03").unwrap();
        spec.initial_committed_tick = 12;
        spec.initial_last_day_end_tick = Some(12);
    }
    let mut world = World::spawn(spec, calendar).unwrap();
    world
        .run_ticks(12)
        .expect("真实日内成交与末拍 Continue 成功");
    assert_eq!(world.report_log.last().unwrap().rounds, 1);
    let real = world
        .report_log
        .iter()
        .flat_map(|record| &record.desk_trades)
        .collect::<Vec<_>>();
    assert_eq!(real.len(), 1, "日内一笔已成交，收盘批仍待真实 Drain 清算");
    assert_eq!(real[0].stock, stock);
    let current = world.committed_calendar_tick().unwrap().unwrap();
    let projected = current.calendar.after_projection();
    assert_eq!(
        projected.stock(StockId(0)).unwrap().date_session_ordinal(),
        Some(if reopen { 1 } else { 0 })
    );
    assert_eq!(
        projected.stock(StockId(1)).unwrap().date_session_ordinal(),
        if reopen { Some(0) } else { None }
    );
    world
}

fn end_market(world: &mut World) {
    let committed = world.committed_calendar_tick().unwrap().unwrap();
    let tick = Arc::clone(&committed.calendar);
    let again = world.committed_calendar_tick().unwrap().unwrap();
    assert!(Arc::ptr_eq(&tick, &again.calendar));
    world
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(tick))
        .unwrap();
}

fn totals(bars: &[MinuteBarFact]) -> (u64, u128, u64) {
    bars.iter().fold((0, 0, 0), |(volume, amount, count), bar| {
        (
            volume + bar.volume_shares,
            amount + bar.turnover_cents,
            count + bar.trade_count,
        )
    })
}

fn check_final(report: &ShutdownReport, reopen: bool) {
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    assert_eq!(report.group_finals.len(), 2);
    assert_eq!(report.desk_finals.len(), 2);
    assert!(report
        .group_finals
        .iter()
        .flat_map(|group| &group.accounts)
        .all(|account| account.buy_purse_held_cents == 0
            && account.pending_sell_qty.iter().all(|&qty| qty == 0)));
    for desk in &report.desk_finals {
        assert!(desk.resting.is_empty());
        let traded = usize::from(desk.stock) == usize::from(reopen);
        if traded {
            assert_eq!(desk.market.candles.len(), 1);
            let candle = &desk.market.candles[0];
            assert_eq!(
                (
                    candle.open,
                    candle.high,
                    candle.low,
                    candle.close,
                    candle.volume
                ),
                (1000, 1000, 1000, 1000, 200)
            );
        } else if !reopen {
            assert!(desk.market.candles.is_empty(), "closed 不产生伪终 K");
            assert!(desk.market.minute_closes.is_empty());
            assert!(desk.market.minute_bars.is_empty());
            assert!(desk.market.recent_prices.is_empty());
            assert_eq!(desk.market.traded_days_total, 0);
        } else {
            assert_eq!(desk.market.candles.len(), 1, "open 零成交仍有真实终 K");
            assert_eq!(desk.market.candles[0].volume, 0);
        }
    }
}

fn unconsumed(reopen: bool) {
    let mut world = market_world(reopen);
    end_market(&mut world);
    assert_eq!(
        world.drained_day_trades.len(),
        1,
        "Drain 产生一笔真实收盘成交"
    );
    let report = world
        .shutdown()
        .expect("未消费桶必须复用，不再次空 Drain 覆盖");
    check_final(&report, reopen);
    assert_eq!(
        totals(
            report.drained_minute_bars[usize::from(reopen)]
                .as_ref()
                .unwrap()
        ),
        (200, 200_000, 2)
    );
    if !reopen {
        assert!(report.drained_minute_bars[1].as_ref().unwrap().is_empty());
    }
}

#[test]
fn mixed_market_day_end_shutdown_preserves_unconsumed_real_trade_bars() {
    unconsumed(false);
}

#[test]
fn reopen_market_day_end_shutdown_preserves_unconsumed_real_trade_bars() {
    unconsumed(true);
}

#[test]
fn mixed_market_day_end_consumed_bars_are_not_replayed_by_shutdown() {
    let mut world = market_world(false);
    end_market(&mut world);
    let bars = world.take_pending_day_bars();
    assert_eq!(totals(bars[0].as_ref().unwrap()), (200, 200_000, 2));
    assert!(bars[1].as_ref().unwrap().is_empty());
    let candles = world.take_pending_day_candles();
    let candle = candles[0].as_ref().unwrap();
    assert_eq!(
        (candle.volume, candle.turnover_cents, candle.trade_count),
        (200, 200_000, 2)
    );
    assert!(candles[1].is_none());
    let report = world.shutdown().unwrap();
    check_final(&report, false);
    assert!(
        report.drained_minute_bars.iter().all(Option::is_none),
        "已消费事实不得重发"
    );
}

#[test]
fn mixed_market_day_end_abort_shutdown_uses_real_drain_again() {
    let mut world = market_world(false);
    let accounts = world.snapshot_accounts().unwrap();
    let books = world.collect_plans().unwrap();
    let resting = world.collect_desk_resting().unwrap();
    let before = world.publication().load();
    end_market(&mut world);
    world.abort_day_end();
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    assert_eq!(world.collect_plans().unwrap(), books);
    assert_eq!(world.collect_desk_resting().unwrap(), resting);
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert!(world.take_pending_day_bars().iter().all(Option::is_none));
    assert!(world.take_pending_day_candles().iter().all(Option::is_none));
    let report = world
        .shutdown()
        .expect("Abort 后真实 Drain 不能沿用已作废空槽");
    check_final(&report, false);
    assert_eq!(
        totals(report.drained_minute_bars[0].as_ref().unwrap()),
        (200, 200_000, 2)
    );
    assert!(report.drained_minute_bars[1].as_ref().unwrap().is_empty());
}

fn partial_day_end_failure(fail_after_drain: bool) {
    let mut world = market_world(false);
    let accounts = world.snapshot_accounts().unwrap();
    let books = world.collect_plans().unwrap();
    let resting = world.collect_desk_resting().unwrap();
    let candles = world.take_candle_projection().unwrap();
    let before = world.publication().load();
    if fail_after_drain {
        world.fail_day_end_after_drain_reports_for_test(1);
    } else {
        world.fail_day_end_before_group_for_test(1);
    }
    let tick = world.committed_calendar_tick().unwrap().unwrap().calendar;
    let error = world
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(Arc::clone(&tick)))
        .expect_err("部分参与者后显式故障");
    if fail_after_drain {
        assert!(matches!(&error, RuntimeError::ProtocolInvariant { detail }
            if detail == "【测试注入】收到 1 个真实 DeskDrained 后收集失败"));
    } else {
        assert!(matches!(&error, RuntimeError::SendFailed { detail }
            if detail == "【测试注入】第 1 组真实断连 Sender 无法发送 DayEnd"));
    }
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    assert_eq!(world.collect_plans().unwrap(), books);
    assert_eq!(world.collect_desk_resting().unwrap(), resting);
    assert_eq!(world.take_candle_projection().unwrap(), candles);
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert!(world.take_pending_day_bars().iter().all(Option::is_none));
    assert!(world.take_pending_day_candles().iter().all(Option::is_none));
    assert_eq!(
        world.live_workers_for_test(),
        4,
        "真实 AbortDayEnd 确认后原四worker仍受World拥有"
    );
    if fail_after_drain {
        end_market(&mut world);
    } else {
        let retry = world
            .run_day_end_with_calendar(DayEndCalendar::MarketTick(tick))
            .expect_err("发送故障即terminal，不能把原typed日结当作可重试");
        assert_eq!(
            retry,
            RuntimeError::ProtocolInvariant {
                detail: format!(
                    "world 已死亡，不可继续日结：日结参与者断连，不可继续运行：{error}"
                ),
            }
        );
        assert!(
            matches!(world.run_ticks(0), Err(RuntimeError::ProtocolInvariant { detail })
            if detail.starts_with("world 已死亡，不可继续跑 tick：日结参与者断连，不可继续运行："))
        );
        assert!(Arc::ptr_eq(&before, &world.publication().load()));
    }
    let report = world.shutdown().unwrap();
    check_final(&report, false);
    assert_eq!(
        totals(report.drained_minute_bars[0].as_ref().unwrap()),
        (200, 200_000, 2)
    );
}

#[test]
fn mixed_partial_drain_failure_confirms_abort_and_retries_once() {
    partial_day_end_failure(true);
}

#[test]
fn mixed_partial_group_day_end_send_failure_is_terminal_and_still_joins() {
    partial_day_end_failure(false);
}

#[test]
fn reopen_partial_desk_start_preserves_prior_day_end_and_joins_without_replaying_drain() {
    let mut world = market_world(false);
    end_market(&mut world);
    let accounts = world.snapshot_accounts().unwrap();
    let books = world.collect_plans().unwrap();
    let next_views = world.public_views().to_vec();
    let before = world.publication().load();
    assert_eq!(before.tick, 12);
    world.set_civil_date(CivilDate::from_iso("2030-01-03").unwrap());
    let (disconnected, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let healthy = world.swap_desk_sender_for_test(1, disconnected);
    let result = world.run_ticks(1);
    world.swap_desk_sender_for_test(1, healthy);
    let error = result.expect_err("复市第一股真实Start已发送后，第二股真实send断连");
    assert!(matches!(error, RuntimeError::SendFailed { ref detail }
        if detail.contains("DeskMsg::Start(tick=13) 广播失败（部已退出）")
            && detail.contains("已 join 4 个真实 worker")));
    assert_eq!(world.live_workers_for_test(), 0);
    assert_eq!(world.global.committed_tick, 12);
    assert_eq!(world.report_log.len(), 12);
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    world.abort_day_end();
    assert_eq!(
        world.public_views(),
        next_views.as_slice(),
        "首个Start已经关闭旧日结撤销窗口"
    );
    assert!(
        matches!(world.run_ticks(0), Err(RuntimeError::ProtocolInvariant { detail })
        if detail.contains("world 已死亡"))
    );
    let report = world
        .shutdown()
        .expect("取真实失败Final，不能重跑已消费或旧日结Drain");
    check_final(&report, false);
    for group in &report.group_finals {
        assert_eq!(
            group.accounts,
            accounts[usize::from(group.group)].1,
            "未收到新Start的组保留上日成功日结资源，不倒放已发布拍"
        );
        assert_eq!(
            group.plan_book_for_test,
            books[usize::from(group.group)].1,
            "完整PlanBook来自实际Final，不能用旧发布镜像自证"
        );
    }
    assert_eq!(report.desk_finals[0].cur_tick_for_test, 13);
    assert_eq!(report.desk_finals[1].cur_tick_for_test, 12);
    assert_eq!(
        totals(report.drained_minute_bars[0].as_ref().unwrap()),
        (200, 200_000, 2),
        "部分Start失败不能将原未消费真实日结桶覆盖为空"
    );
    assert!(report.drained_minute_bars[1].as_ref().unwrap().is_empty());
}
