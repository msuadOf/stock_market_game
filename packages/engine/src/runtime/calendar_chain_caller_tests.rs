//! V7：同一机构账户双股真实开仓、受理、部分成交、定向撤单及日终。
//! 两种合成休市交换本股日龄在 StockId 遍历中的先后，不预测官方安排。

use super::bridge_tests::setup_fixture;
use super::decision::{DecisionSetup, NpcProfile, PendingOp};
use super::group::TickPlan;
use super::protocol::{DayEndCalendar, RoutedPrice};
use super::skeleton_tests::basic_spec;
use super::world::{BeliefParticipantSnapshot, RootProjection, World};
use crate::calendar::{CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar};
use crate::{AccountId, GameSession, Money, Side, StockCode, StockExchange, StockId};
use std::collections::BTreeMap;
use std::sync::Arc;

const OWNER: AccountId = AccountId(2);

fn world(closed_first: CalendarExchange) -> World {
    let first = CivilDate::from_iso("2030-01-02").unwrap();
    let current = CivilDate::from_iso("2030-01-03").unwrap();
    let mut setup = setup_fixture();
    setup.start_date = first;
    setup.ticks_per_day = 12;
    setup.auction_ticks = 3;
    setup.closing_auction_ticks = 3;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 2;
    setup.strategy_params.inst.order_size = 700;
    setup.stocks[0].float_shares = 0;
    setup.stocks[1] = setup.stocks[0].clone();
    setup.stocks[1].code = StockCode("600101".into());
    setup.stocks[1].exchange = StockExchange::Shanghai;
    let session = GameSession::new(setup.clone(), 17).expect("真实两股公司事实与个人初态合法");
    let mut personal = session.runtime_personal_states()[&OWNER].clone();
    let style = crate::strategy::InstitutionStyle::ActiveTrader;
    let profile = crate::strategy::StrategyProfile::Institution(style);
    let mut rng = crate::session::SplitMix64::new(23);
    let analysis = crate::strategy::derive_analysis_profile(&profile, OWNER, &mut rng).unwrap();
    personal.belief = crate::strategy::BeliefBook::new(OWNER, profile, analysis, &mut rng);
    assert_eq!(personal.belief.analysis().weights().fundamental_bp(), 0);
    // 共享时钟前置正控：真实观察不能在首次复市股上回拨到本股分钟零/日龄零。
    personal.belief.experience_mut().feedback.latest_moment = Some(crate::experience::ExperienceMoment {
        civil_date: current, market_minute: 240, trading_day: 1,
    });
    for stock in [StockId(0), StockId(1)] {
        personal.watchlist.record_attention(&stock, 240, 240).unwrap();
        personal.history_reads.record(&stock, 240).unwrap();
    }
    let (catalog, initial_company_facts) = session.runtime_company_handles().unwrap();
    let params = crate::strategy::BeliefChainParams {
        position_step_bp: 5_000, margin: 0.01, order_size: 700, daily_plan_review: true,
    };
    let mut spec = basic_spec(1, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 12; spec.auction_ticks = 3; spec.closing_auction_ticks = 3;
    spec.calendar_start_date = first; spec.civil_start_date = current;
    spec.initial_committed_tick = 12; spec.initial_last_day_end_tick = Some(12);
    spec.stocks[0].exchange = StockExchange::Shenzhen;
    spec.stocks[1].exchange = StockExchange::Shanghai;
    spec.initial_shares = vec![vec![vec![1000, 1000], vec![0, 0]]];
    spec.initial_cash_override = Some(vec![vec![2_000_000, 2_000_000]]);
    spec.initial_plan_books.clear();
    spec.root_projection = Some(Arc::new(RootProjection {
        catalog, initial_company_facts, account_ids: vec![AccountId(1), OWNER],
        auction_ticks: 3, closing_auction_ticks: 3,
        chain_params: vec![Some((params, Some(style))); 2], session_seed: 17,
        urgency_policy: session.runtime_urgency_policy(),
        max_order_qty_limit: session.runtime_max_order_qty_limit(),
        tick_size_cents: session.runtime_tick_size_cents(),
    }));
    spec.library = Some(session.runtime_library_handle().1);
    spec.config = Arc::new(setup.config);
    spec.belief_seeds = vec![None, Some(BeliefParticipantSnapshot {
        attention: personal.attention, watchlist: personal.watchlist,
        price_memory: personal.price_memory, history_reads: personal.history_reads,
        information: personal.information, belief: personal.belief, retail_experience: None,
    })];
    let npc = || NpcProfile {
        strategy: crate::strategy::StrategyData::inst(
            crate::strategy::TargetPolicy::Fixed(Money::from_cents(1000)), 0.01, 700),
        seed: 23, next_due: u64::MAX, attention_base: 1.0, retail_style: None,
    };
    spec.decision = Some(DecisionSetup { profiles: vec![npc(), npc()], attention_gap_ticks: 0 });
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    policy.official_coverage.push(OfficialCoverageEntry::new(closed_first, 2030,
        vec![(CivilDate::from_iso("2030-01-01").unwrap(), first)],
        "synthetic-c2c-dual-plan-caller".into(), "synthetic-c2c-dual-plan-caller-source".into()));
    World::spawn(spec, TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap()).unwrap()
}

fn dual_plan_callers(closed_first: CalendarExchange) {
    let mut world = world(closed_first);
    world.run_ticks(3).unwrap();
    let tick = world.prepare_raw_calendar_tick().unwrap();
    let days: BTreeMap<_, _> = [StockId(0), StockId(1)].into_iter().map(|stock|
        (stock, tick.before_projection().stock(stock).unwrap().date_session_ordinal().unwrap())).collect();
    assert_eq!(days[&StockId(0)], if closed_first == CalendarExchange::Szse { 0 } else { 1 });
    assert_eq!(days[&StockId(1)], if closed_first == CalendarExchange::Sse { 0 } else { 1 });
    world.force_due_for_test(vec![1], 16);
    world.inject_review_assessments_for_test(0, OWNER, [StockId(0), StockId(1)].into_iter().map(|stock|
        (stock, crate::plans::CandidateAssessment::Scored {
            score: crate::plans::SignalScore::new(10_000).unwrap(), used_weight_bp: 10_000, excluded: vec![],
        })).collect());
    world.broadcast_external_plan([0, 1].into_iter().map(|stock| PendingOp {
        local: 0, stock, side: Side::Sell, routed: RoutedPrice::Fixed(1000), purse_reference: 1000, qty: 100,
    }).collect(), vec![]);
    world.run_ticks(1).expect("同一机构真实开两股计划并分别成交100股");
    let trades = &world.report_log.last().unwrap().desk_trades;
    assert_eq!(trades.len(), 2);
    assert!(trades.iter().all(|trade| trade.qty == 100));
    let book = world.collect_plans().unwrap().remove(0).1;
    let original_ids = book.plan_ids().collect::<Vec<_>>();
    assert_eq!(original_ids.len(), 2, "真实run_chain_decision必须创建两份计划");
    for stock in [StockId(0), StockId(1)] {
        let plan = book.active_investor_plan(OWNER, &stock).expect("真实开仓计划在位");
        assert_eq!(plan.created_trading_day(), days[&stock]);
        assert_eq!(plan.last_event_trading_day(), days[&stock]);
        assert_eq!(plan.filled_qty(), 100, "真实fill必须推进对应计划，不能只创建静态PlanOpen");
        assert!(plan.active_child_order_id().is_some());
        assert!(plan.active_child_remaining_qty().unwrap() > 0);
    }
    let after_fill = world.snapshot_accounts().unwrap();
    assert_eq!(after_fill[0].1[1].shares.as_slice(), &[100, 100]);
    assert_eq!(after_fill[0].1[1].frozen_today.as_slice(), &[100, 100]);
    let live_resting = world.collect_desk_resting().unwrap().0;
    let held_purse = live_resting.iter().flatten().filter(|order| order.owner == (0, 1))
        .map(|order| order.purse).sum::<i64>();
    assert!(held_purse > 0, "真实部分成交后两条余单必须仍有实际资金占用");
    assert_eq!(after_fill[0].1[1].buy_purse_held_cents, held_purse);
    let read = world.collect_open_derivations().unwrap();
    let personal = &read[0].3.iter().find(|record| record.account == OWNER).unwrap().personal_in;
    let moment = personal.belief.experience().feedback.latest_moment.unwrap();
    assert_eq!((moment.civil_date, moment.trading_day, moment.market_minute),
        (CivilDate::from_iso("2030-01-03").unwrap(), 1, tick.before_projection().completed_shared_minute_count()));
    for stock in [StockId(0), StockId(1)] {
        let history = &personal.history_reads.stocks[&stock];
        assert!(history.read_count > 1, "实际观察必须重新消费该股history读取");
        assert_eq!(history.last_read_market_minute, moment.market_minute);
    }
    world.defer_due_for_test(vec![1], u64::MAX);
    let resting = world.collect_desk_resting().unwrap().0;
    let cancels = resting.iter().enumerate().flat_map(|(stock, orders)| orders.iter()
        .filter(|order| order.owner == (0, 1))
        .map(move |order| (1, stock as u16, super::group::desk_order_serial(order.order_serial)))).collect::<Vec<_>>();
    assert_eq!(cancels.len(), 2);
    world.broadcast_external_plan(vec![], cancels);
    world.run_ticks(1).expect("真实定向撤单回流释放两条子单");
    let released = world.collect_plans().unwrap().remove(0).1;
    assert_eq!(released.plan_ids().collect::<Vec<_>>(), original_ids, "真实撤单不得丢失或替换计划身份");
    let after_cancel = world.snapshot_accounts().unwrap();
    assert_eq!(after_cancel[0].1[1].buy_purse_held_cents, 0, "必须在撤单本拍退回全部purse，不能等shutdown代清");
    assert!(world.collect_desk_resting().unwrap().0.iter().flatten()
        .all(|order| order.owner != (0, 1)), "定向撤单本拍必须移除本人两股真实余单");
    for stock in [StockId(0), StockId(1)] {
        let plan = released.active_investor_plan(OWNER, &stock).unwrap();
        assert_eq!(plan.last_event_trading_day(), days[&stock]);
        assert!(plan.active_child_order_id().is_none());
        assert_eq!(plan.filled_qty(), 100);
    }
    world.run_ticks(7).unwrap();
    let committed = world.committed_calendar_tick().unwrap().unwrap().calendar;
    world.run_day_end_with_calendar(DayEndCalendar::MarketTick(committed)).unwrap();
    let ended = world.collect_plans().unwrap().remove(0).1;
    assert_eq!(ended.plan_ids().collect::<Vec<_>>(), original_ids, "日终后原两计划全集合必须保持，禁止空循环假绿");
    for id in &original_ids {
        let plan = ended.plan(*id).unwrap();
        assert_eq!(plan.status(), crate::plans::PlanStatus::Terminated { reason: crate::plans::TerminationReason::HorizonExpired });
        assert_eq!(plan.last_event_trading_day(), days[plan.code()]);
        assert_eq!(plan.filled_qty(), 100);
    }
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 3);
    assert!(report.worker_results.iter().all(|(_, result)| result.is_ok()));
    assert_eq!(report.group_finals[0].accounts[1].buy_purse_held_cents, 0);
    assert_eq!(report.group_finals[0].accounts[1].frozen_today.as_slice(), &[0, 0]);
}

#[test]
fn same_owner_dual_plan_callers_keep_shared_personal_clock_when_reopened_stock_sorts_first() {
    dual_plan_callers(CalendarExchange::Szse);
}

#[test]
fn same_owner_dual_plan_callers_keep_shared_personal_clock_when_reopened_stock_sorts_last() {
    dual_plan_callers(CalendarExchange::Sse);
}
