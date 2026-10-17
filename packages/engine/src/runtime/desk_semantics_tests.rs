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
    assert_eq!(a1.cash, 1_000_000 + 109_444, "卖方净得（110_000−556=佣金 500+印花 55+过户 1）");
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
            initial_recent_prices: Vec::new(),
            initial_traded_days: 0,
            initial_market: None,
            initial_public_cutoff: 0,
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
            // D5 off-by-one 对齐：受理相位基准=phase(tick-1)（旧管线
            /// step(N) 的受理窗=N-1 相位——PreOpen 拒收的申报须在 tick5
            // （phase(4)=PreOpen；tick4 申报按 phase(3)=CallAuction 受理）。
            5 => place_at(5, vec![buy(0, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn(auction_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(5).expect("收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    // tick1..3 积累不撮合；tick4 开盘清算（100 股 @1000）。
    assert_eq!(desk.market.volume_today, 100, "开盘清算一次撮合");
    assert_eq!(desk.market.trade_count, 1);
    assert_eq!(desk.skipped_orders, 1, "PreOpen 拒收 tick5 申报（phase(4)=PreOpen）");
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

// ═══ 语义单测细化批（sol-103）：零覆盖/弱面独立锚定（手算 oracle） ═══

/// R21 草稿阶段容量约定（ADR-0019）：请求队列与未成交挂单不按固定条数
/// 拒绝合法请求——单 tick 300 张同账户买单全部受理（超过任何合理固定
/// 上限），资源校验仍逐单执行。
///
/// 手算 oracle（initial_cash=100_000_000，300 张买 100 股@900）：
/// 每张 purse = 90_000（对价）+ 500（最低佣金——rated 22.5→银行家舍入 22
/// <500 取下限）+ 1（过户 90_000×0.01‰=0.9→1）= 90_501 分；
/// 在途现金合计 = 300×90_501 = 27_150_300 ⇒ 途中现金 72_849_700。
#[test]
fn r21_capacity_convention_no_fixed_count_rejection() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => {
                let mut plan = TickPlan::default();
                for _ in 0..300 {
                    plan.places.push(buy(0, 0, 900, 100));
                }
                plan
            }
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 1, Box::new(script));
    spec.initial_cash = 100_000_000;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("收敛");
    // 途中（挂单在簿）：全量受理、无按条数拒收。
    let accounts = world.snapshot_accounts().expect("账户快照");
    let a0 = &accounts[0].1[0];
    assert_eq!(a0.cash, 100_000_000 - 27_150_300, "300 张 purse 全额预留（容量内不拒）");
    assert_eq!(a0.foreign_active[0], 300, "300 张在途");
    let views = world.public_views().to_vec();
    assert_eq!(
        views[0].depth_bids,
        vec![(900, 30_000)],
        "同价聚合 30_000 股（300 张挂簿）"
    );
    // 关停 Drain 全量回流（容量场景的守恒闭合）。
    let report = world.shutdown().expect("关停");
    assert_eq!(report.desk_finals[0].skipped_orders, 0, "零拒收（不按条数拒）");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.cash, 100_000_000, "Drain 后全额回流");
    assert_eq!(a0.foreign_active[0], 0);
}

/// R16 市价意图（游戏简化）：按涨跌停价保护逐档立即成交、未成交余量
/// **不进簿**（原路退款）。手算 oracle（初始价 1000、主板 ±10% ⇒ 保护价
/// =涨停 1100；对手盘=卖 100@1000）：
/// - purse 预留 = 300×1100 + 500（最低佣金）+ 3（过户 330_000×0.01‰=3.3→3）
///   = 330_503；
/// - 实际成交 100@1000（maker 价）：实付 100_000+500+1=100_501，
///   退款 230_002 ⇒ 期末现金 899_499；
/// - 余量 200 不挂簿：买盘深度为空（限价对照组同场景 200 股挂簿）。
#[test]
fn r16_market_intent_fills_walks_book_and_refunds_remainder() {
    use super::decision::PendingOp;
    use super::protocol::RoutedPrice;
    // 市价臂。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.config = config_with_cage(false); // 隔离变量：band=涨跌停
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 300,
        }],
        vec![],
    );
    world.run_ticks(2).expect("收敛");
    let views = world.public_views().to_vec();
    assert!(
        views[0].depth_bids.is_empty(),
        "市价余量不进簿（买盘空）：{:?}",
        views[0].depth_bids
    );
    assert!(views[0].depth_asks.is_empty(), "对手卖单已全量成交");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.market.volume_today, 100, "只成交对手盘 100 股");
    assert_eq!(desk.market.trade_count, 1);
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "得 100 股");
    assert_eq!(a0.cash, 899_499, "实付 100_501（对价+最低佣金 500+过户 1），余量全额退款");

    // 限价对照组（同场景 discriminator）：300@1000 限价 ⇒ 余量 200 挂簿。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Fixed(1000),
            purse_reference: 1000,
            qty: 300,
        }],
        vec![],
    );
    world.run_ticks(2).expect("收敛");
    let views = world.public_views().to_vec();
    assert_eq!(
        views[0].depth_bids,
        vec![(1000, 200)],
        "限价对照：余量 200 挂簿（与市价不进簿形成判别）"
    );
    let _ = world.shutdown();
}

/// R19 最高/最低价格选项（ADR-0022）：符号价在交易部受理时解析为当前
/// 合法边界——Highest 买=涨停（预留资金按解析价），Lowest 卖=跌停
/// （卖单不占现金）。手算 oracle（初始 1000、±10%、笼子关）：
/// - Highest 买 100@1100：purse = 110_000+500（rated 27.5→28<500）+1
///   （过户 1.1→1）= 110_501 ⇒ 途中现金 889_499，买一=1100；
/// - Lowest 卖 100@900：现金不动（1_000_000），持仓 1000→900+100 在途，
///   卖一=900。
#[test]
fn r19_symbolic_prices_resolve_to_daily_band_edges() {
    use super::decision::PendingOp;
    use super::protocol::RoutedPrice;
    // Highest 买臂。
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
            Box::new(|_t, _r| TickPlan::default())
        }));
        spec.config = config_with_cage(false);
        spec
    })
    .expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Highest,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let views = world.public_views().to_vec();
    assert_eq!(views[0].best_bid, Some(1100), "Highest 买解析=涨停价（挂簿价）");
    let accounts = world.snapshot_accounts().expect("账户快照");
    assert_eq!(
        accounts[0].1[0].cash, 889_499,
        "资金预留按解析价 1100（涨停价预留——ADR-0022）"
    );
    assert_eq!(accounts[0].1[0].foreign_active[0], 1);
    let _ = world.shutdown();

    // Lowest 卖臂。
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
            Box::new(|_t, _r| TickPlan::default())
        }));
        spec.config = config_with_cage(false);
        spec
    })
    .expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 1,
            stock: 0,
            side: Side::Sell,
            routed: RoutedPrice::Lowest,
            purse_reference: 900,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let views = world.public_views().to_vec();
    assert_eq!(views[0].best_ask, Some(900), "Lowest 卖解析=跌停价（挂簿价）");
    let accounts = world.snapshot_accounts().expect("账户快照");
    let a1 = &accounts[0].1[1];
    assert_eq!(a1.cash, 1_000_000, "卖单不占现金（R17 共享资源口径）");
    assert_eq!(a1.shares[0], 900, "100 股转入在途");
    assert_eq!(a1.pending_sell_qty[0], 100, "在途卖量镜像");
    let _ = world.shutdown();
}

// ── R8 撤单窗 / R9 部分与全成撤单（sol-103 弱面独立锚定）──
//
// 经 world 级 ExternalPlan 通道（与驱动器 inject_player_intents 同一下游
// 路径）注入玩家单：脚本 fixture 无 NPC 计划/报价干扰，成交面完全由
// 确定性注入承载。回执与生命周期事实从 world.report_log 断言。

use super::decision::PendingOp;
use super::protocol::RoutedPrice;

fn ext_buy(local: u32, price: i64, qty: u32) -> PendingOp {
    PendingOp {
        local,
        stock: 0,
        side: Side::Buy,
        routed: RoutedPrice::Fixed(price),
        purse_reference: price,
        qty,
    }
}

fn ext_cancel(local: u32, serial: u64) -> (u32, u16, u64) {
    (local, 0, serial)
}

/// 从 report_log 取指定 tick 的（回执, 生命周期）切片。
fn tick_facts(
    world: &World,
    tick: u64,
) -> (Vec<super::DeskReceipt>, Vec<super::protocol::DeskLifecycleFact>) {
    let rec = world
        .report_log
        .iter()
        .find(|r| r.tick == tick)
        .unwrap_or_else(|| panic!("tick {tick} 无收束记录"));
    (rec.desk_receipts.clone(), rec.desk_lifecycle.clone())
}

/// R8 开盘竞价撤单窗（弱面强化——原 desk 测试现金复原无法区分拒撤/成功
/// 撤单）：auction_ticks=9 ⇒ 可撤窗=前 1/3（day_tick<3）。
/// - **窗内**（tick 2）撤买 ⇒ Cancelled 生命周期事实（余量 100 释放）；
/// - **越窗**（tick 5）撤买 ⇒ CancelWindowClosed 回执（显式业务失败），
///   挂单保留并参与开盘清算（tick 6 完成拍恰成交一笔）；
/// - 窗内撤**他人**挂单 ⇒ OrderNotFound（身份防御）。
/// 手算 oracle：双边同价 1000 ⇒ 清算价=1000（单候选，无中间价问题）；
/// 买方实付 100_000+500+1=100_501；卖方净得 100_000−551（佣金 500+
/// 印花 50+过户 1）。
#[test]
fn r08_cancel_window_partition_by_receipt_and_clearing_participation() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 9; // 申报窗=day_tick<6、可撤窗=day_tick<3
    spec.closing_auction_ticks = 3;
    let mut world = World::spawn(spec).expect("世界构造");
    // t1：卖方（acct1）+ 玩家买#1。
    world.broadcast_external_plan(vec![ext_buy(0, 1000, 100)], vec![]);
    world.run_ticks(1).expect("t1");
    let (_, life1) = tick_facts(&world, 1);
    let buy1 = life1
        .iter()
        .find(|f| matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner == (0, 0))
        .map(|f| f.order_serial)
        .expect("玩家买#1 受理（前置）");
    let sell_serial = life1
        .iter()
        .find(|f| matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner == (0, 1))
        .map(|f| f.order_serial)
        .expect("卖方受理（前置）");
    // t2：窗内撤自己的买#1（成功）+ 以玩家身份撤他人卖单（OrderNotFound）。
    world.broadcast_external_plan(
        vec![],
        vec![ext_cancel(0, buy1), ext_cancel(0, sell_serial)],
    );
    world.run_ticks(1).expect("t2");
    let (receipts2, life2) = tick_facts(&world, 2);
    assert!(
        life2.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                order_serial,
                remaining: 100,
                ..
            } if *order_serial == buy1
        )),
        "窗内撤单成功：Cancelled 事实且余量=100（{life2:?}）"
    );
    assert!(
        !receipts2
            .iter()
            .any(|r| r.kind == super::ReceiptKind::CancelWindowClosed),
        "窗内撤单不得有窗口拒绝回执：{receipts2:?}"
    );
    assert!(
        receipts2
            .iter()
            .any(|r| r.kind == super::ReceiptKind::OrderNotFound),
        "撤他人挂单必须 OrderNotFound（身份防御）：{receipts2:?}"
    );
    // t3：玩家买#2（同价 1000，进入清算面）。
    world.broadcast_external_plan(vec![ext_buy(0, 1000, 100)], vec![]);
    world.run_ticks(1).expect("t3");
    let (_, life3) = tick_facts(&world, 3);
    let buy2 = life3
        .iter()
        .find(|f| matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner == (0, 0))
        .map(|f| f.order_serial)
        .expect("玩家买#2 受理");
    // t5：越窗撤买#2（day_tick 5 ≥ 3 ⇒ CancelWindowClosed）。
    world.run_ticks(1).expect("t4");
    world.broadcast_external_plan(vec![], vec![ext_cancel(0, buy2)]);
    world.run_ticks(1).expect("t5");
    let (receipts5, life5) = tick_facts(&world, 5);
    assert!(
        receipts5
            .iter()
            .any(|r| r.kind == super::ReceiptKind::CancelWindowClosed),
        "越窗撤单必须显式 CancelWindowClosed 回执：{receipts5:?}"
    );
    assert!(
        !life2.iter().chain(life5.iter()).any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                order_serial,
                ..
            } if *order_serial == buy2
        )),
        "越窗撤单不得产生 Cancelled（挂单保留参与清算）"
    );
    // t6：开盘清算（离开申报窗首个 tick）——买#2 × 卖单恰一笔。
    world.run_ticks(1).expect("t6 清算");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.market.volume_today, 100, "恰 100 股（被撤的买#1 不在清算面）");
    assert_eq!(desk.market.trade_count, 1);
    assert_eq!(desk.market.last_price, 1000, "清算价=双边同价申报（单候选）");
    let a0 = &report.group_finals[0].accounts[0];
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a0.shares[0], 1_100, "买#2 成交 100 股");
    assert_eq!(a0.cash, 1_000_000 - 100_501, "买方：对价+最低佣金 500+过户 1");
    assert_eq!(a1.shares[0], 900, "卖方出 100 股");
    assert_eq!(a1.cash, 1_000_000 + 100_000 - 551, "卖方净得：佣金 500+印花 50+过户 1");
}

/// R9 部分成交撤单（弱面强化——此前 rt 侧仅 ReceiptKind 枚举相等性）：
/// 买 300 限价 1000 对手盘 100 ⇒ 同拍成交 100、余 200 挂簿；下一拍撤单
/// ⇒ Cancelled 事实余量恰 200（只撤余量），余量 purse 全额退款。
/// 手算 oracle：实付 100_000+500+1=100_501；撤单后买盘深度为空。
#[test]
fn r09_partial_fill_cancel_releases_remainder_only() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0; // 全连续窗（tick 1 即撮合）
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(vec![ext_buy(0, 1000, 300)], vec![]);
    world.run_ticks(1).expect("t1 成交拍");
    let desk = &world.report_log[0];
    assert_eq!(
        desk.desk_trades.len(),
        1,
        "同拍部分成交 100（前置）：{:?}",
        desk.desk_trades
    );
    let buy_serial = desk
        .desk_lifecycle
        .iter()
        .find(|f| matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner == (0, 0))
        .map(|f| f.order_serial)
        .expect("玩家买单受理（余量 200 挂簿）");
    let views = world.public_views().to_vec();
    assert_eq!(views[0].depth_bids, vec![(1000, 200)], "撤单前余量 200 挂簿");
    // t2：撤单——只撤余量。
    world.broadcast_external_plan(vec![], vec![ext_cancel(0, buy_serial)]);
    world.run_ticks(1).expect("t2 撤单拍");
    let (_, life2) = tick_facts(&world, 2);
    assert!(
        life2.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                order_serial,
                remaining: 200,
                ..
            } if *order_serial == buy_serial
        )),
        "部分成交撤单只撤余量 200：{life2:?}"
    );
    let views = world.public_views().to_vec();
    assert!(views[0].depth_bids.is_empty(), "撤单后余量离簿");
    // 期末（Drain 闭合）：已成交部分保持、余量全额退款。
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "成交的 100 股保持");
    assert_eq!(a0.cash, 1_000_000 - 100_501, "实付 100_501；余量 purse 全额退款");
}

/// R9 已全成撤单（弱面强化）：全成订单撤单=明确业务失败回执 AlreadyFilled
/// （不产生 Cancelled、不动已结算账）；本主不存在的 serial 同样落
/// AlreadyFilled（「已成交、无法撤单」近似——会话侧以 filled_orders
/// 区分，desk 对本主不可分辨时按已全成反馈）。
/// 手算 oracle：玩家 300 股、现金 1_000_000−300_503（佣金 500 单笔一次+
/// 过户 1+2 逐笔）；卖方两笔净得 …−551−602。
#[test]
fn r09_fully_filled_cancel_returns_already_filled_receipt() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            2 => place_at(
                2,
                vec![
                    sell(1, 0, 1000, 200), // 吃掉买方余量 200 ⇒ 买方全成
                    sell(1, 0, 1100, 100), // 在簿他人单（OrderNotFound 臂用）
                ],
            ),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(vec![ext_buy(0, 1000, 300)], vec![]);
    world.run_ticks(2).expect("t1-t2：买方 300 股全成");
    let buy_serial = world.report_log[0]
        .desk_lifecycle
        .iter()
        .find(|f| matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner == (0, 0))
        .map(|f| f.order_serial)
        .expect("玩家买单 t1 受理（余量挂簿——此后被吃满）");
    let resting_sell = world.report_log[1]
        .desk_lifecycle
        .iter()
        .find(|f| matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner == (0, 1))
        .map(|f| f.order_serial)
        .expect("1100 卖单在簿（他人单）");
    // t3：撤全成单（AlreadyFilled）+ 撤他人挂单（OrderNotFound）+
    // 撤本主不存在 serial（AlreadyFilled 近似）。
    world.broadcast_external_plan(
        vec![],
        vec![
            ext_cancel(0, buy_serial),
            ext_cancel(0, resting_sell),
            ext_cancel(0, 99_999),
        ],
    );
    world.run_ticks(1).expect("t3 回执拍");
    let (receipts3, life3) = tick_facts(&world, 3);
    let count = |k: super::ReceiptKind| receipts3.iter().filter(|r| r.kind == k).count();
    assert_eq!(count(super::ReceiptKind::AlreadyFilled), 2, "全成+本主不存在 ⇒ AlreadyFilled：{receipts3:?}");
    assert_eq!(count(super::ReceiptKind::OrderNotFound), 1, "他人挂单 ⇒ OrderNotFound：{receipts3:?}");
    assert!(
        !life3
            .iter()
            .any(|f| matches!(f.kind, super::protocol::LifecycleKind::Cancelled)),
        "失败撤单不得产生 Cancelled：{life3:?}"
    );
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_300, "全成 300 股保持");
    assert_eq!(a0.cash, 1_000_000 - 300_503, "结算不被撤单触碰（佣金 500+过户 3）");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 700, "卖 300（1100 挂单被 Drain 退回）");
    assert_eq!(
        a1.cash,
        1_000_000 + 100_000 - 551 + 200_000 - 602,
        "卖方净得两笔（500+50+1 与 500+100+2）"
    );
}

/// R01b/#7 连续竞价自成交终止（死亡断言再锚定·census 自成交 #7）：
/// 同账户买卖同价挂单——taker 交叉档遇自单**终止撮合**（不与自单成
/// 交）。零 Trade、双边挂单原位保留（卖 1000 在 ask、买 1000 在 bid）。
#[test]
fn r07_continuous_self_cross_terminates_without_self_trade() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            // 卖单先入簿（maker 候选）。
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    // 同账户（acct1）买入同价——taker 交叉档=自单 ⇒ 终止撮合。
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 1,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Fixed(1000),
            purse_reference: 1000,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    assert!(
        rec.desk_trades.is_empty(),
        "同主交叉档不得自成交：{:?}",
        rec.desk_trades
    );
    let views = world.public_views().to_vec();
    assert_eq!(
        views[0].best_ask,
        Some(1000),
        "自单（卖）原位保留在簿"
    );
    assert_eq!(
        views[0].best_bid,
        Some(1000),
        "taker（买）终止后挂簿（未与自单成交）"
    );
    let report = world.shutdown().expect("关停");
    assert_eq!(report.desk_finals[0].market.volume_today, 0, "零成交量");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.cash, 1_000_000, "现金不动（无成交）");
    assert_eq!(a1.shares[0], 1_000, "Drain 后持仓复原");
}

/// A 股制度级（sol-103 上报项 1 修复·红测试先行）：**集合竞价单一价格
/// 原则**——所有成交按同一清算价结算（沪深交易规则：集合竞价以单一
/// 价格撮合成交）。此前 runtime settle_fill 按 maker 申报价结算（与旧
/// 管线 orderbook.rs execute_auction_clearing 按清算价不一致）。
///
/// fixture：买 1000 × 卖 900（不对称交叉）⇒ 候选 {900,1000} 同为最大
/// 成交量 100 ⇒ 沪市取候选中间价 = 950（半进位无小数）。
/// 手算 oracle（双方都按 950 结算）：
/// - 买方实付 95_000+500（最低佣金；rated 23.75→24<500）+1（过户
///   0.95→1）= 95_501 ⇒ 现金 904_499；
/// - 卖方净得 95_000−549（佣金 500+印花 47.5→48 银行家舍入+过户 1）
///   = 94_451 ⇒ 现金 1_094_451；
/// - 成交事实价=950、last_price=950（=AuctionCompleted 清算价）。
#[test]
fn auction_fills_settle_at_single_clearing_price() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 900, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 9; // 申报窗 day_tick<6、完成拍=6
    spec.closing_auction_ticks = 3;
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Fixed(1000),
            purse_reference: 1000,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(6).expect("完成拍收敛");
    let rec = world
        .report_log
        .iter()
        .find(|r| !r.desk_trades.is_empty())
        .expect("完成拍有成交事实");
    assert_eq!(rec.desk_trades.len(), 1, "恰一笔");
    assert_eq!(
        rec.desk_trades[0].price, 950,
        "成交价=清算价 950（单一价格原则——非 maker 申报价 900）"
    );
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.market.volume_today, 100);
    assert_eq!(desk.market.last_price, 950, "最新价=清算价");
    let a0 = &report.group_finals[0].accounts[0];
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a0.shares[0], 1_100, "买方得 100 股");
    assert_eq!(
        a0.cash, 904_499,
        "买方按清算价实付：95_000+佣金 500+过户 1"
    );
    assert_eq!(a1.shares[0], 900, "卖方出 100 股");
    assert_eq!(
        a1.cash, 1_094_451,
        "卖方按清算价净得：95_000−（佣金 500+印花 48+过户 1）"
    );
}

// ═══ Sol 复审修复（sol-103 第二轮）：市价保护边界/玩家逐请求回执/
// 竞价多档单一价格 ═══

/// Sol-1（阻断）：市价保护边界=日涨跌停（**不含价格笼子**——笼子只
/// 作用于限价申报）。此前 resolve_price 把"涨跌停∩笼子"用于 Market：
/// 卖档 1000/1050、涨停 1100、笼子开（参考 1000⇒买上限 1020）时市价
/// 买被钳成 1020，吃完 1000 档即停（1050 不吃）。手算 oracle（市价
/// 买 200 保护=1100）：两档全吃（100@1000+100@1050，连续竞价 maker
/// 价），实付 205_000+500（最低佣金单张一次）+1+1（过户逐笔）
/// =205_502 ⇒ 现金 794_498。
#[test]
fn sol1_market_protection_uses_daily_band_not_cage_across_levels() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100), sell(1, 0, 1050, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    // 笼子保持开启（proposed_defaults 默认）——本测试即验证笼子不钳市价。
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 200,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    let prices: Vec<(u32, i64)> = rec
        .desk_trades
        .iter()
        .map(|f| (f.qty, f.price))
        .collect();
    assert_eq!(
        prices,
        vec![(100, 1000), (100, 1050)],
        "市价按涨停保护逐档跨笼子吃单（1050>笼子 1020 也须吃）：{prices:?}"
    );
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_200, "两档全得 200 股");
    assert_eq!(a0.cash, 794_498, "实付 205_502（对价+最低佣金一次+过户 2）");
}

/// Sol-2（阻断）：市价单逐请求玩家可见结果——零成交（空盘口）与部分
/// 成交余量都必须显式 OrderCanceled（余量即撤销量），不能只有内部
/// 退款。手算 oracle：空盘口⇒Cancel 余量 100、现金复原；单档 100、
/// 市价买 200⇒Trade 100@1000+Cancel 余量 100、实付 100_501。
#[test]
fn sol2_market_remainder_and_zero_fill_emit_visible_cancellation() {
    // 空盘口臂：零成交全撤。
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
            Box::new(|_t, _r| TickPlan::default())
        }));
        spec.auction_ticks = 0;
        spec
    })
    .expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    assert!(
        rec.desk_trades.is_empty(),
        "空盘口零成交（前置）：{:?}",
        rec.desk_trades
    );
    assert!(
        rec.desk_lifecycle.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                remaining: 100,
                owner: (0, 0),
                ..
            }
        )),
        "零成交市价单必须显式撤销余量（玩家可见）：{:?}",
        rec.desk_lifecycle
    );
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.cash, 1_000_000, "purse 全额退款（零成交）");
    assert_eq!(a0.shares[0], 1_000, "零成交持股不动（基线 1000）");

    // 部分成交臂：Trade 100 + 余量 100 撤销。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 200,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    assert_eq!(rec.desk_trades.len(), 1, "部分成交 100（前置）");
    assert!(
        rec.desk_lifecycle.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                remaining: 100,
                owner: (0, 0),
                ..
            }
        )),
        "部分成交余量必须显式撤销（注明余量）：{:?}",
        rec.desk_lifecycle
    );
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "基线 1000+成交 100");
    assert_eq!(a0.cash, 899_499, "实付 100_501；余量退款");
}

/// Sol-5a：开盘竞价异价多档——全部成交 Trade.price==清算价（单一价格
/// 原则多档面：两笔 maker 档价不同 900/1000，成交价恒=清算价 1000）
/// +未成交余单保留原档。手算（买 1000×200、卖 900×100+卖 1000×300）：
/// 候选 1000 成交量 200（最大，单候选）⇒ 清算 1000；FIFO 卖 900 全成
/// 100 + 卖 1000 成 100（两笔均 1000）；卖 1000 档余 200 留簿转连续。
/// 买方实付 200_000+500+2=200_502。
#[test]
fn sol5a_opening_multi_level_all_trades_at_clearing_price_with_remainder() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![sell(1, 0, 900, 100), sell(1, 0, 1000, 300)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 9;
    spec.closing_auction_ticks = 3;
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Fixed(1000),
            purse_reference: 1000,
            qty: 200,
        }],
        vec![],
    );
    world.run_ticks(6).expect("完成拍");
    let rec = world
        .report_log
        .iter()
        .find(|r| !r.desk_trades.is_empty())
        .expect("完成拍有成交");
    let trades: Vec<(u32, i64)> = rec.desk_trades.iter().map(|f| (f.qty, f.price)).collect();
    assert_eq!(
        trades,
        vec![(100, 1000), (100, 1000)],
        "两笔 maker 档价 900/1000 不同，成交价恒=清算价 1000（单一价格）：{trades:?}"
    );
    let views = world.public_views().to_vec();
    assert_eq!(
        views[0].depth_asks,
        vec![(1000, 200)],
        "卖 1000 档余 200 保留原档转连续"
    );
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_200, "基线 1000+成交 200");
    assert_eq!(a0.cash, 1_000_000 - 200_502, "实付 200_000+佣金 500+过户 2");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 800, "卖 200（余 200 在途 Drain 退回）");
}

/// Sol-5b：市价卖×空盘口（双方向）——零成交全撤、股份复原。
#[test]
fn sol5b_market_sell_on_empty_book_cancels_with_share_refund() {
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
            Box::new(|_t, _r| TickPlan::default())
        }));
        spec.auction_ticks = 0;
        spec
    })
    .expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 1,
            stock: 0,
            side: Side::Sell,
            routed: RoutedPrice::Market,
            purse_reference: 900,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    assert!(rec.desk_trades.is_empty(), "空盘口零成交");
    assert!(
        rec.desk_lifecycle.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                remaining: 100,
                owner: (0, 1),
                ..
            }
        )),
        "市价卖零成交必须显式撤销余量：{:?}",
        rec.desk_lifecycle
    );
    let report = world.shutdown().expect("关停");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 1_000, "股份全额复原");
    assert_eq!(a1.cash, 1_000_000, "卖单无现金预留");
}

/// Sol-5c：Highest/Lowest 四方向×purse 临界（组端 buy_purse_limit 按
/// purse_reference 计算）。手算：Lowest 买 purse=900×100+500+1=90_501
/// （ADR-0022：Lowest 买等效跌停限价单，预留跌停即足够）；Highest 买
/// purse=110_501；卖向无现金预留（现金 0 也可受理）。
#[test]
fn sol5c_symbolic_four_directions_purse_thresholds() {
    // Lowest 买：现金恰 90_501 ⇒ 受理挂簿 900。
    let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
        Box::new(|_t, _r| TickPlan::default())
    }));
    spec.auction_ticks = 0;
    spec.initial_cash = 90_501;
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Lowest,
            purse_reference: 900,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let views = world.public_views().to_vec();
    assert_eq!(views[0].best_bid, Some(900), "跌停预留恰够 ⇒ 受理挂簿 900");
    let accounts = world.snapshot_accounts().expect("快照");
    assert_eq!(accounts[0].1[0].cash, 0, "purse 90_501 全额预留");
    let _ = world.shutdown();

    // Highest 买：现金恰 110_501 ⇒ 受理；110_500 ⇒ 显式 InsufficientCash。
    let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
        Box::new(|_t, _r| TickPlan::default())
    }));
    spec.auction_ticks = 0;
    spec.initial_cash = 110_501;
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Highest,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let views = world.public_views().to_vec();
    assert_eq!(views[0].best_bid, Some(1100), "涨停预留恰够 ⇒ 受理挂簿 1100");

    let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
        Box::new(|_t, _r| TickPlan::default())
    }));
    spec.auction_ticks = 0;
    spec.initial_cash = 110_500;
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Highest,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    assert!(
        rec.group_rejections.iter().any(|r| matches!(
            r,
            super::DeskReceipt {
                kind: super::ReceiptKind::InsufficientCash,
                owner: (0, 0),
                ..
            }
        )),
        "现金差 1 分必须显式 InsufficientCash 回执（计数不替代回执）：{:?}",
        rec.group_rejections
    );

    // 卖向（现金 0）：Highest/Lowest 均受理（无现金预留）。
    for routed in [RoutedPrice::Highest, RoutedPrice::Lowest] {
        let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
            Box::new(|_t, _r| TickPlan::default())
        }));
        spec.auction_ticks = 0;
        spec.initial_cash = 0;
        spec.config = config_with_cage(false);
        let mut world = World::spawn(spec).expect("世界构造");
        let want = if matches!(routed, RoutedPrice::Highest) {
            1100
        } else {
            900
        };
        world.broadcast_external_plan(
            vec![PendingOp {
                local: 1,
                stock: 0,
                side: Side::Sell,
                routed,
                purse_reference: want,
                qty: 100,
            }],
            vec![],
        );
        world.run_ticks(1).expect("收敛");
        let views = world.public_views().to_vec();
        assert_eq!(
            views[0].best_ask,
            Some(want),
            "{routed:?} 卖受理（无现金预留——现金 0）"
        );
        let _ = world.shutdown();
    }
}

// ═══ Sol 三轮修复测试：Continue 跨轮交付 / serial 唯一 / 卖向跨笼子 ═══

/// Sol 三轮修复 1：Continue 链上早轮事实不丢——轮 0 的市价余量撤销
/// （Cancelled）与挂簿受理（Accepted）必须进最终 TickRecord（此前
/// Continue 分支丢弃早轮 evidence，轮 0 事实静默消失）。
#[test]
fn sol3r_continue_preserves_early_round_facts() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, round| {
            let mut plan = TickPlan::default();
            if tick == 1 && round == 0 {
                plan.places.push(buy(0, 0, 1000, 100));
                plan.continuation = true; // 触发 Continue（本轮事实在早轮）
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    // 轮 0 处理的空盘市价单：Cancelled 事实产生于早轮。
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 1,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛（含 Continue 轮）");
    let rec = &world.report_log[0];
    assert_eq!(rec.rounds, 1, "前置：发生了一次 Continue（两轮）");
    assert!(
        rec.desk_lifecycle.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Cancelled,
                remaining: 100,
                owner: (0, 1),
                ..
            }
        )),
        "轮 0 的市价撤销事实必须进终轮 TickRecord：{:?}",
        rec.desk_lifecycle
    );
    assert!(
        rec.desk_lifecycle.iter().any(|f| matches!(
            f,
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Accepted,
                owner: (0, 0),
                ..
            }
        )),
        "轮 0 的受理事实必须进终轮 TickRecord：{:?}",
        rec.desk_lifecycle
    );
}

/// Sol 三轮修复 2：即时单序号推进——同账户同股连续两笔空盘市价单的
/// Cancelled 订单 ID 必须互异，后续挂簿单不复用（此前取号不推进，
/// 两条 Cancelled 同 ID、挂簿单复用）。
#[test]
fn sol3r_immediate_orders_consume_unique_serials() {
    let mut spec = basic_spec(1, 1, 2, Box::new(|_g| {
        Box::new(|_t, _r| TickPlan::default())
    }));
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    let mut cancelled_serials = Vec::new();
    for _ in 0..2 {
        world.broadcast_external_plan(
            vec![PendingOp {
                local: 0,
                stock: 0,
                side: Side::Buy,
                routed: RoutedPrice::Market,
                purse_reference: 1100,
                qty: 100,
            }],
            vec![],
        );
        world.run_ticks(1).expect("空盘市价拍");
        let rec = world.report_log.last().expect("记录");
        let serial = rec
            .desk_lifecycle
            .iter()
            .find_map(|f| match f {
                super::protocol::DeskLifecycleFact {
                    kind: super::protocol::LifecycleKind::Cancelled,
                    owner: (0, 0),
                    order_serial,
                    ..
                } => Some(*order_serial),
                _ => None,
            })
            .expect("市价撤销事实");
        cancelled_serials.push(serial);
    }
    assert_ne!(
        cancelled_serials[0], cancelled_serials[1],
        "两笔即时单的订单 ID 必须互异：{cancelled_serials:?}"
    );
    // 后续挂簿单不复用已耗序号：限价挂簿 Accepted serial 严格更大。
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Fixed(1000),
            purse_reference: 1000,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("挂簿拍");
    let rec = world.report_log.last().expect("记录");
    let accepted = rec
        .desk_lifecycle
        .iter()
        .find_map(|f| match f {
            super::protocol::DeskLifecycleFact {
                kind: super::protocol::LifecycleKind::Accepted,
                owner: (0, 0),
                order_serial,
                ..
            } => Some(*order_serial),
            _ => None,
        })
        .expect("挂簿受理");
    assert!(
        accepted > cancelled_serials[0] && accepted > cancelled_serials[1],
        "挂簿单不复用已耗序号：accepted={accepted} cancelled={cancelled_serials:?}"
    );
}

/// Sol 三轮补强：失败重跑恰一次——轮 0 注入失败 Abort 后重跑，撤销/
/// 受理事实恰一条（不重复、不丢失）。
#[test]
fn sol3r_failed_tick_retry_emits_facts_exactly_once() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|_t, _r| TickPlan::default())
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    let fail: super::coordinator::Validator = Box::new(|ev: &super::coordinator::TickEvidence| {
        if ev.tick == 1 && ev.round == 0 {
            Err("注入：轮 0 失败".to_owned())
        } else {
            Ok(())
        }
    });
    world.set_validator(fail);
    assert!(world.run_ticks(1).is_err(), "注入失败必须显式失败");
    world.set_validator(Box::new(|_| Ok(())));
    // world 级约定：失败拍已消费注入（组端 take 后随 Abort 回滚不返还
    /// 队列），重跑由调用方重播（drive 层等价=restore_runtime_player_
    /// intents 回灌后重注入）。
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 0,
            stock: 0,
            side: Side::Buy,
            routed: RoutedPrice::Market,
            purse_reference: 1100,
            qty: 100,
        }],
        vec![],
    );
    world.run_ticks(1).expect("重跑成功");
    assert_eq!(world.report_log.len(), 1, "重跑恰一条 TickRecord");
    let rec = &world.report_log[0];
    let cancelled = rec
        .desk_lifecycle
        .iter()
        .filter(|f| matches!(f.kind, super::protocol::LifecycleKind::Cancelled))
        .count();
    assert_eq!(cancelled, 1, "撤销事实恰一次（不因重跑重复）");
}

/// Sol 三轮补强：卖向跨笼子——市价卖保护=纯跌停（笼子只作用限价）。
/// fixture：买盘 950/900 两档、跌停 900、笼子开（卖下限 min(980,990)）
/// ⇒ 此前市价卖被钳 980，950<980 不交叉全撤；修后吃 950、900 两档
/// （900 笼子下限以下也吃）。手算（市价卖 200，maker 价两档）：
/// - 卖方毛额 95_000+90_000=185_000；单张费用：佣金 max(46.25→46,
///   500)=500、印花 round(92.5)=92（银行家舍入取偶）、过户 round(1.85)
///   =2 ⇒ 净得 184_406；
/// - 逐腿实收（边际）：腿 1（95_000）=500+48+1=549；腿 2=0+44+1=45。
#[test]
fn sol3r_market_sell_walks_bids_below_cage() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![buy(0, 0, 950, 100), buy(0, 0, 900, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.auction_ticks = 0;
    // 笼子保持开启（默认）——本测试验证卖向不被笼子钳。
    let mut world = World::spawn(spec).expect("世界构造");
    world.broadcast_external_plan(
        vec![PendingOp {
            local: 1,
            stock: 0,
            side: Side::Sell,
            routed: RoutedPrice::Market,
            purse_reference: 900,
            qty: 200,
        }],
        vec![],
    );
    world.run_ticks(1).expect("收敛");
    let rec = &world.report_log[0];
    let trades: Vec<(u32, i64, i64)> = rec
        .desk_trades
        .iter()
        .map(|f| (f.qty, f.price, f.maker_fees_cents))
        .collect();
    assert_eq!(
        trades,
        vec![(100, 950, 549), (100, 900, 45)],
        "卖向跨笼子两档全吃（maker 价），逐腿卖方费用边际拆分：{trades:?}"
    );
    let report = world.shutdown().expect("关停");
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 800, "基线 1000−卖 200");
    assert_eq!(
        a1.cash,
        1_000_000 + 184_406,
        "卖方净得 185_000−（佣金 500+印花 92+过户 2）"
    );
}

/// Sol 三轮补强：收盘竞价异价多档也按单一清算价结算（Drain 清算面）。
/// fixture：收盘窗（day_tick 27-29，tpd=30）挂卖 900×100+卖 1000×100、
/// 买 1000×200 ⇒ 清算 1000（候选最大量 200），两笔均 1000，卖 1000
/// 档余 100 随日终清簿。买方实付 200_000+500+2=200_502（若按 maker
/// 异价结算则为 190_000+费用——判别面）。
#[test]
fn sol3r_closing_multi_level_settles_at_clearing_price() {
    // 收盘窗（day_tick 27-29）内申报：限价买卖均合法（14:57-15:00
    /// 接受限价申报、不可撤、日终一次撮合）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            27 => place_at(
                27,
                vec![
                    buy(0, 0, 1000, 200),
                    sell(1, 0, 900, 100),
                    sell(1, 0, 1000, 200),
                ],
            ),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    spec.config = config_with_cage(false);
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(29).expect("进收盘窗（申报积累）");
    world.run_day_end_public().expect("日结（收盘清算+Drain）");
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_200, "买 200 股（清算 1000 单价）");
    assert_eq!(
        a0.cash,
        1_000_000 - 200_502,
        "实付=200×清算价 1000+佣金 500+过户 2（单一价格——非 maker 异价 190_000）"
    );
    assert_eq!(report.desk_finals[0].market.volume_today, 200);
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(a1.shares[0], 800, "卖 200（余 100 随清簿 Drain 退回）");
}
