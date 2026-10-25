//! #121：收盘竞价改变昨收后，次日首拍的公共行情与交易部受理基准必须一致。

use super::decision::{build_market_view, intent_to_pending};
use super::fees::buy_purse_limit;
use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::protocol::TickSnapshot;
use super::skeleton_tests::basic_spec;
use super::world::{World, WorldSpec};
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use crate::strategy::{Intent, LimitPrice};
use crate::StockId;
use std::sync::Arc;

const DAY_END_TICK: u64 = 240;
const INITIAL_PRICE: i64 = 1_000;
const CLOSING_PRICE: i64 = 1_100;
const LOT: u32 = 100;

fn partial_drain_spec() -> WorldSpec {
    let mut spec = basic_spec(
        2,
        2,
        2,
        Box::new(|_group| {
            Box::new(move |tick, _| {
                let mut plan = TickPlan::default();
                if tick == 239 {
                    for (local, side) in [(0, Side::Buy), (1, Side::Sell)] {
                        plan.places.push(PlaceSpec {
                            local,
                            stock: 0,
                            side,
                            price: 1_100,
                            qty: 100,
                        });
                    }
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 1,
                        side: Side::Buy,
                        price: 1_000,
                        qty: 100,
                    });
                }
                if tick == 240 {
                    plan.places.push(PlaceSpec {
                        local: 1,
                        stock: 1,
                        side: Side::Buy,
                        price: 1_000,
                        qty: 100,
                    });
                }
                plan
            })
        }),
    );
    spec.ticks_per_day = 240;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    spec.initial_committed_tick = 238;
    spec.initial_frozen_today = vec![vec![vec![0, 100], vec![0, 0]]; 2];
    let mut config = GameConfig::proposed_defaults();
    config.price_cage_enabled = false;
    spec.config = Arc::new(config);
    spec
}

fn world_after_successful_day_end() -> World {
    let mut world = World::spawn(partial_drain_spec()).expect("构造两股两组日结世界");
    world.run_ticks(2).expect("tick240 成功发布");
    world
        .run_day_end_public()
        .expect("日结成功且撤销窗口仍开启");
    world
}

fn assert_partial_start_keeps_the_committed_day_end(group_failure: bool) {
    let mut world = world_after_successful_day_end();
    let accounts = world.snapshot_accounts().expect("日结后的真实账户");
    assert!(accounts.iter().all(|(_, a)| a[0].frozen_today[1] == 0));
    let next_views = world.public_views().to_vec();
    assert_eq!(next_views[0].daily_upper_limit, 1_210);
    let publication = world.publication();
    let published = publication.load();
    let published_accounts: Vec<_> = published
        .groups
        .iter()
        .map(|g| (g.group, g.accounts.clone()))
        .collect();
    let generation = published.generation;
    assert_eq!(published.tick, DAY_END_TICK);

    // 真 Sender::send 断连；保留真实 Sender，终止协议须回收真实 worker。
    let error = if group_failure {
        let (dead, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        let real = world.swap_group_sender_for_test(1, dead);
        let result = world.run_ticks(1);
        world.swap_group_sender_for_test(1, real);
        result.expect_err("第一组已收到 Start 后，第二组发送失败")
    } else {
        let (dead, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        let real = world.swap_desk_sender_for_test(1, dead);
        let result = world.run_ticks(1);
        world.swap_desk_sender_for_test(1, real);
        result.expect_err("第一股已收到 Start 后，第二股发送失败")
    };
    eprintln!("partial Start group_failure={group_failure}: {error}");
    assert!(matches!(
        error,
        super::protocol::RuntimeError::SendFailed { .. }
    ));
    let abort = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.abort_day_end()));
    assert!(
        abort.is_ok(),
        "过期日结撤销不得向已 Start 的参与者发送 tick240 AbortDayEnd"
    );
    assert_eq!(
        world.public_views(),
        next_views,
        "成功日结的新基准不得退回旧昨收"
    );
    assert_eq!(world.global.committed_tick, DAY_END_TICK);
    let latest = publication.load();
    assert!(Arc::ptr_eq(&published, &latest));
    assert_eq!(latest.generation, generation);
    assert_eq!(
        latest
            .groups
            .iter()
            .map(|g| (g.group, g.accounts.clone()))
            .collect::<Vec<_>>(),
        published_accounts
    );
    assert_eq!(
        world.live_workers_for_test(),
        0,
        "返回失败前已 join 全部真实 worker"
    );
    assert!(
        matches!(world.run_ticks(1), Err(super::protocol::RuntimeError::ProtocolInvariant { detail }) if detail.contains("world 已死亡"))
    );
    assert!(
        matches!(world.run_day_end_public(), Err(super::protocol::RuntimeError::ProtocolInvariant { detail }) if detail.contains("world 已死亡"))
    );
    let report = world
        .shutdown()
        .expect("失败终止后取缓存 Final，不再次 Drain");
    assert_eq!(report.worker_results.len(), 4);
    assert!(
        report
            .worker_results
            .iter()
            .all(|(_, result)| result.is_ok()),
        "不得以 tick 错配驱退真实线程：{:?}",
        report.worker_results
    );
    assert_eq!(report.desk_finals.len(), 2);
    assert_eq!(
        report
            .group_finals
            .iter()
            .map(|g| (g.group, g.accounts.clone()))
            .collect::<Vec<_>>(),
        accounts,
        "日结解冻与回流账户不能被过期撤销倒放"
    );
    if group_failure {
        assert!(
            report.group_finals[0]
                .observed
                .contains(&super::group::Observed::Start(241)),
            "早收 Start 的真实组已推进"
        );
        assert!(!report.group_finals[1]
            .observed
            .contains(&super::group::Observed::Start(241)));
    } else {
        assert_eq!(
            report.desk_finals[0].cur_tick_for_test, 241,
            "早收 Start 的真实交易部已推进"
        );
        assert_eq!(report.desk_finals[1].cur_tick_for_test, 240);
    }
}

#[test]
fn partial_desk_start_failure_closes_day_end_abort_and_stops_world() {
    assert_partial_start_keeps_the_committed_day_end(false);
}

#[test]
fn partial_group_start_failure_closes_day_end_abort_and_stops_world() {
    assert_partial_start_keeps_the_committed_day_end(true);
}

#[test]
fn partial_start_cleanup_joins_workers_before_resuming_the_original_panic() {
    let mut spec = partial_drain_spec();
    let script_for = std::mem::replace(
        &mut spec.script_for,
        Box::new(|_| Box::new(|_, _| TickPlan::default())),
    );
    spec.script_for = Box::new(move |group| {
        let script = script_for(group);
        Box::new(move |tick, round| {
            let mut plan = script(tick, round);
            if group == 0 && tick == 241 {
                plan.panic_payload = Some("partial-start-original-panic".to_owned());
            }
            plan
        })
    });
    let mut world = World::spawn(spec).expect("构造真实 panic 世界");
    world.run_ticks(2).expect("发布240");
    world.run_day_end_public().expect("成功日结");
    let publication = world.publication();
    let published = publication.load();
    let (dead, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let real = world.swap_group_sender_for_test(1, dead);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.run_ticks(1)));
    world.swap_group_sender_for_test(1, real);
    let payload = outcome.expect_err("排队 TaskDied 不能被 Start 发送失败吞掉");
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("partial-start-original-panic")
    );
    assert_eq!(
        world.live_workers_for_test(),
        0,
        "恢复原 panic 前必须 join 全部真实线程"
    );
    world.abort_day_end();
    assert!(Arc::ptr_eq(&published, &publication.load()));
    let report = world.shutdown().expect("panic 已运输，Final 已收齐");
    assert_eq!(report.worker_results.len(), 4);
}

#[test]
fn partial_start_cleanup_reports_unknown_messages_after_joining_every_worker() {
    let mut world = world_after_successful_day_end();
    world.set_start_cleanup_message_for_test(super::protocol::UpMsg::DayEndAborted {
        who: super::protocol::WorkerId::Desk(0),
        tick: 240,
    });
    let (dead, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let real = world.swap_desk_sender_for_test(1, dead);
    let error = world
        .run_ticks(1)
        .expect_err("真实发送失败并伴随异常上行消息");
    world.swap_desk_sender_for_test(1, real);
    assert!(
        matches!(error, super::protocol::RuntimeError::ProtocolInvariant { detail } if detail.contains("Start") && detail.contains("DayEndAborted")),
        "未知清理消息必须带原 Start 失败显式报告"
    );
    assert_eq!(
        world.live_workers_for_test(),
        0,
        "清理协议错误也必须 join 全部真实线程"
    );
    world.abort_day_end();
    let report = world.shutdown().expect("可取得已收齐的诊断 Final");
    assert_eq!(report.worker_results.len(), 4);
}

fn assert_partial_day_end_preserves_published_tick(failure: u8) {
    let mut world = World::spawn(partial_drain_spec()).expect("构造两股真实日结世界");
    world.run_ticks(2).expect("tick240 已完成 Seal 与发布");
    let before = world.snapshot_accounts().expect("发布后组账户");
    assert!(before
        .iter()
        .all(|(_, accounts)| accounts[0].cash < 1_000_000));
    assert!(before
        .iter()
        .all(|(_, accounts)| accounts[0].foreign_active[1] > 0));
    assert!(
        before
            .iter()
            .all(|(_, accounts)| accounts[1].cash < 1_000_000),
        "最后一拍真实扣出资金，不能以空 undo 规避 P1"
    );
    let prior_ticks = world.group_ticks_for_test();
    assert!(prior_ticks.iter().all(|&(_, tick, _)| tick == 240));
    eprintln!("P1 before failure={failure}: group cursors={prior_ticks:?}");
    let before_orders = world.collect_desk_resting().expect("日结前真实挂单");
    let before_candles = world.take_candle_projection().expect("日结前行情");
    let before_views = world.public_views().to_vec();
    let publication = world.publication();
    let published = publication.load();
    assert_eq!(published.tick, 240);
    let error = match failure {
        0 => {
            let (dead, receiver) = std::sync::mpsc::channel();
            drop(receiver);
            let original = world.swap_desk_sender_for_test(1, dead);
            let result = world.run_day_end_public();
            world.swap_desk_sender_for_test(1, original);
            result.expect_err("第二股真实 Drain Sender 发送失败")
        }
        1 => {
            world.fail_day_end_after_drain_reports_for_test(1);
            world
                .run_day_end_public()
                .expect_err("收到首个真实 Drain 结果后失败")
        }
        2 => {
            world.fail_day_end_before_group_for_test(1);
            world
                .run_day_end_public()
                .expect_err("已发第一组 DayEnd 后发送失败")
        }
        _ => unreachable!(),
    };
    eprintln!("P1 failure={failure}: {error:?}");
    if failure == 1 {
        assert!(matches!(&error, super::protocol::RuntimeError::ProtocolInvariant { detail }
            if detail.contains("收到 1 个真实 DeskDrained 后收集失败")));
    } else {
        assert!(matches!(error, super::protocol::RuntimeError::SendFailed { .. }));
    }
    let ticks = world.group_ticks_for_test();
    assert_eq!(
        ticks
            .iter()
            .map(|&(group, tick, _)| (group, tick))
            .collect::<Vec<_>>(),
        vec![(0, 240), (1, 240)],
        "失败不得倒放已发布的 tick240：{ticks:?}"
    );
    assert_eq!(world.global.committed_tick, 240);
    assert!(
        Arc::ptr_eq(&published, &publication.load()),
        "失败不改已发布句柄"
    );
    assert_eq!(world.public_views(), before_views);
    assert_eq!(
        world.snapshot_accounts().expect("失败后真实组账户"),
        before,
        "普通 tick 资金不能倒放，Drain 即时成交/退款不能残留"
    );
    assert_eq!(
        world.collect_desk_resting().expect("失败后所有交易部"),
        before_orders,
        "未 Drain 的交易部不能撤销普通 tick；已 Drain 的交易部须完整恢复"
    );
    assert_eq!(
        world.take_candle_projection().expect("失败后行情"),
        before_candles
    );
    assert!(world.last_day_end_candidate.is_none());
    assert!(world.drained_day_trades.is_empty());
    assert!(world.drained_lifecycle.is_empty());
    if failure != 1 {
        assert!(world.run_ticks(1).is_err(), "断连失败后不得假称可继续交易");
        assert!(
            world.run_day_end_public().is_err(),
            "断连失败后不得重跑日结"
        );
        world.shutdown().expect("仅恢复测试 Sender 以完成资源回收");
        return;
    }
    world
        .run_day_end_public()
        .expect("清理完失败载荷后重试日结");
    assert_eq!(
        world
            .drained_day_trades
            .iter()
            .map(|trade| u64::from(trade.qty))
            .sum::<u64>(),
        200,
        "真实收盘成交只交付一次"
    );
    world.run_ticks(1).expect("下一真实 Start241 正常执行");
    world.shutdown().expect("失败重试后正常关停");
}

#[test]
fn failing_partial_drain_send_preserves_the_published_tick() {
    assert_partial_day_end_preserves_published_tick(0);
}

#[test]
fn failing_drain_collection_reverts_only_day_end_entities() {
    assert_partial_day_end_preserves_published_tick(1);
}

#[test]
fn failing_partial_group_day_end_send_preserves_unprepared_groups() {
    assert_partial_day_end_preserves_published_tick(2);
}

/// 只跑日终前两拍与次日首拍，不以长时间模拟代替最小复现。
fn closing_spec(closing_price: i64) -> WorldSpec {
    let mut spec = basic_spec(
        2,
        1,
        2,
        Box::new(move |_group| -> GroupScript {
            Box::new(move |tick, _round| {
                let mut plan = TickPlan::default();
                if tick == DAY_END_TICK - 1 {
                    for (local, side) in [(0, Side::Buy), (1, Side::Sell)] {
                        plan.places.push(PlaceSpec {
                            local,
                            stock: 0,
                            side,
                            price: closing_price,
                            qty: LOT,
                        });
                    }
                }
                plan
            })
        }),
    );
    spec.ticks_per_day = DAY_END_TICK;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    spec.initial_committed_tick = DAY_END_TICK - 2;
    // 关闭价格笼子是已有的显式游戏简化；Highest 受理价因此等于次日涨停价。
    let mut config = GameConfig::proposed_defaults();
    config.price_cage_enabled = false;
    spec.config = Arc::new(config);
    spec
}

#[test]
fn closing_rise_next_day_highest_buy_reserves_the_new_daily_limit() {
    let spec = closing_spec(CLOSING_PRICE);
    let config = Arc::clone(&spec.config);
    let civil_date = spec.civil_start_date;
    let mut world = World::spawn(spec).expect("构造两组收盘竞价世界");
    world.run_ticks(2).expect("收盘竞价申报两拍收束");
    assert_eq!(
        world.public_views()[0].last_price,
        INITIAL_PRICE,
        "竞价尚未清算"
    );
    world.run_day_end_public().expect("日结执行收盘竞价清算");
    let candidate = world.last_day_end_candidate.as_ref().expect("日结候选");
    assert_eq!(
        candidate.desks[0].2, CLOSING_PRICE,
        "真实收盘竞价成交抬升昨收"
    );
    let cash_before = candidate
        .groups
        .iter()
        .find(|(g, _)| *g == 0)
        .expect("组0候选")
        .1[0]
        .cash;
    let snapshot = TickSnapshot {
        tick: DAY_END_TICK + 1,
        market_minute: 0,
        civil_date,
        stocks: Arc::new(world.public_views().to_vec()),
    };
    let op = intent_to_pending(
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: LimitPrice::Highest,
            qty: LOT,
        },
        &build_market_view(&snapshot),
    )
    .expect("正式 NPC 意图转换路径");
    let expected_purse = buy_purse_limit(&config, Money::from_cents(1_210), LOT, Money::ZERO)
        .expect("次日涨停含费预留")
        .cents();
    eprintln!("#121 tick241: close={CLOSING_PRICE}, broadcast_upper={}, broadcast_lower={}, purse_reference={}, expected_upper=1210, expected_lower=990, expected_purse={expected_purse}",
        snapshot.stocks[0].daily_upper_limit, snapshot.stocks[0].daily_lower_limit, op.purse_reference);
    world.broadcast_external_plan(vec![op], vec![]);
    world
        .run_ticks(1)
        .expect("次日首拍符号买单受理，不得因旧涨停预留不足失败");
    assert_eq!(op.purse_reference, 1_210, "预留基准为新昨收涨停价");
    assert_eq!(
        snapshot.stocks[0].daily_lower_limit, 990,
        "次日跌停价同步更新"
    );
    assert_eq!(
        world.public_views()[0].best_bid,
        Some(1_210),
        "首拍按新涨停价成功挂单"
    );
    let accounts = world.snapshot_accounts().expect("首拍账户快照");
    let buyer = &accounts.iter().find(|(g, _)| *g == 0).expect("组0账户").1[0];
    assert_eq!(
        cash_before - buyer.cash,
        expected_purse,
        "实际扣出 purse 必须逐分吻合新上限"
    );
    assert_eq!(buyer.foreign_active[0], 1, "首拍存在已受理买单");
    world.shutdown().expect("清簿并回收线程");
}

#[test]
fn failed_day_end_restores_desk_and_broadcast_before_retry() {
    let mut world = World::spawn(closing_spec(CLOSING_PRICE)).expect("构造世界");
    world.run_ticks(2).expect("日终两拍收束");
    let prior_views = world.public_views().to_vec();
    let prior_candles = world.take_candle_projection().expect("日结前 K 线");
    let prior_orders = world.collect_desk_resting().expect("日结前挂单");
    let prior_accounts = world.snapshot_accounts().expect("日结前账户");
    let publication = world.publication();
    let prior_publication = publication.load();
    world.set_day_end_validator(Box::new(|_| Err("注入日结候选校验失败".to_owned())));
    let error = world.run_day_end_public().expect_err("失败必须显式传播");
    assert!(matches!(
        error,
        super::protocol::RuntimeError::DayEndValidation { day: 0, .. }
    ));
    assert_eq!(world.public_views(), prior_views, "失败不得半安装广播基");
    assert!(
        Arc::ptr_eq(&publication.load(), &prior_publication),
        "失败不发布混合交易日版本"
    );
    assert!(world.last_day_end_candidate.is_none(), "失败不保留候选");
    assert_eq!(
        world.collect_desk_resting().expect("回滚后挂单"),
        prior_orders,
        "收盘成交与清余单全部冲回"
    );
    assert_eq!(
        world.snapshot_accounts().expect("回滚后账户"),
        prior_accounts,
        "日结资源落账全部冲回"
    );
    let restored_candles = world.take_candle_projection().expect("失败后 K 线");
    eprintln!("#121 failed DayEnd rollback: prior_candles={prior_candles:?}, restored_candles={restored_candles:?}");
    assert_eq!(
        restored_candles, prior_candles,
        "Drain 的 reroll 不得泄漏终 K 或次日占位 K"
    );
    world.set_day_end_validator(Box::new(|_| Ok(())));
    world.run_day_end_public().expect("失败后可重跑同一日结");
    assert_eq!(world.public_views()[0].daily_upper_limit, 1_210);
    assert_eq!(world.public_views()[0].daily_lower_limit, 990);
    assert_eq!(
        world
            .drained_day_trades
            .iter()
            .map(|f| u64::from(f.qty))
            .sum::<u64>(),
        200,
        "重试只交付一次竞价成交"
    );
    assert!(world.take_pending_day_candles()[0].is_some());
    world.run_ticks(1).expect("重试成功后的真实 Start241");
    let report = world.shutdown().expect("关停");
    assert_eq!(
        report.desk_finals[0].market.last_close, CLOSING_PRICE,
        "次日昨收与广播基一致"
    );
    assert_eq!(
        report.desk_finals[0].market.candles[0].volume, 200,
        "收盘量不因重试加倍"
    );
    assert_eq!(
        report.desk_finals[0].market.traded_days_total, 1,
        "有效成交日只计一次"
    );
}

#[test]
fn explicit_abort_day_end_restores_the_real_prior_then_retries() {
    let mut world = World::spawn(closing_spec(CLOSING_PRICE)).expect("构造世界");
    world.run_ticks(2).expect("日终两拍收束");
    let prior_views = world.public_views().to_vec();
    let prior_candles = world.take_candle_projection().expect("日结前 K 线");
    let prior_orders = world.collect_desk_resting().expect("日结前挂单");
    let prior_accounts = world.snapshot_accounts().expect("日结前账户");
    let publication = world.publication();
    let prior_publication = publication.load();
    world
        .run_day_end_public()
        .expect("成功日结后仍在 Abort 窗口");
    assert!(
        Arc::ptr_eq(&publication.load(), &prior_publication),
        "日结成功也不单独发布混合账户/行情版本"
    );
    world.abort_day_end();
    assert!(
        Arc::ptr_eq(&publication.load(), &prior_publication),
        "Abort 不替换已发布内容或回退 generation"
    );
    assert_eq!(
        world.public_views(),
        prior_views,
        "显式 Abort 恢复真实 prior，而非已安装新基准"
    );
    assert!(world.last_day_end_candidate.is_none());
    assert!(world.drained_day_trades.is_empty());
    assert!(world.drained_auction.is_empty());
    assert!(world.drained_lifecycle.is_empty());
    assert!(world.take_pending_day_candles().iter().all(Option::is_none));
    assert!(world.take_pending_day_bars().iter().all(Option::is_none));
    assert_eq!(
        world.collect_desk_resting().expect("Abort 后挂单"),
        prior_orders
    );
    assert_eq!(
        world.snapshot_accounts().expect("Abort 后账户"),
        prior_accounts
    );
    let restored_candles = world.take_candle_projection().expect("Abort 后 K 线");
    eprintln!("#121 explicit Abort rollback: prior_candles={prior_candles:?}, restored_candles={restored_candles:?}");
    assert_eq!(
        restored_candles, prior_candles,
        "显式 Abort 必须恢复交易部行情，而非只恢复 World 向量"
    );
    world.run_day_end_public().expect("显式 Abort 后重跑日结");
    assert_eq!(world.public_views()[0].daily_upper_limit, 1_210);
    world.run_ticks(1).expect("显式重试后的真实 Start241");
    let report = world.shutdown().expect("关停");
    assert_eq!(report.desk_finals[0].market.last_close, CLOSING_PRICE);
    assert_eq!(report.desk_finals[0].market.traded_days_total, 1);
}

#[test]
fn manual_drain_before_roll_prices_the_next_real_start_not_the_current_day() {
    let spec = closing_spec(CLOSING_PRICE);
    let civil_date = spec.civil_start_date;
    let mut world = World::spawn(spec).expect("构造世界");
    world.run_ticks(1).expect("tick239 收盘申报");
    world.run_day_end_public().expect("未 roll 前的手动日结");
    let snapshot = TickSnapshot {
        tick: DAY_END_TICK,
        market_minute: 0,
        civil_date,
        stocks: Arc::new(world.public_views().to_vec()),
    };
    let op = intent_to_pending(
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: LimitPrice::Highest,
            qty: LOT,
        },
        &build_market_view(&snapshot),
    )
    .expect("Highest 意图转换");
    eprintln!("#121 manual Drain tick239→Start240: broadcast_upper={}, purse_reference={}, expected_upper=1210", snapshot.stocks[0].daily_upper_limit, op.purse_reference);
    world.broadcast_external_plan(vec![op], vec![]);
    world
        .run_ticks(1)
        .expect("Start240 日界重置后也必须付得起已生成的符号买单");
    assert_eq!(op.purse_reference, 1_210, "预测下一次真实 Start 的新涨停");
    let orders = world.collect_desk_resting().expect("真实受理挂单").0;
    let order = orders[0]
        .iter()
        .find(|o| o.owner == (0, 0))
        .expect("手动日结后买单入簿");
    assert_eq!((order.price, order.purse), (1_210, 121_501));
    world.shutdown().expect("关停");
}

#[test]
fn automatic_day_end_broadcast_reaches_real_npcs_in_every_group() {
    use super::decision::{
        decide_account, evaluate_attention, DecisionSetup, NpcProfile, PendingOp,
    };
    use super::group::GroupAccount;
    use crate::strategy::StrategyData;
    // chase_prob=0：方向只由独立 RNG 决定，不依赖历史长度或竞价到达顺序。
    let mut spec = closing_spec(CLOSING_PRICE);
    let template = World::spawn(closing_spec(CLOSING_PRICE)).expect("行情模板");
    let snapshot = TickSnapshot {
        tick: DAY_END_TICK + 1,
        market_minute: 0,
        civil_date: spec.civil_start_date,
        stocks: Arc::new(template.public_views().to_vec()),
    };
    let market = build_market_view(&snapshot);
    drop(template);
    let mut profiles = Vec::new();
    for group in 0..2u64 {
        let seed = (0..200u64)
            .find(|seed| {
                let mut p = NpcProfile {
                    strategy: StrategyData::retail(1.0, LOT, 0.0),
                    seed: seed ^ group.wrapping_mul(0x9E3779B97F4A7C15),
                    next_due: DAY_END_TICK + 1,
                    attention_base: 1.0,
                    retail_style: None,
                };
                evaluate_attention(
                    &mut p,
                    crate::session::market_attention_signal(&market),
                    DAY_END_TICK + 1,
                );
                let (ops, _) = decide_account(
                    &mut p,
                    group.wrapping_mul(0x9E3779B9),
                    DAY_END_TICK + 1,
                    0,
                    &market,
                    &GroupAccount::new(1_000_000, 1),
                    &spec.config,
                );
                ops.iter().any(|op| op.side == Side::Buy && op.qty == LOT)
            })
            .expect("200 个种子内必须找到确定买入种子");
        profiles.push(NpcProfile {
            strategy: StrategyData::retail(1.0, LOT, 0.0),
            seed,
            next_due: DAY_END_TICK + 1,
            attention_base: 1.0,
            retail_style: None,
        });
        profiles.push(NpcProfile {
            strategy: StrategyData::retail(0.0, LOT, 0.0),
            seed: 0,
            next_due: u64::MAX,
            attention_base: 1.0,
            retail_style: None,
        });
    }
    spec.decision = Some(DecisionSetup {
        profiles,
        attention_gap_ticks: 0,
    });
    let mut world = World::spawn(spec).expect("两组真实 NPC 世界");
    world.send_external_plan_grouped(
        (0..2)
            .map(|_| {
                vec![
                    PendingOp {
                        local: 0,
                        stock: 0,
                        side: Side::Buy,
                        routed: super::RoutedPrice::Fixed(CLOSING_PRICE),
                        purse_reference: CLOSING_PRICE,
                        qty: LOT,
                    },
                    PendingOp {
                        local: 1,
                        stock: 0,
                        side: Side::Sell,
                        routed: super::RoutedPrice::Fixed(CLOSING_PRICE),
                        purse_reference: CLOSING_PRICE,
                        qty: LOT,
                    },
                ]
            })
            .collect(),
        vec![],
    );
    world.run_ticks(2).expect("tick239/240 竞价申报收束");
    world
        .run_ticks(1)
        .expect("自动日结后 Start241 两组真实 NPC 买单受理");
    let orders = world.collect_desk_resting().expect("真实组端下单结果").0;
    for group in 0..2 {
        let order = orders[0]
            .iter()
            .find(|o| o.owner == (group, 0))
            .expect("每组各有一笔真实决策买单");
        assert_eq!(
            (order.price, order.purse),
            (1_210, 121_501),
            "各组实际广播/预留/受理同一新基准"
        );
    }
    let report = world.shutdown().expect("关停");
    for group in report.group_finals {
        assert!(group.observed.contains(&super::group::Observed::Start(241)));
        assert_eq!(group.decide_calls_total, 1, "每组仅首拍到期并决策一次");
    }
}

#[test]
fn two_groups_two_stocks_keep_distinct_up_and_down_closing_bases() {
    use super::decision::PendingOp;
    let mut spec = closing_spec(CLOSING_PRICE);
    spec.stocks
        .push(super::world::DeskStockSpec::main_board(INITIAL_PRICE));
    for accounts in &mut spec.initial_shares {
        for shares in accounts {
            shares.push(1_000);
        }
    }
    spec.script_for = Box::new(|_| {
        Box::new(|tick, _| {
            let mut plan = TickPlan::default();
            if tick == DAY_END_TICK - 1 {
                for (stock, price) in [(0, 1_100), (1, 900)] {
                    for (local, side) in [(0, Side::Buy), (1, Side::Sell)] {
                        plan.places.push(PlaceSpec {
                            local,
                            stock,
                            side,
                            price,
                            qty: LOT,
                        });
                    }
                }
            }
            plan
        })
    });
    let mut world = World::spawn(spec).expect("两组两股世界");
    world.run_ticks(2).expect("两股收盘竞价申报");
    world.run_day_end_public().expect("两股一次日结");
    let views = world.public_views().to_vec();
    assert_eq!(
        (
            views[0].last_price,
            views[0].daily_upper_limit,
            views[0].daily_lower_limit
        ),
        (1_100, 1_210, 990)
    );
    assert_eq!(
        (
            views[1].last_price,
            views[1].daily_upper_limit,
            views[1].daily_lower_limit
        ),
        (900, 990, 810)
    );
    let by_group = (0..2)
        .map(|_| {
            views
                .iter()
                .enumerate()
                .map(|(stock, view)| PendingOp {
                    local: 0,
                    stock: stock as u16,
                    side: Side::Buy,
                    routed: super::RoutedPrice::Highest,
                    purse_reference: view.daily_upper_limit,
                    qty: LOT,
                })
                .collect()
        })
        .collect();
    world.send_external_plan_grouped(by_group, vec![]);
    world
        .run_ticks(1)
        .expect("Start241 两组分别按两股新上限预留并受理");
    let orders = world.collect_desk_resting().expect("两股实际受理").0;
    for (stock, price, purse) in [(0usize, 1_210, 121_501), (1, 990, 99_501)] {
        assert_eq!(orders[stock].len(), 2, "每股每组恰有一笔受理买单");
        for group in 0..2 {
            let order = orders[stock]
                .iter()
                .find(|o| o.owner == (group, 0))
                .expect("按真实组身份查单");
            assert_eq!((order.price, order.purse), (price, purse));
        }
    }
    world.shutdown().expect("关停");
}

#[test]
fn closing_moves_refresh_next_day_limits_and_buy_cage_in_both_directions() {
    use super::decision::PendingOp;
    for (close, upper, lower, cage) in [(1_100, 1_210, 990, 1_122), (900, 990, 810, 918)] {
        let mut spec = closing_spec(close);
        Arc::make_mut(&mut spec.config).price_cage_enabled = true;
        let mut world = World::spawn(spec).expect("开启笼子的收盘竞价世界");
        world.run_ticks(2).expect("收盘申报收束");
        world
            .run_day_end_public()
            .expect("收盘清算并构造次日观察基");
        let view = &world.public_views()[0];
        assert_eq!(
            (
                view.daily_upper_limit,
                view.daily_lower_limit,
                view.max_buy_price
            ),
            (upper, lower, cage),
            "涨跌双向的次日 band 与买入笼子交集"
        );
        world.broadcast_external_plan(
            vec![PendingOp {
                local: 0,
                stock: 0,
                side: Side::Buy,
                routed: super::RoutedPrice::Highest,
                purse_reference: view.daily_upper_limit,
                qty: LOT,
            }],
            vec![],
        );
        world
            .run_ticks(1)
            .expect("首拍真实受理仍使用既有笼子阶段口径");
        let orders = world.collect_desk_resting().expect("受理价与精确 purse").0;
        let order = &orders[0][0];
        let expected_purse = buy_purse_limit(
            &GameConfig::proposed_defaults(),
            Money::from_cents(cage),
            LOT,
            Money::ZERO,
        )
        .expect("实际笼子价含费")
        .cents();
        assert_eq!((order.price, order.purse), (cage, expected_purse));
        world.shutdown().expect("关停");
    }
}

/// 真实机构策略会读取 min_sell_price；只在指定 Start 首次到期卖出。
fn sell_decision_setup(next_tick: u64) -> super::decision::DecisionSetup {
    use super::decision::NpcProfile;
    use crate::strategy::{StrategyData, TargetPolicy};
    let profiles = (0..2)
        .flat_map(|_| {
            [
                NpcProfile {
                    strategy: StrategyData::retail(0.0, LOT, 0.0),
                    seed: 0,
                    next_due: u64::MAX,
                    attention_base: 1.0,
                    retail_style: None,
                },
                NpcProfile {
                    strategy: StrategyData::inst(
                        TargetPolicy::Fixed(Money::from_cents(1)),
                        0.0,
                        LOT,
                    ),
                    seed: 0,
                    next_due: next_tick,
                    attention_base: 1.0,
                    retail_style: None,
                },
            ]
        })
        .collect();
    super::decision::DecisionSetup {
        profiles,
        attention_gap_ticks: 0,
    }
}

/// 对照从合法活动 K 恢复；跨日首拍与关停都走真实 desk 协议。
fn same_new_day_sell_control_spec(close: i64, final_tick: u64) -> WorldSpec {
    let mut spec = closing_spec(close);
    spec.stocks[0] = super::world::DeskStockSpec::main_board(close);
    spec.stocks[0].initial_market = Some(super::desk::DeskMarketSeed {
        last_price: close,
        last_close: close,
        day_high: close,
        day_low: close,
        volume_today: 0,
        turnover_today: 0,
        trade_count: 0,
        candle: Some((close, close, close, close, 0)),
        minute_closes: Vec::new(),
        minute_bars: Vec::new(),
    });
    spec.initial_committed_tick = final_tick;
    spec.initial_last_day_end_tick = Some(final_tick);
    Arc::make_mut(&mut spec.config).price_cage_enabled = true;
    spec
}

#[test]
fn next_start_real_sell_consumers_match_the_same_new_day_control() {
    use super::decision::PendingOp;
    for close in [1_100, 900] {
        for (final_tick, manual) in [(239, true), (240, true), (240, false)] {
            let next_tick = final_tick + 1;
            let mut spec = closing_spec(close);
            Arc::make_mut(&mut spec.config).price_cage_enabled = true;
            spec.decision = Some(sell_decision_setup(next_tick));
            let mut crossed = World::spawn(spec).expect("跨日卖向世界");
            crossed.send_external_plan_grouped(
                (0..2)
                    .map(|_| {
                        vec![
                            PendingOp {
                                local: 0,
                                stock: 0,
                                side: Side::Buy,
                                routed: super::RoutedPrice::Fixed(close),
                                purse_reference: close,
                                qty: LOT,
                            },
                            PendingOp {
                                local: 1,
                                stock: 0,
                                side: Side::Sell,
                                routed: super::RoutedPrice::Fixed(close),
                                purse_reference: close,
                                qty: LOT,
                            },
                        ]
                    })
                    .collect(),
                vec![],
            );
            crossed
                .run_ticks(final_tick - (DAY_END_TICK - 2))
                .expect("收盘竞价申报");
            if manual {
                crossed.run_day_end_public().expect("手动日结");
            }
            crossed
                .run_ticks(1)
                .expect("真实 Start 使用新基准生成并受理机构卖单");
            let mut control_spec = same_new_day_sell_control_spec(close, final_tick);
            control_spec.decision = Some(sell_decision_setup(next_tick));
            let mut control = World::spawn(control_spec).expect("同新日昨收稳态对照");
            control.run_ticks(1).expect("对照 Start 真实机构卖向消费");
            let lower = close * 9 / 10;
            let crossed_view = &crossed.public_views()[0];
            let control_view = &control.public_views()[0];
            assert_eq!(
                (
                    crossed_view.last_price,
                    crossed_view.daily_lower_limit,
                    crossed_view.min_sell_price
                ),
                (close, lower, lower),
                "按真实新日基准消费，不残留前日 lower/min_sell"
            );
            assert_eq!(
                (crossed_view.daily_lower_limit, crossed_view.min_sell_price),
                (control_view.daily_lower_limit, control_view.min_sell_price)
            );
            let crossed_orders = crossed.collect_desk_resting().expect("跨日实际卖单").0;
            let control_orders = control.collect_desk_resting().expect("对照实际卖单").0;
            for group in 0..2 {
                let actual = crossed_orders[0]
                    .iter()
                    .find(|o| o.owner == (group, 1))
                    .expect("每组真实机构卖单成功受理");
                let expected = control_orders[0]
                    .iter()
                    .find(|o| o.owner == (group, 1))
                    .expect("对照机构卖单");
                assert_eq!(
                    (actual.side, actual.price, actual.qty),
                    (Side::Sell, close, LOT)
                );
                assert_eq!(
                    (actual.side, actual.price, actual.qty),
                    (expected.side, expected.price, expected.qty),
                    "日界预测不新增卖向价格或时点失配"
                );
            }
            assert!(crossed
                .report_log
                .last()
                .expect("首拍记录")
                .desk_receipts
                .is_empty());
            eprintln!("#121 sell Start{next_tick}, manual={manual}, close={close}: public_lower={lower}, public_min_sell={lower}, actual_inst_sell={close}, control_inst_sell={close}");
            crossed.shutdown().expect("跨日世界关停");
            control.shutdown().expect("对照世界关停");
        }
    }
}

#[test]
fn next_start_sell_boundary_records_issue_125_without_a_new_cross_day_gap() {
    use super::decision::PendingOp;
    for (close, lower, actual_floor) in [(1_100, 990, 1_078), (900, 810, 882)] {
        for final_tick in [239, 240] {
            let mut spec = closing_spec(close);
            Arc::make_mut(&mut spec.config).price_cage_enabled = true;
            let mut crossed = World::spawn(spec).expect("开启笼子的日界世界");
            crossed
                .run_ticks(final_tick - (DAY_END_TICK - 2))
                .expect("收盘申报");
            crossed.run_day_end_public().expect("手动日结");
            let view = &crossed.public_views()[0];
            assert_eq!(
                (view.daily_lower_limit, view.min_sell_price),
                (lower, lower)
            );
            // #125 的现状证据：公开 min_sell 仍不是卖笼子下限，不将合法拒单称为规则 bug。
            // #125 修复后应更新此证据；本测试不宣称卖向整体语义验收通过。
            let public_floor = view.min_sell_price;
            let mut control = World::spawn(same_new_day_sell_control_spec(close, final_tick))
                .expect("同基准、同阶段对照");
            for world in [&mut crossed, &mut control] {
                let prices = [public_floor, actual_floor - 1, actual_floor];
                let ops = prices
                    .into_iter()
                    .map(|price| PendingOp {
                        local: 1,
                        stock: 0,
                        side: Side::Sell,
                        routed: super::RoutedPrice::Fixed(price),
                        purse_reference: price,
                        qty: LOT,
                    })
                    .collect();
                world.broadcast_external_plan(ops, vec![]);
                world
                    .run_ticks(1)
                    .expect("真实 Start 消费公开下限及边界相邻卖单");
                let record = world.report_log.last().expect("首拍受理记录");
                assert_eq!(
                    record.desk_receipts.len(),
                    2,
                    "公开下限和笼子外一分均显式拒绝"
                );
                assert!(record
                    .desk_receipts
                    .iter()
                    .all(|r| r.kind == super::ReceiptKind::PriceCageExceeded));
                assert!(
                    record.group_rejections.is_empty(),
                    "不是资源不足掩盖价格拒单"
                );
                let orders = world.collect_desk_resting().expect("真实卖向入簿边界").0;
                assert_eq!(orders[0].len(), 1, "仅真实边界价受理");
                assert_eq!(
                    (orders[0][0].side, orders[0][0].price, orders[0][0].qty),
                    (Side::Sell, actual_floor, LOT)
                );
                let accounts = world.snapshot_accounts().expect("拒单资源退回后账户");
                let seller = &accounts.iter().find(|(g, _)| *g == 0).expect("组0").1[1];
                assert_eq!(seller.pending_sell_qty[0], LOT, "两笔拒单不泄漏股份占用");
            }
            eprintln!("#121/#125 sell Start{}: close={close}, public_lower={lower}, public_min_sell={public_floor}, actual_sell_floor={actual_floor}, floor_minus_one=rejected, floor=accepted; same as control", final_tick + 1);
            crossed.shutdown().expect("跨日关停");
            control.shutdown().expect("对照关停");
        }
    }
}

#[test]
fn successful_next_start_closes_the_day_end_abort_window() {
    let mut world = World::spawn(closing_spec(CLOSING_PRICE)).expect("构造世界");
    world.run_ticks(2).expect("日终两拍");
    let publication = world.publication();
    let prior = publication.load();
    world.run_day_end_public().expect("日结成功");
    assert!(Arc::ptr_eq(&publication.load(), &prior));
    world.run_ticks(1).expect("下一真实 Start 转正日结");
    let published = publication.load();
    assert_eq!(published.generation, prior.generation + 1);
    let views = world.public_views().to_vec();
    let candles = world.take_candle_projection().expect("转正后 K");
    let accounts = world.snapshot_accounts().expect("转正后账户");
    world.abort_day_end();
    assert!(
        Arc::ptr_eq(&publication.load(), &published),
        "过期日结 Abort 不改发布内容或 generation"
    );
    assert_eq!(world.public_views(), views);
    assert_eq!(
        world.take_candle_projection().expect("过期 Abort 后 K"),
        candles
    );
    assert_eq!(
        world.snapshot_accounts().expect("过期 Abort 后账户"),
        accounts
    );
    assert!(world.last_day_end_candidate.is_some(), "已转正日结不得作废");
    world.shutdown().expect("关停");
}

#[test]
fn failing_first_tick_rolls_back_only_the_tick_not_the_committed_day_end() {
    let mut world = World::spawn(closing_spec(CLOSING_PRICE)).expect("构造世界");
    world.run_ticks(2).expect("日终两拍");
    world.run_day_end_public().expect("日结成功等待 Start");
    let views = world.public_views().to_vec();
    let candles = world.take_candle_projection().expect("日结后 K");
    let accounts = world.snapshot_accounts().expect("日结后账户");
    let publication = world.publication();
    let prior = publication.load();
    world.set_validator(Box::new(|_| Err("注入次日首拍失败".to_owned())));
    let error = world.run_ticks(1).expect_err("Start 后首拍校验失败");
    assert!(matches!(
        error,
        super::protocol::RuntimeError::TickValidation { tick: 241, .. }
    ));
    assert!(Arc::ptr_eq(&publication.load(), &prior), "失败首拍不发布");
    world.abort_day_end();
    assert!(
        world.last_day_end_candidate.is_some(),
        "Start 已关闭日结 Abort 窗口"
    );
    assert_eq!(
        world.public_views(),
        views,
        "首拍失败也保留已转正日结广播基"
    );
    assert_eq!(
        world.take_candle_projection().expect("首拍回滚后 K"),
        candles
    );
    assert_eq!(world.snapshot_accounts().expect("首拍回滚后账户"), accounts);
    world.set_validator(Box::new(|_| Ok(())));
    world.run_ticks(1).expect("重跑首拍而不是重复日结");
    assert_eq!(publication.load().generation, prior.generation + 1);
    assert_eq!(
        world
            .drained_day_trades
            .iter()
            .map(|f| u64::from(f.qty))
            .sum::<u64>(),
        200
    );
    let report = world.shutdown().expect("关停");
    for group in report.group_finals {
        assert_eq!(
            group
                .observed
                .iter()
                .filter(|o| matches!(o, super::group::Observed::DayEnd(0)))
                .count(),
            1,
            "首拍重试不双做日结"
        );
    }
}
