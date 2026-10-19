//! B2 件 2：desk 分钟K观察面语义测试（对照 18725d07 `retained_history.rs`
//! 的 `MinuteBar` 语义基线重立）。
//!
//! 语义合同（与旧契约逐条对齐）：
//! - 分钟归一在 desk 侧：集合竞价完成成交 → 固定分钟 565（OpenAuction）；
//!   收盘竞价清算成交 → 900（ClosingAuction）；连续竞价成交 → 墙钟分钟
//!   （旧管线按 `CivilInstant.second_of_day()/60`；desk 按 tick 线性映射
//!   到 240 游戏分钟后折墙钟 570..690 / 780..900）。
//! - 零成交分钟不造 bar（观察面占位收盘价由 `minute_closes` 承载——两读
//!   面语义分离，旧管线 `record()` 只消费真实成交的同式）。
//! - OHLC/量/额/笔数并入既有成交更新循环（不加第二遍遍历）。
//! - Abort 回滚（undo 快照含桶尾长）与跨日 roll_day 清桶。

use super::coordinator::Validator;
use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::skeleton_tests::basic_spec;
use super::world::{World, WorldSpec};
use super::GroupIdx;
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use std::sync::Arc;
use std::time::Duration;

/// 无笼子测试配置（隔离价格带变量——与 desk_semantics_tests 同参）。
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
            Money::from_cents(1_000_000_000_000),
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

fn empty_script() -> Box<dyn Fn(GroupIdx) -> GroupScript> {
    Box::new(|_| Box::new(|_t, _r| TickPlan::default()))
}

/// tpd=30 / auction=0 / closing=3（basic_spec 缺省形态；continuous=27）。
/// tick1 的连续游戏分钟 idx = floor(1*240/27) = 8 → 墙钟 578。
fn continuous_world(script: Box<dyn Fn(GroupIdx) -> GroupScript>) -> World {
    let mut spec = basic_spec(1, 1, 3, script);
    spec.config = config_no_cage();
    World::spawn(spec).expect("世界构造")
}

#[test]
fn continuous_fills_build_exact_minute_ohlc_volume_turnover_and_count() {
    // tick1 三档卖单 900/1000/1100 各 100 股 + 一笔限价 1100 买单 300：
    // 同 tick 逐档成交三笔（900→1000→1100），同桶聚合
    // open=900 high=1100 low=900 close=1100，量 300、额 300_000、笔 3。
    // 手算 oracle 对照旧基线 real_trades_build_exact_minute_ohlcv。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![
                    sell(1, 900, 100),
                    sell(1, 1000, 100),
                    sell(2, 1100, 100),
                    buy(0, 1100, 300),
                ];
            }
            plan
        })
    };
    let mut world = continuous_world(Box::new(script));
    world.run_ticks(1).expect("tick1 收敛");
    let report = world.shutdown().expect("关停");
    let bars = &report.desk_finals[0].market.minute_bars;
    assert_eq!(bars.len(), 1, "同 tick 三笔成交聚合为恰一个分钟桶");
    let bar = &bars[0];
    assert_eq!(bar.minute_of_day, 570, "tick1 → 首个新覆盖分钟 idx0 → 墙钟 570");
    assert_eq!(bar.phase, super::protocol::MinuteBarPhase::Continuous);
    assert_eq!(
        (bar.open.cents(), bar.high.cents(), bar.low.cents(), bar.close.cents()),
        (900, 1100, 900, 1100)
    );
    assert_eq!(bar.volume_shares, 300);
    assert_eq!(bar.turnover_cents, 300_000_u128);
    assert_eq!(bar.trade_count, 3);
}

#[test]
fn zero_volume_ticks_do_not_create_minute_bars() {
    // 零成交 tick 不造 bar（对齐旧契约：观察面 minute_closes 允许占位
    // 收盘价，分钟K桶只承载真实成交）。
    let mut world = continuous_world(empty_script());
    world.run_ticks(5).expect("空转 5 tick");
    let report = world.shutdown().expect("关停");
    let market = &report.desk_finals[0].market;
    assert!(market.minute_bars.is_empty(), "零成交不得伪造分钟 bar");
    assert!(!market.minute_closes.is_empty(), "观察面占位收盘价不受影响");
}

#[test]
fn fills_across_minute_boundaries_split_into_ordered_buckets() {
    // tpd=60 / auction=0 / closing=0（continuous=60）——1 tick 恰一游戏分钟：
    // tick1→idx0(墙钟570)、tick2→idx1(571)。两 tick 各成交一笔 → 两桶，
    // 键严格递增。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1000, 100), buy(0, 1000, 100)];
            } else if tick == 2 {
                plan.places = vec![sell(1, 1005, 100), buy(0, 1005, 100)];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    spec.ticks_per_day = 60;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("2 tick 收敛");
    let report = world.shutdown().expect("关停");
    let bars = &report.desk_finals[0].market.minute_bars;
    assert_eq!(bars.len(), 2, "两个 tick 分属两个分钟桶");
    assert_eq!(bars[0].minute_of_day, 570, "tick1 → idx0 → 570");
    assert_eq!(bars[0].close.cents(), 1000);
    assert_eq!(bars[1].minute_of_day, 574, "tick2 → idx=floor(1*240/60)=4 → 574");
    assert_eq!(bars[1].close.cents(), 1005);
    assert!(
        bars.windows(2).all(|pair| {
            (pair[0].minute_of_day, pair[0].phase) < (pair[1].minute_of_day, pair[1].phase)
        }),
        "桶键严格递增"
    );
}

#[test]
fn opening_auction_clearing_fills_land_at_minute_565() {
    // auction_spec 形态（tpd=30 / auction=6 / closing=3）：tick1..3 申报窗
    // 挂交叉限价单，tick4 离开申报窗触发开盘清算——成交归固定分钟
    // 565 / OpenAuction（旧契约：竞价完成事实取 565，不从指示价反推）。
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
    world.run_ticks(4).expect("tick4 触发开盘清算");
    let report = world.shutdown().expect("关停");
    let bars = &report.desk_finals[0].market.minute_bars;
    assert_eq!(bars.len(), 1, "清算成交恰一桶");
    assert_eq!(bars[0].minute_of_day, 565);
    assert_eq!(bars[0].phase, super::protocol::MinuteBarPhase::OpenAuction);
    assert_eq!(bars[0].volume_shares, 100);
    assert_eq!(bars[0].trade_count, 1);
}

#[test]
fn closing_auction_drain_fills_land_at_minute_900() {
    // 收盘竞价窗（tick27..29）挂交叉限价单；tick30 日末 → 下一拍 run_ticks
    // 触发日结 Drain 收盘清算——成交归固定分钟 900 / ClosingAuction。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 28 {
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
    world.run_ticks(30).expect("跑到日末");
    let report = world.shutdown().expect("关停（Drain 收盘清算）");
    // 期末 Drain 应答捎带桶（跨日形态 live 已并入——观察面统一走
    /// 报告字段；desk_finals 的 live 桶在跨日形态已清）。
    let bars = report.drained_minute_bars.into_iter().next().flatten().unwrap_or_default();
    assert_eq!(bars.len(), 1, "收盘清算恰一桶");
    assert_eq!(bars[0].minute_of_day, 900);
    assert_eq!(bars[0].phase, super::protocol::MinuteBarPhase::ClosingAuction);
    assert_eq!(bars[0].volume_shares, 100);
}

#[test]
fn abort_replay_rolls_back_minute_bars_exactly_once() {
    // tick1 成交 → 注入失败 Abort（Market 快照须含分钟桶尾长）→ 重跑同批
    // 单再成交。回滚缺失则桶残留翻倍（两桶/量 200/笔 2）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1000, 100), buy(0, 1005, 100)];
            }
            plan
        })
    };
    let mut world = continuous_world(Box::new(script));
    let fail: Validator = Box::new(|ev| {
        if ev.tick == 1 { Err("分钟桶回滚注入".to_owned()) } else { Ok(()) }
    });
    world.set_validator(fail);
    assert!(world.run_ticks(1).is_err(), "tick1 注入失败（Abort）");
    world.set_validator(Box::new(|_| Ok(())));
    world.run_ticks(1).expect("tick1 重跑");
    let report = world.shutdown().expect("关停");
    let bars = &report.desk_finals[0].market.minute_bars;
    assert_eq!(bars.len(), 1, "回滚后恰一桶");
    assert_eq!(bars[0].volume_shares, 100, "回滚后恰 100 股");
    assert_eq!(bars[0].trade_count, 1, "回滚后恰 1 笔");
}

#[test]
fn second_trading_day_minute_bars_restart_empty() {
    // 跨日 roll_day 清当日桶（对齐 clear_runtime_minute_history 的跨日
    // 对齐语义）——第二日桶从零起，第一日数据不残留。
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
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(31).expect("跨入第二日");
    let views = world.public_views().to_vec();
    assert!(
        views[0].minute_bars.is_empty(),
        "第二日桶从零起（roll_day 清桶）——实际残留 {} 条",
        views[0].minute_bars.len()
    );
    let _ = world.shutdown();
}

#[test]
fn minute_bars_publish_through_the_public_view() {
    // 发布贯通：StockPublic 扩字段后，发布版本（publication）的分钟桶
    // 与 desk 权威逐位一致——查询面（B2 件 4/5）的数据源合同。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![sell(1, 1000, 100), buy(0, 1005, 100)];
            }
            plan
        })
    };
    let mut world = continuous_world(Box::new(script));
    world.run_ticks(2).expect("2 tick");
    let published = world.publication().load();
    let view = &published.desks[0].view;
    let bars = &view.minute_bars;
    assert_eq!(bars.len(), 1, "发布版本携带当日分钟桶");
    assert_eq!(bars[0].minute_of_day, 570);
    // 成交价=maker 价 1000（价格改善归买方）→ 额=1000×100。
    assert_eq!(bars[0].turnover_cents, 100_000_u128);
    assert_eq!(bars[0].volume_shares, 100);
}
