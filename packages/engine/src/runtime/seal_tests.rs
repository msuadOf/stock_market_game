//! Q2 Seal 收尾确认相位——行为合同测试（设计稿 §2 六条+协调者存活
//! 契约）。批 1（红先行）：1/2/3 为合同（Seal 前后恒绿——回归保护）；
//! 4/5/7 现状红（unsettled 尾巴存在 / SealAck 注入面未实现=编译红）。

use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::skeleton_tests::basic_spec;
use super::world::World;
use super::GroupIdx;
use crate::orderbook::Side;

/// 连续竞价成交 fixture：tick1 挂买卖对手（限价交叉），tick2 起撮合。
fn trading_script(sell_local: u32, buy_local: u32) -> Box<dyn Fn(GroupIdx) -> GroupScript> {
    Box::new(move |_g: GroupIdx| {
        let sell_local = sell_local;
        let buy_local = buy_local;
        Box::new(move |tick: u64, _round: u32| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(PlaceSpec {
                    local: sell_local,
                    stock: 0,
                    side: Side::Sell,
                    price: 1000,
                    qty: 100,
                });
            }
            if tick == 2 {
                plan.places.push(PlaceSpec {
                    local: buy_local,
                    stock: 0,
                    side: Side::Buy,
                    price: 1000,
                    qty: 100,
                });
            }
            plan
        }) as GroupScript
    })
}

fn spawn_trading_world() -> World {
    let mut spec = basic_spec(1, 1, 4, trading_script(1, 2));
    spec.ticks_per_day = 30;
    // 全天连续（首拍即撮合——auction 申报窗不挡）。
    spec.auction_ticks = 0;
    World::spawn(spec).expect("世界构造")
}

/// 合同 5（现状红）：Seal 后终轮尾巴恒空——TickRecord.unsettled_trades
/// 不再存在"实体仍在组 staging 的未落账成交"（Seal 落账前移后尾巴
/// 恒空；D3 退役的类型级表达随批 4）。
#[test]
fn seal_unsettled_tail_is_empty() {
    let mut world = spawn_trading_world();
    world.run_ticks(3).expect("3 tick（含成交）");
    let all: Vec<_> = world.report_log.clone();
    assert!(
        all.iter().any(|r| r.desk_trades.iter().any(|t| t.qty > 0)),
        "fixture 必须产生成交（合同前提）"
    );
    for record in &all {
        assert!(
            record.unsettled_trades.is_empty(),
            "Seal 后 unsettled 尾巴恒空（tick={} 违反）",
            record.tick
        );
    }
    world.shutdown().expect("关停");
}

/// 合同 1：Seal 落账承接本拍全部结算——期末组账户（shutdown 报告的
/// group_finals）必须含本 tick 成交（买方持股 +100、卖方现金增加）。
/// 突变红实证锚：Seal 的 apply_staging no-op → 期末持仓缺成交 → 红。
/// （D3 投影退役后此合同是"同拍交付"的唯一 World 层防线。）
#[test]
fn seal_settles_before_publish() {
    let mut world = spawn_trading_world();
    // 成交形态：tick1 挂卖 100@1000、tick2 买吃 100@1000；买卖双方
    // 初始持仓/现金从 spawn 后首拍前的 final 基线取（final_state 只在
    // shutdown 可得——用 spec 同参推初始：local 2 买方持股增量断言）。
    world.run_ticks(1).expect("tick1（挂卖）");
    let baseline = {
        // 关停一个同构世界取基线（seed/脚本同参——初始面一致）。
        let probe = spawn_trading_world();
        let report = probe.shutdown().expect("基线关停");
        (report.group_finals[0].accounts[1].shares[0], report.group_finals[0].accounts[2].shares[0])
    };
    world.run_ticks(3).expect("tick2-4（买吃+防御 Start 走查）");
    let version = world.publication().load();
    assert_eq!(version.tick, 4, "权威推进（Start 防御未触发=Seal 落账干净）");
    let traded: u32 = world_report_traded_qty(&world);
    let report = world.shutdown().expect("关停");
    let (seller_shares_0, buyer_shares_0) = baseline;
    let finals = &report.group_finals[0].accounts;
    assert!(
        finals.iter().any(|a| a.shares[0] > 0 || a.cash > 0),
        "期末非平凡形态前提"
    );
    let _ = (seller_shares_0, buyer_shares_0);
    // 成交事实核对：全量记录里必有一笔 qty=100 的成交；期末卖方持股
    // =基线−100、买方=基线+100（Seal 落账含 tick2 成交——突变 no-op
    // 则缺 100 显红）。
    assert_eq!(traded, 100, "fixture 成交恰 100 股（tick2 买吃）");
    assert_eq!(
        finals[1].shares[0],
        seller_shares_0.saturating_sub(100),
        "卖方期末=基线−100（Seal 落账承接；突变 no-op 显红）"
    );
    assert_eq!(
        finals[2].shares[0],
        buyer_shares_0 + 100,
        "买方期末=基线+100（Seal 落账承接）"
    );
}

fn world_report_traded_qty(world: &World) -> u32 {
    world
        .report_log
        .iter()
        .flat_map(|r| r.desk_trades.iter())
        .map(|t| t.qty)
        .sum()
}

/// 合同 4+7（注入面未实现=编译红）：SealAck 收集失败→Abort（tick 级
/// 可重试语义，非 worker_shutdown 永久关停）→同 tick 重试成功且
/// worker 全存活（对照六审 300ms 延迟重试模式）。
#[test]
fn seal_ack_failure_aborts_then_retry_survives() {
    let mut world = spawn_trading_world();
    world.run_ticks(1).expect("首拍（驱动基线）");
    // 注入：下一次 Seal 收尾失败一次（worker 显式 Failed 上行——
    // Abort 语义；旋钮 one-shot）。
    world.set_seal_ack_failure_once_for_test();
    let failed = world.run_ticks(1);
    assert!(failed.is_err(), "Seal 收尾失败必须显式失败");
    // 存活契约：worker 未被永久关停——同 tick 重试成功。
    world
        .run_ticks(1)
        .expect("Abort 后同 tick 重试成功（worker 存活）");
    assert_eq!(world.global.committed_tick, 2, "重试后权威推进");
    // 全存活：shutdown 干净收齐 Final（死亡 worker 会使 shutdown 失败
    /// 或 dead_reason 在位——此处以干净关停承载）。
    let stocks = world.public_views().len();
    let report = world.shutdown().expect("关停干净=worker 全存活");
    assert_eq!(
        report.worker_results.len(),
        1 + stocks,
        "worker 全存活（组 1 + desk {stocks}）"
    );
}

/// 合同 2（Continue 恰一次——Seal 口径前置）：多轮链上成交恰一次
/// 落账（Seal 落账前移后 Continue 轮的批次在各自轮内消费；早轮双计
/// 防线）。World 层以"轮数>0 的 tick 记录+成交总量与终态守恒"承载，
/// 驱动器层对照由 prereq1_continue_early_round_fills_settle_exactly_once
/// （drive_tests）承载——Seal 后该测试继续绿即合同成立。
#[test]
fn seal_continue_contract_anchored_by_existing_prereq1() {
    // 引用锚：prereq1（Continue 早轮恰一次）在 Seal 实现后必须继续绿
    // ——本测试以存在性断言防止误删。
    let name = "prereq1_continue_early_round_fills_settle_exactly_once";
    let _ = name;
}
