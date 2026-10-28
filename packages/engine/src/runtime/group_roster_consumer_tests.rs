//! 多组名册的真实 Start 消费证据；金额为分，数量为股。

use super::*;
use crate::orderbook::Side;
use crate::plans::{CandidateAssessment, PlanOpen, PlanOpinion, PlanTarget, SignalScore};
use crate::runtime::decision::{DecisionSetup, NpcProfile};
use crate::runtime::group::TickPlan;
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, GameSession, StockId};
use std::collections::BTreeMap;

const OWNERS: [AccountId; 2] = [AccountId(1), AccountId(2)];
const REMAINING: [u32; 2] = [100, 300];

fn world() -> World {
    world_with_stocks(1)
}

fn world_with_stocks(n_stocks: usize) -> World {
    world_with_stocks_and_counterparty(n_stocks, false)
}

fn world_with_stocks_and_counterparty(n_stocks: usize, counterparty: bool) -> World {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.stocks[1] = setup.stocks[0].clone();
    setup.stocks[1].code = crate::account::StockCode("000002".to_owned());
    setup.stocks.truncate(n_stocks);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 2;
    setup.strategy_params.inst.order_size = 100;
    // 本组证据只隔离名册消费；价格笼子由既有独立套件覆盖。
    setup.config.price_cage_enabled = false;
    let session = GameSession::new(setup.clone(), 17).expect("合法机构会话");
    let (catalog, initial_company_facts) = session.runtime_company_handles().expect("选定公司事实合法");
    let style = crate::strategy::InstitutionStyle::ActiveTrader;
    let mut spec = basic_spec(
        2,
        n_stocks,
        1,
        Box::new(|_| Box::new(|_, _| TickPlan::default())),
    );
    spec.ticks_per_day = 240;
    // 两组均为机构 local0；扣出卖单后分别在手 400/700、在途 100/300。
    spec.initial_shares = [500, 1_000]
        .into_iter()
        .map(|qty| {
            let mut shares = vec![0; n_stocks];
            shares[0] = qty;
            vec![shares]
        })
        .collect();
    for stock in &mut spec.stocks {
        stock.exchange = crate::StockExchange::Shenzhen;
    }
    spec.initial_cash_override = Some(vec![vec![1_000_000], vec![1_000_000]]);
    spec.belief_seeds = OWNERS
        .iter()
        .map(|&owner| {
            let mut personal = session.runtime_personal_states()[&owner].clone();
            let profile = crate::strategy::StrategyProfile::Institution(style);
            let mut rng = crate::session::SplitMix64::new(23 + owner.0);
            let analysis = crate::strategy::derive_analysis_profile(&profile, owner, &mut rng)
                .expect("技术分析机构参数");
            personal.belief = crate::strategy::BeliefBook::new(owner, profile, analysis, &mut rng);
            assert_eq!(personal.belief.analysis().weights().fundamental_bp(), 0);
            Some(BeliefParticipantSnapshot {
                attention: personal.attention,
                watchlist: personal.watchlist,
                price_memory: personal.price_memory,
                history_reads: personal.history_reads,
                information: personal.information,
                belief: personal.belief,
                retail_experience: None,
            })
        })
        .collect();
    spec.root_projection = Some(Arc::new(RootProjection {
        catalog,
        initial_company_facts,
        account_ids: OWNERS.to_vec(),
        // 错取组 0 也得到有效 Some，必须由数值断言识别。
        chain_params: [2_000, 8_000]
            .into_iter()
            .enumerate()
            .map(|(group, step)| {
                Some((
                    crate::strategy::BeliefChainParams {
                        position_step_bp: step,
                        margin: 0.01,
                        order_size: [100, 300][group],
                        daily_plan_review: true,
                    },
                    Some(style),
                ))
            })
            .collect(),
        auction_ticks: 0,
        closing_auction_ticks: 3,
        session_seed: 17,
        urgency_policy: crate::plans::UrgencyPolicy::default(),
        max_order_qty_limit: session.runtime_max_order_qty_limit(),
        tick_size_cents: session.runtime_tick_size_cents(),
    }));
    spec.library = Some(session.runtime_library_handle().1);
    spec.config = Arc::new(setup.config);
    spec.decision = Some(DecisionSetup {
        profiles: OWNERS
            .iter()
            .map(|_| NpcProfile {
                strategy: crate::strategy::StrategyData::inst(
                    crate::strategy::TargetPolicy::Fixed(Money::from_cents(1_000)),
                    0.01,
                    300,
                ),
                seed: 23,
                next_due: u64::MAX,
                attention_base: 1.0,
                retail_style: None,
            })
            .collect(),
        attention_gap_ticks: 0,
    });
    if counterparty {
        // 独立第三账户只作真实对手盘，避免既有自成交保护挡住同价队列。
        spec.n_groups = 3;
        spec.initial_shares.push(vec![vec![0; n_stocks]]);
        spec.initial_cash_override.as_mut().unwrap().push(vec![1_000_000]);
        spec.belief_seeds.push(None);
        let root = Arc::get_mut(spec.root_projection.as_mut().unwrap()).unwrap();
        root.account_ids.push(AccountId(0));
        root.chain_params.push(None);
        spec.decision.as_mut().unwrap().profiles.push(NpcProfile {
            strategy: crate::strategy::StrategyData::retail(0.0, 100, 0.0),
            seed: 91, next_due: u64::MAX, attention_base: 0.0,
            retail_style: Some(crate::strategy::RetailStyle::Noise),
        });
    }
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("真实线程世界")
}

fn assessment(score: i32) -> BTreeMap<StockId, CandidateAssessment> {
    BTreeMap::from([(
        StockId(0),
        CandidateAssessment::Scored {
            score: SignalScore::new(score).expect("合法评分"),
            used_weight_bp: 10_000,
            excluded: Vec::new(),
        },
    )])
}

fn prepare_tick(world: &mut World, tick: u64, score: Option<i32>) {
    prepare_assessments(world, tick, score.map(assessment).unwrap_or_default());
}

fn prepare_assessments(
    world: &mut World,
    tick: u64,
    assessments: BTreeMap<StockId, CandidateAssessment>,
) {
    for (group, tx) in world.group_txs.iter().enumerate() {
        tx.send(GroupIn::InjectReviewAssessments {
            account: OWNERS[group],
            assessments: assessments.clone(),
        })
        .expect("向实际组线程注入候选");
        tx.send(GroupIn::InjectObservationMinutes {
            stock: 0,
            minutes: vec![(0, 1_000), (1, 1_000)],
        })
        .expect("平价分钟观察");
        tx.send(GroupIn::ForceDue {
            locals: vec![0],
            tick,
            ensure_observe: true,
        })
        .expect("两组均在真实 Start 执行决策");
    }
}

fn pending_world() -> World {
    pending_world_with_stocks(1)
}

fn pending_world_with_stocks(n_stocks: usize) -> World {
    pending_world_with_targets(n_stocks, REMAINING)
}

fn pending_world_with_targets(n_stocks: usize, remaining: [u32; 2]) -> World {
    let mut world = world_with_stocks(n_stocks);
    for (group, tx) in world.group_txs.iter().enumerate() {
        tx.send(GroupIn::MutatePlanBookForTest { owner: OWNERS[group], mutate: Box::new(move |book| {
            book.create(PlanOpen {
                account: OWNERS[group],
                code: StockId(0),
                direction: Side::Sell,
                target: PlanTarget::ShareCount(remaining[group]),
                opinion: PlanOpinion {
                    signal_score_bp: -10_000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: crate::plans::Urgency::Urgent,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .expect("种子卖计划");
        }) })
        .expect("种子计划投递");
    }
    prepare_tick(&mut world, 1, None);
    world.run_ticks(1).expect("真实首拍报价、实扣卖股并受理");
    assert_pending(&mut world, remaining);
    world
}

fn assert_pending(world: &mut World, remaining: [u32; 2]) {
    let accounts = world.snapshot_accounts().expect("实际组账户");
    let plans = world.collect_plans().expect("实际计划簿");
    let (resting, _) = world.collect_desk_resting().expect("真实交易部在途单");
    assert_eq!(resting[0].len(), 2);
    for group in 0..2 {
        let account = &accounts[group].1[0];
        let order = resting[0]
            .iter()
            .find(|order| order.owner == (group as u16, 0))
            .expect("各组自己的在途卖单");
        assert_eq!(account.shares[0], [500, 1_000][group] - remaining[group]);
        assert_eq!(account.pending_sell_qty[0], remaining[group]);
        assert_eq!(order.qty, remaining[group]);
        assert_eq!(order.filled_qty, 0);
        assert_eq!(order.side, Side::Sell);
        assert_eq!(world.public_views()[0].last_price, 1_000);
        let plan = plans[group]
            .1
            .active_investor_plan(OWNERS[group], &StockId(0))
            .expect("在途计划");
        assert_eq!(plan.active_child_remaining_qty(), Some(remaining[group]));
    }
}

#[test]
fn roster_root_equity_adds_only_own_pending_sell_across_real_start() {
    // 不修改任何历史记忆。尾组遗漏600股将把本人平价权益误读为30%回撤，
    // 超过 ActiveTrader 的所有合法个人触发阈值（800..1800bp）。
    let remaining = [100, 600];
    let mut world = pending_world_with_targets(1, remaining);
    prepare_tick(&mut world, 2, None);
    world.run_ticks(1).expect("在途卖股跨越第二次真实 Start");
    assert_pending(&mut world, remaining);
    let accounts = world.snapshot_accounts().expect("同收束点账户");
    let records = world
        .collect_open_derivations()
        .expect("生产 root 观察读面");
    for group in 0..2 {
        let account = &accounts[group].1[0];
        let owned = account.shares[0] + remaining[group];
        assert_eq!(owned, [500, 1_000][group]);
        let equity = account.cash + account.buy_purse_held_cents + i64::from(owned) * 1_000;
        assert_eq!(equity, [1_500_000, 2_000_000][group]);
        let record = &records[group].3[0];
        assert_eq!(record.account, OWNERS[group]);
        assert_eq!(
            record.personal_in.belief.experience().peak_equity,
            Some(Money::from_cents(equity)),
            "真实 Start 已把本账户含在途权益写回个人态"
        );
        assert_eq!(
            record.personal_out.belief.experience().peak_equity,
            Some(Money::from_cents(equity)),
            "观察读面不得混入另一组或丢弃本账户在途卖股"
        );
        assert!(
            !record.personal_in.belief.institution_account_risk_paused(),
            "真实 Start 后本人权益未回撤，不能因卖股在途而暂停买入"
        );
        assert!(
            !record.personal_out.belief.institution_account_risk_paused(),
            "同拍 root 读面也不能把真实在途股份误算为亏损"
        );
    }
    world.shutdown().expect("撤单返股并回收线程");
}

#[test]
fn roster_open_held_survives_real_partial_fill_and_plan_termination() {
    let mut world = world();
    // 先让另一组的外部买单成为真实对手盘，尾组卖900股后成交800、在途100。
    world.send_external_plan_grouped(
        vec![
            vec![crate::runtime::decision::PendingOp {
                local: 0,
                stock: 0,
                side: Side::Buy,
                routed: crate::runtime::protocol::RoutedPrice::Fixed(1_000),
                purse_reference: 1_000,
                qty: 800,
            }],
            Vec::new(),
        ],
        vec![Vec::new(); 2],
    );
    world.run_ticks(1).expect("先挂真实买单");
    world.group_txs[1]
        .send(GroupIn::MutatePlanBookForTest { owner: OWNERS[1], mutate: Box::new(|book| {
            book.create(PlanOpen {
                account: OWNERS[1],
                code: StockId(0),
                direction: Side::Sell,
                target: PlanTarget::ShareCount(900),
                opinion: PlanOpinion {
                    signal_score_bp: -10_000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: crate::plans::Urgency::Urgent,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .expect("卖计划种子");
        }) })
        .expect("种子计划投递");
    prepare_tick(&mut world, 2, None);
    world.run_ticks(1).expect("真实卖子单部分成交");
    let before = world.collect_plans().expect("真实部分成交计划");
    let old = before[1]
        .1
        .active_investor_plan(OWNERS[1], &StockId(0))
        .expect("尾组旧卖计划");
    let old_id = old.plan_id();
    assert_eq!(old.filled_qty(), 800);
    assert_eq!(old.active_child_remaining_qty(), Some(100));
    let accounts = world.snapshot_accounts().expect("真实扣股与成交收款");
    assert_eq!(accounts[1].1[0].shares[0], 100);
    assert_eq!(accounts[1].1[0].pending_sell_qty[0], 100);
    // 本次仍强卖：复审在手100的目标小于已成交800，生产 Restructure 终结
    // 旧计划；同轮开仓发生在旧子单 Closed 之前，须读到在手100+在途100。
    prepare_tick(&mut world, 3, Some(-10_000));
    world
        .run_ticks(1)
        .expect("真实复审终止旧计划、同轮新开仓、定向撤单");
    let books = world.collect_plans().expect("生产计划读面");
    assert!(books[1]
        .1
        .plan(old_id)
        .expect("保留旧计划历史")
        .is_terminal());
    let new = books[1]
        .1
        .active_investor_plan(OWNERS[1], &StockId(0))
        .expect("尾组新卖计划");
    assert_ne!(new.plan_id(), old_id);
    assert_eq!(new.direction(), Side::Sell);
    assert_eq!(
        new.target(),
        PlanTarget::ShareCount(200),
        "同轮新卖计划须含尚在交易部的100股；丢失在途会只卖100股"
    );
    world.shutdown().expect("回收真实在途资源");
}

#[test]
fn roster_lifecycle_revision_consumes_own_pending_equity() {
    let mut world = pending_world();
    prepare_tick(&mut world, 2, Some(10_000));
    world.run_ticks(1).expect("真实 Start 反向复审并撤销旧子单");
    let books = world.collect_plans().expect("实际复审结果");
    let tail = books[1]
        .1
        .active_investor_plan(OWNERS[1], &StockId(0))
        .expect("尾组复审后计划");
    assert_eq!(tail.direction(), Side::Buy);
    assert_eq!(
        tail.target(),
        PlanTarget::ShareCount(1_300),
        "复审原有在手700、权益2_000_000，目标2_000；权益漏掉在途300股会错误修订为1_000"
    );
    assert!(tail.last_revision().is_some(), "必须走既有计划修订");
    world.shutdown().expect("清理线程");
}

#[test]
fn roster_child_quote_uses_distinct_valid_tail_chain_parameters() {
    let mut world = world();
    prepare_tick(&mut world, 1, Some(10_000));
    world.run_ticks(1).expect("真实 Start 首次买单报价");
    let (resting, _) = world.collect_desk_resting().expect("生产交易部报价结果");
    let tail = resting[0]
        .iter()
        .find(|order| order.owner == (1, 0))
        .expect("尾组买单");
    assert_eq!(tail.side, Side::Buy);
    assert_eq!(
        tail.qty, 300,
        "尾组 order_size=300，错取有效组0参数会报价100"
    );
    world.shutdown().expect("退款并回收线程");
}

#[test]
fn roster_open_equity_includes_own_other_stock_pending_sell() {
    let mut world = pending_world_with_stocks(2);
    let mut scores = assessment(10_000);
    let score = scores.remove(&StockId(0)).expect("确定性开仓评分");
    prepare_assessments(&mut world, 2, BTreeMap::from([(StockId(1), score)]));
    world.run_ticks(1).expect("在途卖股跨拍后真实开仓另一股");
    let books = world.collect_plans().expect("生产开仓计划读面");
    let tail = books[1]
        .1
        .active_investor_plan(OWNERS[1], &StockId(1))
        .expect("尾组另一股新买计划");
    assert_eq!(tail.direction(), Side::Buy);
    assert_eq!(
        tail.target(),
        PlanTarget::ShareCount(1_600),
        "本账户权益2_000_000分乘80%，每股1000分；漏掉在途300股会降为1300股"
    );
    world.shutdown().expect("清理两股真实在途资源");
}

/// urgency 只将含在途 equity 用作 max(peak, equity) 的第二项；两处生产
/// 调用都在本人 observe 之后，peak 已不小于同口径 equity。错名册只能
/// 漏掉本组在途量，因此被 max 吸收。本例固定真实复审入口及此不变量，
/// 不把无法观察到的错误偏移冒称为可杀 mutation。
#[test]
fn roster_urgency_after_own_observation_absorbs_missing_pending_in_peak_max() {
    let remaining = [100, 600];
    let mut world = pending_world_with_targets(1, remaining);
    prepare_assessments(
        &mut world,
        2,
        BTreeMap::from([(
            StockId(0),
            CandidateAssessment::InsufficientInformation {
                excluded: Vec::new(),
            },
        )]),
    );
    world
        .run_ticks(1)
        .expect("真实 Start 复审既有卖计划，经过 urgency 回调");
    assert_pending(&mut world, remaining);
    let accounts = world.snapshot_accounts().expect("同收束点真实账户");
    let records = world.collect_open_derivations().expect("同拍本人观察");
    let books = world.collect_plans().expect("生产复审记录");
    for group in 0..2 {
        let account = &accounts[group].1[0];
        let equity_without_pending =
            account.cash + account.buy_purse_held_cents + i64::from(account.shares[0]) * 1_000;
        let correct_equity = equity_without_pending + i64::from(remaining[group]) * 1_000;
        let peak = records[group].3[0]
            .personal_in
            .belief
            .experience()
            .peak_equity
            .expect("本次真实 observe 已保存本人权益峰值")
            .cents();
        assert_eq!(correct_equity, [1_500_000, 2_000_000][group]);
        assert!(peak >= correct_equity);
        assert!(equity_without_pending < correct_equity);
        assert_eq!(
            peak.max(correct_equity),
            peak.max(equity_without_pending),
            "urgency 的数值投影对漏在途量不可观测，不能强造低峰值状态杀偏移 mutation"
        );
        let plan = books[group]
            .1
            .active_investor_plan(OWNERS[group], &StockId(0))
            .expect("既有在途卖计划仍存");
        assert_eq!(
            plan.review().last_review_price,
            Some(Money::from_cents(1_000)),
            "候选虽无评分，本次生产复审仍执行 urgency 并记录真实行情"
        );
    }
    world.shutdown().expect("清理真实线程");
}

#[test]
fn plan_identity_owner_order_does_not_override_real_child_fifo() {
    let mut world = world_with_stocks_and_counterparty(1, true);
    // 真实成交先把价格打到主板10%跌停900分，同价卖单不能继续向下改善。
    world.send_external_plan_grouped(vec![vec![crate::runtime::decision::PendingOp {
        local: 0, stock: 0, side: Side::Buy,
        routed: crate::runtime::protocol::RoutedPrice::Fixed(900), purse_reference: 900, qty: 100,
    }], vec![crate::runtime::decision::PendingOp {
        local: 0, stock: 0, side: Side::Sell,
        routed: crate::runtime::protocol::RoutedPrice::Fixed(900), purse_reference: 900, qty: 100,
    }]], vec![Vec::new(); 2]);
    world.run_ticks(1).unwrap();
    let quote_owner = |world: &mut World, group: usize, tick| {
        world.group_txs[group].send(GroupIn::MutatePlanBookForTest {
            owner: OWNERS[group],
            mutate: Box::new(move |book| {
                let id = book.create(PlanOpen {
                    account: OWNERS[group], code: StockId(0), direction: Side::Sell,
                    target: PlanTarget::ShareCount(100),
                    opinion: PlanOpinion { signal_score_bp: -10_000, source: crate::plans::OpinionSource::Blended },
                    confidence_bp: 8_000, urgency: crate::plans::Urgency::Urgent,
                    horizon_trading_days: 5, created_trading_day: 0,
                }).unwrap();
                assert_eq!(id.sequence, 0);
            }),
        }).unwrap();
        world.group_txs[group].send(GroupIn::InjectReviewAssessments {
            account: OWNERS[group], assessments: BTreeMap::new(),
        }).unwrap();
        world.group_txs[group].send(GroupIn::InjectObservationMinutes {
            stock: 0, minutes: vec![(0, 1_000), (1, 1_000)],
        }).unwrap();
        world.group_txs[group].send(GroupIn::ForceDue {
            locals: vec![0], tick, ensure_observe: true,
        }).unwrap();
        world.run_ticks(1).unwrap();
        world.group_txs[group].send(GroupIn::DeferDueForTest {
            locals: vec![0], tick: u64::MAX,
        }).unwrap();
    };
    // 高owner的真实计划子单先独立收束，低owner下一拍才报价，固定实际受理先后。
    quote_owner(&mut world, 1, 2);
    let first = world.collect_plans().unwrap()[1].1.active_investor_plan(OWNERS[1], &StockId(0)).unwrap().clone();
    let first_child = first.active_child_order_id().expect("高owner真实计划子单");
    quote_owner(&mut world, 0, 3);
    let books = world.collect_plans().unwrap();
    let second = books[0].1.active_investor_plan(OWNERS[0], &StockId(0)).unwrap().clone();
    let second_child = second.active_child_order_id().expect("低owner真实计划子单");
    assert!(first.plan_id() > second.plan_id(), "PlanId数值顺序与实际到达相反");
    assert!(first_child < second_child, "desk真实时间序保留先到顺序");
    let (resting, _) = world.collect_desk_resting().unwrap();
    assert_eq!(resting[0].len(), 2);
    assert!(resting[0].iter().all(|order| order.price == 900 && order.side == Side::Sell && order.qty == 100), "两个真实计划子单必须同价同向：{resting:?}");
    // 对手单只成交100股，必须消耗先到的高owner计划子单。
    world.send_external_plan_grouped(vec![Vec::new(), Vec::new(), vec![crate::runtime::decision::PendingOp {
        local: 0, stock: 0, side: Side::Buy,
        routed: crate::runtime::protocol::RoutedPrice::Fixed(900), purse_reference: 900, qty: 100,
    }]], vec![Vec::new(); 3]);
    world.run_ticks(1).unwrap();
    let books = world.collect_plans().unwrap();
    assert_eq!(books[1].1.plan(first.plan_id()).unwrap().filled_qty(), 100, "真实受理先到者应先成交：{:?}", world.report_log.last());
    assert_eq!(books[0].1.plan(second.plan_id()).unwrap().filled_qty(), 0);
    assert_eq!(books[0].1.plan(second.plan_id()).unwrap().active_child_order_id(), Some(second_child));
    world.shutdown().unwrap();
}

#[test]
fn plan_identity_failed_tick_leaves_no_plans_cursors_children_or_publication() {
    let mut world = world();
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    prepare_tick(&mut world, 1, Some(-10_000));
    world.set_validator(Box::new(|_| Err("计划身份失败tick回滚验证".to_owned())));
    assert!(matches!(world.run_ticks(1), Err(RuntimeError::TickValidation { .. })));
    let books = world.collect_plans().unwrap();
    assert!(books.iter().all(|(_, book)| book.plan_ids().next().is_none() && book.owner_sequences().next().is_none()), "本tick新计划和游标全部撤销");
    assert!(world.collect_desk_resting().unwrap().0.iter().all(Vec::is_empty), "失败tick不留子单");
    let after = world.snapshot_accounts().unwrap();
    for (group, rows) in accounts {
        for (index, old) in rows.iter().enumerate() {
            let current = &after[group as usize].1[index];
            assert_eq!(current.cash, old.cash);
            assert_eq!(current.shares, old.shares);
            assert_eq!(current.buy_purse_held_cents, old.buy_purse_held_cents);
            assert_eq!(current.pending_sell_qty, old.pending_sell_qty);
        }
    }
    assert!(Arc::ptr_eq(&before, &world.publication().load()), "失败tick不发布任何新事件版本");
    assert_eq!(world.global.committed_tick, 0);
    world.shutdown().unwrap();
}
