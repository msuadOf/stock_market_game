//! B2 件 3：`DeskTradeFact` 双腿实收费用三分项（对照 18725d07 旧收据
//! `PersonalTradeConfirmation.actual_fees` 的语义基线——个人域交割单的
//! 费用数据源合同）。
//!
//! 语义合同：
//! - 扣费处直接记录双腿**实收**三分项（佣金/印花税/过户费），按卖/买
//!   归户：`maker` 恒卖腿（封顶分配实收）、`taker` 恒买腿（边际佣金+
//!   边际过户、印花税恒 0——A 股印花税单边卖出）；
//! - 合计字段 `maker_fees_cents`/`taker_fees_cents` ≡ 三分项之和（守恒
//!   审计口径不变）；
//! - 手算 oracle 六形态：最低佣金、比例佣金、部分成交边际（telescoping）、
//!   双向（主动买/主动卖）、竞价清算成交。

use super::coordinator::Validator;
use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::skeleton_tests::basic_spec;
use super::world::{World, WorldSpec};
use super::GroupIdx;
use crate::config::GameConfig;
use crate::fees::FeeComponents;
use crate::money::Money;
use crate::orderbook::Side;
use std::sync::Arc;

/// 无笼子（隔离价格带变量）；佣金 0.00025/最低 500、印花 0.0005、
/// 过户 0.01‰（`A_SHARE_TRANSFER_FEE_RATE` 常量）。
fn config_no_cage() -> Arc<GameConfig> {
    Arc::new(
        GameConfig::new(
            0.000_25,
            Money::from_cents(500),
            0.000_5,
            0.10,
            0.10,
            false,
            100,
            Money::from_cents(100_000_000_000),
        )
        .expect("测试配置合法"),
    )
}

fn sell(local: u32, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec { local, stock: 0, side: Side::Sell, price, qty }
}

fn buy(local: u32, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec { local, stock: 0, side: Side::Buy, price, qty }
}

/// 累计全部已提交 tick 的双腿成交事实。
fn all_trades(world: &World) -> Vec<super::protocol::DeskTradeFact> {
    world
        .report_log
        .iter()
        .flat_map(|record| record.desk_trades.iter().copied())
        .collect()
}

fn assert_split(
    fact: &super::protocol::DeskTradeFact,
    sell_split: (i64, i64, i64),
    buy_split: (i64, i64, i64),
) {
    let maker = &fact.maker_fee_split;
    let taker = &fact.taker_fee_split;
    assert_eq!(
        (
            maker.commission.cents(),
            maker.stamp_tax.cents(),
            maker.transfer_fee.cents()
        ),
        sell_split,
        "卖腿（maker）三分项"
    );
    assert_eq!(
        (
            taker.commission.cents(),
            taker.stamp_tax.cents(),
            taker.transfer_fee.cents()
        ),
        buy_split,
        "买腿（taker）三分项"
    );
    // 合计口径自洽（守恒审计输入不变）。
    assert_eq!(
        fact.maker_fees_cents,
        (maker.commission.cents() + maker.stamp_tax.cents() + maker.transfer_fee.cents()),
        "maker 合计=三分项之和"
    );
    assert_eq!(
        fact.taker_fees_cents,
        (taker.commission.cents() + taker.stamp_tax.cents() + taker.transfer_fee.cents()),
        "taker 合计=三分项之和"
    );
}

#[test]
fn taker_buy_leg_records_minimum_commission_and_transfer_only() {
    // 主动买（taker=买方 local0 吃卖方 maker local1）：额 1000×100=100_000
    // ——佣金触发最低 500（25<500），过户 round(100_000×0.01‰)=1，
    // 印花税恒 0；卖腿 nominal{500, 50, 1}（封顶远未触）→ 实收同名义。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1000, 100), buy(0, 1005, 100)];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("tick1");
    let trades = all_trades(&world);
    let _ = world.shutdown();
    assert_eq!(trades.len(), 1);
    assert_split(&trades[0], (500, 50, 1), (500, 0, 1));
}

#[test]
fn taker_sell_leg_normalizes_maker_as_seller_side() {
    // 主动卖（taker=卖方 local1 吃买方 maker local0）：归一化后
    // maker 恒卖腿/taker 恒买腿——费用三分项按归一化身份分账（Sol 六审
    // 修复 1 的口径：主动卖时双方费用不得互换）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![buy(0, 995, 100), sell(1, 995, 100)];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("tick1");
    let trades = all_trades(&world);
    let _ = world.shutdown();
    assert_eq!(trades.len(), 1);
    // 成交价=maker（买方）价 995：卖腿 nominal{500, round(99_500×0.0005)=50,
    // round(99_500×0.00001)=1}；买腿边际{500, 0, 1}。
    assert_split(&trades[0], (500, 50, 1), (500, 0, 1));
}

#[test]
fn partial_fills_telescope_buy_commission_by_delta() {
    // 比例佣金域（额 2_200_000 → 佣金 550>最低 500）+ 两笔部分成交边际：
    // 买方限价 1100 买 2000 股，分两 tick 各吃卖方 1000 股。
    // 首笔：commission=delta(0→1_100_000)=max(275,500)=500（仍最低）；
    // 第二笔：delta(1_100_000→2_200_000)=550−500=50（telescoping——
    // 累计口径差，不逐笔重吃最低）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            match tick {
                1 => plan.places = vec![sell(1, 1100, 1000), buy(0, 1100, 2000)],
                2 => plan.places = vec![sell(2, 1100, 1000)],
                _ => {}
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 4, Box::new(script));
    spec.config = config_no_cage();
    spec.initial_cash = 100_000_000;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("2 tick");
    let trades = all_trades(&world);
    let _ = world.shutdown();
    assert_eq!(trades.len(), 2, "两笔部分成交各一条事实");
    // 首笔：额 1_100_000——买{500,0,round(11)}=transfer 11；卖 nominal
    // {500, 550, 11}。
    assert_split(&trades[0], (500, 550, 11), (500, 0, 11));
    // 第二笔：额 1_100_000——买边际 commission=550−500=50、transfer=
    // delta(11)=11；卖腿（新卖单 local2）nominal{500,550,11}。
    assert_split(&trades[1], (500, 550, 11), (50, 0, 11));
}

#[test]
fn proportional_commission_exact_bankers_rounding_on_large_notional() {
    // 单笔大额：2_000_000×1 股不可行（lot=100）——改 1100 价×2000 股
    // 单吃（一档 2000 股）：额 2_200_000 → 佣金 550、印花 1100、过户 22
    // （银行家舍入逐位手算）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1100, 2000), buy(0, 1100, 2000)];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    // 名册资源足额（220 万元对价现金 + 卖方 2000 股持仓——basic_spec
    // 缺省 1000 股/人会使卖单因可卖不足被组端整单拒收）。
    spec.initial_cash = 10_000_000_000;
    spec.initial_shares = vec![vec![vec![10_000_u32; 1]; 3]; 1];
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("tick1");
    let trades = all_trades(&world);
    let _ = world.shutdown();
    assert_eq!(trades.len(), 1);
    assert_split(&trades[0], (550, 1100, 22), (550, 0, 22));
}

#[test]
fn opening_auction_clearing_records_both_leg_fee_splits() {
    // 竞价清算成交（单一价格原则）：双腿费用照记（开盘 565 分钟
    // 归一在件 2 已测，此处聚焦费用面）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1000, 100), buy(0, 1000, 100)];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    spec.ticks_per_day = 30;
    spec.auction_ticks = 6;
    spec.closing_auction_ticks = 3;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(4).expect("tick4 开盘清算");
    let trades = all_trades(&world);
    let _ = world.shutdown();
    assert_eq!(trades.len(), 1, "清算成交恰一笔");
    assert_split(&trades[0], (500, 50, 1), (500, 0, 1));
}

#[test]
fn aborted_tick_facts_never_reach_the_trade_log() {
    // Abort 重试零泄漏：注入失败的 tick 成交事实不进 report_log（tick
    // 级 Abort 回滚），重跑后恰一组事实（Continue 累积器与 Abort 窗口
    // 的合同——receipt 派生的去重前提）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1000, 100), buy(0, 1005, 100)];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    let mut world = World::spawn(spec).expect("世界构造");
    let fail: Validator = Box::new(|ev| {
        if ev.tick == 1 { Err("三分项 Abort 注入".to_owned()) } else { Ok(()) }
    });
    world.set_validator(fail);
    assert!(world.run_ticks(1).is_err(), "tick1 Abort");
    world.set_validator(Box::new(|_| Ok(())));
    world.run_ticks(1).expect("tick1 重跑");
    let trades = all_trades(&world);
    let _ = world.shutdown();
    assert_eq!(trades.len(), 1, "Abort 成交零泄漏，重跑恰一组");
    assert_split(&trades[0], (500, 50, 1), (500, 0, 1));
}

/// 便捷：`FeeComponents::ZERO` 与三分项合计的口径（供个人域 receipt 派生
/// 测试复用的基础断言——本文件锚定字段语义）。
#[test]
fn fee_components_zero_and_total_delegate_neutral_domain() {
    let split = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::from_cents(50),
        transfer_fee: Money::from_cents(1),
    };
    assert_eq!(split.total().unwrap(), Money::from_cents(551));
    assert_eq!(FeeComponents::ZERO.total().unwrap(), Money::ZERO);
}
