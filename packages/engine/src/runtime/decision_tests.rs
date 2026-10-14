//! 批 4 决策内联测试（G 批 4 验收判据）：decide_data 内联直调、到期调度
//! （attention 简化 due 检查）、观察基快照（上一收束点公共态）、市价路径
//! 意图转换、T+1 日界解冻、决策确定性（同种子同输出）、严格所有权守恒。

use super::decision::DecisionSetup;
use super::decision::{
    build_market_view, build_self_view, decide_account, intent_to_pending, NpcProfile,
};
use super::group::{GroupAccount, GroupScript, PlaceSpec, TickPlan};
use super::protocol::{RoutedPrice, TickSnapshot};
use super::skeleton_tests::basic_spec;
use super::world::World;
use super::GroupIdx;
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use crate::strategy::StrategyData;
use crate::strategy::{Intent, LimitPrice, MarketView};
use crate::StockId;
use std::collections::BTreeMap;

fn snapshot_one_stock(last: i64, upper: i64) -> TickSnapshot {
    TickSnapshot {
        tick: 1,
        market_minute: 0,
        stocks: vec![super::protocol::StockPublic {
            last_price: last,
            best_bid: None,
            best_ask: None,
            max_buy_price: upper,
            daily_upper_limit: upper,
            min_sell_price: (last / 10).max(1),
            recent_prices: vec![last],
            minute_closes: Vec::new(),
            volume_today: 0,
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        }],
    }
}

// ── 意图转换（含市价路径与 Cancel 登记）──

#[test]
fn intent_conversion_routes_market_and_symbol_prices() {
    let mut market = MarketView {
        stocks: BTreeMap::new(),
        tick: 1,
        market_minute: 0,
    };
    market.stocks.insert(
        StockId(0),
        crate::strategy::StockView {
            best_bid: None,
            best_ask: None,
            last_price: Money::from_cents(1000),
            max_buy_price: Money::from_cents(1020),
            daily_upper_limit: Money::from_cents(1100),
            min_sell_price: Money::from_cents(980),
            recent_prices: vec![],
            recent_market_minute_prices: vec![],
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        },
    );
    // Fixed：路由原价、purse 参考价=限价。
    let op = intent_to_pending(
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1005)),
            qty: 100,
        },
        &market,
    )
    .expect("Fixed 转换");
    assert_eq!(op.routed, RoutedPrice::Fixed(1005));
    assert_eq!(op.purse_reference, 1005);
    // 符号价：路由符号、purse 参考价=日涨停（F §1.5 最坏情况）。
    let op = intent_to_pending(
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: LimitPrice::Highest,
            qty: 100,
        },
        &market,
    )
    .expect("符号价转换");
    assert_eq!(op.routed, RoutedPrice::Highest);
    assert_eq!(op.purse_reference, 1100, "purse 最坏情况=日涨停");
    // 市价：涨跌停保护、余量不进簿、purse=日涨停（批 4 接入点）。
    let op = intent_to_pending(
        Intent::PlaceMarket {
            code: StockId(0),
            side: Side::Sell,
            qty: 100,
        },
        &market,
    )
    .expect("市价转换");
    assert_eq!(op.routed, RoutedPrice::Market);
    assert_eq!(op.purse_reference, 1100);
    // Cancel：显式登记（None），不静默执行。
    assert!(intent_to_pending(
        Intent::Cancel {
            code: StockId(0),
            id: crate::orderbook::OrderId(7)
        },
        &market,
    )
    .is_none());
}

// ── 决策确定性与到期调度 ──

#[test]
fn decide_account_is_deterministic_per_seed_tick_and_reschedules() {
    let config = GameConfig::proposed_defaults();
    let snapshot = snapshot_one_stock(1000, 1100);
    let account = GroupAccount::new(1_000_000, 1);
    let mut profile = NpcProfile {
        strategy: StrategyData::retail(1.0, 100, 1.0),
        seed: 42,
        next_due: 0,
    };
    let run = |p: &mut NpcProfile| decide_account(p, 0x1234, 1, 0, 0, &snapshot, &account, &config);
    let mut a = profile.clone();
    let mut b = profile.clone();
    let (ops_a, due_a, _) = run(&mut a);
    let (ops_b, due_b, _) = run(&mut b);
    assert!(due_a && due_b);
    assert_eq!(ops_a.len(), ops_b.len(), "同种子同 tick 同输出");
    assert_eq!(ops_a, ops_b, "decide_data 纯函数 + 种子化 RNG ⇒ 决策确定");
    // 重排：gap=0 ⇒ next_due = tick+1（下一 tick 再决策）。
    assert_eq!(a.next_due, 2);
    // 未到期：tick < next_due ⇒ 不决策。
    let (_, due_again, _) = decide_account(&mut a, 0x1234, 1, 0, 0, &snapshot, &account, &config);
    assert!(!due_again, "同 tick 不重复决策");
}

#[test]
fn attention_gap_suppresses_non_due_ticks() {
    // gap=100：tick1 决策一次后，tick2..5 均未到期 ⇒ decide_calls_total 停在 1。
    let config = GameConfig::proposed_defaults();
    let snapshot = snapshot_one_stock(1000, 1100);
    let account = GroupAccount::new(1_000_000, 1);
    let mut profile = NpcProfile {
        strategy: StrategyData::retail(0.0, 100, 0.0),
        seed: 7,
        next_due: 0,
    };
    let mut calls = 0;
    for tick in 1..=5u64 {
        let (_, due, _) =
            decide_account(&mut profile, 0, tick, 0, 100, &snapshot, &account, &config);
        calls += u64::from(due);
    }
    assert_eq!(calls, 1, "只有首个 tick 到期（gap 内静默等待）");
}

// ── 视图构建（观察基 = 上一收束点公共态）──

#[test]
fn market_and_self_views_mirror_snapshot_and_account() {
    let snapshot = snapshot_one_stock(1000, 1100);
    let view = build_market_view(&snapshot);
    let sv = view.stocks.get(&StockId(0)).expect("StockId 直映射");
    assert_eq!(sv.last_price.cents(), 1000);
    assert_eq!(sv.daily_upper_limit.cents(), 1100);
    assert_eq!(view.tick, 1);
    let mut account = GroupAccount::new(500_000, 1);
    account.shares[0] = 800;
    account.frozen_today[0] = 300;
    let own = build_self_view(&account);
    assert_eq!(own.cash.cents(), 500_000);
    let pos = own.positions.get(&StockId(0)).expect("持仓视图");
    assert_eq!(pos.qty, 800);
    assert_eq!(pos.sellable_qty, 500, "T+1 冻结反映进可卖视图");
}

// ── 世界级：内联决策驱动真实订单流 ──

/// 找一个"首决策必产生买入意图"的种子（确定性构造，避免测试依赖随机路径）。
fn seed_that_buys() -> u64 {
    let config = GameConfig::proposed_defaults();
    let snapshot = snapshot_one_stock(1000, 1100);
    let account = GroupAccount::new(1_000_000, 1);
    for seed in 0..200u64 {
        let mut profile = NpcProfile {
            strategy: StrategyData::retail(1.0, 100, 1.0),
            seed,
            next_due: 0,
        };
        let (ops, due, _) = decide_account(&mut profile, 0, 1, 0, 0, &snapshot, &account, &config);
        if due && ops.iter().any(|op| op.side == Side::Buy && op.qty >= 100) {
            return seed;
        }
    }
    panic!("200 个种子内应存在产生买入的种子（decide_data 到达率 1.0）");
}

#[test]
fn inline_decision_drives_orders_and_conservation_holds() {
    let seed = seed_that_buys();
    let setup = DecisionSetup {
        profiles: vec![NpcProfile {
            strategy: StrategyData::retail(1.0, 100, 1.0),
            seed,
            next_due: 0,
        }],
        attention_gap_ticks: 0,
    };
    let mut spec = basic_spec(
        1,
        1,
        1,
        Box::new(|_g: GroupIdx| -> GroupScript { Box::new(|_t, _r| TickPlan::default()) }),
    );
    // 初始零持仓：与种子搜索的单元上下文一致（持仓影响策略分类）。
    spec.initial_shares = vec![vec![vec![0u32]]];
    spec.decision = Some(setup);
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(3).expect("3 tick 收敛");
    let resting_tick1 = world.report_log[0].desk_resting.clone();
    let report = world.shutdown().expect("关停");
    let group = &report.group_finals[0];
    // 每 tick 到期一次（gap=0）。
    assert_eq!(group.decide_calls_total, 3, "attention 简化：每 tick due");
    // 守恒：现金总量 + 费用桶 = 初始（严格所有权下的决策-扣除-回流闭环）。
    let cash: i64 = group.accounts.iter().map(|a| a.cash).sum();
    let fees: i64 = report.desk_finals.iter().map(|d| d.fee_sink).sum();
    assert_eq!(cash + fees, 1_000_000, "决策路径守恒");
    // 决策确已产生订单流：tick1 收束时交易部挂单 > 0（符号价买单挂 @笼子上限）。
    assert!(
        resting_tick1.iter().any(|(_, r)| *r > 0),
        "决策产生的订单应到达交易部挂簿：{resting_tick1:?}"
    );
}

#[test]
fn snapshot_reflects_last_converged_desk_state() {
    // tick1 成交后，下一 tick 的决策观察基应反映成交价（last_price 移动）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(PlaceSpec {
                    local: 1,
                    stock: 0,
                    side: Side::Sell,
                    price: 1000,
                    qty: 100,
                });
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: Side::Buy,
                    price: 1005,
                    qty: 100,
                });
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let views = world.public_views();
    assert_eq!(views[0].last_price, 1000, "成交价=maker 价 1000 进入观察基");
    assert_eq!(views[0].volume_today, 100);
    assert!(
        views[0].recent_prices.contains(&1000),
        "tick 收盘价历史进入观察基：{:?}",
        views[0].recent_prices
    );
}

// ── T+1 日界解冻（组线程接入）──

#[test]
fn t1_frozen_unfreezes_at_day_boundary_in_group_thread() {
    // ticks_per_day=4：tick1 买入成交（冻结）；tick5 为新交易日首 tick ⇒ 解冻后可卖。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            match tick {
                1 => {
                    plan.places.push(PlaceSpec {
                        local: 1,
                        stock: 0,
                        side: Side::Sell,
                        price: 1000,
                        qty: 100,
                    });
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Buy,
                        price: 1000,
                        qty: 100,
                    });
                }
                // tick2..4 仍在当日：T+1 冻结中，卖出应被组端 skip。
                2 => {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Sell,
                        price: 1000,
                        qty: 100,
                    });
                }
                // tick5 跨日：解冻后卖出成功（acct1 提供对手买单）。
                5 => {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Sell,
                        price: 1000,
                        qty: 100,
                    });
                    plan.places.push(PlaceSpec {
                        local: 1,
                        stock: 0,
                        side: Side::Buy,
                        price: 1000,
                        qty: 100,
                    });
                }
                _ => {}
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.initial_shares = vec![vec![vec![0u32], vec![1_000u32]]];
    spec.ticks_per_day = 4;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(5).expect("5 tick 收敛");
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.frozen_today[0], 0, "日界解冻");
    assert_eq!(a0.shares[0], 0, "tick2 卖出被 T+1 拦、tick5 解冻后卖出");
    assert_eq!(
        report.group_finals[0].skipped_orders, 1,
        "当日卖出显式 skip"
    );
}
