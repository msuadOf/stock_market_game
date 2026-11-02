//! V6/V7：真实买入与同日卖出限制、两日单所休市、复市自身日龄与分钟。
//! 人工日历差异仅用于测试，不代表官方未来安排。

use super::calendar_cancel_all_tests::mixed_spec;
use super::group::{PlaceSpec, TickPlan};
use super::protocol::DayEndCalendar;
use super::world::World;
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar,
};
use crate::plans::{
    PlanBook, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, TerminationReason, Urgency,
};
use crate::{AccountId, Side, StockId};

fn end_market(world: &mut World) {
    let last = world.committed_calendar_tick().unwrap().unwrap().calendar;
    world
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(last))
        .unwrap();
}

#[test]
fn real_buy_survives_two_closed_sse_dates_and_sells_on_own_next_session() {
    let (mut spec, _) = mixed_spec(Box::new(|group| {
        Box::new(move |tick, round| {
            let mut plan = TickPlan::default();
            if round == 0 {
                let side = match (tick, group) {
                    (4, 0) | (40, 1) => Some(Side::Buy),
                    (4, 1) | (5, 0) | (16, 0) | (28, 0) | (40, 0) => Some(Side::Sell),
                    _ => None,
                };
                if let Some(side) = side {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 1,
                        side,
                        price: 1000,
                        qty: 100,
                    });
                }
            }
            plan
        })
    }));
    spec.initial_shares[0][0][1] = 0;
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            account: AccountId(0),
            code: StockId(1),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8000,
            urgency: Urgency::Normal,
            horizon_trading_days: 2,
            created_trading_day: 0,
        })
        .unwrap();
    spec.initial_plan_books =
        super::plan_ownership::split(book, &[AccountId(0), AccountId(1), AccountId(2)], 2, 2)
            .unwrap();
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    policy.official_coverage.push(OfficialCoverageEntry::new(
        CalendarExchange::Sse,
        2030,
        vec![(
            CivilDate::from_iso("2030-01-03").unwrap(),
            CivilDate::from_iso("2030-01-04").unwrap(),
        )],
        "synthetic-test-c2c-real-reopen".into(),
        "synthetic-test-c2c-real-reopen-source".into(),
    ));
    policy
        .official_coverage
        .sort_by_key(|entry| (entry.exchange, entry.year));
    let calendar =
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap();
    let mut world = World::spawn(spec, calendar).unwrap();

    world.run_ticks(4).unwrap();
    assert_eq!(world.report_log.last().unwrap().desk_trades.len(), 1);
    let after_buy = world.snapshot_accounts().unwrap();
    assert_eq!(
        (
            after_buy[0].1[0].shares[1],
            after_buy[0].1[0].frozen_today[1]
        ),
        (100, 100),
        "零初始持股账户的真实买入当天全部冻结"
    );
    let serials = world.collect_desk_resting().unwrap().1;
    world.run_ticks(1).unwrap();
    let same_day = world.report_log.last().unwrap();
    assert!(same_day.desk_trades.is_empty());
    assert!(same_day.desk_receipts.is_empty());
    assert_eq!(
        same_day
            .group_rejections
            .iter()
            .map(|r| (r.owner, r.order, r.kind))
            .collect::<Vec<_>>(),
        vec![((0, 0), (1, 0), super::ReceiptKind::InsufficientShares)],
        "必须显式命中T+1可卖不足，不能用其它拒绝或未发送冒充"
    );
    assert_eq!(
        world.collect_desk_resting().unwrap().1,
        serials,
        "同日不可卖的100股不能进入desk受理"
    );
    let after_same_day_sell = world.snapshot_accounts().unwrap();
    assert_eq!(
        (
            after_same_day_sell[0].1[0].shares[1],
            after_same_day_sell[0].1[0].frozen_today[1]
        ),
        (100, 100)
    );
    world.run_ticks(7).unwrap();
    end_market(&mut world);
    let after_first_day = world.snapshot_accounts().unwrap();
    assert_eq!(
        (
            after_first_day[0].1[0].shares[1],
            after_first_day[0].1[0].frozen_today[1]
        ),
        (100, 0),
        "日终T+1转换为之后的本股交易日可卖；closed请求仍须另行拒绝"
    );
    let closed_books = world.collect_plans().unwrap();
    assert_eq!(
        closed_books[0].1.plan(id).unwrap().status(),
        PlanStatus::Active
    );
    for (date, target) in [("2030-01-03", 16), ("2030-01-04", 28)] {
        world.set_civil_date(CivilDate::from_iso(date).unwrap());
        world.run_ticks(4).unwrap();
        let record = world.report_log.last().unwrap();
        assert_eq!(record.tick, target);
        assert_eq!(
            record
                .group_rejections
                .iter()
                .map(|r| (r.owner, r.order, r.kind))
                .collect::<Vec<_>>(),
            vec![((0, 0), (1, 0), super::ReceiptKind::ExchangeClosed)]
        );
        assert!(record.desk_receipts.is_empty());
        assert!(record.desk_trades.is_empty());
        world.run_ticks(8).unwrap();
        end_market(&mut world);
        assert_eq!(
            world.snapshot_accounts().unwrap(),
            after_first_day,
            "单所closed不能改变真实账户资源"
        );
        assert_eq!(
            world.collect_plans().unwrap(),
            closed_books,
            "完整PlanBook不耗他所交易日龄或身份"
        );
        let closed = &world.publication().load().desks[1].view;
        assert_eq!(closed.daily_bars.len(), 1, "两日closed均不得新增当日终K");
        assert!(closed.minute_bars.is_empty());
        assert_eq!(closed.volume_today, 0);
    }

    world.set_civil_date(CivilDate::from_iso("2030-01-07").unwrap());
    let first = world.prepare_raw_calendar_tick().unwrap();
    assert_eq!(
        first
            .before_projection()
            .stock(StockId(1))
            .unwrap()
            .date_session_ordinal(),
        Some(1)
    );
    assert_eq!(
        first
            .before_projection()
            .stock(StockId(0))
            .unwrap()
            .date_session_ordinal(),
        Some(3)
    );
    world.run_ticks(4).unwrap();
    let committed = world.committed_calendar_tick().unwrap().unwrap();
    assert_eq!(committed.source.tick, 40);
    assert_eq!(
        committed
            .calendar
            .after_projection()
            .stock(StockId(1))
            .unwrap()
            .date_session_ordinal(),
        Some(1)
    );
    assert_eq!(
        committed
            .calendar
            .after_projection()
            .stock(StockId(0))
            .unwrap()
            .date_session_ordinal(),
        Some(3)
    );
    let record = world.report_log.last().unwrap();
    assert_eq!(record.tick, 40);
    assert_eq!(
        record.desk_trades.len(),
        1,
        "复市真实卖腿与另一账户真实买腿成交"
    );
    assert_eq!(record.desk_trades[0].stock, 1);
    assert_eq!(record.desk_trades[0].qty, 100);
    let reopened = world.publication().load();
    assert_eq!(
        reopened.desks[1]
            .view
            .minute_closes
            .iter()
            .map(|(minute, _)| *minute)
            .collect::<Vec<_>>(),
        (240..280).collect::<Vec<_>>(),
        "SSE真实发布绝对分钟从自身第1会话起步"
    );
    assert_eq!(
        reopened.desks[0]
            .view
            .minute_closes
            .iter()
            .map(|(minute, _)| *minute)
            .collect::<Vec<_>>(),
        (720..760).collect::<Vec<_>>(),
        "SZSE真实发布绝对分钟从自身第3会话起步"
    );
    assert_eq!(reopened.desks[1].view.minute_bars.len(), 1);
    assert_eq!(
        reopened.desks[1].view.minute_bars[0].minute_of_day, 570,
        "本股复市首个连续竞价分钟从09:30开始，不沿用他所closed期间的进度"
    );
    assert_eq!(world.snapshot_accounts().unwrap()[0].1[0].shares[1], 0);
    world.run_ticks(8).unwrap();
    end_market(&mut world);
    assert_eq!(
        world.collect_plans().unwrap()[0]
            .1
            .plan(id)
            .unwrap()
            .status(),
        PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired
        },
        "horizon2只在本股第1会话结束到期"
    );
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}
