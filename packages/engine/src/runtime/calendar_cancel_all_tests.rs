//! V4 raw CancelAll：真实脚本请求、四相位闭市反馈与账户/计划/证券身份不变。

use super::group::{CancelSpec, GroupScript, PlaceSpec, TickPlan};
use super::skeleton_tests::basic_spec;
use super::world::{RootProjection, World, WorldSpec};
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar,
};
use crate::plans::{
    OwnerSequence, PlanBook, PlanEvent, PlanOpen, PlanOpinion, PlanPolicy, PlanTarget,
    TerminationReason, Urgency,
};
use crate::{AccountId, GameSession, Side, StockCode, StockExchange, StockId, TradingPhase};
use std::sync::Arc;

fn plan_open(owner: AccountId, code: StockId) -> PlanOpen {
    PlanOpen {
        account: owner,
        code,
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion {
            signal_score_bp: 3000,
            source: crate::plans::OpinionSource::Blended,
        },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

pub(super) fn mixed_spec(
    script_for: Box<dyn Fn(super::GroupIdx) -> GroupScript>,
) -> (WorldSpec, TradingCalendar) {
    let first = CivilDate::from_iso("2030-01-02").unwrap();
    let mut setup = super::bridge_tests::setup_fixture();
    setup.start_date = first;
    setup.ticks_per_day = 12;
    setup.auction_ticks = 3;
    setup.closing_auction_ticks = 3;
    setup.stocks[0].float_shares = 0;
    setup.stocks[1] = setup.stocks[0].clone();
    setup.stocks[1].code = StockCode("600101".into());
    setup.stocks[1].exchange = StockExchange::Shanghai;
    let session = GameSession::new(setup, 83).expect("三真实账户目录与公司公开事实合法");
    let (catalog, initial_company_facts) = session.runtime_company_handles().unwrap();
    let mut spec = basic_spec(2, 2, 2, script_for);
    spec.ticks_per_day = 12;
    spec.auction_ticks = 3;
    spec.closing_auction_ticks = 3;
    spec.civil_start_date = first;
    spec.calendar_start_date = first;
    spec.stocks[0].exchange = StockExchange::Shenzhen;
    spec.stocks[1].exchange = StockExchange::Shanghai;
    // 三个真实 owner 与两组矩形槽分开；尾空位没有现金、股份或 PlanId 游标。
    spec.initial_cash_override = Some(vec![vec![1_000_000, 1_000_000], vec![1_000_000, 0]]);
    spec.initial_shares[1][1].fill(0);
    spec.root_projection = Some(Arc::new(RootProjection {
        catalog,
        initial_company_facts,
        account_ids: vec![AccountId(0), AccountId(1), AccountId(2)],
        auction_ticks: 3,
        closing_auction_ticks: 3,
        chain_params: vec![None; 3],
        session_seed: session.runtime_session_seed(),
        urgency_policy: session.runtime_urgency_policy(),
        max_order_qty_limit: session.runtime_max_order_qty_limit(),
        tick_size_cents: session.runtime_tick_size_cents(),
    }));
    let mut book = PlanBook::default();
    book.create(plan_open(AccountId(0), StockId(1))).unwrap();
    let terminal = book.create(plan_open(AccountId(2), StockId(0))).unwrap();
    book.apply(
        terminal,
        PlanEvent::Terminated {
            reason: TerminationReason::Cancelled,
            trading_day: 0,
        },
    )
    .unwrap();
    let (_, _, plans) = book.into_parts();
    let book = PlanBook::from_parts(
        PlanPolicy::default(),
        vec![
            OwnerSequence {
                owner: AccountId(0),
                next_sequence: Some(40),
            },
            OwnerSequence {
                owner: AccountId(1),
                next_sequence: None,
            },
            OwnerSequence {
                owner: AccountId(2),
                next_sequence: Some(90),
            },
        ],
        plans,
    )
    .unwrap();
    spec.initial_plan_books =
        super::plan_ownership::split(book, &[AccountId(0), AccountId(1), AccountId(2)], 2, 2)
            .unwrap();
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    // 2030 SSE/SZSE 差异仅为 synthetic-test，不预测官方休市日期。
    policy.official_coverage.push(OfficialCoverageEntry::new(
        CalendarExchange::Sse,
        2030,
        vec![(CivilDate::from_iso("2030-01-01").unwrap(), first)],
        "synthetic-test-c2c-raw-cancel-all".into(),
        "synthetic-test-c2c-raw-cancel-all-source".into(),
    ));
    let calendar =
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap();
    (spec, calendar)
}

fn closed_cancel_all(target: u64, phase: TradingPhase) {
    let (spec, calendar) = mixed_spec(Box::new(move |group| {
        Box::new(move |tick, round| {
            let mut plan = TickPlan::default();
            if group == 0 && tick == target && round == 0 {
                plan.cancels.push(CancelSpec { local: 0, stock: 1 });
            }
            plan
        })
    }));
    let mut world = World::spawn(spec, calendar).unwrap();
    world.run_ticks(target - 1).unwrap();
    let tick = world.prepare_raw_calendar_tick().unwrap();
    assert_eq!(tick.phase_before(), phase);
    assert!(tick
        .before_projection()
        .stock(StockId(1))
        .unwrap()
        .date_session_ordinal()
        .is_none());
    assert!(tick
        .before_projection()
        .stock(StockId(0))
        .unwrap()
        .date_session_ordinal()
        .is_some());
    let accounts = world.snapshot_accounts().unwrap();
    let books = world.collect_plans().unwrap();
    let (resting, serials) = world.collect_desk_resting().unwrap();
    assert_eq!(
        books[0].1.owner_sequences().collect::<Vec<_>>(),
        vec![
            OwnerSequence {
                owner: AccountId(0),
                next_sequence: Some(40)
            },
            OwnerSequence {
                owner: AccountId(1),
                next_sequence: None
            },
        ],
        "正控含已耗 cursor 与 None 耗尽身份，不能只比较空簿"
    );
    assert_eq!(
        books
            .iter()
            .map(|(_, book)| book.plan_ids().count())
            .sum::<usize>(),
        2
    );
    assert_eq!(accounts[1].1[1].cash, 0, "尾空位不冒充第四个真实 owner");
    world
        .run_ticks(1)
        .expect("closed CancelAll 是业务拒绝，不能变成 fatal");
    let record = world.report_log.last().unwrap();
    assert_eq!(
        record
            .group_rejections
            .iter()
            .map(|r| (r.owner, r.order, r.kind))
            .collect::<Vec<_>>(),
        vec![((0, 0), (1, 0), super::ReceiptKind::ExchangeClosed)],
        "真实 TickPlan.cancels 的 CancelAll 必须恰一次 strict ExchangeClosed"
    );
    assert!(
        record.desk_receipts.is_empty(),
        "closed 请求不得进入交易部受理或撤单"
    );
    assert!(record.desk_trades.is_empty());
    assert!(record.desk_lifecycle.is_empty());
    assert_eq!(
        world.snapshot_accounts().unwrap(),
        accounts,
        "含 foreign_active、purse 与可卖/冻结的完整账户不变"
    );
    assert_eq!(
        world.collect_plans().unwrap(),
        books,
        "完整 PlanBook/PlanId owner 游标均不变"
    );
    assert_eq!(
        world.collect_desk_resting().unwrap(),
        (resting, serials),
        "真实证券 serial 不变"
    );
    let closed = world.publication().load().desks[1].view.clone();
    // 初始 Start seed 与下一相位可报价边界是派生观察；用同拍无请求控制隔离 CancelAll 因果。
    let (control_spec, control_calendar) =
        mixed_spec(Box::new(|_| Box::new(|_, _| TickPlan::default())));
    let mut control = World::spawn(control_spec, control_calendar).unwrap();
    control
        .run_ticks(target)
        .expect("同 fixture 无请求控制真实运行到同一目标拍");
    let control_record = control.report_log.last().unwrap();
    assert!(control_record.group_rejections.is_empty());
    assert!(control_record.desk_receipts.is_empty());
    assert!(control_record.desk_trades.is_empty());
    assert!(control_record.desk_lifecycle.is_empty());
    assert_eq!(
        closed,
        control.publication().load().desks[1].view,
        "closed CancelAll 与同相位无请求控制的完整 StockPublic 必须一致，不删派生报价字段"
    );
    assert!(
        closed.recent_prices.is_empty(),
        "真实 closed 历史没有新增市场样本"
    );
    assert!(closed.minute_closes.is_empty());
    assert!(closed.minute_bars.is_empty());
    assert!(closed.daily_bars.is_empty());
    assert!(closed.daily_active.is_none());
    assert_eq!(
        closed.last_price, 1000,
        "closed 保留真实初始估值，不随别所市场拍变化"
    );
    assert_eq!(closed.daily_total_completed, 0);
    assert_eq!(closed.daily_total_traded, 0);
    assert_eq!(closed.volume_today, 0);
    assert_eq!(closed.daily_active_turnover_cents, 0);
    assert_eq!(closed.daily_active_trade_count, 0);
    let control_report = control.shutdown().unwrap();
    assert_eq!(control_report.worker_results.len(), 4);
    assert!(control_report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    let report = world.shutdown().unwrap();
    assert_eq!(report.group_finals.len(), 2);
    assert_eq!(report.desk_finals.len(), 2);
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn raw_cancel_all_call_auction_is_exchange_closed() {
    closed_cancel_all(1, TradingPhase::CallAuction);
}

#[test]
fn raw_cancel_all_preopen_is_exchange_closed() {
    closed_cancel_all(3, TradingPhase::PreOpen);
}

#[test]
fn raw_cancel_all_continuous_is_exchange_closed() {
    closed_cancel_all(4, TradingPhase::Continuous);
}

#[test]
fn raw_cancel_all_closing_auction_is_exchange_closed() {
    closed_cancel_all(10, TradingPhase::ClosingAuction);
}

#[test]
fn open_raw_cancel_all_releases_real_resting_purse_without_consuming_another_serial() {
    let (spec, calendar) = mixed_spec(Box::new(|group| {
        Box::new(move |tick, _| {
            let mut plan = TickPlan::default();
            if group == 0 && tick == 1 {
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: Side::Buy,
                    price: 1000,
                    qty: 100,
                });
            }
            if group == 0 && tick == 4 {
                plan.cancels.push(CancelSpec { local: 0, stock: 0 });
            }
            plan
        })
    }));
    let mut world = World::spawn(spec, calendar).unwrap();
    world.run_ticks(3).unwrap();
    let before = world.snapshot_accounts().unwrap();
    let (resting, serials) = world.collect_desk_resting().unwrap();
    assert_eq!(resting[0].len(), 1);
    assert!(resting[0][0].purse > 0, "open 正控是真实已扣出的买腿 purse");
    assert!(before[0].1[0].buy_purse_held_cents > 0);
    assert_eq!(before[0].1[0].foreign_active[0], 1);
    world.run_ticks(1).unwrap();
    let record = world.report_log.last().unwrap();
    assert!(record.group_rejections.is_empty());
    assert!(record.desk_receipts.is_empty());
    assert_eq!(
        record
            .desk_lifecycle
            .iter()
            .map(|fact| (fact.owner, fact.stock, fact.kind))
            .collect::<Vec<_>>(),
        vec![((0, 0), 0, super::protocol::LifecycleKind::Cancelled)]
    );
    let after = world.snapshot_accounts().unwrap();
    assert_eq!(after[0].1[0].buy_purse_held_cents, 0);
    assert_eq!(after[0].1[0].foreign_active[0], 0);
    assert_eq!(
        after[0].1[0].cash,
        before[0].1[0].cash + resting[0][0].purse
    );
    assert_eq!(
        world.collect_desk_resting().unwrap(),
        (vec![vec![], vec![]], serials)
    );
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}
