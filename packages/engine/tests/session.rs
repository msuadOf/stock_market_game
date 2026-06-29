//! engine session 模块集成测试（TDD 红绿循环）。
use engine::session::SplitMix64;
use engine::strategy::Rng;

#[test]
fn splitmix64_is_deterministic() {
    let mut a = SplitMix64::new(42);
    let mut b = SplitMix64::new(42);
    for _ in 0..10 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
    assert_ne!(SplitMix64::new(7).next_u64(), SplitMix64::new(42).next_u64());
}

#[test]
fn splitmix64_next_f64_in_unit_range() {
    let mut r = SplitMix64::new(123);
    for _ in 0..100 {
        let x = r.next_f64();
        assert!((0.0..1.0).contains(&x), "next_f64 out of [0,1): {x}");
    }
}

#[test]
fn splitmix64_next_range_u32_in_range() {
    let mut r = SplitMix64::new(999);
    for _ in 0..100 {
        let v = r.next_range_u32(10, 20);
        assert!((10..20).contains(&v), "out of [10,20): {v}");
    }
    assert_eq!(r.next_range_u32(20, 20), 20); // lo>=hi → lo
}

use engine::session::{Event, NpcSetup, RejectionReason, SessionSetup, Snapshot, StockSpec};
use engine::account::StockCode;
use engine::money::Money;
use engine::orderbook::AccountId;

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
        v_params: engine::VParams {
            long_run_mean: Money::from_cents(1000),
            mean_reversion: 0.5,
            volatility: 0.0,
        },
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.5,
                order_size_mean: 100,
                chase_prob: 0.2,
                tick_cents: 1,
            },
            inst: engine::InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: engine::HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 150,
            },
        },
        player_cash: Money::from_cents(10_000_000),
        ticks_per_day: 10,
        history_len: 5,
        t1_enabled: false,
        float_allocation: engine::FloatAllocation::Random,
    }
}

#[test]
fn event_and_setup_construct() {
    let e = Event::IntentRejected {
        seq: 5,
        account: engine::AccountId(1),
        code: StockCode("600101".to_string()),
        reason: RejectionReason::InsufficientCash,
    };
    assert!(matches!(
        e,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    ));
    let snap = Snapshot {
        seq: 0,
        tick: 0,
        day: 0,
        markets: Default::default(),
        accounts: Default::default(),
    };
    assert_eq!(snap.seq, 0);
    assert_eq!(sample_setup().stocks.len(), 1);
}

use engine::session::GameSession;

#[test]
fn session_new_constructs_markets_and_accounts() {
    let s = GameSession::new(sample_setup(), 42).unwrap();
    assert_eq!(s.market_count(), 1);
    assert_eq!(s.account_count(), 5); // 玩家1 + retail2 + inst1 + hot1
    assert!(!s.account(AccountId(0)).unwrap().has_strategy()); // 玩家 None
    assert!(s.account(AccountId(1)).unwrap().has_strategy()); // NPC 有
}

#[test]
fn session_new_rejects_empty_stocks() {
    let mut setup = sample_setup();
    setup.stocks.clear();
    assert!(GameSession::new(setup, 42).is_err());
}

#[test]
fn snapshot_contains_all_markets_and_accounts() {
    let s = GameSession::new(sample_setup(), 42).unwrap();
    let snap = s.snapshot();
    assert_eq!(snap.seq, 0);
    assert_eq!(snap.markets.len(), 1);
    let ms = snap.markets.get(&StockCode("600101".to_string())).unwrap();
    assert_eq!(ms.last_price.cents(), 1000);
    assert_eq!(ms.fundamental_value.cents(), 1000);
    assert_eq!(snap.accounts.len(), 5);
    assert_eq!(snap.accounts.get(&AccountId(0)).unwrap().cash.cents(), 10_000_000);
}

// Task 5: step() 核心循环测试（决策/路由/结算/V/日界/事件）。

fn seq_of(e: &Event) -> u64 {
    match e {
        Event::Trade { seq, .. } | Event::PriceTick { seq, .. } | Event::DayBoundary { seq, .. }
        | Event::IntentRejected { seq, .. } | Event::SettlementError { seq, .. } | Event::VError { seq, .. } => *seq,
    }
}
fn events_summary(ev: &[Event]) -> Vec<String> {
    ev.iter().map(|e| match e {
        Event::Trade { code, price, qty, .. } => format!("T{}:{}:{}", code.0, price.cents(), qty),
        Event::PriceTick { code, last_price, .. } => format!("P{}:{}", code.0, last_price.cents()),
        Event::DayBoundary { day, .. } => format!("D{}", day),
        Event::IntentRejected { reason, .. } => format!("R{:?}", reason),
        Event::SettlementError { reason, .. } => format!("S{}", reason),
        Event::VError { reason, .. } => format!("V{}", reason),
    }).collect()
}

#[test]
fn step_produces_events_with_monotonic_seq() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let events = s.step();
    assert_eq!(s.tick(), 1);
    assert!(events.iter().any(|e| matches!(e, Event::PriceTick { .. })));
    let mut last = 0u64;
    for e in &events {
        let sq = seq_of(e);
        assert!(sq >= last);
        last = last.max(sq);
    }
}

#[test]
fn step_is_deterministic_same_seed() {
    let mut a = GameSession::new(sample_setup(), 42).unwrap();
    let mut b = GameSession::new(sample_setup(), 42).unwrap();
    for _ in 0..5 {
        assert_eq!(events_summary(&a.step()), events_summary(&b.step()));
    }
}

#[test]
fn step_npc_routes_undervalued_buy_intent() {
    // 机构见 last<V 必下买单（ValueStrategy 无 RNG 依赖，确定性）。
    // 说明：在当前撮合/账户语义下，无人持初始仓位 → 卖盘恒空 → 首笔买单只能挂入簿
    // （无对手盘不能成交，见 orderbook "无对手盘时买单应直接挂入簿：无成交"）。
    // 故此处断言「机构低估意图被路由并挂入买盘」(best_bid 出现)，验证 决策→路由→orderbook 链路。
    // 真正的 Trade 需做市商初始仓位/卖盘注入（超出本任务范围，见 honestReport）。
    let mut setup = sample_setup();
    setup.stocks[0].initial_price = Money::from_cents(900);
    setup.stocks[0].v_initial = Money::from_cents(1000);
    setup.npcs = NpcSetup { retail_count: 0, inst_count: 1, hot_count: 0, cash_per_npc: Money::from_cents(10_000_000) };
    let mut s = GameSession::new(setup, 42).unwrap();
    s.step();
    assert_eq!(
        s.snapshot().markets.get(&StockCode("600101".to_string())).unwrap().best_bid,
        Some(Money::from_cents(900)),
        "机构低估买单应挂入买盘（best_bid=900）"
    );
}

#[test]
fn step_evolves_v() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let v0 = s.snapshot().markets.get(&StockCode("600101".to_string())).unwrap().fundamental_value.cents();
    s.step();
    let v1 = s.snapshot().markets.get(&StockCode("600101".to_string())).unwrap().fundamental_value.cents();
    assert_eq!(v0, v1); // V=mean=1000,volatility=0 → 不变
}

#[test]
fn step_day_boundary() {
    let mut setup = sample_setup();
    setup.ticks_per_day = 2;
    let mut s = GameSession::new(setup, 42).unwrap();
    s.step();
    let events = s.step();
    assert!(events.iter().any(|e| matches!(e, Event::DayBoundary { day: 1, .. })));
    assert_eq!(s.day(), 1);
}

// Task 6: enqueue_player_intent + 玩家意图执行（随时入队/step 执行）。
use engine::strategy::Intent;
use engine::Side;

#[test]
fn enqueue_player_intent_executes_in_step() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    s.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    s.step();
    // 玩家挂买单 → best_bid 出现
    assert!(s
        .snapshot()
        .markets
        .get(&StockCode("600101".to_string()))
        .unwrap()
        .best_bid
        .is_some());
}

#[test]
fn enqueue_unknown_player_errors() {
    let mut s = GameSession::new(sample_setup(), 42).unwrap();
    let r = s.enqueue_player_intent(
        AccountId(999),
        Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    );
    assert!(r.is_err());
}

#[test]
fn step_rejects_insufficient_cash_player_intent() {
    let mut setup = sample_setup();
    setup.player_cash = Money::from_cents(500);
    let mut s = GameSession::new(setup, 42).unwrap();
    s.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    let events = s.step();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::IntentRejected { account, reason, .. }
        if *account == AccountId(0) && *reason == RejectionReason::InsufficientCash
    )));
    // 现金不足以买入 → 余额未变（仍为 500）
    assert_eq!(s.account(AccountId(0)).unwrap().cash.cents(), 500);
}

// Task 7: crate 根 re-export（engine::{GameSession,SessionSetup,SplitMix64,Event,Snapshot,SessionError}）。
#[test]
fn reexport_from_crate_root() {
    use engine::{Event, GameSession, SessionError, SessionSetup, Snapshot, SplitMix64};
    let _ = GameSession::new(sample_setup(), 42).unwrap();
    let _ = SplitMix64::new(1);
    // 确保所有 re-export 符号可命名（编译期校验）。
    let _: Option<SessionError> = None;
    let _: Option<Event> = None;
    let _: Option<SessionSetup> = None;
    let _: Option<Snapshot> = None;
}
