//! V7：同一机构账户双股真实开仓、受理、部分成交、定向撤单及日终。
//! 两种合成休市交换本股日龄在 StockId 遍历中的先后，不预测官方安排。

use super::bridge_tests::setup_fixture;
use super::decision::{DecisionSetup, NpcProfile, PendingOp};
use super::group::TickPlan;
use super::protocol::{DayEndCalendar, RoutedPrice};
use super::skeleton_tests::basic_spec;
use super::world::{BeliefParticipantSnapshot, RootProjection, World};
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar,
};
use crate::{AccountId, GameSession, Money, Side, StockCode, StockExchange, StockId};
use std::collections::BTreeMap;
use std::sync::Arc;

const OWNER: AccountId = AccountId(2);

fn world(closed_first: CalendarExchange) -> World {
    world_with_paused_plans(closed_first, false)
}

fn world_with_paused_plans(closed_first: CalendarExchange, paused: bool) -> World {
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
    personal.belief.experience_mut().feedback.latest_moment =
        Some(crate::experience::ExperienceMoment {
            civil_date: current,
            market_minute: 240,
            trading_day: 1,
        });
    for stock in [StockId(0), StockId(1)] {
        personal
            .watchlist
            .record_attention(&stock, 240, 240)
            .unwrap();
        personal.history_reads.record(&stock, 240).unwrap();
    }
    let (catalog, initial_company_facts) = session.runtime_company_handles().unwrap();
    let params = crate::strategy::BeliefChainParams {
        position_step_bp: 5_000,
        margin: 0.01,
        order_size: 700,
        daily_plan_review: true,
    };
    let mut spec = basic_spec(1, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 12;
    spec.auction_ticks = 3;
    spec.closing_auction_ticks = 3;
    spec.calendar_start_date = first;
    spec.civil_start_date = current;
    spec.initial_committed_tick = 12;
    spec.initial_last_day_end_tick = Some(12);
    spec.stocks[0].exchange = StockExchange::Shenzhen;
    spec.stocks[1].exchange = StockExchange::Shanghai;
    spec.initial_shares = vec![vec![vec![1000, 1000], vec![0, 0]]];
    spec.initial_cash_override = Some(vec![vec![2_000_000, 2_000_000]]);
    spec.initial_plan_books.clear();
    if paused {
        let mut book = crate::plans::PlanBook::default();
        for stock in [StockId(0), StockId(1)] {
            let day = if (stock == StockId(0) && closed_first == CalendarExchange::Szse)
                || (stock == StockId(1) && closed_first == CalendarExchange::Sse)
            {
                0
            } else {
                1
            };
            let id = book
                .create(crate::plans::PlanOpen {
                    account: OWNER,
                    code: stock,
                    direction: Side::Buy,
                    target: crate::plans::PlanTarget::ShareCount(700),
                    opinion: crate::plans::PlanOpinion {
                        signal_score_bp: 10_000,
                        source: crate::plans::OpinionSource::Blended,
                    },
                    confidence_bp: 5_000,
                    urgency: crate::plans::Urgency::Normal,
                    horizon_trading_days: 1,
                    created_trading_day: day,
                })
                .unwrap();
            // 仅初始化合法 Paused 计划；后续 Resumed 必须来自真实 collector/caller。
            book.apply(
                id,
                crate::plans::PlanEvent::Paused {
                    reason: crate::plans::PauseReason::IntradayDropAcceleration,
                    trading_day: day,
                },
            )
            .unwrap();
        }
        spec.initial_plan_books =
            super::plan_ownership::split(book, &[AccountId(1), OWNER], 1, 2).unwrap();
    }
    spec.root_projection = Some(Arc::new(RootProjection {
        catalog,
        initial_company_facts,
        account_ids: vec![AccountId(1), OWNER],
        auction_ticks: 3,
        closing_auction_ticks: 3,
        chain_params: vec![Some((params, Some(style))); 2],
        session_seed: 17,
        urgency_policy: session.runtime_urgency_policy(),
        max_order_qty_limit: session.runtime_max_order_qty_limit(),
        tick_size_cents: session.runtime_tick_size_cents(),
    }));
    spec.library = Some(session.runtime_library_handle().1);
    spec.config = Arc::new(setup.config);
    spec.belief_seeds = vec![
        None,
        Some(BeliefParticipantSnapshot {
            attention: personal.attention,
            watchlist: personal.watchlist,
            price_memory: personal.price_memory,
            history_reads: personal.history_reads,
            information: personal.information,
            belief: personal.belief,
            retail_experience: None,
        }),
    ];
    let npc = || NpcProfile {
        strategy: crate::strategy::StrategyData::inst(
            crate::strategy::TargetPolicy::Fixed(Money::from_cents(1000)),
            0.01,
            700,
        ),
        seed: 23,
        next_due: u64::MAX,
        attention_base: 1.0,
        retail_style: None,
    };
    spec.decision = Some(DecisionSetup {
        profiles: vec![npc(), npc()],
        attention_gap_ticks: 0,
    });
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    policy.official_coverage.push(OfficialCoverageEntry::new(
        closed_first,
        2030,
        vec![(CivilDate::from_iso("2030-01-01").unwrap(), first)],
        "synthetic-c2c-dual-plan-caller".into(),
        "synthetic-c2c-dual-plan-caller-source".into(),
    ));
    policy
        .official_coverage
        .sort_by_key(|entry| (entry.exchange, entry.year));
    World::spawn(
        spec,
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap(),
    )
    .unwrap()
}

fn dual_plan_callers(closed_first: CalendarExchange) {
    let mut world = world(closed_first);
    world.run_ticks(3).unwrap();
    let tick = world.prepare_raw_calendar_tick().unwrap();
    let days: BTreeMap<_, _> = [StockId(0), StockId(1)]
        .into_iter()
        .map(|stock| {
            (
                stock,
                tick.before_projection()
                    .stock(stock)
                    .unwrap()
                    .date_session_ordinal()
                    .unwrap(),
            )
        })
        .collect();
    assert_eq!(
        days[&StockId(0)],
        if closed_first == CalendarExchange::Szse {
            0
        } else {
            1
        }
    );
    assert_eq!(
        days[&StockId(1)],
        if closed_first == CalendarExchange::Sse {
            0
        } else {
            1
        }
    );
    world.force_due_for_test(vec![1], 16);
    world.inject_review_assessments_for_test(
        0,
        OWNER,
        [StockId(0), StockId(1)]
            .into_iter()
            .map(|stock| {
                (
                    stock,
                    crate::plans::CandidateAssessment::Scored {
                        score: crate::plans::SignalScore::new(10_000).unwrap(),
                        used_weight_bp: 10_000,
                        excluded: vec![],
                    },
                )
            })
            .collect(),
    );
    world.broadcast_external_plan(
        [0, 1]
            .into_iter()
            .map(|stock| PendingOp {
                local: 0,
                stock,
                side: Side::Sell,
                routed: RoutedPrice::Fixed(1000),
                purse_reference: 1000,
                qty: 100,
            })
            .collect(),
        vec![],
    );
    world
        .run_ticks(1)
        .expect("同一机构真实开两股计划并分别成交100股");
    let trades = &world.report_log.last().unwrap().desk_trades;
    assert_eq!(trades.len(), 2);
    assert!(trades.iter().all(|trade| trade.qty == 100));
    let book = world.collect_plans().unwrap().remove(0).1;
    let original_ids = book.plan_ids().collect::<Vec<_>>();
    assert_eq!(
        original_ids.len(),
        2,
        "真实run_chain_decision必须创建两份计划"
    );
    for stock in [StockId(0), StockId(1)] {
        let plan = book
            .active_investor_plan(OWNER, &stock)
            .expect("真实开仓计划在位");
        assert_eq!(plan.created_trading_day(), days[&stock]);
        assert_eq!(plan.last_event_trading_day(), days[&stock]);
        assert_eq!(
            plan.filled_qty(),
            100,
            "真实fill必须推进对应计划，不能只创建静态PlanOpen"
        );
        assert!(plan.active_child_order_id().is_some());
        assert!(plan.active_child_remaining_qty().unwrap() > 0);
    }
    let after_fill = world.snapshot_accounts().unwrap();
    assert_eq!(after_fill[0].1[1].shares.as_slice(), &[100, 100]);
    assert_eq!(after_fill[0].1[1].frozen_today.as_slice(), &[100, 100]);
    let live_resting = world.collect_desk_resting().unwrap().0;
    let held_purse = live_resting
        .iter()
        .flatten()
        .filter(|order| order.owner == (0, 1))
        .map(|order| order.purse)
        .sum::<i64>();
    assert!(held_purse > 0, "真实部分成交后两条余单必须仍有实际资金占用");
    assert_eq!(after_fill[0].1[1].buy_purse_held_cents, held_purse);
    let read = world.collect_open_derivations().unwrap();
    let personal = &read[0]
        .3
        .iter()
        .find(|record| record.account == OWNER)
        .unwrap()
        .personal_in;
    let moment = personal.belief.experience().feedback.latest_moment.unwrap();
    assert_eq!(
        (moment.civil_date, moment.trading_day, moment.market_minute),
        (
            CivilDate::from_iso("2030-01-03").unwrap(),
            1,
            tick.before_projection().completed_shared_minute_count()
        )
    );
    for stock in [StockId(0), StockId(1)] {
        let history = &personal.history_reads.stocks[&stock];
        assert!(
            history.read_count > 1,
            "实际观察必须重新消费该股history读取"
        );
        assert_eq!(history.last_read_market_minute, moment.market_minute);
    }
    world.defer_due_for_test(vec![1], u64::MAX);
    let resting = world.collect_desk_resting().unwrap().0;
    let cancels = resting
        .iter()
        .enumerate()
        .flat_map(|(stock, orders)| {
            orders
                .iter()
                .filter(|order| order.owner == (0, 1))
                .map(move |order| {
                    (
                        1,
                        stock as u16,
                        super::group::desk_order_serial(order.order_serial),
                    )
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(cancels.len(), 2);
    world.broadcast_external_plan(vec![], cancels);
    world.run_ticks(1).expect("真实定向撤单回流释放两条子单");
    let released = world.collect_plans().unwrap().remove(0).1;
    assert_eq!(
        released.plan_ids().collect::<Vec<_>>(),
        original_ids,
        "真实撤单不得丢失或替换计划身份"
    );
    let after_cancel = world.snapshot_accounts().unwrap();
    assert_eq!(
        after_cancel[0].1[1].buy_purse_held_cents, 0,
        "必须在撤单本拍退回全部purse，不能等shutdown代清"
    );
    assert!(
        world
            .collect_desk_resting()
            .unwrap()
            .0
            .iter()
            .flatten()
            .all(|order| order.owner != (0, 1)),
        "定向撤单本拍必须移除本人两股真实余单"
    );
    for stock in [StockId(0), StockId(1)] {
        let plan = released.active_investor_plan(OWNER, &stock).unwrap();
        assert_eq!(plan.last_event_trading_day(), days[&stock]);
        assert!(plan.active_child_order_id().is_none());
        assert_eq!(plan.filled_qty(), 100);
    }
    world.run_ticks(7).unwrap();
    let committed = world.committed_calendar_tick().unwrap().unwrap().calendar;
    world
        .run_day_end_with_calendar(DayEndCalendar::MarketTick(committed))
        .unwrap();
    let ended = world.collect_plans().unwrap().remove(0).1;
    assert_eq!(
        ended.plan_ids().collect::<Vec<_>>(),
        original_ids,
        "日终后原两计划全集合必须保持，禁止空循环假绿"
    );
    for id in &original_ids {
        let plan = ended.plan(*id).unwrap();
        assert_eq!(
            plan.status(),
            crate::plans::PlanStatus::Terminated {
                reason: crate::plans::TerminationReason::HorizonExpired
            }
        );
        assert_eq!(plan.last_event_trading_day(), days[plan.code()]);
        assert_eq!(plan.filled_qty(), 100);
    }
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 3);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    assert_eq!(report.group_finals[0].accounts[1].buy_purse_held_cents, 0);
    assert_eq!(
        report.group_finals[0].accounts[1].frozen_today.as_slice(),
        &[0, 0]
    );
}

#[test]
fn same_owner_dual_plan_callers_keep_shared_personal_clock_when_reopened_stock_sorts_first() {
    dual_plan_callers(CalendarExchange::Szse);
}

#[test]
fn same_owner_dual_plan_callers_keep_shared_personal_clock_when_reopened_stock_sorts_last() {
    dual_plan_callers(CalendarExchange::Sse);
}

fn review_input(score: Option<i32>) -> BTreeMap<StockId, crate::plans::CandidateAssessment> {
    [StockId(0), StockId(1)]
        .into_iter()
        .map(|stock| {
            (
                stock,
                match score {
                    Some(value) => crate::plans::CandidateAssessment::Scored {
                        score: crate::plans::SignalScore::new(value).unwrap(),
                        used_weight_bp: 10_000,
                        excluded: vec![],
                    },
                    None => crate::plans::CandidateAssessment::InsufficientInformation {
                        excluded: vec![],
                    },
                },
            )
        })
        .collect()
}

fn real_dual_open(closed_first: CalendarExchange) -> World {
    let mut world = world(closed_first);
    world.run_ticks(3).unwrap();
    world.force_due_for_test(vec![1], 16);
    world.inject_review_assessments_for_test(0, OWNER, review_input(Some(10_000)));
    world.broadcast_external_plan(
        [0, 1]
            .into_iter()
            .map(|stock| PendingOp {
                local: 0,
                stock,
                side: Side::Sell,
                routed: RoutedPrice::Fixed(1000),
                purse_reference: 1000,
                qty: 100,
            })
            .collect(),
        vec![],
    );
    world.run_ticks(1).unwrap();
    assert_eq!(world.report_log.last().unwrap().desk_trades.len(), 2);
    assert!(world
        .report_log
        .last()
        .unwrap()
        .desk_trades
        .iter()
        .all(|trade| trade.qty == 100));
    let book = world.collect_plans().unwrap().remove(0).1;
    assert_eq!(book.plan_ids().count(), 2);
    assert!([StockId(0), StockId(1)].into_iter().all(|stock| {
        let plan = book.active_investor_plan(OWNER, &stock).unwrap();
        plan.filled_qty() == 100 && plan.active_child_order_id().is_some()
    }));
    world
}

fn assert_action_days_and_read_personal(
    world: &mut World,
    book: &crate::plans::PlanBook,
    expected: &str,
) -> (crate::session::PlanPersonalState, u64) {
    let actions = &world.report_log.last().unwrap().lifecycle_actions_for_test;
    let matching = actions
        .iter()
        .filter(|action| action.kind == expected)
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        2,
        "两股均须由真实collect→apply产生指定动作；完整实际动作={actions:?}"
    );
    for stock in [StockId(0), StockId(1)] {
        let plan = book
            .plan_ids()
            .map(|id| book.plan(id).unwrap())
            .find(|plan| plan.code() == &stock && plan.account() == OWNER)
            .unwrap();
        let action = matching
            .iter()
            .find(|action| action.plan_id == plan.plan_id())
            .unwrap();
        assert_eq!(action.stock, stock);
        assert_eq!(action.event_trading_day, plan.created_trading_day());
        assert_eq!(action.caller_trading_day, plan.created_trading_day());
        assert_eq!(action.last_event_after, plan.created_trading_day());
        assert_eq!(action.review_day_after, plan.created_trading_day());
    }
    let last = world.committed_calendar_tick().unwrap().unwrap().calendar;
    let minute = last.before_projection().completed_shared_minute_count();
    let echo = world.collect_open_derivations().unwrap();
    assert!(
        echo.iter()
            .flat_map(|(_, _, _, rows)| rows)
            .all(|row| row.lifecycle_actions_for_test.is_empty()),
        "write_back=false Echo不能伪造实际已执行动作"
    );
    let personal = echo[0]
        .3
        .iter()
        .find(|row| row.account == OWNER)
        .unwrap()
        .personal_in
        .clone();
    (personal, minute)
}

fn assert_witness_days(world: &mut World, book: &crate::plans::PlanBook, expected: &str) {
    let (personal, minute) = assert_action_days_and_read_personal(world, book, expected);
    let moment = personal.belief.experience().feedback.latest_moment.unwrap();
    assert_eq!(
        (moment.civil_date, moment.trading_day, moment.market_minute),
        (CivilDate::from_iso("2030-01-03").unwrap(), 1, minute)
    );
    for stock in [StockId(0), StockId(1)] {
        assert_eq!(
            personal.history_reads.stocks[&stock].last_read_market_minute,
            minute
        );
    }
}

fn assert_original_ids(world: &mut World, ids: &[crate::plans::PlanId]) -> crate::plans::PlanBook {
    let book = world.collect_plans().unwrap().remove(0).1;
    assert_eq!(
        book.plan_ids().collect::<Vec<_>>(),
        ids,
        "复审必须保留原两PlanId全集合，不得新增替身或空循环"
    );
    book
}

fn close_world(world: World) {
    let report = world.shutdown().unwrap();
    assert_eq!(report.worker_results.len(), 3);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn mixed_real_lifecycle_observe_has_direct_witness_for_both_stock_orders() {
    for closed in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let mut world = real_dual_open(closed);
        let ids = world
            .collect_plans()
            .unwrap()
            .remove(0)
            .1
            .plan_ids()
            .collect::<Vec<_>>();
        world.force_due_for_test(vec![1], 17);
        world.inject_review_assessments_for_test(0, OWNER, review_input(None));
        world.run_ticks(1).unwrap();
        let book = assert_original_ids(&mut world, &ids);
        assert_witness_days(&mut world, &book, "Observe");
        let echo = world.collect_open_derivations().unwrap();
        assert!(
            echo.iter()
                .flat_map(|(_, _, _, rows)| rows)
                .all(|row| row.lifecycle_actions_for_test.is_empty()),
            "write_back=false Echo不能伪造实际已执行动作"
        );
        close_world(world);
    }
}

#[test]
fn mixed_real_lifecycle_revise_after_real_child_release_keeps_stock_days() {
    for closed in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let mut world = real_dual_open(closed);
        let ids = world
            .collect_plans()
            .unwrap()
            .remove(0)
            .1
            .plan_ids()
            .collect::<Vec<_>>();
        world.defer_due_for_test(vec![1], u64::MAX);
        let resting = world.collect_desk_resting().unwrap().0;
        let cancels = resting
            .iter()
            .enumerate()
            .flat_map(|(stock, rows)| {
                rows.iter()
                    .filter(|row| row.owner == (0, 1))
                    .map(move |row| {
                        (
                            1,
                            stock as u16,
                            super::group::desk_order_serial(row.order_serial),
                        )
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(cancels.len(), 2);
        world.broadcast_external_plan(vec![], cancels);
        world.run_ticks(1).unwrap();
        assert_eq!(
            world.snapshot_accounts().unwrap()[0].1[1].buy_purse_held_cents,
            0
        );
        let released = assert_original_ids(&mut world, &ids);
        assert!(ids.iter().all(|id| released
            .plan(*id)
            .unwrap()
            .active_child_order_id()
            .is_none()));
        world.force_due_for_test(vec![1], 18);
        world.inject_review_assessments_for_test(0, OWNER, review_input(Some(9_000)));
        world.run_ticks(1).unwrap();
        let book = assert_original_ids(&mut world, &ids);
        assert_witness_days(&mut world, &book, "Revise");
        for id in &ids {
            assert_eq!(book.plan(*id).unwrap().opinion().signal_score_bp, 9_000);
        }
        close_world(world);
    }
}

#[test]
fn mixed_real_lifecycle_restructure_cancels_actual_children_and_releases_purse_same_tick() {
    for closed in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let mut world = real_dual_open(closed);
        let ids = world
            .collect_plans()
            .unwrap()
            .remove(0)
            .1
            .plan_ids()
            .collect::<Vec<_>>();
        assert!(world.snapshot_accounts().unwrap()[0].1[1].buy_purse_held_cents > 0);
        world.force_due_for_test(vec![1], 17);
        world.inject_review_assessments_for_test(0, OWNER, review_input(Some(-10_000)));
        world.run_ticks(1).unwrap();
        let book = assert_original_ids(&mut world, &ids);
        assert_witness_days(&mut world, &book, "Restructure");
        let accounts = world.snapshot_accounts().unwrap();
        assert_eq!(
            accounts[0].1[1].buy_purse_held_cents, 0,
            "反向重构本拍必须释放实际买余单资金"
        );
        assert_eq!(accounts[0].1[1].shares.as_slice(), &[100, 100]);
        assert_eq!(
            accounts[0].1[1].frozen_today.as_slice(),
            &[100, 100],
            "反向计划不能绕过T+1卖出当日买股"
        );
        assert!(world
            .collect_desk_resting()
            .unwrap()
            .0
            .iter()
            .flatten()
            .all(|order| order.owner != (0, 1)));
        close_world(world);
    }
}

#[test]
fn mixed_real_lifecycle_terminate_cancels_actual_children_without_replacing_plan_ids() {
    for closed in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let mut world = real_dual_open(closed);
        let ids = world
            .collect_plans()
            .unwrap()
            .remove(0)
            .1
            .plan_ids()
            .collect::<Vec<_>>();
        assert!(world.snapshot_accounts().unwrap()[0].1[1].buy_purse_held_cents > 0);
        world.force_due_for_test(vec![1], 17);
        world.inject_review_assessments_for_test(0, OWNER, review_input(Some(0)));
        world.run_ticks(1).unwrap();
        let book = assert_original_ids(&mut world, &ids);
        assert_witness_days(&mut world, &book, "Revise");
        assert_witness_days(&mut world, &book, "Terminate");
        for id in &ids {
            assert_eq!(
                book.plan(*id).unwrap().status(),
                crate::plans::PlanStatus::Terminated {
                    reason: crate::plans::TerminationReason::Cancelled,
                }
            );
        }
        assert_eq!(
            world.snapshot_accounts().unwrap()[0].1[1].buy_purse_held_cents,
            0
        );
        assert!(world
            .collect_desk_resting()
            .unwrap()
            .0
            .iter()
            .flatten()
            .all(|order| order.owner != (0, 1)));
        close_world(world);
    }
}

#[test]
fn mixed_real_lifecycle_execution_resume_is_collected_and_applied_with_stock_days() {
    for closed in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let mut world = world_with_paused_plans(closed, true);
        let paused = world.collect_plans().unwrap().remove(0).1;
        let ids = paused.plan_ids().collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().all(|id| matches!(
            paused.plan(*id).unwrap().status(),
            crate::plans::PlanStatus::Paused { .. }
        )));
        world.run_ticks(4).unwrap();
        assert_eq!(
            world.snapshot_accounts().unwrap()[0].1[1].shares.as_slice(),
            &[0, 0],
            "本fixture没有本人持仓，不能期待不存在的经历事件推进feedback时钟"
        );
        assert!(
            world
                .report_log
                .iter()
                .all(|record| record.desk_trades.is_empty()),
            "Resume前必须真实无成交"
        );
        let before_echo = world.collect_open_derivations().unwrap();
        let feedback_before = before_echo[0]
            .3
            .iter()
            .find(|row| row.account == OWNER)
            .unwrap()
            .personal_in
            .belief
            .experience()
            .feedback
            .clone();
        assert!(
            feedback_before.stocks.is_empty(),
            "无HoldingEpoch的前件必须显式证明"
        );
        let before_moment = feedback_before.latest_moment.unwrap();
        assert_eq!(
            (
                before_moment.civil_date,
                before_moment.trading_day,
                before_moment.market_minute
            ),
            (CivilDate::from_iso("2030-01-03").unwrap(), 1, 240)
        );
        world.force_due_for_test(vec![1], 17);
        world.inject_review_assessments_for_test(0, OWNER, review_input(Some(10_000)));
        world.run_ticks(1).unwrap();
        let book = assert_original_ids(&mut world, &ids);
        let (personal, minute) =
            assert_action_days_and_read_personal(&mut world, &book, "ExecutionState::Resumed");
        assert_eq!(
            minute, 280,
            "真实4拍之后的共享before时钟正控，不能换成任意范围"
        );
        assert_eq!(
            personal.belief.experience().feedback,
            feedback_before,
            "无本人持仓/成交事件时完整feedback必须保持，不凭空推进经历时钟"
        );
        for stock in [StockId(0), StockId(1)] {
            assert_eq!(
                personal.history_reads.stocks[&stock].last_read_market_minute, 280,
                "真实读取仍使用共享280，不能回拨成本股首次复市分钟40"
            );
        }
        assert!(
            world
                .report_log
                .iter()
                .all(|record| record.desk_trades.is_empty()),
            "恢复动作不能伪造成交"
        );
        assert_eq!(
            world.snapshot_accounts().unwrap()[0].1[1].shares.as_slice(),
            &[0, 0]
        );
        assert!(ids
            .iter()
            .all(|id| book.plan(*id).unwrap().status() == crate::plans::PlanStatus::Active));
        close_world(world);
    }
}

fn mixed_due_world(closed_first: CalendarExchange, with_closed_plan: bool) -> World {
    let first = CivilDate::from_iso("2029-12-31").unwrap();
    let current = CivilDate::from_iso("2030-01-02").unwrap();
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
    personal.belief.experience_mut().feedback.latest_moment =
        Some(crate::experience::ExperienceMoment {
            civil_date: current,
            market_minute: 240,
            trading_day: 1,
        });
    for stock in [StockId(0), StockId(1)] {
        personal
            .watchlist
            .record_attention(&stock, 240, 240)
            .unwrap();
        personal.history_reads.record(&stock, 240).unwrap();
    }
    let (catalog, initial_company_facts) = session.runtime_company_handles().unwrap();
    let params = crate::strategy::BeliefChainParams {
        position_step_bp: 5_000,
        margin: 0.01,
        order_size: 700,
        daily_plan_review: true,
    };
    let mut spec = basic_spec(1, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 12;
    spec.auction_ticks = 3;
    spec.closing_auction_ticks = 3;
    spec.calendar_start_date = first;
    spec.civil_start_date = current;
    spec.initial_committed_tick = 12;
    spec.initial_last_day_end_tick = Some(12);
    spec.stocks[0].exchange = StockExchange::Shenzhen;
    spec.stocks[1].exchange = StockExchange::Shanghai;
    spec.initial_shares = vec![vec![vec![1000, 1000], vec![0, 0]]];
    spec.initial_cash_override = Some(vec![vec![2_000_000, 2_000_000]]);
    spec.initial_plan_books.clear();
    let closed = if closed_first == CalendarExchange::Sse {
        StockId(1)
    } else {
        StockId(0)
    };
    let mut book = crate::plans::PlanBook::default();
    for stock in [StockId(0), StockId(1)] {
        if stock == closed && !with_closed_plan {
            continue;
        }
        book.create(crate::plans::PlanOpen {
            account: OWNER,
            code: stock,
            direction: Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(700),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 5_000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 3,
            created_trading_day: 0,
        })
        .unwrap();
    }
    // prior真实日历session为Dec31；Jan1双所closed，Jan2仅一所open，不造closed未来计划。
    spec.initial_plan_books =
        super::plan_ownership::split(book, &[AccountId(1), OWNER], 1, 2).unwrap();
    spec.root_projection = Some(Arc::new(RootProjection {
        catalog,
        initial_company_facts,
        account_ids: vec![AccountId(1), OWNER],
        auction_ticks: 3,
        closing_auction_ticks: 3,
        chain_params: vec![Some((params, Some(style))); 2],
        session_seed: 17,
        urgency_policy: session.runtime_urgency_policy(),
        max_order_qty_limit: session.runtime_max_order_qty_limit(),
        tick_size_cents: session.runtime_tick_size_cents(),
    }));
    spec.library = Some(session.runtime_library_handle().1);
    spec.config = Arc::new(setup.config);
    spec.belief_seeds = vec![
        None,
        Some(BeliefParticipantSnapshot {
            attention: personal.attention,
            watchlist: personal.watchlist,
            price_memory: personal.price_memory,
            history_reads: personal.history_reads,
            information: personal.information,
            belief: personal.belief,
            retail_experience: None,
        }),
    ];
    let npc = || NpcProfile {
        strategy: crate::strategy::StrategyData::inst(
            crate::strategy::TargetPolicy::Fixed(Money::from_cents(1000)),
            0.01,
            700,
        ),
        seed: 23,
        next_due: u64::MAX,
        attention_base: 1.0,
        retail_style: None,
    };
    spec.decision = Some(DecisionSetup {
        profiles: vec![npc(), npc()],
        attention_gap_ticks: 0,
    });
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    policy.official_coverage.push(OfficialCoverageEntry::new(
        closed_first,
        2030,
        vec![(CivilDate::from_iso("2030-01-01").unwrap(), current)],
        "synthetic-c2c-dual-plan-caller".into(),
        "synthetic-c2c-dual-plan-caller-source".into(),
    ));
    policy
        .official_coverage
        .sort_by_key(|entry| (entry.exchange, entry.year));
    World::spawn(
        spec,
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn mixed_real_due_chain_skips_closed_plan_and_creation_with_open_action_positive_control() {
    for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        for with_closed_plan in [false, true] {
            let closed = if exchange == CalendarExchange::Sse {
                StockId(1)
            } else {
                StockId(0)
            };
            let open = StockId(1 - closed.0);
            let mut world = mixed_due_world(exchange, with_closed_plan);
            let preview = world.prepare_raw_calendar_tick().unwrap();
            let model = preview.day().day_model();
            assert_eq!(
                model.current_date(),
                CivilDate::from_iso("2030-01-02").unwrap()
            );
            assert_eq!(model.shared_sessions_before_date(), 1);
            for stock in [closed, open] {
                assert_eq!(model.stock(stock).unwrap().sessions_before_date(), 1);
            }
            assert_eq!(
                preview
                    .before_projection()
                    .stock(closed)
                    .unwrap()
                    .date_session_ordinal(),
                None
            );
            assert_eq!(
                preview
                    .before_projection()
                    .stock(open)
                    .unwrap()
                    .date_session_ordinal(),
                Some(1)
            );
            assert_eq!(preview.before_input().tick, 12);
            let before = world.collect_plans().unwrap().remove(0).1;
            let ids = before.plan_ids().collect::<Vec<_>>();
            let cursors = before.owner_sequences().collect::<Vec<_>>();
            let closed_before = before.active_investor_plan(OWNER, &closed).cloned();
            assert_eq!(closed_before.is_some(), with_closed_plan);
            let open_id = before.active_investor_plan(OWNER, &open).unwrap().plan_id();
            world.run_ticks(3).unwrap();
            world.force_due_for_test(vec![1], 16);
            world.inject_review_assessments_for_test(0, OWNER, review_input(Some(10_000)));
            world.run_ticks(1).unwrap();
            let record = world.report_log.last().unwrap();
            let open_review = record
                .lifecycle_actions_for_test
                .iter()
                .filter(|row| row.plan_id == open_id && row.stock == open && row.kind == "Revise")
                .collect::<Vec<_>>();
            assert_eq!(
                open_review.len(),
                1,
                "必须是原OWNER/open计划真实Revise，不能用其他owner代替：{:?}",
                record.lifecycle_actions_for_test
            );
            let event = open_review[0];
            assert_eq!(
                (
                    event.event_trading_day,
                    event.caller_trading_day,
                    event.last_event_after,
                    event.review_day_after
                ),
                (1, 1, 1, 1)
            );
            assert!(
                record
                    .lifecycle_actions_for_test
                    .iter()
                    .all(|row| row.stock != closed),
                "closed不得真实review/quote/restructure/terminate或resume"
            );
            assert!(record
                .desk_lifecycle
                .iter()
                .all(|row| row.stock != closed.0 as u16));
            let after = world.collect_plans().unwrap().remove(0).1;
            assert_eq!(
                after.plan_ids().collect::<Vec<_>>(),
                ids,
                "closed无初始计划时也不得Create替身"
            );
            assert_eq!(
                after.owner_sequences().collect::<Vec<_>>(),
                cursors,
                "真实open复审不得让closed耗PlanId"
            );
            assert_eq!(
                after.active_investor_plan(OWNER, &closed),
                closed_before.as_ref(),
                "closed既存计划完整不变，包含review时钟、execution、horizon与累计事实"
            );
            assert_eq!(
                after
                    .active_investor_plan(OWNER, &open)
                    .unwrap()
                    .last_event_trading_day(),
                1
            );
            let (resting, _) = world.collect_desk_resting().unwrap();
            assert!(
                resting[closed.0 as usize].is_empty(),
                "closed不得发出真实child"
            );
            close_world(world);
        }
    }
}
