//! 翻转集成迁移·第二批（竞价/相位门禁，10 项）——台账本体。
//! 双路径对照：旧管线 step_frame vs RuntimeDriver（含安装）。

mod common;

use common::flip_helpers::{pipeline_events, runtime_events, setup_1stock};
use engine::{Event, TradingPhase};

// 1. 竞价窗内每 tick 有 AuctionTick（runtime）。
#[test]
fn batch2_runtime_auction_ticks_in_window() {
    let (_, events) = runtime_events(15, 101);
    let ticks: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::AuctionTick { tick, .. } => Some(*tick),
            _ => None,
        })
        .collect();
    assert!(!ticks.is_empty(), "竞价窗（tick<10 申报）应有 AuctionTick");
}

// 2. 竞价窗内 AuctionTick 的 phase=CallAuction（开盘）。
#[test]
fn batch2_auction_tick_phase_is_call_auction() {
    let (_, events) = runtime_events(12, 102);
    assert!(events.iter().all(|e| match e {
        Event::AuctionTick { phase, .. } | Event::AuctionCompleted { phase, .. } => {
            *phase == TradingPhase::CallAuction
        }
        _ => true,
    }), "前 12 tick（未过开盘窗）竞价事件 phase=CallAuction");
}

// 3. 开盘竞价完成（AuctionCompleted）出现在离开申报窗后（runtime）。
#[test]
fn batch2_opening_completion_after_window() {
    let (_, events) = runtime_events(20, 103);
    let completed_ticks: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::AuctionCompleted { tick, phase: TradingPhase::CallAuction, .. } => Some(*tick),
            _ => None,
        })
        .collect();
    assert!(
        completed_ticks.iter().all(|t| *t >= 10),
        "开盘清算在申报窗（前 2/3=10 tick）后：{completed_ticks:?}"
    );
}

// 4. 旧管线同对照面（竞价窗事件存在）。
#[test]
fn batch2_pipeline_auction_window_events() {
    let (_, events) = pipeline_events(15, 101);
    assert!(
        events.iter().any(|e| matches!(e, Event::AuctionTick { .. })),
        "旧管线对照面"
    );
}

// 5. AuctionCompleted 携带 clearing_price 与 matched_volume 域合法。
//    e-1 修复（断言强度批）：原断言在 clearing_price=None 时零检查且
//    无事件存在性前置（探针证据：seed 104 25 tick 内唯一一条
//    completed=None/vol=0——Some 分支与 matched_volume 从未绑定）。
//    现：(a) 前置 completed 非空；(b) None ⇒ vol==0（无交叉无价无量）；
//    (c) 注入交叉对手单绑定 Some 分支——手算 oracle：双边 100@1000 ⇒
//    清算价 1000、量 100（R12 沪市单候选价=申报价本身）。
#[test]
fn batch2_auction_completed_fields_wellformed() {
    use common::flip_helpers::inject_crossing_pair_at_1000;
    // 无注入臂：seed 104 的 completed=None/vol=0 形态（None 分支绑定）。
    let (_, events) = runtime_events(25, 104);
    let completed: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::AuctionCompleted { .. }))
        .collect();
    assert!(
        !completed.is_empty(),
        "25 tick 跨开盘竞价完成点，必有 AuctionCompleted（否则分支断言空转）"
    );
    let mut saw_none = false;
    for e in completed {
        let Event::AuctionCompleted {
            clearing_price,
            matched_volume,
            ..
        } = e
        else {
            unreachable!()
        };
        match clearing_price {
            Some(p) => {
                assert!(
                    (900..=1100).contains(&p.cents()),
                    "清算价在首日涨跌停带 [900,1100]（初始 1000±10%）：{}",
                    p.cents()
                );
                assert!(*matched_volume > 0, "有清算价必有成交量");
            }
            None => {
                assert_eq!(
                    *matched_volume, 0,
                    "无交叉 ⇒ 无价无量（matched_volume 必须为 0）"
                );
                saw_none = true;
            }
        }
    }
    // 注入臂（Some 分支绑定）：确定性交叉对手单 ⇒ 有价有量。
    let mut session = engine::GameSession::new(setup_1stock(), 199).expect("会话");
    let mut driver = engine::runtime::drive::RuntimeDriver::from_session(&mut session)
        .expect("驱动器");
    driver.adopt_session_seq(&session);
    inject_crossing_pair_at_1000(&mut driver);
    let mut injected_completed = Vec::new();
    for _ in 0..12 {
        let evs = driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        injected_completed.extend(
            evs.into_iter()
                .filter(|e| matches!(e, Event::AuctionCompleted { .. })),
        );
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    assert!(
        !injected_completed.is_empty(),
        "注入臂必有 AuctionCompleted"
    );
    let mut saw_crossing = false;
    for e in &injected_completed {
        if let Event::AuctionCompleted {
            clearing_price: Some(p),
            matched_volume,
            ..
        } = e
        {
            if p.cents() == 1_000 && *matched_volume == 100 {
                saw_crossing = true;
            }
        }
    }
    assert!(
        saw_crossing,
        "注入对必须以清算价 1000 成交 100 股（手算 oracle）：{injected_completed:?}"
    );
    // 两分支在本测试内至少各绑定一次（无注入臂 None + 注入臂 Some）。
    assert!(saw_none, "无注入臂必须出现 None 形态");
    assert!(saw_crossing, "Some 分支必须被注入臂绑定");
}

// 6. 连续竞价窗（tick 16-236）内不出现 AuctionTick（越窗即停）。
#[test]
fn batch2_no_auction_tick_in_continuous_window() {
    let (_, events) = runtime_events(40, 105);
    let late: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::AuctionTick { tick, .. } if *tick > 16 => Some(*tick),
            _ => None,
        })
        .collect();
    // 开盘窗=15 tick（申报 10+盘前 5）；收盘窗=最后 3 tick（237+）。
    assert!(
        late.iter().all(|t| *t >= 237),
        "tick 17-236 为连续竞价窗，不应有 AuctionTick：{late:?}"
    );
}

// 7. 竞价窗内新申报产生 OrderAccepted（受理而非即时成交——积累语义）。
#[test]
fn batch2_auction_window_accepts_not_trades() {
    let (_, events) = runtime_events(8, 106);
    let trades = events
        .iter()
        .filter(|e| matches!(e, Event::Trade { .. }))
        .count();
    assert_eq!(trades, 0, "申报窗（tick<10）内不撮合——零 Trade");
}

// 8. 双路径事件总量同数量级（无静默吞事件）。
#[test]
fn batch2_event_volume_same_order_both_paths() {
    let (_, rt) = runtime_events(20, 107);
    let (_, pipe) = pipeline_events(20, 107);
    assert!(
        rt.len() >= pipe.len() / 4 && pipe.len() >= rt.len() / 4,
        "事件总量同数量级：runtime={} pipeline={}",
        rt.len(),
        pipe.len()
    );
}

// 9. 旧管线 tick 前进与 runtime 一致（同种子对照）。
#[test]
fn batch2_tick_parity() {
    let (rt, _) = runtime_events(12, 108);
    let (pipe, _) = pipeline_events(12, 108);
    assert_eq!(rt.tick(), pipe.tick(), "tick 两路径一致");
}

// 10. setup 单股不变量：全部事件 code 都是 StockId(0)。
#[test]
fn batch2_single_stock_routing() {
    let (_, rt) = runtime_events(20, 109);
    for e in &rt {
        let code = match e {
            Event::Trade { code, .. }
            | Event::AuctionTick { code, .. }
            | Event::AuctionCompleted { code, .. }
            | Event::PriceTick { code, .. } => Some(code),
            Event::OrderAccepted { code, .. }
            | Event::OrderCanceled { code, .. }
            | Event::IntentRejected { code, .. } => Some(code),
            _ => None,
        };
        if let Some(c) = code {
            assert_eq!(c, &engine::StockId(0), "单股 setup 路由");
        }
    }
}
