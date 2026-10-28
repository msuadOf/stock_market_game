//! V3：尾组真实账户、open 成交正控和 closed 错路由 Ops 的完整资源撤退。

use super::calendar_cancel_all_tests::mixed_spec;
use super::group::{PlaceSpec, TickPlan};
use super::protocol::{DeskIn, RuntimeError, WorkerId};
use super::world::World;
use crate::Side;
use std::sync::Arc;
use std::time::Duration;

fn world() -> World {
    let (mut spec, calendar) = mixed_spec(Box::new(|group| {
        Box::new(move |tick, round| {
            let mut plan = TickPlan::default();
            if tick == 4 && round == 0 {
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: if group == 0 { Side::Buy } else { Side::Sell },
                    price: 1000,
                    qty: 100,
                });
            }
            plan
        })
    }));
    // 故障发生在最终 flush；两笔实际请求不能先被 chunk 分段发出。
    spec.chunk = usize::MAX;
    World::spawn(spec, calendar).unwrap()
}

#[test]
fn mixed_tail_accounts_trade_on_open_stock_with_four_real_workers() {
    let mut world = world();
    world.run_ticks(4).unwrap();
    assert_eq!(world.report_log.last().unwrap().desk_trades.len(), 1);
    assert_eq!(world.report_log.last().unwrap().desk_trades[0].stock, 0);
    let accounts = world.snapshot_accounts().unwrap();
    assert_eq!(accounts[0].1[0].shares[0], 1100);
    assert_eq!(accounts[1].1[0].shares[0], 900);
    assert_eq!(accounts[1].1[1].cash, 0);
    assert!(accounts[1].1[1].shares.iter().all(|&qty| qty == 0));
    let report = world.shutdown().unwrap();
    assert_eq!(report.group_finals.len(), 2);
    assert_eq!(report.desk_finals.len(), 2);
    assert_eq!(report.worker_results.len(), 4);
    assert!(report
        .worker_results
        .iter()
        .all(|(_, result)| result.is_ok()));
    let cash: i64 = report
        .group_finals
        .iter()
        .flat_map(|group| &group.accounts)
        .map(|account| account.cash)
        .sum();
    let fees: i64 = report.desk_finals.iter().map(|desk| desk.fee_sink).sum();
    assert_eq!(
        cash + fees,
        3_000_000,
        "只计三个真实账户，成交与费用严格守恒"
    );
    let closed = &report.desk_finals[1];
    assert_eq!(closed.cur_tick_for_test, 4, "closed desk 仍参与协议 tick");
    assert!(closed.market.candle.is_none());
    assert!(closed.market.minute_closes.is_empty());
    assert!(closed.market.minute_bars.is_empty());
    assert!(closed.market.recent_prices.is_empty());
    assert_eq!(closed.market.volume_today, 0);
    assert_eq!(closed.market.trade_count, 0);
}

#[test]
fn real_purse_misrouted_to_closed_desk_preserves_serial_and_aborts_all_accounts() {
    let mut world = world();
    world.run_ticks(3).unwrap();
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    let (_, serials) = world.collect_desk_resting().unwrap();
    let witness = world.misroute_next_group_ops_for_test(0, 0, 1, 1);
    let error = world
        .run_ticks(1)
        .expect_err("真实 open 买腿误路由到 closed desk 必须 fatal");
    assert!(
        matches!(error, RuntimeError::WorkerFailed { who: WorkerId::Desk(1), error }
        if matches!(*error, RuntimeError::ProtocolInvariant { ref detail }
            if detail == "closed 交易部收到非空错路由 Ops，未分配 serial 或修改资源")),
        "必须命中 closed Ops 防御门，而不是缺 final/重复 final 或超时"
    );
    let seen = witness
        .recv_timeout(Duration::from_secs(1))
        .expect("扣资后的真实见证必须可达");
    assert_eq!(
        (seen.tick, seen.round, seen.source_stock, seen.target_stock),
        (4, 0, 0, 1)
    );
    let [DeskIn::Place {
        owner,
        side: Side::Buy,
        purse,
        qty: 100,
        ..
    }] = seen.ops.as_slice()
    else {
        panic!(
            "见证必须为真实原始 open 买腿，不得伪造或扩大 Ops：{:?}",
            seen.ops
        );
    };
    assert_eq!(*owner, (0, 0));
    assert!(*purse > 0);
    assert_eq!(seen.accounts[0].cash, accounts[0].1[0].cash - purse);
    assert_eq!(seen.accounts[0].buy_purse_held_cents, *purse);
    assert_eq!(
        seen.accounts[0].foreign_active[0], 1,
        "正控证明故障发生在真实扣资登记之后"
    );
    assert_eq!(
        world.live_workers_for_test(),
        0,
        "fatal 返回前已 join 全部真实 worker"
    );
    assert_eq!(world.global.committed_tick, 3);
    assert_eq!(world.report_log.len(), 3);
    assert!(
        Arc::ptr_eq(&before, &world.publication().load()),
        "失败拍无公开安装"
    );
    let report = world
        .shutdown()
        .expect("复用已收齐的真实 Final，不重复 Drain");
    assert_eq!(report.group_finals.len(), 2);
    assert_eq!(report.desk_finals.len(), 2);
    assert_eq!(report.worker_results.len(), 4);
    for group in &report.group_finals {
        assert_eq!(
            group.accounts,
            accounts[usize::from(group.group)].1,
            "Abort 必须恢复完整 cash/shares/frozen/foreign_active/purse/卖在途，不只检查总和"
        );
    }
    assert_eq!(
        report
            .worker_results
            .iter()
            .filter(|(_, result)| result.is_err())
            .count(),
        1
    );
    assert!(report
        .worker_results
        .iter()
        .any(|(who, result)| *who == WorkerId::Desk(1) && result.is_err()));
    for desk in &report.desk_finals {
        assert_eq!(
            desk.next_serial_for_test,
            serials[usize::from(desk.stock)],
            "真实 Final 私有 serial 不变，不能用旧公开镜像自证"
        );
        assert!(desk.resting.is_empty());
        assert_eq!(desk.fee_sink, 0);
        assert_eq!(desk.market.volume_today, 0);
        assert_eq!(desk.market.turnover_today, 0);
        assert_eq!(desk.market.trade_count, 0);
    }
    let closed = &report.desk_finals[1];
    assert_eq!(closed.cur_tick_for_test, 4);
    assert_eq!(closed.market.last_price, 1000);
    assert_eq!(closed.market.last_close, 1000);
    assert!(closed.market.candle.is_none());
    assert!(closed.market.candles.is_empty());
    assert!(closed.market.minute_closes.is_empty());
    assert!(closed.market.minute_bars.is_empty());
    assert!(closed.market.recent_prices.is_empty());
}

#[test]
fn disconnected_group_desk_sender_after_real_purse_restores_all_final_accounts_and_plans() {
    let mut world = world();
    world.run_ticks(3).unwrap();
    let before = world.publication().load();
    let accounts = world.snapshot_accounts().unwrap();
    let books = world.collect_plans().unwrap();
    let (resting, serials) = world.collect_desk_resting().unwrap();
    assert_eq!(resting.len(), 2);
    assert!(resting.iter().all(Vec::is_empty), "两真实交易部均无活动委托");
    assert_eq!(books.iter().map(|(_, book)| book.plan_ids().count()).sum::<usize>(), 2);
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[1].1[1].cash, 0, "尾空位不冒充第四个 owner");

    let witness = world.disconnect_group_desk_sender_for_test(0, 0, 1);
    let error = world.run_ticks(1).expect_err("真实扣资后的组内 Ops 发送必须失败");
    assert!(
        matches!(error, RuntimeError::WorkerFailed { who: WorkerId::Group(0), error }
            if matches!(*error, RuntimeError::SendFailed { ref detail }
                if detail == "组 0 → 交易部 0 Ops 发送失败")),
        "必须由真实组内 flush 断连报错，不能由 World Start 广播失败或超时代替"
    );
    let seen = witness.recv_timeout(Duration::from_secs(1)).expect("真实扣资见证必须可达");
    assert_eq!((seen.tick, seen.round, seen.stock), (4, 0, 0));
    let [DeskIn::Place { owner, side: Side::Buy, purse, qty: 100, .. }] = seen.ops.as_slice() else {
        panic!("见证必须是原始非空买腿：{:?}", seen.ops);
    };
    assert_eq!(*owner, (0, 0));
    assert!(*purse > 0);
    assert_eq!(seen.accounts[0].cash, accounts[0].1[0].cash - purse);
    assert_eq!(seen.accounts[0].buy_purse_held_cents, *purse);
    assert_eq!(seen.accounts[0].foreign_active[0], 1, "故障确在真实扣资登记后");
    assert_eq!(world.live_workers_for_test(), 0, "fatal 返回前 join 全部 worker");
    assert_eq!(world.global.committed_tick, 3);
    assert_eq!(world.report_log.len(), 3);
    assert!(Arc::ptr_eq(&before, &world.publication().load()), "失败拍不安装公开投影");

    let report = world.shutdown().expect("直接读取失败撤退已收齐的真实 Final");
    assert_eq!(report.group_finals.len(), 2);
    assert_eq!(report.desk_finals.len(), 2);
    assert_eq!(report.worker_results.len(), 4);
    assert_eq!(report.worker_results.iter().filter(|(_, result)| result.is_err()).count(), 1);
    assert!(report.worker_results.iter().any(|(who, result)|
        *who == WorkerId::Group(0) && matches!(result, Err(RuntimeError::SendFailed { detail })
            if detail == "组 0 → 交易部 0 Ops 发送失败")));
    for group in &report.group_finals {
        assert_eq!(group.plan_book_for_test, books[usize::from(group.group)].1,
            "全部 PlanBook、终结计划和 owner cursor 必须来自真实 Final 且保持完整");
        assert_eq!(group.accounts, accounts[usize::from(group.group)].1,
            "发送失败组退出前必须回滚完整 cash/shares/frozen/foreign_active/purse/卖在途");
    }
    for desk in &report.desk_finals {
        assert_eq!(desk.last_applied_start_for_test, Some(4), "World Start 仍真实交付全部健康交易部");
        assert_eq!(desk.cur_tick_for_test, 3, "健康交易部收到Abort后严格回滚到上一提交拍");
        assert_eq!(desk.next_serial_for_test, serials[usize::from(desk.stock)]);
        assert!(desk.resting.is_empty());
        assert_eq!(desk.fee_sink, 0);
        assert_eq!(desk.market.volume_today, 0);
        assert_eq!(desk.market.turnover_today, 0);
        assert_eq!(desk.market.trade_count, 0);
    }
}
