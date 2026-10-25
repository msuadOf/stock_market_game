//! 在途买单退款后的现金观察必须与同一收束点的实际占用一致。

use super::*;
use crate::runtime::decision::{DecisionSetup, NpcProfile};
use crate::runtime::skeleton_tests::basic_spec;
use crate::runtime::world::{BeliefParticipantSnapshot, RootProjection, World};
use crate::{AccountId, GameSession, StockId};

const OWNER: AccountId = AccountId(2);

fn price_improvement_world() -> World {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.stocks.truncate(1);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 2;
    setup.strategy_params.inst.order_size = 100;
    let session = GameSession::new(setup.clone(), 17).expect("合法机构会话");
    let mut personal = session.runtime_personal_states()[&OWNER].clone();
    let profile = crate::strategy::StrategyProfile::Institution(
        crate::strategy::InstitutionStyle::ActiveTrader,
    );
    let mut rng = crate::session::SplitMix64::new(23);
    let analysis = crate::strategy::derive_analysis_profile(&profile, OWNER, &mut rng)
        .expect("技术分析机构参数");
    personal.belief = crate::strategy::BeliefBook::new(OWNER, profile, analysis, &mut rng);
    assert_eq!(personal.belief.analysis().weights().fundamental_bp(), 0);
    let (catalog, company_registry, operations) = session.runtime_company_handles();
    let params = crate::strategy::BeliefChainParams {
        position_step_bp: 5_000,
        margin: 0.01,
        order_size: 700,
        daily_plan_review: true,
    };
    let projection = RootProjection {
        catalog,
        company_registry,
        operations,
        account_ids: vec![AccountId(1), OWNER],
        chain_params: vec![
            Some((
                params,
                Some(crate::strategy::InstitutionStyle::ActiveTrader)
            ));
            2
        ],
        auction_ticks: 0,
        closing_auction_ticks: 3,
        session_seed: 17,
        urgency_policy: crate::plans::UrgencyPolicy::default(),
        total_shares: session.runtime_total_shares(),
        max_order_qty_limit: session.runtime_max_order_qty_limit(),
        tick_size_cents: session.runtime_tick_size_cents(),
        stock_codes: session.runtime_stock_codes(),
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 240;
    spec.initial_shares = vec![vec![vec![1_000], vec![400]]];
    spec.initial_invested = vec![vec![vec![1_000_000], vec![400_000]]];
    spec.initial_cash_override = Some(vec![vec![1_000_000, 1_600_000]]);
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
    spec.root_projection = Some(Arc::new(projection));
    spec.library = Some(session.runtime_library_handle().1);
    spec.config = Arc::new(setup.config);
    let profile = |next_due| NpcProfile {
        strategy: crate::strategy::StrategyData::inst(
            crate::strategy::TargetPolicy::Fixed(Money::from_cents(1_000)),
            0.01,
            700,
        ),
        seed: 23,
        next_due,
        attention_base: 1.0,
        retail_style: None,
    };
    spec.decision = Some(DecisionSetup {
        profiles: vec![profile(u64::MAX), profile(2)],
        attention_gap_ticks: 0,
    });
    World::spawn(spec).expect("真实组线程与交易部")
}

fn install_buy_assessment(world: &mut World) {
    world.inject_review_assessments_for_test(
        OWNER,
        std::collections::BTreeMap::from([(
            StockId(0),
            crate::plans::CandidateAssessment::Scored {
                score: crate::plans::SignalScore::new(10_000).expect("合法评分"),
                used_weight_bp: 10_000,
                excluded: Vec::new(),
            },
        )]),
    );
    world.inject_observation_minutes_for_test(0, vec![(0, 1_000), (1, 1_000)]);
}

#[test]
fn price_improvement_refund_is_counted_once_in_root_equity_across_real_ticks() {
    let mut world = price_improvement_world();
    assert!(world.public_views()[0].last_price == 1_000);
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Sell,
            routed: RoutedPrice::Fixed(1_020),
            purse_reference: 1_020,
            qty: 100,
        }],
        Vec::new(),
    );
    world.run_ticks(1).expect("先挂真实对手卖单");
    install_buy_assessment(&mut world);
    world.run_ticks(1).expect("机构买入后部分成交");
    let (resting, _) = world.collect_desk_resting().expect("实际交易部占用");
    let order = resting[0]
        .iter()
        .find(|order| order.owner == (0, 1))
        .expect("机构未成交部分仍在交易部")
        .clone();
    assert_eq!(order.price, 1_040);
    assert_eq!(order.filled_qty, 100);
    assert_eq!(order.qty, 600);
    assert_eq!(order.purse, 624_006);
    for tick in [3, 4] {
        install_buy_assessment(&mut world);
        world.run_ticks(1).expect("跨越真实交易拍");
        assert_eq!(world.global.committed_tick, tick);
        let accounts = world.snapshot_accounts().expect("同收束点组账户");
        let account = &accounts[0].1[1];
        let (resting, _) = world.collect_desk_resting().expect("同收束点交易部占用");
        let actual = resting[0]
            .iter()
            .find(|order| order.owner == (0, 1))
            .expect("部分成交子单仍在途");
        assert_eq!(actual, &order, "不得以撤单或空票绕过退款读面");
        assert_eq!(account.cash, 873_493);
        assert_eq!(account.shares[0], 500);
        assert_eq!(account.frozen_today[0], 100);
        assert_eq!(account.buy_purse_held_cents, order.purse);
        let true_equity = account.cash + order.purse + i64::from(account.shares[0]) * 1_020;
        assert_eq!(true_equity, 2_007_499);
        let records = world
            .collect_open_derivations()
            .expect("生产 root 观察入口");
        let record = records[0]
            .3
            .iter()
            .find(|record| record.account == OWNER)
            .expect("机构派生记录");
        assert_eq!(
            record.personal_out.belief.experience().peak_equity,
            Some(Money::from_cents(2_007_499)),
            "tick={tick}：价优退款进入现金后，不能再次作为在途占用加回",
        );
    }
    let report = world.shutdown().expect("真实清簿并回收线程");
    assert_eq!(report.group_finals[0].accounts[1].buy_purse_held_cents, 0);
    assert_eq!(report.group_finals[0].accounts[1].cash, 1_497_499);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
}

fn assert_complete_cash_cut(world: &mut World) -> Vec<(GroupIdx, Vec<GroupAccount>)> {
    let accounts = world.snapshot_accounts().expect("已收束账户");
    let (resting, _) = world.collect_desk_resting().expect("已收束交易部在途单");
    for (group, rows) in &accounts {
        for (local, account) in rows.iter().enumerate() {
            let purse: i64 = resting
                .iter()
                .flatten()
                .filter(|order| {
                    order.owner == (*group, local as LocalIdx) && order.side == Side::Buy
                })
                .map(|order| order.purse)
                .sum();
            assert!(
                account.buy_purse_held_cents >= 0,
                "合法单次投递下占用不得负数"
            );
            assert_eq!(
                account.buy_purse_held_cents, purse,
                "同一完整 cut 占用必须等于所有股票/订单 purse 之和"
            );
            assert_eq!(cash_with_pending_at(account), account.cash + purse);
        }
    }
    accounts
}

fn external_buy(stock: StockIdx, price: i64, qty: u32) -> PendingOp {
    PendingOp {
        local: 0,
        stock,
        side: Side::Buy,
        routed: RoutedPrice::Fixed(price),
        purse_reference: price,
        qty,
    }
}

#[test]
fn external_orders_across_stocks_release_exactly_once_on_cancel() {
    let mut spec = basic_spec(1, 2, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 240;
    spec.stocks[1].initial_price = 2_000;
    let mut world = World::spawn(spec).expect("两股显式下单世界");
    world.broadcast_external_plan(
        vec![external_buy(0, 1_000, 200), external_buy(1, 2_000, 100)],
        Vec::new(),
    );
    world.run_ticks(1).expect("两股买单真实受理");
    let accounts = assert_complete_cash_cut(&mut world);
    assert!(accounts[0].1[0].buy_purse_held_cents > 0);
    assert_eq!(cash_with_pending_at(&accounts[0].1[0]), 1_000_000);
    let (resting, _) = world.collect_desk_resting().expect("真实委托身份");
    assert_eq!(resting[0].len(), 1);
    assert_eq!(resting[1].len(), 1);
    // 公开在途事实携带合成 OrderId，定向撤单必须取回 desk 本地 serial。
    world.broadcast_external_plan(
        Vec::new(),
        vec![(0, 1, desk_order_serial(resting[1][0].order_serial))],
    );
    world.run_ticks(1).expect("先撤第二股");
    let accounts = assert_complete_cash_cut(&mut world);
    assert_eq!(accounts[0].1[0].buy_purse_held_cents, resting[0][0].purse);
    assert_eq!(cash_with_pending_at(&accounts[0].1[0]), 1_000_000);
    world.broadcast_external_plan(
        Vec::new(),
        vec![(0, 0, desk_order_serial(resting[0][0].order_serial))],
    );
    world.run_ticks(1).expect("再撤第一股");
    let accounts = assert_complete_cash_cut(&mut world);
    assert_eq!(accounts[0].1[0].buy_purse_held_cents, 0);
    assert_eq!(accounts[0].1[0].cash, 1_000_000);
    world.run_ticks(1).expect("退款后跨拍");
    assert_eq!(
        assert_complete_cash_cut(&mut world),
        accounts,
        "不能重复退款"
    );
    world.shutdown().expect("清理世界");
}

#[test]
fn available_budget_excludes_held_purse_even_when_total_cash_is_sufficient() {
    let mut spec = basic_spec(1, 2, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 240;
    let mut world = World::spawn(spec).expect("预算隔离世界");
    world.broadcast_external_plan(vec![external_buy(0, 1_000, 700)], Vec::new());
    world.run_ticks(1).expect("先扣出第一笔买单预算");
    let before = assert_complete_cash_cut(&mut world);
    assert_eq!(cash_with_pending_at(&before[0].1[0]), 1_000_000);
    assert!(before[0].1[0].cash < 300_000);
    world.broadcast_external_plan(vec![external_buy(1, 1_000, 300)], Vec::new());
    world.run_ticks(1).expect("后单按可用现金拒绝，非引擎失败");
    assert_eq!(assert_complete_cash_cut(&mut world), before);
    assert!(world
        .report_log
        .last()
        .expect("本拍事实")
        .group_rejections
        .iter()
        .any(|receipt| receipt.owner == (0, 0)
            && receipt.kind == crate::runtime::ReceiptKind::InsufficientCash));
    let report = world.shutdown().expect("最终撤单退回");
    assert_eq!(report.group_finals[0].accounts[0].cash, 1_000_000);
    assert_eq!(report.group_finals[0].accounts[0].buy_purse_held_cents, 0);
}

#[test]
fn rejected_and_expired_orders_leave_no_purse_in_the_group_cut() {
    for expire in [false, true] {
        let mut spec = basic_spec(1, 1, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
        spec.ticks_per_day = 240;
        spec.order_ttl_ticks = if expire { 1 } else { 100 };
        let mut world = World::spawn(spec).expect("拒收或到期世界");
        world.broadcast_external_plan(
            vec![external_buy(0, if expire { 1_000 } else { 1_100 }, 100)],
            Vec::new(),
        );
        world.run_ticks(1).expect("真实受理或笼子拒收");
        if !expire {
            assert!(world.report_log[0]
                .desk_receipts
                .iter()
                .any(|receipt| receipt.kind == crate::runtime::ReceiptKind::PriceCageExceeded));
        } else {
            assert!(assert_complete_cash_cut(&mut world)[0].1[0].buy_purse_held_cents > 0);
        }
        world.run_ticks(2).expect("到期窗口后真实收束");
        let accounts = assert_complete_cash_cut(&mut world);
        assert_eq!(accounts[0].1[0].cash, 1_000_000);
        assert_eq!(accounts[0].1[0].buy_purse_held_cents, 0);
        assert_eq!(accounts[0].1[0].foreign_active[0], 0);
        world.shutdown().expect("清理世界");
    }
}

#[test]
fn failed_tick_restores_cash_and_purse_before_replaying_a_partial_fill() {
    for failing_round in [0, 1] {
        let mut spec = basic_spec(
            2,
            1,
            1,
            Box::new(|_| {
                Box::new(|tick, round| TickPlan {
                    continuation: tick == 2 && round == 0,
                    ..TickPlan::default()
                })
            }),
        );
        spec.ticks_per_day = 240;
        let mut world = World::spawn(spec).expect("回滚真实成交世界");
        world.send_external_plan_grouped(
            vec![
                vec![PendingOp {
                    local: 0,
                    stock: 0,
                    side: Side::Sell,
                    routed: RoutedPrice::Fixed(1_020),
                    purse_reference: 1_020,
                    qty: 100,
                }],
                Vec::new(),
            ],
            vec![Vec::new(), Vec::new()],
        );
        world.run_ticks(1).expect("对手盘建立");
        let before_accounts = assert_complete_cash_cut(&mut world);
        let before_resting = world.collect_desk_resting().expect("提交前盘口");
        let publication = world.publication();
        let prior = publication.load();
        world.set_validator(Box::new(move |evidence| {
            if evidence.round == failing_round {
                Err("注入买单成交后校验失败".to_owned())
            } else {
                Ok(())
            }
        }));
        world.send_external_plan_grouped(
            vec![Vec::new(), vec![external_buy(0, 1_040, 700)]],
            vec![Vec::new(), Vec::new()],
        );
        assert!(matches!(
            world.run_ticks(1),
            Err(RuntimeError::TickValidation { tick: 2, .. })
        ));
        assert!(Arc::ptr_eq(&prior, &publication.load()));
        assert_eq!(world.global.committed_tick, 1);
        assert_eq!(assert_complete_cash_cut(&mut world), before_accounts);
        assert_eq!(
            world.collect_desk_resting().expect("回滚后真实盘口"),
            before_resting
        );
        world.set_validator(Box::new(|_| Ok(())));
        world.send_external_plan_grouped(
            vec![Vec::new(), vec![external_buy(0, 1_040, 700)]],
            vec![Vec::new(), Vec::new()],
        );
        world.run_ticks(1).expect("重试成交成功");
        let accounts = assert_complete_cash_cut(&mut world);
        assert_eq!(accounts[1].1[0].shares[0], 1_100);
        assert_eq!(accounts[1].1[0].frozen_today[0], 100);
        assert!(accounts[1].1[0].buy_purse_held_cents > 0);
        assert!(
            world.report_log.last().expect("重试事实").rounds > 0,
            "确实经过 Continue 收束"
        );
        world.shutdown().expect("清理世界");
    }
}

#[test]
fn multiple_external_orders_preserve_cash_through_partial_full_fill_and_cancel() {
    let mut spec = basic_spec(2, 1, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 240;
    let mut world = World::spawn(spec).expect("多单多成交世界");
    let sell = |local, price| PendingOp {
        local,
        stock: 0,
        side: Side::Sell,
        routed: RoutedPrice::Fixed(price),
        purse_reference: price,
        qty: 100,
    };
    world.send_external_plan_grouped(
        vec![vec![sell(0, 1_010), sell(1, 1_020)], Vec::new()],
        vec![Vec::new(), Vec::new()],
    );
    world.run_ticks(1).expect("两笔真实对手单");
    assert_eq!(
        world.public_views()[0].max_buy_price,
        1_030,
        "首个对手卖价1010的实际买笼子上限"
    );
    world.send_external_plan_grouped(
        vec![
            Vec::new(),
            vec![external_buy(0, 1_030, 300), external_buy(0, 1_000, 100)],
        ],
        vec![Vec::new(), Vec::new()],
    );
    world
        .run_ticks(1)
        .expect("一单两次价优成交，同账户另有第二单");
    let accounts = assert_complete_cash_cut(&mut world);
    let (resting, _) = world.collect_desk_resting().expect("两笔非计划在途买单");
    let buys: Vec<_> = resting[0]
        .iter()
        .filter(|order| order.owner == (1, 0))
        .collect();
    assert_eq!(buys.len(), 2);
    assert_eq!(world.report_log[1].desk_trades.len(), 2);
    assert_eq!(
        world.report_log[1]
            .desk_trades
            .iter()
            .map(|trade| trade.qty)
            .sum::<u32>(),
        200
    );
    let low_order = buys
        .iter()
        .find(|order| order.price == 1_000)
        .expect("独立低价挂单")
        .order_serial;
    let charges = |world: &World| {
        world
            .report_log
            .iter()
            .flat_map(|record| &record.desk_trades)
            .filter(|trade| trade.taker == (1, 0))
            .map(|trade| trade.price * i64::from(trade.qty) + trade.taker_fees_cents)
            .sum::<i64>()
    };
    assert_eq!(
        cash_with_pending_at(&accounts[1].1[0]),
        1_000_000 - charges(&world)
    );
    world.send_external_plan_grouped(
        vec![vec![sell(0, 1_020)], Vec::new()],
        vec![Vec::new(), Vec::new()],
    );
    world.run_ticks(1).expect("高价子单余量完全成交");
    let accounts = assert_complete_cash_cut(&mut world);
    assert_eq!(accounts[1].1[0].frozen_today[0], 300);
    let (resting, _) = world.collect_desk_resting().expect("仅低价订单保留");
    assert_eq!(resting[0].len(), 1);
    assert_eq!(resting[0][0].order_serial, low_order);
    assert_eq!(accounts[1].1[0].buy_purse_held_cents, resting[0][0].purse);
    assert_eq!(
        cash_with_pending_at(&accounts[1].1[0]),
        1_000_000 - charges(&world)
    );
    world.send_external_plan_grouped(
        vec![Vec::new(), Vec::new()],
        vec![Vec::new(), vec![(0, 0, desk_order_serial(low_order))]],
    );
    world.run_ticks(1).expect("撤销最后一笔非计划挂单");
    let accounts = assert_complete_cash_cut(&mut world);
    assert_eq!(accounts[1].1[0].buy_purse_held_cents, 0);
    assert_eq!(accounts[1].1[0].cash, 1_000_000 - charges(&world));
    world.shutdown().expect("清理世界");
}

#[test]
fn retail_strategy_orders_share_the_same_entity_driven_purse_cut() {
    let mut spec = basic_spec(1, 2, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 240;
    spec.decision = Some(DecisionSetup {
        profiles: vec![NpcProfile {
            strategy: crate::strategy::StrategyData::retail(1.0, 100, 0.0),
            seed: 42,
            next_due: 1,
            attention_base: 1.0,
            retail_style: None,
        }],
        attention_gap_ticks: 0,
    });
    let mut world = World::spawn(spec).expect("真实散户策略世界");
    for _ in 0..16 {
        world.run_ticks(1).expect("散户自主决策真实交易拍");
        assert_complete_cash_cut(&mut world);
    }
    let buy_orders = world
        .report_log
        .iter()
        .flat_map(|tick| &tick.desk_lifecycle)
        .filter(|fact| {
            fact.side == Side::Buy && fact.kind == crate::runtime::protocol::LifecycleKind::Accepted
        })
        .count();
    assert!(buy_orders > 0, "必须触达实际散户买单，不能空跑");
    let report = world.shutdown().expect("散户最终清簿");
    assert_eq!(report.group_finals[0].accounts[0].buy_purse_held_cents, 0);
}
