//! 刀 2a 合同（协调者刀单裁决②，2026-10-19）：desk 投影轮间缓存
//! （dirty 化）——`build_public`/`build_resting_facts` 每轮全量构建改
//! 版本键缓存复用。等价判据：缓存路径与全量重建**逐字段逐 tick 相等**
//!（发布面一致性守卫的 desk 侧前置——J6 承接发布面终验）。
//!
//! fixture 覆盖面（缓存失效路径全谱）：挂单（簿变）/成交（簿+行情变）/
//! 撤单（簿变）/空轮（缓存命中路径）/相位切换（CallAuction→PreOpen→
//! Continuous→ClosingAuction——公开揭示面随相位变化，缓存键含相位）/
//! 日结 Drain 清簿（簿大失效）/跨日（相位回卷）。

use super::desk::{
    set_desk_projection_cache_disabled_for_test, set_desk_projection_invalidation_skip_for_test,
};
use super::group::{CancelSpec, GroupScript, PlaceSpec, TickPlan};
use super::protocol::StockPublic;
use super::skeleton_tests::basic_spec;
use super::world::{World, WorldSpec};
use super::GroupIdx;
use crate::orderbook::Side;

fn buy(local: u32, stock: u16, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec { local, stock, side: Side::Buy, price, qty }
}

fn sell(local: u32, stock: u16, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec { local, stock, side: Side::Sell, price, qty }
}

fn place_at(_tick: u64, places: Vec<PlaceSpec>) -> TickPlan {
    TickPlan { places, ..TickPlan::default() }
}

/// 相位布局：tpd=10（短日——快覆盖日结/跨日）、auction_ticks=0（无
/// 开盘竞价窗）、closing=3（day_tick 8-10 收盘竞价）。跑 13 tick=
/// 一整日+次日 3 tick。成交开局照搬 desk_semantics::matching_walks
/// 的已验证形态（tick 1 三档卖+tick 2 买吃）。
fn spec_for_run() -> WorldSpec {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            // 三档卖单 1010/1005/1000（matching_walks 原样开局）。
            1 => place_at(
                1,
                vec![sell(1, 0, 1010, 100), sell(1, 0, 1005, 100), sell(1, 0, 1000, 100)],
            ),
            // 买单限价 1010 全吃三档（成交 300——簿+行情双变主路径）。
            2 => place_at(2, vec![buy(0, 0, 1010, 300)]),
            // 买单 1005 挂簿（无交叉——resting 增加）。
            3 => place_at(3, vec![buy(0, 0, 1005, 100)]),
            // 4-5：空轮（无 ops——缓存命中路径）。
            // 撤单（簿变——resting 失效；全簿对齐撤）。
            6 => TickPlan {
                places: Vec::new(),
                cancels: vec![CancelSpec { local: 0, stock: 0 }],
                ..TickPlan::default()
            },
            // 新卖单挂簿（簿变）。
            7 => place_at(7, vec![sell(1, 0, 1002, 50)]),
            // 8-10 收盘竞价（相位切换——公开揭示面只显示截点前老单）。
            // 次日（跨日 Drain 清簿+相位回卷）：挂单重建簿。
            12 => place_at(12, vec![sell(1, 0, 1003, 80)]),
            _ => TickPlan::default(),
        })
    };
    let mut spec = basic_spec(1, 1, 2, Box::new(script));
    spec.ticks_per_day = 10;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    spec
}

/// 单跑采集：35 tick 逐拍发布版本 desks 域 view 快照+终态守恒锚。
fn run_and_collect(disable_cache: bool) -> (Vec<Vec<StockPublic>>, i128, u64) {
    set_desk_projection_cache_disabled_for_test(disable_cache);
    let mut world = World::spawn(spec_for_run()).expect("世界构造");
    let mut snapshots: Vec<Vec<StockPublic>> = Vec::new();
    for _ in 0..13u64 {
        world.run_ticks(1).expect("单 tick 收敛");
        let version = world.publication().load();
        snapshots.push(version.desks.iter().map(|s| (*s.view).clone()).collect());
    }
    let report = world.shutdown().expect("关停");
    let cash: i128 = report
        .group_finals
        .iter()
        .flat_map(|f| f.accounts.iter().map(|a| a.cash as i128))
        .sum();
    // 跨日 fixture 的终态 volume_today=末日量（次日可能 0）——成交锚
    // 改取快照序列内最大当日量（跨双跑一致判据用现金守恒+逐 tick 快照）。
    let volume: u64 = snapshots
        .iter()
        .map(|snap| snap.iter().map(|v| v.volume_today).sum())
        .max()
        .unwrap_or(0);
    set_desk_projection_cache_disabled_for_test(false);
    (snapshots, cash, volume)
}

/// 刀 2a 合同（等价+突变判别合并——旋钮为进程级 static，两判据必须
/// 串行防并行测试互相污染）：
/// ①双跑等价：禁缓存（直构建独立权威源）vs 默认（缓存路径），同
/// fixture 同种子——逐 tick view 序列**整结构相等**（缓存不得改变
/// 任何观察面值），终态守恒锚双跑一致（缓存不得扰动结算面）。
/// ②突变判别（批 1 红先行锚——桩阶段本段红=缓存未接入的证据；批 2
/// 实现后绿）：失效跳过（写点不推进版本）时缓存臂必须陈旧分叉。
#[test]
fn desk_projection_cache_contract_equivalence_and_stale_mutation() {
    // ── ① 等价（双跑对照） ──
    let (direct, cash_a, vol_a) = run_and_collect(true);
    let (cached, cash_b, vol_b) = run_and_collect(false);
    assert_eq!(direct.len(), 13);
    assert_eq!(cached.len(), 13, "双跑 tick 数一致");
    for (tick, (a, b)) in direct.iter().zip(cached.iter()).enumerate() {
        assert_eq!(
            a, b,
            "tick {} 发布面 desks view 必须整结构相等（缓存不得改值）",
            tick + 1
        );
    }
    assert_eq!(cash_a, cash_b, "终态现金守恒锚双跑一致");
    assert_eq!(vol_a, vol_b, "终态成交量锚双跑一致");
    assert!(vol_a > 0, "fixture 必须产生成交（否则失效路径无覆盖）");
    // ── ② 突变判别（红先行→批 2 绿） ──
    set_desk_projection_invalidation_skip_for_test(true);
    // 直构建臂不受 skip 旋钮影响（正交：skip 只作用于缓存命中的失效判定）。
    let (direct2, _, _) = run_and_collect(true);
    // 缓存臂+跳过失效：写点不推进版本 ⇒ 成交后缓存不失效 ⇒ 陈旧。
    let (stale_cached, _, _) = run_and_collect(false);
    set_desk_projection_invalidation_skip_for_test(false);
    assert_ne!(
        direct2, stale_cached,
        "跳过失效必须使缓存臂陈旧（两臂相等=缓存未接入或失效无效果——判别面红）"
    );
    // 旋钮复位无残留：复位后缓存臂恢复与直构建等价。
    let (direct3, _, _) = run_and_collect(true);
    let (cached3, _, _) = run_and_collect(false);
    assert_eq!(
        direct3, cached3,
        "旋钮复位后缓存臂必须恢复等价（无残留污染）"
    );
}
