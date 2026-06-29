//! engine market 模块集成测试（TDD 红绿循环）。
use engine::account::StockCode;
use engine::market::{Market, MarketError, VParams};
use engine::Money;

#[test]
fn market_error_and_vparams_basics() {
    let e = MarketError::LimitExceeded {
        code: StockCode("600101".to_string()),
        price: Money::from_cents(1101),
        down: Money::from_cents(900),
        up: Money::from_cents(1100),
    };
    assert!(e.to_string().contains("600101"));
    let e2 = MarketError::InvalidVParams { reason: "bad".to_string() };
    assert!(e2.to_string().contains("bad"));

    let vp = VParams {
        long_run_mean: Money::from_cents(1000),
        mean_reversion: 0.5,
        volatility: 0.02,
    };
    assert_eq!(vp.long_run_mean.cents(), 1000);
}

fn mk_market() -> Market {
    // last_close=last_price=10.00，limit=0.10，V=10.00，tick=0.01
    Market::new(
        StockCode("600101".to_string()),
        Money::from_cents(1000),
        0.10,
        Money::from_cents(1000),
        Money::from_cents(1),
    )
    .unwrap()
}

#[test]
fn market_new_and_limit_stops() {
    let m = mk_market();
    assert_eq!(m.last_price().cents(), 1000);
    assert_eq!(m.last_close().cents(), 1000);
    assert_eq!(m.fundamental_value().cents(), 1000);
    assert_eq!(m.up_stop().cents(), 1100); // 1000 × 1.10
    assert_eq!(m.down_stop().cents(), 900); // 1000 × 0.90
}

#[test]
fn market_new_rejects_invalid() {
    // limit_pct ∉ (0,1)
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        1.5,
        Money::from_cents(1000),
        Money::from_cents(1)
    )
    .is_err());
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        0.0,
        Money::from_cents(1000),
        Money::from_cents(1)
    )
    .is_err());
    // initial_price ≤ 0
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::ZERO,
        0.10,
        Money::from_cents(1000),
        Money::from_cents(1)
    )
    .is_err());
    // v_initial ≤ 0
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        0.10,
        Money::ZERO,
        Money::from_cents(1)
    )
    .is_err());
}

use engine::orderbook::{AccountId, Order, OrderId, Side};
use engine::strategy::Rng;

// 固定种子 mock Rng：next_f64 恒返回固定值（用于驱动 evolve_v 的 z = next_f64*2-1）；
// next_range_u32 返回 lo（evolve_v 不用，仅满足 trait）。
struct FixedRng(f64);
impl Rng for FixedRng {
    fn next_f64(&mut self) -> f64 {
        self.0
    }
    fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
        lo
    }
}

fn buy(id: u64, price_cents: i64, qty: u32) -> Order {
    Order {
        id: OrderId(id),
        side: Side::Buy,
        price: Money::from_cents(price_cents),
        qty,
        owner: AccountId(1),
        seq: 0,
    }
}
fn sell(id: u64, price_cents: i64, qty: u32) -> Order {
    Order {
        id: OrderId(id),
        side: Side::Sell,
        price: Money::from_cents(price_cents),
        qty,
        owner: AccountId(2),
        seq: 0,
    }
}

#[test]
fn place_rejects_price_above_up_stop() {
    let mut m = mk_market(); // up_stop=1100
    let err = m.place(buy(1, 1101, 100)).unwrap_err(); // 11.01 > 11.00
    assert!(matches!(err, MarketError::LimitExceeded { .. }));
    assert!(m.best_bid().is_none()); // book 未被改动
}

#[test]
fn place_accepts_boundary_up_stop() {
    let mut m = mk_market(); // up_stop=1100
    let r = m.place(buy(1, 1100, 100)).unwrap(); // 边界价合法
    assert!(r.resting.is_some());
    assert_eq!(m.best_bid(), Some(Money::from_cents(1100)));
}

#[test]
fn place_rejects_price_below_down_stop() {
    let mut m = mk_market(); // down_stop=900
    let err = m.place(sell(1, 899, 100)).unwrap_err(); // 8.99 < 9.00
    assert!(matches!(err, MarketError::LimitExceeded { .. }));
    assert!(m.best_ask().is_none());
}

#[test]
fn place_updates_last_price_on_trade() {
    let mut m = mk_market(); // last_price=1000
    m.place(sell(1, 1000, 100)).unwrap();
    assert_eq!(m.last_price().cents(), 1000); // 无成交，last_price 不变
    m.place(buy(2, 1000, 100)).unwrap(); // 撮合成交价 1000
    assert_eq!(m.last_price().cents(), 1000); // 末笔 trade 价 1000
}

#[test]
fn place_match_result_matches_book() {
    let mut m = mk_market();
    m.place(sell(1, 1000, 100)).unwrap();
    let r = m.place(buy(2, 1000, 100)).unwrap();
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].price.cents(), 1000);
    assert_eq!(r.trades[0].qty, 100);
}

#[test]
fn evolve_v_mean_reverts_toward_long_run_mean() {
    let mut m = mk_market(); // V=1000, last_close=1000
    // long_run_mean=800, α=0.5, σ=0, z=0 → drift=0.5*((800-1000)/1000)=-0.1
    // V_new = 1000 * 0.9 = 900
    let vp = VParams {
        long_run_mean: Money::from_cents(800),
        mean_reversion: 0.5,
        volatility: 0.0,
    };
    m.evolve_v(&vp, &mut FixedRng(0.5)).unwrap();
    assert_eq!(m.fundamental_value().cents(), 900); // 向 800 回复
}

#[test]
fn evolve_v_with_volatility_changes() {
    let mut m = mk_market(); // V=1000
    // σ=0.1, z=next_f64*2-1。FixedRng(1.0) → z=1.0 → drift=0+0.1*1=0.1 → V_new=1100
    let vp = VParams {
        long_run_mean: Money::from_cents(1000),
        mean_reversion: 0.0,
        volatility: 0.1,
    };
    m.evolve_v(&vp, &mut FixedRng(1.0)).unwrap();
    assert_eq!(m.fundamental_value().cents(), 1100);
}

#[test]
fn evolve_v_rejects_invalid_params() {
    let mut m = mk_market();
    // volatility < 0
    let vp = VParams {
        long_run_mean: Money::from_cents(1000),
        mean_reversion: 0.0,
        volatility: -0.1,
    };
    assert!(m.evolve_v(&vp, &mut FixedRng(0.5)).is_err());
    // mean_reversion < 0
    let vp2 = VParams {
        long_run_mean: Money::from_cents(1000),
        mean_reversion: -0.1,
        volatility: 0.0,
    };
    assert!(m.evolve_v(&vp2, &mut FixedRng(0.5)).is_err());
    // long_run_mean ≤ 0
    let vp3 = VParams {
        long_run_mean: Money::ZERO,
        mean_reversion: 0.0,
        volatility: 0.0,
    };
    assert!(m.evolve_v(&vp3, &mut FixedRng(0.5)).is_err());
}

#[test]
fn evolve_v_rejects_multiplier_le_zero() {
    let mut m = mk_market(); // V=1000
    // long_run_mean=1(分), α 极大 → gap=(1-1000)/1000≈-0.999, drift=10*-0.999=-9.99 → multiplier<0
    let vp = VParams {
        long_run_mean: Money::from_cents(1),
        mean_reversion: 10.0,
        volatility: 0.0,
    };
    assert!(m.evolve_v(&vp, &mut FixedRng(0.5)).is_err());
    assert_eq!(m.fundamental_value().cents(), 1000); // V 不变
}
