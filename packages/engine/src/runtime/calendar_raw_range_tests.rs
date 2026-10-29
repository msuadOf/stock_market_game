//! V2 真实 raw caller 边界：范围预检、全休市静点与末拍；不据公开投影推断全部私有身份。

use super::group::{Observed, PlaceSpec, TickPlan};
use super::protocol::RuntimeError;
use super::skeleton_tests::basic_spec;
use super::world::{ShutdownReport, World};
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, DayStatus, OfficialCoverageEntry, TradingCalendar,
};
use crate::session::exchange_calendar_model::SharedDayProgress;
use std::sync::Arc;

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).expect("fixture 日期合法")
}

fn assert_joined(report: &ShutdownReport, groups: usize, stocks: usize) {
    assert_eq!(report.group_finals.len(), groups);
    assert_eq!(report.desk_finals.len(), stocks);
    assert_eq!(report.worker_results.len(), groups + stocks);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn raw_range_rejects_whole_batch_and_keeps_live_resources_before_next_start() {
    let mut spec = basic_spec(
        2,
        2,
        2,
        Box::new(|group| {
            Box::new(move |tick, _| {
                let mut plan = TickPlan::default();
                if group == 0 && tick == 1 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: crate::Side::Buy,
                        price: 1000,
                        qty: 100,
                    });
                }
                plan
            })
        }),
    );
    spec.ticks_per_day = 2;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    spec.stocks[1].exchange = crate::StockExchange::Shenzhen;
    let mut world =
        World::spawn(spec, TradingCalendar::current_default_calendar().unwrap()).unwrap();
    world.run_ticks(1).expect("真实首拍挂出买单");
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    assert!(
        accounts[0].1[0].buy_purse_held_cents > 0,
        "正控须有真实买腿 purse"
    );
    assert_eq!(before.desks[0].resting.len(), 1, "正控须有真实在途证券委托");
    assert!(before.desks[0].resting[0].purse > 0);
    let serial = before.desks[0].next_serial;
    let error = world.run_ticks(2).expect_err("余一拍时整段两拍必须拒绝");
    assert!(matches!(error, RuntimeError::ProtocolInvariant { detail }
        if detail.contains("TickRangeCrossesCivilDay: n=2, remaining=1")));
    assert_eq!(world.global.committed_tick, 1);
    assert_eq!(world.report_log.len(), 1);
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    let rejected = world.publication().load();
    assert_eq!(rejected.desks[0].next_serial, serial, "公开序号镜像不变");
    assert_eq!(rejected.desks[0].resting, before.desks[0].resting);
    world.run_ticks(1).expect("拒绝整段后剩余一拍仍可真实提交");
    assert_eq!(world.global.committed_tick, 2);
    assert_eq!(world.report_log.len(), 2);
    let completed = world.publication().load();
    let completed_accounts = world.snapshot_accounts().unwrap();
    assert_eq!(world.run_ticks(0).unwrap(), 2, "末拍完成后的零拍读点合法");
    assert!(Arc::ptr_eq(&completed, &world.publication().load()));
    let error = world.run_ticks(1).expect_err("同一日期不能重复市场拍");
    assert!(matches!(error, RuntimeError::ProtocolInvariant { detail }
        if detail.contains("TickRangeCrossesCivilDay: n=1, remaining=0")));
    assert_eq!(world.global.committed_tick, 2);
    assert_eq!(world.report_log.len(), 2);
    assert!(Arc::ptr_eq(&completed, &world.publication().load()));
    assert_eq!(world.snapshot_accounts().unwrap(), completed_accounts);
    let report = world.shutdown().unwrap();
    assert_joined(&report, 2, 2);
    // 真实组通道日志只证明各组 Start；不把它冒称为全部 desk 私有消息计数。
    for group in &report.group_finals {
        let starts: Vec<_> = group
            .observed
            .iter()
            .filter_map(|item| match item {
                Observed::Start(tick) => Some(*tick),
                _ => None,
            })
            .collect();
        assert_eq!(starts, vec![1, 2]);
    }
}

#[test]
fn all_closed_zero_ticks_keep_initial_resources_and_shutdown_without_market_facts() {
    let first = date("2030-01-02");
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    // 双所2030休市仅为 synthetic-test，不预测或冒称交易所官方公告。
    policy.official_coverage = [CalendarExchange::Sse, CalendarExchange::Szse]
        .into_iter()
        .map(|exchange| {
            OfficialCoverageEntry::new(
                exchange,
                2030,
                vec![(date("2030-01-01"), first)],
                "synthetic-test-v2-all-closed".into(),
                "synthetic-test-v2-source".into(),
            )
        })
        .collect();
    let calendar =
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap();
    for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        assert!(matches!(
            calendar.day_status(exchange, first).unwrap(),
            DayStatus::Closed(_)
        ));
    }
    let mut spec = basic_spec(2, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.civil_start_date = first;
    spec.calendar_start_date = first;
    spec.ticks_per_day = 2;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    spec.stocks[1].exchange = crate::StockExchange::Shenzhen;
    let mut world = World::spawn(spec, calendar).unwrap();
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    assert_eq!(world.run_ticks(0).unwrap(), 0);
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    let error = world.run_ticks(1).expect_err("全休市不得造市场拍");
    assert!(matches!(error, RuntimeError::ProtocolInvariant { detail }
        if detail.contains("TickRangeCrossesCivilDay: n=1, remaining=0")));
    assert_eq!(world.global.committed_tick, 0);
    assert!(world.report_log.is_empty());
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    let report = world.shutdown().expect("全休市无市场拍的静点仍须完整关停");
    assert_joined(&report, 2, 2);
    for group in &report.group_finals {
        assert_eq!(group.accounts, accounts[usize::from(group.group)].1);
        assert!(!group.observed.iter().any(|item| matches!(
            item,
            Observed::Start(_) | Observed::Continue(_, _) | Observed::DayEnd(_)
        )));
    }
    for desk in &report.desk_finals {
        assert_eq!(desk.cur_tick_for_test, 0);
        assert!(desk.resting.is_empty());
        assert_eq!(desk.market.last_price, 1000);
        assert_eq!(desk.market.last_close, 1000);
        assert_eq!(desk.market.volume_today, 0);
        assert_eq!(desk.market.turnover_today, 0);
        assert_eq!(desk.market.trade_count, 0);
        assert_eq!(desk.fee_sink, 0);
        assert!(desk.market.candle.is_none());
        assert!(desk.market.candles.is_empty());
        assert!(desk.market.minute_closes.is_empty());
        assert!(desk.market.minute_bars.is_empty());
        assert!(desk.market.recent_prices.is_empty());
    }
}

#[test]
fn zero_ticks_still_validate_calendar_date_and_tick_base() {
    let mut spec = basic_spec(1, 1, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 2;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    let first = spec.civil_start_date;
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let next = calendar
        .next_trading_day(CalendarExchange::Sse, first)
        .unwrap();
    let mut world = World::spawn(spec, calendar).unwrap();
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    world.set_civil_date(next);
    let error = world
        .run_ticks(0)
        .expect_err("零拍也不能绕过日期与tick基线不一致");
    assert!(matches!(error, RuntimeError::ProtocolInvariant { detail }
        if detail == "raw tick 早于当前 civil date"));
    assert_eq!(world.global.committed_tick, 0);
    assert!(world.report_log.is_empty());
    assert!(Arc::ptr_eq(&before, &world.publication().load()));
    assert_eq!(world.snapshot_accounts().unwrap(), accounts);
    world.set_civil_date(first);
    let report = world.shutdown().unwrap();
    assert_joined(&report, 1, 1);
    assert!(!report.group_finals[0]
        .observed
        .iter()
        .any(|item| matches!(item, Observed::Start(_))));
}

#[test]
fn final_tick_preview_and_real_minutes_keep_the_ended_civil_date() {
    let mut spec = basic_spec(1, 1, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 12;
    spec.auction_ticks = 3;
    spec.closing_auction_ticks = 3;
    let first = spec.civil_start_date;
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let next = calendar
        .next_trading_day(CalendarExchange::Sse, first)
        .unwrap();
    let mut world = World::spawn(spec, calendar).unwrap();
    world.run_ticks(11).unwrap();
    // preview 是纯投影证据；下面另跑真实末拍并核已发布分钟，不冒称此Arc已经交付。
    let preview = world.prepare_raw_calendar_tick().unwrap();
    assert_eq!(preview.phase_before(), crate::TradingPhase::ClosingAuction);
    assert_eq!(
        preview.after_input().progress,
        SharedDayProgress::TradingCompleted
    );
    assert_eq!(
        preview.observation_instant_after(),
        crate::calendar::CivilInstant::new(first, 15 * 3600).unwrap()
    );
    world.run_ticks(1).unwrap();
    let completed = world.publication().load();
    assert_eq!(completed.tick, 12);
    assert_eq!(completed.desks[0].view.minute_closes.len(), 240);
    assert_eq!(completed.desks[0].view.minute_closes[0].0, 0);
    assert_eq!(completed.desks[0].view.minute_closes[239].0, 239);
    let error = world.run_ticks(1).expect_err("真实末拍之后不能重复市场拍");
    assert!(matches!(error, RuntimeError::ProtocolInvariant { detail }
        if detail.contains("TickRangeCrossesCivilDay: n=1, remaining=0")));
    world.run_day_end_public().unwrap();
    world.set_civil_date(next);
    world
        .run_ticks(1)
        .expect("显式成功日终和下一日期构成真实正路径");
    assert_eq!(world.global.committed_tick, 13);
    assert_joined(&world.shutdown().unwrap(), 1, 1);
}
