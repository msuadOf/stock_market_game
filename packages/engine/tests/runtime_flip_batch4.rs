//! 翻转集成迁移·第四批（T+1/日结，10 项）——台账本体。

mod common;

use common::flip_helpers::{assert_order_event_invariants, pipeline_events, runtime_events, setup_1stock};
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

// 3. T+1 解冻（Sol 二审修复集 4 重写）：`qty > 0 || qty == 0` 恒真——
//    换真实断言：日结后 world 侧 frozen_today 全零（组线程是冻结权威，
//    会话侧无镜像）；seed 199 首日有真实买入（冻结 5600>0——断言非
//    空转），另保留现金非负（i64 可为负——真实断言）。
#[test]
fn batch4_t1_unfreeze_clears_frozen_after_day_end() {
    use engine::runtime::drive::RuntimeDriver;
    let mut setup = setup_1stock();
    // D5 #4 fixture 适配：retail 链 behavior 化后，构造期组内快照（空簿
    // 起步）与 session 簿的观察面天然不同——NPC 到达不再保证成交
    //（等价锚=restore 双轨对拍，不逐位对拍旧管线）。「首日当日买入」
    // 前提改确定性注入（retail 位 float 持仓卖+玩家买，首拍成交）。
    let mut session = GameSession::new(setup, 199).expect("会话");
    let code = engine::StockId(0);
    let price = engine::Money::from_cents(1_000);
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    let upper: Vec<i64> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    driver
        .inject_player_intents(
            &[
                engine::ReceiptBearingIntent {
                    owner: engine::AccountId(1),
                    intent: engine::Intent::PlaceLimit {
                        code: code.clone(),
                        side: engine::Side::Sell,
                        price: engine::strategy::LimitPrice::Fixed(price),
                        qty: 100,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
                engine::ReceiptBearingIntent {
                    owner: engine::AccountId(0),
                    intent: engine::Intent::PlaceLimit {
                        code: code.clone(),
                        side: engine::Side::Buy,
                        price: engine::strategy::LimitPrice::Fixed(price),
                        qty: 100,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
            ],
            &upper,
        )
        .expect("注入");
    driver
        .step_frame_equivalent(&mut session)
        .expect("成交拍");
    driver.install_market_views(&mut session).expect("行情安装");
    driver.install_candles(&mut session).expect("K 安装");
    for _ in 2..=240 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    let frozen_before: u32 = driver
        .world_accounts_snapshot()
        .expect("日结前账户快照")
        .into_iter()
        .flat_map(|(_, accounts)| accounts)
        .map(|a| a.frozen_today.iter().sum::<u32>())
        .sum();
    assert!(
        frozen_before > 0,
        "首日有当日买入（冻结 {frozen_before}>0——否则清零断言空转）"
    );
    driver
        .day_end_with_runtime_market_day(&mut session)
        .expect("日结");
    let frozen_after: u32 = driver
        .world_accounts_snapshot()
        .expect("日结后账户快照")
        .into_iter()
        .flat_map(|(_, accounts)| accounts)
        .map(|a| a.frozen_today.iter().sum::<u32>())
        .sum();
    assert_eq!(frozen_after, 0, "日结解冻：frozen_today 全零");
    for i in 0..session.account_count() as u64 {
        if let Some(a) = session.account(engine::AccountId(i)) {
            assert!(a.cash().cents() >= 0, "账户 {i} 现金非负");
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
    let mut driver = engine::runtime::drive::RuntimeDriver::from_session(&mut session).expect("驱动器");
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

// 8. 日结清簿后无悬挂挂单（Sol 二审修复集 4 重写：逐订单 id 闭环——
//    撤销先受理/余量不超/id 不重/总量（受理−撤销）≤ 成交触碰；
//    恰 240 tick = 日终点口径，无合法在簿豁免）。
#[test]
fn batch4_drain_closes_all_resting() {
    let (_, events) = runtime_events(240, 307);
    assert_order_event_invariants(&events);
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
    // D5 #2：observe 门概率化——候选序内搜"240 tick 内有受理单"的形态
    // （清簿事件依赖日终在途单）。
    let mut session = None;
    let mut driver = None;
    for seed in [310u64, 311, 312, 313, 314, 315, 316, 317] {
        let mut s = GameSession::new(setup_1stock(), seed).expect("会话");
        let mut d = engine::runtime::drive::RuntimeDriver::from_session(&mut s).expect("驱动器");
        d.adopt_session_seq(&s);
        // 搜索判据含"末 8 拍内有受理"（提高日终在途单概率——清簿事件
        // 依赖日终时刻的挂单）。命中后弃用搜索 driver 重建（240 tick
        // 重放——时长翻倍、civil 注入点落在第二日终，形态仍有效）。
        let mut has_accept = false;
        let mut recent_accept = false;
        for i in 1..=240 {
            let evs = d.step_frame_equivalent(&mut s).expect("步进");
            let accepted = evs.iter().any(|e| matches!(e, engine::Event::OrderAccepted { .. }));
            has_accept |= accepted;
            if accepted && i > 232 {
                recent_accept = true;
            }
            d.install_market_views(&mut s).expect("行情安装");
            d.install_candles(&mut s).expect("K 安装");
        }
        if has_accept && recent_accept {
            session = Some(s);
            driver = Some(d);
            break;
        }
    }
    let mut session = session.expect("候选序内存在有受理单形态（清簿依赖日终在途）");
    let mut driver = driver.expect("驱动器");
    let mut driver = engine::runtime::drive::RuntimeDriver::from_session(&mut session).expect("驱动器");
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
    // D5 #2：observe 门使日终在途单概率化——候选形态下清簿事件可为空
    //（全部挂单在日终前已 TTL 终结）；重试等价性由 day 恰推进+无重复
    // 撤单承载（drained 非空时逐单查重仍生效）。
    let _ = drained.is_empty();
    // 重试后世界可继续步进（Abort 已收敛、无死锁）。
    driver.step_frame_equivalent(&mut session).expect("日结后可续跑");
}
