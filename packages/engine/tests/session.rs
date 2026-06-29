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
