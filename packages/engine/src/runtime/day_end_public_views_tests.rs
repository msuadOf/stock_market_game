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

/// 直接观察 spawn 生成的 initial_views；不运行 tick，避免被 DeskReport 覆盖。
fn assert_initial_sell_floor(first_tick: u64, cage_on: bool, floor: i64) {
    let mut spec = basic_spec(2, 1, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = DAY_END_TICK;
    spec.auction_ticks = 9;
    spec.closing_auction_ticks = 3;
    spec.initial_committed_tick = first_tick - 1;
    spec.stocks[0] = super::world::DeskStockSpec::main_board(CLOSING_PRICE);
    let mut config = GameConfig::proposed_defaults();
    config.price_cage_enabled = cage_on;
    spec.config = Arc::new(config);
    let world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造首拍卖向行情世界");
    assert_eq!(world.global.committed_tick, first_tick - 1);
    assert!(
        world.report_log.is_empty(),
        "尚未运行任何 tick，必须直测 initial_views"
    );
    let view = &world.public_views()[0];
    assert_eq!(
        (view.last_price, view.daily_lower_limit, view.min_sell_price),
        (CLOSING_PRICE, 990, floor)
    );
    let published = world.publication().load();
    assert_eq!(published.tick, first_tick - 1);
    assert_eq!(*published.desks[0].view, *view, "初始发布与公共观察基一致");
    world.shutdown().expect("首拍直测正常关停并回收线程");
}

#[test]
fn initial_views_continuous_with_cage_uses_sell_cage_floor() {
    assert_initial_sell_floor(9, true, 1078);
}

#[test]
fn initial_views_call_auction_uses_daily_lower_limit() {
    assert_initial_sell_floor(1, true, 990);
}

#[test]
fn initial_views_pre_open_uses_daily_lower_limit() {
    assert_initial_sell_floor(6, true, 990);
}

#[test]
fn initial_views_closing_auction_uses_daily_lower_limit() {
    assert_initial_sell_floor(237, true, 990);
}

#[test]
fn initial_views_continuous_without_cage_uses_daily_lower_limit() {
    assert_initial_sell_floor(9, false, 990);
}

fn sell_cage_floor(last: i64, daily_lower: i64) -> i64 {
    let pct = super::desk::bound_price(Money::from_cents(last), 9_800, false)
        .expect("卖向笼子下界计算")
        .cents();
    pct.min(last - 10).max(1).max(daily_lower)
}

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
    let spec = partial_drain_spec();
    let calendar = crate::calendar::TradingCalendar::current_default_calendar()
        .expect("显式 raw Calendar 政策合法");
    let next_date = calendar
        .next_trading_day(
            crate::calendar::CalendarExchange::Sse,
            spec.civil_start_date,
        )
        .expect("fixture 下一 SSE trading date 合法");
    let mut world = World::spawn(spec, calendar).expect("构造两股两组日结世界");
    world.run_ticks(2).expect("tick240 成功发布");
    world
        .run_day_end_public()
        .expect("日结成功且撤销窗口仍开启");
    world.set_civil_date(next_date);
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
    let calendar = crate::calendar::TradingCalendar::current_default_calendar()
        .expect("显式 raw Calendar 政策合法");
    let next_date = calendar
        .next_trading_day(
            crate::calendar::CalendarExchange::Sse,
            spec.civil_start_date,
        )
        .expect("fixture 下一 SSE trading date 合法");
    let mut world = World::spawn(spec, calendar).expect("构造真实 panic 世界");
    world.run_ticks(2).expect("发布240");
    world.run_day_end_public().expect("成功日结");
    world.set_civil_date(next_date);
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
    let spec = partial_drain_spec();
    let calendar = crate::calendar::TradingCalendar::current_default_calendar()
        .expect("显式 raw Calendar 政策合法");
    let next_date = calendar
        .next_trading_day(
            crate::calendar::CalendarExchange::Sse,
            spec.civil_start_date,
        )
        .expect("fixture 下一 SSE trading date 合法");
    let mut world = World::spawn(spec, calendar).expect("构造两股真实日结世界");
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
        assert!(
            matches!(&error, super::protocol::RuntimeError::ProtocolInvariant { detail }
            if detail.contains("收到 1 个真实 DeskDrained 后收集失败"))
        );
    } else {
        assert!(matches!(
            error,
            super::protocol::RuntimeError::SendFailed { .. }
        ));
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
    // raw World 的日期由 caller 推进；成功日结后使用同一 Calendar 的下一开市日。
    world.set_civil_date(next_date);
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
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造两组收盘竞价世界");
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
        calendar: super::calendar_test_support::open_tick(
            DAY_END_TICK + 1,
            240,
            9,
            3,
            1,
            civil_date,
        ),
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
    let mut world = World::spawn(
        closing_spec(CLOSING_PRICE),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造世界");
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
    let mut world = World::spawn(
        closing_spec(CLOSING_PRICE),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造世界");
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
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造世界");
    world.run_ticks(1).expect("tick239 收盘申报");
    world.run_day_end_public().expect("未 roll 前的手动日结");
    let snapshot = TickSnapshot {
        calendar: super::calendar_test_support::open_tick(DAY_END_TICK, 240, 9, 3, 1, civil_date),
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
    eprintln!("#121 manual Drain tick239→Start240: broadcast_upper={}, purse_reference={}, expected_upper=1100", snapshot.stocks[0].daily_upper_limit, op.purse_reference);
    world.broadcast_external_plan(vec![op], vec![]);
    world
        .run_ticks(1)
        .expect("Start240 仍属当日，广播和受理必须使用同一旧昨收");
    assert_eq!(op.purse_reference, 1_100, "最后一拍仍使用当日旧昨收涨停");
    let orders = world.collect_desk_resting().expect("真实受理挂单").0;
    let order = orders[0]
        .iter()
        .find(|o| o.owner == (0, 0))
        .expect("手动日结后买单入簿");
    assert_eq!((order.price, order.purse), (1_100, 110_501));
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
    let template = World::spawn(
        closing_spec(CLOSING_PRICE),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("行情模板");
    let snapshot = TickSnapshot {
        calendar: super::calendar_test_support::open_tick(
            DAY_END_TICK + 1,
            240,
            9,
            3,
            1,
            spec.civil_start_date,
        ),
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
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("两组真实 NPC 世界");
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
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("两组两股世界");
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
        let mut world = World::spawn(
            spec,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .expect("开启笼子的收盘竞价世界");
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

/// 对照从相同最新价及真实昨收恢复；最后一拍保旧昨收，下一日才用收盘价。
fn same_new_day_sell_control_spec(close: i64, final_tick: u64) -> WorldSpec {
    let previous_close = if final_tick % DAY_END_TICK == 0 {
        close
    } else {
        INITIAL_PRICE
    };
    let mut spec = closing_spec(close);
    spec.stocks[0] = super::world::DeskStockSpec::main_board(close);
    spec.stocks[0].initial_market = Some(super::desk::DeskMarketSeed {
        last_price: close,
        last_close: previous_close,
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
            let mut crossed = World::spawn(
                spec,
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .expect("跨日卖向世界");
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
            let mut control = World::spawn(
                control_spec,
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .expect("同新日昨收稳态对照");
            control.run_ticks(1).expect("对照 Start 真实机构卖向消费");
            let lower = if final_tick == DAY_END_TICK {
                close * 9 / 10
            } else {
                900
            };
            let sell_floor = sell_cage_floor(close, lower);
            let crossed_view = &crossed.public_views()[0];
            let control_view = &control.public_views()[0];
            assert_eq!(
                (
                    crossed_view.last_price,
                    crossed_view.daily_lower_limit,
                    crossed_view.min_sell_price
                ),
                (close, lower, sell_floor),
                "按真实新日基准消费，卖向公开下限=笼子下限，不残留前日口径"
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
            eprintln!("#121 sell Start{next_tick}, manual={manual}, close={close}: public_lower={lower}, public_min_sell={sell_floor}, actual_inst_sell={close}, control_inst_sell={close}");
            crossed.shutdown().expect("跨日世界关停");
            control.shutdown().expect("对照世界关停");
        }
    }
}

#[test]
fn next_start_sell_boundary_matches_the_sell_cage_floor() {
    use super::decision::PendingOp;
    for (close, lower, actual_floor) in [(1_100, 990, 1_078), (900, 810, 882)] {
        assert_eq!(sell_cage_floor(close, lower), actual_floor);
        for final_tick in [239, 240] {
            let lower = if final_tick == DAY_END_TICK {
                lower
            } else {
                900
            };
            let actual_floor = sell_cage_floor(close, lower);
            let rejection = if actual_floor == lower {
                super::ReceiptKind::PriceOutOfBand
            } else {
                super::ReceiptKind::PriceCageExceeded
            };
            let mut spec = closing_spec(close);
            Arc::make_mut(&mut spec.config).price_cage_enabled = true;
            let mut crossed = World::spawn(
                spec,
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .expect("开启笼子的日界世界");
            crossed
                .run_ticks(final_tick - (DAY_END_TICK - 2))
                .expect("收盘申报");
            crossed.run_day_end_public().expect("手动日结");
            let view = &crossed.public_views()[0];
            assert_eq!(
                (view.daily_lower_limit, view.min_sell_price),
                (lower, actual_floor),
                "公开卖向下限必须等于 desk 实际受理的卖笼子下限"
            );
            let public_floor = view.min_sell_price;
            let mut control = World::spawn(
                same_new_day_sell_control_spec(close, final_tick),
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .expect("同基准、同阶段对照");
            for world in [&mut crossed, &mut control] {
                let prices = [actual_floor - 1, actual_floor, actual_floor + 1];
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
                assert_eq!(record.desk_receipts.len(), 1, "仅笼子外一分显式拒绝");
                assert!(record.desk_receipts.iter().all(|r| r.kind == rejection));
                assert!(
                    record.group_rejections.is_empty(),
                    "不是资源不足掩盖价格拒单"
                );
                let orders = world.collect_desk_resting().expect("真实卖向入簿边界").0;
                let mut prices: Vec<_> = orders[0].iter().map(|o| o.price).collect();
                prices.sort_unstable();
                assert_eq!(
                    prices,
                    vec![actual_floor, actual_floor + 1],
                    "公开下限与其上一分均受理"
                );
                assert!(orders[0]
                    .iter()
                    .all(|o| o.side == Side::Sell && o.qty == LOT));
                let accounts = world.snapshot_accounts().expect("拒单资源退回后账户");
                let seller = &accounts.iter().find(|(g, _)| *g == 0).expect("组0").1[1];
                assert_eq!(
                    seller.pending_sell_qty[0],
                    2 * LOT,
                    "一笔拒单不泄漏股份占用，两笔受理正常冻结"
                );
            }
            eprintln!("#125 sell Start{}: close={close}, public_lower={lower}, public_min_sell={public_floor}, desk_sell_floor={actual_floor}, floor_minus_one=rejected, floor=accepted; same as control", final_tick + 1);
            crossed.shutdown().expect("跨日关停");
            control.shutdown().expect("对照关停");
        }
    }
}

#[test]
fn successful_next_start_closes_the_day_end_abort_window() {
    let mut world = World::spawn(
        closing_spec(CLOSING_PRICE),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造世界");
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
    let mut world = World::spawn(
        closing_spec(CLOSING_PRICE),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造世界");
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

#[test]
fn final_tick_uses_old_close_then_next_tick_uses_the_drained_close() {
    let mut spec = basic_spec(2, 3, 26, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 30;
    spec.auction_ticks = 8;
    spec.closing_auction_ticks = 4;
    spec.initial_committed_tick = 88;
    spec.initial_cash = 2_000_000;
    spec.stocks[2].exchange = crate::StockExchange::Shenzhen;
    spec.stocks[2].initial_price = 1274;
    spec.stocks[2].limit_bps = 2000;
    spec.stocks[2].max_order_qty_limit = 300_000;
    spec.stocks[2].max_order_qty_market = 150_000;
    spec.stocks[2].initial_market = Some(super::desk::DeskMarketSeed {
        last_price: 1529,
        last_close: 1274,
        day_high: 1529,
        day_low: 1274,
        volume_today: 100,
        turnover_today: 152900,
        trade_count: 1,
        candle: Some((1274, 1529, 1274, 1529, 100)),
        minute_closes: vec![],
        minute_bars: vec![],
    });
    let date = spec.civil_start_date;
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造原失败数字的两组三股短 fixture");
    world.run_ticks(1).expect("tick89 发布当日观察基");
    assert_eq!(world.public_views()[2].daily_upper_limit, 1529);
    let make_buy = |world: &World, tick| {
        let snapshot = TickSnapshot {
            calendar: super::calendar_test_support::open_tick(tick, 90, 9, 3, 3, date),
            tick,
            market_minute: 0,
            civil_date: date,
            stocks: Arc::new(world.public_views().to_vec()),
        };
        let mut op = intent_to_pending(
            Intent::PlaceLimit {
                code: StockId(2),
                side: Side::Buy,
                price: LimitPrice::Highest,
                qty: 300,
            },
            &build_market_view(&snapshot),
        )
        .expect("正式意图转换");
        op.local = 25;
        op
    };
    world.broadcast_external_plan(vec![make_buy(&world, 90)], vec![]);
    world
        .run_ticks(1)
        .expect("tick90 仍属原交易日，不能提前提高昨收和精确 purse");
    let orders = world.collect_desk_resting().expect("日末真实挂单").0;
    let order = orders[2]
        .iter()
        .find(|o| o.owner == (0, 25))
        .expect("历史失败owner已受理");
    assert_eq!(
        (order.price, order.purse),
        (1529, 459205),
        "旧昨收1274的20%上限与含费预留"
    );
    let publication = world.publication();
    let published = publication.load();
    world.run_day_end_public().expect("日末成功 Drain");
    assert_eq!(world.public_views()[2].daily_upper_limit, 1835);
    world.broadcast_external_plan(vec![make_buy(&world, 91)], vec![]);
    world
        .run_ticks(1)
        .expect("tick91 使用真实日结后的新昨收，不得再次roll");
    assert_eq!(published.tick, 90, "日结前发布句柄不可变");
    let orders = world.collect_desk_resting().expect("次日真实挂单").0;
    let order = orders[2]
        .iter()
        .find(|o| o.owner == (0, 25))
        .expect("次日买单已受理");
    assert_eq!(
        (order.price, order.purse),
        (1835, 551006),
        "新昨收1529的20%上限与含费预留"
    );
    let report = world.shutdown().expect("正常退出并回收全部线程");
    assert_eq!(report.worker_results.len(), 5);
    let buyer = &report.group_finals[0].accounts[25];
    assert_eq!(buyer.cash, 2_000_000, "两次无成交买单最终逐分退回");
    assert_eq!(buyer.buy_purse_held_cents, 0);
    assert_eq!(
        report.desk_finals[2].market.candles.len(),
        1,
        "同日二次Drain不重复归档"
    );
    assert_eq!(report.desk_finals[2].market.last_close, 1529);
}

/// 低价股十个最小价位更宽，非整分百分比按交易所规则四舍五入。
#[test]
fn initial_public_cage_bounds_cover_ten_ticks_and_rounding() {
    for (price, lower, sell, buy, upper) in
        [(200, 180, 190, 210, 220), (1_026, 923, 1_005, 1_047, 1_129)]
    {
        let world = World::spawn(
            phase_view_spec(price, 0, 0, true),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .expect("构造低价与四舍五入边界世界");
        let view = &world.public_views()[0];
        assert_eq!(
            (
                view.daily_lower_limit,
                view.min_sell_price,
                view.max_buy_price,
                view.daily_upper_limit
            ),
            (lower, sell, buy, upper),
            "参考价={price}：公开边界遵守十个最小价位取宽及四舍五入"
        );
        world.shutdown().expect("关停");
    }
}

/// #125 覆盖：固定 ticks_per_day=30、closing=3，用 auction_ticks 与
/// initial_committed_tick 把首个真实 tick 放进指定相位。
fn phase_view_spec(
    initial_price: i64,
    auction_ticks: u64,
    initial_committed_tick: u64,
    cage: bool,
) -> WorldSpec {
    let mut spec = basic_spec(
        1,
        1,
        1,
        Box::new(|_group| -> GroupScript { Box::new(|_, _| TickPlan::default()) }),
    );
    spec.stocks = vec![super::world::DeskStockSpec::main_board(initial_price)];
    spec.ticks_per_day = 30;
    spec.auction_ticks = auction_ticks;
    spec.closing_auction_ticks = 3;
    spec.initial_committed_tick = initial_committed_tick;
    let mut config = GameConfig::proposed_defaults();
    config.price_cage_enabled = cage;
    spec.config = Arc::new(config);
    spec
}

/// 相位×笼子：只有连续竞价叠加卖笼子下限；其余相位等于日跌停。
/// 买向同步锁定，防止只修卖向时把买向口径带偏。
#[test]
fn public_view_cage_bounds_follow_the_trading_phase() {
    // (auction_ticks, initial_committed_tick, 期望 min_sell, 期望 max_buy)
    let cases = [
        (6u64, 0u64, 990i64, 1_210i64), // tick1 CallAuction
        (6, 3, 990, 1_210),             // tick4 PreOpen
        (0, 0, 1_078, 1_122),           // tick1 Continuous
        (6, 26, 990, 1_210),            // tick27 ClosingAuction
    ];
    for (auction_ticks, initial_tick, sell, buy) in cases {
        let mut world = World::spawn(
            phase_view_spec(1_100, auction_ticks, initial_tick, true),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .expect("构造相位世界");
        world.run_ticks(1).expect("单拍收束");
        let view = &world.public_views()[0];
        assert_eq!(
            (
                view.daily_lower_limit,
                view.min_sell_price,
                view.max_buy_price
            ),
            (990, sell, buy),
            "auction_ticks={auction_ticks} initial={initial_tick}：笼子只在连续竞价生效"
        );
        world.shutdown().expect("关停");
    }
}

/// 笼子关闭时，所有相位都必须退回纯日涨跌停口径。
#[test]
fn disabled_cage_keeps_daily_limits_in_every_phase() {
    for (auction_ticks, initial_tick) in [(0u64, 0u64), (6, 0), (6, 3), (6, 26)] {
        let mut world = World::spawn(
            phase_view_spec(1_100, auction_ticks, initial_tick, false),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .expect("构造关闭笼子的相位世界");
        world.run_ticks(1).expect("单拍收束");
        let view = &world.public_views()[0];
        assert_eq!(
            (
                view.daily_lower_limit,
                view.daily_upper_limit,
                view.min_sell_price,
                view.max_buy_price
            ),
            (990, 1_210, 990, 1_210),
            "auction_ticks={auction_ticks} initial={initial_tick}：关闭笼子后不得残留卖/买笼子口径"
        );
        world.shutdown().expect("关停");
    }
}

/// initial_views 是首个 Start 的决策观察基：必须与 desk 首拍受理边界同口径，
/// 且只有首拍处于连续竞价时才叠加笼子。
#[test]
fn initial_public_view_matches_the_first_start_phase() {
    // (initial_price, auction_ticks, initial_tick, cage, 期望 lower, min_sell, max_buy)
    let cases = [
        (1_100i64, 0u64, 0u64, true, 990i64, 1_078i64, 1_122i64), // Continuous
        (900, 0, 0, true, 810, 882, 918),                         // Continuous 下跌向
        (1_100, 6, 0, true, 990, 990, 1_210),                     // CallAuction
        (1_100, 6, 3, true, 990, 990, 1_210),                     // PreOpen
        (1_100, 0, 26, true, 990, 990, 1_210),                    // ClosingAuction
        (1_100, 0, 0, false, 990, 990, 1_210),                    // 笼子关闭
    ];
    for (price, auction_ticks, initial_tick, cage, lower, sell, buy) in cases {
        let world = World::spawn(
            phase_view_spec(price, auction_ticks, initial_tick, cage),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .expect("构造初始观察基");
        let view = &world.public_views()[0];
        assert_eq!(
            (
                view.daily_lower_limit,
                view.min_sell_price,
                view.max_buy_price
            ),
            (lower, sell, buy),
            "初始广播价格={price} auction_ticks={auction_ticks} initial={initial_tick} cage={cage}"
        );
        world.shutdown().expect("关停");
    }
}

/// 首拍连续竞价：初始公开边界必须与 desk 实际受理完全一致。
/// 卖 1077 拒 / 1078 受，买 1123 拒 / 1122 受（分股隔离，避免同拍交叉成交）。
#[test]
fn first_start_accepts_exactly_the_initial_public_cage_bounds() {
    use super::decision::PendingOp;
    let mut spec = basic_spec(
        1,
        2,
        1,
        Box::new(|_group| -> GroupScript { Box::new(|_, _| TickPlan::default()) }),
    );
    spec.stocks = vec![
        super::world::DeskStockSpec::main_board(1_100),
        super::world::DeskStockSpec::main_board(1_100),
    ];
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    let mut config = GameConfig::proposed_defaults();
    config.price_cage_enabled = true;
    spec.config = Arc::new(config);
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造首拍连续竞价世界");
    let view = &world.public_views()[0];
    assert_eq!(
        (view.min_sell_price, view.max_buy_price),
        (1_078, 1_122),
        "初始公开边界"
    );
    let ops = vec![
        PendingOp {
            local: 0,
            stock: 0,
            side: Side::Sell,
            routed: super::RoutedPrice::Fixed(1_077),
            purse_reference: 1_077,
            qty: LOT,
        },
        PendingOp {
            local: 0,
            stock: 0,
            side: Side::Sell,
            routed: super::RoutedPrice::Fixed(1_078),
            purse_reference: 1_078,
            qty: LOT,
        },
        PendingOp {
            local: 0,
            stock: 1,
            side: Side::Buy,
            routed: super::RoutedPrice::Fixed(1_123),
            purse_reference: 1_123,
            qty: LOT,
        },
        PendingOp {
            local: 0,
            stock: 1,
            side: Side::Buy,
            routed: super::RoutedPrice::Fixed(1_122),
            purse_reference: 1_122,
            qty: LOT,
        },
    ];
    world.broadcast_external_plan(ops, vec![]);
    world.run_ticks(1).expect("首拍真实受理");
    let record = world.report_log.last().expect("首拍记录");
    assert_eq!(
        record
            .desk_receipts
            .iter()
            .filter(|r| r.kind == super::ReceiptKind::PriceCageExceeded)
            .count(),
        2,
        "买/卖各一笔笼子外一分显式拒绝"
    );
    assert!(
        record.group_rejections.is_empty(),
        "不得以资源不足掩盖价格笼子拒单"
    );
    let orders = world.collect_desk_resting().expect("首拍挂单").0;
    let mut sells: Vec<_> = orders[0]
        .iter()
        .filter(|o| o.side == Side::Sell)
        .map(|o| o.price)
        .collect();
    sells.sort_unstable();
    let mut buys: Vec<_> = orders[1]
        .iter()
        .filter(|o| o.side == Side::Buy)
        .map(|o| o.price)
        .collect();
    buys.sort_unstable();
    assert_eq!(sells, vec![1_078], "卖向下限恰为公开 min_sell_price");
    assert_eq!(buys, vec![1_122], "买向上限恰为公开 max_buy_price");
    world.shutdown().expect("关停");
}

/// 初始行情计算失败必须在启动 worker 前返回，不能留下未 join 的后台线程。
#[test]
fn invalid_initial_cage_bound_starts_no_workers() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let started = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&started);
    let mut spec = phase_view_spec(i64::MAX, 0, 0, true);
    spec.script_for = Box::new(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Box::new(|_, _| TickPlan::default())
    });
    let error = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .err()
    .expect("非法笼子边界明确失败");
    assert!(matches!(
        error,
        super::protocol::RuntimeError::ProtocolInvariant { .. }
    ));
    assert_eq!(
        started.load(Ordering::SeqCst),
        0,
        "行情失败前不得启动组 worker"
    );
}

fn shenzhen_closing_tie_world() -> World {
    let mut spec = basic_spec(
        2,
        1,
        2,
        Box::new(|group| {
            Box::new(move |tick, round| {
                let mut plan = TickPlan::default();
                if group == 0 && tick == 235 {
                    for (local, side) in [(0, Side::Buy), (1, Side::Sell)] {
                        plan.places.push(PlaceSpec {
                            local,
                            stock: 0,
                            side,
                            price: 1100,
                            qty: 100,
                        });
                    }
                }
                if group == 0 && tick == 239 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Buy,
                        price: 1100,
                        qty: 100,
                    });
                    plan.places.push(PlaceSpec {
                        local: 1,
                        stock: 0,
                        side: Side::Sell,
                        price: 900,
                        qty: 100,
                    });
                }
                // 最后一拍真实 Continue 仍属同一交易日，不能在轮间切换基准。
                if tick == 240 && round == 0 {
                    plan.continuation = true;
                }
                plan
            })
        }),
    );
    spec.stocks[0].exchange = crate::StockExchange::Shenzhen;
    spec.ticks_per_day = 240;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 3;
    spec.initial_committed_tick = 233;
    Arc::make_mut(&mut spec.config).price_cage_enabled = false;
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("构造真实日内成交与收盘并列候选")
}

#[test]
fn shenzhen_closing_tie_keeps_the_declared_prior_close_and_full_daily_totals() {
    let mut world = shenzhen_closing_tie_world();
    world.run_ticks(7).expect("tick240多轮成功收束");
    assert_eq!(
        world.public_views()[0].daily_upper_limit,
        1100,
        "最后一拍仍用昨收1000"
    );
    world.run_day_end_public().expect("收盘清算后归档");
    assert_eq!(world.drained_day_trades.len(), 1);
    assert_eq!(
        world.drained_day_trades[0].price, 900,
        "已登记深市统一昨收简化：900/1100距1000同距取低价"
    );
    let bars = world.take_pending_day_bars()[0]
        .take()
        .expect("完整当日分钟桶");
    let totals = bars.iter().fold((0_u64, 0_u128, 0_u64), |(v, a, n), b| {
        (v + b.volume_shares, a + b.turnover_cents, n + b.trade_count)
    });
    assert_eq!(
        totals,
        (200, 200000, 2),
        "日内100股与收盘100股不得丢失或重复"
    );
    let candle = world.take_pending_day_candles()[0].take().expect("真实终K");
    assert_eq!(
        (
            candle.volume,
            u128::try_from(candle.turnover_cents).expect("成交额非负"),
            candle.trade_count
        ),
        totals
    );
    assert_eq!((candle.high, candle.low, candle.close), (1100, 900, 900));
    let report = world.shutdown().expect("重复Drain只作诊断并join");
    assert_eq!(report.desk_finals[0].market.candles.len(), 1);
    assert_eq!(report.desk_finals[0].market.traded_days_total, 1);
    assert_eq!(report.desk_finals[0].market.candles[0].volume, 200);
    assert_eq!(report.desk_finals[0].market.last_close, 900);
}

#[test]
fn final_tick_multi_round_and_day_end_retry_keep_each_board_and_direction() {
    for (limit_bps, upper_before) in [(1000, 1100), (2000, 1200)] {
        for close in [1100, 900] {
            let mut spec = closing_spec(close);
            spec.stocks[0].limit_bps = limit_bps;
            if limit_bps == 2000 {
                spec.stocks[0].exchange = crate::StockExchange::Shenzhen;
                spec.stocks[0].max_order_qty_limit = 300_000;
                spec.stocks[0].max_order_qty_market = 150_000;
            }
            let script_for = std::mem::replace(
                &mut spec.script_for,
                Box::new(|_| Box::new(|_, _| TickPlan::default())),
            );
            spec.script_for = Box::new(move |group| {
                let script = script_for(group);
                Box::new(move |tick, round| {
                    let mut plan = script(tick, round);
                    plan.continuation = tick == 240 && round == 0;
                    plan
                })
            });
            let mut world = World::spawn(
                spec,
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .expect("构造分板块双向多轮日末");
            world.run_ticks(2).expect("tick239/240与Continue均受理成功");
            assert_eq!(world.public_views()[0].daily_upper_limit, upper_before);
            let prior_orders = world.collect_desk_resting().expect("日末真实簿");
            let prior_candles = world.take_candle_projection().expect("日末K事实");
            let publication = world.publication();
            let published = publication.load();
            world.set_day_end_validator(Box::new(|_| {
                Err("日结拒绝，用于验证清算后roll可逆".into())
            }));
            world
                .run_day_end_public()
                .expect_err("失败日结必须显式返回");
            assert_eq!(world.collect_desk_resting().expect("回滚簿"), prior_orders);
            assert_eq!(
                world.take_candle_projection().expect("回滚K"),
                prior_candles
            );
            assert!(Arc::ptr_eq(&published, &publication.load()));
            assert_eq!(world.public_views()[0].daily_upper_limit, upper_before);
            world.set_day_end_validator(Box::new(|_| Ok(())));
            world.run_day_end_public().expect("相同事实重试日结");
            let expected_upper = close * (10_000 + i64::from(limit_bps)) / 10_000;
            let expected_lower = close * (10_000 - i64::from(limit_bps)) / 10_000;
            assert_eq!(
                (
                    world.public_views()[0].daily_upper_limit,
                    world.public_views()[0].daily_lower_limit
                ),
                (expected_upper, expected_lower)
            );
            let bars = world.take_pending_day_bars()[0].take().expect("重试完整桶");
            let candle = world.take_pending_day_candles()[0].take().expect("重试终K");
            assert_eq!(bars.iter().map(|b| b.volume_shares).sum::<u64>(), 200);
            assert_eq!(
                (candle.volume, candle.turnover_cents, candle.trade_count),
                (200, close * 200, 2)
            );
            let report = world
                .shutdown()
                .expect("成功日结后重复Drain不得再次清算/归档");
            assert_eq!(report.desk_finals[0].market.candles.len(), 1);
            assert_eq!(report.desk_finals[0].market.traded_days_total, 1);
            assert_eq!(report.desk_finals[0].market.candles[0].volume, 200);
        }
    }
}

#[test]
fn shutdown_preserves_unconsumed_day_end_minute_bars() {
    let mut world = shenzhen_closing_tie_world();
    world.run_ticks(7).expect("日末真实成交与收盘申报");
    world
        .run_day_end_public()
        .expect("成功日结且尚未取得分钟桶");
    let report = world.shutdown().expect("同一Start后的关停");
    let bars = report.drained_minute_bars[0]
        .as_ref()
        .expect("已完成日结的完整分钟桶不可被二次Drain覆盖");
    let totals = bars.iter().fold((0_u64, 0_u128, 0_u64), |(v, a, n), b| {
        (v + b.volume_shares, a + b.turnover_cents, n + b.trade_count)
    });
    assert_eq!(totals, (200, 200000, 2), "关停必须保留未消费的日结分钟事实");
    assert_eq!(report.desk_finals[0].market.candles.len(), 1);
    assert_eq!(report.worker_results.len(), 3);
}

#[test]
fn shutdown_day_end_reuse_respects_consumed_aborted_and_new_start_paths() {
    use super::decision::PendingOp;
    for path in 0..4 {
        let mut world = shenzhen_closing_tie_world();
        world
            .run_ticks(if path == 3 { 2 } else { 7 })
            .expect("真实成交起点");
        world.run_day_end_public().expect("成功日结");
        if path == 0 {
            assert_eq!(
                world.take_pending_day_bars()[0]
                    .as_ref()
                    .expect("先消费桶")
                    .iter()
                    .map(|b| b.volume_shares)
                    .sum::<u64>(),
                200
            );
        } else if path == 1 {
            world.abort_day_end();
        } else if path == 2 {
            // 新Start的真实买单须在关停时撤销，不能因旧last_day_end_tick漏Drain。
            world.broadcast_external_plan(
                vec![PendingOp {
                    local: 0,
                    stock: 0,
                    side: Side::Buy,
                    routed: super::RoutedPrice::Fixed(900),
                    purse_reference: 900,
                    qty: 100,
                }],
                vec![],
            );
            world.run_ticks(1).expect("新Start真实挂单");
            assert!(!world.collect_desk_resting().expect("关停前活动单").0[0].is_empty());
        }
        let report = world.shutdown().expect("按真实日结与Start状态关停");
        assert_eq!(report.worker_results.len(), 3);
        assert!(report.desk_finals[0].resting.is_empty());
        assert!(report
            .group_finals
            .iter()
            .flat_map(|g| &g.accounts)
            .all(|a| a.buy_purse_held_cents == 0 && a.pending_sell_qty[0] == 0));
        if path == 0 {
            assert!(
                report.drained_minute_bars[0].is_none(),
                "已消费日结桶不得重发"
            );
        } else {
            let bars = report.drained_minute_bars[0]
                .as_ref()
                .expect("未消费或真实新Drain结果");
            let expected = if path == 2 {
                0
            } else if path == 3 {
                100
            } else {
                200
            };
            assert_eq!(bars.iter().map(|b| b.volume_shares).sum::<u64>(), expected);
        }
    }
}

#[test]
fn restored_day_end_shutdown_does_not_archive_the_next_day_zero_candle() {
    let mut spec = basic_spec(2, 1, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.initial_committed_tick = 240;
    spec.initial_last_day_end_tick = Some(240);
    spec.ticks_per_day = 240;
    spec.stocks[0].initial_price = 900;
    spec.stocks[0].initial_daily_bars = vec![(1100, 1100, 900, 900, 200)];
    spec.stocks[0].initial_market = Some(super::desk::DeskMarketSeed {
        last_price: 900,
        last_close: 900,
        day_high: 900,
        day_low: 900,
        volume_today: 0,
        turnover_today: 0,
        trade_count: 0,
        candle: Some((900, 900, 900, 900, 0)),
        minute_closes: vec![],
        minute_bars: vec![],
    });
    let world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .expect("恢复完整日结静点");
    let report = world.shutdown().expect("恢复后未Start即可关停");
    assert_eq!(
        report.desk_finals[0].market.candles.len(),
        1,
        "不能再归档次日零活动K"
    );
    assert_eq!(report.desk_finals[0].market.traded_days_total, 1);
    assert_eq!(
        report.desk_finals[0]
            .market
            .candle
            .as_ref()
            .expect("次日零K仍活动")
            .volume,
        0
    );
    assert!(
        report.drained_minute_bars[0].is_none(),
        "恢复没有新的日结桶可发"
    );
}
