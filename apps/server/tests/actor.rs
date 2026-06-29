//! WS-5 actor-per-session 集成测试（ADR-0005 §5，无锁、GameSession 独占）。
//!
//! 直接驱动 SessionManager（不经 HTTP），验证 actor 行为：
//! - new_session 后 actor 启动并按 base_ms/speed 推 step；
//! - subscribe broadcast 能收到 Event（各带 seq）；
//! - Snapshot 命令返回完整快照；
//! - SetSpeed 调整 interval；
//! - intent 入队后被 actor 接受（Ok）；
//! - 未知 session_id 查询返回 None（不静默）。

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{NpcSetup, SessionSetup, StockSpec};
use engine::strategy::Intent;
use engine::Side;
use server::SessionManager;

/// 与 engine/tests/session.rs sample_setup 等价的最小合法 setup。
fn sample_setup() -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            initial_price: Money::from_cents(1000),
            limit_pct: 0.10,
            v_initial: Money::from_cents(1000),
            tick: Money::from_cents(1),
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 1,
            hot_count: 1,
            cash_per_npc: Money::from_cents(10_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        v_params: engine::VParams { long_run_mean: Money::from_cents(1000), mean_reversion: 0.5, volatility: 0.0 },
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams { arrival_rate: 0.5, order_size_mean: 100, chase_prob: 0.2, tick_cents: 1 },
            inst: engine::InstParams { margin: 0.05, order_size: 200 },
            hot: engine::HotParams { lookback: 3, trend_threshold: 0.02, order_size: 150 },
        },
        player_cash: Money::from_cents(10_000_000),
        ticks_per_day: 10,
        history_len: 5,
        t1_enabled: false,
        float_allocation: engine::FloatAllocation::Random,
    }
}

#[tokio::test]
async fn manager_new_session_returns_unique_ids_and_lookup_hits() {
    let mgr = SessionManager::default();
    let a = mgr.new_session(sample_setup(), 1).expect("应能创建 session");
    let b = mgr.new_session(sample_setup(), 2).expect("应能创建 session");
    assert_ne!(a, b, "两次创建应得到不同 session_id");
    assert!(mgr.lookup(&a).is_some(), "lookup 已存在 session 应命中");
    assert!(mgr.lookup(&b).is_some());
    assert!(mgr.lookup("nope").is_none(), "lookup 未知 session 应 None（不静默）");
}

#[tokio::test]
async fn actor_broadcasts_events_with_seq() {
    // 用很小的 base_ms 让 actor 快速跑出 step 事件。
    let mgr = SessionManager::with_base_ms(5);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 订阅事件流（必须在 step 前 subscribe，否则丢历史；连接先发快照对齐基线的设计见 ws 路由）。
    let mut rx = handles.event_tx.subscribe();

    // 等收到至少一个事件（PriceTick 每 step 一定出）。
    let mut got_price_tick = false;
    for _ in 0..200 {
        match tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await {
            Ok(Ok(ev)) => {
                let seq = event_seq(&ev);
                assert!(seq > 0, "事件必须带正 seq，got {seq}");
                if matches!(ev, engine::Event::PriceTick { .. }) {
                    got_price_tick = true;
                    break;
                }
            }
            Ok(Err(_)) => break, // lagged 或关闭
            Err(_) => continue,
        }
    }
    assert!(got_price_tick, "actor 应广播 PriceTick 事件");
}

#[tokio::test]
async fn actor_snapshot_command_returns_full_snapshot() {
    let mgr = SessionManager::with_base_ms(10_000); // 慢 interval，避免 step 干扰
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 真实路径：经 SessionHandles 提供的 snapshot helper（内部发 Snapshot 命令给 actor）。
    let real_snap = handles.snapshot().await.expect("snapshot 应返回 Ok");
    assert_eq!(real_snap.markets.len(), 1, "快照应含全部 markets");
    assert_eq!(real_snap.accounts.len(), 5, "快照应含全部 accounts（玩家+4NPC）");
}

#[tokio::test]
async fn actor_enqueue_intent_accepted_for_known_player() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 玩家 AccountId(0) 存在 → 入队 Ok（单玩家 v1，固定 player 0）。
    handles
        .enqueue(Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        })
        .await
        .expect("玩家意图应入队成功");
}

/// 与 engine/tests/session.rs `allocated_market_produces_trades` 等价的 setup：
/// float_shares>0 + ByKind 分配 → NPC 持仓可卖 → 卖盘有货 → 撮合出 Trade。
///
/// 这是任务「市场转活(有 Trade)」断言所需的 setup（默认 sample_setup 的 float_shares=0，
/// NPC 无仓只能挂买单、无对手盘 → 无成交；故这里单独构造一份带流通盘的 setup）。
fn active_market_setup() -> SessionSetup {
    let mut s = sample_setup();
    s.stocks[0].float_shares = 10_000_000;
    s.float_allocation = engine::FloatAllocation::ByKind { retail: 0.3, inst: 0.4, hot: 0.3 };
    s
}

/// 任务核心断言：new_session 后市场转活——actor 的 step 应广播出 Trade（成交）事件。
///
/// 直接驱动 SessionManager（不经 HTTP）：分配流通盘 → 订阅事件流 → 用很小的 base_ms
/// 让 actor 快速跑 step → 应在若干 tick 内收到至少一条 `Event::Trade`（maker/taker 双方结算）。
/// 这是 WS-5「市场转活」的最小契约：不是只出 PriceTick，而是真的撮合成交。
#[tokio::test]
async fn actor_market_goes_live_produces_trade_events() {
    let mgr = SessionManager::with_base_ms(5);
    let id = mgr.new_session(active_market_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    let mut rx = handles.event_tx.subscribe();

    // 收集事件，最多等 800 次 50ms 超时窗口（≈40s 上限，给慢机足够余量）。
    let mut got_trade = false;
    for _ in 0..800 {
        match tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await {
            Ok(Ok(ev)) => {
                if let engine::Event::Trade { seq, code, qty, maker, taker, .. } = &ev {
                    assert!(*seq > 0, "Trade 必须带正 seq");
                    assert!(*qty > 0, "Trade 成交量必须 >0");
                    assert_ne!(*maker, *taker, "Trade 的 maker/taker 必须是不同账户");
                    assert_eq!(code.0, "600101", "成交股票代码应匹配 setup");
                    got_trade = true;
                    break;
                }
            }
            Ok(Err(_)) => break, // lagged 或通道关闭
            Err(_) => continue,  // 单次超时，继续等下一个事件
        }
    }
    assert!(
        got_trade,
        "分配流通盘后，actor 应在若干 step 内广播出至少一条 Trade 事件（市场转活）"
    );
}

#[tokio::test]
async fn actor_set_speed_applied_without_error() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 提速到 10x：不应报错；SetSpeed 仅改 interval。
    handles.set_speed(10.0).await.expect("SetSpeed 应 Ok");
    // 再设回 1x。
    handles.set_speed(1.0).await.expect("SetSpeed 应 Ok");
}

/// 从 Event 提取 seq（镜像 engine/tests/session.rs 的 seq_of；Event 字段已 pub）。
fn event_seq(e: &engine::Event) -> u64 {
    match e {
        engine::Event::Trade { seq, .. }
        | engine::Event::PriceTick { seq, .. }
        | engine::Event::DayBoundary { seq, .. }
        | engine::Event::IntentRejected { seq, .. }
        | engine::Event::SettlementError { seq, .. }
        | engine::Event::VError { seq, .. } => *seq,
    }
}
