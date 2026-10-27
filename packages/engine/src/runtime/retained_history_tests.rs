//! B2 件 4：保留历史快照链（日结冻结 desk 日快照——对照 18725d07
//! `RetainedHistoryDay`/`SharedHistoryDay` 语义基线）。
//!
//! 语义合同（Sol 设计门裁决）：
//! - 日结 Drain 应答捎带当日分钟K桶（roll_day 清桶前的唯一窗口）；
//! - 协调者索引持不可变句柄（`Arc<RetainedDaySnapshot>`——封闭块共享，
//!   全历史保留（ADR-0034），五日仅显示窗），经发布面读取；
//! - 日结全成功后 history/日期/settled_through 原子发布——版本
//!   `generation` 区分同 tick 的 civil 推进。

use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::publication::RetainedDaySnapshot;
use super::skeleton_tests::basic_spec;
use super::world::{World, WorldSpec};
use super::GroupIdx;
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use std::sync::Arc;

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

/// tpd=30 / auction=6 / closing=3（basic 形态）。
fn auction_world(script: Box<dyn Fn(GroupIdx) -> GroupScript>) -> World {
    let mut spec: WorldSpec = basic_spec(1, 1, 3, script);
    spec.config = config_no_cage();
    spec.ticks_per_day = 30;
    spec.auction_ticks = 6;
    spec.closing_auction_ticks = 3;
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("世界构造")
}

#[test]
fn trading_day_end_freezes_retained_snapshot_with_exact_bars() {
    // tick1 连续成交一笔（565 无——auction=6 时 tick1 是申报窗；改用
    // tick4 开盘清算成交），跑满 30 tick + 日结：retained 链恰一条当日
    // 快照，bars 与成交逐位一致（trading=true）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places = vec![
                    PlaceSpec { local: 1, stock: 0, side: Side::Sell, price: 1000, qty: 100 },
                    PlaceSpec { local: 0, stock: 0, side: Side::Buy, price: 1000, qty: 100 },
                ];
            }
            plan
        })
    };
    let mut world = auction_world(Box::new(script));
    world.run_ticks(30).expect("跑到日末");
    world.run_day_end_public().expect("日结（Drain 捎带 bars）");
    let committed_before = world.global.committed_tick;
    let mut snapshot_seen = None;
    // 日结（Drain 捎带 bars → driver 编排冻结——本测试直接调 world 的
    // 冻结入口验证编排产物；session 侧编排在 session 层测试覆盖）。
    // 模拟 driver：取 pending bars + 组装 + freeze。
    {
        let pending = world.take_pending_day_bars();
        assert_eq!(pending.len(), 1, "恰一股 pending bars");
        let bars = pending[0].clone().expect("交易日必有 bars 投影（可为空桶）");
        assert_eq!(bars.len(), 1, "开盘清算成交恰一桶（565）");
        assert_eq!(bars[0].minute_of_day, 565);
        assert_eq!(bars[0].volume_shares, 100);
        let snapshot = Arc::new(RetainedDaySnapshot {
            date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期"),
            securities: [(
                0_u32,
                super::publication::RetainedMinuteSession {
                    trading: true,
                    bars: Arc::new(bars),
                },
            )]
            .into_iter()
            .collect(),
        });
        world.freeze_retained_day(Arc::clone(&snapshot));
        snapshot_seen = Some(snapshot);
    }
    let retained = world.retained_days();
    assert_eq!(retained.len(), 1, "保留链恰一条（ADR-0034 全保留）");
    assert!(Arc::ptr_eq(&retained[0], &snapshot_seen.expect("已冻结")), "协调者持同一不可变句柄");
    let committed_final = world.global.committed_tick;
    let _ = world.shutdown();
    assert!(committed_final >= committed_before, "tick 前进不变量");
}

#[test]
fn day_end_drain_carries_minute_bars_before_roll_day() {
    // 两日各一笔成交（tick1 首日、tick31 第二日申报窗），显式完成两次市场日终，
    // 再按冻结 Calendar 推进 civil date；待取桶仍是第二日当日桶，首日不重放。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 || tick == 31 {
                plan.places = vec![
                    PlaceSpec { local: 1, stock: 0, side: Side::Sell, price: 1000, qty: 100 },
                    PlaceSpec { local: 0, stock: 0, side: Side::Buy, price: 1005, qty: 100 },
                ];
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    spec.ticks_per_day = 30;
    spec.auction_ticks = 6;
    spec.closing_auction_ticks = 3;
    let first_date = spec.civil_start_date;
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let mut world = World::spawn(spec, calendar.clone()).expect("显式冻结Calendar世界构造");
    let second_date = calendar.next_trading_day(
        crate::calendar::CalendarExchange::Sse, first_date,
    ).unwrap();
    let third_date = calendar.next_trading_day(
        crate::calendar::CalendarExchange::Sse, second_date,
    ).unwrap();
    world.run_ticks(30).expect("首日全部市场拍");
    world.run_day_end_public().expect("显式首日市场日终");
    world.set_civil_date(second_date);
    world.run_ticks(30).expect("第二日全部市场拍");
    world.run_day_end_public().expect("显式第二日市场日终");
    world.set_civil_date(third_date);
    world.run_ticks(1).expect("第三日首拍保留第二日待取桶");
    let pending = world.take_pending_day_bars();
    let bars = pending[0].clone().expect("day1 bars");
    assert_eq!(bars.len(), 1, "第二日桶恰一桶（565 开盘清算）");
    assert_eq!(bars[0].volume_shares, 100);
    let _ = world.shutdown();
}

#[test]
fn published_retained_and_generation_advance_atomically_on_freeze() {
    // 发布面：冻结后 PublishedTick.retained 携带新链、generation 递增
    // （同 tick 的 civil 推进版本区分）；未冻结时沿用旧 Arc（封闭块共享
    // ——非冻结 tick 不复制历史）。
    let script = |_: GroupIdx| -> GroupScript {
        Box::new(|_t, _r| TickPlan::default())
    };
    let mut spec = basic_spec(1, 1, 3, Box::new(script));
    spec.config = config_no_cage();
    spec.ticks_per_day = 30;
    spec.auction_ticks = 6;
    spec.closing_auction_ticks = 3;
    let first_date = spec.civil_start_date;
    let calendar = crate::calendar::TradingCalendar::current_default_calendar()
        .expect("显式 raw Calendar 政策合法");
    let next_date = calendar.next_trading_day(
        crate::calendar::CalendarExchange::Sse, first_date,
    ).expect("fixture 下一 SSE trading date 合法");
    let mut world = World::spawn(spec, calendar).expect("世界构造");
    world.run_ticks(1).expect("tick1");
    let published_before = world.publication().load();
    assert!(
        published_before.retained.days.is_empty(),
        "冻结前发布版本保留链为空"
    );
    let gen_before = published_before.generation;
    world.run_ticks(29).expect("首日剩余二十九拍");
    world.run_day_end_public().expect("显式首日市场日终");
    world.set_civil_date(next_date);
    world.run_ticks(1).expect("次日首拍保留同一发布口径");
    let empty = Arc::new(Vec::new());
    world.freeze_retained_day(Arc::new(RetainedDaySnapshot {
        date: first_date,
        securities: [(0_u32, super::publication::RetainedMinuteSession {
            trading: true,
            bars: empty,
        })]
        .into_iter()
        .collect(),
    }));
    let published_after = world.publication().load();
    assert_eq!(published_after.tick, published_before.tick + 30, "tick 正常推进");
    assert_eq!(published_after.retained.days.len(), 1, "冻结原子进发布版本");
    assert!(
        published_after.generation > gen_before,
        "generation 区分同 tick 的 civil/保留链推进"
    );
    let _ = world.shutdown();
}
