//! V7：通过真实 committed TickArc 与 typed DayEnd 验证逐股 T+1 与计划日龄。
//! 使用真实 caller；人工 frozen sentinel 不冒充公共存档恢复。

use super::calendar_cancel_all_tests::mixed_spec;
use super::group::TickPlan;
use super::protocol::DayEndCalendar;
use super::world::World;
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar,
};
use crate::plans::{
    PlanBook, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, TerminationReason, Urgency,
};
use crate::{AccountId, Side, StockId};
use std::sync::Arc;

fn plans(closed: StockId, horizon: u32) -> PlanBook {
    let mut book = PlanBook::default();
    for (account, code) in [
        (AccountId(0), closed),
        (AccountId(2), StockId(1 - closed.0)),
    ] {
        book.create(PlanOpen {
            account,
            code,
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8000,
            urgency: Urgency::Normal,
            horizon_trading_days: horizon,
            created_trading_day: 0,
        })
        .unwrap();
    }
    book
}

fn calendar(closed: CalendarExchange) -> TradingCalendar {
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    // 2030 所间差异仅为 synthetic-test，不表示官方日历。
    policy.official_coverage.push(OfficialCoverageEntry::new(
        closed,
        2030,
        vec![(
            CivilDate::from_iso("2030-01-01").unwrap(),
            CivilDate::from_iso("2030-01-02").unwrap(),
        )],
        "synthetic-test-c2c-v7-group-day-end".into(),
        "synthetic-test-c2c-v7-group-day-end-source".into(),
    ));
    TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap()
}

fn closed_day_end(closed: StockId, exchange: CalendarExchange) {
    let (mut spec, _) = mixed_spec(Box::new(|_| Box::new(|_, _| TickPlan::default())));
    let book = plans(closed, 1);
    let ids = book.plan_ids().collect::<Vec<_>>();
    spec.initial_plan_books =
        super::plan_ownership::split(book, &[AccountId(0), AccountId(1), AccountId(2)], 2, 2)
            .unwrap();
    spec.initial_frozen_today = vec![
        vec![vec![100, 200], vec![0, 0]],
        vec![vec![0, 0], vec![0, 0]],
    ];
    let mut world = World::spawn(spec, calendar(exchange)).unwrap();
    world.run_ticks(12).unwrap();
    let committed = world
        .committed_calendar_tick()
        .unwrap()
        .expect("真实末拍提交后必须保留原 TickArc");
    let last = Arc::clone(&committed.calendar);
    let again = world.committed_calendar_tick().unwrap().unwrap();
    assert!(
        Arc::ptr_eq(&last, &again.calendar),
        "getter 保持真实已提交原件，不从 preview 重建"
    );
    assert_eq!(committed.source.tick, world.global.committed_tick);
    assert_eq!(
        last.after_input().progress,
        crate::session::exchange_calendar_model::SharedDayProgress::TradingCompleted
    );
    assert!(last
        .after_projection()
        .stock(closed)
        .unwrap()
        .date_session_ordinal()
        .is_none());
    assert_eq!(
        last.after_projection()
            .stock(StockId(1 - closed.0))
            .unwrap()
            .date_session_ordinal(),
        Some(0)
    );
    let accounts = world.snapshot_accounts().unwrap();
    assert_eq!(
        accounts[0].1[0].frozen_today.as_slice(),
        &[100, 200],
        "raw 人工 frozen sentinel 必须真实装入且在日终前保留，排除 0→0 假阳性"
    );
    let before = world.collect_plans().unwrap();
    world
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(Arc::clone(&last)))
        .expect("真实已提交末拍原件通过市场 typed 日终强门");
    let after_accounts = world.snapshot_accounts().unwrap();
    assert_eq!(
        after_accounts[0].1[0].frozen_today[closed.0 as usize],
        accounts[0].1[0].frozen_today[closed.0 as usize],
        "closed 本股冻结不能被另一所日结清零"
    );
    assert_eq!(
        after_accounts[0].1[0].frozen_today[(1 - closed.0) as usize],
        0,
        "open 本股正常解锁，正控排除整批跳过"
    );
    let after = world.collect_plans().unwrap();
    assert_eq!(
        after[0].1, before[0].1,
        "closed 所属组完整 PlanBook、policy 与 owner 游标保持"
    );
    assert_eq!(
        after[1].1.plan(ids[1]).unwrap().status(),
        PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired
        },
        "open horizon=1 的真实本股首会话正常结束"
    );
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn group_day_end_preserves_closed_sse_frozen_and_plan() {
    closed_day_end(StockId(1), CalendarExchange::Sse);
}

#[test]
fn group_day_end_preserves_closed_szse_frozen_and_plan() {
    closed_day_end(StockId(0), CalendarExchange::Szse);
}

#[test]
fn reopen_group_day_end_ages_plans_by_own_session_ordinal() {
    let (mut spec, _) = mixed_spec(Box::new(|_| Box::new(|_, _| TickPlan::default())));
    let book = plans(StockId(1), 2);
    let ids = book.plan_ids().collect::<Vec<_>>();
    spec.initial_plan_books =
        super::plan_ownership::split(book, &[AccountId(0), AccountId(1), AccountId(2)], 2, 2)
            .unwrap();
    spec.civil_start_date = CivilDate::from_iso("2030-01-03").unwrap();
    spec.initial_committed_tick = 12;
    spec.initial_last_day_end_tick = Some(12);
    let mut world = World::spawn(spec, calendar(CalendarExchange::Sse)).unwrap();
    world.run_ticks(12).unwrap();
    let committed = world
        .committed_calendar_tick()
        .unwrap()
        .expect("真实末拍提交后必须保留原 TickArc");
    let last = Arc::clone(&committed.calendar);
    let again = world.committed_calendar_tick().unwrap().unwrap();
    assert!(
        Arc::ptr_eq(&last, &again.calendar),
        "getter 保持真实已提交原件，不从 preview 重建"
    );
    assert_eq!(committed.source.tick, world.global.committed_tick);
    assert_eq!(last.after_input().tick, 24);
    assert_eq!(
        last.after_projection()
            .stock(StockId(1))
            .unwrap()
            .date_session_ordinal(),
        Some(0)
    );
    assert_eq!(
        last.after_projection()
            .stock(StockId(0))
            .unwrap()
            .date_session_ordinal(),
        Some(1)
    );
    world
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(last))
        .unwrap();
    let after = world.collect_plans().unwrap();
    assert_eq!(
        after[0].1.plan(ids[0]).unwrap().status(),
        PlanStatus::Active,
        "复市SSE仅完成自身第0会话，不耗另一所已发生的一天horizon"
    );
    assert_eq!(
        after[1].1.plan(ids[1]).unwrap().status(),
        PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired
        },
        "SZSE已完成自身第1会话，horizon=2按自身日龄到期"
    );
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn mixed_nonzero_open_plan_and_t1_day_end_changes_abort_and_retry_restore_real_group_authority() {
    for (closed, exchange) in [(StockId(1), CalendarExchange::Sse), (StockId(0), CalendarExchange::Szse)] {
        let open = StockId(1 - closed.0);
        let (mut spec, _) = mixed_spec(Box::new(|_| Box::new(|_, _| TickPlan::default())));
        let book = plans(closed, 1);let ids = book.plan_ids().collect::<Vec<_>>();
        let closed_id = ids[0];let open_id = ids[1];
        spec.initial_plan_books = super::plan_ownership::split(book, &[AccountId(0), AccountId(1), AccountId(2)], 2, 2).unwrap();
        spec.initial_frozen_today = vec![vec![vec![100, 200], vec![0, 0]], vec![vec![0, 0], vec![0, 0]]];
        let mut world = World::spawn(spec, calendar(exchange)).unwrap();world.run_ticks(12).unwrap();
        let tick = world.committed_calendar_tick().unwrap().unwrap().calendar;
        let before_accounts = world.snapshot_accounts().unwrap();let before_books = world.collect_plans().unwrap();
        let before_resting = world.collect_desk_resting().unwrap();let before_public = world.publication().load();
        assert_eq!(before_accounts[0].1[0].frozen_today.as_slice(), &[100, 200], "双股非零sentinel必须真实保到DayEnd前");
        assert_eq!(before_books[0].1.plan(closed_id).unwrap().status(), PlanStatus::Active);
        assert_eq!(before_books[1].1.plan(open_id).unwrap().status(), PlanStatus::Active);
        assert_eq!(before_accounts[0].1[0].frozen_today[open.0 as usize], if open == StockId(0) { 100 } else { 200 });
        world.run_day_end_with_calendar(DayEndCalendar::MarketTick(Arc::clone(&tick))).unwrap();
        let ended_accounts = world.snapshot_accounts().unwrap();let ended_books = world.collect_plans().unwrap();
        assert_eq!(ended_accounts[0].1[0].frozen_today[open.0 as usize], 0, "open非零T+1必须真实先解锁");
        assert_eq!(ended_accounts[0].1[0].frozen_today[closed.0 as usize], before_accounts[0].1[0].frozen_today[closed.0 as usize]);
        assert_eq!(ended_books[1].1.plan(open_id).unwrap().status(), PlanStatus::Terminated { reason: TerminationReason::HorizonExpired });
        assert_eq!(ended_books[0].1, before_books[0].1, "closed计划完整不变");
        assert_ne!(ended_books[1].1, before_books[1].1, "必须真实发生计划迁移，不能用全空Book证明Abort");
        world.abort_day_end();
        assert_eq!(world.snapshot_accounts().unwrap(), before_accounts);
        assert_eq!(world.collect_plans().unwrap(), before_books, "真实完整组PlanBook含execution/终结/owner游标全部恢复");
        assert_eq!(world.collect_desk_resting().unwrap(), before_resting);
        assert!(Arc::ptr_eq(&before_public, &world.publication().load()));
        assert!(world.take_pending_day_bars().iter().all(Option::is_none));
        assert!(world.take_pending_day_candles().iter().all(Option::is_none));
        // 同一真实committed TickArc合法重试；不能伪造preview或偷偷跑下个Start退役undo。
        world.run_day_end_with_calendar(DayEndCalendar::MarketTick(tick)).unwrap();
        assert_eq!(world.snapshot_accounts().unwrap(), ended_accounts);
        assert_eq!(world.collect_plans().unwrap(), ended_books);
        let report = world.shutdown().unwrap();assert_eq!(report.worker_results.len(), 4);
        assert!(report.worker_results.iter().all(|(_, result)| result.is_ok()));
    }
}
