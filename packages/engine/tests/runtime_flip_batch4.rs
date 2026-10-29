//! 翻转集成迁移·第四批（T+1/日结，10 项）——台账本体。

mod common;

use common::flip_helpers::{
    assert_order_event_invariants, pipeline_events, runtime_events, setup_1stock,
};
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
    let setup = setup_1stock();
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
                        code,
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
                        code,
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
    driver.step_frame_equivalent(&mut session).expect("成交拍");
    driver.install_market_views(&mut session).expect("行情安装");
    driver.install_candles(&mut session).expect("K 安装");
    for _ in 2..=240 {
        driver.step_frame_equivalent(&mut session).expect("步进");
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
    let mut driver =
        engine::runtime::drive::RuntimeDriver::from_session(&mut session).expect("驱动器");
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
        driver
            .step_frame_equivalent(&mut session)
            .expect("日结续跑");
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
//    a-3 修复（断言强度批）：原容差 +5_670_000_000 分（≈整个流通市值
//    的 56 倍）使任何现实铸造 bug 恒过。同种子同 fixture 无随机注入
//    通道：费用只减不增 ⇒ 收紧为恒等方向 `total <= base`（T+1 解冻/
//    日结清簿只移动冻结位与在途票，不动现金总量）。
#[test]
fn batch4_cash_no_injection_across_days() {
    let initial = GameSession::new(setup_1stock(), 308).expect("基线");
    let base: i64 = (0..initial.account_count() as u64)
        .filter_map(|i| {
            initial
                .account(engine::AccountId(i))
                .map(|a| a.cash().cents())
        })
        .sum();
    for (name, (session, _)) in [
        ("runtime", runtime_events(245, 308)),
        ("pipeline", pipeline_events(245, 308)),
    ] {
        let total: i64 = (0..session.account_count() as u64)
            .filter_map(|i| {
                session
                    .account(engine::AccountId(i))
                    .map(|a| a.cash().cents())
            })
            .sum();
        assert!(
            total <= base,
            "{name} 跨日现金不增（费用只减不增；无铸造通道）：{total} vs 基线 {base}"
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

// 11. 市场日结已安装后的 civil 不变量失败保留收盘并终结会话；
//     save、civil、step 与重复手动日结均返回首错，不重放已提交事件。
#[test]
fn batch4_civil_failure_preserves_installed_market_close_and_first_error() {
    let mut session = GameSession::new(setup_1stock(), 310).expect("会话");
    let mut driver =
        engine::runtime::drive::RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    for _ in 1..=240 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    assert_eq!(
        (session.tick(), session.day()),
        (240, 0),
        "末拍尚未安装市场日结"
    );
    let date = session.civil_date();
    let publication = driver.publication();
    let generation_before = publication.load().generation;
    // 始终沿用原 driver：TradingCompleted 后尚未显式安装市场 day，
    // 此时重建 driver 会先触发 shared_sessions 失配，根本到不了 civil。
    // 保留原失步注入，已提交的 5+1 市场日结不能因 civil 拒绝而撤回。
    let seq_before = session.current_seq();
    session.rollback_runtime_day_end(5, seq_before);
    let err = driver
        .day_end_with_runtime_market_day(&mut session)
        .expect_err("civil 失败必须显式报错");
    let cause = engine::SessionError::CivilClock(
        engine::session::CivilClockError::MarketSessionOutOfSync {
            date,
            completed_sessions: 6,
            expected_sessions: 1,
        },
    );
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "RuntimeDriver::day_end::civil".to_owned(),
        description: format!("市场日结安装后的自然日推进失败：{cause}"),
    };
    assert_eq!(
        err,
        fatal.to_string(),
        "必须到达已安装市场后的真实 civil 拒绝"
    );
    assert_eq!(session.poison_reason(), Some(&fatal), "封存同一个首错");
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (240, 6, 240)
    );
    assert_eq!(session.civil_date(), date, "civil 候选未提交，日期不推进");
    let published = publication.load();
    assert_eq!(published.tick, 240);
    assert!(
        published.generation > generation_before,
        "市场日结已实际发布"
    );
    assert_eq!(published.retained.days.len(), 1, "收盘保留链恰安装一次");
    assert_eq!(published.retained.days[0].date, date);
    let drained = driver.take_drained_events();
    assert!(
        drained.iter().any(|event| matches!(
            event,
            Event::AuctionCompleted {
                phase: engine::TradingPhase::ClosingAuction,
                ..
            }
        )),
        "已提交的收盘竞价事件不可丢弃"
    );
    for (index, event) in drained.iter().enumerate() {
        assert_eq!(
            event.seq(),
            seq_before + index as u64 + 1,
            "已安装市场事件逐条续号"
        );
    }
    let seq_after = seq_before + drained.len() as u64;
    assert_eq!(session.current_seq(), seq_after, "只安装真实市场事件");
    assert_eq!(
        driver.next_seq(),
        seq_after,
        "driver 与 Session 使用同一序号"
    );
    let hash_after = session.business_state_hash().expect("终态业务哈希");
    assert!(matches!(session.save(), Err(engine::SessionError::Step(actual)) if actual == fatal));
    assert!(
        matches!(session.end_civil_day(), Err(engine::SessionError::Step(actual)) if actual == fatal)
    );
    assert_eq!(
        driver.step_frame_equivalent(&mut session).unwrap_err(),
        fatal
    );
    assert_eq!(
        driver
            .day_end_with_runtime_market_day(&mut session)
            .unwrap_err(),
        fatal.to_string()
    );
    assert_eq!(
        session.business_state_hash().unwrap(),
        hash_after,
        "拒绝续跑不得再改业务状态"
    );
    assert_eq!(
        (
            session.tick(),
            session.day(),
            session.current_seq(),
            driver.next_seq(),
            driver.world_committed_tick()
        ),
        (240, 6, seq_after, seq_after, 240)
    );
    assert_eq!(session.civil_date(), date);
    assert!(
        std::sync::Arc::ptr_eq(&published, &publication.load()),
        "终态不重复发布收盘"
    );
    assert!(
        driver.take_drained_events().is_empty(),
        "拒绝续跑不得重复清簿事件"
    );
}
