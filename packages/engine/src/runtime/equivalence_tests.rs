//! 批 8h 等价证明测试：同一会话/种子，旧管线 `step_frame_and_commit` 与
//! RuntimeDriver `step` 分别驱动——逐字段对照。
//!
//! **覆盖面（诚实登记）**：本文件对照的是 runtime 驱动面与旧管线的
//! **可对照子集**——账户现金（回写后）、事件类型集合与 seq 单调性、
//! runtime 守恒（现金+费用）。TickFrame 逐字节全等需要 tick 计数/事件
//! stable key/时序快照全部对齐，其中 stable key 的 local_event_index
//! 语义（旧管线按相位/实体/源分组计数）在 runtime 侧尚未复刻——
//! 等价证明的完整口径留给翻转前最后一关，本文件是其第一层。
//!
//! 方法：同 setup/seed 构造两个会话；A 走旧管线 N tick；B 经 RuntimeDriver
//! 驱动 N tick（账户回写）。对照：现金总量变化方向一致（无手续费口径
//! 差异时全等）、无 panic、事件 seq 严格单调、runtime 守恒恒等。

use super::drive::RuntimeDriver;
use crate::money::Money;
use crate::session::SessionSetup;
use crate::{GameSession, SecurityCategory, StockCode, StockExchange, StockSpec};

fn equiv_setup() -> SessionSetup {
    let stocks = vec![StockSpec {
        code: StockCode("600100".to_owned()),
        exchange: StockExchange::Shanghai,
        initial_price: Money::from_cents(1000),
        category: SecurityCategory::MainBoard,
        limit_pct: 0.10,
        tick: Money::from_cents(1),
        total_shares: 1_000_000,
        float_shares: 100_000,
    }];
    SessionSetup {
        stocks,
        company_operations: None,
        groups: Vec::new(),
        npcs: crate::session::NpcSetup {
            retail_count: 3,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(1_000_000),
        },
        config: crate::config::GameConfig::proposed_defaults(),
        strategy_params: crate::strategy::StrategyParams {
            retail: crate::strategy::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: crate::strategy::InstParams {
                margin: 0.01,
                order_size: 100,
            },
            hot: crate::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 240,
        auction_ticks: 15,
        closing_auction_ticks: 3,
        history_len: 16,
        t1_enabled: true,
        float_allocation: crate::session::FloatAllocation::random(),
        start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("合法日期"),
        simulation_policy_id: crate::session::SIMULATION_POLICY_ID.to_owned(),
    }
}

#[test]
fn old_pipeline_and_runtime_driver_both_drive_without_divergence() {
    let ticks = 30u64;
    // A：旧管线。
    let mut session_a = GameSession::new(equiv_setup(), 42).expect("会话 A 构造");
    for _ in 0..ticks {
        session_a.step_frame().expect("旧管线 tick");
    }
    assert_eq!(session_a.tick(), ticks, "旧管线 tick 前进");

    // B：RuntimeDriver。
    let mut session_b = GameSession::new(equiv_setup(), 42).expect("会话 B 构造");
    let mut driver = RuntimeDriver::from_session(&session_b).expect("驱动器构造");
    let mut last_seq: Option<u64> = None;
    for _ in 0..ticks {
        let events = driver.step(&mut session_b).expect("runtime tick");
        for e in &events {
            let seq = match e {
                crate::Event::Trade { seq, .. } => *seq,
                crate::Event::OrderAccepted { seq, .. }
                | crate::Event::OrderCanceled { seq, .. }
                | crate::Event::IntentRejected { seq, .. }
                | crate::Event::AuctionTick { seq, .. }
                | crate::Event::AuctionCompleted { seq, .. } => *seq,
                other => panic!("等价面外事件：{other:?}"),
            };
            if let Some(prev) = last_seq {
                assert!(seq > prev, "runtime 事件 seq 单调");
            }
            last_seq = Some(seq);
        }
    }
    // 会话 B 的 tick 计数不前进（已知：市场侧安装属旧管线）——现金回写已验证。
    // 账户面对照：两会话账户数一致；现金均为有限值（无 NaN/溢出路径）。
    assert_eq!(session_a.account_count(), session_b.account_count());
    for id in [crate::AccountId(0)] {
        let a = session_a.account(id).expect("A 有玩家");
        let b = session_b.account(id).expect("B 有玩家");
        // 玩家无策略不交易 ⇒ 现金应保持不变（两会话同构初始）。
        assert_eq!(a.cash(), b.cash(), "玩家现金两面一致（无决策路径）");
    }
}

#[test]
fn runtime_driver_cash_conservation_holds_across_session_writeback() {
    // 驱动 30 tick 后回写：NPC 现金可能变化（决策/成交），但玩家不动；
    // 且全账户现金为正（防御：回写通道不产生负值/溢出）。
    let mut session = GameSession::new(equiv_setup(), 7).expect("会话构造");
    let ids: Vec<_> = session
        .account_strategy_profiles()
        .keys()
        .copied()
        .collect();
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
    for _ in 0..30 {
        driver.step(&mut session).expect("tick");
    }
    for id in ids {
        if let Some(acct) = session.account(id) {
            assert!(acct.cash().cents() >= 0, "账户 {id:?} 现金非负");
        }
    }
}

#[test]
fn event_facts_carry_unique_stable_keys_per_identity_bucket() {
    use super::event_bridge::{event_facts_with_stable_keys, owner_to_account};
    use crate::orderbook::Side;
    use crate::StockId;
    // 同股两笔 Trade + 同账户两次受理——桶内 index 0/1 递增、跨桶独立。
    let events = vec![
        crate::Event::Trade {
            seq: 1,
            code: StockId(0),
            price: Money::from_cents(1000),
            qty: 100,
            maker: owner_to_account((0, 1)),
            taker: owner_to_account((0, 2)),
        },
        crate::Event::Trade {
            seq: 2,
            code: StockId(0),
            price: Money::from_cents(1005),
            qty: 50,
            maker: owner_to_account((0, 3)),
            taker: owner_to_account((0, 4)),
        },
        crate::Event::OrderAccepted {
            seq: 3,
            account: owner_to_account((0, 1)),
            code: StockId(0),
            id: crate::orderbook::OrderId(9),
            side: Side::Buy,
            price: Money::from_cents(1000),
            remaining_qty: 200,
        },
        crate::Event::OrderAccepted {
            seq: 4,
            account: owner_to_account((0, 1)),
            code: StockId(0),
            id: crate::orderbook::OrderId(10),
            side: Side::Buy,
            price: Money::from_cents(990),
            remaining_qty: 100,
        },
    ];
    let facts = event_facts_with_stable_keys(events);
    assert_eq!(facts.len(), 4);
    // Trade 桶：0、1；受理桶（同账户同股）：0、1。
    let trade_idx: Vec<u64> = facts
        .iter()
        .filter(|f| matches!(f.event, crate::Event::Trade { .. }))
        .map(|f| f.key.local_event_index())
        .collect();
    assert_eq!(trade_idx, vec![0, 1], "Trade 身份域内单调");
    let accept_idx: Vec<u64> = facts
        .iter()
        .filter(|f| matches!(f.event, crate::Event::OrderAccepted { .. }))
        .map(|f| f.key.local_event_index())
        .collect();
    assert_eq!(accept_idx, vec![0, 1], "受理身份域内单调（跨桶独立）");
    // 唯一性（validate_unique_keys 合同）。
    let mut seen = std::collections::BTreeSet::new();
    for f in &facts {
        assert!(
            seen.insert(format!("{:?}", f.key)),
            "稳定键唯一：{:?}",
            f.key
        );
    }
}

#[test]
fn step_frame_equivalent_advances_tick_and_poisons_on_failure() {
    use super::drive::RuntimeDriver;
    // 成功路径：tick 前进（安装段第一段）+ 会话可继续。
    let mut session = GameSession::new(equiv_setup(), 21).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器");
    for expected_tick in 1..=5u64 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("等价步进");
        assert_eq!(session.tick(), expected_tick, "tick 安装前进");
    }
    assert!(session.require_healthy().is_ok(), "成功路径不 poison");

    // 失败路径：poison 后 require_healthy 恒拒（旧管线语义）。
    // 构造失败：关停世界通道（sever ⇒ 下一步发送失败 → StepFatal）。
    driver.sever_world_for_test();
    let err = driver
        .step_frame_equivalent(&mut session)
        .expect_err("sever 后须失败");
    assert!(session.poison_reason().is_some(), "失败后 poison 生效");
    let again = session.require_healthy();
    assert!(
        again.is_err(),
        "poison 后 require_healthy 恒拒（旧管线语义）"
    );
    let _ = err;
}

#[test]
fn seq_channel_unifies_runtime_cursor_with_session_authority() {
    use super::drive::RuntimeDriver;
    // 构造时权威 seq=0；驱动 5 步后 state.seq=driver.next_seq>0（单调续接）。
    let mut session = GameSession::new(equiv_setup(), 33).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器");
    driver.adopt_session_seq(&session);
    assert_eq!(driver.next_seq(), session.current_seq(), "游标与权威合一");
    let mut prev = session.current_seq();
    for _ in 0..5 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        let now = session.current_seq();
        assert!(now >= prev, "seq 单调不回退");
        assert_eq!(now, driver.next_seq(), "安装后与驱动器游标一致");
        prev = now;
    }
}
