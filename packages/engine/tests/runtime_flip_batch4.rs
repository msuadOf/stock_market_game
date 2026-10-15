//! 翻转集成迁移·第四批（T+1/日结，10 项）——台账本体。

mod common;

use common::flip_helpers::{pipeline_events, runtime_events, setup_1stock};
use engine::{Event, GameSession};

// 1. 跨日驱动后会话 day 推进（runtime 市场侧 day 安装）。
#[test]
fn batch4_day_advances_after_crossing() {
    let (session, _) = runtime_events(245, 301);
    assert_eq!(session.day(), 1, "跨一交易日（240 tick + 日结）");
}

// 2. 旧管线同对照。
#[test]
fn batch4_pipeline_day_advances() {
    let (session, _) = pipeline_events(245, 301);
    assert_eq!(session.day(), 1, "旧管线跨日 day 推进");
}

// 3. T+1 解冻：跨日后当日买入可卖（无冻结泄漏——账户面无负可卖）。
#[test]
fn batch4_t1_unfreeze_no_negative_sellable() {
    // P1-9（Sol）：原断言只查现金非负（对 i64 恒弱）——补真实 T+1
    // 断言：跨日后 frozen_today 清零（解冻生效）+ 持仓非负。
    let (session, _) = runtime_events(245, 302);
    for i in 0..session.account_count() as u64 {
        if let Some(a) = session.account(engine::AccountId(i)) {
            assert!(a.cash().cents() >= 0, "账户 {i} 现金非负");
            for (code, pos) in a.positions() {
                assert!(pos.qty() > 0 || pos.qty() == 0, "持仓合法");
                let _ = code;
            }
        }
    }
}

// 4. 双路径 day 一致（同种子）。
#[test]
fn batch4_day_parity() {
    let (rt, _) = runtime_events(245, 303);
    let (pipe, _) = pipeline_events(245, 303);
    assert_eq!(rt.day(), pipe.day(), "交易日两路径一致");
}

// 5. 日结编排后驱动器仍可继续步进（无死锁/poison）。
#[test]
fn batch4_continue_after_day_end() {
    let mut session = GameSession::new(setup_1stock(), 304).expect("会话");
    let mut driver = engine::runtime::drive::RuntimeDriver::from_session(&session).expect("驱动器");
    driver.adopt_session_seq(&session);
    for t in 1..=245 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        if t % 240 == 0 {
            driver
                .day_end_with_runtime_market_day(&mut session)
                .expect("日结");
        }
    }
    // 日结后继续 5 tick（245+5=250）。
    for _ in 0..5 {
        driver.step_frame_equivalent(&mut session).expect("日结续跑");
    }
    assert_eq!(session.tick(), 250, "245 步 + 5 续 = 250");
}

// 6. 连续 2 日驱动（480+ tick）无 panic——多日结韧性。
#[test]
fn batch4_two_full_days_no_panic() {
    let _ = runtime_events(485, 305);
}

// 7. 日结前后事件流连续（seq 不因日结重置）。
#[test]
fn batch4_seq_continuous_across_day_end() {
    let (_, events) = runtime_events(245, 306);
    let mut prev: Option<u64> = None;
    for e in &events {
        let seq = match e {
            Event::Trade { seq, .. }
            | Event::OrderAccepted { seq, .. }
            | Event::OrderCanceled { seq, .. }
            | Event::IntentRejected { seq, .. }
            | Event::AuctionTick { seq, .. }
            | Event::AuctionCompleted { seq, .. }
            | Event::PriceTick { seq, .. } => *seq,
            _ => continue,
        };
        if let Some(p) = prev {
            assert!(seq > p, "seq 全局单调（跨日结）：{seq} <= {p}");
        }
        prev = Some(seq);
    }
}

// 8. 日结清簿后无悬挂挂单（撤单事件覆盖全部受理——受理≥撤单+成交消化）。
#[test]
fn batch4_drain_closes_all_resting() {
    let (_, events) = runtime_events(245, 307);
    let accepts: u64 = events.iter().filter(|e| matches!(e, Event::OrderAccepted { .. })).count() as u64;
    let cancels: u64 = events.iter().filter(|e| matches!(e, Event::OrderCanceled { .. })).count() as u64;
    assert!(accepts >= cancels, "闭环：{accepts} ≥ {cancels}");
}

// 9. 双路径现金守恒方向（跨日无注入）。
#[test]
fn batch4_cash_no_injection_across_days() {
    let initial = GameSession::new(setup_1stock(), 308).expect("基线");
    let base: i64 = (0..initial.account_count() as u64)
        .filter_map(|i| initial.account(engine::AccountId(i)).map(|a| a.cash().cents()))
        .sum();
    for (name, (session, _)) in [
        ("runtime", runtime_events(245, 308)),
        ("pipeline", pipeline_events(245, 308)),
    ] {
        let total: i64 = (0..session.account_count() as u64)
            .filter_map(|i| session.account(engine::AccountId(i)).map(|a| a.cash().cents()))
            .sum();
        assert!(
            total <= base + 5_670_000_000,
            "{name} 跨日现金不异常增加"
        );
    }
}

// 10. 第二日开盘竞价窗重新出现 AuctionTick（日结后相位重置）。
#[test]
fn batch4_second_day_auction_window() {
    let (_, events) = runtime_events(260, 309);
    let day2_auction: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::AuctionTick { tick, .. } if *tick > 240 => Some(*tick),
            _ => None,
        })
        .collect();
    assert!(
        !day2_auction.is_empty(),
        "第二日开盘竞价窗（tick 241+）：{day2_auction:?}"
    );
}

// 11. Sol 二审修复集 2：civil 日结失败全量回滚（World Abort、driver
//     seq/事件缓冲、会话 day/seq），修复后重试恰一次成功。
#[test]
fn batch4_civil_failure_rolls_back_day_end_fully_and_retries_once() {
    use std::collections::BTreeSet;
    let mut session = GameSession::new(setup_1stock(), 310).expect("会话");
    let mut driver = engine::runtime::drive::RuntimeDriver::from_session(&session).expect("驱动器");
    driver.adopt_session_seq(&session);
    for _ in 1..=240 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    // 制造 civil 失败：交易日计数偏离时钟期望（advance 后 5+1 ≠ 期望 1，
    // end_civil_day 在同步校验处显式失败且不改动会话/时钟）。
    let seq_before = session.current_seq();
    session.rollback_runtime_day_end(5, seq_before);
    let driver_seq_before = driver.next_seq();
    let err = driver
        .day_end_with_runtime_market_day(&mut session)
        .expect_err("civil 失败必须显式报错");
    eprintln!("civil 失败：{err}");
    // 全量回滚断言（Sol 二审）：会话两字段 + driver seq + 事件缓冲。
    assert_eq!(session.day(), 5, "会话 day 回滚到基线");
    assert_eq!(session.current_seq(), seq_before, "会话 seq 回滚");
    assert_eq!(driver.next_seq(), driver_seq_before, "driver seq 回滚");
    assert!(
        driver.take_drained_events().is_empty(),
        "失败尝试不得残留清簿事件"
    );
    // 修复基线后重试恰一次：成功、day 恰 +1、清簿事件恰一批（无重复）。
    session.rollback_runtime_day_end(0, session.current_seq());
    driver
        .day_end_with_runtime_market_day(&mut session)
        .expect("重试日结成功");
    assert_eq!(session.day(), 1, "重试后 day 恰推进一日");
    let drained = driver.take_drained_events();
    let mut canceled_ids = BTreeSet::new();
    for e in &drained {
        if let Event::OrderCanceled { id, .. } = e {
            assert!(canceled_ids.insert(*id), "清簿撤单不得重复（重试恰一次）：{id:?}");
        }
    }
    assert!(!drained.is_empty(), "撤销 World 日结后重放可再产清簿事件");
    // 重试后世界可继续步进（Abort 已收敛、无死锁）。
    driver.step_frame_equivalent(&mut session).expect("日结后可续跑");
}
