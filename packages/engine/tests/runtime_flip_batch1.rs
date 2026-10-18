//! 翻转集成测试迁移·第一批（撮合核心，10 项）：
//! 同一会话/种子双路径（旧管线 step_frame vs RuntimeDriver
//! step_with_frame）——对照事件类型集合/seq 单调/tick 前进/现金守恒/
//! 成交量方向。逐字段锚定=旧管线断言在 runtime 路径复现。
//!
//! 迁移台账（本文件即台账）：每测试标注对照的旧管线语义点。
//!
//! 断言强度批（sol-103 f-1/a-2 修复）：本文件原自带 setup_1stock 与
//! common/flip_helpers 几乎相同、唯一差异 arrival_rate=0.0——两路径都
//! 零下单，成交面测试（#8）与现金守恒方向（#9）空转（探针证据：
//! sol-103-red-evidence/batch1-vacuity-probe.txt，30 tick 种子 1..15
//! 全零成交）。已合并回公共 fixture（arrival=1.0）；成交面改确定性
//! 注入对手单（batch4_t1 写法），现金守恒收紧为 `total <= base`
//! （费用只减不增，无外部注入时现金无铸造通道）。

mod common;

use common::flip_helpers::{pipeline_events, runtime_events, setup_1stock};
use engine::{Event, GameSession};

// 1. 事件类型集合面：两条路径产生的事件都落在引擎合法事件集内。
#[test]
fn batch1_event_types_within_engine_vocabulary() {
    let (_, rt) = runtime_events(30, 1);
    let (_, pipe) = pipeline_events(30, 1);
    for e in rt.iter().chain(pipe.iter()) {
        assert!(matches!(
            e,
            Event::Trade { .. }
                | Event::OrderAccepted { .. }
                | Event::OrderCanceled { .. }
                | Event::IntentRejected { .. }
                | Event::AuctionTick { .. }
                | Event::AuctionCompleted { .. }
                | Event::PriceTick { .. }
        ));
    }
}

// 2. seq 单调（两路径各自）。
#[test]
fn batch1_seq_monotonic_both_paths() {
    for (name, events) in [
        ("runtime", runtime_events(30, 2).1),
        ("pipeline", pipeline_events(30, 2).1),
    ] {
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
                assert!(seq > p, "{name} seq 单调：{seq} <= {p}");
            }
            prev = Some(seq);
        }
    }
}

// 3. tick 前进（两路径）。
#[test]
fn batch1_tick_advances_both_paths() {
    let (rt_session, _) = runtime_events(10, 3);
    assert_eq!(rt_session.tick(), 10, "runtime tick 安装");
    let (pipe_session, _) = pipeline_events(10, 3);
    assert_eq!(pipe_session.tick(), 10, "旧管线 tick");
}

// 4. 玩家现金不动（两路径同构初始的确定性对照点）。
#[test]
fn batch1_player_cash_untouched_both_paths() {
    let (rt_session, _) = runtime_events(20, 4);
    let (pipe_session, _) = pipeline_events(20, 4);
    let initial = GameSession::new(setup_1stock(), 4).expect("基线构造");
    let player_cash = |s: &GameSession| s.account(engine::AccountId(0)).expect("玩家").cash();
    assert_eq!(
        player_cash(&rt_session),
        player_cash(&initial),
        "runtime 玩家现金不变"
    );
    assert_eq!(
        player_cash(&pipe_session),
        player_cash(&initial),
        "旧管线玩家现金不变"
    );
}

// 5. 账户数一致（两路径不丢账户）。
#[test]
fn batch1_account_count_matches() {
    let (rt, _) = runtime_events(5, 5);
    let (pipe, _) = pipeline_events(5, 5);
    assert_eq!(rt.account_count(), pipe.account_count());
}

// 6. runtime 事件含竞价窗事件（相位面：tick 1-15 为开盘竞价窗）。
#[test]
fn batch1_runtime_emits_auction_window_events() {
    let (_, events) = runtime_events(20, 6);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::AuctionTick { .. })),
        "竞价窗内应有 AuctionTick：{} 事件",
        events.len()
    );
}

// 7. 旧管线事件也含竞价窗事件（对照面成立）。
#[test]
fn batch1_pipeline_emits_auction_window_events() {
    let (_, events) = pipeline_events(20, 6);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::AuctionTick { .. })),
        "旧管线竞价窗事件对照面"
    );
}

// 8. Trade 事件字段域（价格>0/数量>0/maker≠taker）。
//    f-1 修复：公共 fixture（arrival=1.0）30 tick 内 NPC 仍零成交
//    （observe 门概率化——探针种子 1..15 全零），成交面改确定性注入：
//    retail 位 float 持仓卖 + 玩家买（100 股@1000），竞价完成拍
//    （tick 10）必产 Trade。手算 oracle：清算价=1000（双边同价）、
//    量=100、maker=卖方 AccountId(1)、taker=买方 AccountId(0)。
#[test]
fn batch1_trade_event_fields_wellformed() {
    use common::flip_helpers::inject_crossing_pair_at_1000;
    use engine::runtime::drive::RuntimeDriver;
    let mut session = GameSession::new(setup_1stock(), 199).expect("会话");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    inject_crossing_pair_at_1000(&mut driver);
    let mut events = Vec::new();
    for _ in 0..12 {
        let evs = driver
            .step_frame_equivalent(&mut session)
            .expect("runtime tick（含安装）");
        events.extend(evs);
        driver
            .install_market_views(&mut session)
            .expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    let trades: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::Trade { .. }))
        .collect();
    assert!(
        !trades.is_empty(),
        "注入对手单后必有 Trade（竞价完成拍；空则字段断言空转）"
    );
    let mut saw_injected_pair = false;
    for t in trades {
        let Event::Trade {
            price,
            qty,
            maker,
            taker,
            code,
            ..
        } = t
        else {
            unreachable!()
        };
        assert!(price.cents() > 0, "价格正");
        assert!(*qty > 0, "数量正");
        assert_ne!(maker, taker, "对手方不同（自成交限制已修）");
        assert_eq!(code, &engine::StockId(0), "单股 setup 路由");
        // 手算 oracle：注入对（卖 1@1000 × 买 0@1000）恰在竞价完成拍
        // 以清算价 1000 成交 100 股（R12 沪市单候选价=申报价本身）。
        if price.cents() == 1_000 && *qty == 100 {
            assert_eq!(
                (maker, taker),
                (&engine::AccountId(1), &engine::AccountId(0)),
                "注入对成交的腿身份（maker=卖方 1、taker=买方 0）"
            );
            saw_injected_pair = true;
        }
    }
    assert!(
        saw_injected_pair,
        "注入对必须真实成交（否则 fixture 失效）"
    );
}

// 9. 现金守恒方向（两路径现金总量不增——无外部注入）。
//    a-2 修复：原容差 +5_670_000_000 分（≈整个流通市值的 56 倍）使任何
//    现实铸造 bug 恒过。同种子同 fixture 无随机注入通道：费用只减不增
//    ⇒ 收紧为恒等方向 `total <= base`。
#[test]
fn batch1_cash_never_increases() {
    let initial = GameSession::new(setup_1stock(), 8).expect("基线");
    let base: i64 = (0..initial.account_count() as u64)
        .filter_map(|i| {
            initial
                .account(engine::AccountId(i))
                .map(|a| a.cash().cents())
        })
        .sum();
    for (name, (session, _)) in [
        ("runtime", runtime_events(20, 8)),
        ("pipeline", pipeline_events(20, 8)),
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
            "{name} 现金总量不增（费用只减不增；无铸造通道）：{total} vs 基线 {base}"
        );
    }
}

// 10. 相位一致性：驱动 30 tick 内不出现非法相位组合（越窗后 CallAuction
//     事件不再出现）。f-13 补强：越窗后的缺席面显式断言（tick 16-236
//     连续竞价窗内不得出现 CallAuction 事件——与 batch2 #6 同口径）。
#[test]
fn batch1_phase_window_consistency() {
    let (_, events) = runtime_events(30, 9);
    let mut call_seen_tick: Option<u64> = None;
    for e in &events {
        if let Event::AuctionTick { tick, .. } | Event::AuctionCompleted { tick, .. } = e {
            if *tick <= 15 {
                call_seen_tick = Some(*tick);
            }
        }
    }
    assert!(call_seen_tick.is_some(), "前 15 tick 应有开盘竞价事件");
    for e in &events {
        if let Event::AuctionTick { tick, phase, .. } | Event::AuctionCompleted { tick, phase, .. } =
            e
        {
            assert!(
                !(*tick > 15 && *tick <= 30 && matches!(phase, engine::TradingPhase::CallAuction)),
                "tick {tick}（连续竞价窗）不得再有 CallAuction 相位事件"
            );
        }
    }
}
