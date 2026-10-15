//! 批 3 交易部语义测试（G 批 3 验收判据）：价格档序与同价 FIFO、maker 价、
//! 部分成交、涨跌停带（闭区间）、价格笼子（连续竞价限定）、单笔上限（主板/
//! 创业板差异）、T+1、卖腿费用封顶、相位门禁与开盘竞价清算、到期撤销、
//! 市价余量不进簿、现金守恒。
//!
//! 对照现引擎（脊树）语义：撮合 `orderbook.rs place_inner`、带/笼 `market.rs`、
//! 上限 `session.rs SecurityCategory`、费用 `config.rs`/`transition.rs`、相位
//! `pipeline/phase.rs`。

use super::group::{CancelSpec, GroupScript, PlaceSpec, TickPlan};
use super::skeleton_tests::basic_spec;
use super::world::{DeskStockSpec, World, WorldSpec};
use super::GroupIdx;
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use crate::StockExchange;
use std::sync::Arc;
use std::time::Duration;

fn config_with_cage(cage: bool) -> Arc<GameConfig> {
    Arc::new(
        GameConfig::new(
            0.000_25,
            Money::from_cents(500),
            0.000_5,
            0.10,
            0.10,
            cage,
            100,
            Money::from_cents(1_000_000_000_000),
        )
        .expect("测试配置合法"),
    )
}

/// 拍卖形态世界：ticks_per_day=30、auction_ticks=6（前 2/3=4 个申报 tick）、
/// closing=3。day_tick 1..3 CallAuction、4..5 PreOpen、6..26 Continuous、
/// 27..29 ClosingAuction。
fn auction_spec(
    n_groups: usize,
    n_stocks: usize,
    accounts_per_group: usize,
    script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
) -> WorldSpec {
    let mut spec = basic_spec(n_groups, n_stocks, accounts_per_group, script_for);
    spec.ticks_per_day = 30;
    spec.auction_ticks = 6;
    spec.closing_auction_ticks = 3;
    spec
}

fn buy(local: u32, stock: u16, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec {
        local,
        stock,
        side: Side::Buy,
        price,
        qty,
    }
}

fn sell(local: u32, stock: u16, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec {
        local,
        stock,
        side: Side::Sell,
        price,
        qty,
    }
}

fn place_at(_tick: u64, places: Vec<PlaceSpec>) -> TickPlan {
    // 调用方按 tick 分派（match tick）；本 helper 只装计划。
    let mut plan = TickPlan::default();
    plan.places = places;
    plan
}

// ── 价格时间优先 ──

#[test]
fn matching_walks_best_price_first_and_fills_at_maker_price() {
    // 三档卖单 1010/1005/1000；买单限价 1010 全吃：逐档从最优（1000）起，
    // 成交价恒取 maker 价（价格改善归买方）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(
                1,
                vec![
                    sell(1, 0, 1010, 100),
                    sell(1, 0, 1005, 100),
                    sell(1, 0, 1000, 100),
                ],
            ),
            2 => place_at(2, vec![buy(0, 0, 1010, 300)]),
            _ => TickPlan::default(),
        })
    };
    // 关笼子，避免 1010 触发买方上限（参考价 1000 时笼子上限 1020——1010 合法，
    // 但本测试聚焦档序，关闭以隔离变量）。
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(script));
        spec.config = config_with_cage(false);
        spec
    })
    .expect("世界构造");
    world.run_ticks(2).expect("2 tick 收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.market.trade_count, 3, "三档各一笔");
    assert_eq!(desk.market.volume_today, 300);
    assert_eq!(desk.market.last_price, 1010, "末笔成交价=最高档 maker 价");
    // 买方支付 = 1000×100 + 1005×100 + 1010×100 = 301_500 + 费用。
    let acct0 = &report.group_finals[0].accounts[0];
    // 边际费用口径（fee_delta）：最低佣金只在首笔吃满（后续 delta=0），
    // 过户费逐笔累加 1+1+1。
    let fees_buy = 501 + 1 + 1;
    assert_eq!(acct0.cash, 1_000_000 - 301_500 - fees_buy, "逐档累加扣费");
    assert_eq!(acct0.shares[0], 1_300, "三档全得");
}

#[test]
fn same_price_level_fills_fifo_by_arrival() {
    // 同价两笔卖单（先 1050 后 1051），买单同价部分成交 ⇒ 先到先成交。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(0, 0, 1000, 100), sell(1, 0, 1000, 300)]),
            2 => place_at(2, vec![buy(2, 0, 1000, 300)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 3, Box::new(script))).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let report = world.shutdown().expect("关停");
    // FIFO：先到 acct0（100 股）全成；后到 acct1 成交 200、余 100 回流。
    let a0 = &report.group_finals[0].accounts[0];
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a0.shares[0], 900, "先到先全成");
    assert_eq!(a1.shares[0], 800, "后到部分成交（200）+余股（100）回流");
    assert!(a0.frozen_today[0] == 0 && a1.frozen_today[0] == 0);
}

#[test]
fn partial_fill_rests_remainder_at_taker_limit() {
    // 买单 300 限价 1000，卖方只有 100 ⇒ 成交 100，余 200 挂簿（限价）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100), buy(0, 0, 1000, 300)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(1).expect("收敛");
    // tick1 收束后：200 股买单挂簿（在 purse 上限内）。
    // Drain 回流后：买方实付 100 股对价+费用，卖方净得。
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.market.volume_today, 100, "部分成交 100");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "得 100 股");
    // 扣除=purse（300_503=300_000+500+3），退款=purse−charge=200_002。
    assert_eq!(
        a0.cash,
        1_000_000 - 300_503 + 200_002,
        "部分成交扣费+余量退款"
    );
}

// ── 涨跌停带（闭区间）与价格笼子 ──

#[test]
fn price_band_is_closed_interval_and_cage_only_continuous() {
    // 笼子关闭：买 1100（涨停边界）受理、1101 拒；卖 900（跌停边界）受理、899 拒。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![buy(0, 0, 1101, 100)]),
            2 => place_at(2, vec![buy(0, 0, 1100, 100)]),
            3 => place_at(3, vec![sell(1, 0, 899, 100)]),
            4 => place_at(4, vec![sell(1, 0, 900, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(script));
        spec.config = config_with_cage(false);
        spec
    })
    .expect("世界构造");
    world.run_ticks(4).expect("收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.skipped_orders, 2, "带外 1101/899 拒收");
    // 边界单合法：买 1100 挂簿，卖 900 与之交叉 ⇒ 以 maker 价 1100 成交。
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "边界买单成交");
    assert_eq!(a0.cash, 1_000_000 - 110_501, "买方付 maker 价+费用");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.cash, 1_000_000 + 109_444, "卖方净得（110_000−551）");
}

#[test]
fn price_cage_rejects_beyond_wider_bound_in_continuous() {
    // 笼子开启（连续竞价）：买上限 = max(参考×102%, 参考+10 档)=1020；
    // 卖下限 = min(98%, 参考−10 档)=980。初始 1000、空簿参考=最新价。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![buy(0, 0, 1021, 100)]),
            2 => place_at(2, vec![buy(0, 0, 1020, 100)]),
            3 => place_at(3, vec![sell(1, 0, 979, 100)]),
            4 => place_at(4, vec![sell(1, 0, 980, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(4).expect("收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    // tick2 的 1020 挂簿后，卖方参考价上移 1020 ⇒ 笼子下界=1000：
    // 979 与 980 双双低于下界被拒（边界随参考价移动——对齐连续竞价逐笔解析）。
    assert_eq!(desk.skipped_orders, 3, "笼子外 1021/979/980 拒收");
    assert_eq!(
        report.group_finals[0].accounts[0].cash, 1_000_000,
        "拒收退款复原"
    );
}

// ── 单笔上限（类别差异）──

#[test]
fn order_cap_differs_by_board() {
    // 创业板：限价单笔上限 30 万股（主板 100 万）。
    // 市价上限差异（15 万）经 RoutedPrice::Market 注入，批 4 决策层接线后覆盖
    // （登记待办）。
    let mut spec = basic_spec(
        1,
        2,
        1,
        Box::new(|_g| {
            Box::new(|tick, _round| {
                match tick {
                    // 创业板（stock 1）限价 30 万+1 ⇒ 拒（cap 300_000）。
                    1 => place_at(1, vec![buy(0, 1, 900, 300_100)]),
                    // 主板（stock 0）同量 ⇒ 受理（cap 1_000_000；band 内、资金足）。
                    2 => place_at(2, vec![buy(0, 0, 900, 300_100)]),
                    _ => TickPlan::default(),
                }
            })
        }),
    );
    spec.stocks = vec![
        DeskStockSpec::main_board(1000),
        DeskStockSpec {
            exchange: StockExchange::Shenzhen,
            initial_price: 1000,
            limit_bps: 2000,
            max_order_qty_limit: 300_000,
            max_order_qty_market: 150_000,
            initial_daily_bars: Vec::new(),
            initial_traded_days: 0,
        },
    ];
    spec.initial_cash = 30_000_000_000;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let report = world.shutdown().expect("关停");
    assert_eq!(
        report.desk_finals[1].skipped_orders, 1,
        "创业板超 30 万拒收"
    );
    assert_eq!(report.desk_finals[0].skipped_orders, 0, "主板 30 万合法");
    assert_eq!(report.desk_finals[0].resting.len(), 0, "Drain 清簿");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_000, "主板单无成交 Drain 回流（持仓=初始）");
    assert_eq!(a0.cash, 30_000_000_000, "purse 全额退款");
}

// ── T+1（组端）──

#[test]
fn t1_frozen_shares_cannot_sell_same_day() {
    // tick1：acct0 买 100（对手 acct1 卖）成交冻结；tick2：acct0 卖 100 ⇒ skip。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100), buy(0, 0, 1000, 100)]),
            2 => place_at(2, vec![sell(0, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    // acct0 初始零持仓：当日买入是其唯一可卖来源 ⇒ T+1 全额冻结。
    spec.initial_shares = vec![vec![vec![0u32], vec![1_000u32]]];
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.frozen_today[0], 100, "当日买入冻结");
    assert_eq!(a0.shares[0], 100);
    assert_eq!(a0.cash, 1_000_000 - 100_501, "买入实扣");
    assert_eq!(
        report.group_finals[0].skipped_orders, 1,
        "T+1 卖出被组端显式 skip"
    );
}

// ── 卖腿费用封顶（ADR-0017 顺序：佣金→印花→过户费）──

#[test]
fn seller_fee_capped_by_proceeds_in_priority_order() {
    // 极小额卖单（100 股 × 1 分 = 100 分 < 最低佣金 500 分）：实收费用封顶
    // = min(501, 100) = 100，全部为佣金；净得 0。
    // 挂单侧：买方限价 1 分。初始价 1000 ⇒ 跌停 900：1 分报价带外 ⇒ 需要用
    // 低初始价股票隔离（initial 10 分，band [9,11]，报价 1 分仍带外）。
    // ⇒ 改用直接费用函数单元验证（世界路径无法构造合法极小价）。
    let config = config_with_cage(false);
    let nominal = super::fees::FeeQuote::nominal(&config, Side::Sell, Money::from_cents(100))
        .expect("名义费用");
    assert_eq!(nominal.commission.cents(), 500, "最低佣金 500 分");
    let charged = super::fees::seller_charge(nominal, Default::default(), Money::from_cents(100))
        .expect("封顶分配");
    assert_eq!(charged.commission.cents(), 100, "佣金吃满 cap");
    assert_eq!(charged.stamp_tax.cents(), 0, "印花税次序在后无余量");
    assert_eq!(charged.transfer_fee.cents(), 0, "过户费最后");
    assert_eq!(charged.deliver_cash.cents(), 0, "净得 0");
    // 部分封顶：成交 400 分 < 名义 501 ⇒ cap=400，佣金 400、印花 0、净得 0。
    let charged2 = super::fees::seller_charge(nominal, Default::default(), Money::from_cents(400))
        .expect("部分封顶");
    assert_eq!(charged2.commission.cents(), 400);
    assert_eq!(charged2.deliver_cash.cents(), 0);
}

// ── 相位门禁 ──

#[test]
fn preopen_rejects_and_auction_requires_limit_orders() {
    // day_tick 4..5 为 PreOpen：拒收（PhaseRejected 回执）。
    // CallAuction（day_tick 1..3）：市价注入路径未开（批 4），以限价受理验证
    // 积累语义——tick1..3 申报只积累，tick4 进入 PreOpen 时开盘清算一次撮合。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![buy(0, 0, 1000, 100), sell(1, 0, 1000, 100)]),
            4 => place_at(4, vec![buy(0, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(auction_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(4).expect("收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    // tick1..3 积累不撮合；tick4 开盘清算（100 股 @1000）。
    assert_eq!(desk.market.volume_today, 100, "开盘清算一次撮合");
    assert_eq!(desk.market.trade_count, 1);
    assert_eq!(desk.skipped_orders, 1, "PreOpen 拒收 tick4 申报");
    let a0 = &report.group_finals[0].accounts[0];
    // 买方两单：清算成交 100 + PreOpen 拒单退款 ⇒ 持股 1100、现金扣一笔费用。
    assert_eq!(a0.shares[0], 1_100);
    assert_eq!(a0.cash, 1_000_000 - 100_501);
}

#[test]
fn cancel_window_closes_after_first_third_of_opening_auction() {
    // auction_ticks=6 ⇒ 不可撤窗口 = 6/3 = 2：day_tick 1 可撤、day_tick 3 不可撤。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            match tick {
                1 => place_at(1, vec![buy(0, 0, 1000, 100)]),
                // day_tick 2（tick2）：elapsed=2 ≥ 2 ⇒ 不可撤（显式回执）。
                2 => {
                    let mut p = TickPlan::default();
                    p.cancels.push(CancelSpec { local: 0, stock: 0 });
                    p
                }
                _ => TickPlan::default(),
            }
        })
    };
    let mut world = World::spawn(auction_spec(1, 1, 1, Box::new(script))).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let report = world.shutdown().expect("关停");
    // 撤单被拒：挂单仍在簿（Drain 后回流）；在途经 Closed 归零。
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.cash, 1_000_000, "拒撤+Drain 回流复原");
    assert_eq!(a0.foreign_active[0], 0);
}

// ── 到期撤销 ──

#[test]
fn orders_expire_at_ttl_with_entity_refund() {
    // TTL=1：tick1 挂单，tick2 起点到期撤销（先于本 tick 新意图）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![buy(0, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 1, Box::new(script));
    spec.order_ttl_ticks = 1;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.expired_orders, 1, "到期撤销计数");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.cash, 1_000_000, "到期退款复原");
    assert_eq!(a0.foreign_active[0], 0, "在途闭合");
}

// ── 市价余量不进簿 ──

#[test]
fn market_order_remainder_refunds_instead_of_resting() {
    // 市价注入路径（RoutedPrice::Market）当前经 DeskIn 直发测试：批 4 决策层
    // 接线前，以限价满额成交等价路径验证"余量不进簿"的退款闭合——
    // 完整市价路径由批 4 `RoutedPrice::Market` 脚本注入覆盖（登记待办）。
    // 此处验证：部分成交后余量挂簿（限价语义）与 Drain 回流的在途闭合。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100), buy(0, 0, 1000, 300)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(1).expect("收敛");
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.foreign_active[0], 0, "Drain 后在途清零");
}

// ── 守恒 ──

#[test]
fn cash_and_shares_conserve_across_many_fills() {
    // 两账户多轮交叉成交：现金总量 = 初始 − 费用桶；股份总量不变。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 200), buy(0, 0, 1005, 100)]),
            2 => place_at(2, vec![buy(0, 0, 1000, 100)]),
            3 => place_at(3, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(3).expect("收敛");
    let report = world.shutdown().expect("关停");
    let mut cash = 0i64;
    let mut shares = 0u32;
    for f in &report.group_finals {
        for a in &f.accounts {
            cash += a.cash;
            shares += a.shares[0];
        }
    }
    let fees: i64 = report.desk_finals.iter().map(|d| d.fee_sink).sum();
    assert_eq!(cash + fees, 2 * 1_000_000, "现金守恒（含 fee_sink）");
    assert_eq!(shares, 2_000, "股份守恒");
    assert!(fees > 0, "有成交即有费用入桶");
}

// ── 零股一次性（组端）──

#[test]
fn odd_lot_sell_allowed_only_as_full_remainder() {
    // 持仓 1050：卖 50（=全部零头）受理；卖 150（非整手且≠零头）skip。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            match tick {
                1 => place_at(1, vec![sell(0, 0, 1000, 150)]),
                // tick2：acct0 零头全清 1050（=全部可卖，受理）；acct1 整手买 1100
                // 吃下 1050，余 50 不成（部分成交余量，Drain 回流）。
                2 => place_at(2, vec![sell(0, 0, 1000, 1_050), buy(1, 0, 1000, 1_100)]),
                _ => TickPlan::default(),
            }
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.initial_shares = vec![vec![vec![1_050u32], vec![0u32]]];
    spec.initial_cash = 3_000_000;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("收敛");
    let report = world.shutdown().expect("关停");
    assert_eq!(
        report.group_finals[0].skipped_orders, 1,
        "非整手且非全部零头 ⇒ skip"
    );
    assert_eq!(
        report.desk_finals[0].skipped_orders, 0,
        "零头全清单受理（无拒收）"
    );
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 0, "零头一次性全清（1050 全部成交）");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 1_050, "买方得 1050（部分成交）");
}

// ── Sol P1-1/P1-2（拍卖自成交资源闭合+实际成交量）红→绿 ──
// 注：P1-1 语义为**只撤卖侧自单**（买侧续找第三方流动性）——早期
// "双撤"措辞已过时，资源闭合结论不变（卖侧皆自单时循环自然结束）。

#[test]
fn p1_auction_self_trade_double_cancel_refunds_and_reports_actual_volume() {
    use super::group::{GroupScript, PlaceSpec, TickPlan};
    // 拍卖窗（auction_ticks=15 ⇒ 申报 tick<10）内：acct0 挂买+挂卖同价
    // （同主交叉）+acct1 挂第三方卖。开盘清算：自卖单撤回（退款退股+
    // Closed/Cancelled），acct0 买与 acct1 第三方卖按实际量配对。
    let script = |_g: super::GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(PlaceSpec { local: 0, stock: 0, side: Side::Buy, price: 1000, qty: 100 });
                plan.places.push(PlaceSpec { local: 0, stock: 0, side: Side::Sell, price: 1000, qty: 100 });
                plan.places.push(PlaceSpec { local: 1, stock: 0, side: Side::Sell, price: 1000, qty: 100 });
            }
            plan
        })
    };
    let mut spec = super::skeleton_tests::basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 15;
    spec.closing_auction_ticks = 3;
    let mut world = super::world::World::spawn(spec).expect("世界构造");
    world.run_ticks(11).expect("跨申报窗触发开盘清算");
    let report = world.shutdown().expect("关停");
    // P1-1+P1-2 修正后语义：acct0 买 × acct1 卖成交 100（非自单配对——
    // 第三方流动性不被消耗）；acct0 自卖单撤回（资源闭合）。
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.cash, 1_000_000 - 100_501, "买成交实扣（1000×100+佣金500+过户1）");
    assert_eq!(a0.shares[0], 1_100, "买入 100 股入账（T+1 冻结）");
    assert_eq!(a0.frozen_today[0], 100, "当日买入冻结");
    assert!(a0.foreign_active.iter().all(|&x| x == 0), "在途全闭合（P1-1）");
    // 自卖撤回：acct1 卖 100 给 acct0 ⇒ acct1 shares=900；acct0 自卖
    // 单 Cancelled（资源退回已含在上列）。
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 900, "第三方卖成交扣股");
    // P1-2：实际成交量 100（选择量 200 含自单对——剔除后按实际）。
    assert_eq!(report.desk_finals[0].market.volume_today, 100, "实际成交量（P1-2）");
}


// ── Sol P1-3/P1-4/P1-6 反例红→绿 ──

#[test]
fn p1_3_undo_replay_clears_self_trade_index() {
    // 反例（Sol）：A 卖单挂簿→Abort（Inserted 回放）→重跑 A 买第三方
    // 卖单——若回放不清自成交索引，A 买被"幽灵自单"阻断。
    use super::group::{GroupScript, PlaceSpec, TickPlan};
    use std::sync::atomic::{AtomicBool, Ordering};
    let first_run = Arc::new(AtomicBool::new(false));
    let fr = Arc::clone(&first_run);
    let script = move |_g: super::GroupIdx| -> GroupScript {
        let fr = Arc::clone(&fr);
        Box::new(move |tick: u64, _round: u32| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                if !fr.swap(true, Ordering::SeqCst) {
                    plan.places.push(PlaceSpec { local: 0, stock: 0, side: Side::Sell, price: 1000, qty: 100 });
                } else {
                    plan.places.push(PlaceSpec { local: 1, stock: 0, side: Side::Sell, price: 1000, qty: 100 });
                    plan.places.push(PlaceSpec { local: 0, stock: 0, side: Side::Buy, price: 1000, qty: 100 });
                }
            }
            plan
        })
    };
    let mut spec = super::skeleton_tests::basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    let mut world = super::world::World::spawn(spec).expect("世界构造");
    let fail: super::coordinator::Validator =
        Box::new(|ev: &super::coordinator::TickEvidence| {
            if ev.tick == 1 { Err("注入失败".to_owned()) } else { Ok(()) }
        });
    world.set_validator(fail);
    assert!(world.run_ticks(1).is_err(), "tick1 注入失败");
    world.set_validator(Box::new(|_| Ok(())));
    world.run_ticks(1).expect("tick1 重跑成功");
    let report = world.shutdown().expect("关停");
    assert_eq!(report.desk_finals[0].market.volume_today, 100, "P1-3 回放清索引");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "买入入账");
}

#[test]
fn p1_6_second_day_minute_keys_not_doubled() {
    // 反例（Sol）：第二日 tick 255 连续分钟为零——不应填出 240 条偏移记录。
    // 经 desk 状态直达验证（世界跨日后的公共态分钟键域）。
    use super::group::{GroupScript, TickPlan};
    let script = |_g: super::GroupIdx| -> GroupScript {
        Box::new(|_t, _r| TickPlan::default())
    };
    let mut spec = super::skeleton_tests::basic_spec(1, 1, 1, Box::new(script));
    spec.ticks_per_day = 240;
    spec.auction_ticks = 15;
    spec.closing_auction_ticks = 3;
    let mut world = super::world::World::spawn(spec).expect("世界构造");
    world.run_ticks(255).expect("跨日");
    let views = world.public_views();
    for (minute, _) in &views[0].minute_closes {
        // 第二日（tick 241-255 ⇒ day=1）连续分钟=255-240-15=0——
        // 键域不应出现第二日偏移记录（240+）。
        if *minute >= 240 {
            panic!("P1-6 第二日零连续分钟却出现偏移键 {minute}");
        }
    }
    let _ = world.shutdown();
}

// ── Sol 二审修复集 1（回放删除清空价档）反例红→绿 ──

#[test]
fn sol2_abort_replay_empty_level_does_not_break_auction_clearing() {
    // 反例（Sol 二审）：挂卖 900 → tick 注入失败 Abort（Inserted 回放
    // 删除）→ 若 remove_by_ref 不删空档，空卖档 900 残留 → 重跑第三方
    // 卖 1000/买 1000：选价面只见有效单（可选 100 股），但清算循环
    // asks.keys().next() 先踩空档 900（front=None）提前结束——零成交。
    use super::group::{GroupScript, PlaceSpec, TickPlan};
    use std::sync::atomic::{AtomicBool, Ordering};
    let first_run = Arc::new(AtomicBool::new(false));
    let fr = Arc::clone(&first_run);
    let script = move |_g: super::GroupIdx| -> GroupScript {
        let fr = Arc::clone(&fr);
        Box::new(move |tick: u64, _round: u32| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                if !fr.swap(true, Ordering::SeqCst) {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Sell,
                        price: 900,
                        qty: 100,
                    });
                } else {
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
            }
            plan
        })
    };
    let mut spec = super::skeleton_tests::basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    // auction_ticks=6 ⇒ day_tick 0..3 申报窗、4..5 盘前：tick4 触发开盘清算。
    spec.auction_ticks = 6;
    spec.closing_auction_ticks = 3;
    let mut world = super::world::World::spawn(spec).expect("世界构造");
    let fail: super::coordinator::Validator = Box::new(|ev: &super::coordinator::TickEvidence| {
        if ev.tick == 1 {
            Err("注入失败".to_owned())
        } else {
            Ok(())
        }
    });
    world.set_validator(fail);
    assert!(world.run_ticks(1).is_err(), "tick1 注入失败（Abort 回放删 900 档）");
    world.set_validator(Box::new(|_| Ok(())));
    world.run_ticks(4).expect("tick1-4 重跑+开盘清算");
    // 开盘清算事实（completed）：实际成交量必须 100（空档残留时为 0）。
    let clearing = world
        .report_log
        .iter()
        .flat_map(|r| r.desk_auction.iter())
        .find(|f| f.completed)
        .expect("开盘清算事实存在");
    assert_eq!(
        clearing.matched_volume, 100,
        "清算实际成交 100 股（空卖档不得截断清算）"
    );
    let report = world.shutdown().expect("关停");
    assert_eq!(
        report.desk_finals[0].market.volume_today, 100,
        "成交量口径与清算一致"
    );
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "买入入账");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 900, "第三方卖出扣股");
}

// ── Sol 二审修复集 4：turnover 回滚（Abort 冲回日内累计口径） ──

#[test]
fn sol4_turnover_rolls_back_on_abort() {
    // tick1 买卖交叉成交（turnover=100×1000）；注入失败 Abort——
    // Market 快照回放须含 turnover_today；重跑同批单后终态 turnover
    // 恰为 100_000（回滚缺失则残留翻倍 200_000）。
    use super::group::{GroupScript, PlaceSpec, TickPlan};
    let script = |_g: super::GroupIdx| -> GroupScript {
        Box::new(|tick: u64, _round: u32| {
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
    let mut spec = super::skeleton_tests::basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    let mut world = super::world::World::spawn(spec).expect("世界构造");
    let fail: super::coordinator::Validator = Box::new(|ev| {
        if ev.tick == 1 {
            Err("sol4 注入".to_owned())
        } else {
            Ok(())
        }
    });
    world.set_validator(fail);
    assert!(world.run_ticks(1).is_err(), "tick1 注入失败（Abort）");
    world.set_validator(Box::new(|_| Ok(())));
    world.run_ticks(1).expect("tick1 重跑（同批单再成交）");
    let report = world.shutdown().expect("关停");
    let market = &report.desk_finals[0].market;
    assert_eq!(market.turnover_today, 100_000, "turnover 回滚后恰一轮成交额");
    assert_eq!(market.volume_today, 100, "成交量回滚后恰 100 股");
    assert_eq!(market.trade_count, 1, "笔数回滚后恰 1 笔");
}
